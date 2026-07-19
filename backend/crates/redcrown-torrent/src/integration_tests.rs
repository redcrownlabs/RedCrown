//! Verifies torrent transfer without the desktop renderer or catalog.
// Rust guideline compliant 2026-02-21

use std::error::Error;
use std::net::SocketAddr;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use librqbit::api::TorrentIdOrHash;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, BlockingSpawner, CreateTorrentOptions,
    ListenerMode, ListenerOptions, Session, SessionOptions, create_torrent,
};
use redcrown_core::{StreamCachePolicy, TrackerListConfig};
use tempfile::tempdir;
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::time::timeout;
use uuid::Uuid;

use super::{
    HdrTransfer, MediaTools, PlaybackPreparation, PlaybackStage, PreparationSnapshot,
    TorrentEngine, VideoBridge,
};

const TEST_TIMEOUT: Duration = Duration::from_secs(15);
const TEST_FILE_BYTES: usize = 1024 * 1024;
const TEST_PIECE_BYTES: u32 = 64 * 1024;
const EXTERNAL_MAGNET_TIMEOUT: Duration = Duration::from_secs(180);

#[tokio::test(flavor = "multi_thread")]
#[ignore = "run through scripts/test-magnet.ps1 with an explicit public magnet"]
async fn resolves_and_downloads_from_external_magnet() -> Result<(), Box<dyn Error>> {
    let _diagnostics = redcrown_diagnostics::Diagnostics::initialize()?;
    let magnet = std::env::var("REDCROWN_TEST_MAGNET")?;
    let metadata_timeout = std::env::var("REDCROWN_TEST_METADATA_TIMEOUT_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map_or(EXTERNAL_MAGNET_TIMEOUT, Duration::from_secs);
    let transfer_timeout = std::env::var("REDCROWN_TEST_TRANSFER_TIMEOUT_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map_or(EXTERNAL_MAGNET_TIMEOUT, Duration::from_secs);
    let client_root = tempdir()?;
    let engine = TorrentEngine::start(
        client_root.path().to_path_buf(),
        StreamCachePolicy::standard(),
        TrackerListConfig::default(),
        MediaTools::unavailable_for_transfer_test(),
    )
    .await?;

    let started = tokio::time::Instant::now();
    let listed = timeout(metadata_timeout, engine.resolve_metadata(&magnet)).await??;
    eprintln!("metadata resolved after {:?}", started.elapsed());
    let ticket = timeout(
        EXTERNAL_MAGNET_TIMEOUT,
        engine.start_resolved_playback(listed, None),
    )
    .await??;
    let handle = engine
        .api
        .mgr_handle(TorrentIdOrHash::Id(ticket.torrent_id))?;
    timeout(EXTERNAL_MAGNET_TIMEOUT, handle.wait_until_initialized()).await??;
    let client = reqwest::Client::builder().build()?;
    let mut response = client
        .get(&ticket.stream_url)
        .header(reqwest::header::RANGE, "bytes=0-262143")
        .send()
        .await?;
    let stream_status = response.status();
    if !stream_status.is_success() {
        let bytes = response.bytes().await?;
        return Err(format!(
            "loopback stream returned {stream_status}: {}",
            String::from_utf8_lossy(&bytes)
        )
        .into());
    }
    let mut received = 0_usize;
    let transfer_started = tokio::time::Instant::now();
    let deadline = transfer_started + transfer_timeout;
    while received == 0 && tokio::time::Instant::now() < deadline {
        match timeout(Duration::from_secs(10), response.chunk()).await {
            Ok(Ok(Some(chunk))) => received = received.saturating_add(chunk.len()),
            Ok(Ok(None)) => break,
            Ok(Err(error)) => return Err(error.into()),
            Err(_) => {
                let torrent_stats = handle.stats();
                if let Some(live) = torrent_stats.live {
                    eprintln!(
                        "transfer pending: progress={} peers={{queued:{}, connecting:{}, live:{}, seen:{}, dead:{}, not_needed:{}}}",
                        torrent_stats.progress_bytes,
                        live.snapshot.peer_stats.queued,
                        live.snapshot.peer_stats.connecting,
                        live.snapshot.peer_stats.live,
                        live.snapshot.peer_stats.seen,
                        live.snapshot.peer_stats.dead,
                        live.snapshot.peer_stats.not_needed,
                    );
                } else {
                    eprintln!("transfer pending: torrent is not live");
                }
            }
        }
    }

    assert!(received > 0, "the external swarm returned no media bytes");
    eprintln!(
        "first media bytes received after {:?}",
        transfer_started.elapsed()
    );
    engine.stop_playback(ticket.torrent_id).await?;
    engine.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "run through scripts/test-media.ps1 with the pinned media toolchain"]
async fn media_bridge_transcodes_audio_and_exposes_subtitles() -> Result<(), Box<dyn Error>> {
    let tools = MediaTools::from_environment()?;
    let seed_root = tempdir()?;
    let media_path = seed_root.path().join("redcrown-media-bridge.mkv");
    create_media_fixture(&tools, seed_root.path(), &media_path).await?;

    let client_root = tempdir()?;
    let engine = TorrentEngine::start_with_peer_listener(
        client_root.path().to_path_buf(),
        StreamCachePolicy::standard(),
        TrackerListConfig::default(),
        tools.clone(),
        "127.0.0.1:0".parse()?,
    )
    .await?;
    let (_seed_session, listed) = listed_from_local_seed(&engine, seed_root.path()).await?;
    let ticket = engine.start_resolved_playback(listed, None).await?;
    timeout(TEST_TIMEOUT, engine.prebuffer(&ticket)).await??;
    let ticket = engine.inspect_media(ticket).await?;

    assert_eq!(ticket.audio_tracks[0].codec, "eac3");
    assert_eq!(ticket.subtitle_tracks[0].codec, "subrip");
    let media = reqwest::get(&ticket.playback_url).await?.bytes().await?;
    let output_path = client_root.path().join("media-bridge-output.mp4");
    tokio::fs::write(&output_path, media).await?;
    let probe = probe_file(&tools, &output_path).await?;
    assert!(probe.contains("\"codec_name\": \"aac\""));
    assert!(probe.contains("\"codec_type\": \"video\""));

    let mut seek_url = reqwest::Url::parse(&ticket.playback_url)?;
    seek_url.query_pairs_mut().append_pair("start", "3.000");
    let seeked_media = reqwest::get(seek_url).await?.bytes().await?;
    let seeked_output_path = client_root.path().join("media-bridge-seek-output.mp4");
    tokio::fs::write(&seeked_output_path, seeked_media).await?;
    let video_timestamps = probe_packet_timestamps(&tools, &seeked_output_path, "v:0").await?;
    let audio_timestamps = probe_packet_timestamps(&tools, &seeked_output_path, "a:0").await?;
    let first_advancing_video = first_advancing_timestamp(&video_timestamps)?;
    let first_advancing_audio = first_advancing_timestamp(&audio_timestamps)?;
    assert!(
        (first_advancing_video - first_advancing_audio).abs() <= 0.05,
        "seeked bridge output started out of sync: video={video_timestamps:?}, audio={audio_timestamps:?}"
    );

    {
        let mut manifests = engine.media_manifests.write().await;
        let manifest = manifests
            .get_mut(&(ticket.torrent_id, ticket.file_id))
            .ok_or("media manifest missing")?;
        manifest.video_bridge = VideoBridge::TranscodeToH264 {
            bitrate: 1_000_000,
            hdr_transfer: Some(HdrTransfer::Pq),
        };
    }
    let mut transcoded_url = reqwest::Url::parse(&ticket.playback_url)?;
    transcoded_url
        .query_pairs_mut()
        .append_pair("start", "8.000");
    let transcoded_media = reqwest::get(transcoded_url).await?.bytes().await?;
    let transcoded_output_path = client_root
        .path()
        .join("media-bridge-transcoded-output.mp4");
    tokio::fs::write(&transcoded_output_path, transcoded_media).await?;
    let transcoded_probe = probe_file(&tools, &transcoded_output_path).await?;
    assert!(transcoded_probe.contains("\"codec_name\": \"h264\""));
    assert!(transcoded_probe.contains("\"codec_name\": \"aac\""));
    assert!(transcoded_probe.contains("\"color_transfer\": \"bt709\""));
    let transcoded_video_timestamps =
        probe_packet_timestamps(&tools, &transcoded_output_path, "v:0").await?;
    let transcoded_audio_timestamps =
        probe_packet_timestamps(&tools, &transcoded_output_path, "a:0").await?;
    let first_transcoded_video = first_advancing_timestamp(&transcoded_video_timestamps)?;
    let first_transcoded_audio = first_advancing_timestamp(&transcoded_audio_timestamps)?;
    assert!(
        (first_transcoded_video - first_transcoded_audio).abs() <= 0.05,
        "transcoded bridge output started out of sync: video={transcoded_video_timestamps:?}, audio={transcoded_audio_timestamps:?}"
    );

    let subtitle_url = ticket.subtitle_tracks[0]
        .stream_url
        .as_deref()
        .ok_or("subtitle URL missing")?;
    let subtitles = reqwest::get(subtitle_url).await?.text().await?;
    assert!(subtitles.contains("RedCrown subtitle fixture"));
    Ok(())
}

async fn create_media_fixture(
    tools: &MediaTools,
    root: &Path,
    output: &Path,
) -> Result<(), Box<dyn Error>> {
    let subtitles = root.join("fixture.srt");
    tokio::fs::write(
        &subtitles,
        "1\n00:00:00,250 --> 00:00:02,500\nRedCrown subtitle fixture\n",
    )
    .await?;
    let status = Command::new(&tools.ffmpeg)
        .args(["-y", "-nostdin", "-hide_banner", "-loglevel", "error"])
        .args([
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x180:rate=24,format=yuv420p,setparams=color_primaries=bt2020:color_trc=smpte2084:colorspace=bt2020nc",
        ])
        .args(["-f", "lavfi", "-i", "sine=frequency=1000:sample_rate=48000"])
        .arg("-i")
        .arg(subtitles)
        .args([
            "-t",
            "10",
            "-c:v",
            "libopenh264",
            "-g",
            "96",
            "-b:v",
            "500k",
        ])
        .args(["-c:a", "eac3", "-c:s", "srt"])
        .arg(output)
        .stdin(Stdio::null())
        .status()
        .await?;
    if !status.success() {
        return Err("FFmpeg could not create the deterministic media fixture".into());
    }
    Ok(())
}

async fn listed_from_local_seed(
    engine: &TorrentEngine,
    seed_root: &Path,
) -> Result<(Arc<Session>, librqbit::ListOnlyResponse), Box<dyn Error>> {
    let torrent = create_torrent(
        seed_root,
        CreateTorrentOptions {
            name: Some("redcrown-media-bridge"),
            trackers: Vec::new(),
            piece_length: Some(TEST_PIECE_BYTES),
        },
        &BlockingSpawner::new(1),
    )
    .await?;
    let torrent_bytes = torrent.as_bytes()?;
    let seed_session = Session::new_with_opts(
        seed_root.to_path_buf(),
        SessionOptions {
            dht: None,
            persistence: None,
            listen: Some(ListenerOptions {
                mode: ListenerMode::TcpOnly,
                listen_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
                ..ListenerOptions::default()
            }),
            ..SessionOptions::default()
        },
    )
    .await?;
    let seed_handle = seed_session
        .add_torrent(
            AddTorrent::from_bytes(torrent_bytes.clone()),
            Some(AddTorrentOptions {
                output_folder: Some(seed_root.to_string_lossy().into_owned()),
                overwrite: true,
                ..AddTorrentOptions::default()
            }),
        )
        .await?
        .into_handle()
        .ok_or("seed torrent did not return a managed handle")?;
    timeout(TEST_TIMEOUT, seed_handle.wait_until_completed()).await??;
    let seed_address = seed_session
        .listen_addr()
        .ok_or("seed session did not bind a TCP port")?;
    let response = engine
        .api
        .session()
        .add_torrent(
            AddTorrent::from_bytes(torrent_bytes),
            Some(AddTorrentOptions {
                list_only: true,
                ..AddTorrentOptions::default()
            }),
        )
        .await?;
    let mut listed = match response {
        AddTorrentResponse::ListOnly(listed) => listed,
        AddTorrentResponse::Added(_, _) | AddTorrentResponse::AlreadyManaged(_, _) => {
            return Err("metadata request unexpectedly started a torrent".into());
        }
    };
    listed.seen_peers = vec![seed_address];
    Ok((seed_session, listed))
}

async fn probe_file(tools: &MediaTools, path: &Path) -> Result<String, Box<dyn Error>> {
    let output = Command::new(&tools.ffprobe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_name,codec_type,pix_fmt,color_space,color_transfer,color_primaries",
        ])
        .args(["-of", "json"])
        .arg(path)
        .output()
        .await?;
    if !output.status.success() {
        return Err(format!(
            "FFprobe rejected media bridge output: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

async fn probe_packet_timestamps(
    tools: &MediaTools,
    path: &Path,
    stream: &str,
) -> Result<Vec<f64>, Box<dyn Error>> {
    let output = Command::new(&tools.ffprobe)
        .args(["-v", "error", "-select_streams", stream])
        .args(["-read_intervals", "%+#3"])
        .args(["-show_entries", "packet=pts_time", "-of", "csv=p=0"])
        .arg(path)
        .output()
        .await?;
    if !output.status.success() {
        return Err(format!("FFprobe could not inspect {stream} packet timestamps").into());
    }
    String::from_utf8(output.stdout)?
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| line.parse::<f64>().map_err(Into::into))
        .collect()
}

fn first_advancing_timestamp(timestamps: &[f64]) -> Result<f64, Box<dyn Error>> {
    timestamps
        .iter()
        .copied()
        .find(|timestamp| *timestamp > 0.001)
        .ok_or_else(|| "FFprobe returned no advancing packet timestamp".into())
}

#[tokio::test(flavor = "multi_thread")]
async fn downloads_prebuffers_and_serves_from_a_local_seed() -> Result<(), Box<dyn Error>> {
    let seed_root = tempdir()?;
    let media_path = seed_root.path().join("redcrown-transfer-test.mp4");
    let expected = transfer_fixture_bytes()?;
    tokio::fs::write(&media_path, &expected).await?;
    let client_root = tempdir()?;
    let engine = TorrentEngine::start_with_peer_listener(
        client_root.path().to_path_buf(),
        StreamCachePolicy::standard(),
        TrackerListConfig::default(),
        MediaTools::unavailable_for_transfer_test(),
        "127.0.0.1:0".parse()?,
    )
    .await?;
    let (seed_session, listed) = listed_from_local_seed(&engine, seed_root.path()).await?;
    let torrent_bytes = listed.torrent_bytes.clone();
    let info_hash = listed.info_hash.as_string();

    let ticket = engine.start_resolved_playback(listed, None).await?;
    timeout(TEST_TIMEOUT, engine.prebuffer(&ticket)).await??;
    let response = reqwest::get(&ticket.stream_url).await?;
    assert!(response.status().is_success());
    assert_eq!(response.bytes().await?.as_ref(), expected.as_slice());

    let preparation_id = Uuid::new_v4();
    *engine.preparation.lock().await = Some(Arc::new(PlaybackPreparation {
        id: preparation_id,
        source: "magnet:?xt=urn:btih:local-redcrown-test".to_owned(),
        snapshot: RwLock::new(PreparationSnapshot {
            stage: PlaybackStage::Ready,
            ticket: Some(ticket.clone()),
            error: None,
        }),
        task: Mutex::new(None),
    }));
    let diagnostics = engine.diagnostics(preparation_id).await?;
    assert_eq!(diagnostics.playback.downloaded_bytes, expected.len() as u64);
    assert_eq!(diagnostics.engine_state.as_deref(), Some("live"));
    assert_eq!(
        diagnostics.magnet_link.as_deref(),
        Some("magnet:?xt=urn:btih:local-redcrown-test")
    );
    assert_eq!(diagnostics.pieces.total, 16);
    assert!(diagnostics.pieces.available > 0);
    assert!(diagnostics.pieces.downloaded_this_session > 0);
    assert!(diagnostics.downloaded_this_session_bytes > 0);
    assert_eq!(diagnostics.peers.seeders, None);

    engine.cancel_preparation(preparation_id).await?;

    let fastresume = client_root
        .path()
        .join(&info_hash)
        .join(".rqbit-have-pieces");
    assert!(fastresume.is_file());
    let listed = match engine
        .api
        .session()
        .add_torrent(
            AddTorrent::from_bytes(torrent_bytes),
            Some(AddTorrentOptions {
                list_only: true,
                ..AddTorrentOptions::default()
            }),
        )
        .await?
    {
        AddTorrentResponse::ListOnly(listed) => listed,
        AddTorrentResponse::Added(_, _) | AddTorrentResponse::AlreadyManaged(_, _) => {
            return Err("cached torrent metadata unexpectedly started a transfer".into());
        }
    };
    let resumed = engine.start_resolved_playback(listed, None).await?;
    let resumed_handle = engine
        .api
        .mgr_handle(TorrentIdOrHash::Id(resumed.torrent_id))?;
    timeout(TEST_TIMEOUT, resumed_handle.wait_until_initialized()).await??;
    timeout(TEST_TIMEOUT, engine.prebuffer(&resumed)).await??;
    let resumed_preparation_id = Uuid::new_v4();
    *engine.preparation.lock().await = Some(Arc::new(PlaybackPreparation {
        id: resumed_preparation_id,
        source: "cached-test-torrent".to_owned(),
        snapshot: RwLock::new(PreparationSnapshot {
            stage: PlaybackStage::Ready,
            ticket: Some(resumed.clone()),
            error: None,
        }),
        task: Mutex::new(None),
    }));
    let resumed_diagnostics = engine.diagnostics(resumed_preparation_id).await?;
    assert_eq!(
        resumed_diagnostics.pieces.available,
        u64::from(resumed_diagnostics.pieces.total)
    );
    assert_eq!(resumed_diagnostics.pieces.downloaded_this_session, 0);
    assert_eq!(resumed_diagnostics.downloaded_this_session_bytes, 0);

    engine.cancel_preparation(resumed_preparation_id).await?;
    engine.shutdown().await?;
    seed_session.cancellation_token().cancel();
    Ok(())
}

fn transfer_fixture_bytes() -> Result<Vec<u8>, std::num::TryFromIntError> {
    (0..TEST_FILE_BYTES)
        .map(|offset| u8::try_from(offset % 251))
        .collect()
}
