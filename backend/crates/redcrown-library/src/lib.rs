//! Stores durable library state and imports compatible Popcorn Time profiles.
// Rust guideline compliant 2026-02-21

use std::backtrace::Backtrace;
use std::collections::{HashMap, HashSet};
use std::fmt::{Debug, Display, Formatter};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use directories::BaseDirs;
use rusqlite::{Connection, Transaction, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

const DATABASE_SCHEMA_VERSION: i64 = 2;
const MAX_SETTINGS_BYTES: u64 = 2 * 1024 * 1024;
const MAX_LIBRARY_BYTES: u64 = 64 * 1024 * 1024;
const MAX_METADATA_BYTES: u64 = 128 * 1024 * 1024;
const MAX_NEDB_LINE_BYTES: usize = 4 * 1024 * 1024;

/// Selects which compatible data categories are imported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "IPC selection mirrors four independent user-facing import checkboxes"
)]
pub struct PopcornImportSelection {
    /// Import compatible catalog API endpoints.
    pub api_urls: bool,
    /// Import favorite movies and series.
    pub favorites: bool,
    /// Import watched movies and episodes.
    pub watched: bool,
    /// Import a resumable position when identity is unambiguous.
    pub playback_progress: bool,
}

impl Default for PopcornImportSelection {
    fn default() -> Self {
        Self {
            api_urls: true,
            favorites: true,
            watched: true,
            playback_progress: true,
        }
    }
}

/// Summarizes one detected Popcorn Time profile without exposing its path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PopcornProfilePreview {
    /// Opaque identifier accepted by the import command.
    pub id: String,
    /// User-facing profile label.
    pub label: String,
    /// Popcorn Time version when recorded.
    pub version: Option<String>,
    /// Last profile modification time in Unix milliseconds.
    pub modified_at_ms: Option<u64>,
    /// Compatible ordered catalog endpoints.
    pub api_urls: Vec<String>,
    /// Number of favorite media records.
    pub favorite_count: usize,
    /// Number of watched movie records.
    pub watched_movie_count: usize,
    /// Number of watched episode records.
    pub watched_episode_count: usize,
    /// Whether a safe resumable playback record is available.
    pub has_playback_progress: bool,
    /// Import limitations relevant to this profile.
    pub notes: Vec<String>,
}

/// Describes one durable library item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LibraryItem {
    /// Stable provider identifier, usually `IMDb`.
    pub external_id: String,
    /// Media category.
    pub kind: LibraryMediaKind,
    /// Display title when cached by the source profile.
    pub title: Option<String>,
    /// Release year when cached by the source profile.
    pub year: Option<u16>,
    /// Poster URL when cached by the source profile.
    pub poster_url: Option<String>,
}

/// Identifies a durable library category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryMediaKind {
    /// Feature-length movie.
    Movie,
    /// Episodic series.
    Series,
}

impl LibraryMediaKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Movie => "movie",
            Self::Series => "series",
        }
    }
}

/// Summarizes durable favorites and watched state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LibrarySummary {
    /// Total favorite movies and series.
    pub favorite_count: u64,
    /// Total watched movies.
    pub watched_movie_count: u64,
    /// Total watched episodes.
    pub watched_episode_count: u64,
    /// Favorite records available for immediate display.
    pub favorites: Vec<LibraryItem>,
    /// Watched movie records used to suppress completed titles from discovery.
    pub watched_movies: Vec<LibraryItem>,
}

/// Reports the durable part of a completed import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LibraryImportReport {
    /// Favorite records accepted by the transaction.
    pub favorites_imported: usize,
    /// Watched movie records accepted by the transaction.
    pub watched_movies_imported: usize,
    /// Watched episode records accepted by the transaction.
    pub watched_episodes_imported: usize,
    /// Playback-progress records accepted by the transaction.
    pub playback_progress_imported: usize,
    /// Records skipped because required identity was invalid or ambiguous.
    pub skipped_records: usize,
    /// Durable state after the import transaction.
    pub library: LibrarySummary,
}

/// Owns the versioned `SQLite` library database.
pub struct Library {
    connection: Mutex<Connection>,
}

impl Debug for Library {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Library")
            .field("connection", &"<redacted>")
            .finish()
    }
}

impl Library {
    /// Opens or creates the library database at `path`.
    ///
    /// # Errors
    ///
    /// Returns an error when the directory, database, pragmas, or migration fails.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, LibraryError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| LibraryError::from_io(&error))?;
        }
        let connection = Connection::open(path).map_err(|error| LibraryError::from_sql(&error))?;
        connection
            .execute_batch(
                "PRAGMA foreign_keys = ON;
                 PRAGMA journal_mode = WAL;
                 PRAGMA synchronous = FULL;
                 PRAGMA busy_timeout = 5000;",
            )
            .map_err(|error| LibraryError::from_sql(&error))?;
        migrate(&connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    /// Returns current favorites and watched counts.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be locked or queried.
    pub fn summary(&self) -> Result<LibrarySummary, LibraryError> {
        let connection = self.connection.lock().map_err(LibraryError::poisoned)?;
        query_summary(&connection)
    }

    /// Imports selected Popcorn Time data in one `SQLite` transaction.
    ///
    /// Repeating the same import is safe: stable keys are upserted and flags only
    /// move from false to true.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be locked or committed.
    pub fn import_popcorn(
        &self,
        data: &PopcornImportData,
        selection: PopcornImportSelection,
    ) -> Result<LibraryImportReport, LibraryError> {
        let mut connection = self.connection.lock().map_err(LibraryError::poisoned)?;
        let transaction = connection
            .transaction()
            .map_err(|error| LibraryError::from_sql(&error))?;
        let mut report = LibraryImportReport {
            favorites_imported: 0,
            watched_movies_imported: 0,
            watched_episodes_imported: 0,
            playback_progress_imported: 0,
            skipped_records: data.skipped_records,
            library: empty_summary(),
        };

        if selection.favorites {
            for item in &data.favorites {
                upsert_media(&transaction, item, true, false, None)?;
                report.favorites_imported += 1;
            }
        }
        if selection.watched {
            for item in &data.watched_movies {
                upsert_media(&transaction, item, false, true, item.watched_at_ms)?;
                report.watched_movies_imported += 1;
            }
            for episode in &data.watched_episodes {
                upsert_episode(&transaction, episode)?;
                report.watched_episodes_imported += 1;
            }
        }
        if selection.playback_progress
            && let Some(progress) = &data.playback_progress
        {
            report.playback_progress_imported = upsert_progress(&transaction, progress)?;
        }

        transaction
            .commit()
            .map_err(|error| LibraryError::from_sql(&error))?;
        report.library = query_summary(&connection)?;
        Ok(report)
    }
}

/// Holds a read-only snapshot parsed from a detected Popcorn Time profile.
#[derive(Debug, Clone)]
pub struct PopcornImportData {
    /// Opaque source profile identifier.
    pub profile_id: String,
    /// Compatible catalog API endpoint strings.
    pub api_urls: Vec<String>,
    favorites: Vec<ImportedMedia>,
    watched_movies: Vec<ImportedMedia>,
    watched_episodes: Vec<ImportedEpisode>,
    playback_progress: Option<ImportedProgress>,
    skipped_records: usize,
}

/// Discovers supported Popcorn Time profiles from bounded known locations.
///
/// Full paths remain backend-only and are never included in previews.
///
/// # Errors
///
/// Returns an error when a detected profile cannot be safely parsed.
pub fn discover_popcorn_profiles() -> Result<Vec<PopcornProfile>, LibraryError> {
    let Some(base) = BaseDirs::new() else {
        return Ok(Vec::new());
    };
    let local = base.data_local_dir();
    let candidates = [
        (
            "popcorn-time-nw-default",
            "Popcorn Time",
            local.join("Popcorn-Time/User Data/Default/data"),
        ),
        (
            "popcorn-time-nw-data",
            "Popcorn Time",
            local.join("Popcorn-Time/data"),
        ),
        (
            "popcorn-time-community",
            "Popcorn Time Community",
            local.join("Popcorn Time/data"),
        ),
    ];

    let mut profiles = Vec::new();
    for (id, label, data_path) in candidates {
        if is_popcorn_data_path(&data_path) {
            profiles.push(parse_profile(id, label, &data_path)?);
        }
    }
    profiles.sort_by_key(|profile| std::cmp::Reverse(profile.preview.modified_at_ms));
    Ok(profiles)
}

/// Represents one detected, read-only Popcorn Time profile.
pub struct PopcornProfile {
    preview: PopcornProfilePreview,
    data: PopcornImportData,
}

impl Debug for PopcornProfile {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PopcornProfile")
            .field("preview", &self.preview)
            .field("data", &"<redacted source data>")
            .finish()
    }
}

impl PopcornProfile {
    /// Returns the renderer-safe profile preview.
    #[must_use]
    pub const fn preview(&self) -> &PopcornProfilePreview {
        &self.preview
    }

    /// Returns parsed import data without re-reading the source profile.
    #[must_use]
    pub const fn data(&self) -> &PopcornImportData {
        &self.data
    }
}

#[derive(Debug, Clone)]
struct ImportedMedia {
    key: String,
    external_id: String,
    kind: LibraryMediaKind,
    title: Option<String>,
    year: Option<u16>,
    poster_url: Option<String>,
    watched_at_ms: Option<i64>,
}

#[derive(Debug, Clone)]
struct ImportedEpisode {
    key: String,
    tvdb_id: String,
    imdb_id: Option<String>,
    season: u32,
    episode: u32,
    watched_at_ms: Option<i64>,
}

#[derive(Debug, Clone)]
struct ImportedProgress {
    key: String,
    position_seconds: i64,
}

fn migrate(connection: &Connection) -> Result<(), LibraryError> {
    connection
        .execute_batch(
            "BEGIN IMMEDIATE;
             CREATE TABLE IF NOT EXISTS schema_migrations (
                 version INTEGER PRIMARY KEY NOT NULL,
                 applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );
             CREATE TABLE IF NOT EXISTS media_state (
                 state_key TEXT PRIMARY KEY NOT NULL,
                 external_id TEXT NOT NULL,
                 media_kind TEXT NOT NULL CHECK (media_kind IN ('movie', 'series', 'episode')),
                 parent_external_id TEXT,
                 season INTEGER,
                 episode INTEGER,
                 title TEXT,
                 year INTEGER,
                 poster_url TEXT,
                 favorite INTEGER NOT NULL DEFAULT 0 CHECK (favorite IN (0, 1)),
                 watched INTEGER NOT NULL DEFAULT 0 CHECK (watched IN (0, 1)),
                 watched_at_ms INTEGER,
                 progress_seconds INTEGER,
                 imported_from TEXT NOT NULL,
                 updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );
             CREATE INDEX IF NOT EXISTS media_state_favorite_idx
                 ON media_state(favorite, media_kind);
             CREATE INDEX IF NOT EXISTS media_state_watched_idx
                 ON media_state(watched, media_kind);
             INSERT OR IGNORE INTO schema_migrations(version) VALUES (1);
             UPDATE media_state
             SET poster_url = REPLACE(
                 poster_url,
                 'http://image.tmdb.org/',
                 'https://image.tmdb.org/'
             )
             WHERE poster_url LIKE 'http://image.tmdb.org/%';
             INSERT OR IGNORE INTO schema_migrations(version) VALUES (2);
             COMMIT;",
        )
        .map_err(|error| LibraryError::from_sql(&error))?;
    let version: i64 = connection
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .map_err(|error| LibraryError::from_sql(&error))?;
    if version != DATABASE_SCHEMA_VERSION {
        return Err(LibraryError::new(format!(
            "unsupported library schema version {version}"
        )));
    }
    Ok(())
}

fn upsert_media(
    transaction: &Transaction<'_>,
    item: &ImportedMedia,
    favorite: bool,
    watched: bool,
    watched_at_ms: Option<i64>,
) -> Result<(), LibraryError> {
    transaction
        .execute(
            "INSERT INTO media_state (
                 state_key, external_id, media_kind, title, year, poster_url,
                 favorite, watched, watched_at_ms, imported_from
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'popcorn_time')
             ON CONFLICT(state_key) DO UPDATE SET
                 title = COALESCE(excluded.title, media_state.title),
                 year = COALESCE(excluded.year, media_state.year),
                 poster_url = COALESCE(excluded.poster_url, media_state.poster_url),
                 favorite = MAX(media_state.favorite, excluded.favorite),
                 watched = MAX(media_state.watched, excluded.watched),
                 watched_at_ms = CASE
                     WHEN excluded.watched_at_ms IS NULL THEN media_state.watched_at_ms
                     WHEN media_state.watched_at_ms IS NULL THEN excluded.watched_at_ms
                     ELSE MAX(media_state.watched_at_ms, excluded.watched_at_ms)
                 END,
                 updated_at = CURRENT_TIMESTAMP",
            params![
                item.key,
                item.external_id,
                item.kind.as_str(),
                item.title,
                item.year,
                item.poster_url,
                favorite,
                watched,
                watched_at_ms,
            ],
        )
        .map_err(|error| LibraryError::from_sql(&error))?;
    Ok(())
}

fn upsert_episode(
    transaction: &Transaction<'_>,
    item: &ImportedEpisode,
) -> Result<(), LibraryError> {
    transaction
        .execute(
            "INSERT INTO media_state (
                 state_key, external_id, media_kind, parent_external_id, season,
                 episode, watched, watched_at_ms, imported_from
             ) VALUES (?1, ?2, 'episode', ?3, ?4, ?5, 1, ?6, 'popcorn_time')
             ON CONFLICT(state_key) DO UPDATE SET
                 parent_external_id = COALESCE(
                     excluded.parent_external_id,
                     media_state.parent_external_id
                 ),
                 watched = 1,
                 watched_at_ms = CASE
                     WHEN excluded.watched_at_ms IS NULL THEN media_state.watched_at_ms
                     WHEN media_state.watched_at_ms IS NULL THEN excluded.watched_at_ms
                     ELSE MAX(media_state.watched_at_ms, excluded.watched_at_ms)
                 END,
                 updated_at = CURRENT_TIMESTAMP",
            params![
                item.key,
                item.tvdb_id,
                item.imdb_id,
                item.season,
                item.episode,
                item.watched_at_ms,
            ],
        )
        .map_err(|error| LibraryError::from_sql(&error))?;
    Ok(())
}

fn upsert_progress(
    transaction: &Transaction<'_>,
    progress: &ImportedProgress,
) -> Result<usize, LibraryError> {
    transaction
        .execute(
            "UPDATE media_state
             SET progress_seconds = ?2, updated_at = CURRENT_TIMESTAMP
             WHERE state_key = ?1",
            params![progress.key, progress.position_seconds],
        )
        .map_err(|error| LibraryError::from_sql(&error))
}

fn query_summary(connection: &Connection) -> Result<LibrarySummary, LibraryError> {
    let favorite_count = count_where(connection, "favorite = 1 AND media_kind != 'episode'")?;
    let watched_movie_count = count_where(connection, "watched = 1 AND media_kind = 'movie'")?;
    let watched_episode_count = count_where(connection, "watched = 1 AND media_kind = 'episode'")?;
    let favorites = query_media_items(connection, "favorite = 1 AND media_kind != 'episode'")?;
    let watched_movies = query_media_items(connection, "watched = 1 AND media_kind = 'movie'")?;
    Ok(LibrarySummary {
        favorite_count,
        watched_movie_count,
        watched_episode_count,
        favorites,
        watched_movies,
    })
}

fn query_media_items(
    connection: &Connection,
    condition: &str,
) -> Result<Vec<LibraryItem>, LibraryError> {
    let sql = format!(
        "SELECT external_id, media_kind, title, year, poster_url
         FROM media_state
         WHERE {condition}
         ORDER BY COALESCE(title, external_id) COLLATE NOCASE"
    );
    let mut statement = connection
        .prepare(&sql)
        .map_err(|error| LibraryError::from_sql(&error))?;
    let rows = statement
        .query_map([], |row| {
            let kind: String = row.get(1)?;
            Ok(LibraryItem {
                external_id: row.get(0)?,
                kind: if kind == "movie" {
                    LibraryMediaKind::Movie
                } else {
                    LibraryMediaKind::Series
                },
                title: row.get(2)?,
                year: row.get(3)?,
                poster_url: row.get(4)?,
            })
        })
        .map_err(|error| LibraryError::from_sql(&error))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| LibraryError::from_sql(&error))
}

fn count_where(connection: &Connection, condition: &str) -> Result<u64, LibraryError> {
    let sql = format!("SELECT COUNT(*) FROM media_state WHERE {condition}");
    let count: i64 = connection
        .query_row(&sql, [], |row| row.get(0))
        .map_err(|error| LibraryError::from_sql(&error))?;
    u64::try_from(count).map_err(|_| LibraryError::new("library count was negative"))
}

const fn empty_summary() -> LibrarySummary {
    LibrarySummary {
        favorite_count: 0,
        watched_movie_count: 0,
        watched_episode_count: 0,
        favorites: Vec::new(),
        watched_movies: Vec::new(),
    }
}

fn is_popcorn_data_path(path: &Path) -> bool {
    path.join("settings.db").is_file()
        && (path.join("bookmarks.db").is_file() || path.join("watched.db").is_file())
}

fn parse_profile(id: &str, label: &str, data_path: &Path) -> Result<PopcornProfile, LibraryError> {
    let settings = read_settings(&data_path.join("settings.db"))?;
    let metadata = read_media_metadata(data_path)?;
    let (favorites, favorite_skipped) = read_favorites(&data_path.join("bookmarks.db"), &metadata)?;
    let (watched_movies, watched_episodes, watched_skipped) =
        read_watched(&data_path.join("watched.db"))?;
    let api_urls = extract_api_urls(&settings);
    let playback_progress = extract_progress(&settings, &favorites, &watched_movies);
    let version = settings
        .get("version")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            settings
                .get("dhtInfo")
                .and_then(|value| value.get("v"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        });
    let modified_at_ms = newest_modified_at(data_path);
    let mut notes = Vec::new();
    if playback_progress.is_none()
        && settings
            .get("lastWatchedTitle")
            .and_then(Value::as_str)
            .is_some()
    {
        notes.push(
            "The last playback title has no usable position or cannot be matched safely."
                .to_owned(),
        );
    }
    if favorite_skipped + watched_skipped > 0 {
        notes.push(format!(
            "{} malformed records will be skipped.",
            favorite_skipped + watched_skipped
        ));
    }
    let preview = PopcornProfilePreview {
        id: id.to_owned(),
        label: label.to_owned(),
        version,
        modified_at_ms,
        api_urls: api_urls.clone(),
        favorite_count: favorites.len(),
        watched_movie_count: watched_movies.len(),
        watched_episode_count: watched_episodes.len(),
        has_playback_progress: playback_progress.is_some(),
        notes,
    };
    Ok(PopcornProfile {
        data: PopcornImportData {
            profile_id: id.to_owned(),
            api_urls,
            favorites,
            watched_movies,
            watched_episodes,
            playback_progress,
            skipped_records: favorite_skipped + watched_skipped,
        },
        preview,
    })
}

fn read_settings(path: &Path) -> Result<HashMap<String, Value>, LibraryError> {
    let rows = read_nedb(path, MAX_SETTINGS_BYTES)?;
    let mut settings = HashMap::new();
    for row in rows {
        if let Some(key) = row.get("key").and_then(Value::as_str)
            && let Some(value) = row.get("value")
        {
            settings.insert(key.to_owned(), value.clone());
        }
    }
    Ok(settings)
}

fn read_media_metadata(
    data_path: &Path,
) -> Result<HashMap<String, ImportedMetadata>, LibraryError> {
    let mut metadata = HashMap::new();
    for file in ["movies.db", "shows.db"] {
        for row in read_nedb(&data_path.join(file), MAX_METADATA_BYTES)? {
            let Some(imdb_id) = row.get("imdb_id").and_then(value_string) else {
                continue;
            };
            metadata.insert(
                imdb_id,
                ImportedMetadata {
                    title: row.get("title").and_then(value_string),
                    year: row.get("year").and_then(value_u16),
                    poster_url: row
                        .get("images")
                        .and_then(|images| images.get("poster"))
                        .and_then(value_string)
                        .and_then(|value| normalize_poster_url(&value)),
                },
            );
        }
    }
    Ok(metadata)
}

#[derive(Debug, Clone, Default)]
struct ImportedMetadata {
    title: Option<String>,
    year: Option<u16>,
    poster_url: Option<String>,
}

fn read_favorites(
    path: &Path,
    metadata: &HashMap<String, ImportedMetadata>,
) -> Result<(Vec<ImportedMedia>, usize), LibraryError> {
    let mut favorites = HashMap::new();
    let mut skipped = 0;
    for row in read_nedb(path, MAX_LIBRARY_BYTES)? {
        let Some(external_id) = row.get("imdb_id").and_then(value_string) else {
            skipped += 1;
            continue;
        };
        if !valid_external_id(&external_id) {
            skipped += 1;
            continue;
        }
        let kind = match row.get("type").and_then(Value::as_str) {
            Some("movie") => LibraryMediaKind::Movie,
            Some("show" | "series" | "anime") => LibraryMediaKind::Series,
            _ => {
                skipped += 1;
                continue;
            }
        };
        let cached = metadata.get(&external_id).cloned().unwrap_or_default();
        favorites.insert(
            external_id.clone(),
            ImportedMedia {
                key: format!("imdb:{external_id}"),
                external_id,
                kind,
                title: cached.title,
                year: cached.year,
                poster_url: cached.poster_url,
                watched_at_ms: None,
            },
        );
    }
    Ok((favorites.into_values().collect(), skipped))
}

fn read_watched(
    path: &Path,
) -> Result<(Vec<ImportedMedia>, Vec<ImportedEpisode>, usize), LibraryError> {
    let mut movies: HashMap<String, ImportedMedia> = HashMap::new();
    let mut episodes: HashMap<String, ImportedEpisode> = HashMap::new();
    let mut skipped = 0;
    for row in read_nedb(path, MAX_LIBRARY_BYTES)? {
        match row.get("type").and_then(Value::as_str) {
            Some("movie") => {
                let Some(external_id) = row.get("movie_id").and_then(value_string) else {
                    skipped += 1;
                    continue;
                };
                if !valid_external_id(&external_id) {
                    skipped += 1;
                    continue;
                }
                let watched_at_ms = nedb_date_ms(row.get("date"));
                let entry = movies
                    .entry(external_id.clone())
                    .or_insert_with(|| ImportedMedia {
                        key: format!("imdb:{external_id}"),
                        external_id,
                        kind: LibraryMediaKind::Movie,
                        title: None,
                        year: None,
                        poster_url: None,
                        watched_at_ms,
                    });
                entry.watched_at_ms = newest_timestamp(entry.watched_at_ms, watched_at_ms);
            }
            Some("episode") => {
                let Some(tvdb_id) = row.get("tvdb_id").and_then(value_string) else {
                    skipped += 1;
                    continue;
                };
                let Some(season) = row.get("season").and_then(value_u32) else {
                    skipped += 1;
                    continue;
                };
                let Some(episode) = row.get("episode").and_then(value_u32) else {
                    skipped += 1;
                    continue;
                };
                if tvdb_id.is_empty() {
                    skipped += 1;
                    continue;
                }
                let key = format!("tvdb:{tvdb_id}:s{season}:e{episode}");
                let watched_at_ms = nedb_date_ms(row.get("date"));
                let entry = episodes
                    .entry(key.clone())
                    .or_insert_with(|| ImportedEpisode {
                        key,
                        tvdb_id,
                        imdb_id: row.get("imdb_id").and_then(value_string),
                        season,
                        episode,
                        watched_at_ms,
                    });
                entry.watched_at_ms = newest_timestamp(entry.watched_at_ms, watched_at_ms);
            }
            _ => {
                skipped += 1;
            }
        }
    }
    Ok((
        movies.into_values().collect(),
        episodes.into_values().collect(),
        skipped,
    ))
}

fn extract_api_urls(settings: &HashMap<String, Value>) -> Vec<String> {
    let mut candidates = Vec::new();
    for key in [
        "customMoviesServer",
        "customSeriesServer",
        "customAnimeServer",
    ] {
        if let Some(value) = settings.get(key).and_then(Value::as_str) {
            candidates.extend(value.split(',').map(str::trim).map(str::to_owned));
        }
    }
    if let Some(value) = settings
        .get("dhtInfo")
        .and_then(|info| info.get("server"))
        .and_then(Value::as_str)
    {
        candidates.extend(value.split(',').map(str::trim).map(str::to_owned));
    }
    if let Some(value) = settings.get("dhtData").and_then(Value::as_str)
        && let Ok(data) = serde_json::from_str::<Value>(value)
        && let Some(servers) = data.get("server").and_then(Value::as_str)
    {
        candidates.extend(servers.split(',').map(str::trim).map(str::to_owned));
    }

    let mut seen = HashSet::new();
    candidates
        .into_iter()
        .filter_map(|value| normalize_api_url(&value))
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

fn normalize_api_url(value: &str) -> Option<String> {
    let mut url = Url::parse(value).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    url.set_fragment(None);
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    Some(url.to_string())
}

fn extract_progress(
    settings: &HashMap<String, Value>,
    favorites: &[ImportedMedia],
    watched_movies: &[ImportedMedia],
) -> Option<ImportedProgress> {
    let position_seconds = settings
        .get("lastWatchedTime")
        .and_then(value_u64)
        .and_then(|value| i64::try_from(value).ok())?;
    if position_seconds == 0 {
        return None;
    }
    let title = settings.get("lastWatchedTitle")?.as_str()?.trim();
    let mut matches = favorites.iter().chain(watched_movies).filter(|item| {
        item.title
            .as_deref()
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(title))
    });
    let item = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    Some(ImportedProgress {
        key: item.key.clone(),
        position_seconds,
    })
}

fn read_nedb(path: &Path, max_bytes: u64) -> Result<Vec<Value>, LibraryError> {
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let length = fs::metadata(path)
        .map_err(|error| LibraryError::from_io(&error))?
        .len();
    if length > max_bytes {
        return Err(LibraryError::new(format!(
            "Popcorn Time data file exceeds the supported {} MiB limit",
            max_bytes / (1024 * 1024)
        )));
    }
    let reader = BufReader::new(File::open(path).map_err(|error| LibraryError::from_io(&error))?);
    let mut by_id: HashMap<String, (usize, Value)> = HashMap::new();
    for (sequence, line) in reader.lines().enumerate() {
        let line = line.map_err(|error| LibraryError::from_io(&error))?;
        if line.len() > MAX_NEDB_LINE_BYTES {
            return Err(LibraryError::new(
                "Popcorn Time contains an oversized database record",
            ));
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if value.get("$$indexCreated").is_some() || value.get("$$indexRemoved").is_some() {
            continue;
        }
        let Some(id) = value.get("_id").and_then(Value::as_str) else {
            continue;
        };
        if value
            .get("$$deleted")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            by_id.remove(id);
        } else {
            by_id.insert(id.to_owned(), (sequence, value));
        }
    }
    let mut values = by_id.into_values().collect::<Vec<_>>();
    values.sort_by_key(|(sequence, _)| *sequence);
    Ok(values.into_iter().map(|(_, value)| value).collect())
}

fn newest_modified_at(data_path: &Path) -> Option<u64> {
    ["settings.db", "bookmarks.db", "watched.db"]
        .into_iter()
        .filter_map(|name| fs::metadata(data_path.join(name)).ok())
        .filter_map(|metadata| metadata.modified().ok())
        .filter_map(|time| time.duration_since(UNIX_EPOCH).ok())
        .filter_map(|duration| u64::try_from(duration.as_millis()).ok())
        .max()
}

fn value_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) if !value.trim().is_empty() => Some(value.trim().to_owned()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn value_u16(value: &Value) -> Option<u16> {
    value_u64(value).and_then(|value| u16::try_from(value).ok())
}

fn value_u32(value: &Value) -> Option<u32> {
    value_u64(value).and_then(|value| u32::try_from(value).ok())
}

fn value_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Number(value) => value.as_u64(),
        Value::String(value) => value.parse().ok(),
        _ => None,
    }
}

fn nedb_date_ms(value: Option<&Value>) -> Option<i64> {
    value?
        .get("$$date")
        .and_then(Value::as_i64)
        .or_else(|| value?.as_i64())
}

fn newest_timestamp(left: Option<i64>, right: Option<i64>) -> Option<i64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn valid_external_id(value: &str) -> bool {
    let length = value.len();
    (3..=64).contains(&length)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':'))
}

fn normalize_poster_url(value: &str) -> Option<String> {
    let mut url = Url::parse(value).ok()?;
    if url.scheme() == "http" && url.host_str() == Some("image.tmdb.org") {
        url.set_scheme("https").ok()?;
    }
    Some(url.to_string())
}

/// Reports a library database or profile-import failure.
#[derive(Debug)]
pub struct LibraryError {
    message: String,
    backtrace: Backtrace,
}

impl LibraryError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            backtrace: Backtrace::capture(),
        }
    }

    fn from_io(error: &std::io::Error) -> Self {
        Self::new(format!("failed to read Popcorn Time data: {error}"))
    }

    fn from_sql(error: &rusqlite::Error) -> Self {
        Self::new(format!("library database operation failed: {error}"))
    }

    fn poisoned<T>(_error: std::sync::PoisonError<T>) -> Self {
        Self::new("library database lock was poisoned")
    }

    /// Creates an error for a profile that disappeared after preview.
    #[must_use]
    pub fn profile_unavailable() -> Self {
        Self::new("The selected Popcorn Time profile is no longer available")
    }

    /// Returns a bounded message safe for the desktop renderer.
    #[must_use]
    pub fn user_message(&self) -> &str {
        &self.message
    }
}

impl Display for LibraryError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}\n{}", self.message, self.backtrace)
    }
}

impl std::error::Error for LibraryError {}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{Library, PopcornImportSelection, parse_profile, read_nedb};

    #[test]
    fn nedb_replays_updates_and_tombstones() {
        let directory = tempdir().expect("directory");
        let path = directory.path().join("records.db");
        fs::write(
            &path,
            concat!(
                "{\"_id\":\"one\",\"value\":1}\n",
                "{\"_id\":\"one\",\"value\":2}\n",
                "{\"_id\":\"two\",\"value\":3}\n",
                "{\"_id\":\"two\",\"$$deleted\":true}\n"
            ),
        )
        .expect("fixture");
        let rows = read_nedb(&path, 1024).expect("records");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["value"], 2);
    }

    #[test]
    fn repeated_import_is_idempotent() {
        let directory = tempdir().expect("directory");
        let profile = directory.path().join("profile");
        fs::create_dir_all(&profile).expect("profile");
        fs::write(
            profile.join("settings.db"),
            concat!(
                "{\"_id\":\"s1\",\"key\":\"version\",\"value\":\"0.5.1\"}\n",
                "{\"_id\":\"s2\",\"key\":\"dhtInfo\",\"value\":{\"server\":\"https://one.example,https://two.example\"}}\n"
            ),
        )
        .expect("settings");
        fs::write(
            profile.join("bookmarks.db"),
            "{\"_id\":\"b1\",\"imdb_id\":\"tt123\",\"type\":\"movie\"}\n",
        )
        .expect("bookmarks");
        fs::write(
            profile.join("watched.db"),
            concat!(
                "{\"_id\":\"w1\",\"movie_id\":\"tt123\",\"type\":\"movie\",\"date\":{\"$$date\":1000}}\n",
                "{\"_id\":\"w2\",\"tvdb_id\":\"42\",\"imdb_id\":\"tt456\",\"season\":\"1\",\"episode\":\"2\",\"type\":\"episode\"}\n"
            ),
        )
        .expect("watched");
        fs::write(profile.join("movies.db"), "").expect("movies");
        fs::write(profile.join("shows.db"), "").expect("shows");

        let parsed = parse_profile("test", "Test", &profile).expect("profile");
        assert_eq!(parsed.preview().api_urls.len(), 2);
        let library = Library::open(directory.path().join("library.sqlite3")).expect("library");
        library
            .import_popcorn(parsed.data(), PopcornImportSelection::default())
            .expect("first import");
        let second = library
            .import_popcorn(parsed.data(), PopcornImportSelection::default())
            .expect("second import");
        assert_eq!(second.library.favorite_count, 1);
        assert_eq!(second.library.watched_movie_count, 1);
        assert_eq!(second.library.watched_episode_count, 1);
        assert_eq!(second.library.watched_movies.len(), 1);
        assert_eq!(second.library.watched_movies[0].external_id, "tt123");
    }

    #[test]
    fn migration_upgrades_existing_tmdb_posters() {
        let directory = tempdir().expect("directory");
        let path = directory.path().join("library.sqlite3");
        let connection = rusqlite::Connection::open(&path).expect("database");
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                     version INTEGER PRIMARY KEY NOT NULL,
                     applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                 );
                 CREATE TABLE media_state (
                     state_key TEXT PRIMARY KEY NOT NULL,
                     external_id TEXT NOT NULL,
                     media_kind TEXT NOT NULL,
                     parent_external_id TEXT,
                     season INTEGER,
                     episode INTEGER,
                     title TEXT,
                     year INTEGER,
                     poster_url TEXT,
                     favorite INTEGER NOT NULL DEFAULT 0,
                     watched INTEGER NOT NULL DEFAULT 0,
                     watched_at_ms INTEGER,
                     progress_seconds INTEGER,
                     imported_from TEXT NOT NULL,
                     updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                 );
                 INSERT INTO schema_migrations(version) VALUES (1);
                 INSERT INTO media_state (
                     state_key, external_id, media_kind, title, poster_url,
                     favorite, imported_from
                 ) VALUES (
                     'imdb:tt123', 'tt123', 'series', 'Title',
                     'http://image.tmdb.org/t/p/w500/poster.jpg', 1, 'popcorn_time'
                 );",
            )
            .expect("legacy schema");
        drop(connection);

        let library = Library::open(&path).expect("migrated library");
        let summary = library.summary().expect("summary");
        assert_eq!(
            summary.favorites[0].poster_url.as_deref(),
            Some("https://image.tmdb.org/t/p/w500/poster.jpg")
        );
    }
}
