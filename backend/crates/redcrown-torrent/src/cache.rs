//! Owns temporary torrent cache entries and lease-aware cleanup.
// Rust guideline compliant 2026-02-21

use std::backtrace::Backtrace;
use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use redcrown_core::StreamCachePolicy;
use serde::{Deserialize, Serialize};
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, RwLock};
use tracing::{Level, event};
use uuid::Uuid;

const MANIFEST_SCHEMA_VERSION: u32 = 1;
const MANIFEST_SLOT_A: &str = ".redcrown-cache-a.json";
const MANIFEST_SLOT_B: &str = ".redcrown-cache-b.json";

#[derive(Debug)]
pub(super) struct StreamCache {
    root: PathBuf,
    policy: RwLock<StreamCachePolicy>,
    leases: Mutex<HashMap<CacheKey, usize>>,
    operations: Mutex<()>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct CacheKey(String);

impl CacheKey {
    pub(super) fn parse(value: impl AsRef<str>) -> Result<Self, CacheError> {
        let value = value.as_ref();
        if value.len() != 40 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(CacheError::new("invalid torrent cache identifier"));
        }
        Ok(Self(value.to_ascii_lowercase()))
    }

    pub(super) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Manifest {
    schema_version: u32,
    generation: u64,
    created_unix_secs: u64,
    last_access_unix_secs: u64,
}

#[derive(Debug)]
struct Candidate {
    key: CacheKey,
    path: PathBuf,
    manifest: Manifest,
    size: u64,
}

impl StreamCache {
    pub(super) async fn open(root: PathBuf, policy: StreamCachePolicy) -> Result<Self, CacheError> {
        policy
            .validate()
            .map_err(|error| CacheError::new(error.user_message()))?;
        fs::create_dir_all(&root)
            .await
            .map_err(|error| CacheError::new(format!("failed to create stream cache: {error}")))?;
        let cache = Self {
            root,
            policy: RwLock::new(policy),
            leases: Mutex::new(HashMap::new()),
            operations: Mutex::new(()),
        };
        if let Err(error) = cache.cleanup().await {
            event!(
                name: "torrent.cache.cleanup.failed",
                Level::WARN,
                error.message = error.user_message(),
                "initial stream cache cleanup failed"
            );
        }
        Ok(cache)
    }

    pub(super) async fn acquire(&self, key: CacheKey) -> Result<(), CacheError> {
        self.acquire_at(key, SystemTime::now()).await
    }

    async fn acquire_at(&self, key: CacheKey, now: SystemTime) -> Result<(), CacheError> {
        let _operation = self.operations.lock().await;
        let entry = self.root.join(key.as_str());
        fs::create_dir_all(&entry)
            .await
            .map_err(|error| CacheError::new(format!("failed to create cache entry: {error}")))?;
        ensure_direct_child(&self.root, &entry).await?;
        let mut manifest = load_manifest(&entry).await.unwrap_or_else(|| Manifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            generation: 0,
            created_unix_secs: unix_seconds(now),
            last_access_unix_secs: unix_seconds(now),
        });
        manifest.last_access_unix_secs = unix_seconds(now);
        save_manifest(&entry, &mut manifest).await?;
        let mut leases = self.leases.lock().await;
        let count = leases.entry(key).or_insert(0);
        *count = count
            .checked_add(1)
            .ok_or_else(|| CacheError::new("stream-cache lease count exhausted"))?;
        Ok(())
    }

    pub(super) async fn release(&self, key: &CacheKey) -> Result<(), CacheError> {
        self.release_at(key, SystemTime::now()).await
    }

    async fn release_at(&self, key: &CacheKey, now: SystemTime) -> Result<(), CacheError> {
        let _operation = self.operations.lock().await;
        let entry = self.root.join(key.as_str());
        if fs::try_exists(&entry)
            .await
            .map_err(|error| CacheError::new(format!("failed to inspect cache entry: {error}")))?
        {
            ensure_direct_child(&self.root, &entry).await?;
        }
        if let Some(mut manifest) = load_manifest(&entry).await {
            manifest.last_access_unix_secs = unix_seconds(now);
            save_manifest(&entry, &mut manifest).await?;
        }
        let mut leases = self.leases.lock().await;
        match leases.get_mut(key) {
            Some(count) if *count > 1 => *count -= 1,
            Some(_) => {
                leases.remove(key);
            }
            None => {}
        }
        Ok(())
    }

    pub(super) async fn update_policy(&self, policy: StreamCachePolicy) -> Result<(), CacheError> {
        policy
            .validate()
            .map_err(|error| CacheError::new(error.user_message()))?;
        *self.policy.write().await = policy;
        if let Err(error) = self.cleanup().await {
            event!(
                name: "torrent.cache.cleanup.failed",
                Level::WARN,
                error.message = error.user_message(),
                "stream cache cleanup after policy change failed"
            );
        }
        Ok(())
    }

    pub(super) async fn cleanup(&self) -> Result<(), CacheError> {
        self.cleanup_at(SystemTime::now()).await
    }

    pub(super) async fn maintain(&self) -> Result<(), CacheError> {
        let now = SystemTime::now();
        self.touch_active_at(now).await?;
        self.cleanup_at(now).await
    }

    async fn touch_active_at(&self, now: SystemTime) -> Result<(), CacheError> {
        let _operation = self.operations.lock().await;
        let active = self.leases.lock().await.keys().cloned().collect::<Vec<_>>();
        for key in active {
            let entry = self.root.join(key.as_str());
            ensure_direct_child(&self.root, &entry).await?;
            if let Some(mut manifest) = load_manifest(&entry).await {
                manifest.last_access_unix_secs = unix_seconds(now);
                save_manifest(&entry, &mut manifest).await?;
            }
        }
        Ok(())
    }

    async fn cleanup_at(&self, now: SystemTime) -> Result<(), CacheError> {
        let _operation = self.operations.lock().await;
        let active = self
            .leases
            .lock()
            .await
            .keys()
            .cloned()
            .collect::<HashSet<_>>();
        let policy = *self.policy.read().await;
        let mut candidates = scan_candidates(&self.root, &active).await?;
        let now_secs = unix_seconds(now);
        let mut retained = Vec::with_capacity(candidates.len());

        for candidate in candidates.drain(..) {
            let idle_age = now_secs.saturating_sub(candidate.manifest.last_access_unix_secs);
            let absolute_age = now_secs.saturating_sub(candidate.manifest.created_unix_secs);
            if idle_age >= policy.idle_expiration_secs || absolute_age >= policy.maximum_age_secs {
                remove_candidate(&self.root, &candidate).await?;
            } else {
                retained.push(candidate);
            }
        }

        retained.sort_by_key(|candidate| candidate.manifest.last_access_unix_secs);
        let mut retained_bytes = retained.iter().fold(0_u64, |total, candidate| {
            total.saturating_add(candidate.size)
        });
        for candidate in retained {
            if retained_bytes <= policy.size_budget_bytes {
                break;
            }
            remove_candidate(&self.root, &candidate).await?;
            retained_bytes = retained_bytes.saturating_sub(candidate.size);
        }
        Ok(())
    }
}

async fn scan_candidates(
    root: &Path,
    active: &HashSet<CacheKey>,
) -> Result<Vec<Candidate>, CacheError> {
    let mut directory = fs::read_dir(root)
        .await
        .map_err(|error| CacheError::new(format!("failed to inspect stream cache: {error}")))?;
    let mut candidates = Vec::new();
    while let Some(entry) = directory
        .next_entry()
        .await
        .map_err(|error| CacheError::new(format!("failed to read stream cache: {error}")))?
    {
        let file_type = entry.file_type().await.map_err(|error| {
            CacheError::new(format!("failed to inspect cache entry type: {error}"))
        })?;
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        let Ok(key) = CacheKey::parse(entry.file_name().to_string_lossy()) else {
            continue;
        };
        if active.contains(&key) {
            continue;
        }
        let path = entry.path();
        let Some(manifest) = load_manifest(&path).await else {
            continue;
        };
        candidates.push(Candidate {
            key,
            size: directory_size(&path).await?,
            path,
            manifest,
        });
    }
    Ok(candidates)
}

async fn load_manifest(entry: &Path) -> Option<Manifest> {
    let mut manifests = Vec::with_capacity(2);
    for slot in [entry.join(MANIFEST_SLOT_A), entry.join(MANIFEST_SLOT_B)] {
        let Ok(bytes) = fs::read(slot).await else {
            continue;
        };
        let Ok(manifest) = serde_json::from_slice::<Manifest>(&bytes) else {
            continue;
        };
        if manifest.schema_version == MANIFEST_SCHEMA_VERSION
            && manifest.last_access_unix_secs >= manifest.created_unix_secs
        {
            manifests.push(manifest);
        }
    }
    manifests
        .into_iter()
        .max_by_key(|manifest| manifest.generation)
}

async fn save_manifest(entry: &Path, manifest: &mut Manifest) -> Result<(), CacheError> {
    manifest.generation = manifest
        .generation
        .checked_add(1)
        .ok_or_else(|| CacheError::new("cache manifest generation exhausted"))?;
    let bytes = serde_json::to_vec(manifest)
        .map_err(|error| CacheError::new(format!("failed to encode cache manifest: {error}")))?;
    let slot = entry.join(if manifest.generation.is_multiple_of(2) {
        MANIFEST_SLOT_A
    } else {
        MANIFEST_SLOT_B
    });
    let temporary = entry.join(format!(".redcrown-cache-{}.tmp", Uuid::new_v4().simple()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .await
        .map_err(|error| CacheError::new(format!("failed to stage cache manifest: {error}")))?;
    file.write_all(&bytes)
        .await
        .map_err(|error| CacheError::new(format!("failed to write cache manifest: {error}")))?;
    file.flush()
        .await
        .map_err(|error| CacheError::new(format!("failed to flush cache manifest: {error}")))?;
    file.sync_all()
        .await
        .map_err(|error| CacheError::new(format!("failed to sync cache manifest: {error}")))?;
    drop(file);
    if let Err(error) = fs::remove_file(&slot).await
        && error.kind() != std::io::ErrorKind::NotFound
    {
        let _ = fs::remove_file(&temporary).await;
        return Err(CacheError::new(format!(
            "failed to rotate cache manifest: {error}"
        )));
    }
    fs::rename(&temporary, slot)
        .await
        .map_err(|error| CacheError::new(format!("failed to commit cache manifest: {error}")))
}

async fn directory_size(root: &Path) -> Result<u64, CacheError> {
    let mut total = 0_u64;
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let mut directory = fs::read_dir(path)
            .await
            .map_err(|error| CacheError::new(format!("failed to measure cache entry: {error}")))?;
        while let Some(entry) = directory
            .next_entry()
            .await
            .map_err(|error| CacheError::new(format!("failed to measure cache entry: {error}")))?
        {
            let file_type = entry.file_type().await.map_err(|error| {
                CacheError::new(format!("failed to inspect cache file type: {error}"))
            })?;
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                pending.push(entry.path());
            } else if file_type.is_file() {
                let metadata = entry.metadata().await.map_err(|error| {
                    CacheError::new(format!("failed to measure cache file: {error}"))
                })?;
                total = total.saturating_add(metadata.len());
            }
        }
        tokio::task::yield_now().await;
    }
    Ok(total)
}

async fn remove_candidate(root: &Path, candidate: &Candidate) -> Result<(), CacheError> {
    let expected = root.join(candidate.key.as_str());
    if candidate.path != expected {
        return Err(CacheError::new(
            "refused to remove a mismatched stream-cache path",
        ));
    }
    let canonical_root = fs::canonicalize(root)
        .await
        .map_err(|error| CacheError::new(format!("failed to resolve cache root: {error}")))?;
    let canonical_entry = fs::canonicalize(&candidate.path)
        .await
        .map_err(|error| CacheError::new(format!("failed to resolve cache entry: {error}")))?;
    if canonical_entry.parent() != Some(canonical_root.as_path()) {
        return Err(CacheError::new(
            "refused to remove a path outside the stream-cache root",
        ));
    }
    fs::remove_dir_all(canonical_entry)
        .await
        .map_err(|error| CacheError::new(format!("failed to remove cache entry: {error}")))?;
    event!(
        name: "torrent.cache.entry.removed",
        Level::INFO,
        cache.bytes = candidate.size,
        "removed temporary stream cache entry"
    );
    Ok(())
}

async fn ensure_direct_child(root: &Path, entry: &Path) -> Result<(), CacheError> {
    let metadata = fs::symlink_metadata(entry)
        .await
        .map_err(|error| CacheError::new(format!("failed to inspect cache entry: {error}")))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(CacheError::new(
            "refused to use a redirected stream-cache path",
        ));
    }
    let canonical_root = fs::canonicalize(root)
        .await
        .map_err(|error| CacheError::new(format!("failed to resolve cache root: {error}")))?;
    let canonical_entry = fs::canonicalize(entry)
        .await
        .map_err(|error| CacheError::new(format!("failed to resolve cache entry: {error}")))?;
    if canonical_entry.parent() != Some(canonical_root.as_path()) {
        return Err(CacheError::new(
            "refused to use a path outside the stream-cache root",
        ));
    }
    Ok(())
}

fn unix_seconds(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

#[derive(Debug)]
pub(super) struct CacheError {
    message: String,
    backtrace: Backtrace,
}

impl CacheError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            backtrace: Backtrace::capture(),
        }
    }

    pub(super) fn user_message(&self) -> &str {
        &self.message
    }
}

impl Display for CacheError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}\n{}", self.message, self.backtrace)
    }
}

impl std::error::Error for CacheError {}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use redcrown_core::StreamCachePolicy;
    use tempfile::tempdir;
    use tokio::fs;

    use super::{CacheKey, Candidate, Manifest, StreamCache, remove_candidate};

    const KEY_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const KEY_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const GIB: u64 = 1024 * 1024 * 1024;

    fn policy() -> StreamCachePolicy {
        StreamCachePolicy {
            idle_expiration_secs: 60,
            maximum_age_secs: 120,
            size_budget_bytes: GIB / 2,
        }
    }

    #[tokio::test]
    async fn active_lease_survives_expiration() {
        let directory = tempdir().expect("directory");
        let cache = StreamCache::open(directory.path().to_path_buf(), policy())
            .await
            .expect("cache");
        let key = CacheKey::parse(KEY_A).expect("key");
        cache
            .acquire_at(key.clone(), UNIX_EPOCH + Duration::from_secs(10))
            .await
            .expect("acquire");
        cache
            .cleanup_at(UNIX_EPOCH + Duration::from_secs(1_000))
            .await
            .expect("cleanup");
        assert!(directory.path().join(KEY_A).is_dir());

        cache
            .release_at(&key, UNIX_EPOCH + Duration::from_secs(1_000))
            .await
            .expect("release");
        cache
            .cleanup_at(UNIX_EPOCH + Duration::from_secs(1_061))
            .await
            .expect("cleanup");
        assert!(!directory.path().join(KEY_A).exists());
    }

    #[tokio::test]
    async fn size_pressure_evicts_least_recently_used_entry() {
        let directory = tempdir().expect("directory");
        let cache = StreamCache::open(directory.path().to_path_buf(), policy())
            .await
            .expect("cache");
        for (key, time) in [(KEY_A, 10), (KEY_B, 20)] {
            let key = CacheKey::parse(key).expect("key");
            cache
                .acquire_at(key.clone(), UNIX_EPOCH + Duration::from_secs(time))
                .await
                .expect("acquire");
            let file = fs::File::create(directory.path().join(key.as_str()).join("media.bin"))
                .await
                .expect("file");
            file.set_len(300 * 1024 * 1024).await.expect("size");
            cache
                .release_at(&key, UNIX_EPOCH + Duration::from_secs(time))
                .await
                .expect("release");
        }
        cache
            .cleanup_at(UNIX_EPOCH + Duration::from_secs(30))
            .await
            .expect("cleanup");
        assert!(!directory.path().join(KEY_A).exists());
        assert!(directory.path().join(KEY_B).exists());
    }

    #[tokio::test]
    async fn restart_recovers_manifest_and_expires_entry() {
        let directory = tempdir().expect("directory");
        let key = CacheKey::parse(KEY_A).expect("key");
        {
            let cache = StreamCache::open(directory.path().to_path_buf(), policy())
                .await
                .expect("cache");
            cache
                .acquire_at(key.clone(), UNIX_EPOCH + Duration::from_secs(10))
                .await
                .expect("acquire");
            cache
                .release_at(&key, UNIX_EPOCH + Duration::from_secs(20))
                .await
                .expect("release");
        }
        let cache = StreamCache::open(directory.path().to_path_buf(), policy())
            .await
            .expect("reopen");
        cache
            .cleanup_at(UNIX_EPOCH + Duration::from_secs(81))
            .await
            .expect("cleanup");
        assert!(!directory.path().join(KEY_A).exists());
    }

    #[tokio::test]
    async fn unknown_directory_is_never_removed() {
        let directory = tempdir().expect("directory");
        let unknown = directory.path().join("not-redcrown-owned");
        fs::create_dir_all(&unknown).await.expect("unknown");
        let cache = StreamCache::open(directory.path().to_path_buf(), policy())
            .await
            .expect("cache");
        cache
            .cleanup_at(UNIX_EPOCH + Duration::from_secs(10_000))
            .await
            .expect("cleanup");
        assert!(unknown.exists());
    }

    #[tokio::test]
    async fn root_state_file_is_never_removed() {
        let directory = tempdir().expect("directory");
        let state_file = directory.path().join(".redcrown-dht.json");
        fs::write(&state_file, b"routing state")
            .await
            .expect("state file");
        let cache = StreamCache::open(directory.path().to_path_buf(), policy())
            .await
            .expect("cache");
        cache
            .cleanup_at(UNIX_EPOCH + Duration::from_secs(10_000))
            .await
            .expect("cleanup");
        assert_eq!(
            fs::read(&state_file).await.expect("preserved state"),
            b"routing state"
        );
    }

    #[tokio::test]
    async fn active_heartbeat_preserves_recent_cache_after_restart() {
        let directory = tempdir().expect("directory");
        let cache = StreamCache::open(directory.path().to_path_buf(), policy())
            .await
            .expect("cache");
        let key = CacheKey::parse(KEY_A).expect("key");
        cache
            .acquire_at(key.clone(), UNIX_EPOCH + Duration::from_secs(10))
            .await
            .expect("acquire");
        cache
            .touch_active_at(UNIX_EPOCH + Duration::from_secs(100))
            .await
            .expect("heartbeat");
        cache
            .release_at(&key, UNIX_EPOCH + Duration::from_secs(100))
            .await
            .expect("release");
        cache
            .cleanup_at(UNIX_EPOCH + Duration::from_secs(119))
            .await
            .expect("cleanup");
        assert!(directory.path().join(KEY_A).exists());
        cache
            .cleanup_at(UNIX_EPOCH + Duration::from_secs(130))
            .await
            .expect("absolute cleanup");
        assert!(!directory.path().join(KEY_A).exists());
    }

    #[tokio::test]
    async fn removal_rejects_mismatched_candidate_path() {
        let directory = tempdir().expect("directory");
        let outside = tempdir().expect("outside");
        let candidate = Candidate {
            key: CacheKey::parse(KEY_A).expect("key"),
            path: outside.path().to_path_buf(),
            manifest: Manifest {
                schema_version: 1,
                generation: 1,
                created_unix_secs: 1,
                last_access_unix_secs: 1,
            },
            size: 0,
        };
        assert!(
            remove_candidate(directory.path(), &candidate)
                .await
                .is_err()
        );
        assert!(outside.path().exists());
    }
}
