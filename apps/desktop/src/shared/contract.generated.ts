// RedCrown protocol v1 projection. Rust-driven generation is the next contract gate.

export type MediaKind = "movie" | "series" | "anime";
export type CatalogSort =
  | "trending"
  | "popularity"
  | "updated"
  | "last_added"
  | "year"
  | "title"
  | "rating";
export type ThemePreference = "system" | "light" | "dark";

export interface SourceEndpoint {
  id: string;
  url: string;
  enabled: boolean;
}

export interface SourceConfig {
  id: string;
  name: string;
  enabled: boolean;
  endpoints: SourceEndpoint[];
}

export interface StreamCachePolicy {
  idle_expiration_secs: number;
  maximum_age_secs: number;
  size_budget_bytes: number;
}

export type TrackerListSource =
  | { kind: "url"; url: string }
  | { kind: "file"; path: string };

export interface TrackerListConfig {
  enabled: boolean;
  source: TrackerListSource;
}

export interface AppSettings {
  schema_version: number;
  sources: SourceConfig[];
  stream_cache: StreamCachePolicy;
  tracker_list: TrackerListConfig;
  theme: ThemePreference;
  hide_watched_movies: boolean;
}

export interface TorrentOption {
  quality: string;
  source: string;
  size_bytes?: number;
  seeders?: number;
  provider?: string;
  file_path?: string;
  file_name?: string;
}

export interface MediaEpisode {
  season: number;
  episode: number;
  title: string;
  synopsis: string;
  torrents: TorrentOption[];
}

export interface MediaItem {
  id: string;
  title: string;
  year?: number;
  synopsis: string;
  poster_url?: string;
  backdrop_url?: string;
  rating?: number;
  kind: MediaKind;
  genres: string[];
  torrents: TorrentOption[];
}

export interface CatalogQuery {
  kind: MediaKind;
  page: number;
  sort: CatalogSort;
  genre?: string;
  keywords?: string;
}

export interface CatalogPage {
  items: MediaItem[];
  page: number;
  has_more: boolean;
}

export interface BootstrapState {
  protocol_version: number;
  settings: AppSettings;
  featured: MediaItem[];
  torrent_engine_ready: boolean;
}

export interface EndpointHealth {
  endpoint_id: string;
  reachable: boolean;
  status?: number;
  message: string;
  latency_ms: number;
}

export interface PlaybackTicket {
  torrent_id: number;
  file_id: number;
  file_name: string;
  file_length: number;
  stream_url: string;
  playback_url: string;
  duration_seconds?: number;
  audio_tracks: MediaTrack[];
  subtitle_tracks: MediaTrack[];
}

export interface MediaTrack {
  id: number;
  codec: string;
  language?: string;
  title?: string;
  channels?: number;
  is_default: boolean;
  is_forced: boolean;
  stream_url?: string;
}

export type PlaybackStage = "resolving_metadata" | "validating_cache" | "buffering" | "ready" | "failed";

export interface PlaybackStatus {
  preparation_id: string;
  stage: PlaybackStage;
  downloaded_bytes: number;
  total_bytes: number;
  download_mib_per_second: number;
  connected_peers: number;
  ticket?: PlaybackTicket;
  error?: string;
}

export interface PeerDiagnostics {
  queued: number;
  connecting: number;
  connected: number;
  seen: number;
  dead: number;
  not_needed: number;
  seeders?: number;
}

export interface PieceDiagnostics {
  available: number;
  downloaded_this_session: number;
  total: number;
  average_download_ms?: number;
}

export interface DhtDiagnostics {
  node_id: string;
  outstanding_requests: number;
  routing_table_size: number;
}

export interface TorrentDiagnostics {
  playback: PlaybackStatus;
  engine_state?: string;
  info_hash?: string;
  magnet_link?: string;
  trackers: string[];
  uploaded_bytes: number;
  downloaded_this_session_bytes: number;
  upload_mib_per_second: number;
  peers: PeerDiagnostics;
  pieces: PieceDiagnostics;
  dht?: DhtDiagnostics;
}

export type LibraryMediaKind = "movie" | "series";

export interface LibraryItem {
  external_id: string;
  kind: LibraryMediaKind;
  title?: string;
  year?: number;
  poster_url?: string;
}

export interface LibraryEpisode {
  season: number;
  episode: number;
}

export interface WatchedSeries {
  external_id: string;
  title?: string;
  year?: number;
  poster_url?: string;
  latest_season: number;
  latest_episode: number;
  watched_at_ms?: number;
  episodes: LibraryEpisode[];
}

export interface LibrarySummary {
  favorite_count: number;
  watched_movie_count: number;
  watched_episode_count: number;
  favorites: LibraryItem[];
  watched_movies: LibraryItem[];
  watched_series: WatchedSeries[];
  continue_watching_hidden: string[];
}

export interface PopcornProfilePreview {
  id: string;
  label: string;
  version?: string;
  modified_at_ms?: number;
  api_urls: string[];
  favorite_count: number;
  watched_movie_count: number;
  watched_episode_count: number;
  has_playback_progress: boolean;
  notes: string[];
}

export interface PopcornImportSelection {
  api_urls: boolean;
  favorites: boolean;
  watched: boolean;
  playback_progress: boolean;
}

export interface LibraryImportReport {
  favorites_imported: number;
  watched_movies_imported: number;
  watched_episodes_imported: number;
  playback_progress_imported: number;
  skipped_records: number;
  library: LibrarySummary;
}

export interface PopcornImportReport {
  api_urls_added: number;
  settings: AppSettings;
  library: LibraryImportReport;
}
