//! Keeping the shadow repository healthy: pinned copies (A.17), rare
//! garbage collection (A.18), stale lock files.
//!
//! **Pinned copies.** In a repository folder a checkpoint borrows what the
//! user's repository has, so a history rewrite plus a prune there could
//! take the content an undo needs. At the end of each turn the objects an
//! undo of that turn reads are copied into the shadow's own store: the old
//! version of every file the turn changed or deleted, and the trees on the
//! way to them in both snapshots (a diff of the turn reads exactly those).
//! Never the whole tree. What git's gc does to such a copy was established
//! by test (below): a copy of an object the alternate has packed is dropped
//! by the shadow's gc, so the copy is a pack with a `.keep` file, which gc
//! and repack leave alone. The `.keep` file names the turn's checkpoint
//! group; when [`super::prune`] drops the group the line goes, and with
//! the last line the file, and the next collection drops the copy.
//!
//! **Garbage collection.** Dropping a checkpoint's refs is cheap and stays
//! on every turn; `git gc` is the largest cost of a turn (B.3: 0.26 s at
//! 1,000 files, 3.3 s at 100,000 in a plain folder, on a slow disk). It
//! runs only when due: [`GC_EVERY`] groups dropped since the last one, or
//! git's own auto-gc thresholds for loose objects and packs; never more
//! often than [`GC_INTERVAL`]. The state is a small file in the shadow
//! (`harness-gc`), so a restart keeps it. It runs after the turn has ended
//! ([`gc_in_background`]), only while none of our commands that write
//! unreferenced objects runs there ([`hold`]: snapshots, pinning, a whole
//! restore), and prunes only objects older than its own start, so a
//! snapshot that starts while it runs keeps what it writes.
//!
//! **Lock files.** A private index and its lock go with the call that made
//! them, also at a timeout ([`super::IndexFiles`]); a lock older than any
//! command of ours can live is removed at start-up and before each
//! collection ([`sweep_stale_locks`]).

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::{GIT_TIMEOUT, Names, ORDER, Repo, borrows, copy_into_own_store, run, run_raw};

/// A collection is due once this many checkpoint groups were dropped since
/// the last one. At 3.3 s for a plain folder of 100,000 files (B.3) that is
/// about 0.2 s a turn; the dropped groups' objects stay that much longer.
pub(crate) const GC_EVERY: u64 = 16;

/// Never more often than this per shadow repository.
pub(crate) const GC_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// Loose objects that make a collection due earlier (git's `gc.auto`).
const LOOSE_LIMIT: u64 = 6700;

/// Packs that make a collection due earlier (git's `gc.autoPackLimit`);
/// each turn in a repository folder adds one (its copies).
const PACK_LIMIT: u64 = 50;

/// A lock or private index older than this belongs to no running command
/// of ours: one git call is killed at [`GIT_TIMEOUT`], and the longest-lived
/// private index (a restore's) spans two calls.
pub(crate) const STALE_AFTER: Duration = Duration::from_secs(2 * GIT_TIMEOUT.as_secs() + 60);

/// The line prefix in a `.keep` file that marks it as ours.
const PIN: &str = "harness-pin ";

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

// ---- pinned copies (A.17) ----

/// What [`pin_changed`] could not copy.
#[derive(Debug, Default)]
pub struct Pinned {
    /// Files whose old version was already gone (sorted, for showing).
    pub missing: Names,
}

/// Copy into the shadow's own store what an undo of the change from
/// `before` to `after` reads, protected by a `.keep` file naming `group`
/// (`refs/harness/<session>/<turn>`): the old version of every file that
/// changed or was deleted, and both snapshots' trees on the way to the
/// changes. Nothing in a folder that borrows nothing (every object is the
/// shadow's own and gc keeps what a ref reaches). No folder scan and no gc:
/// one tree diff, one existence check, one `pack-objects` of exactly those
/// objects. Objects that are already gone are reported, not an error.
pub async fn pin_changed(
    repo: &Repo,
    group: &str,
    before: &str,
    after: &str,
) -> Result<Pinned, String> {
    if !borrows(repo) {
        return Ok(Pinned::default());
    }
    let _busy = hold(repo);
    let roots = run(
        repo,
        &[
            "rev-parse",
            &format!("{before}^{{tree}}"),
            &format!("{after}^{{tree}}"),
        ],
        None,
    )
    .await?;
    let out = run_raw(
        repo,
        &[
            "diff-tree",
            "-r",
            "-t",
            "-z",
            "--raw",
            "--no-renames",
            "--no-abbrev",
            before,
            after,
        ],
        None,
        None,
    )
    .await?;
    let zero = |id: &str| id.bytes().all(|b| b == b'0');
    let mut ids: Vec<String> = roots.lines().map(str::to_string).collect();
    // The old blob of each changed path, and the path to name if it is gone.
    let mut blobs: Vec<(String, Vec<u8>)> = Vec::new();
    for r in super::parse_raw(&out) {
        if r.old_mode == "040000" && !zero(&r.old_id) {
            ids.push(r.old_id.clone());
        }
        if r.new_mode == "040000" && !zero(&r.new_id) {
            ids.push(r.new_id.clone());
        }
        let blob = matches!(r.old_mode.as_str(), "100644" | "100755" | "120000");
        if blob && !zero(&r.old_id) {
            blobs.push((r.old_id.clone(), r.path.clone()));
        }
    }
    ids.extend(blobs.iter().map(|(id, _)| id.clone()));
    ids.sort_unstable();
    ids.dedup();
    let list: Vec<u8> = ids
        .iter()
        .flat_map(|i| format!("{i}\n").into_bytes())
        .collect();
    let checked = run_raw(
        repo,
        &["cat-file", "--batch-check=%(objectname) %(objecttype)"],
        None,
        Some(list),
    )
    .await?;
    let text = String::from_utf8_lossy(&checked);
    let present: Vec<&str> = text
        .lines()
        .filter(|l| !l.ends_with(" missing"))
        .filter_map(|l| l.split(' ').next())
        .collect();
    let mut missing: Vec<Vec<u8>> = blobs
        .iter()
        .filter(|(id, _)| !present.contains(&id.as_str()))
        .map(|(_, p)| p.clone())
        .collect();
    missing.sort();
    missing.dedup();
    if !present.is_empty() {
        let list: Vec<u8> = present
            .iter()
            .flat_map(|i| format!("{i}\n").into_bytes())
            .collect();
        let name = copy_into_own_store(repo, list, false).await?;
        keep_for(repo, &name, group).await?;
    }
    Ok(Pinned {
        missing: Names::of(&missing),
    })
}

/// Add `group` to the `.keep` file of pack `name` (made if absent; two
/// turns that copy the same objects get the same pack name).
async fn keep_for(repo: &Repo, name: &str, group: &str) -> Result<(), String> {
    let Some(dir) = &repo.shadow else {
        return Ok(());
    };
    if name.is_empty() || !name.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("pack-objects named no pack ({name:?})"));
    }
    let path = dir.join(format!("objects/pack/pack-{name}.keep"));
    let _g = ORDER.lock().await;
    let mut text = tokio::fs::read_to_string(&path).await.unwrap_or_default();
    let line = format!("{PIN}{group}");
    if !text.lines().any(|l| l == line) {
        text.push_str(&line);
        text.push('\n');
        write_atomic(&path, text.as_bytes())
            .await
            .map_err(|e| format!("could not keep the copies: {e}"))?;
    }
    Ok(())
}

/// Release the copies of every group not in `kept` (the caller holds
/// `ORDER`): its line goes from each of our `.keep` files, and a file left
/// with none is removed, so the next collection drops those copies. A
/// `.keep` file with any line that is not ours is never touched.
pub(super) async fn release_pins(dir: &Path, kept: &[String]) {
    let packs = dir.join("objects/pack");
    let Ok(mut rd) = tokio::fs::read_dir(&packs).await else {
        return;
    };
    while let Ok(Some(e)) = rd.next_entry().await {
        let path = e.path();
        if path.extension().is_none_or(|x| x != "keep") {
            continue;
        }
        let Ok(text) = tokio::fs::read_to_string(&path).await else {
            continue;
        };
        if text.lines().any(|l| !l.starts_with(PIN)) {
            continue;
        }
        let left: Vec<&str> = text
            .lines()
            .filter(|l| kept.iter().any(|k| l.strip_prefix(PIN) == Some(k.as_str())))
            .collect();
        if left.is_empty() {
            let _ = tokio::fs::remove_file(&path).await;
        } else if left.len() != text.lines().count() {
            let mut t = left.join("\n");
            t.push('\n');
            let _ = write_atomic(&path, t.as_bytes()).await;
        }
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
    use std::path::Path;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) -> std::process::Output {
        Command::new("git")
            .arg("-c")
            .arg("gc.auto=0")
            .args(args)
            .current_dir(dir)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .output()
            .unwrap()
    }

    fn ok(dir: &Path, args: &[&str]) -> String {
        let o = git(dir, args);
        assert!(o.status.success(), "git {args:?}: {o:?}");
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }

    fn ok_in(dir: &Path, args: &[&str], input: &str) -> String {
        use std::io::Write;
        let mut c = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        c.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
        let o = c.wait_with_output().unwrap();
        assert!(o.status.success(), "git {args:?}: {o:?}");
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }

    /// A user repository and a bare shadow borrowing from it, with one ref
    /// in the shadow to a commit of the user's tree.
    struct Pair {
        _d: tempfile::TempDir,
        user: std::path::PathBuf,
        shadow: std::path::PathBuf,
        x: String,
    }

    impl Pair {
        fn new() -> Self {
            let d = tempfile::tempdir().unwrap();
            let user = d.path().join("user");
            let shadow = d.path().join("sh.git");
            std::fs::create_dir(&user).unwrap();
            ok(&user, &["init", "-q", "-b", "main"]);
            std::fs::write(user.join("keep.txt"), "X\n").unwrap();
            std::fs::write(user.join("other.txt"), "base\n").unwrap();
            ok(&user, &["add", "."]);
            ok(&user, &["commit", "-qm", "c"]);
            ok(&user, &["gc", "-q"]);
            let x = ok(&user, &["rev-parse", "HEAD:keep.txt"]);
            let t = ok(&user, &["rev-parse", "HEAD^{tree}"]);
            ok(d.path(), &["init", "-q", "--bare", "sh.git"]);
            let alt = user.join(".git/objects");
            std::fs::write(
                shadow.join("objects/info/alternates"),
                format!("{}\n", alt.display()),
            )
            .unwrap();
            let c = ok(&shadow, &["commit-tree", "-m", "s", &t]);
            ok(&shadow, &["update-ref", "refs/harness/a", &c]);
            Self {
                _d: d,
                user,
                shadow,
                x,
            }
        }

        /// Whether the shadow holds `id` in its own store (alternates moved
        /// out of the way for the check).
        fn owns(&self, id: &str) -> bool {
            let alt = self.shadow.join("objects/info/alternates");
            let tmp = self.shadow.join("alternates.off");
            std::fs::rename(&alt, &tmp).unwrap();
            let has = git(&self.shadow, &["cat-file", "-e", id]).status.success();
            std::fs::rename(&tmp, &alt).unwrap();
            has
        }

        fn pack_copy(&self, id: &str) -> String {
            let base = self.shadow.join("objects/pack/pack");
            ok_in(
                &self.shadow,
                &["pack-objects", "-q", "--window=0", base.to_str().unwrap()],
                &format!("{id}\n"),
            )
        }
    }

    /// What A.17 rests on, established against the git on this machine: a
    /// local copy of an object the alternate also has is dropped by the
    /// shadow's own `gc` (and `repack -a -d -l`), packed or loose; one in a
    /// pack with a `.keep` file survives every gc and repack, and is dropped
    /// at the next gc once the `.keep` file is gone.
    #[test]
    fn gc_drops_local_copies_the_alternate_has_unless_their_pack_is_kept() {
        let p = Pair::new();
        p.pack_copy(&p.x);
        assert!(p.owns(&p.x), "the copy was not made");
        ok(&p.shadow, &["gc", "--prune=now", "-q"]);
        assert!(!p.owns(&p.x), "gc kept a packed copy the alternate has");

        let name = p.pack_copy(&p.x);
        let keep = p.shadow.join(format!("objects/pack/pack-{name}.keep"));
        std::fs::write(&keep, "pin\n").unwrap();
        for args in [
            &["gc", "--prune=now", "-q"][..],
            &["gc", "-q"],
            &["repack", "-a", "-d", "-l", "-q"],
            &["repack", "-A", "-d", "-q"],
        ] {
            ok(&p.shadow, args);
            assert!(p.owns(&p.x), "{args:?} dropped the kept copy");
        }
        std::fs::remove_file(&keep).unwrap();
        ok(&p.shadow, &["gc", "--prune=now", "-q"]);
        assert!(!p.owns(&p.x), "the released copy stayed");
    }

    /// A local copy written while the alternate lacked the object, which
    /// then gained it (the user commits a file a checkpoint already stored):
    /// the shadow's gc keeps it while the user's copy is loose and drops it
    /// once the user's repository has packed it (its own gc).
    #[test]
    fn gc_drops_a_loose_local_copy_the_alternate_gained_later() {
        let p = Pair::new();
        std::fs::write(p.user.join("new.txt"), "N\n").unwrap();
        let n = ok(
            &p.shadow,
            &[
                "hash-object",
                "-w",
                p.user.join("new.txt").to_str().unwrap(),
            ],
        );
        assert!(p.owns(&n));
        ok(&p.user, &["add", "new.txt"]);
        ok(&p.user, &["commit", "-qm", "n"]);
        // Reachable from the shadow too.
        let t = ok(&p.user, &["rev-parse", "HEAD^{tree}"]);
        let c = ok(&p.shadow, &["commit-tree", "-m", "s", &t]);
        ok(&p.shadow, &["update-ref", "refs/harness/b", &c]);
        // `--local` leaves out only objects found in the alternate's packs:
        // while the user's copy is loose, the shadow's copy is packed.
        ok(&p.shadow, &["gc", "--prune=now", "-q"]);
        assert!(p.owns(&n), "dropped while the alternate's copy is loose");
        ok(&p.user, &["gc", "-q"]);
        ok(&p.shadow, &["gc", "--prune=now", "-q"]);
        assert!(!p.owns(&n), "gc kept a copy the alternate has packed");
    }

    /// The shadow's gc when an object a kept ref reaches is gone from the
    /// alternate (the user rewrote and pruned): gc fails, and nothing of the
    /// shadow is lost by trying.
    #[test]
    fn gc_fails_while_a_kept_ref_reaches_a_lost_object() {
        let p = Pair::new();
        ok(&p.user, &["checkout", "-q", "--orphan", "new"]);
        std::fs::write(p.user.join("keep.txt"), "other\n").unwrap();
        ok(&p.user, &["commit", "-qam", "fresh"]);
        ok(&p.user, &["branch", "-qD", "main"]);
        ok(&p.user, &["reflog", "expire", "--expire=now", "--all"]);
        ok(&p.user, &["gc", "-q", "--prune=now"]);
        assert!(!git(&p.user, &["cat-file", "-e", &p.x]).status.success());
        let out = git(&p.shadow, &["gc", "--prune=now", "-q"]);
        assert!(!out.status.success(), "gc succeeded: {out:?}");
        let refs = ok(&p.shadow, &["for-each-ref", "--format=%(refname)"]);
        assert_eq!(refs, "refs/harness/a");
    }

    // ---- the functions ----

    use super::super::tests::{Hostile, dir_state, fake_git, sh};
    use super::super::{TEST_GIT, repo_for, snapshot, snapshot_within};
    use super::*;

    /// Whether `repo`'s shadow has `id` in its own store.
    async fn owns(repo: &Repo, id: &str) -> bool {
        let alt = repo.shadow.clone().unwrap().join("objects/info/alternates");
        let off = alt.with_extension("off");
        std::fs::rename(&alt, &off).unwrap();
        let has = run(repo, &["cat-file", "-e", id], None).await.is_ok();
        std::fs::rename(&off, &alt).unwrap();
        has
    }

    async fn id_of(repo: &Repo, rev: &str) -> String {
        run(repo, &["rev-parse", rev], None).await.unwrap()
    }

    /// Make a collection due by the count of dropped groups.
    async fn make_due(repo: &Repo) {
        count_dropped(repo.shadow.as_ref().unwrap(), GC_EVERY).await;
    }

    /// A.17, the twin of `securing_blobs_copies_them_and_writes_nothing_in_the_users_repository`:
    /// pinning a turn copies the old version of exactly the changed file,
    /// and neither it nor the node's collection afterwards runs anything
    /// the user's repository defines (fsmonitor, hooks) or changes any file
    /// of the user's `.git`, not even a modification time.
    #[tokio::test]
    async fn pinning_and_collecting_write_nothing_in_the_users_repository() {
        let h = Hostile::new();
        sh(&h.repo, &["gc", "-q"]); // packed there: the shadow's gc drops plain copies
        h.config("core.fsmonitor", &h.script("fsmonitor").to_string_lossy());
        for hook in ["post-checkout", "reference-transaction", "pre-auto-gc"] {
            h.hook(&h.repo.join(".git/hooks"), hook);
        }
        let repo = repo_for(&h.repo, Some(&h.base)).await.unwrap();
        let before = snapshot(&repo, "refs/harness/s/t/before", "before")
            .await
            .unwrap();
        std::fs::write(h.repo.join("a.txt"), "two\n").unwrap();
        let after = snapshot(&repo, "refs/harness/s/t/after", "after")
            .await
            .unwrap();
        let a1 = id_of(&repo, &format!("{before}:a.txt")).await;
        let b1 = id_of(&repo, &format!("{before}:b.txt")).await;
        assert!(!owns(&repo, &a1).await, "a.txt's blob is not borrowed");
        let user_git = dir_state(&h.repo.join(".git"));
        tokio::time::sleep(Duration::from_millis(20)).await;
        let p = pin_changed(&repo, "refs/harness/s/t", &before, &after)
            .await
            .unwrap();
        assert!(p.missing.is_empty(), "{p:?}");
        assert!(owns(&repo, &a1).await, "a.txt's old version was not copied");
        assert!(!owns(&repo, &b1).await, "b.txt did not change");
        make_due(&repo).await;
        assert!(maybe_gc(&repo).await, "the collection did not run");
        assert!(owns(&repo, &a1).await, "the collection dropped the copy");
        assert!(
            dir_state(&h.repo.join(".git")) == user_git,
            "the user's .git changed"
        );
        assert_eq!(h.marked(), Vec::<String>::new());
    }

    /// A copy is released when its group is no longer kept, and only ours
    /// are touched: a `.keep` file with a line that is not ours stays.
    #[tokio::test]
    async fn copies_are_released_with_their_group() {
        let h = Hostile::new();
        sh(&h.repo, &["gc", "-q"]);
        let repo = repo_for(&h.repo, Some(&h.base)).await.unwrap();
        let before = snapshot(&repo, "refs/harness/s/t/before", "b")
            .await
            .unwrap();
        std::fs::write(h.repo.join("a.txt"), "two\n").unwrap();
        let after = snapshot(&repo, "refs/harness/s/t/after", "a")
            .await
            .unwrap();
        pin_changed(&repo, "refs/harness/s/t", &before, &after)
            .await
            .unwrap();
        let packs = repo.shadow.clone().unwrap().join("objects/pack");
        let foreign = packs.join("pack-0000.keep");
        std::fs::write(&foreign, "not ours\n").unwrap();
        let keeps = || {
            let mut v: Vec<String> = std::fs::read_dir(&packs)
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .filter(|n| n.ends_with(".keep"))
                .collect();
            v.sort();
            v
        };
        assert_eq!(keeps().len(), 2);
        release_pins(repo.shadow.as_ref().unwrap(), &["refs/harness/s/t".into()]).await;
        assert_eq!(keeps().len(), 2, "a kept group's copy was released");
        release_pins(repo.shadow.as_ref().unwrap(), &[]).await;
        assert_eq!(keeps(), ["pack-0000.keep"]);
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

    /// Timing only (A.17, A.18): what a steady-state turn's checkpoints cost
    /// with the old per-turn `gc --prune=now` and with the new path (copies,
    /// refs dropped, no gc), for 10 and 1,000 changed files.
    /// `A17_DIR=<dir on the disk to measure> A17_FILES=100000 cargo test -p
    /// harness-node --lib checkpoint_turn_cost -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn checkpoint_turn_cost() {
        use std::time::Instant;
        let base = std::env::var_os("A17_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let n: usize = std::env::var("A17_FILES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(100_000);
        let runs = 5;
        let ms = |t: Instant| t.elapsed().as_secs_f64() * 1000.0;
        let median = |mut v: Vec<f64>| {
            v.sort_by(|a, b| a.total_cmp(b));
            v[v.len() / 2]
        };
        for kind in ["repo", "plain"] {
            let d = tempfile::tempdir_in(&base).unwrap();
            let ws = d.path().join("ws");
            let path = |i: usize| ws.join(format!("d{}/e{}/f{i}.txt", i / 2500, (i / 50) % 50));
            let t = Instant::now();
            for i in 0..n {
                let p = path(i);
                if i % 50 == 0 {
                    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                }
                std::fs::write(&p, format!("file {i}\n").repeat(1 + i % 60)).unwrap();
            }
            if kind == "repo" {
                sh(&ws, &["init", "-q", "-b", "main"]);
                sh(&ws, &["add", "."]);
                sh(&ws, &["commit", "-qm", "init", "--no-gpg-sign"]);
            } else {
                // `A17_DIR` may lie inside a repository (this one); a gitfile
                // that leads nowhere stops git's search, so the folder is a
                // plain one (git never stages an entry named `.git`).
                std::fs::write(ws.join(".git"), "gitdir: /nonexistent\n").unwrap();
            }
            eprintln!("[{kind}] {n} files made in {:.0} ms", ms(t));
            let repo = repo_for(&ws, Some(&d.path().join("cp"))).await.unwrap();
            assert_eq!(repo.root, ws.canonicalize().unwrap());
            assert_eq!(borrows(&repo), kind == "repo");
            if kind == "repo" {
                // What the copy's `pack-objects` costs with and without
                // git's flush of the pack (see `copy_into_own_store`).
                let snap = snapshot(&repo, "refs/harness/p/p/before", "p")
                    .await
                    .unwrap();
                let id = run(&repo, &["rev-parse", &format!("{snap}:d0/e0/f0.txt")], None)
                    .await
                    .unwrap();
                for durable in [true, false] {
                    let mut v = Vec::new();
                    for _ in 0..5 {
                        let t = Instant::now();
                        copy_into_own_store(&repo, format!("{id}\n").into_bytes(), durable)
                            .await
                            .unwrap();
                        v.push(ms(t));
                    }
                    eprintln!(
                        "[{kind}] one-blob copy, flushed: {durable}: {:.0} ms",
                        median(v)
                    );
                }
            }
            let mut turn = 0usize;
            // One turn as the session runs it; `old` adds the old per-turn gc.
            let mut one = |changed: usize, old: bool| {
                turn += 1;
                let (repo, ws_turn) = (repo.clone(), turn);
                let edit: Vec<PathBuf> = (0..changed).map(|k| path(k * (n / changed))).collect();
                async move {
                    let g = format!("refs/harness/s/t{ws_turn}");
                    let t = Instant::now();
                    let b = snapshot(&repo, &format!("{g}/before"), "b").await.unwrap();
                    let snaps0 = ms(t);
                    for p in &edit {
                        std::fs::write(p, format!("edit {ws_turn}\n")).unwrap();
                    }
                    let t = Instant::now();
                    let a = snapshot(&repo, &format!("{g}/after"), "a").await.unwrap();
                    super::super::changed(&repo, &b, &a).await.unwrap();
                    let snaps = snaps0 + ms(t);
                    let t = Instant::now();
                    pin_changed(&repo, &g, &b, &a).await.unwrap();
                    let pin = ms(t);
                    let t = Instant::now();
                    super::super::prune(&repo, 10).await;
                    let prune = ms(t);
                    let t = Instant::now();
                    if old {
                        run(&repo, &["gc", "--prune=now", "--quiet"], None)
                            .await
                            .unwrap();
                    }
                    (snaps, pin, prune, ms(t))
                }
            };
            for _ in 0..11 {
                one(10, false).await;
            }
            for changed in [10, 1000] {
                for old in [false, true] {
                    let mut rows = Vec::new();
                    for _ in 0..runs {
                        rows.push(one(changed, old).await);
                    }
                    let col =
                        |f: fn(&(f64, f64, f64, f64)) -> f64| median(rows.iter().map(f).collect());
                    let total = median(rows.iter().map(|r| r.0 + r.1 + r.2 + r.3).collect());
                    eprintln!(
                        "[{kind}] {changed} changed, {}: turn {total:.0} ms = snapshots+changed {:.0} \
                         + copies {:.0} + prune {:.0} + gc {:.0} (medians of {runs})",
                        if old { "old (gc every turn)" } else { "new" },
                        col(|r| r.0),
                        col(|r| r.1),
                        col(|r| r.2),
                        col(|r| r.3),
                    );
                }
            }
            // The rule's own collection, once, on the new path's store.
            for _ in 0..3 {
                one(10, false).await;
            }
            make_due(&repo).await;
            let t = Instant::now();
            assert!(maybe_gc(&repo).await);
            eprintln!("[{kind}] one collection by the rule: {:.0} ms", ms(t));
        }
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
