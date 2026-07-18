//! Imports bounded supplemental tracker lists for trackerless magnets.
// Rust guideline compliant 2026-02-21

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use redcrown_core::{TrackerListConfig, TrackerListSource};
use serde::{Deserialize, Serialize};
use tokio::fs;
use tokio::io::AsyncReadExt;
use tokio::sync::{Mutex, RwLock};
use tracing::{Level, event};
use url::Url;

use super::TorrentError;

/// External lists are small; this cap prevents unbounded remote or file input.
const MAX_TRACKER_LIST_BYTES: usize = 1024 * 1024;
/// A malformed or hostile list cannot create an unbounded announce fan-out.
const MAX_TRACKERS: usize = 512;
const CACHE_SCHEMA_VERSION: u32 = 1;
const CACHE_FILENAME: &str = ".redcrown-trackers.json";

#[derive(Debug, Clone)]
struct TrackerListState {
    config: TrackerListConfig,
    trackers: Arc<[String]>,
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
    source: TrackerListSource,
    trackers: Vec<String>,
}

/// Owns validated supplemental trackers and their durable cache.
#[derive(Debug)]
pub(super) struct TrackerList {
    client: reqwest::Client,
    cache_path: PathBuf,
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
        let cache_path = cache_root.join(CACHE_FILENAME);
        let cached = load_matching_cache(&cache_path, &config).await;
        let has_cached_trackers = !cached.is_empty();
        let tracker_list = Arc::new(Self {
            client,
            cache_path,
            state: RwLock::new(TrackerListState {
                config,
                trackers: cached.into(),
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
        let PreparedTrackerList { config, trackers } = prepared;
        if config.enabled {
            persist_cache(&self.cache_path, &config.source, &trackers).await;
        }
        let count = trackers.len();
        *self.state.write().await = TrackerListState {
            config,
            trackers: trackers.into(),
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
        persist_cache(&self.cache_path, &config.source, &trackers).await;
        let count = trackers.len();
        *self.state.write().await = TrackerListState {
            config,
            trackers: trackers.into(),
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

async fn load_matching_cache(path: &PathBuf, config: &TrackerListConfig) -> Vec<String> {
    if !config.enabled {
        return Vec::new();
    }
    let Ok(bytes) = fs::read(path).await else {
        return Vec::new();
    };
    let Ok(cache) = serde_json::from_slice::<TrackerListCache>(&bytes) else {
        return Vec::new();
    };
    if cache.schema_version != CACHE_SCHEMA_VERSION || cache.source != config.source {
        return Vec::new();
    }
    parse_tracker_list(cache.trackers.join("\n").as_bytes()).unwrap_or_default()
}

async fn persist_cache(path: &PathBuf, source: &TrackerListSource, trackers: &[String]) {
    let cache = TrackerListCache {
        schema_version: CACHE_SCHEMA_VERSION,
        source: source.clone(),
        trackers: trackers.to_vec(),
    };
    let result = match serde_json::to_vec(&cache) {
        Ok(bytes) => {
            let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4().simple()));
            let result = async {
                fs::write(&temporary, bytes).await?;
                fs::rename(&temporary, path).await
            }
            .await;
            if result.is_err() {
                let _ = fs::remove_file(&temporary).await;
            }
            result
        }
        Err(error) => Err(std::io::Error::other(error)),
    };
    if let Err(error) = result {
        event!(
            name: "torrent.tracker_list.cache.failed",
            Level::WARN,
            error.message = %error,
            "tracker-list cache write failed"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_tracker_list, requires_supplemental_trackers};

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
}
