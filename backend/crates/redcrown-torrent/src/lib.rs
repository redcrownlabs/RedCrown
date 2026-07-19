//! Isolates the selected `BitTorrent` engine behind `RedCrown`-owned types.
// Rust guideline compliant 2026-02-21

use std::backtrace::Backtrace;
use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter};
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path as FsPath, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, RwLock as StdRwLock};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::header::{
    ACCEPT_RANGES, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, RANGE,
};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use librqbit::api::TorrentIdOrHash;
use librqbit::dht::DhtPersistenceConfig;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, Api, DhtSessionConfig, ListOnlyResponse,
    ListenerMode, ListenerOptions, PeerConnectionOptions, Session, SessionOptions,
    TorrentStatsState,
};
use redcrown_core::{StreamCachePolicy, TrackerListConfig};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncSeekExt, SeekFrom};
use tokio::net::TcpListener;
use tokio::process::Command;
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;
use tokio_util::io::ReaderStream;
use tracing::{Level, event};
use uuid::Uuid;

mod cache;
mod tracker_list;

use cache::{CacheKey, StreamCache};
#[doc(inline)]
pub use tracker_list::PreparedTrackerList;
use tracker_list::TrackerList;

const STREAM_READ_BUFFER_BYTES: usize = 64 * 1024;
/// DHT routing state is separate from expiring media-cache entries.
const DHT_STATE_FILENAME: &str = ".redcrown-dht.json";
/// Startup buffering balances prompt playback against resilience to peer jitter.
const MIN_START_BUFFER_BYTES: u64 = 8 * 1024 * 1024;
/// Large files must not require an excessive wait before playback can begin.
const MAX_START_BUFFER_BYTES: u64 = 32 * 1024 * 1024;
/// One percent adapts startup buffering to ordinary movie and episode sizes.
const START_BUFFER_DIVISOR: u64 = 100;
/// Five-minute maintenance bounds expiration delay and manifest write frequency.
const CACHE_MAINTENANCE_INTERVAL: Duration = Duration::from_secs(5 * 60);
/// The default public list updates daily, so more frequent polling adds little value.
const TRACKER_LIST_REFRESH_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
/// Metadata candidates must churn quickly through stale public peer lists.
const METADATA_CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
/// Metadata blocks are small; a silent peer should not occupy a slot for long.
const METADATA_READ_WRITE_TIMEOUT: Duration = Duration::from_secs(5);
/// Media inspection must fail instead of hanging playback indefinitely.
const MEDIA_PROBE_TIMEOUT: Duration = Duration::from_secs(30);
/// `FFmpeg` stderr is bounded before it enters structured diagnostics.
const MAX_MEDIA_ERROR_BYTES: u64 = 64 * 1024;
/// A one-mebibyte pipe decouples `FFmpeg` bursts from Chromium reads.
const MEDIA_PIPE_BYTES: usize = 1024 * 1024;
/// H.264 can be copied into fragmented MP4 without video quality loss.
const COMPATIBLE_VIDEO_CODEC: &str = "h264";
/// HEVC requires conversion because Chromium does not provide dependable playback support.
const TRANSCODED_VIDEO_CODEC: &str = "hevc";
/// H.264 needs additional bitrate to retain detail from a more efficient HEVC source.
const HEVC_TO_H264_BITRATE_NUMERATOR: u64 = 3;
const HEVC_TO_H264_BITRATE_DENOMINATOR: u64 = 2;
/// Low-resolution sources still need enough bitrate to avoid visible `OpenH264` artifacts.
const MIN_H264_TRANSCODE_BITRATE: u64 = 2_000_000;
/// This bound prevents one playback process from producing an excessive local stream.
const MAX_H264_TRANSCODE_BITRATE: u64 = 40_000_000;
/// A short GOP keeps fragmented playback responsive without excessive keyframes.
const H264_TRANSCODE_GOP_FRAMES: &str = "60";
/// PQ HDR is normalized and tone-mapped into Chromium-compatible BT.709 SDR.
const PQ_TO_SDR_FILTER: &str = "zscale=primariesin=bt2020:transferin=smpte2084:matrixin=bt2020nc:rangein=limited:transfer=linear:npl=100,format=gbrpf32le,tonemap=mobius:desat=0,zscale=primaries=bt709:transfer=bt709:matrix=bt709:range=limited,format=yuv420p";
/// HLG HDR uses the same output policy with its distinct source transfer function.
const HLG_TO_SDR_FILTER: &str = "zscale=primariesin=bt2020:transferin=arib-std-b67:matrixin=bt2020nc:rangein=limited:transfer=linear:npl=100,format=gbrpf32le,tonemap=mobius:desat=0,zscale=primaries=bt709:transfer=bt709:matrix=bt709:range=limited,format=yuv420p";

/// Identifies the pinned media executables used by playback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaTools {
    ffmpeg: PathBuf,
    ffprobe: PathBuf,
}

impl MediaTools {
    /// Loads verified media-tool paths supplied by the desktop launcher.
    ///
    /// # Errors
    ///
    /// Returns an error when either environment variable is missing or does
    /// not identify a regular file.
    pub fn from_environment() -> Result<Self, TorrentError> {
        let ffmpeg = required_tool_path("REDCROWN_FFMPEG_BIN")?;
        let ffprobe = required_tool_path("REDCROWN_FFPROBE_BIN")?;
        Ok(Self { ffmpeg, ffprobe })
    }

    #[cfg(test)]
    fn unavailable_for_transfer_test() -> Self {
        Self {
            ffmpeg: PathBuf::new(),
            ffprobe: PathBuf::new(),
        }
    }
}

/// Describes one file available in torrent metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TorrentFile {
    /// Zero-based engine file identifier.
    pub id: usize,
    /// Sanitized display path.
    pub name: String,
    /// File length in bytes.
    pub length: u64,
}

/// Grants access to one active playback stream.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlaybackTicket {
    /// Engine torrent identifier.
    pub torrent_id: usize,
    /// Selected media file identifier.
    pub file_id: usize,
    /// Selected media filename.
    pub file_name: String,
    /// Selected media file length in bytes.
    pub file_length: u64,
    /// Tokenized loopback stream URL.
    pub stream_url: String,
    /// Chromium-compatible media URL with selectable audio.
    pub playback_url: String,
    /// Media duration reported by `FFprobe`.
    pub duration_seconds: Option<f64>,
    /// Audio tracks embedded in the selected media file.
    pub audio_tracks: Vec<MediaTrack>,
    /// Subtitle tracks embedded in the selected media file.
    pub subtitle_tracks: Vec<MediaTrack>,
}

/// Describes an audio or subtitle stream discovered by `FFprobe`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MediaTrack {
    /// `FFmpeg`'s global stream index.
    pub id: usize,
    /// Codec name reported by `FFprobe`.
    pub codec: String,
    /// BCP-47-like or provider language tag when present.
    pub language: Option<String>,
    /// Human-readable container title when present.
    pub title: Option<String>,
    /// Channel count for audio streams.
    pub channels: Option<u16>,
    /// Container default disposition.
    pub is_default: bool,
    /// Container forced disposition.
    pub is_forced: bool,
    /// Tokenized `WebVTT` URL for subtitle streams.
    pub stream_url: Option<String>,
}

/// Describes the current playback preparation stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackStage {
    /// Resolving magnet metadata and selecting the requested media file.
    ResolvingMetadata,
    /// Validating an existing stream-cache entry before it can be trusted.
    ValidatingCache,
    /// Prioritizing and buffering the beginning of the selected file.
    Buffering,
    /// The loopback stream is ready for the media element.
    Ready,
    /// Preparation stopped because an error occurred.
    Failed,
}

/// Reports observable playback preparation and torrent transfer state.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlaybackStatus {
    /// Correlates polling and cancellation requests.
    pub preparation_id: Uuid,
    /// Current preparation stage.
    pub stage: PlaybackStage,
    /// Verified bytes available for the selected media file.
    pub downloaded_bytes: u64,
    /// Selected media file length when metadata is available.
    pub total_bytes: u64,
    /// Current binary mebibytes downloaded per second.
    pub download_mib_per_second: f64,
    /// Number of currently connected peers.
    pub connected_peers: u32,
    /// Stream ticket after metadata and file selection complete.
    pub ticket: Option<PlaybackTicket>,
    /// Bounded failure message for the renderer.
    pub error: Option<String>,
}

/// Summarizes peer discovery and connection state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct PeerDiagnostics {
    /// Peers waiting for connection attempts.
    pub queued: u32,
    /// Peers with an in-progress connection.
    pub connecting: u32,
    /// Peers with an active protocol connection.
    pub connected: u32,
    /// Unique peers observed by discovery mechanisms.
    pub seen: u32,
    /// Peers currently considered unreachable or invalid.
    pub dead: u32,
    /// Connected peers that do not have currently needed pieces.
    pub not_needed: u32,
    /// Connected seeders when the engine exposes remote completion state.
    pub seeders: Option<u32>,
}

/// Summarizes verified `BitTorrent` pieces.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct PieceDiagnostics {
    /// Pieces already verified and available from storage.
    pub available: u64,
    /// Pieces downloaded and hash-verified in this engine session.
    pub downloaded_this_session: u64,
    /// Total pieces described by torrent metadata.
    pub total: u32,
    /// Mean verified-piece download duration in milliseconds.
    pub average_download_ms: Option<u64>,
}

/// Summarizes distributed hash table activity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DhtDiagnostics {
    /// Local DHT node identifier.
    pub node_id: String,
    /// Requests currently awaiting a response.
    pub outstanding_requests: usize,
    /// Nodes in the local routing table.
    pub routing_table_size: usize,
}

/// Reports torrent internals for the diagnostics screen.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TorrentDiagnostics {
    /// Current playback preparation and transfer summary.
    pub playback: PlaybackStatus,
    /// librqbit state after the torrent is managed.
    pub engine_state: Option<String>,
    /// Version-one torrent info hash.
    pub info_hash: Option<String>,
    /// Original magnet URI when playback was prepared from one.
    pub magnet_link: Option<String>,
    /// Trackers configured by the torrent or magnet source.
    pub trackers: Vec<String>,
    /// Bytes uploaded to connected peers.
    pub uploaded_bytes: u64,
    /// Bytes downloaded and hash-verified in this engine session.
    pub downloaded_this_session_bytes: u64,
    /// Current binary mebibytes uploaded per second.
    pub upload_mib_per_second: f64,
    /// Peer discovery and connection counters.
    pub peers: PeerDiagnostics,
    /// Verified piece counters.
    pub pieces: PieceDiagnostics,
    /// DHT health when DHT is enabled.
    pub dht: Option<DhtDiagnostics>,
}

#[derive(Debug, Clone)]
struct PreparationSnapshot {
    stage: PlaybackStage,
    ticket: Option<PlaybackTicket>,
    error: Option<String>,
}

#[derive(Debug)]
struct PlaybackPreparation {
    id: Uuid,
    source: String,
    snapshot: StdRwLock<PreparationSnapshot>,
    task: Mutex<Option<JoinHandle<()>>>,
}

struct StreamState {
    api: Api,
    token: String,
    ffmpeg: PathBuf,
    manifests: Arc<RwLock<HashMap<(usize, usize), MediaManifest>>>,
}

#[derive(Debug, Clone)]
struct MediaManifest {
    native_url: String,
    duration_seconds: Option<f64>,
    video_bridge: VideoBridge,
    audio_track_ids: HashSet<usize>,
    subtitle_track_ids: HashSet<usize>,
    default_audio_track: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VideoBridge {
    CopyH264,
    TranscodeToH264 {
        bitrate: u64,
        hdr_transfer: Option<HdrTransfer>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HdrTransfer {
    Hlg,
    Pq,
}

/// Owns the torrent session and loopback stream endpoint.
pub struct TorrentEngine {
    api: Api,
    stream_address: SocketAddr,
    stream_token: String,
    stream_task: JoinHandle<()>,
    cleanup_task: JoinHandle<()>,
    tracker_refresh_task: JoinHandle<()>,
    cache: Arc<StreamCache>,
    tracker_list: Arc<TrackerList>,
    ffprobe: PathBuf,
    media_manifests: Arc<RwLock<HashMap<(usize, usize), MediaManifest>>>,
    active_torrents: Mutex<HashMap<usize, CacheKey>>,
    preparation: Mutex<Option<Arc<PlaybackPreparation>>>,
}

impl std::fmt::Debug for TorrentEngine {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TorrentEngine")
            .field("stream_address", &self.stream_address)
            .field("stream_task", &self.stream_task)
            .field("cleanup_task", &self.cleanup_task)
            .field("tracker_refresh_task", &self.tracker_refresh_task)
            .finish_non_exhaustive()
    }
}

impl TorrentEngine {
    /// Starts a torrent session rooted in the temporary stream cache.
    ///
    /// # Errors
    ///
    /// Returns an error when the cache or loopback listener cannot initialize.
    pub async fn start(
        cache_root: PathBuf,
        cache_policy: StreamCachePolicy,
        tracker_list_config: TrackerListConfig,
        media_tools: MediaTools,
    ) -> Result<Self, TorrentError> {
        Self::start_with_peer_listener(
            cache_root,
            cache_policy,
            tracker_list_config,
            media_tools,
            ListenerOptions::default().listen_addr,
        )
        .await
    }

    async fn start_with_peer_listener(
        cache_root: PathBuf,
        cache_policy: StreamCachePolicy,
        tracker_list_config: TrackerListConfig,
        media_tools: MediaTools,
        peer_listen_addr: SocketAddr,
    ) -> Result<Self, TorrentError> {
        let cache = Arc::new(
            StreamCache::open(cache_root.clone(), cache_policy)
                .await
                .map_err(|error| TorrentError::new(error.user_message()))?,
        );
        let tracker_list = TrackerList::open(cache_root.clone(), tracker_list_config).await?;
        let options = session_options(&cache_root, peer_listen_addr);
        let session = Session::new_with_opts(cache_root, options)
            .await
            .map_err(|error| {
                TorrentError::new(format!("failed to start torrent session: {error}"))
            })?;
        let api = Api::new(session, None);
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|error| {
                TorrentError::new(format!("failed to bind stream listener: {error}"))
            })?;
        let stream_address = listener.local_addr().map_err(|error| {
            TorrentError::new(format!("failed to inspect stream listener: {error}"))
        })?;
        let stream_token = Uuid::new_v4().simple().to_string();
        let media_manifests = Arc::new(RwLock::new(HashMap::new()));
        let state = Arc::new(StreamState {
            api: Api::new(api.session().clone(), None),
            token: stream_token.clone(),
            ffmpeg: media_tools.ffmpeg.clone(),
            manifests: Arc::clone(&media_manifests),
        });
        let router = Router::new()
            .route("/stream/{torrent_id}/{file_id}", get(stream_file))
            .route("/play/{torrent_id}/{file_id}", get(play_media))
            .route(
                "/subtitle/{torrent_id}/{file_id}/{track_id}",
                get(stream_subtitle),
            )
            .with_state(state);
        let stream_task = tokio::spawn(async move {
            if let Err(error) = axum::serve(listener, router).await {
                event!(
                    name: "torrent.stream.server.failed",
                    Level::ERROR,
                    error.message = %error,
                    "torrent stream server failed"
                );
            }
        });
        let cleanup_cache = Arc::clone(&cache);
        let cleanup_task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(CACHE_MAINTENANCE_INTERVAL);
            interval.tick().await;
            loop {
                interval.tick().await;
                if let Err(error) = cleanup_cache.maintain().await {
                    event!(
                        name: "torrent.cache.cleanup.failed",
                        Level::WARN,
                        error.message = error.user_message(),
                        "stream cache cleanup failed"
                    );
                }
            }
        });
        let refresh_tracker_list = Arc::clone(&tracker_list);
        let tracker_refresh_task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(TRACKER_LIST_REFRESH_INTERVAL);
            loop {
                interval.tick().await;
                if let Err(error) = refresh_tracker_list.refresh().await {
                    event!(
                        name: "torrent.tracker_list.refresh.failed",
                        Level::WARN,
                        error.message = error.user_message(),
                        "scheduled tracker-list refresh failed; retaining the prior list"
                    );
                }
            }
        });
        Ok(Self {
            api,
            stream_address,
            stream_token,
            stream_task,
            cleanup_task,
            tracker_refresh_task,
            cache,
            tracker_list,
            ffprobe: media_tools.ffprobe,
            media_manifests,
            active_torrents: Mutex::new(HashMap::new()),
            preparation: Mutex::new(None),
        })
    }

    /// Starts asynchronous metadata resolution and startup buffering.
    ///
    /// A new preparation replaces and cancels any prior preparation. Poll the
    /// returned identifier with [`Self::playback_status`].
    pub async fn prepare_playback(
        self: &Arc<Self>,
        source: String,
        file_path: Option<String>,
    ) -> PlaybackStatus {
        let preparation = Arc::new(PlaybackPreparation {
            id: Uuid::new_v4(),
            source: source.clone(),
            snapshot: StdRwLock::new(PreparationSnapshot {
                stage: PlaybackStage::ResolvingMetadata,
                ticket: None,
                error: None,
            }),
            task: Mutex::new(None),
        });
        let previous = self
            .preparation
            .lock()
            .await
            .replace(Arc::clone(&preparation));
        if let Some(previous) = previous {
            abort_preparation(&previous).await;
            let ticket = {
                previous
                    .snapshot
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .ticket
                    .clone()
            };
            if let Some(ticket) = ticket {
                let _ = self.stop_playback(ticket.torrent_id).await;
            }
        }

        let engine = Arc::clone(self);
        let job = Arc::clone(&preparation);
        let task = tokio::spawn(run_preparation(engine, job, source, file_path));
        *preparation.task.lock().await = Some(task);
        PlaybackStatus {
            preparation_id: preparation.id,
            stage: PlaybackStage::ResolvingMetadata,
            downloaded_bytes: 0,
            total_bytes: 0,
            download_mib_per_second: 0.0,
            connected_peers: 0,
            ticket: None,
            error: None,
        }
    }

    /// Returns current preparation and torrent transfer state.
    ///
    /// # Errors
    ///
    /// Returns an error when `preparation_id` is stale or unavailable.
    pub async fn playback_status(
        &self,
        preparation_id: Uuid,
    ) -> Result<PlaybackStatus, TorrentError> {
        let preparation = self
            .preparation
            .lock()
            .await
            .as_ref()
            .filter(|preparation| preparation.id == preparation_id)
            .cloned()
            .ok_or_else(|| TorrentError::new("playback preparation is unavailable"))?;
        let snapshot = preparation
            .snapshot
            .read()
            .map_err(|_| TorrentError::new("playback status lock was poisoned"))?
            .clone();
        let Some(ticket) = snapshot.ticket.clone() else {
            return Ok(PlaybackStatus {
                preparation_id,
                stage: snapshot.stage,
                downloaded_bytes: 0,
                total_bytes: 0,
                download_mib_per_second: 0.0,
                connected_peers: 0,
                ticket: None,
                error: snapshot.error,
            });
        };
        let handle = self
            .api
            .mgr_handle(TorrentIdOrHash::Id(ticket.torrent_id))
            .map_err(|error| TorrentError::new(format!("failed to inspect torrent: {error}")))?;
        let stats = handle.stats();
        let downloaded_bytes = stats
            .file_progress
            .get(ticket.file_id)
            .copied()
            .unwrap_or_default()
            .min(ticket.file_length);
        let (download_mib_per_second, connected_peers) = stats.live.map_or((0.0, 0), |live| {
            (live.download_speed.mbps, live.snapshot.peer_stats.live)
        });
        Ok(PlaybackStatus {
            preparation_id,
            stage: if matches!(stats.state, TorrentStatsState::Initializing) {
                PlaybackStage::ValidatingCache
            } else {
                snapshot.stage
            },
            downloaded_bytes,
            total_bytes: ticket.file_length,
            download_mib_per_second,
            connected_peers,
            ticket: Some(ticket),
            error: snapshot.error.or(stats.error),
        })
    }

    /// Returns live torrent internals for one playback preparation.
    ///
    /// Seeder classification remains `None` because librqbit does not
    /// expose remote bitfield completion through its stable aggregate API.
    ///
    /// # Errors
    ///
    /// Returns an error when the preparation or its managed torrent is unavailable.
    pub async fn diagnostics(
        &self,
        preparation_id: Uuid,
    ) -> Result<TorrentDiagnostics, TorrentError> {
        let source = self
            .preparation
            .lock()
            .await
            .as_ref()
            .filter(|preparation| preparation.id == preparation_id)
            .map(|preparation| preparation.source.clone());
        let magnet_link = source.as_deref().and_then(magnet_link);
        let playback = self.playback_status(preparation_id).await?;
        let dht = self.api.api_dht_stats().ok().map(|stats| DhtDiagnostics {
            node_id: stats.id.as_string(),
            outstanding_requests: stats.outstanding_requests,
            routing_table_size: stats.routing_table_size,
        });
        let Some(ticket) = playback.ticket.as_ref() else {
            let trackers = match source {
                Some(source) => self.tracker_list.trackers_for(&source).await.to_vec(),
                None => Vec::new(),
            };
            return Ok(TorrentDiagnostics {
                playback,
                engine_state: None,
                info_hash: None,
                magnet_link,
                trackers,
                uploaded_bytes: 0,
                downloaded_this_session_bytes: 0,
                upload_mib_per_second: 0.0,
                peers: PeerDiagnostics::default(),
                pieces: PieceDiagnostics::default(),
                dht,
            });
        };
        let handle = self
            .api
            .mgr_handle(TorrentIdOrHash::Id(ticket.torrent_id))
            .map_err(|error| TorrentError::new(format!("failed to inspect torrent: {error}")))?;
        let stats = handle.stats();
        let (available_piece_bits, total_pieces) = self
            .api
            .api_dump_haves(TorrentIdOrHash::Id(ticket.torrent_id))
            .map_err(|error| {
                TorrentError::new(format!("failed to inspect torrent pieces: {error}"))
            })?;
        let available_pieces = u64::try_from(available_piece_bits.count_ones()).unwrap_or(u64::MAX);
        let mut trackers = handle
            .shared()
            .trackers
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        trackers.sort_unstable();
        let (upload_mib_per_second, peers, pieces) = stats.live.as_ref().map_or_else(
            || (0.0, PeerDiagnostics::default(), PieceDiagnostics::default()),
            |live| {
                let peer_stats = &live.snapshot.peer_stats;
                let average_download_ms = live
                    .average_piece_download_time
                    .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX));
                (
                    live.upload_speed.mbps,
                    PeerDiagnostics {
                        queued: peer_stats.queued,
                        connecting: peer_stats.connecting,
                        connected: peer_stats.live,
                        seen: peer_stats.seen,
                        dead: peer_stats.dead,
                        not_needed: peer_stats.not_needed,
                        seeders: None,
                    },
                    PieceDiagnostics {
                        available: available_pieces,
                        downloaded_this_session: live.snapshot.downloaded_and_checked_pieces,
                        total: total_pieces,
                        average_download_ms,
                    },
                )
            },
        );
        Ok(TorrentDiagnostics {
            playback,
            engine_state: Some(stats.state.to_string()),
            info_hash: Some(handle.info_hash().as_string()),
            magnet_link,
            trackers,
            uploaded_bytes: stats.uploaded_bytes,
            downloaded_this_session_bytes: stats
                .live
                .as_ref()
                .map_or(0, |live| live.snapshot.downloaded_and_checked_bytes),
            upload_mib_per_second,
            peers,
            pieces,
            dht,
        })
    }

    /// Cancels preparation and releases its active torrent.
    ///
    /// # Errors
    ///
    /// Returns an error when torrent cleanup fails.
    pub async fn cancel_preparation(&self, preparation_id: Uuid) -> Result<(), TorrentError> {
        let preparation = {
            let mut current = self.preparation.lock().await;
            match current
                .as_ref()
                .filter(|preparation| preparation.id == preparation_id)
            {
                Some(_) => current.take(),
                None => None,
            }
        };
        let Some(preparation) = preparation else {
            return Ok(());
        };
        abort_preparation(&preparation).await;
        let ticket = preparation
            .snapshot
            .read()
            .map_err(|_| TorrentError::new("playback status lock was poisoned"))?
            .ticket
            .clone();
        if let Some(ticket) = ticket {
            self.stop_playback(ticket.torrent_id).await?;
        }
        Ok(())
    }

    /// Applies cache limits without evicting active playback.
    ///
    /// # Errors
    ///
    /// Returns an error when the policy is invalid or cleanup fails.
    pub async fn update_cache_policy(&self, policy: StreamCachePolicy) -> Result<(), TorrentError> {
        self.cache
            .update_policy(policy)
            .await
            .map_err(|error| TorrentError::new(error.user_message()))
    }

    /// Imports a tracker-list configuration without activating it.
    ///
    /// This lets the settings store commit first and activate without fallible I/O.
    ///
    /// # Errors
    ///
    /// Returns an error when the source is unavailable or has no valid trackers.
    pub async fn prepare_tracker_list(
        &self,
        config: TrackerListConfig,
    ) -> Result<PreparedTrackerList, TorrentError> {
        self.tracker_list.prepare_config(config).await
    }

    /// Activates a successfully imported tracker-list configuration.
    pub async fn activate_tracker_list(&self, prepared: PreparedTrackerList) -> usize {
        self.tracker_list.activate(prepared).await
    }

    /// Resolves metadata and starts streaming the requested media file.
    ///
    /// When `file_path` is absent, the largest supported media file is selected.
    /// A supplied path must match torrent metadata exactly after slash normalization.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid sources, missing media, or engine failures.
    async fn start_playback(
        &self,
        source: impl AsRef<str>,
        file_path: Option<&str>,
    ) -> Result<PlaybackTicket, TorrentError> {
        let listed = self.resolve_metadata(source.as_ref()).await?;
        self.start_resolved_playback(listed, file_path).await
    }

    async fn start_resolved_playback(
        &self,
        listed: ListOnlyResponse,
        file_path: Option<&str>,
    ) -> Result<PlaybackTicket, TorrentError> {
        let files = torrent_files(&listed);
        let selected = select_media_file(&files, file_path)?.clone();
        let cache_key = CacheKey::parse(listed.info_hash.as_string())
            .map_err(|error| TorrentError::new(error.user_message()))?;
        self.cache
            .acquire(cache_key.clone())
            .await
            .map_err(|error| TorrentError::new(error.user_message()))?;
        let torrent_id = match self
            .add_selected_torrent(listed, &selected, &cache_key)
            .await
        {
            Ok(torrent_id) => torrent_id,
            Err(error) => {
                self.cache
                    .release(&cache_key)
                    .await
                    .map_err(|cache_error| TorrentError::new(cache_error.user_message()))?;
                return Err(error);
            }
        };
        let replaced = self
            .active_torrents
            .lock()
            .await
            .insert(torrent_id, cache_key.clone());
        if let Some(previous) = replaced {
            self.cache
                .release(&previous)
                .await
                .map_err(|error| TorrentError::new(error.user_message()))?;
        }
        let stream_url = format!(
            "http://{}/stream/{}/{}?token={}",
            self.stream_address, torrent_id, selected.id, self.stream_token
        );
        event!(
            name: "torrent.playback.started",
            Level::INFO,
            torrent.id = torrent_id,
            file.id = selected.id,
            file.size = selected.length,
            "torrent playback started"
        );
        Ok(PlaybackTicket {
            torrent_id,
            file_id: selected.id,
            file_name: selected.name,
            file_length: selected.length,
            playback_url: stream_url.clone(),
            stream_url,
            duration_seconds: None,
            audio_tracks: Vec::new(),
            subtitle_tracks: Vec::new(),
        })
    }

    async fn inspect_media(
        &self,
        mut ticket: PlaybackTicket,
    ) -> Result<PlaybackTicket, TorrentError> {
        let output = probe_media(&self.ffprobe, &ticket.stream_url).await?;
        let video_stream = output
            .streams
            .iter()
            .find(|stream| stream.codec_type.as_deref() == Some("video"))
            .ok_or_else(|| TorrentError::new("selected media has no identifiable video track"))?;
        let video_bridge = select_video_bridge(video_stream, output.format.as_ref())?;
        let duration_seconds = output
            .format
            .as_ref()
            .and_then(|format| format.duration.as_deref())
            .and_then(|duration| duration.parse::<f64>().ok())
            .filter(|duration| duration.is_finite() && *duration > 0.0);
        let mut audio_tracks = Vec::new();
        let mut subtitle_tracks = Vec::new();
        for stream in output.streams {
            let Some(codec) = stream.codec_name else {
                continue;
            };
            let track = MediaTrack {
                id: stream.index,
                codec,
                language: stream.tags.language,
                title: stream.tags.title,
                channels: stream.channels,
                is_default: stream.disposition.default != 0,
                is_forced: stream.disposition.forced != 0,
                stream_url: None,
            };
            match stream.codec_type.as_deref() {
                Some("audio") => audio_tracks.push(track),
                Some("subtitle") => subtitle_tracks.push(track),
                Some(_) | None => {}
            }
        }
        let default_audio_track = audio_tracks
            .iter()
            .find(|track| track.is_default)
            .or_else(|| audio_tracks.first())
            .map(|track| track.id);
        let playback_url = media_url(
            self.stream_address,
            "play",
            ticket.torrent_id,
            ticket.file_id,
            &self.stream_token,
        );
        for track in &mut subtitle_tracks {
            track.stream_url = Some(subtitle_url(
                self.stream_address,
                ticket.torrent_id,
                ticket.file_id,
                track.id,
                &self.stream_token,
            ));
        }
        self.media_manifests.write().await.insert(
            (ticket.torrent_id, ticket.file_id),
            MediaManifest {
                native_url: ticket.stream_url.clone(),
                duration_seconds,
                video_bridge,
                audio_track_ids: audio_tracks.iter().map(|track| track.id).collect(),
                subtitle_track_ids: subtitle_tracks.iter().map(|track| track.id).collect(),
                default_audio_track,
            },
        );
        ticket.playback_url = playback_url;
        ticket.duration_seconds = duration_seconds;
        ticket.audio_tracks = audio_tracks;
        ticket.subtitle_tracks = subtitle_tracks;
        Ok(ticket)
    }

    async fn prebuffer(&self, ticket: &PlaybackTicket) -> Result<(), TorrentError> {
        let handle = self
            .api
            .mgr_handle(TorrentIdOrHash::Id(ticket.torrent_id))
            .map_err(|error| {
                TorrentError::new(format!("failed to find startup torrent: {error}"))
            })?;
        handle.wait_until_initialized().await.map_err(|error| {
            TorrentError::new(format!("torrent initialization failed: {error}"))
        })?;
        let mut stream = handle.stream(ticket.file_id).await.map_err(|error| {
            TorrentError::new(format!("failed to open startup buffer: {error}"))
        })?;
        let target = start_buffer_target(ticket.file_length);
        let mut buffered = 0_u64;
        let mut bytes = vec![0_u8; STREAM_READ_BUFFER_BYTES];
        while buffered < target {
            let remaining = target.saturating_sub(buffered);
            let read_length = usize::try_from(remaining)
                .unwrap_or(usize::MAX)
                .min(bytes.len());
            let count = stream
                .read(&mut bytes[..read_length])
                .await
                .map_err(|error| TorrentError::new(format!("startup buffering failed: {error}")))?;
            if count == 0 {
                break;
            }
            buffered = buffered.saturating_add(u64::try_from(count).unwrap_or(u64::MAX));
        }
        if buffered < target {
            return Err(TorrentError::new(
                "media stream ended before startup buffering completed",
            ));
        }
        Ok(())
    }

    async fn resolve_metadata(&self, source: &str) -> Result<ListOnlyResponse, TorrentError> {
        let supplemental = self.tracker_list.trackers_for(source).await;
        let trackers = (!supplemental.is_empty()).then(|| supplemental.iter().cloned().collect());
        let response = self
            .api
            .session()
            .add_torrent(
                AddTorrent::from_url(source),
                Some(AddTorrentOptions {
                    list_only: true,
                    trackers,
                    peer_opts: Some(PeerConnectionOptions {
                        connect_timeout: Some(METADATA_CONNECT_TIMEOUT),
                        read_write_timeout: Some(METADATA_READ_WRITE_TIMEOUT),
                        ..PeerConnectionOptions::default()
                    }),
                    ..AddTorrentOptions::default()
                }),
            )
            .await
            .map_err(|error| TorrentError::new(format!("failed to resolve torrent: {error}")))?;
        match response {
            AddTorrentResponse::ListOnly(listed) => Ok(listed),
            AddTorrentResponse::Added(_, _) | AddTorrentResponse::AlreadyManaged(_, _) => Err(
                TorrentError::new("torrent metadata request returned an unexpected state"),
            ),
        }
    }

    async fn add_selected_torrent(
        &self,
        listed: ListOnlyResponse,
        selected: &TorrentFile,
        cache_key: &CacheKey,
    ) -> Result<usize, TorrentError> {
        let response = self
            .api
            .session()
            .add_torrent(
                AddTorrent::from_bytes(listed.torrent_bytes),
                Some(AddTorrentOptions {
                    only_files: Some(vec![selected.id]),
                    overwrite: true,
                    initial_peers: Some(listed.seen_peers),
                    sub_folder: Some(cache_key.as_str().to_owned()),
                    ..AddTorrentOptions::default()
                }),
            )
            .await
            .map_err(|error| TorrentError::new(format!("failed to start torrent: {error}")))?;
        match response {
            AddTorrentResponse::Added(id, _) => Ok(id),
            AddTorrentResponse::AlreadyManaged(id, _) => {
                self.api
                    .api_torrent_action_update_only_files(
                        TorrentIdOrHash::Id(id),
                        &HashSet::from([selected.id]),
                    )
                    .await
                    .map_err(|error| {
                        TorrentError::new(format!(
                            "failed to select the requested torrent file: {error}"
                        ))
                    })?;
                Ok(id)
            }
            AddTorrentResponse::ListOnly(_) => Err(TorrentError::new(
                "torrent remained metadata-only after playback start",
            )),
        }
    }

    /// Stops networking while retaining temporary verified cache bytes.
    ///
    /// # Errors
    ///
    /// Returns an error when the engine cannot forget the active torrent.
    pub async fn stop_playback(&self, torrent_id: usize) -> Result<(), TorrentError> {
        self.media_manifests
            .write()
            .await
            .retain(|(active_torrent_id, _), _| *active_torrent_id != torrent_id);
        self.api
            .api_torrent_action_forget(TorrentIdOrHash::Id(torrent_id))
            .await
            .map_err(|error| TorrentError::new(format!("failed to stop torrent: {error}")))?;
        if let Some(cache_key) = self.active_torrents.lock().await.remove(&torrent_id) {
            self.cache
                .release(&cache_key)
                .await
                .map_err(|error| TorrentError::new(error.user_message()))?;
        }
        self.cache
            .cleanup()
            .await
            .map_err(|error| TorrentError::new(error.user_message()))?;
        Ok(())
    }

    /// Stops active torrents and releases every cache lease.
    ///
    /// # Errors
    ///
    /// Returns the first engine or cache failure encountered during shutdown.
    pub async fn shutdown(&self) -> Result<(), TorrentError> {
        if let Some(preparation) = self.preparation.lock().await.take() {
            abort_preparation(&preparation).await;
        }
        let active = {
            let mut active = self.active_torrents.lock().await;
            active.drain().collect::<Vec<_>>()
        };
        let mut first_error = None;
        for (torrent_id, cache_key) in active {
            if let Err(error) = self
                .api
                .api_torrent_action_forget(TorrentIdOrHash::Id(torrent_id))
                .await
                && first_error.is_none()
            {
                first_error = Some(TorrentError::new(format!(
                    "failed to stop torrent during shutdown: {error}"
                )));
            }
            if let Err(error) = self.cache.release(&cache_key).await
                && first_error.is_none()
            {
                first_error = Some(TorrentError::new(error.user_message()));
            }
        }
        if let Err(error) = self.cache.cleanup().await
            && first_error.is_none()
        {
            first_error = Some(TorrentError::new(error.user_message()));
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

fn session_options(cache_root: &FsPath, peer_listen_addr: SocketAddr) -> SessionOptions {
    SessionOptions {
        // A shared TCP/uTP listener lets rqbit race both peer transports while
        // advertising one OS-assigned port. uTP is required for swarms whose
        // peers do not accept TCP; disabling it regresses cold magnet startup.
        listen: Some(ListenerOptions {
            mode: ListenerMode::TcpAndUtp,
            listen_addr: peer_listen_addr,
            ..ListenerOptions::default()
        }),
        dht: Some(DhtSessionConfig {
            persistence: Some(DhtPersistenceConfig {
                config_filename: Some(cache_root.join(DHT_STATE_FILENAME)),
                ..DhtPersistenceConfig::default()
            }),
            ..DhtSessionConfig::default()
        }),
        // Torrent membership remains session-scoped. RedCrown persists only
        // DHT discovery and verified-piece state. The latter lives inside each
        // expiring cache entry, so media and verification state are evicted
        // together without restoring torrents as persistent downloads.
        fastresume: true,
        fastresume_root: Some(cache_root.to_path_buf()),
        persistence: None,
        ..SessionOptions::default()
    }
}

impl Drop for TorrentEngine {
    fn drop(&mut self) {
        self.stream_task.abort();
        self.cleanup_task.abort();
        self.tracker_refresh_task.abort();
        if let Some(preparation) = self.preparation.get_mut().take()
            && let Some(task) = preparation
                .task
                .try_lock()
                .ok()
                .and_then(|mut task| task.take())
        {
            task.abort();
        }
        self.api.session().cancellation_token().cancel();
        self.stream_task.abort();
        self.cleanup_task.abort();
    }
}

async fn run_preparation(
    engine: Arc<TorrentEngine>,
    preparation: Arc<PlaybackPreparation>,
    source: String,
    file_path: Option<String>,
) {
    let ticket = match engine.start_playback(&source, file_path.as_deref()).await {
        Ok(ticket) => ticket,
        Err(error) => {
            fail_preparation(&preparation, &error);
            return;
        }
    };
    {
        let mut snapshot = preparation
            .snapshot
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        snapshot.stage = PlaybackStage::Buffering;
        snapshot.ticket = Some(ticket.clone());
    }
    if let Err(error) = engine.prebuffer(&ticket).await {
        let _ = engine.stop_playback(ticket.torrent_id).await;
        fail_preparation(&preparation, &error);
        return;
    }
    let torrent_id = ticket.torrent_id;
    match engine.inspect_media(ticket).await {
        Ok(ticket) => {
            let mut snapshot = preparation
                .snapshot
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            snapshot.ticket = Some(ticket);
            snapshot.stage = PlaybackStage::Ready;
        }
        Err(error) => {
            let _ = engine.stop_playback(torrent_id).await;
            fail_preparation(&preparation, &error);
        }
    }
}

fn fail_preparation(preparation: &PlaybackPreparation, error: &TorrentError) {
    let mut snapshot = preparation
        .snapshot
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    snapshot.stage = PlaybackStage::Failed;
    snapshot.ticket = None;
    snapshot.error = Some(error.user_message().to_owned());
}

async fn abort_preparation(preparation: &PlaybackPreparation) {
    if let Some(task) = preparation.task.lock().await.take() {
        task.abort();
    }
}

fn start_buffer_target(file_length: u64) -> u64 {
    let adaptive = file_length / START_BUFFER_DIVISOR;
    adaptive
        .clamp(MIN_START_BUFFER_BYTES, MAX_START_BUFFER_BYTES)
        .min(file_length)
}

fn magnet_link(source: &str) -> Option<String> {
    let source = source.trim();
    source
        .get(..7)
        .filter(|scheme| scheme.eq_ignore_ascii_case("magnet:"))
        .map(|_| source.to_owned())
}

fn select_media_file<'a>(
    files: &'a [TorrentFile],
    requested_path: Option<&str>,
) -> Result<&'a TorrentFile, TorrentError> {
    if let Some(requested_path) = requested_path {
        let requested_path = normalized_torrent_path(requested_path);
        return files
            .iter()
            .find(|file| {
                is_supported_media(&file.name)
                    && normalized_torrent_path(&file.name) == requested_path
            })
            .ok_or_else(|| {
                TorrentError::new("selected episode file was not found in torrent metadata")
            });
    }
    files
        .iter()
        .filter(|file| is_supported_media(&file.name))
        .max_by_key(|file| file.length)
        .ok_or_else(|| TorrentError::new("torrent contains no supported media file"))
}

fn normalized_torrent_path(path: &str) -> String {
    path.replace('\\', "/")
        .trim_start_matches("./")
        .trim_start_matches('/')
        .to_owned()
}

fn torrent_files(listed: &ListOnlyResponse) -> Vec<TorrentFile> {
    listed
        .info
        .iter_file_details()
        .enumerate()
        .map(|(id, details)| TorrentFile {
            id,
            name: details.filename.to_string(),
            length: details.len,
        })
        .collect()
}

fn is_supported_media(name: &str) -> bool {
    let lowercase = name.to_ascii_lowercase();
    [".mp4", ".mkv", ".webm", ".avi", ".mov", ".m4v"]
        .iter()
        .any(|extension| lowercase.ends_with(extension))
}

fn required_tool_path(variable: &str) -> Result<PathBuf, TorrentError> {
    let path = std::env::var_os(variable)
        .map(PathBuf::from)
        .ok_or_else(|| TorrentError::new(format!("{variable} is not configured")))?;
    if !path.is_file() {
        return Err(TorrentError::new(format!(
            "{variable} does not identify a bundled executable"
        )));
    }
    Ok(path)
}

fn media_url(
    address: SocketAddr,
    route: &str,
    torrent_id: usize,
    file_id: usize,
    token: &str,
) -> String {
    format!("http://{address}/{route}/{torrent_id}/{file_id}?token={token}")
}

fn subtitle_url(
    address: SocketAddr,
    torrent_id: usize,
    file_id: usize,
    track_id: usize,
    token: &str,
) -> String {
    format!("http://{address}/subtitle/{torrent_id}/{file_id}/{track_id}?token={token}")
}

#[derive(Debug, Deserialize)]
struct ProbeOutput {
    #[serde(default)]
    streams: Vec<ProbeStream>,
    format: Option<ProbeFormat>,
}

#[derive(Debug, Deserialize)]
struct ProbeStream {
    index: usize,
    codec_name: Option<String>,
    codec_type: Option<String>,
    bit_rate: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    color_transfer: Option<String>,
    channels: Option<u16>,
    #[serde(default)]
    disposition: ProbeDisposition,
    #[serde(default)]
    tags: ProbeTags,
}

#[derive(Debug, Default, Deserialize)]
struct ProbeDisposition {
    default: u8,
    forced: u8,
}

#[derive(Debug, Default, Deserialize)]
struct ProbeTags {
    language: Option<String>,
    title: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
    bit_rate: Option<String>,
}

async fn probe_media(ffprobe: &FsPath, source: &str) -> Result<ProbeOutput, TorrentError> {
    let output = tokio::time::timeout(
        MEDIA_PROBE_TIMEOUT,
        Command::new(ffprobe)
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=index,codec_type,codec_name,bit_rate,width,height,color_transfer,channels:stream_tags=language,title:stream_disposition=default,forced",
                "-show_entries",
                "format=duration,bit_rate",
                "-of",
                "json",
                source,
            ])
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| TorrentError::new("media inspection timed out"))?
    .map_err(|error| TorrentError::new(format!("failed to inspect media tracks: {error}")))?;
    if !output.status.success() {
        return Err(TorrentError::new(format!(
            "media inspection failed: {}",
            bounded_process_error(&output.stderr)
        )));
    }
    parse_probe_output(&output.stdout)
}

fn parse_probe_output(bytes: &[u8]) -> Result<ProbeOutput, TorrentError> {
    serde_json::from_slice(bytes)
        .map_err(|error| TorrentError::new(format!("invalid media inspection response: {error}")))
}

fn select_video_bridge(
    stream: &ProbeStream,
    format: Option<&ProbeFormat>,
) -> Result<VideoBridge, TorrentError> {
    let codec = stream
        .codec_name
        .as_deref()
        .ok_or_else(|| TorrentError::new("selected media has no identifiable video codec"))?;
    match codec {
        COMPATIBLE_VIDEO_CODEC => Ok(VideoBridge::CopyH264),
        TRANSCODED_VIDEO_CODEC => {
            let source_bitrate = parse_positive_bitrate(stream.bit_rate.as_deref()).or_else(|| {
                format.and_then(|value| parse_positive_bitrate(value.bit_rate.as_deref()))
            });
            Ok(VideoBridge::TranscodeToH264 {
                bitrate: h264_transcode_bitrate(source_bitrate, stream.width, stream.height),
                hdr_transfer: match stream.color_transfer.as_deref() {
                    Some("arib-std-b67") => Some(HdrTransfer::Hlg),
                    Some("smpte2084") => Some(HdrTransfer::Pq),
                    Some(_) | None => None,
                },
            })
        }
        _ => Err(TorrentError::new(format!(
            "video codec {codec} is not supported by the media bridge"
        ))),
    }
}

fn parse_positive_bitrate(value: Option<&str>) -> Option<u64> {
    value
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
}

fn h264_transcode_bitrate(
    source_bitrate: Option<u64>,
    width: Option<u32>,
    height: Option<u32>,
) -> u64 {
    let target = source_bitrate.map_or_else(
        || resolution_bitrate(width, height),
        |bitrate| {
            bitrate.saturating_mul(HEVC_TO_H264_BITRATE_NUMERATOR)
                / HEVC_TO_H264_BITRATE_DENOMINATOR
        },
    );
    target.clamp(MIN_H264_TRANSCODE_BITRATE, MAX_H264_TRANSCODE_BITRATE)
}

fn resolution_bitrate(width: Option<u32>, height: Option<u32>) -> u64 {
    let pixels = u64::from(width.unwrap_or(1920)) * u64::from(height.unwrap_or(1080));
    match pixels {
        0..=921_600 => 5_000_000,
        921_601..=2_073_600 => 8_000_000,
        2_073_601..=3_686_400 => 12_000_000,
        _ => 24_000_000,
    }
}

fn hdr_to_sdr_filter(transfer: HdrTransfer) -> &'static str {
    match transfer {
        HdrTransfer::Hlg => HLG_TO_SDR_FILTER,
        HdrTransfer::Pq => PQ_TO_SDR_FILTER,
    }
}

fn bounded_process_error(bytes: &[u8]) -> String {
    let limit = usize::try_from(MAX_MEDIA_ERROR_BYTES).unwrap_or(usize::MAX);
    let start = bytes.len().saturating_sub(limit);
    String::from_utf8_lossy(&bytes[start..]).trim().to_owned()
}

#[derive(Debug, Deserialize)]
struct StreamQuery {
    token: String,
}

#[derive(Debug, Deserialize)]
struct PlaybackQuery {
    token: String,
    audio: Option<usize>,
    start: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct SubtitleQuery {
    token: String,
    start: Option<f64>,
}

async fn stream_file(
    State(state): State<Arc<StreamState>>,
    Path((torrent_id, file_id)): Path<(usize, usize)>,
    Query(query): Query<StreamQuery>,
    headers: HeaderMap,
) -> Response {
    if query.token != state.token {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match build_stream_response(&state.api, torrent_id, file_id, &headers).await {
        Ok(response) => response,
        Err(error) => {
            event!(
                name: "torrent.stream.request.failed",
                Level::WARN,
                torrent.id = torrent_id,
                file.id = file_id,
                error.message = error.summary(),
                "torrent stream request failed"
            );
            (StatusCode::BAD_REQUEST, error.summary().to_owned()).into_response()
        }
    }
}

async fn play_media(
    State(state): State<Arc<StreamState>>,
    Path((torrent_id, file_id)): Path<(usize, usize)>,
    Query(query): Query<PlaybackQuery>,
) -> Response {
    if query.token != state.token {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match build_playback_response(&state, torrent_id, file_id, &query).await {
        Ok(response) => response,
        Err(error) => {
            event!(
                name: "media.transcode.request.failed",
                Level::WARN,
                torrent.id = torrent_id,
                file.id = file_id,
                error.message = error.summary(),
                "media transcode request failed"
            );
            (StatusCode::BAD_REQUEST, error.summary().to_owned()).into_response()
        }
    }
}

async fn stream_subtitle(
    State(state): State<Arc<StreamState>>,
    Path((torrent_id, file_id, track_id)): Path<(usize, usize, usize)>,
    Query(query): Query<SubtitleQuery>,
) -> Response {
    if query.token != state.token {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match build_subtitle_response(&state, torrent_id, file_id, track_id, query.start).await {
        Ok(response) => response,
        Err(error) => {
            event!(
                name: "media.subtitle.request.failed",
                Level::WARN,
                torrent.id = torrent_id,
                file.id = file_id,
                media.track.id = track_id,
                error.message = error.summary(),
                "subtitle stream request failed"
            );
            (StatusCode::BAD_REQUEST, error.summary().to_owned()).into_response()
        }
    }
}

async fn build_playback_response(
    state: &StreamState,
    torrent_id: usize,
    file_id: usize,
    query: &PlaybackQuery,
) -> Result<Response, TorrentError> {
    let manifest = state
        .manifests
        .read()
        .await
        .get(&(torrent_id, file_id))
        .cloned()
        .ok_or_else(|| TorrentError::new("media manifest is unavailable"))?;
    let audio_track = query.audio.or(manifest.default_audio_track);
    if audio_track.is_some_and(|track| !manifest.audio_track_ids.contains(&track)) {
        return Err(TorrentError::new("requested audio track is unavailable"));
    }
    let start = query.start.unwrap_or(0.0);
    if !start.is_finite()
        || start < 0.0
        || manifest
            .duration_seconds
            .is_some_and(|duration| start >= duration)
    {
        return Err(TorrentError::new("requested playback position is invalid"));
    }
    let arguments = playback_arguments(
        &manifest.native_url,
        audio_track,
        start,
        manifest.video_bridge,
    );
    media_process_response(&state.ffmpeg, &arguments, "video/mp4")
}

fn playback_arguments(
    native_url: &str,
    audio_track: Option<usize>,
    start: f64,
    video_bridge: VideoBridge,
) -> Vec<String> {
    let mut arguments = vec![
        "-nostdin".to_owned(),
        "-hide_banner".to_owned(),
        "-loglevel".to_owned(),
        "error".to_owned(),
    ];
    if start > 0.0 {
        if video_bridge == VideoBridge::CopyH264 {
            arguments.push("-noaccurate_seek".to_owned());
        }
        arguments.extend(["-ss".to_owned(), format!("{start:.3}")]);
    }
    arguments.extend([
        "-readrate".to_owned(),
        "1.1".to_owned(),
        "-i".to_owned(),
        native_url.to_owned(),
        "-map".to_owned(),
        "0:v:0".to_owned(),
    ]);
    if let Some(audio_track) = audio_track {
        arguments.extend(["-map".to_owned(), format!("0:{audio_track}")]);
    }
    match video_bridge {
        VideoBridge::CopyH264 => {
            arguments.extend(["-c:v".to_owned(), "copy".to_owned()]);
        }
        VideoBridge::TranscodeToH264 {
            bitrate,
            hdr_transfer,
        } => {
            arguments.extend([
                "-c:v".to_owned(),
                "libopenh264".to_owned(),
                "-profile:v".to_owned(),
                "high".to_owned(),
                "-rc_mode".to_owned(),
                "quality".to_owned(),
                "-b:v".to_owned(),
                bitrate.to_string(),
            ]);
            if let Some(transfer) = hdr_transfer {
                arguments.extend(["-vf".to_owned(), hdr_to_sdr_filter(transfer).to_owned()]);
            }
            arguments.extend([
                "-pix_fmt".to_owned(),
                "yuv420p".to_owned(),
                "-g".to_owned(),
                H264_TRANSCODE_GOP_FRAMES.to_owned(),
                "-fps_mode".to_owned(),
                "passthrough".to_owned(),
            ]);
        }
    }
    arguments.extend([
        "-c:a".to_owned(),
        "aac".to_owned(),
        "-b:a".to_owned(),
        "192k".to_owned(),
        "-af".to_owned(),
        "aresample=async=1000".to_owned(),
        "-map_metadata".to_owned(),
        "-1".to_owned(),
        "-movflags".to_owned(),
        "frag_keyframe+empty_moov+default_base_moof".to_owned(),
        "-f".to_owned(),
        "mp4".to_owned(),
        "pipe:1".to_owned(),
    ]);
    arguments
}

async fn build_subtitle_response(
    state: &StreamState,
    torrent_id: usize,
    file_id: usize,
    track_id: usize,
    start: Option<f64>,
) -> Result<Response, TorrentError> {
    let manifest = state
        .manifests
        .read()
        .await
        .get(&(torrent_id, file_id))
        .cloned()
        .ok_or_else(|| TorrentError::new("media manifest is unavailable"))?;
    if !manifest.subtitle_track_ids.contains(&track_id) {
        return Err(TorrentError::new("requested subtitle track is unavailable"));
    }
    let start = start.unwrap_or(0.0);
    if !start.is_finite()
        || start < 0.0
        || manifest
            .duration_seconds
            .is_some_and(|duration| start >= duration)
    {
        return Err(TorrentError::new("requested subtitle position is invalid"));
    }
    let mut arguments = vec![
        "-nostdin".to_owned(),
        "-hide_banner".to_owned(),
        "-loglevel".to_owned(),
        "error".to_owned(),
    ];
    if start > 0.0 {
        arguments.extend(["-ss".to_owned(), format!("{start:.3}")]);
    }
    arguments.extend([
        "-readrate".to_owned(),
        "1.0".to_owned(),
        "-i".to_owned(),
        manifest.native_url,
        "-map".to_owned(),
        format!("0:{track_id}"),
        "-c:s".to_owned(),
        "webvtt".to_owned(),
        "-f".to_owned(),
        "webvtt".to_owned(),
        "pipe:1".to_owned(),
    ]);
    media_process_response(&state.ffmpeg, &arguments, "text/vtt; charset=utf-8")
}

fn media_process_response(
    ffmpeg: &FsPath,
    arguments: &[String],
    content_type: &'static str,
) -> Result<Response, TorrentError> {
    let mut child = Command::new(ffmpeg)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| TorrentError::new(format!("failed to start media bridge: {error}")))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| TorrentError::new("media bridge stdout is unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| TorrentError::new("media bridge stderr is unavailable"))?;
    let (mut writer, reader) = tokio::io::duplex(MEDIA_PIPE_BYTES);
    tokio::spawn(async move {
        let error_task = tokio::spawn(async move {
            let mut bytes = Vec::new();
            let _ = stderr
                .take(MAX_MEDIA_ERROR_BYTES)
                .read_to_end(&mut bytes)
                .await;
            bytes
        });
        let copy_result = tokio::io::copy(&mut stdout, &mut writer).await;
        drop(writer);
        if copy_result.is_err() {
            let _ = child.kill().await;
        }
        let status = child.wait().await;
        let errors = error_task.await.unwrap_or_default();
        if let Ok(status) = status
            && !status.success()
        {
            event!(
                name: "media.bridge.process.failed",
                Level::WARN,
                process.exit.code = status.code(),
                error.message = bounded_process_error(&errors),
                "media bridge process failed"
            );
        }
    });
    let mut response = Response::new(Body::from_stream(ReaderStream::new(reader)));
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

async fn build_stream_response(
    api: &Api,
    torrent_id: usize,
    file_id: usize,
    headers: &HeaderMap,
) -> Result<Response, TorrentError> {
    let mut stream = api
        .api_stream(TorrentIdOrHash::Id(torrent_id), file_id)
        .await
        .map_err(|error| TorrentError::new(format!("failed to open media stream: {error}")))?;
    let total_length = stream.len();
    let range = parse_range(headers.get(RANGE), total_length)?;
    let (status, start, end) = match range {
        Some((start, end)) => {
            stream.seek(SeekFrom::Start(start)).await.map_err(|error| {
                TorrentError::new(format!("failed to seek media stream: {error}"))
            })?;
            (StatusCode::PARTIAL_CONTENT, start, end)
        }
        None => (StatusCode::OK, 0, total_length.saturating_sub(1)),
    };
    let content_length = end.saturating_sub(start).saturating_add(1);
    let mime = api
        .torrent_file_mime_type(TorrentIdOrHash::Id(torrent_id), file_id)
        .unwrap_or("application/octet-stream");
    let body = Body::from_stream(ReaderStream::with_capacity(
        stream.take(content_length),
        STREAM_READ_BUFFER_BYTES,
    ));
    let mut response = Response::new(body);
    *response.status_mut() = status;
    let response_headers = response.headers_mut();
    response_headers.insert(ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    response_headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_str(mime).map_err(|error| {
            TorrentError::new(format!("invalid media content type from engine: {error}"))
        })?,
    );
    response_headers.insert(
        CONTENT_LENGTH,
        HeaderValue::from_str(&content_length.to_string())
            .map_err(|error| TorrentError::new(format!("invalid content length: {error}")))?,
    );
    if status == StatusCode::PARTIAL_CONTENT {
        response_headers.insert(
            CONTENT_RANGE,
            HeaderValue::from_str(&format!("bytes {start}-{end}/{total_length}"))
                .map_err(|error| TorrentError::new(format!("invalid content range: {error}")))?,
        );
    }
    Ok(response)
}

fn parse_range(
    value: Option<&HeaderValue>,
    total_length: u64,
) -> Result<Option<(u64, u64)>, TorrentError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let text = value
        .to_str()
        .map_err(|_| TorrentError::new("invalid Range header"))?;
    let range = text
        .strip_prefix("bytes=")
        .ok_or_else(|| TorrentError::new("only byte ranges are supported"))?;
    if range.contains(',') {
        return Err(TorrentError::new("multiple byte ranges are unsupported"));
    }
    let (start, end) = range
        .split_once('-')
        .ok_or_else(|| TorrentError::new("invalid byte range"))?;
    let start = start
        .parse::<u64>()
        .map_err(|_| TorrentError::new("range start must be an integer"))?;
    let end = if end.is_empty() {
        total_length.saturating_sub(1)
    } else {
        end.parse::<u64>()
            .map_err(|_| TorrentError::new("range end must be an integer"))?
            .min(total_length.saturating_sub(1))
    };
    if total_length == 0 || start > end || start >= total_length {
        return Err(TorrentError::new("requested range is not satisfiable"));
    }
    Ok(Some((start, end)))
}

/// Reports torrent initialization or streaming failures.
#[derive(Debug)]
pub struct TorrentError {
    message: String,
    backtrace: Backtrace,
}

impl TorrentError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            backtrace: Backtrace::capture(),
        }
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

impl Display for TorrentError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}\n{}", self.message, self.backtrace)
    }
}

impl std::error::Error for TorrentError {}

#[cfg(test)]
mod integration_tests;

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;
    use std::path::Path;

    use axum::http::HeaderValue;

    use super::{
        DHT_STATE_FILENAME, HdrTransfer, ListenerMode, MAX_START_BUFFER_BYTES,
        MIN_START_BUFFER_BYTES, TorrentFile, VideoBridge, h264_transcode_bitrate, magnet_link,
        parse_probe_output, parse_range, playback_arguments, select_media_file,
        select_video_bridge, session_options, start_buffer_target,
    };

    #[test]
    fn dht_routing_state_is_persistent_and_cache_scoped() {
        let cache_root = Path::new("stream-cache");
        let options = session_options(cache_root, (Ipv4Addr::LOCALHOST, 0).into());
        let dht = options.dht.expect("DHT config");
        let persistence = dht.persistence.expect("DHT persistence config");

        assert_eq!(
            persistence.config_filename.as_deref(),
            Some(cache_root.join(DHT_STATE_FILENAME).as_path())
        );
        assert!(options.persistence.is_none());
        assert!(options.fastresume);
        assert_eq!(options.fastresume_root.as_deref(), Some(cache_root));
    }

    #[test]
    fn selects_largest_supported_media_file() {
        let files = vec![
            TorrentFile {
                id: 0,
                name: "sample.mkv".to_owned(),
                length: 50,
            },
            TorrentFile {
                id: 1,
                name: "movie.mkv".to_owned(),
                length: 500,
            },
            TorrentFile {
                id: 2,
                name: "archive.zip".to_owned(),
                length: 1_000,
            },
        ];
        assert_eq!(select_media_file(&files, None).expect("media").id, 1);
    }

    #[test]
    fn selects_exact_episode_path_instead_of_largest_file() {
        let files = vec![
            TorrentFile {
                id: 0,
                name: "Series/Series.S01E01.mkv".to_owned(),
                length: 900,
            },
            TorrentFile {
                id: 1,
                name: "Series/Series.S01E02.mkv".to_owned(),
                length: 800,
            },
        ];

        assert_eq!(
            select_media_file(&files, Some(r".\Series\Series.S01E02.mkv"))
                .expect("episode")
                .id,
            1
        );
    }

    #[test]
    fn never_falls_back_when_requested_episode_is_missing() {
        let files = vec![TorrentFile {
            id: 0,
            name: "Series/Series.S01E01.mkv".to_owned(),
            length: 900,
        }];

        assert!(select_media_file(&files, Some("Series/Series.S01E02.mkv")).is_err());
    }

    #[test]
    fn startup_buffer_is_adaptive_and_bounded() {
        assert_eq!(start_buffer_target(1_000_000), 1_000_000);
        assert_eq!(start_buffer_target(500_000_000), MIN_START_BUFFER_BYTES);
        assert_eq!(start_buffer_target(2_000_000_000), 20_000_000);
        assert_eq!(start_buffer_target(10_000_000_000), MAX_START_BUFFER_BYTES);
    }

    #[test]
    fn session_enables_tcp_and_utp_on_an_os_assigned_port() {
        let options = session_options(Path::new("cache"), (Ipv4Addr::LOCALHOST, 0).into());
        let listen = options.listen.expect("peer listener");

        assert!(matches!(listen.mode, ListenerMode::TcpAndUtp));
        assert!(listen.listen_addr.ip().is_loopback());
        assert_eq!(listen.listen_addr.port(), 0);
    }

    #[test]
    fn diagnostics_preserve_only_actual_magnet_sources() {
        let source = "  MAGNET:?xt=urn:btih:0123456789abcdef  ";
        assert_eq!(
            magnet_link(source).as_deref(),
            Some("MAGNET:?xt=urn:btih:0123456789abcdef")
        );
        assert_eq!(magnet_link("https://example.test/media.torrent"), None);
    }

    #[test]
    fn parses_single_open_ended_range() {
        let value = HeaderValue::from_static("bytes=10-");
        assert_eq!(
            parse_range(Some(&value), 100).expect("range"),
            Some((10, 99))
        );
    }

    #[test]
    fn rejects_multiple_ranges() {
        let value = HeaderValue::from_static("bytes=0-1,3-4");
        assert!(parse_range(Some(&value), 100).is_err());
    }

    #[test]
    fn parses_real_audio_and_subtitle_track_metadata() {
        let output = parse_probe_output(
            br#"{
                "streams": [
                    {
                        "index": 3,
                        "codec_name": "eac3",
                        "codec_type": "audio",
                        "channels": 6,
                        "disposition": { "default": 1, "forced": 0 },
                        "tags": { "language": "eng", "title": "English" }
                    },
                    {
                        "index": 6,
                        "codec_name": "subrip",
                        "codec_type": "subtitle",
                        "disposition": { "default": 0, "forced": 1 },
                        "tags": { "language": "eng", "title": "English (SDH)" }
                    }
                ],
                "format": { "duration": "6874.752000" }
            }"#,
        )
        .expect("probe output");

        assert_eq!(output.streams.len(), 2);
        assert_eq!(output.streams[0].codec_name.as_deref(), Some("eac3"));
        assert_eq!(output.streams[0].channels, Some(6));
        assert_eq!(output.streams[0].tags.language.as_deref(), Some("eng"));
        assert_eq!(output.streams[1].disposition.forced, 1);
        assert_eq!(
            output.format.and_then(|format| format.duration).as_deref(),
            Some("6874.752000")
        );
    }

    #[test]
    fn selects_bounded_hevc_compatibility_transcoding() {
        let output = parse_probe_output(
            br#"{
                "streams": [{
                    "index": 0,
                    "codec_name": "hevc",
                    "codec_type": "video",
                    "bit_rate": "8000000",
                    "width": 1920,
                    "height": 1080,
                    "color_transfer": "smpte2084"
                }],
                "format": { "duration": "120", "bit_rate": "9000000" }
            }"#,
        )
        .expect("probe output");

        assert_eq!(
            select_video_bridge(&output.streams[0], output.format.as_ref()).expect("HEVC bridge"),
            VideoBridge::TranscodeToH264 {
                bitrate: 12_000_000,
                hdr_transfer: Some(HdrTransfer::Pq),
            }
        );
        assert_eq!(
            h264_transcode_bitrate(Some(100_000_000), Some(3840), Some(2160)),
            40_000_000
        );
        assert_eq!(
            h264_transcode_bitrate(None, Some(3840), Some(2160)),
            24_000_000
        );
    }

    #[test]
    fn rejects_unqualified_video_codecs() {
        let output = parse_probe_output(
            br#"{
                "streams": [{
                    "index": 0,
                    "codec_name": "av1",
                    "codec_type": "video",
                    "width": 1920,
                    "height": 1080
                }]
            }"#,
        )
        .expect("probe output");

        let error = select_video_bridge(&output.streams[0], None)
            .expect_err("AV1 must remain unsupported until qualified");
        assert!(error.summary().contains("video codec av1 is not supported"));
    }

    #[test]
    fn seeked_playback_preserves_keyframe_preroll_for_audio_and_video() {
        let arguments = playback_arguments(
            "http://127.0.0.1/media",
            Some(2),
            13.5,
            VideoBridge::CopyH264,
        );
        let noaccurate_seek = arguments
            .iter()
            .position(|argument| argument == "-noaccurate_seek")
            .expect("non-accurate seek option");
        let seek = arguments
            .iter()
            .position(|argument| argument == "-ss")
            .expect("seek option");
        let input = arguments
            .iter()
            .position(|argument| argument == "-i")
            .expect("input option");

        assert!(noaccurate_seek < seek && seek < input);
        assert!(
            arguments
                .windows(2)
                .any(|pair| { pair == ["-af".to_owned(), "aresample=async=1000".to_owned()] })
        );
    }

    #[test]
    fn playback_from_the_beginning_does_not_request_input_seeking() {
        let arguments = playback_arguments(
            "http://127.0.0.1/media",
            Some(2),
            0.0,
            VideoBridge::CopyH264,
        );

        assert!(!arguments.iter().any(|argument| argument == "-ss"));
        assert!(
            !arguments
                .iter()
                .any(|argument| argument == "-noaccurate_seek")
        );
    }

    #[test]
    fn hevc_transcoding_uses_accurate_seek_and_the_bundled_cpu_encoder() {
        let arguments = playback_arguments(
            "http://127.0.0.1/media",
            Some(2),
            13.5,
            VideoBridge::TranscodeToH264 {
                bitrate: 12_000_000,
                hdr_transfer: Some(HdrTransfer::Pq),
            },
        );

        assert!(
            !arguments
                .iter()
                .any(|argument| argument == "-noaccurate_seek")
        );
        assert!(
            arguments
                .windows(2)
                .any(|pair| pair == ["-c:v".to_owned(), "libopenh264".to_owned()])
        );
        assert!(
            arguments
                .windows(2)
                .any(|pair| pair == ["-b:v".to_owned(), "12000000".to_owned()])
        );
        assert!(
            arguments
                .windows(2)
                .any(|pair| { pair[0] == "-vf" && pair[1].contains("transferin=smpte2084") })
        );
    }
}
