//! Runs the `RedCrown` desktop backend process.
// Rust guideline compliant 2026-02-21

use std::backtrace::Backtrace;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use directories::ProjectDirs;
use redcrown_catalog::{ButterCatalog, CatalogProvider, EndpointChain};
use redcrown_core::{
    AppSettings, BootstrapState, CatalogQuery, IpcRequest, IpcResponse, MediaItem, MediaKind,
    SourceConfig,
};
use redcrown_diagnostics::Diagnostics;
use redcrown_library::{
    Library, LibraryImportReport, PopcornImportSelection, PopcornProfilePreview,
    discover_popcorn_profiles,
};
use redcrown_torrent::{MediaTools, TorrentEngine};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::fs::{self, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinSet;
use tracing::{Instrument, Level, event};
use url::Url;
use uuid::Uuid;

const PROTOCOL_VERSION: u32 = 1;
const MAX_IPC_LINE_BYTES: usize = 1024 * 1024;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[derive(Debug)]
struct AppState {
    settings: SettingsStore,
    library: Arc<Library>,
    torrent: Option<Arc<TorrentEngine>>,
}

#[derive(Debug)]
struct SettingsStore {
    root: PathBuf,
    current: RwLock<SettingsSnapshot>,
}

#[derive(Debug, Clone)]
struct SettingsSnapshot {
    generation: u64,
    settings: AppSettings,
}

#[derive(Debug, Serialize, Deserialize)]
struct SettingsEnvelope {
    generation: u64,
    settings: AppSettings,
}

impl SettingsStore {
    async fn open(root: PathBuf) -> Result<Self, BackendError> {
        fs::create_dir_all(&root).await.map_err(|error| {
            BackendError::new(format!("failed to create settings directory: {error}"))
        })?;
        let current = load_latest_settings(&root)
            .await
            .unwrap_or_else(|| SettingsSnapshot {
                generation: 0,
                settings: AppSettings::initial(),
            });
        Ok(Self {
            root,
            current: RwLock::new(current),
        })
    }

    async fn get(&self) -> AppSettings {
        self.current.read().await.settings.clone()
    }

    async fn save(&self, settings: AppSettings) -> Result<AppSettings, BackendError> {
        settings
            .validate()
            .map_err(|error| BackendError::new(error.user_message()))?;
        let mut current = self.current.write().await;
        let generation = current
            .generation
            .checked_add(1)
            .ok_or_else(|| BackendError::new("settings generation exhausted"))?;
        let envelope = SettingsEnvelope {
            generation,
            settings: settings.clone(),
        };
        let serialized = serde_json::to_vec_pretty(&envelope)
            .map_err(|error| BackendError::new(format!("failed to encode settings: {error}")))?;
        let slot = settings_slot(&self.root, generation);
        let temporary = self
            .root
            .join(format!("settings.{}.tmp", Uuid::new_v4().simple()));

        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .await
            .map_err(|error| BackendError::new(format!("failed to stage settings: {error}")))?;
        file.write_all(&serialized)
            .await
            .map_err(|error| BackendError::new(format!("failed to write settings: {error}")))?;
        file.flush()
            .await
            .map_err(|error| BackendError::new(format!("failed to flush settings: {error}")))?;
        file.sync_all()
            .await
            .map_err(|error| BackendError::new(format!("failed to sync settings: {error}")))?;
        drop(file);

        if let Err(error) = fs::remove_file(&slot).await
            && error.kind() != std::io::ErrorKind::NotFound
        {
            let _ = fs::remove_file(&temporary).await;
            return Err(BackendError::new(format!(
                "failed to rotate settings journal: {error}"
            )));
        }
        fs::rename(&temporary, &slot)
            .await
            .map_err(|error| BackendError::new(format!("failed to commit settings: {error}")))?;
        *current = SettingsSnapshot {
            generation,
            settings: settings.clone(),
        };
        Ok(settings)
    }
}

async fn load_latest_settings(root: &Path) -> Option<SettingsSnapshot> {
    let mut snapshots = Vec::with_capacity(2);
    for slot in [root.join("settings.a.json"), root.join("settings.b.json")] {
        let Ok(bytes) = fs::read(slot).await else {
            continue;
        };
        let Ok(envelope) = serde_json::from_slice::<SettingsEnvelope>(&bytes) else {
            continue;
        };
        if envelope.settings.validate().is_ok() {
            snapshots.push(SettingsSnapshot {
                generation: envelope.generation,
                settings: envelope.settings,
            });
        }
    }
    snapshots
        .into_iter()
        .max_by_key(|snapshot| snapshot.generation)
}

fn settings_slot(root: &Path, generation: u64) -> PathBuf {
    root.join(if generation.is_multiple_of(2) {
        "settings.a.json"
    } else {
        "settings.b.json"
    })
}

#[derive(Debug, Deserialize)]
struct SaveSettingsParams {
    settings: AppSettings,
}

#[derive(Debug, Deserialize)]
struct TestSourceParams {
    source: SourceConfig,
}

#[derive(Debug, Deserialize)]
struct BrowseParams {
    #[serde(flatten)]
    query: CatalogQuery,
}

#[derive(Debug, Deserialize)]
struct CatalogEpisodesParams {
    media_id: String,
}

#[derive(Debug, Deserialize)]
struct StartPlaybackParams {
    source: String,
    file_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PlaybackPreparationParams {
    preparation_id: Uuid,
}

#[derive(Debug, Deserialize)]
struct StopPlaybackParams {
    torrent_id: usize,
}

#[derive(Debug, Deserialize)]
struct ImportPopcornParams {
    profile_id: String,
    selection: PopcornImportSelection,
}

#[derive(Debug, Serialize)]
struct PopcornImportReport {
    api_urls_added: usize,
    settings: AppSettings,
    library: LibraryImportReport,
}

fn main() -> anyhow::Result<()> {
    let mut diagnostics = Diagnostics::initialize().context("diagnostics initialization failed")?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("Tokio runtime initialization failed")?;
    let result = runtime.block_on(run_application());
    drop(runtime);
    diagnostics.shutdown();
    result
}

async fn run_application() -> anyhow::Result<()> {
    let directories = ProjectDirs::from("app", "RedCrown", "RedCrown")
        .context("Windows application directories are unavailable")?;
    let settings = SettingsStore::open(directories.data_local_dir().to_path_buf())
        .await
        .context("settings initialization failed")?;
    let library = Arc::new(
        Library::open(directories.data_local_dir().join("library.sqlite3"))
            .context("library initialization failed")?,
    );
    let initial_settings = settings.get().await;
    let torrent_result = match MediaTools::from_environment() {
        Ok(media_tools) => {
            TorrentEngine::start(
                directories.cache_dir().join("streams"),
                initial_settings.stream_cache,
                media_tools,
            )
            .await
        }
        Err(error) => Err(error),
    };
    let torrent = match torrent_result {
        Ok(engine) => Some(Arc::new(engine)),
        Err(error) => {
            event!(
                name: "torrent.engine.start.failed",
                Level::ERROR,
                error.message = error.user_message(),
                "torrent engine unavailable"
            );
            None
        }
    };
    run_ipc(Arc::new(AppState {
        settings,
        library,
        torrent,
    }))
    .await
}

async fn run_ipc(state: Arc<AppState>) -> anyhow::Result<()> {
    let mut reader = BufReader::new(tokio::io::stdin());
    let stdout = Arc::new(Mutex::new(tokio::io::stdout()));
    let mut tasks = JoinSet::new();
    loop {
        let mut line = String::new();
        let bytes = reader.read_line(&mut line).await?;
        if bytes == 0 {
            break;
        }
        if bytes > MAX_IPC_LINE_BYTES {
            event!(
                name: "ipc.request.rejected",
                Level::WARN,
                request.bytes = bytes,
                "IPC request exceeded the size limit"
            );
            continue;
        }
        let request = match serde_json::from_str::<IpcRequest>(line.trim_end()) {
            Ok(request) => request,
            Err(error) => {
                event!(
                    name: "ipc.request.invalid",
                    Level::WARN,
                    error.message = %error,
                    "invalid IPC request"
                );
                continue;
            }
        };
        let state = Arc::clone(&state);
        let stdout = Arc::clone(&stdout);
        tasks.spawn(async move {
            let response = dispatch(&state, request).await;
            if let Err(error) = write_response(&stdout, &response).await {
                event!(
                    name: "ipc.response.failed",
                    Level::ERROR,
                    error.message = %error,
                    "failed to write IPC response"
                );
            }
        });
        while tasks.try_join_next().is_some() {}
    }
    while tasks.join_next().await.is_some() {}
    if let Some(torrent) = &state.torrent
        && let Err(error) = torrent.shutdown().await
    {
        event!(
            name: "torrent.engine.shutdown.failed",
            Level::WARN,
            error.message = error.user_message(),
            "torrent engine shutdown was incomplete"
        );
    }
    Ok(())
}

async fn dispatch(state: &AppState, request: IpcRequest) -> IpcResponse<Value> {
    let span = tracing::info_span!(
        target: "redcrown_telemetry",
        "desktop.ipc.command",
        rpc.system = "redcrown.ipc",
        rpc.method = telemetry_method(&request.method),
        redcrown.protocol_version = PROTOCOL_VERSION,
        otel.status_code = tracing::field::Empty,
        error.type = tracing::field::Empty
    );
    let result = dispatch_method(state, &request.method, request.params)
        .instrument(span.clone())
        .await;
    match result {
        Ok(value) => {
            span.record("otel.status_code", "OK");
            IpcResponse::success(request.id, value)
        }
        Err(error) => {
            span.record("otel.status_code", "ERROR");
            span.record("error.type", error.code());
            event!(
                name: "ipc.command.failed",
                Level::WARN,
                command.name = request.method,
                error.message = error.user_message(),
                "backend command failed"
            );
            IpcResponse::failure(request.id, error.code(), error.user_message())
        }
    }
}

fn telemetry_method(method: &str) -> &'static str {
    match method {
        "health" => "health",
        "bootstrap" => "bootstrap",
        "settings.save" => "settings.save",
        "source.test" => "source.test",
        "catalog.browse" => "catalog.browse",
        "catalog.episodes" => "catalog.episodes",
        "playback.prepare" => "playback.prepare",
        "playback.status" => "playback.status",
        "playback.diagnostics" => "playback.diagnostics",
        "playback.cancel" => "playback.cancel",
        "playback.stop" => "playback.stop",
        "library.summary" => "library.summary",
        "migration.popcorn.discover" => "migration.popcorn.discover",
        "migration.popcorn.import" => "migration.popcorn.import",
        _ => "unknown",
    }
}

async fn dispatch_method(
    state: &AppState,
    method: &str,
    params: Value,
) -> Result<Value, BackendError> {
    match method {
        "health" => Ok(json!({ "protocolVersion": PROTOCOL_VERSION })),
        "bootstrap" => serde_json::to_value(BootstrapState {
            protocol_version: PROTOCOL_VERSION,
            settings: state.settings.get().await,
            featured: fixture_catalog(),
            torrent_engine_ready: state.torrent.is_some(),
        })
        .map_err(|error| BackendError::serialization(&error)),
        "settings.save" => {
            let params: SaveSettingsParams = decode_params(params)?;
            let saved = state.settings.save(params.settings).await?;
            if let Some(torrent) = &state.torrent {
                torrent
                    .update_cache_policy(saved.stream_cache)
                    .await
                    .map_err(|error| BackendError::new(error.user_message()))?;
            }
            serde_json::to_value(saved).map_err(|error| BackendError::serialization(&error))
        }
        "source.test" => {
            let params: TestSourceParams = decode_params(params)?;
            let chain = EndpointChain::new(params.source)
                .map_err(|error| BackendError::new(error.user_message()))?;
            serde_json::to_value(chain.test_all().await)
                .map_err(|error| BackendError::serialization(&error))
        }
        "catalog.browse" => {
            let params: BrowseParams = decode_params(params)?;
            let provider = active_catalog(state).await?;
            let items = provider
                .browse(&params.query)
                .await
                .map_err(|error| BackendError::new(error.user_message()))?;
            serde_json::to_value(items).map_err(|error| BackendError::serialization(&error))
        }
        "catalog.episodes" => {
            let params: CatalogEpisodesParams = decode_params(params)?;
            let provider = active_catalog(state).await?;
            let episodes = provider
                .episodes(&params.media_id)
                .await
                .map_err(|error| BackendError::new(error.user_message()))?;
            serde_json::to_value(episodes).map_err(|error| BackendError::serialization(&error))
        }
        method if method.starts_with("playback.") => dispatch_playback(state, method, params).await,
        "library.summary" => library_summary(state).await,
        "migration.popcorn.discover" => discover_popcorn().await,
        "migration.popcorn.import" => {
            let params: ImportPopcornParams = decode_params(params)?;
            import_popcorn(state, params).await
        }
        _ => Err(BackendError::with_code(
            "unknown_method",
            format!("Unknown backend method: {method}"),
        )),
    }
}

async fn dispatch_playback(
    state: &AppState,
    method: &str,
    params: Value,
) -> Result<Value, BackendError> {
    let torrent = state
        .torrent
        .as_ref()
        .ok_or_else(|| BackendError::new("Torrent engine is unavailable"))?;
    match method {
        "playback.prepare" => {
            let params: StartPlaybackParams = decode_params(params)?;
            serde_json::to_value(
                torrent
                    .prepare_playback(params.source, params.file_path)
                    .await,
            )
            .map_err(|error| BackendError::serialization(&error))
        }
        "playback.status" => {
            let params: PlaybackPreparationParams = decode_params(params)?;
            serde_json::to_value(
                torrent
                    .playback_status(params.preparation_id)
                    .await
                    .map_err(|error| BackendError::new(error.user_message()))?,
            )
            .map_err(|error| BackendError::serialization(&error))
        }
        "playback.diagnostics" => {
            let params: PlaybackPreparationParams = decode_params(params)?;
            serde_json::to_value(
                torrent
                    .diagnostics(params.preparation_id)
                    .await
                    .map_err(|error| BackendError::new(error.user_message()))?,
            )
            .map_err(|error| BackendError::serialization(&error))
        }
        "playback.cancel" => {
            let params: PlaybackPreparationParams = decode_params(params)?;
            torrent
                .cancel_preparation(params.preparation_id)
                .await
                .map_err(|error| BackendError::new(error.user_message()))?;
            Ok(Value::Null)
        }
        "playback.stop" => {
            let params: StopPlaybackParams = decode_params(params)?;
            torrent
                .stop_playback(params.torrent_id)
                .await
                .map_err(|error| BackendError::new(error.user_message()))?;
            Ok(Value::Null)
        }
        _ => Err(BackendError::with_code(
            "unknown_method",
            format!("Unknown backend method: {method}"),
        )),
    }
}

async fn active_catalog(state: &AppState) -> Result<ButterCatalog, BackendError> {
    let source = state
        .settings
        .get()
        .await
        .sources
        .into_iter()
        .find(|source| source.enabled && source.endpoints.iter().any(|endpoint| endpoint.enabled))
        .ok_or_else(|| {
            BackendError::new("Configure at least one enabled catalog API URL in Settings")
        })?;
    ButterCatalog::new(source).map_err(|error| BackendError::new(error.user_message()))
}

async fn library_summary(state: &AppState) -> Result<Value, BackendError> {
    let library = Arc::clone(&state.library);
    let summary = tokio::task::spawn_blocking(move || library.summary())
        .await
        .map_err(|error| BackendError::new(format!("library task failed: {error}")))?
        .map_err(|error| BackendError::new(error.user_message()))?;
    serde_json::to_value(summary).map_err(|error| BackendError::serialization(&error))
}

async fn discover_popcorn() -> Result<Value, BackendError> {
    let previews = tokio::task::spawn_blocking(|| {
        discover_popcorn_profiles().map(|profiles| {
            profiles
                .into_iter()
                .map(|profile| profile.preview().clone())
                .collect::<Vec<PopcornProfilePreview>>()
        })
    })
    .await
    .map_err(|error| BackendError::new(format!("profile discovery task failed: {error}")))?
    .map_err(|error| BackendError::new(error.user_message()))?;
    serde_json::to_value(previews).map_err(|error| BackendError::serialization(&error))
}

async fn import_popcorn(
    state: &AppState,
    params: ImportPopcornParams,
) -> Result<Value, BackendError> {
    let profile_id = params.profile_id.clone();
    let data = tokio::task::spawn_blocking(move || {
        discover_popcorn_profiles()?
            .into_iter()
            .find(|profile| profile.preview().id == profile_id)
            .map(|profile| profile.data().clone())
            .ok_or_else(redcrown_library::LibraryError::profile_unavailable)
    })
    .await
    .map_err(|error| BackendError::new(format!("profile import task failed: {error}")))?
    .map_err(|error| BackendError::new(error.user_message()))?;

    let original_settings = state.settings.get().await;
    let mut imported_settings = original_settings.clone();
    let api_urls_added = if params.selection.api_urls {
        merge_imported_endpoints(&mut imported_settings, &data.api_urls)?
    } else {
        0
    };
    if imported_settings != original_settings {
        state.settings.save(imported_settings.clone()).await?;
    }

    let library = Arc::clone(&state.library);
    let selection = params.selection;
    let library_result =
        tokio::task::spawn_blocking(move || library.import_popcorn(&data, selection))
            .await
            .map_err(|error| BackendError::new(format!("library import task failed: {error}")))?;
    let library = match library_result {
        Ok(report) => report,
        Err(error) => {
            if imported_settings != original_settings {
                state
                    .settings
                    .save(original_settings)
                    .await
                    .map_err(|rollback| {
                        BackendError::new(format!(
                            "library import failed and settings rollback also failed: {}; {}",
                            error.user_message(),
                            rollback.user_message()
                        ))
                    })?;
            }
            return Err(BackendError::new(error.user_message()));
        }
    };
    serde_json::to_value(PopcornImportReport {
        api_urls_added,
        settings: imported_settings,
        library,
    })
    .map_err(|error| BackendError::serialization(&error))
}

fn merge_imported_endpoints(
    settings: &mut AppSettings,
    imported: &[String],
) -> Result<usize, BackendError> {
    let source = settings.sources.first_mut().ok_or_else(|| {
        BackendError::new("RedCrown has no catalog source to receive imported endpoints")
    })?;
    let mut known: std::collections::HashSet<String> = source
        .endpoints
        .iter()
        .map(|endpoint| endpoint.url.to_string())
        .collect();
    let mut added = 0;
    for value in imported {
        let endpoint = redcrown_core::SourceEndpoint::parse(value)
            .map_err(|error| BackendError::new(error.user_message()))?;
        if known.insert(endpoint.url.to_string()) {
            source.endpoints.push(endpoint);
            added += 1;
        }
    }
    if added > 0 && source.name == "Primary catalog" {
        "Popcorn-compatible catalog".clone_into(&mut source.name);
    }
    Ok(added)
}

fn decode_params<T: for<'de> Deserialize<'de>>(params: Value) -> Result<T, BackendError> {
    serde_json::from_value(params)
        .map_err(|error| BackendError::with_code("invalid_params", error.to_string()))
}

async fn write_response(
    stdout: &Mutex<tokio::io::Stdout>,
    response: &IpcResponse<Value>,
) -> anyhow::Result<()> {
    let mut serialized = serde_json::to_vec(response)?;
    serialized.push(b'\n');
    let mut stdout = stdout.lock().await;
    stdout.write_all(&serialized).await?;
    stdout.flush().await?;
    Ok(())
}

fn fixture_catalog() -> Vec<MediaItem> {
    [
        (
            "big-buck-bunny",
            "Big Buck Bunny",
            2008,
            "A gentle giant of a rabbit turns the tables on three woodland bullies.",
            "https://upload.wikimedia.org/wikipedia/commons/thumb/c/c5/Big_buck_bunny_poster_big.jpg/512px-Big_buck_bunny_poster_big.jpg",
        ),
        (
            "sintel",
            "Sintel",
            2010,
            "A young traveler crosses a wintry world while searching for a lost dragon.",
            "https://upload.wikimedia.org/wikipedia/commons/thumb/8/8f/Sintel_poster.jpg/512px-Sintel_poster.jpg",
        ),
        (
            "tears-of-steel",
            "Tears of Steel",
            2012,
            "A group of warriors and scientists gather in Amsterdam to save the future.",
            "https://upload.wikimedia.org/wikipedia/commons/thumb/9/90/Tears_of_Steel_poster.jpg/512px-Tears_of_Steel_poster.jpg",
        ),
    ]
    .into_iter()
    .map(|(id, title, year, synopsis, poster)| MediaItem {
        id: id.to_owned(),
        title: title.to_owned(),
        year: Some(year),
        synopsis: synopsis.to_owned(),
        poster_url: Url::parse(poster).ok(),
        backdrop_url: None,
        rating: None,
        kind: MediaKind::Movie,
        genres: Vec::new(),
        torrents: Vec::new(),
    })
    .collect()
}

/// Reports a bounded backend command failure.
#[derive(Debug)]
struct BackendError {
    code: &'static str,
    message: String,
    backtrace: Backtrace,
}

impl BackendError {
    fn new(message: impl Into<String>) -> Self {
        Self::with_code("backend_error", message)
    }

    fn with_code(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            backtrace: Backtrace::capture(),
        }
    }

    fn serialization(error: &serde_json::Error) -> Self {
        Self::new(format!("failed to encode backend response: {error}"))
    }

    const fn code(&self) -> &'static str {
        self.code
    }

    fn user_message(&self) -> &str {
        &self.message
    }
}

impl Display for BackendError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}\n{}", self.message, self.backtrace)
    }
}

impl std::error::Error for BackendError {}

#[cfg(test)]
mod tests {
    use redcrown_core::{AppSettings, SourceEndpoint};
    use tempfile::tempdir;

    use super::SettingsStore;

    #[tokio::test]
    async fn settings_journal_recovers_latest_valid_generation() {
        let directory = tempdir().expect("directory");
        let store = SettingsStore::open(directory.path().to_path_buf())
            .await
            .expect("store");
        let mut settings = AppSettings::initial();
        settings.sources[0]
            .endpoints
            .push(SourceEndpoint::parse("https://example.com").expect("endpoint"));
        store.save(settings.clone()).await.expect("first save");
        settings.sources[0].name = "Updated catalog".to_owned();
        store.save(settings.clone()).await.expect("second save");

        let reopened = SettingsStore::open(directory.path().to_path_buf())
            .await
            .expect("reopen");
        assert_eq!(reopened.get().await, settings);
    }
}
