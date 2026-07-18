use std::path::PathBuf;

use anyhow::{Context, bail};

use crate::{
    api::TorrentIdOrHash,
    bitv::{BitV, DiskBackedBitV},
    spawn_utils::BlockingSpawner,
    type_aliases::BF,
};

const FASTRESUME_FILENAME: &str = ".rqbit-have-pieces";

#[async_trait::async_trait]
pub trait BitVFactory: Send + Sync {
    async fn load(&self, id: TorrentIdOrHash) -> anyhow::Result<Option<Box<dyn BitV>>>;
    async fn clear(&self, id: TorrentIdOrHash) -> anyhow::Result<()>;
    async fn store_initial_check(
        &self,
        id: TorrentIdOrHash,
        b: BF,
    ) -> anyhow::Result<Box<dyn BitV>>;
}

pub struct NonPersistentBitVFactory {}

/// Persists verified-piece state without persisting torrent membership.
pub struct FolderBitVFactory {
    root: PathBuf,
    spawner: BlockingSpawner,
}

impl FolderBitVFactory {
    pub fn new(root: PathBuf, spawner: BlockingSpawner) -> Self {
        Self { root, spawner }
    }

    fn filename(&self, id: TorrentIdOrHash) -> anyhow::Result<PathBuf> {
        let TorrentIdOrHash::Hash(info_hash) = id else {
            bail!("standalone fast-resume storage requires an info hash");
        };
        Ok(self
            .root
            .join(info_hash.as_string())
            .join(FASTRESUME_FILENAME))
    }
}

#[async_trait::async_trait]
impl BitVFactory for FolderBitVFactory {
    async fn load(&self, id: TorrentIdOrHash) -> anyhow::Result<Option<Box<dyn BitV>>> {
        let filename = self.filename(id)?;
        match DiskBackedBitV::new(filename, self.spawner.clone()).await {
            Ok(bitv) => Ok(Some(bitv.into_dyn())),
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    async fn clear(&self, id: TorrentIdOrHash) -> anyhow::Result<()> {
        let filename = self.filename(id)?;
        match tokio::fs::remove_file(&filename).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error).with_context(|| format!("error removing {filename:?}")),
        }
    }

    async fn store_initial_check(
        &self,
        id: TorrentIdOrHash,
        bits: BF,
    ) -> anyhow::Result<Box<dyn BitV>> {
        let filename = self.filename(id)?;
        let directory = filename
            .parent()
            .context("fast-resume path has no parent")?;
        tokio::fs::create_dir_all(directory)
            .await
            .with_context(|| format!("error creating {directory:?}"))?;
        let temporary = filename.with_extension("tmp");
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary)
            .await
            .with_context(|| format!("error opening {temporary:?}"))?;
        tokio::io::AsyncWriteExt::write_all(&mut file, bits.as_raw_slice())
            .await
            .with_context(|| format!("error writing {temporary:?}"))?;
        tokio::io::AsyncWriteExt::flush(&mut file)
            .await
            .with_context(|| format!("error flushing {temporary:?}"))?;
        file.sync_all()
            .await
            .with_context(|| format!("error syncing {temporary:?}"))?;
        drop(file);
        tokio::fs::rename(&temporary, &filename)
            .await
            .with_context(|| format!("error renaming {temporary:?} to {filename:?}"))?;
        Ok(DiskBackedBitV::new(filename, self.spawner.clone())
            .await?
            .into_dyn())
    }
}

#[async_trait::async_trait]
impl BitVFactory for NonPersistentBitVFactory {
    async fn load(&self, _: TorrentIdOrHash) -> anyhow::Result<Option<Box<dyn BitV>>> {
        Ok(None)
    }

    async fn clear(&self, _id: TorrentIdOrHash) -> anyhow::Result<()> {
        Ok(())
    }

    async fn store_initial_check(
        &self,
        _id: TorrentIdOrHash,
        b: BF,
    ) -> anyhow::Result<Box<dyn BitV>> {
        Ok(Box::new(b))
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use librqbit_core::Id20;

    use super::{BitVFactory, FASTRESUME_FILENAME, FolderBitVFactory};
    use crate::{api::TorrentIdOrHash, spawn_utils::BlockingSpawner, type_aliases::BF};

    #[tokio::test]
    async fn standalone_fastresume_round_trips_inside_hash_directory() {
        let root = tempfile::tempdir().expect("temporary directory");
        let hash =
            Id20::from_str("18badf35b4622f33e1bdbbcf8c323ce28a6dd5b9").expect("valid info hash");
        let factory = FolderBitVFactory::new(root.path().to_path_buf(), BlockingSpawner::new(1));
        let mut bits = BF::from_boxed_slice(vec![0_u8; 2].into_boxed_slice());
        bits.set(0, true);
        bits.set(9, true);

        let stored = factory
            .store_initial_check(TorrentIdOrHash::Hash(hash), bits)
            .await
            .expect("store fast-resume state");
        drop(stored);
        let loaded = factory
            .load(TorrentIdOrHash::Hash(hash))
            .await
            .expect("load fast-resume state")
            .expect("stored state");

        assert_eq!(loaded.as_slice().count_ones(), 2);
        assert!(
            root.path()
                .join(hash.as_string())
                .join(FASTRESUME_FILENAME)
                .is_file()
        );
    }
}
