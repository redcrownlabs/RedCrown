//! Verifies torrent transfer without the desktop renderer or catalog.
// Rust guideline compliant 2026-02-21

use std::error::Error;
use std::net::SocketAddr;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, CreateTorrentOptions, Session,
    SessionOptions, create_torrent,
};
use redcrown_core::StreamCachePolicy;
use tempfile::tempdir;
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::time::timeout;
use uuid::Uuid;

use super::{MediaTools, PlaybackPreparation, PlaybackStage, PreparationSnapshot, TorrentEngine};

const TEST_TIMEOUT: Duration = Duration::from_secs(15);
const TEST_FILE_BYTES: usize = 1024 * 1024;
const TEST_PIECE_BYTES: u32 = 64 * 1024;

#[tokio::test(flavor = "multi_thread")]
#[ignore = "run through scripts/test-media.ps1 with the pinned media toolchain"]
async fn media_bridge_transcodes_audio_and_exposes_subtitles() -> Result<(), Box<dyn Error>> {
    let tools = MediaTools::from_environment()?;
    let seed_root = tempdir()?;
    let media_path = seed_root.path().join("redcrown-media-bridge.mkv");
    create_media_fixture(&tools, seed_root.path(), &media_path).await?;

    let client_root = tempdir()?;
    let engine = TorrentEngine::start(
        client_root.path().to_path_buf(),
        StreamCachePolicy::standard(),
        tools.clone(),
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
        .args(["-f", "lavfi", "-i", "testsrc2=size=320x180:rate=24"])
        .args(["-f", "lavfi", "-i", "sine=frequency=1000:sample_rate=48000"])
        .arg("-i")
        .arg(subtitles)
        .args(["-t", "3", "-c:v", "libopenh264", "-b:v", "500k"])
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
            piece_length: Some(TEST_PIECE_BYTES),
        },
    )
    .await?;
    let torrent_bytes = torrent.as_bytes()?;
    let seed_session = Session::new_with_opts(
        seed_root.to_path_buf(),
        SessionOptions {
            disable_dht: true,
            enable_upnp_port_forwarding: false,
            persistence: None,
            listen_port_range: Some(16_301..16_401),
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
    let seed_address = SocketAddr::from((
        [127, 0, 0, 1],
        seed_session
            .tcp_listen_port()
            .ok_or("seed session did not bind a TCP port")?,
    ));
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
            "stream=codec_name,codec_type",
        ])
        .args(["-of", "json"])
        .arg(path)
        .output()
        .await?;
    if !output.status.success() {
        return Err("FFprobe rejected media bridge output".into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

#[tokio::test(flavor = "multi_thread")]
async fn downloads_prebuffers_and_serves_from_a_local_seed() -> Result<(), Box<dyn Error>> {
    let seed_root = tempdir()?;
    let media_path = seed_root.path().join("redcrown-transfer-test.mp4");
    let expected = (0..TEST_FILE_BYTES)
        .map(|offset| u8::try_from(offset % 251))
        .collect::<Result<Vec<_>, _>>()?;
    tokio::fs::write(&media_path, &expected).await?;
    let torrent = create_torrent(
        seed_root.path(),
        CreateTorrentOptions {
            name: Some("redcrown-transfer-test"),
            piece_length: Some(TEST_PIECE_BYTES),
        },
    )
    .await?;
    let torrent_bytes = torrent.as_bytes()?;

    let seed_session = Session::new_with_opts(
        seed_root.path().to_path_buf(),
        SessionOptions {
            disable_dht: true,
            enable_upnp_port_forwarding: false,
            persistence: None,
            listen_port_range: Some(16_201..16_301),
            ..SessionOptions::default()
        },
    )
    .await?;
    let seed_handle = seed_session
        .add_torrent(
            AddTorrent::from_bytes(torrent_bytes.clone()),
            Some(AddTorrentOptions {
                output_folder: Some(seed_root.path().to_string_lossy().into_owned()),
                overwrite: true,
                ..AddTorrentOptions::default()
            }),
        )
        .await?
        .into_handle()
        .ok_or("seed torrent did not return a managed handle")?;
    timeout(TEST_TIMEOUT, seed_handle.wait_until_completed()).await??;
    let seed_address = SocketAddr::from((
        [127, 0, 0, 1],
        seed_session
            .tcp_listen_port()
            .ok_or("seed session did not bind a TCP port")?,
    ));

    let client_root = tempdir()?;
    let engine = TorrentEngine::start(
        client_root.path().to_path_buf(),
        StreamCachePolicy::standard(),
        MediaTools::unavailable_for_transfer_test(),
    )
    .await?;
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
    assert!(diagnostics.pieces.verified > 0);
    assert_eq!(diagnostics.peers.seeders, None);

    engine.cancel_preparation(preparation_id).await?;
    engine.shutdown().await?;
    seed_session.cancellation_token().cancel();
    Ok(())
}
