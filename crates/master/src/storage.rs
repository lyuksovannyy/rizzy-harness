//! The blob store shared by Media and Artifacts: encrypted files under the
//! data dir, quotas and sweeps (docs/media.md). Owned by the files backend.
//! Phase 0 holds the signatures the rest of the master calls.

use serde::{Deserialize, Serialize};

use crate::Master;

/// `[storage]` in config.toml; left out of the file while it is the
/// default. Sizes in MB.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StorageConfig {
    /// One upload, one published file or folder pack.
    pub max_file_mb: u64,
    /// An account's Media and Artifacts together (0: unlimited).
    pub account_quota_mb: u64,
    /// Every stored file together.
    pub server_quota_mb: u64,
    /// Free space kept on the data disk.
    pub min_free_mb: u64,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            max_file_mb: 200,
            account_quota_mb: 4096,
            server_quota_mb: 20480,
            min_free_mb: 1024,
        }
    }
}

impl StorageConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// The open blob store, in `MasterInner.storage`.
pub struct Storage {}

/// Open the blob store under `data_dir`. Called before the first node link,
/// so a replayed publish always finds it.
pub fn open(m: &Master, _data_dir: &std::path::Path) -> std::io::Result<()> {
    let _ = m.inner.storage.set(Storage {});
    Ok(())
}

/// After an account was deleted: unlink its files and checkpoint the WAL.
pub async fn on_user_deleted(_m: &Master) {}

/// The hourly part of the blob store: sweeps and retention.
pub async fn hourly(_m: &Master) {}
