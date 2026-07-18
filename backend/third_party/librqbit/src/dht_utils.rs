use std::{
    collections::{HashSet, VecDeque},
    net::SocketAddr,
    sync::Arc,
};

use anyhow::Context;
use buffers::ByteBufOwned;
use futures::{Stream, StreamExt, stream::FuturesUnordered};
use librqbit_core::torrent_metainfo::TorrentMetaV1Info;
use tracing::{Instrument, debug, debug_span};

use crate::{
    peer_connection::PeerConnectionOptions, peer_info_reader, spawn_utils::BlockingSpawner,
    stream_connect::StreamConnector,
};
use librqbit_core::hash_id::Id20;

/// Maximum simultaneous metadata handshakes.
///
/// Keeping the pending set bounded prevents stale tracker peers from placing
/// newly discovered DHT peers behind thousands of tasks waiting for a permit.
const MAX_CONCURRENT_METADATA_PEERS: usize = 256;
/// Maximum discovered peers retained while all handshake slots are occupied.
///
/// Discovery streams can contain thousands of stale addresses. Retaining only
/// the newest candidates bounds memory and favors recent tracker/DHT answers.
const MAX_QUEUED_METADATA_PEERS: usize = 4_096;

#[derive(Debug)]
struct MetadataCandidates {
    queued: VecDeque<SocketAddr>,
}

impl MetadataCandidates {
    fn new(initial: Vec<SocketAddr>) -> Self {
        Self {
            queued: VecDeque::from(initial),
        }
    }

    fn push_recent(&mut self, addr: SocketAddr) {
        if self.queued.len() == MAX_QUEUED_METADATA_PEERS {
            self.queued.pop_front();
        }
        self.queued.push_back(addr);
    }

    fn pop_recent(&mut self) -> Option<SocketAddr> {
        self.queued.pop_back()
    }

    fn is_empty(&self) -> bool {
        self.queued.is_empty()
    }
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum ReadMetainfoResult<Rx> {
    Found {
        info: TorrentMetaV1Info<ByteBufOwned>,
        info_bytes: ByteBufOwned,
        source: SocketAddr,
        rx: Rx,
        seen: HashSet<SocketAddr>,
    },
    ChannelClosed {
        #[allow(dead_code)]
        seen: HashSet<SocketAddr>,
    },
}

pub async fn read_metainfo_from_peer_receiver<A: Stream<Item = SocketAddr> + Unpin>(
    peer_id: Id20,
    info_hash: Id20,
    initial_addrs: Vec<SocketAddr>,
    addrs_stream: A,
    peer_connection_options: Option<PeerConnectionOptions>,
    connector: Arc<StreamConnector>,
    client_name_and_version: String,
) -> ReadMetainfoResult<A> {
    let mut seen = HashSet::<SocketAddr>::new();
    let mut addrs = addrs_stream;

    let read_info = |addr| {
        let connector = connector.clone();
        let client_name_and_version = client_name_and_version.clone();
        async move {
            let (info, info_bytes) = peer_info_reader::read_metainfo_from_peer(
                addr,
                peer_id,
                info_hash,
                peer_connection_options,
                // This shouldn't be called anyway as we aren't reading/writing to disk, so it's
                // ok not to use a shared one.
                BlockingSpawner::new(1),
                connector,
                client_name_and_version,
            )
            .instrument(debug_span!("read_metainfo_from_peer", ?addr))
            .await
            .with_context(|| format!("error reading metainfo from {addr}"))?;
            Ok::<_, anyhow::Error>((addr, info, info_bytes))
        }
    };

    let mut unordered = FuturesUnordered::new();
    let mut candidates = MetadataCandidates::new(initial_addrs);

    let mut addrs_completed = false;

    loop {
        while unordered.len() < MAX_CONCURRENT_METADATA_PEERS {
            let Some(addr) = candidates.pop_recent() else {
                break;
            };
            if seen.insert(addr) {
                unordered.push(read_info(addr));
            }
        }

        if addrs_completed && candidates.is_empty() && unordered.is_empty() {
            return ReadMetainfoResult::ChannelClosed { seen };
        }

        tokio::select! {
            done = unordered.next(), if !unordered.is_empty() => {
                match done {
                    Some(Ok((source, info, info_bytes))) => return ReadMetainfoResult::Found { info, info_bytes, source, seen, rx: addrs },
                    Some(Err(e)) => {
                        debug!("{:#}", e);
                    },
                    None => unreachable!()
                }
            }

            next_addr = addrs.next(), if !addrs_completed => {
                match next_addr {
                    Some(addr) => {
                        if seen.insert(addr) {
                            if unordered.len() < MAX_CONCURRENT_METADATA_PEERS {
                                unordered.push(read_info(addr));
                            } else {
                                candidates.push_recent(addr);
                            }
                        }
                        continue;
                    },
                    None => {
                        addrs_completed = true;
                    },
                }
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use dht::{DhtBuilder, Id20};
    use librqbit_core::peer_id::generate_peer_id;

    use super::*;
    use std::{
        str::FromStr,
        sync::{Arc, Once},
    };

    static LOG_INIT: Once = Once::new();

    #[test]
    fn metadata_candidates_are_bounded_and_newest_first() {
        let mut candidates = MetadataCandidates::new(Vec::new());
        for port in 1..=MAX_QUEUED_METADATA_PEERS + 1 {
            candidates.push_recent(SocketAddr::from((
                [127, 0, 0, 1],
                u16::try_from(port).expect("test port fits u16"),
            )));
        }

        assert_eq!(candidates.queued.len(), MAX_QUEUED_METADATA_PEERS);
        assert_eq!(
            candidates.pop_recent(),
            Some(SocketAddr::from((
                [127, 0, 0, 1],
                u16::try_from(MAX_QUEUED_METADATA_PEERS + 1).expect("test port fits u16"),
            )))
        );
        assert!(!candidates.queued.iter().any(|addr| addr.port() == 1));
    }

    fn init_logging() {
        #[allow(unused_must_use)]
        LOG_INIT.call_once(|| {
            // pretty_env_logger::try_init();
        })
    }

    #[tokio::test]
    #[ignore]
    async fn read_metainfo_from_dht() {
        init_logging();

        let info_hash = Id20::from_str("cab507494d02ebb1178b38f2e9d7be299c86b862").unwrap();
        let dht = DhtBuilder::new().await.unwrap();

        let peer_rx = dht.get_peers(info_hash, None);
        let peer_id = generate_peer_id(b"-xx1234-");
        match read_metainfo_from_peer_receiver(
            peer_id,
            info_hash,
            Vec::new(),
            peer_rx,
            None,
            Arc::new(StreamConnector::new(Default::default()).await.unwrap()),
            crate::client_name_and_version().to_owned(),
        )
        .await
        {
            ReadMetainfoResult::Found { info, .. } => dbg!(info),
            ReadMetainfoResult::ChannelClosed { .. } => todo!("should not have happened"),
        };
    }
}
