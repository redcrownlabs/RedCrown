//! Resolves compatible catalog APIs through deterministic endpoint failover.
// Rust guideline compliant 2026-02-21

use std::backtrace::Backtrace;
use std::fmt::{Display, Formatter};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use redcrown_core::{
    CatalogPage, CatalogQuery, CatalogSort, EndpointHealth, EndpointId, MediaEpisode, MediaItem,
    MediaKind, SourceConfig, SourceEndpoint, TorrentOption,
};
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use tokio::sync::RwLock;
use tracing::{Level, event};
use url::Url;

const ENDPOINT_TIMEOUT: Duration = Duration::from_secs(6);
const PREFERENCE_WINDOW: Duration = Duration::from_secs(5 * 60);
const MAX_CATALOG_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const CATALOG_PAGE_SIZE: usize = 50;
const MAX_KEYWORDS_LENGTH: usize = 200;
const MAX_GENRE_LENGTH: usize = 80;
const MAX_MEDIA_ID_LENGTH: usize = 200;

/// Fetches normalized catalog items.
#[async_trait]
pub trait CatalogProvider: Send + Sync + std::fmt::Debug {
    /// Browses one media category.
    ///
    /// # Errors
    ///
    /// Returns an error when every configured endpoint fails.
    async fn browse(&self, query: &CatalogQuery) -> Result<CatalogPage, CatalogError>;

    /// Loads episodes and their exact torrent sources.
    ///
    /// # Errors
    ///
    /// Returns an error when the identifier is invalid or every endpoint fails.
    async fn episodes(&self, media_id: &str) -> Result<Vec<MediaEpisode>, CatalogError>;
}

#[derive(Debug, Clone, Copy)]
struct PreferredEndpoint {
    id: EndpointId,
    expires_at: Instant,
}

#[derive(Debug)]
struct EndpointChainInner {
    client: Client,
    source: SourceConfig,
    preferred: RwLock<Option<PreferredEndpoint>>,
}

/// Executes requests through an ordered compatible endpoint chain.
#[derive(Debug, Clone)]
pub struct EndpointChain {
    inner: Arc<EndpointChainInner>,
}

impl EndpointChain {
    /// Creates a validated endpoint chain.
    ///
    /// # Errors
    ///
    /// Returns an error when the source configuration is invalid.
    pub fn new(source: SourceConfig) -> Result<Self, CatalogError> {
        source
            .validate()
            .map_err(|error| CatalogError::new(error.to_string()))?;
        let client = Client::builder()
            .timeout(ENDPOINT_TIMEOUT)
            .user_agent(concat!("RedCrown/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| CatalogError::new(format!("failed to build HTTP client: {error}")))?;
        Ok(Self {
            inner: Arc::new(EndpointChainInner {
                client,
                source,
                preferred: RwLock::new(None),
            }),
        })
    }

    /// Tests every enabled endpoint without changing active configuration.
    #[must_use = "endpoint tests must be observed"]
    pub async fn test_all(&self) -> Vec<EndpointHealth> {
        let endpoints = self.enabled_endpoints_in_order().await;
        let mut health = Vec::with_capacity(endpoints.len());
        for endpoint in endpoints {
            health.push(self.test_endpoint(&endpoint).await);
        }
        health
    }

    /// Tests one endpoint without changing active configuration.
    #[must_use = "endpoint tests must be observed"]
    pub async fn test_endpoint(&self, endpoint: &SourceEndpoint) -> EndpointHealth {
        let started = Instant::now();
        let result = self.inner.client.get(endpoint.url.clone()).send().await;
        match result {
            Ok(response) => {
                let status = response.status();
                EndpointHealth {
                    endpoint_id: endpoint.id,
                    reachable: status.is_success(),
                    status: Some(status.as_u16()),
                    message: if status.is_success() {
                        "Endpoint is reachable".to_owned()
                    } else {
                        format!("Endpoint returned HTTP {}", status.as_u16())
                    },
                    latency_ms: elapsed_millis(started),
                }
            }
            Err(error) => EndpointHealth {
                endpoint_id: endpoint.id,
                reachable: false,
                status: error.status().map(|status| status.as_u16()),
                message: bounded_error_message(&error),
                latency_ms: elapsed_millis(started),
            },
        }
    }

    async fn get_json(&self, relative: &str) -> Result<serde_json::Value, CatalogError> {
        let endpoints = self.enabled_endpoints_in_order().await;
        if endpoints.is_empty() {
            return Err(CatalogError::new("source has no enabled API endpoints"));
        }

        let mut failures = Vec::with_capacity(endpoints.len());
        for endpoint in endpoints {
            let url = endpoint
                .url
                .join(relative)
                .map_err(|error| CatalogError::new(format!("invalid catalog path: {error}")))?;
            match self.fetch_json(&url).await {
                Ok(value) => {
                    self.remember_healthy(endpoint.id).await;
                    event!(
                        name: "catalog.endpoint.request.success",
                        Level::INFO,
                        source.id = self.inner.source.id.as_str(),
                        endpoint.host = endpoint.url.host_str().unwrap_or("<unknown>"),
                        "catalog request succeeded"
                    );
                    return Ok(value);
                }
                Err(error) => {
                    failures.push(format!(
                        "{}: {}",
                        endpoint.url.host_str().unwrap_or("<unknown>"),
                        error.summary()
                    ));
                    event!(
                        name: "catalog.endpoint.request.failed",
                        Level::WARN,
                        source.id = self.inner.source.id.as_str(),
                        endpoint.host = endpoint.url.host_str().unwrap_or("<unknown>"),
                        error.message = error.summary(),
                        "catalog request failed"
                    );
                }
            }
        }
        Err(CatalogError::new(format!(
            "all catalog endpoints failed: {}",
            failures.join("; ")
        )))
    }

    async fn fetch_json(&self, url: &Url) -> Result<serde_json::Value, CatalogError> {
        let response = self
            .inner
            .client
            .get(url.clone())
            .send()
            .await
            .map_err(|error| CatalogError::from_request(&error))?;
        let status = response.status();
        if !status.is_success() {
            return Err(CatalogError::http(status));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_CATALOG_RESPONSE_BYTES as u64)
        {
            return Err(CatalogError::new("catalog response exceeds 8 MiB"));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|error| CatalogError::from_request(&error))?;
        if bytes.len() > MAX_CATALOG_RESPONSE_BYTES {
            return Err(CatalogError::new("catalog response exceeds 8 MiB"));
        }
        serde_json::from_slice(&bytes)
            .map_err(|error| CatalogError::new(format!("invalid catalog JSON: {error}")))
    }

    async fn enabled_endpoints_in_order(&self) -> Vec<SourceEndpoint> {
        let mut endpoints: Vec<_> = self
            .inner
            .source
            .endpoints
            .iter()
            .filter(|endpoint| endpoint.enabled)
            .cloned()
            .collect();
        let preferred = *self.inner.preferred.read().await;
        if let Some(preferred) = preferred.filter(|entry| entry.expires_at > Instant::now())
            && let Some(index) = endpoints
                .iter()
                .position(|endpoint| endpoint.id == preferred.id)
        {
            let endpoint = endpoints.remove(index);
            endpoints.insert(0, endpoint);
        }
        endpoints
    }

    async fn remember_healthy(&self, endpoint_id: EndpointId) {
        let primary = self
            .inner
            .source
            .endpoints
            .iter()
            .find(|endpoint| endpoint.enabled)
            .map(|endpoint| endpoint.id);
        let mut preferred = self.inner.preferred.write().await;
        *preferred = if primary == Some(endpoint_id) {
            None
        } else {
            Some(PreferredEndpoint {
                id: endpoint_id,
                expires_at: Instant::now() + PREFERENCE_WINDOW,
            })
        };
    }
}

/// Implements the Popcorn Time-compatible catalog contract.
#[derive(Debug, Clone)]
pub struct ButterCatalog {
    endpoints: EndpointChain,
}

impl ButterCatalog {
    /// Creates a compatible catalog provider.
    ///
    /// # Errors
    ///
    /// Returns an error when the source configuration is invalid.
    pub fn new(source: SourceConfig) -> Result<Self, CatalogError> {
        Ok(Self {
            endpoints: EndpointChain::new(source)?,
        })
    }

    /// Returns the endpoint chain for health operations.
    #[must_use]
    pub const fn endpoints(&self) -> &EndpointChain {
        &self.endpoints
    }
}

#[async_trait]
impl CatalogProvider for ButterCatalog {
    async fn browse(&self, query: &CatalogQuery) -> Result<CatalogPage, CatalogError> {
        let path = build_browse_path(query)?;
        let value = self.endpoints.get_json(&path).await?;
        let items = normalize_catalog(value, query.kind)?;
        let has_more = items.len() == CATALOG_PAGE_SIZE;
        Ok(CatalogPage {
            items,
            page: query.page,
            has_more,
        })
    }

    async fn episodes(&self, media_id: &str) -> Result<Vec<MediaEpisode>, CatalogError> {
        let path = build_episodes_path(media_id)?;
        let value = self.endpoints.get_json(&path).await?;
        normalize_episodes(value)
    }
}

fn build_browse_path(query: &CatalogQuery) -> Result<String, CatalogError> {
    if query.page == 0 {
        return Err(CatalogError::new("catalog page must be at least 1"));
    }
    let keywords = validated_filter(query.keywords.as_deref(), MAX_KEYWORDS_LENGTH, "keywords")?;
    let genre = validated_filter(query.genre.as_deref(), MAX_GENRE_LENGTH, "genre")?;
    let resource = match query.kind {
        MediaKind::Movie => "movies",
        MediaKind::Series | MediaKind::Anime => "shows",
    };
    let mut parameters = url::form_urlencoded::Serializer::new(String::new());
    parameters
        .append_pair("sort", provider_sort(query.kind, query.sort))
        .append_pair("limit", &CATALOG_PAGE_SIZE.to_string())
        .append_pair("locale", "en")
        .append_pair("contentLocale", "en")
        .append_pair("showAll", "1");
    if query.kind == MediaKind::Anime {
        parameters.append_pair("anime", "1");
    }
    if let Some(genre) = genre {
        parameters.append_pair("genre", genre);
    }
    if let Some(keywords) = keywords {
        parameters.append_pair("keywords", keywords);
    }
    Ok(format!("{resource}/{}?{}", query.page, parameters.finish()))
}

fn build_episodes_path(media_id: &str) -> Result<String, CatalogError> {
    let media_id = media_id.trim();
    if media_id.is_empty() {
        return Err(CatalogError::new("media identifier cannot be empty"));
    }
    if media_id.chars().count() > MAX_MEDIA_ID_LENGTH {
        return Err(CatalogError::new("media identifier exceeds 200 characters"));
    }
    if !media_id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(CatalogError::new(
            "media identifier contains unsupported characters",
        ));
    }
    Ok(format!(
        "show/{media_id}?locale=en&contentLocale=en&showAll=1"
    ))
}

fn provider_sort(kind: MediaKind, sort: CatalogSort) -> &'static str {
    match sort {
        CatalogSort::Trending => "trending",
        CatalogSort::Popularity => "popularity",
        CatalogSort::Updated | CatalogSort::LastAdded => match kind {
            MediaKind::Movie => "last added",
            MediaKind::Series | MediaKind::Anime => "updated",
        },
        CatalogSort::Year => "year",
        CatalogSort::Title => match kind {
            MediaKind::Movie => "title",
            MediaKind::Series | MediaKind::Anime => "name",
        },
        CatalogSort::Rating => "rating",
    }
}

fn validated_filter<'a>(
    value: Option<&'a str>,
    maximum_length: usize,
    name: &str,
) -> Result<Option<&'a str>, CatalogError> {
    let value = value.map(str::trim).filter(|value| !value.is_empty());
    if value.is_some_and(|value| value.chars().count() > maximum_length) {
        return Err(CatalogError::new(format!(
            "catalog {name} exceeds {maximum_length} characters"
        )));
    }
    Ok(value)
}

#[derive(Debug, Deserialize)]
struct ButterItem {
    #[serde(default)]
    id: String,
    #[serde(default, rename = "_id")]
    internal_id: String,
    #[serde(default)]
    imdb_id: String,
    #[serde(default)]
    tvdb_id: String,
    #[serde(default, alias = "name")]
    title: String,
    #[serde(default)]
    slug: String,
    #[serde(default)]
    year: serde_json::Value,
    #[serde(default, alias = "description", alias = "overview")]
    synopsis: String,
    #[serde(default)]
    rating: serde_json::Value,
    #[serde(default)]
    images: Option<ButterImages>,
    #[serde(default, alias = "genre")]
    genres: Option<Vec<String>>,
    #[serde(default)]
    torrents: serde_json::Value,
}

#[derive(Debug, Default, Deserialize)]
struct ButterImages {
    #[serde(default)]
    poster: serde_json::Value,
    #[serde(default)]
    fanart: serde_json::Value,
}

#[derive(Debug, Default, Deserialize)]
struct ButterShow {
    #[serde(default)]
    episodes: Vec<ButterEpisode>,
}

#[derive(Debug, Default, Deserialize)]
struct ButterEpisode {
    #[serde(default)]
    season: serde_json::Value,
    #[serde(default)]
    episode: serde_json::Value,
    #[serde(default)]
    title: String,
    #[serde(default, alias = "description", alias = "overview")]
    synopsis: String,
    #[serde(default)]
    torrents: serde_json::Value,
}

fn normalize_catalog(
    value: serde_json::Value,
    kind: MediaKind,
) -> Result<Vec<MediaItem>, CatalogError> {
    let items_value = match value {
        serde_json::Value::Array(items) => serde_json::Value::Array(items),
        serde_json::Value::Object(mut object) => object
            .remove("results")
            .or_else(|| object.remove("movies"))
            .or_else(|| object.remove("shows"))
            .unwrap_or(serde_json::Value::Array(Vec::new())),
        _ => return Err(CatalogError::new("catalog response must contain an array")),
    };
    let items: Vec<ButterItem> = serde_json::from_value(items_value)
        .map_err(|error| CatalogError::new(format!("incompatible catalog payload: {error}")))?;
    Ok(items
        .into_iter()
        .filter(|item| !item.title.trim().is_empty() || !item.slug.trim().is_empty())
        .map(|item| {
            let title = if kind == MediaKind::Anime && !item.slug.trim().is_empty() {
                title_from_slug(&item.slug)
            } else {
                item.title.clone()
            };
            let id = [
                item.id.as_str(),
                item.imdb_id.as_str(),
                item.tvdb_id.as_str(),
                item.internal_id.as_str(),
            ]
            .into_iter()
            .find(|candidate| !candidate.is_empty())
            .unwrap_or(&title)
            .to_owned();
            MediaItem {
                id,
                title,
                year: normalize_year(&item.year),
                synopsis: item.synopsis,
                poster_url: item
                    .images
                    .as_ref()
                    .and_then(|images| normalize_image_url(&images.poster)),
                backdrop_url: item
                    .images
                    .as_ref()
                    .and_then(|images| normalize_image_url(&images.fanart)),
                rating: normalize_rating(&item.rating),
                kind,
                genres: item.genres.unwrap_or_default(),
                torrents: normalize_torrents(&item.torrents),
            }
        })
        .collect())
}

fn title_from_slug(slug: &str) -> String {
    slug.split('-')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut characters = part.chars();
            characters.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(characters).collect()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_torrents(value: &serde_json::Value) -> Vec<TorrentOption> {
    let Some(object) = value.as_object() else {
        return Vec::new();
    };
    let direct = object
        .iter()
        .filter_map(|(quality, torrent)| normalize_torrent(quality, torrent))
        .collect::<Vec<_>>();
    if !direct.is_empty() {
        return deduplicate_torrents(direct);
    }

    deduplicate_torrents(
        object
            .iter()
            .flat_map(|(language, qualities)| {
                qualities
                    .as_object()
                    .into_iter()
                    .flatten()
                    .filter_map(move |(quality, torrent)| {
                        let label = if object.len() == 1 {
                            quality.clone()
                        } else {
                            format!("{quality} · {language}")
                        };
                        normalize_torrent(&label, torrent)
                    })
            })
            .collect(),
    )
}

fn normalize_torrent(quality: &str, value: &serde_json::Value) -> Option<TorrentOption> {
    let source = value
        .get("url")
        .or_else(|| value.get("magnet"))
        .and_then(serde_json::Value::as_str)?;
    let seeders = value
        .get("seed")
        .or_else(|| value.get("seeds"))
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok());
    let normalized_quality = value
        .get("quality")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(quality);
    Some(TorrentOption {
        quality: normalized_quality.to_owned(),
        source: source.replace("&amp;", "&"),
        size_bytes: torrent_size_bytes(value),
        seeders,
        provider: value
            .get("provider")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        file_path: value
            .get("file")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned),
        file_name: ["file", "filename", "name", "title"]
            .into_iter()
            .find_map(|key| {
                value
                    .get(key)
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
            })
            .map(str::to_owned),
    })
}

fn torrent_size_bytes(value: &serde_json::Value) -> Option<u64> {
    ["size", "bytes"].into_iter().find_map(|key| {
        let size = value.get(key)?;
        size.as_u64().or_else(|| {
            size.as_str()
                .map(str::trim)
                .filter(|size| !size.is_empty())
                .and_then(|size| size.parse().ok())
        })
    })
}

fn deduplicate_torrents(torrents: Vec<TorrentOption>) -> Vec<TorrentOption> {
    let mut unique = Vec::with_capacity(torrents.len());
    for torrent in torrents {
        if let Some(existing) = unique.iter_mut().find(|existing: &&mut TorrentOption| {
            existing.source == torrent.source && existing.file_path == torrent.file_path
        }) {
            merge_duplicate_torrent(existing, torrent);
        } else {
            unique.push(torrent);
        }
    }
    unique
}

/// Merge provider aliases that identify the same torrent and exact media file.
///
/// Compatible APIs sometimes repeat one source under a numeric key and a
/// quality key, with metadata split between those aliases. Keeping whichever
/// alias happens to be iterated first would make size and release details
/// dependent on JSON object ordering.
fn merge_duplicate_torrent(existing: &mut TorrentOption, duplicate: TorrentOption) {
    existing.size_bytes = existing.size_bytes.or(duplicate.size_bytes);
    existing.seeders = match (existing.seeders, duplicate.seeders) {
        (Some(existing), Some(duplicate)) => Some(existing.max(duplicate)),
        (existing, duplicate) => existing.or(duplicate),
    };
    existing.provider = existing.provider.take().or(duplicate.provider);
    existing.file_name = existing.file_name.take().or(duplicate.file_name);
}

fn normalize_episodes(value: serde_json::Value) -> Result<Vec<MediaEpisode>, CatalogError> {
    let show: ButterShow = serde_json::from_value(value)
        .map_err(|error| CatalogError::new(format!("incompatible show payload: {error}")))?;
    let mut episodes = show
        .episodes
        .into_iter()
        .filter_map(|episode| {
            let season = normalize_episode_number(&episode.season)?;
            let number = normalize_episode_number(&episode.episode)?;
            Some(MediaEpisode {
                season,
                episode: number,
                title: episode.title,
                synopsis: episode.synopsis,
                torrents: normalize_torrents(&episode.torrents),
            })
        })
        .collect::<Vec<_>>();
    episodes.sort_by_key(|episode| (episode.season, episode.episode));
    Ok(episodes)
}

fn normalize_episode_number(value: &serde_json::Value) -> Option<u16> {
    value
        .as_u64()
        .and_then(|number| u16::try_from(number).ok())
        .or_else(|| value.as_str()?.trim().parse().ok())
}

fn normalize_year(value: &serde_json::Value) -> Option<u16> {
    value
        .as_u64()
        .and_then(|year| u16::try_from(year).ok())
        .or_else(|| value.as_str()?.trim().parse().ok())
}

fn normalize_rating(value: &serde_json::Value) -> Option<f32> {
    let rating = value
        .as_f64()
        .and_then(f64_to_f32)
        .or_else(|| value.as_str()?.trim().parse().ok())
        .or_else(|| {
            value
                .get("percentage")
                .and_then(serde_json::Value::as_f64)
                .and_then(|percentage| f64_to_f32(percentage / 10.0))
        })
        .or_else(|| {
            value
                .get("rating")
                .and_then(serde_json::Value::as_f64)
                .and_then(f64_to_f32)
        })?;
    rating.is_finite().then_some(rating.clamp(0.0, 10.0))
}

fn normalize_image_url(value: &serde_json::Value) -> Option<Url> {
    let value = value.as_str()?.trim();
    if value.is_empty() {
        return None;
    }
    let mut url = Url::parse(value).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    if url.scheme() == "http" && url.host_str() == Some("image.tmdb.org") {
        url.set_scheme("https").ok()?;
    }
    Some(url)
}

fn f64_to_f32(value: f64) -> Option<f32> {
    value.to_string().parse().ok()
}

fn elapsed_millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn bounded_error_message(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "Connection timed out".to_owned()
    } else if error.is_connect() {
        "Could not connect".to_owned()
    } else {
        "Endpoint request failed".to_owned()
    }
}

/// Reports a catalog or endpoint-chain failure.
#[derive(Debug)]
pub struct CatalogError {
    message: String,
    backtrace: Backtrace,
}

impl CatalogError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            backtrace: Backtrace::capture(),
        }
    }

    fn from_request(error: &reqwest::Error) -> Self {
        Self::new(bounded_error_message(error))
    }

    fn http(status: StatusCode) -> Self {
        Self::new(format!(
            "catalog endpoint returned HTTP {}",
            status.as_u16()
        ))
    }

    fn summary(&self) -> &str {
        &self.message
    }

    /// Returns the bounded message safe to show in the desktop renderer.
    #[must_use]
    pub fn user_message(&self) -> &str {
        &self.message
    }
}

impl Display for CatalogError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}\n{}", self.message, self.backtrace)
    }
}

impl std::error::Error for CatalogError {}

#[cfg(test)]
mod tests {
    use redcrown_core::{
        CatalogQuery, CatalogSort, MediaKind, SourceConfig, SourceEndpoint, SourceId,
    };
    use url::Url;

    use super::{
        ButterCatalog, CatalogProvider, build_browse_path, build_episodes_path, normalize_catalog,
        normalize_episodes, normalize_torrent,
    };

    #[tokio::test]
    async fn empty_endpoint_chain_fails_without_hanging() {
        let provider = ButterCatalog::new(SourceConfig {
            id: SourceId::new("test"),
            name: "Test".to_owned(),
            enabled: true,
            endpoints: vec![SourceEndpoint::parse("https://127.0.0.1:1").expect("endpoint")],
        })
        .expect("provider");
        let error = provider
            .browse(&CatalogQuery {
                kind: MediaKind::Movie,
                page: 1,
                sort: CatalogSort::Trending,
                genre: None,
                keywords: None,
            })
            .await
            .expect_err("request should fail");
        assert!(error.to_string().contains("all catalog endpoints failed"));
    }

    #[test]
    fn normalizes_popcorn_rating_year_and_language_torrents() {
        let payload = serde_json::json!([{
            "imdb_id": "tt1234567",
            "title": "Compatible title",
            "year": "2026",
            "synopsis": "Fixture",
            "rating": {
                "percentage": 73,
                "votes": 100
            },
            "images": {
                "poster": "https://example.com/poster.jpg"
            },
            "torrents": {
                "en": {
                    "720p": {
                        "url": "magnet:?xt=urn:btih:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                        "seed": 12
                    },
                    "1080p": {
                        "url": "magnet:?xt=urn:btih:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                        "seed": 24
                    }
                }
            }
        }]);

        let items = normalize_catalog(payload, MediaKind::Movie).expect("catalog");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].year, Some(2026));
        assert_eq!(items[0].rating, Some(7.3));
        assert_eq!(
            items[0].poster_url.as_ref().map(Url::as_str),
            Some("https://example.com/poster.jpg")
        );
        assert_eq!(items[0].torrents.len(), 2);
        let qualities = items[0]
            .torrents
            .iter()
            .map(|torrent| torrent.quality.as_str())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(
            qualities,
            std::collections::HashSet::from(["720p", "1080p"])
        );
    }

    #[test]
    fn prefers_imdb_when_series_contains_multiple_identifiers() {
        let payload = serde_json::json!([{
            "_id": "tt2861424",
            "imdb_id": "tt2861424",
            "tvdb_id": "275274",
            "title": "Compatible series",
            "year": "2013",
            "rating": { "percentage": 86 }
        }]);

        let items = normalize_catalog(payload, MediaKind::Series).expect("catalog");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "tt2861424");
        assert_eq!(items[0].year, Some(2013));
        assert_eq!(items[0].rating, Some(8.6));
    }

    #[test]
    fn upgrades_known_tmdb_posters_to_https() {
        let payload = serde_json::json!([{
            "imdb_id": "tt1234567",
            "title": "Secure poster",
            "images": {
                "poster": "http://image.tmdb.org/t/p/w500/poster.jpg"
            }
        }]);

        let items = normalize_catalog(payload, MediaKind::Movie).expect("catalog");
        assert_eq!(
            items[0].poster_url.as_ref().map(Url::as_str),
            Some("https://image.tmdb.org/t/p/w500/poster.jpg")
        );
    }

    #[test]
    fn ignores_invalid_optional_images_without_rejecting_catalog_items() {
        let payload = serde_json::json!([
            {
                "imdb_id": "tt0000001",
                "title": "Empty image",
                "images": { "poster": "", "fanart": "   " }
            },
            {
                "imdb_id": "tt0000002",
                "title": "Relative image",
                "images": { "poster": "/poster.jpg", "fanart": 42 }
            },
            {
                "imdb_id": "tt0000003",
                "title": "Missing images",
                "images": null
            }
        ]);

        let items = normalize_catalog(payload, MediaKind::Movie).expect("catalog");
        assert_eq!(items.len(), 3);
        assert!(items.iter().all(|item| item.poster_url.is_none()));
        assert!(items.iter().all(|item| item.backdrop_url.is_none()));
    }

    #[test]
    fn builds_encoded_anime_page_query_without_collapsing_to_page_one() {
        let path = build_browse_path(&CatalogQuery {
            kind: MediaKind::Anime,
            page: 3,
            sort: CatalogSort::Rating,
            genre: Some("Sci-Fi & Fantasy".to_owned()),
            keywords: Some("one piece".to_owned()),
        })
        .expect("path");

        assert!(path.starts_with("shows/3?"));
        assert!(path.contains("anime=1"));
        assert!(path.contains("sort=rating"));
        assert!(path.contains("genre=Sci-Fi+%26+Fantasy"));
        assert!(path.contains("keywords=one+piece"));
    }

    #[test]
    fn normalizes_anime_slug_and_secure_backdrop() {
        let payload = serde_json::json!([{
            "tvdb_id": "81797",
            "title": "ワンピース",
            "slug": "one-piece",
            "images": {
                "poster": "http://image.tmdb.org/t/p/w500/poster.jpg",
                "fanart": "http://image.tmdb.org/t/p/w500/fanart.jpg"
            },
            "genres": null
        }]);

        let items = normalize_catalog(payload, MediaKind::Anime).expect("catalog");
        assert_eq!(items[0].title, "One Piece");
        assert!(items[0].genres.is_empty());
        assert_eq!(
            items[0].backdrop_url.as_ref().map(Url::as_str),
            Some("https://image.tmdb.org/t/p/w500/fanart.jpg")
        );
    }

    #[test]
    fn normalizes_episode_sources_and_removes_provider_duplicates() {
        let payload = serde_json::json!({
            "episodes": [{
                "season": 2,
                "episode": "3",
                "title": "The episode",
                "overview": "Episode synopsis",
                "torrents": {
                    "1080p": {
                        "url": "magnet:?xt=urn:btih:abc&amp;tr=udp://tracker",
                        "quality": "1080p",
                        "seeds": 42,
                        "size": 1_571_000_320,
                        "provider": "Provider",
                        "title": "Series.S02E03.1080p.mkv",
                        "file": "Series/S02E03.mkv"
                    },
                    "0": {
                        "url": "magnet:?xt=urn:btih:abc&amp;tr=udp://tracker",
                        "quality": "1080p",
                        "seeds": 42,
                        "provider": "Provider",
                        "file": "Series/S02E03.mkv"
                    }
                }
            }]
        });

        let episodes = normalize_episodes(payload).expect("episodes");
        assert_eq!(episodes.len(), 1);
        assert_eq!((episodes[0].season, episodes[0].episode), (2, 3));
        assert_eq!(episodes[0].torrents.len(), 1);
        assert_eq!(episodes[0].torrents[0].quality, "1080p");
        assert_eq!(episodes[0].torrents[0].size_bytes, Some(1_571_000_320));
        assert_eq!(
            episodes[0].torrents[0].source,
            "magnet:?xt=urn:btih:abc&tr=udp://tracker"
        );
        assert_eq!(
            episodes[0].torrents[0].file_path.as_deref(),
            Some("Series/S02E03.mkv")
        );
        assert_eq!(
            episodes[0].torrents[0].file_name.as_deref(),
            Some("Series/S02E03.mkv")
        );
    }

    #[test]
    fn normalizes_numeric_string_size_and_release_name() {
        let torrent = normalize_torrent(
            "720p",
            &serde_json::json!({
                "url": "magnet:?xt=urn:btih:abc",
                "size": "2147483648",
                "title": "Release.Name.2026.720p.mkv"
            }),
        )
        .expect("torrent");

        assert_eq!(torrent.size_bytes, Some(2_147_483_648));
        assert_eq!(
            torrent.file_name.as_deref(),
            Some("Release.Name.2026.720p.mkv")
        );
    }

    #[test]
    fn validates_show_identifiers_before_building_paths() {
        assert_eq!(
            build_episodes_path("tt11198330").expect("path"),
            "show/tt11198330?locale=en&contentLocale=en&showAll=1"
        );
        assert!(build_episodes_path("../settings").is_err());
        assert!(build_episodes_path("show/id").is_err());
    }
}
