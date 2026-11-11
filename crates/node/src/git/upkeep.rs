//! Keeping the shadow repository healthy: rare garbage collection (A.18)
//! and stale lock files.
//!
//! **Garbage collection.** Dropping a checkpoint's refs is cheap and stays
//! on every turn; `git gc` is the largest cost of a turn (B.3: 0.26 s at
//! 1,000 files, 3.3 s at 100,000 in a plain folder, on a slow disk). It
//! runs only when due: [`GC_EVERY`] groups dropped since the last one, or
//! git's own auto-gc thresholds for loose objects and packs; never more
//! often than [`GC_INTERVAL`]. The state is a small file in the shadow
//! (`harness-gc`), so a restart keeps it. It runs after the turn has ended
//! ([`gc_in_background`]), only while none of our commands that write
//! unreferenced objects runs there ([`hold`]: snapshots, a whole
//! restore), and prunes only objects older than its own start, so a
//! snapshot that starts while it runs keeps what it writes.
//!
//! **Lock files.** A private index and its lock go with the call that made
//! them, also at a timeout ([`super::IndexFiles`]); a lock older than any
//! command of ours can live is removed at start-up and before each
//! collection ([`sweep_stale_locks`]).

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::{GIT_TIMEOUT, ORDER, Repo, run};

/// A collection is due once this many checkpoint groups were dropped since
/// the last one. At 3.3 s for a plain folder of 100,000 files (B.3) that is
/// about 0.2 s a turn; the dropped groups' objects stay that much longer.
pub(crate) const GC_EVERY: u64 = 16;

/// Never more often than this per shadow repository.
pub(crate) const GC_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// Loose objects that make a collection due earlier (git's `gc.auto`).
const LOOSE_LIMIT: u64 = 6700;

/// Packs that make a collection due earlier (git's `gc.autoPackLimit`);
/// a restore in a repository folder adds one (its copy).
const PACK_LIMIT: u64 = 50;

/// A lock or private index older than this belongs to no running command
/// of ours: one git call is killed at [`GIT_TIMEOUT`], and the longest-lived
/// private index (a restore's) spans two calls.
pub(crate) const STALE_AFTER: Duration = Duration::from_secs(2 * GIT_TIMEOUT.as_secs() + 60);

/// The state file of the collection rule, in the shadow.
const STATE: &str = "harness-gc";

// ---- holds ----

#[derive(Default)]
struct Busy {
    holds: usize,
    collecting: bool,
}

static BUSY: std::sync::Mutex<Vec<(PathBuf, Busy)>> = std::sync::Mutex::new(Vec::new());

fn with_busy<T>(dir: &Path, f: impl FnOnce(&mut Busy) -> T) -> T {
    let mut v = BUSY.lock().unwrap_or_else(|p| p.into_inner());
    let i = match v.iter().position(|(d, _)| d == dir) {
        Some(i) => i,
        None => {
            v.push((dir.to_path_buf(), Busy::default()));
            v.len() - 1
        }
    };
    let out = f(&mut v[i].1);
    if v[i].1.holds == 0 && !v[i].1.collecting {
        v.swap_remove(i);
    }
    out
}

/// While held, no garbage collection starts in this shadow (one already
/// running goes on; it prunes nothing younger than its start).
pub struct Hold(Option<PathBuf>);

/// Hold `repo`'s shadow for a command that writes objects no ref names yet,
/// or reads a copy no `.keep` protects (a restore). Nothing for a project
/// repository.
pub fn hold(repo: &Repo) -> Hold {
    Hold(repo.shadow.clone().inspect(|d| {
        with_busy(d, |b| b.holds += 1);
    }))
}

impl Drop for Hold {
    fn drop(&mut self) {
        if let Some(d) = self.0.take() {
            with_busy(&d, |b| b.holds = b.holds.saturating_sub(1));
        }
    }
}

/// The right to collect in one shadow: only when nothing holds it and no
/// other collection runs there.
struct Collecting(PathBuf);

impl Collecting {
    fn try_start(dir: &Path) -> Option<Self> {
        with_busy(dir, |b| {
            if b.holds > 0 || b.collecting {
                return false;
            }
            b.collecting = true;
            true
        })
        .then(|| Self(dir.to_path_buf()))
    }
}

impl Drop for Collecting {
    fn drop(&mut self) {
        with_busy(&self.0, |b| b.collecting = false);
    }
}

async fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".tmp-{}", harness_core::new_id()));
    let tmp = PathBuf::from(tmp);
    tokio::fs::write(&tmp, data).await?;
    let r = tokio::fs::rename(&tmp, path).await;
    if r.is_err() {
        let _ = tokio::fs::remove_file(&tmp).await;
    }
    r
}

// ---- garbage collection (A.18) ----

/// The rule's memory: when the last collection started, and how many
/// groups were dropped since.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct GcState {
    last: u64,
    dropped: u64,
}

impl GcState {
    fn parse(text: &str) -> Self {
        let mut s = Self::default();
        for l in text.lines() {
            if let Some(v) = l.strip_prefix("last ") {
                s.last = v.trim().parse().unwrap_or(0);
            } else if let Some(v) = l.strip_prefix("dropped ") {
                s.dropped = v.trim().parse().unwrap_or(0);
            }
        }
        s
    }

    fn text(self) -> String {
        format!("last {}\ndropped {}\n", self.last, self.dropped)
    }
}

async fn read_state(dir: &Path) -> GcState {
    GcState::parse(
        &tokio::fs::read_to_string(dir.join(STATE))
            .await
            .unwrap_or_default(),
    )
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Count `n` dropped groups (the caller holds `ORDER`).
pub(super) async fn count_dropped(dir: &Path, n: u64) {
    if n == 0 {
        return;
    }
    let mut s = read_state(dir).await;
    s.dropped += n;
    let _ = write_atomic(&dir.join(STATE), s.text().as_bytes()).await;
}

/// Whether a collection is due by the rule, read from the state and, only
/// when the count of dropped groups alone does not decide, `count-objects`.
async fn due(repo: &Repo, s: GcState, now: u64) -> bool {
    if s.dropped == 0 || now.saturating_sub(s.last) < GC_INTERVAL.as_secs() {
        return false;
    }
    if s.dropped >= GC_EVERY {
        return true;
    }
    let Ok(out) = run(repo, &["count-objects", "-v"], None).await else {
        return false;
    };
    let field = |k: &str| -> u64 {
        out.lines()
            .find_map(|l| l.strip_prefix(k))
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0)
    };
    field("count:") >= LOOSE_LIMIT || field("packs:") >= PACK_LIMIT
}

/// Run the shadow's garbage collection if the rule says it is due and
/// nothing holds the shadow; whether it ran. Never in a project repository.
/// It prunes only objects older than its own start (`--prune=<start - 1>`,
/// seconds since the epoch: git compares whole seconds), so a snapshot that
/// starts meanwhile keeps the objects it writes; the packs it removes are
/// those that existed when it started, so a copy made meanwhile stays.
pub(crate) async fn maybe_gc(repo: &Repo) -> bool {
    let Some(dir) = repo.shadow.clone() else {
        return false;
    };
    let start = now_secs();
    let seen = read_state(&dir).await;
    if !due(repo, seen, start).await {
        return false;
    }
    let Some(_running) = Collecting::try_start(&dir) else {
        return false;
    };
    let d = dir.clone();
    let _ = tokio::task::spawn_blocking(move || sweep_shadow(&d, STALE_AFTER)).await;
    let res = run(
        repo,
        &[
            "gc",
            &format!("--prune={}", start.saturating_sub(1)),
            "--quiet",
        ],
        None,
    )
    .await;
    let _g = ORDER.lock().await;
    let mut s = read_state(&dir).await;
    // A failed collection (a kept checkpoint reaches an object the user's
    // repository lost) is retried after the interval, not every turn.
    s.last = start;
    if res.is_ok() {
        s.dropped = s.dropped.saturating_sub(seen.dropped);
    }
    let _ = write_atomic(&dir.join(STATE), s.text().as_bytes()).await;
    if let Err(e) = res {
        tracing::warn!(error = %e, shadow = %dir.display(), "checkpoint garbage collection failed");
    }
    true
}

/// Start [`maybe_gc`] for `repo` in its own task: after a turn has ended
/// (its result and `turn_ended` are not held back) or a restore has
/// finished.
pub fn gc_in_background(repo: Repo) {
    if repo.is_shadow() {
        tokio::spawn(async move {
            maybe_gc(&repo).await;
        });
    }
}

// ---- lock files (A.18) ----

/// Remove stale lock files and private indexes in every shadow under
/// `base` (start-up; see [`STALE_AFTER`]). Returns how many went.
pub fn sweep_stale_locks(base: &Path) -> usize {
    let Ok(rd) = std::fs::read_dir(base) else {
        return 0;
    };
    rd.filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "git") && p.join("HEAD").is_file())
        .map(|p| sweep_shadow(&p, STALE_AFTER))
        .sum()
}

/// In one shadow: `*.lock` at its top and under `refs/`, and our private
/// indexes (`harness-index-*`), each only if a regular file older than
/// `older`. A younger one may belong to a running command and stays.
fn sweep_shadow(dir: &Path, older: Duration) -> usize {
    let now = SystemTime::now();
    let stale = |p: &Path| {
        std::fs::symlink_metadata(p).is_ok_and(|m| {
            m.is_file()
                && m.modified()
                    .ok()
                    .and_then(|t| now.duration_since(t).ok())
                    .is_some_and(|age| age > older)
        })
    };
    let mut gone = 0;
    let mut try_remove = |p: &Path| {
        if stale(p) && std::fs::remove_file(p).is_ok() {
            tracing::info!(file = %p.display(), "stale checkpoint lock removed");
            gone += 1;
        }
    };
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.filter_map(|e| e.ok()) {
            let name = e.file_name();
            let n = name.to_string_lossy();
            if n.ends_with(".lock") || n.starts_with("harness-index-") {
                try_remove(&e.path());
            }
        }
    }
    for e in walkdir::WalkDir::new(dir.join("refs"))
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if e.file_type().is_file() && e.file_name().to_string_lossy().ends_with(".lock") {
            try_remove(e.path());
        }
    }
    gone
}

#[cfg(test)]
mod tests {
    // ---- the functions ----

    use super::super::tests::fake_git;
    use super::super::{TEST_GIT, repo_for, snapshot, snapshot_within};
    use super::*;

    /// Make a collection due by the count of dropped groups.
    async fn make_due(repo: &Repo) {
        count_dropped(repo.shadow.as_ref().unwrap(), GC_EVERY).await;
    }

    /// A.18: the rule. Not before `GC_EVERY` dropped groups (a small store
    /// is not due by its own counts), not while something holds the shadow,
    /// then once, and not again within `GC_INTERVAL`.
    #[tokio::test]
    async fn the_rule_collects_once_per_many_drops_and_not_twice_within_the_interval() {
        let d = tempfile::tempdir().unwrap();
        let ws = d.path().join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(ws.join("a.txt"), "a\n").unwrap();
        let repo = repo_for(&ws, Some(&d.path().join("cp"))).await.unwrap();
        snapshot(&repo, "refs/harness/s/t/before", "b")
            .await
            .unwrap();
        let dir = repo.shadow.clone().unwrap();
        count_dropped(&dir, GC_EVERY - 1).await;
        assert!(!maybe_gc(&repo).await, "ran before it was due");
        count_dropped(&dir, 1).await;
        let held = hold(&repo);
        assert!(
            !maybe_gc(&repo).await,
            "ran while a snapshot held the shadow"
        );
        drop(held);
        assert!(maybe_gc(&repo).await, "did not run when due");
        let s = read_state(&dir).await;
        assert_eq!(s.dropped, 0);
        assert!(now_secs() - s.last < 60, "{s:?}");
        count_dropped(&dir, GC_EVERY).await;
        assert!(!maybe_gc(&repo).await, "ran twice within the interval");
        // After a restart: the state is in the shadow.
        assert_eq!(read_state(&dir).await.dropped, GC_EVERY);
    }

    /// The collection prunes an unreferenced object only if it is older
    /// than the collection's start (git reads `--prune=<seconds>` as seconds
    /// since the epoch), so what a snapshot running meanwhile writes stays.
    #[tokio::test]
    async fn the_collection_keeps_objects_younger_than_its_start() {
        let d = tempfile::tempdir().unwrap();
        let ws = d.path().join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(ws.join("a.txt"), "a\n").unwrap();
        let repo = repo_for(&ws, Some(&d.path().join("cp"))).await.unwrap();
        snapshot(&repo, "refs/harness/s/t/before", "b")
            .await
            .unwrap();
        let write = |text: &'static str| {
            let repo = repo.clone();
            async move {
                let out = super::super::run_raw(
                    &repo,
                    &["hash-object", "-w", "--stdin"],
                    None,
                    Some(text.as_bytes().to_vec()),
                )
                .await
                .unwrap();
                String::from_utf8(out).unwrap().trim().to_string()
            }
        };
        let old = write("unreferenced, an hour old\n").await;
        let young = write("unreferenced, just written\n").await;
        let path = |id: &str| {
            repo.shadow
                .clone()
                .unwrap()
                .join(format!("objects/{}/{}", &id[..2], &id[2..]))
        };
        let hour_ago = SystemTime::now() - Duration::from_secs(3600);
        // Loose objects are read-only; the owner may still set the times.
        std::fs::File::open(path(&old))
            .unwrap()
            .set_modified(hour_ago)
            .unwrap();
        make_due(&repo).await;
        assert!(maybe_gc(&repo).await);
        let has = |id: String| {
            let repo = repo.clone();
            async move { run(&repo, &["cat-file", "-e", &id], None).await.is_ok() }
        };
        assert!(!has(old).await, "the old unreferenced object stayed");
        assert!(has(young).await, "a young object was pruned");
    }

    /// A.18: a snapshot killed at its time limit while git holds the lock
    /// on its private index (a stand-in `git` takes the lock in `add` and
    /// hangs) leaves neither the index nor its lock, and the next snapshot
    /// works.
    #[tokio::test]
    async fn a_timed_out_snapshot_leaves_no_lock_and_the_next_one_works() {
        let d = tempfile::tempdir().unwrap();
        let ws = d.path().join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(ws.join("a.txt"), "a\n").unwrap();
        let repo = repo_for(&ws, Some(&d.path().join("cp"))).await.unwrap();
        snapshot(&repo, "refs/harness/s/t/before", "b")
            .await
            .unwrap();
        let path = std::env::var("PATH").unwrap_or_default();
        let real = std::env::split_paths(&path)
            .map(|d| d.join("git"))
            .find(|g| g.is_file())
            .expect("git");
        let bin = d.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let fake = fake_git(
            &bin,
            &format!(
                "for a in \"$@\"; do if [ \"$a\" = add ]; then : > \"$GIT_INDEX_FILE.lock\"; \
                 sleep 30; fi; done\nexec '{}' \"$@\"",
                real.display()
            ),
        );
        // See `exec_script`: a fork elsewhere may hold the script open for
        // a moment after it was written.
        tokio::time::sleep(Duration::from_millis(200)).await;
        std::fs::write(ws.join("a.txt"), "changed\n").unwrap();
        TEST_GIT.with(|g| *g.borrow_mut() = Some(fake));
        let e = snapshot_within(
            &repo,
            "refs/harness/s/u/before",
            "b",
            Duration::from_secs(2),
        )
        .await;
        TEST_GIT.with(|g| *g.borrow_mut() = None);
        let e = e.unwrap_err();
        assert!(e.contains("too long"), "{e}");
        let shadow = repo.shadow.clone().unwrap();
        let left: Vec<String> = std::fs::read_dir(&shadow)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("harness-index-") || n.ends_with(".lock"))
            .collect();
        assert!(left.is_empty(), "left behind: {left:?}");
        snapshot(&repo, "refs/harness/s/v/before", "b")
            .await
            .unwrap();
    }

    /// A.18: the sweep at start-up removes lock files and private indexes
    /// older than any command of ours can live, and leaves young ones (a
    /// running command's) and everything that is not a lock.
    #[test]
    fn stale_locks_go_and_young_ones_stay() {
        let d = tempfile::tempdir().unwrap();
        let shadow = d.path().join("abc.git");
        std::fs::create_dir_all(shadow.join("refs/harness/s/t")).unwrap();
        std::fs::create_dir_all(shadow.join("objects/pack")).unwrap();
        std::fs::write(shadow.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        let old = SystemTime::now() - STALE_AFTER - Duration::from_secs(60);
        let make = |rel: &str, aged: bool| {
            let p = shadow.join(rel);
            std::fs::write(&p, "").unwrap();
            if aged {
                std::fs::OpenOptions::new()
                    .write(true)
                    .open(&p)
                    .unwrap()
                    .set_modified(old)
                    .unwrap();
            }
        };
        make("harness-index-1.lock", true);
        make("harness-index-1", true);
        make("packed-refs.lock", true);
        make("refs/harness/s/t/after.lock", true);
        make("harness-index-2.lock", false);
        make("index.lock", false);
        make("config", true);
        make("objects/pack/pack-1.keep", true);
        assert_eq!(sweep_stale_locks(d.path()), 4);
        for gone in [
            "harness-index-1.lock",
            "harness-index-1",
            "packed-refs.lock",
            "refs/harness/s/t/after.lock",
        ] {
            assert!(!shadow.join(gone).exists(), "{gone} stayed");
        }
        for kept in [
            "harness-index-2.lock",
            "index.lock",
            "config",
            "objects/pack/pack-1.keep",
        ] {
            assert!(shadow.join(kept).exists(), "{kept} was removed");
        }
    }
}
