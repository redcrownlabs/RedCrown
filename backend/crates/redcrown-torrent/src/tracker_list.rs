//! Imports bounded supplemental tracker lists for trackerless magnets.
// Rust guideline compliant 2026-02-21

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use redcrown_core::{TrackerListConfig, TrackerListSource};
use serde::{Deserialize, Serialize};
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{Mutex, RwLock};
use tracing::{Level, event};
use url::Url;

use super::TorrentError;

/// External lists are small; this cap prevents unbounded remote or file input.
const MAX_TRACKER_LIST_BYTES: usize = 1024 * 1024;
/// A malformed or hostile list cannot create an unbounded announce fan-out.
const MAX_TRACKERS: usize = 512;
// Tracker-list refreshes must preserve the last valid cache even when Windows
// refuses to rename over an existing file or a write is interrupted. Alternating
// generation-numbered slots provides that invariant without a platform-specific
// replacement primitive. The accepted tradeoff is one additional small cache
// file and selecting the newest valid generation during startup.
const CACHE_SCHEMA_VERSION: u32 = 2;
const CACHE_SLOT_A: &str = ".redcrown-trackers.a.json";
const CACHE_SLOT_B: &str = ".redcrown-trackers.b.json";

#[derive(Debug, Clone)]
struct TrackerListState {
    config: TrackerListConfig,
    trackers: Arc<[String]>,
    cache_generation: u64,
}

/// Holds a validated tracker-list update ready for an atomic settings commit.
#[derive(Debug)]
pub struct PreparedTrackerList {
    config: TrackerListConfig,
    trackers: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TrackerListCache {
    schema_version: u32,
    generation: u64,
    source: TrackerListSource,
    trackers: Vec<String>,
}

/// Owns validated supplemental trackers and their durable cache.
#[derive(Debug)]
pub(super) struct TrackerList {
    client: reqwest::Client,
    cache_root: PathBuf,
    state: RwLock<TrackerListState>,
    refresh_lock: Mutex<()>,
}

impl TrackerList {
    /// Opens the tracker importer and refreshes its configured source.
    ///
    /// A matching on-disk cache remains available when startup refresh fails.
    ///
    /// # Errors
    ///
    /// Returns an error when the HTTP client cannot be constructed.
    pub(super) async fn open(
        cache_root: PathBuf,
        config: TrackerListConfig,
    ) -> Result<Arc<Self>, TorrentError> {
        let client = reqwest::Client::builder()
            .user_agent(concat!("RedCrown/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|error| {
                TorrentError::new(format!("failed to create tracker-list client: {error}"))
            })?;
        let cached = load_matching_cache(&cache_root, &config).await;
        let has_cached_trackers = !cached.trackers.is_empty();
        let tracker_list = Arc::new(Self {
            client,
            cache_root,
            state: RwLock::new(TrackerListState {
                config,
                trackers: cached.trackers.into(),
                cache_generation: cached.generation,
            }),
            refresh_lock: Mutex::new(()),
        });

        if !has_cached_trackers && let Err(error) = tracker_list.refresh().await {
            event!(
                name: "torrent.tracker_list.refresh.failed",
                Level::WARN,
                error.message = error.user_message(),
                "tracker-list refresh failed; retaining the matching cached list"
            );
        }
        Ok(tracker_list)
    }

    /// Imports a configuration without changing the active tracker set.
    ///
    /// # Errors
    ///
    /// Returns an error when the new source cannot be read or contains no trackers.
    pub(super) async fn prepare_config(
        &self,
        config: TrackerListConfig,
    ) -> Result<PreparedTrackerList, TorrentError> {
        config
            .validate()
            .map_err(|error| TorrentError::new(error.user_message()))?;
        {
            let current = self.state.read().await;
            if current.config == config && (!config.enabled || !current.trackers.is_empty()) {
                return Ok(PreparedTrackerList {
                    config,
                    trackers: current.trackers.to_vec(),
                });
            }
        }
        let _guard = self.refresh_lock.lock().await;
        let trackers = if config.enabled {
            match self.read_source(&config.source).await {
                Ok(trackers) => trackers,
                Err(error) => {
                    let current = self.state.read().await;
                    if current.config == config && !current.trackers.is_empty() {
                        current.trackers.to_vec()
                    } else {
                        return Err(error);
                    }
                }
            }
        } else {
            Vec::new()
        };
        Ok(PreparedTrackerList { config, trackers })
    }

    /// Activates a previously imported configuration without further I/O.
    pub(super) async fn activate(&self, prepared: PreparedTrackerList) -> usize {
        let _guard = self.refresh_lock.lock().await;
        let PreparedTrackerList { config, trackers } = prepared;
        let current_generation = self.state.read().await.cache_generation;
        let cache_generation = if config.enabled {
            persist_cache(
                &self.cache_root,
                &config.source,
                &trackers,
                current_generation,
            )
            .await
        } else {
            current_generation
        };
        let count = trackers.len();
        *self.state.write().await = TrackerListState {
            config,
            trackers: trackers.into(),
            cache_generation,
        };
        count
    }

    /// Refreshes the current source while retaining prior data on failure.
    ///
    /// # Errors
    ///
    /// Returns an error when the source cannot be read or contains no trackers.
    pub(super) async fn refresh(&self) -> Result<usize, TorrentError> {
        let _guard = self.refresh_lock.lock().await;
        let config = self.state.read().await.config.clone();
        if !config.enabled {
            return Ok(0);
        }
        let trackers = self.read_source(&config.source).await?;
        let current_generation = self.state.read().await.cache_generation;
        let cache_generation = persist_cache(
            &self.cache_root,
            &config.source,
            &trackers,
            current_generation,
        )
        .await;
        let count = trackers.len();
        *self.state.write().await = TrackerListState {
            config,
            trackers: trackers.into(),
            cache_generation,
        };
        event!(
            name: "torrent.tracker_list.refresh.succeeded",
            Level::INFO,
            tracker.count = count,
            "tracker list refreshed"
        );
        Ok(count)
    }

    /// Returns imported trackers only for a trackerless magnet or bare hash.
    pub(super) async fn trackers_for(&self, source: &str) -> Arc<[String]> {
        if !requires_supplemental_trackers(source) {
            return Arc::from([]);
        }
        self.state.read().await.trackers.clone()
    }

    async fn read_source(&self, source: &TrackerListSource) -> Result<Vec<String>, TorrentError> {
        let bytes = match source {
            TrackerListSource::Url { url } => self.download(url).await?,
            TrackerListSource::File { path } => read_bounded_file(path).await?,
        };
        parse_tracker_list(&bytes)
    }

    async fn download(&self, url: &Url) -> Result<Vec<u8>, TorrentError> {
        let mut response = self
            .client
            .get(url.clone())
            .send()
            .await
            .map_err(|error| TorrentError::new(format!("tracker-list request failed: {error}")))?
            .error_for_status()
            .map_err(|error| TorrentError::new(format!("tracker-list request failed: {error}")))?;
        if response
            .content_length()
            .is_some_and(|length| length > MAX_TRACKER_LIST_BYTES as u64)
        {
            return Err(TorrentError::new("tracker list exceeds the 1 MiB limit"));
        }

        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|error| {
            TorrentError::new(format!("failed to read tracker-list response: {error}"))
        })? {
            if bytes.len().saturating_add(chunk.len()) > MAX_TRACKER_LIST_BYTES {
                return Err(TorrentError::new("tracker list exceeds the 1 MiB limit"));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
}

async fn read_bounded_file(path: &PathBuf) -> Result<Vec<u8>, TorrentError> {
    let file = fs::File::open(path)
        .await
        .map_err(|error| TorrentError::new(format!("failed to open tracker-list file: {error}")))?;
    let mut bytes = Vec::new();
    file.take((MAX_TRACKER_LIST_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .await
        .map_err(|error| TorrentError::new(format!("failed to read tracker-list file: {error}")))?;
    if bytes.len() > MAX_TRACKER_LIST_BYTES {
        return Err(TorrentError::new("tracker list exceeds the 1 MiB limit"));
    }
    Ok(bytes)
}

fn parse_tracker_list(bytes: &[u8]) -> Result<Vec<String>, TorrentError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| TorrentError::new("tracker list must be UTF-8 text"))?;
    let mut seen = HashSet::new();
    let mut trackers = Vec::new();
    for candidate in text.split_whitespace() {
        let Ok(mut url) = Url::parse(candidate) else {
            continue;
        };
        if !matches!(url.scheme(), "http" | "https" | "udp")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.host_str().is_none()
        {
            continue;
        }
        url.set_fragment(None);
        let normalized = url.to_string();
        if seen.insert(normalized.clone()) {
            trackers.push(normalized);
            if trackers.len() == MAX_TRACKERS {
                break;
            }
        }
    }
    if trackers.is_empty() {
        return Err(TorrentError::new(
            "tracker list contains no supported HTTP, HTTPS, or UDP tracker URLs",
        ));
    }
    Ok(trackers)
}

fn requires_supplemental_trackers(source: &str) -> bool {
    if source.len() == 40 && source.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return true;
    }
    let Ok(url) = Url::parse(source) else {
        return false;
    };
    url.scheme() == "magnet"
        && !url
            .query_pairs()
            .any(|(key, value)| key == "tr" && !value.is_empty())
}

#[derive(Debug, Default)]
struct CacheSnapshot {
    generation: u64,
    trackers: Vec<String>,
}

async fn load_matching_cache(root: &Path, config: &TrackerListConfig) -> CacheSnapshot {
    if !config.enabled {
        return CacheSnapshot::default();
    }
    let mut snapshots = Vec::with_capacity(2);
    for slot in [root.join(CACHE_SLOT_A), root.join(CACHE_SLOT_B)] {
        let Ok(bytes) = fs::read(slot).await else {
            continue;
        };
        let Ok(cache) = serde_json::from_slice::<TrackerListCache>(&bytes) else {
            continue;
        };
        if cache.schema_version != CACHE_SCHEMA_VERSION || cache.source != config.source {
            continue;
        }
        let Ok(trackers) = parse_tracker_list(cache.trackers.join("\n").as_bytes()) else {
            continue;
        };
        snapshots.push(CacheSnapshot {
            generation: cache.generation,
            trackers,
        });
    }
    snapshots
        .into_iter()
        .max_by_key(|snapshot| snapshot.generation)
        .unwrap_or_default()
}

async fn persist_cache(
    root: &Path,
    source: &TrackerListSource,
    trackers: &[String],
    current_generation: u64,
) -> u64 {
    let Some(generation) = current_generation.checked_add(1) else {
        event!(
            name: "torrent.tracker_list.cache.failed",
            Level::WARN,
            "tracker-list cache generation exhausted"
        );
        return current_generation;
    };
    let cache = TrackerListCache {
        schema_version: CACHE_SCHEMA_VERSION,
        generation,
        source: source.clone(),
        trackers: trackers.to_vec(),
    };
    let result = write_cache_slot(root, &cache).await;
    if let Err(error) = result {
        event!(
            name: "torrent.tracker_list.cache.failed",
            Level::WARN,
            error.message = %error,
            "tracker-list cache write failed"
        );
        return current_generation;
    }
    generation
}

async fn write_cache_slot(root: &Path, cache: &TrackerListCache) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(cache).map_err(std::io::Error::other)?;
    let slot = root.join(if cache.generation.is_multiple_of(2) {
        CACHE_SLOT_A
    } else {
        CACHE_SLOT_B
    });
    let temporary = root.join(format!(
        ".redcrown-trackers-{}.tmp",
        uuid::Uuid::new_v4().simple()
    ));
    let result = async {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .await?;
        file.write_all(&bytes).await?;
        file.flush().await?;
        file.sync_all().await?;
        drop(file);
        if let Err(error) = fs::remove_file(&slot).await
            && error.kind() != std::io::ErrorKind::NotFound
        {
            return Err(error);
        }
        fs::rename(&temporary, slot).await
    }
    .await;
    if result.is_err() {
        let _ = fs::remove_file(&temporary).await;
    }
    result
}

#[cfg(test)]
mod tests {
    use redcrown_core::{TrackerListConfig, TrackerListSource};
    use tempfile::tempdir;
    use url::Url;

    use super::{
        load_matching_cache, parse_tracker_list, persist_cache, requires_supplemental_trackers,
    };

    #[test]
    fn parses_whitespace_lists_and_removes_duplicates() {
        let trackers = parse_tracker_list(
            b"udp://tracker.example:80/announce\n\nhttps://tracker.example/announce udp://tracker.example:80/announce",
        )
        .expect("valid tracker list");
        assert_eq!(trackers.len(), 2);
    }

    #[test]
    fn supplements_only_trackerless_public_sources() {
        assert!(requires_supplemental_trackers(
            "magnet:?xt=urn:btih:18BADF35B4622F33E1BDBBCF8C323CE28A6DD5B9"
        ));
        assert!(requires_supplemental_trackers(
            "18BADF35B4622F33E1BDBBCF8C323CE28A6DD5B9"
        ));
        assert!(!requires_supplemental_trackers(
            "magnet:?xt=urn:btih:18BADF35B4622F33E1BDBBCF8C323CE28A6DD5B9&tr=https%3A%2F%2Ftracker.example%2Fannounce"
        ));
        assert!(!requires_supplemental_trackers(
            "https://example.test/file.torrent"
        ));
    }

    #[tokio::test]
    async fn replaces_cache_reliably_across_multiple_generations() {
        let root = tempdir().expect("cache root");
        let source = TrackerListSource::Url {
            url: Url::parse("https://example.test/trackers.txt").expect("source URL"),
        };
        let config = TrackerListConfig {
            enabled: true,
            source: source.clone(),
        };
        let first = vec!["udp://first.example:80/announce".to_owned()];
        let second = vec!["udp://second.example:80/announce".to_owned()];

        let generation = persist_cache(root.path(), &source, &first, 0).await;
        let generation = persist_cache(root.path(), &source, &second, generation).await;
        let loaded = load_matching_cache(root.path(), &config).await;

        assert_eq!(generation, 2);
        assert_eq!(loaded.generation, 2);
        assert_eq!(loaded.trackers, second);
    }
}
