//! Staging files an agent or the user publishes, for the master to pull
//! (docs/artifacts.md). Owned by the files backend; Phase 0 holds the
//! configuration every runtime is built with.

use std::path::PathBuf;

/// Where a worker node stages published files, and the data dir that is
/// never published from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageConfig {
    /// The runtime's data dir (for the embedded node, the master's): keys,
    /// secrets and databases there are refused whatever the path says.
    pub data_dir: PathBuf,
    pub stage_dir: PathBuf,
}

impl StageConfig {
    /// Stages under `<data_dir>/artifacts-stage`. Built through here, so
    /// fields added later need no change where runtimes are configured.
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            stage_dir: data_dir.join("artifacts-stage"),
            data_dir,
        }
    }
}
