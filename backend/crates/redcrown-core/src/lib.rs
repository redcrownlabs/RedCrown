//! Defines `RedCrown`'s stable domain and IPC contract.
// Rust guideline compliant 2026-02-21

use std::backtrace::Backtrace;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

const DEFAULT_IDLE_EXPIRATION_SECS: u64 = 6 * 60 * 60;
const DEFAULT_MAXIMUM_AGE_SECS: u64 = 24 * 60 * 60;
const DEFAULT_CACHE_SIZE_BYTES: u64 = 20 * 1024 * 1024 * 1024;
const MIN_CACHE_DURATION_SECS: u64 = 60;
const MAX_CACHE_DURATION_SECS: u64 = 7 * 24 * 60 * 60;
const MIN_CACHE_SIZE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_CACHE_SIZE_BYTES: u64 = 500 * 1024 * 1024 * 1024;
const DEFAULT_TRACKER_LIST_URL: &str =
    "https://raw.githubusercontent.com/ngosang/trackerslist/refs/heads/master/trackers_all.txt";

/// Identifies one logical catalog source.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SourceId(String);

impl SourceId {
    /// Creates a source identifier from a stable value.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the identifier text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Identifies one configured source endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EndpointId(Uuid);

impl EndpointId {
    /// Creates a random endpoint identifier.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for EndpointId {
    fn default() -> Self {
        Self::new()
    }
}

/// Stores one validated API endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceEndpoint {
    /// Stable endpoint identifier.
    pub id: EndpointId,
    /// Base URL for the compatible API.
    pub url: Url,
    /// Whether failover may select this endpoint.
    pub enabled: bool,
}

impl SourceEndpoint {
    /// Validates and creates an API endpoint.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid URLs, unsupported schemes, or credentials.
    pub fn parse(value: impl AsRef<str>) -> Result<Self, ConfigurationError> {
        let mut url = Url::parse(value.as_ref())
            .map_err(|error| ConfigurationError::new(format!("invalid API URL: {error}")))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(ConfigurationError::new("API URL must use HTTP or HTTPS"));
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(ConfigurationError::new(
                "API URL must not contain credentials",
            ));
        }
        url.set_fragment(None);
        if !url.path().ends_with('/') {
            let normalized = format!("{}/", url.path());
            url.set_path(&normalized);
        }
        Ok(Self {
            id: EndpointId::new(),
            url,
            enabled: true,
        })
    }

    /// Validates a deserialized endpoint against the same rules as [`Self::parse`].
    ///
    /// # Errors
    ///
    /// Returns an error for unsupported schemes or embedded credentials.
    pub fn validate(&self) -> Result<(), ConfigurationError> {
        if !matches!(self.url.scheme(), "http" | "https") {
            return Err(ConfigurationError::new("API URL must use HTTP or HTTPS"));
        }
        if !self.url.username().is_empty() || self.url.password().is_some() {
            return Err(ConfigurationError::new(
                "API URL must not contain credentials",
            ));
        }
        Ok(())
    }
}

/// Configures one logical source and its ordered fallback chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceConfig {
    /// Stable source identifier.
    pub id: SourceId,
    /// User-facing source name.
    pub name: String,
    /// Whether the source participates in catalog requests.
    pub enabled: bool,
    /// Ordered compatible API endpoints.
    pub endpoints: Vec<SourceEndpoint>,
}

impl SourceConfig {
    /// Validates this source configuration.
    ///
    /// # Errors
    ///
    /// Returns an error when the name or enabled endpoint chain is empty.
    pub fn validate(&self) -> Result<(), ConfigurationError> {
        if self.name.trim().is_empty() {
            return Err(ConfigurationError::new("source name must not be empty"));
        }
        if self.enabled && !self.endpoints.iter().any(|endpoint| endpoint.enabled) {
            return Err(ConfigurationError::new(
                "enabled source requires at least one enabled endpoint",
            ));
        }
        for endpoint in &self.endpoints {
            endpoint.validate()?;
        }
        Ok(())
    }
}

/// Controls temporary torrent cache retention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamCachePolicy {
    /// Idle seconds after playback releases a cache lease.
    pub idle_expiration_secs: u64,
    /// Absolute maximum cache-entry age in seconds.
    pub maximum_age_secs: u64,
    /// Maximum aggregate stream-cache bytes.
    pub size_budget_bytes: u64,
}

impl StreamCachePolicy {
    /// Returns the default bounded cache policy.
    #[must_use]
    pub const fn standard() -> Self {
        Self {
            idle_expiration_secs: DEFAULT_IDLE_EXPIRATION_SECS,
            maximum_age_secs: DEFAULT_MAXIMUM_AGE_SECS,
            size_budget_bytes: DEFAULT_CACHE_SIZE_BYTES,
        }
    }

    /// Validates cache bounds and ordering.
    ///
    /// # Errors
    ///
    /// Returns an error when durations or size exceed supported bounds.
    pub fn validate(&self) -> Result<(), ConfigurationError> {
        if !(MIN_CACHE_DURATION_SECS..=MAX_CACHE_DURATION_SECS).contains(&self.idle_expiration_secs)
        {
            return Err(ConfigurationError::new(
                "idle expiration must be between 60 seconds and 7 days",
            ));
        }
        if !(self.idle_expiration_secs..=MAX_CACHE_DURATION_SECS).contains(&self.maximum_age_secs) {
            return Err(ConfigurationError::new(
                "maximum age must be at least idle expiration and at most 7 days",
            ));
        }
        if !(MIN_CACHE_SIZE_BYTES..=MAX_CACHE_SIZE_BYTES).contains(&self.size_budget_bytes) {
            return Err(ConfigurationError::new(
                "cache size must be between 512 MiB and 500 GiB",
            ));
        }
        Ok(())
    }

    /// Returns the idle expiration duration.
    #[must_use]
    pub const fn idle_expiration(&self) -> Duration {
        Duration::from_secs(self.idle_expiration_secs)
    }

    /// Returns the absolute maximum age.
    #[must_use]
    pub const fn maximum_age(&self) -> Duration {
        Duration::from_secs(self.maximum_age_secs)
    }
}

impl Default for StreamCachePolicy {
    fn default() -> Self {
        Self::standard()
    }
}

/// Selects where `RedCrown` imports supplemental public trackers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TrackerListSource {
    /// Download the list over HTTP.
    Url {
        /// Remote tracker-list URL.
        url: Url,
    },
    /// Read the list from an absolute local path.
    File {
        /// Local tracker-list file.
        path: PathBuf,
    },
}

impl TrackerListSource {
    /// Validates the configured tracker-list location.
    ///
    /// # Errors
    ///
    /// Returns an error for insecure URLs, credentials, or relative file paths.
    pub fn validate(&self) -> Result<(), ConfigurationError> {
        match self {
            Self::Url { url } => {
                if url.scheme() != "https" {
                    return Err(ConfigurationError::new("tracker-list URL must use HTTPS"));
                }
                if !url.username().is_empty() || url.password().is_some() {
                    return Err(ConfigurationError::new(
                        "tracker-list URL must not contain credentials",
                    ));
                }
                if url.host_str().is_none() {
                    return Err(ConfigurationError::new(
                        "tracker-list URL must include a host",
                    ));
                }
            }
            Self::File { path } if !path.is_absolute() => {
                return Err(ConfigurationError::new(
                    "tracker-list file path must be absolute",
                ));
            }
            Self::File { .. } => {}
        }
        Ok(())
    }
}

impl Default for TrackerListSource {
    fn default() -> Self {
        Self::Url {
            url: Url::parse(DEFAULT_TRACKER_LIST_URL)
                .expect("the built-in tracker-list URL must remain valid"),
        }
    }
}

/// Configures supplemental tracker discovery for trackerless magnets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackerListConfig {
    /// Whether imported trackers participate in metadata discovery.
    pub enabled: bool,
    /// Remote or local tracker-list location.
    pub source: TrackerListSource,
}

impl TrackerListConfig {
    /// Validates the tracker-list configuration.
    ///
    /// # Errors
    ///
    /// Returns an error when the selected source is invalid.
    pub fn validate(&self) -> Result<(), ConfigurationError> {
        self.source.validate()
    }
}

impl Default for TrackerListConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            source: TrackerListSource::default(),
        }
    }
}

/// Stores user-editable application settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppSettings {
    /// Version for explicit migrations.
    pub schema_version: u32,
    /// Configured logical catalog sources.
    pub sources: Vec<SourceConfig>,
    /// Temporary stream-cache policy.
    pub stream_cache: StreamCachePolicy,
    /// Supplemental public tracker-list import.
    #[serde(default)]
    pub tracker_list: TrackerListConfig,
    /// Preferred interface theme.
    pub theme: ThemePreference,
}

impl AppSettings {
    /// Returns safe initial settings.
    #[must_use]
    pub fn initial() -> Self {
        Self {
            schema_version: 1,
            sources: vec![SourceConfig {
                id: SourceId::new("primary"),
                name: "Primary catalog".to_owned(),
                enabled: true,
                endpoints: Vec::new(),
            }],
            stream_cache: StreamCachePolicy::standard(),
            tracker_list: TrackerListConfig::default(),
            theme: ThemePreference::System,
        }
    }

    /// Validates every user-editable setting.
    ///
    /// # Errors
    ///
    /// Returns the first invalid source or cache-policy error.
    pub fn validate(&self) -> Result<(), ConfigurationError> {
        self.stream_cache.validate()?;
        self.tracker_list.validate()?;
        for source in &self.sources {
            if source.enabled && source.endpoints.is_empty() {
                continue;
            }
            source.validate()?;
        }
        Ok(())
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self::initial()
    }
}

/// Selects the renderer color theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    /// Follow the operating-system preference.
    System,
    /// Use the light theme.
    Light,
    /// Use the dark theme.
    Dark,
}

/// Describes a catalog media item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaItem {
    /// Provider-scoped media identifier.
    pub id: String,
    /// Display title.
    pub title: String,
    /// Release year when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<u16>,
    /// Short synopsis.
    pub synopsis: String,
    /// Poster URL when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poster_url: Option<Url>,
    /// Wide artwork used for feature and row presentation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backdrop_url: Option<Url>,
    /// Rating on a ten-point scale.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rating: Option<f32>,
    /// Media kind.
    pub kind: MediaKind,
    /// Provider-supplied genres.
    pub genres: Vec<String>,
    /// Available torrent sources.
    pub torrents: Vec<TorrentOption>,
}

/// Identifies a media category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    /// Feature-length movie.
    Movie,
    /// Episodic series.
    Series,
    /// Episodic animation selected through the provider's anime catalog mode.
    Anime,
}

/// Selects the provider-supported catalog ordering.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogSort {
    /// Provider-defined trend score.
    #[default]
    Trending,
    /// Provider-defined popularity score.
    Popularity,
    /// Most recently changed catalog entries.
    Updated,
    /// Most recently added movies.
    LastAdded,
    /// Descending release year.
    Year,
    /// Alphabetical title or show name.
    Title,
    /// Descending provider rating.
    Rating,
}

/// Describes one server-side catalog page request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogQuery {
    /// Requested media category.
    pub kind: MediaKind,
    /// One-based provider page.
    pub page: u32,
    /// Provider-supported ordering.
    #[serde(default)]
    pub sort: CatalogSort,
    /// Optional exact provider genre.
    #[serde(default)]
    pub genre: Option<String>,
    /// Optional provider-side title search.
    #[serde(default)]
    pub keywords: Option<String>,
}

/// Contains a normalized provider page and continuation state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogPage {
    /// Normalized media items.
    pub items: Vec<MediaItem>,
    /// One-based page returned by the provider.
    pub page: u32,
    /// Whether another provider page is likely available.
    pub has_more: bool,
}

/// Describes one playable torrent option.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TorrentOption {
    /// User-facing quality label.
    pub quality: String,
    /// Magnet or torrent URL.
    pub source: String,
    /// Total download size in bytes when supplied by the catalog.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    /// Seeder count when supplied by the catalog.
    pub seeders: Option<u32>,
    /// Provider name when supplied by the catalog.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Exact media path inside a multi-file torrent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    /// Provider release or filename shown as source detail.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
}

/// Describes one series or anime episode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaEpisode {
    /// Provider season number.
    pub season: u16,
    /// Provider episode number.
    pub episode: u16,
    /// Display title.
    pub title: String,
    /// Episode synopsis when supplied by the catalog.
    pub synopsis: String,
    /// Available torrent sources for this exact episode.
    pub torrents: Vec<TorrentOption>,
}

/// Reports one endpoint health check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointHealth {
    /// Endpoint being checked.
    pub endpoint_id: EndpointId,
    /// Whether the endpoint returned a compatible response.
    pub reachable: bool,
    /// HTTP status when a response was received.
    pub status: Option<u16>,
    /// Human-readable bounded result.
    pub message: String,
    /// Request duration in milliseconds.
    pub latency_ms: u64,
}

/// Represents a desktop backend command.
#[derive(Debug, Deserialize)]
pub struct IpcRequest {
    /// Correlates responses with requests.
    pub id: Uuid,
    /// Command name.
    pub method: String,
    /// Command-specific JSON payload.
    #[serde(default)]
    pub params: serde_json::Value,
}

/// Represents a desktop backend response.
#[derive(Debug, Serialize)]
pub struct IpcResponse<T> {
    /// Correlates responses with requests.
    pub id: Uuid,
    /// Successful result when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<T>,
    /// Structured failure when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<IpcError>,
}

impl<T> IpcResponse<T> {
    /// Creates a successful response.
    #[must_use]
    pub const fn success(id: Uuid, result: T) -> Self {
        Self {
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Creates a failed response.
    #[must_use]
    pub fn failure(id: Uuid, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            id,
            result: None,
            error: Some(IpcError {
                code: code.into(),
                message: message.into(),
            }),
        }
    }
}

/// Contains a safe renderer-facing backend error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IpcError {
    /// Stable error category.
    pub code: String,
    /// Safe user-facing error message.
    pub message: String,
}

/// Reports application bootstrap state.
#[derive(Debug, Clone, Serialize)]
pub struct BootstrapState {
    /// Backend protocol version.
    pub protocol_version: u32,
    /// Application settings.
    pub settings: AppSettings,
    /// Built-in lawful fixture catalog.
    pub featured: Vec<MediaItem>,
    /// Torrent engine readiness.
    pub torrent_engine_ready: bool,
}

/// Reports invalid application configuration.
#[derive(Debug)]
pub struct ConfigurationError {
    message: String,
    backtrace: Backtrace,
}

impl ConfigurationError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            backtrace: Backtrace::capture(),
        }
    }

    /// Returns the bounded message safe to show in the desktop renderer.
    #[must_use]
    pub fn user_message(&self) -> &str {
        &self.message
    }
}

impl Display for ConfigurationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}\n{}", self.message, self.backtrace)
    }
}

impl std::error::Error for ConfigurationError {}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{AppSettings, SourceEndpoint, StreamCachePolicy, TrackerListSource};

    #[test]
    fn endpoint_rejects_credentials() {
        let error = SourceEndpoint::parse("https://user:secret@example.com").expect_err("error");
        assert!(error.to_string().contains("credentials"));
    }

    #[test]
    fn cache_policy_rejects_maximum_age_before_idle_expiration() {
        let policy = StreamCachePolicy {
            idle_expiration_secs: 3_600,
            maximum_age_secs: 60,
            size_budget_bytes: 1024 * 1024 * 1024,
        };
        assert!(policy.validate().is_err());
    }

    #[test]
    fn tracker_list_requires_https_or_an_absolute_file() {
        let insecure = TrackerListSource::Url {
            url: url::Url::parse("http://example.com/trackers.txt").expect("test URL"),
        };
        let relative = TrackerListSource::File {
            path: PathBuf::from("trackers.txt"),
        };

        assert!(insecure.validate().is_err());
        assert!(relative.validate().is_err());
    }

    #[test]
    fn older_settings_receive_the_default_tracker_list() {
        let value = serde_json::json!({
            "schema_version": 1,
            "sources": [],
            "stream_cache": {
                "idle_expiration_secs": 21_600,
                "maximum_age_secs": 86_400,
                "size_budget_bytes": 21_474_836_480_u64
            },
            "theme": "system"
        });
        let settings: AppSettings = serde_json::from_value(value).expect("legacy settings");

        assert!(settings.tracker_list.enabled);
        assert!(settings.tracker_list.validate().is_ok());
    }
}
