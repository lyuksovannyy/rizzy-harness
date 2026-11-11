//! Experiment B.3: what a turn's checkpoints cost as the folder grows.
//! Ignored benchmark, never part of the normal suite; the numbers and what
//! they mean are in `docs/checkpoint-cost.md`.
//!
//! ```text
//! B3_DIR=$(mktemp -d -p /some/disk) \
//!   cargo test --release -p harness-node --lib checkpoint_cost -- --ignored --nocapture
//! ```
//!
//! Environment:
//! - `B3_DIR`: parent directory for the generated trees (required; must be
//!   on a real disk, not a tmpfs). Everything created under it is removed
//!   when the run ends, also on a panic.
//! - `B3_SIZES`: comma-separated file counts (default `1000,10000,100000`).
//! - `B3_KINDS`: `plain`, `repo` or `plain,repo` (default both).
//! - `B3_RUNS`: measured runs per cell after one warm-up run (default 5).
//! - `B3_RUNS_BIG`: the same for sizes of 250,000 files and more (default
//!   `B3_RUNS`).
//!
//! It lives inside the crate because the `git` module is private. It calls
//! the same functions in the same order as `session.rs` (`snapshot_full`,
//! `changed`, `prune`, `restore_diff`, `restore_plan`, `secure_blobs`,
//! `restore_paths`), plus the private staging commands one by one for the
//! step breakdown.

use super::*;
use std::collections::BTreeMap;
use std::os::unix::fs::MetadataExt;
use std::time::Instant;

const FILES_PER_DIR: usize = 50;
/// `checkpoints_keep` default (policy.rs).
const KEEP: usize = 10;

// ---------------------------------------------------------------- tree

fn fnv(s: &str) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

const WORDS: &[&str] = &[
    "fn", "let", "mut", "match", "self", "impl", "struct", "return", "async", "await", "use",
    "pub", "const", "type", "where", "Some", "None", "Ok", "Err", "string", "value", "result",
    "index", "path", "config", "session", "event", "node", "master", "token", "error", "handle",
    "request", "response", "buffer", "stream", "state", "update", "render", "parse", "format",
    "length", "offset", "count", "total", "name", "kind", "data", "item", "list", "map", "set",
    "get", "put", "new", "from", "into", "iter", "next", "take", "skip", "fold", "zip", "chain",
];

/// Directory of file `i`: depth 3 to 6, `FILES_PER_DIR` files each.
fn dir_of(i: usize) -> String {
    let d = i / FILES_PER_DIR;
    let depth = 3 + d % 4;
    let mut p = String::new();
    let mut x = d;
    for level in 0..depth - 1 {
        p.push_str(&format!("l{level}_{}/", x % 8));
        x /= 8;
    }
    p.push_str(&format!("d{d}"));
    p
}

fn path_of(i: usize) -> String {
    let ext = ["rs", "ts", "json", "md", "txt"][i % 5];
    format!("{}/f{i}.{ext}", dir_of(i))
}

/// Size of file `i` at `version` (0 = as generated): 1 to 8 KiB, and five
/// files of 1 MiB. Edited versions differ in size by a few bytes too.
fn size_of(n: usize, i: usize, version: u64) -> usize {
    let big = (1..=5).any(|k| i == n / 6 * k);
    let base = if big {
        1 << 20
    } else {
        1024 + (fnv(&format!("size{i}")) % 7169) as usize
    };
    if version == 0 {
        base
    } else {
        base + (version % 3) as usize * 13
    }
}

/// Deterministic text from the path and version.
fn content(n: usize, i: usize, version: u64) -> Vec<u8> {
    let size = size_of(n, i, version);
    let mut s = fnv(&format!("{}#{version}", path_of(i))) | 1;
    let mut out = Vec::with_capacity(size + 16);
    let mut col = 0;
    while out.len() < size {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        let w = WORDS[(s % WORDS.len() as u64) as usize];
        out.extend_from_slice(w.as_bytes());
        col += 1;
        if col % 9 == 0 {
            out.push(b'\n');
        } else {
            out.push(b' ');
        }
        // A little unique noise so blobs rarely compress to nothing.
        if col % 5 == 0 {
            out.extend_from_slice(format!("{:x} ", s >> 40).as_bytes());
        }
    }
    out.truncate(size);
    out
}

/// The ignored directory: as many 1 KiB files as the tree.
fn ignored_path(i: usize) -> String {
    format!("ignored_deps/p{}/q{}/f{i}.js", i / 2500, (i / 50) % 50)
}

fn gen_tree(root: &Path, n: usize) {
    let threads = std::thread::available_parallelism()
        .map_or(4, usize::from)
        .min(16);
    std::thread::scope(|sc| {
        for t in 0..threads {
            sc.spawn(move || {
                let mut i = t * FILES_PER_DIR;
                while i < n {
                    let end = (i + FILES_PER_DIR).min(n);
                    std::fs::create_dir_all(root.join(dir_of(i))).unwrap();
                    for j in i..end {
                        std::fs::write(root.join(path_of(j)), content(n, j, 0)).unwrap();
                    }
                    i += threads * FILES_PER_DIR;
                }
            });
        }
    });
    std::fs::write(root.join(".gitignore"), "ignored_deps/\n").unwrap();
    std::thread::scope(|sc| {
        for t in 0..threads {
            sc.spawn(move || {
                let body = "x".repeat(1024);
                let mut made = std::collections::HashSet::new();
                let mut i = t;
                while i < n {
                    let p = root.join(ignored_path(i));
                    let dir = p.parent().unwrap().to_path_buf();
                    if made.insert(dir.clone()) {
                        std::fs::create_dir_all(&dir).unwrap();
                    }
                    std::fs::write(p, &body).unwrap();
                    i += threads;
                }
            });
        }
    });
}

/// Read every file of the tree once (not the ignored directory): what a
/// first touch after creation costs, and warms the page cache.
fn read_all(root: &Path, n: usize) -> f64 {
    let t = Instant::now();
    let threads = 8;
    std::thread::scope(|sc| {
        for k in 0..threads {
            sc.spawn(move || {
                for i in (k..n).step_by(threads) {
                    let _ = std::fs::read(root.join(path_of(i)));
                }
            });
        }
    });
    ms(t.elapsed())
}

/// Rewrite files at `version` (spread over the tree).
fn edit(root: &Path, n: usize, count: usize, version: u64) -> Vec<usize> {
    let count = count.min(n);
    let stride = (n / count).max(1);
    let ids: Vec<usize> = (0..count).map(|k| (k * stride + 7) % n).collect();
    for &i in &ids {
        std::fs::write(root.join(path_of(i)), content(n, i, version)).unwrap();
    }
    ids
}

/// Add `count` new files in a new directory named by `tag`.
fn add_files(root: &Path, count: usize, tag: &str) {
    for j in 0..count {
        let dir = root.join(format!("added_{tag}/d{}", j / 50));
        if j % 50 == 0 {
            std::fs::create_dir_all(&dir).unwrap();
        }
        let body = format!("added {tag} {j}\n").repeat(100);
        std::fs::write(dir.join(format!("n{j}.txt")), body).unwrap();
    }
}

// ------------------------------------------------------------- helpers

/// Removes the benchmark's own directory, also when a test panics.
struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// median, min, max in milliseconds.
fn stats(v: &[f64]) -> (f64, f64, f64) {
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let med = if s.len() % 2 == 1 {
        s[s.len() / 2]
    } else {
        (s[s.len() / 2 - 1] + s[s.len() / 2]) / 2.0
    };
    (med, s[0], s[s.len() - 1])
}

fn load() -> String {
    std::fs::read_to_string("/proc/loadavg")
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// (allocated bytes, apparent bytes, file count) under `dir`.
fn du(dir: &Path) -> (u64, u64, u64) {
    let (mut a, mut l, mut c) = (0, 0, 0);
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let Ok(m) = e.metadata() else { continue };
            a += m.blocks() * 512;
            l += m.len();
            if m.is_dir() {
                stack.push(e.path());
            } else {
                c += 1;
            }
        }
    }
    (a, l, c)
}

fn mib(b: u64) -> String {
    format!("{:.1}", b as f64 / 1048576.0)
}

fn shadow_dir(repo: &Repo) -> PathBuf {
    repo.shadow.clone().unwrap()
}

/// Leftovers a snapshot or restore should never leave in the shadow.
fn leftovers(repo: &Repo) -> Vec<String> {
    let d = shadow_dir(repo);
    let mut out = vec![];
    for sub in [".", "objects/pack", "objects"] {
        if let Ok(rd) = std::fs::read_dir(d.join(sub)) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                if n.starts_with("harness-index-") || n.starts_with("tmp_") || n.ends_with(".lock")
                {
                    out.push(format!("{sub}/{n}"));
                }
            }
        }
    }
    out
}

/// Accumulates durations per cell and prints every result as it is made.
#[derive(Default)]
struct Results {
    cells: BTreeMap<(String, usize, String), String>,
    notes: Vec<String>,
}

impl Results {
    fn put(&mut self, kind: &str, n: usize, op: &str, v: &[f64], flag: &str) {
        let (med, lo, hi) = stats(v);
        let cell = format!("{med:.0} [{lo:.0}-{hi:.0}]{flag}");
        eprintln!("RES {kind:5} {n:>7} {op:<34} {cell}  (n={})", v.len());
        self.cells.insert((kind.into(), n, op.into()), cell);
    }
    fn text(&mut self, kind: &str, n: usize, op: &str, cell: String) {
        eprintln!("RES {kind:5} {n:>7} {op:<34} {cell}");
        self.cells.insert((kind.into(), n, op.into()), cell);
    }
    fn note(&mut self, s: String) {
        eprintln!("NOTE {s}");
        self.notes.push(s);
    }
}

/// `snapshot_full` as the session calls it; a timeout is recorded and the
/// true time found with the same steps minus the timeout.
#[derive(Clone, Default)]
struct Snapper {
    timeouts: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl Snapper {
    async fn snap(&self, repo: &Repo, r: &str) -> (String, f64) {
        let t = Instant::now();
        match snapshot_full(repo, r, "bench").await {
            Ok(s) => (s.sha, ms(t.elapsed())),
            Err(e) if e.contains("too long") => {
                self.timeouts
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                eprintln!(
                    "TIMEOUT after {:?}: {e}; timing the same steps without it",
                    t.elapsed()
                );
                let t = Instant::now();
                let tree = match worktree_tree(repo).await {
                    Ok(t) => t,
                    Err(e) => {
                        eprintln!(
                            "FAILED again without the snapshot timeout after {:?}: {e}",
                            t.elapsed()
                        );
                        return (String::new(), f64::INFINITY);
                    }
                };
                let head = run(repo, &["rev-parse", "--verify", "-q", "HEAD"], None)
                    .await
                    .ok();
                let mut args = vec![
                    "commit-tree",
                    "--no-gpg-sign",
                    tree.id.as_str(),
                    "-m",
                    "bench",
                ];
                if let Some(h) = head.as_deref().filter(|h| !h.is_empty()) {
                    args.extend(["-p", h]);
                }
                let sha = run(repo, &args, None).await.unwrap();
                run(repo, &["update-ref", r, &sha], None).await.unwrap();
                record_order(repo, r).await;
                (sha, ms(t.elapsed()))
            }
            Err(e) => panic!("snapshot failed: {e}"),
        }
    }
    fn count(&self) -> usize {
        self.timeouts.load(std::sync::atomic::Ordering::Relaxed)
    }
    fn flag(&self) -> &'static str {
        if self.count() > 0 { " T/O" } else { "" }
    }
}

async fn fresh_repo(root: &Path, base: &Path, borrow: bool) -> Repo {
    let repo = repo_for(root, Some(base)).await.unwrap();
    if !borrow {
        // Not through `repo_for` again: it would write the file back.
        let _ = std::fs::remove_file(shadow_dir(&repo).join("objects/info/alternates"));
    }
    repo
}

/// The staging steps one by one, as `stage_without_nested` runs them (no
/// nested repositories here), then `write-tree` and the commit. `seed`:
/// start from the shadow's index (a repeat) or from an empty one (a first
/// snapshot, a rebuild). The private index is dropped afterwards.
async fn breakdown(repo: &Repo, seed: bool) -> Vec<(&'static str, f64)> {
    let mut steps = vec![];
    let t = Instant::now();
    let (_real, idx) = seeded_index(repo, seed).await.unwrap();
    steps.push(("copy index", ms(t.elapsed())));
    let t = Instant::now();
    let staged = run_raw(repo, &["ls-files", "-z", "--stage"], Some(&idx), None)
        .await
        .unwrap();
    steps.push(("ls-files --stage", ms(t.elapsed())));
    let t = Instant::now();
    let (tracked, _) = split_stage(&staged);
    let n_tracked = tracked.len();
    steps.push(("rust: parse stage list", ms(t.elapsed())));
    let t = Instant::now();
    let others = run_raw(
        repo,
        &["ls-files", "-z", "--others", "--exclude-standard"],
        Some(&idx),
        None,
    )
    .await
    .unwrap();
    steps.push(("ls-files -o (listing)", ms(t.elapsed())));
    let t = Instant::now();
    let new: Vec<u8> = others
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty() && !p.ends_with(b"/"))
        .flat_map(|p| p.iter().copied().chain([0]))
        .collect();
    steps.push(("rust: split untracked", ms(t.elapsed())));
    let t = Instant::now();
    run(repo, &["add", "-u"], Some(&idx)).await.unwrap();
    steps.push(("add -u (tracked)", ms(t.elapsed())));
    let t = Instant::now();
    if !new.is_empty() {
        run_raw(
            repo,
            &["update-index", "--add", "--remove", "-z", "--stdin"],
            Some(&idx),
            Some(new),
        )
        .await
        .unwrap();
    }
    steps.push(("update-index --add (hash new)", ms(t.elapsed())));
    let t = Instant::now();
    run_raw(repo, &["ls-files", "-z", "--stage"], Some(&idx), None)
        .await
        .unwrap();
    steps.push(("ls-files --stage (again)", ms(t.elapsed())));
    let t = Instant::now();
    let tree = run(repo, &["write-tree"], Some(&idx)).await.unwrap();
    steps.push(("write-tree", ms(t.elapsed())));
    let t = Instant::now();
    let head = run(repo, &["rev-parse", "--verify", "-q", "HEAD"], None)
        .await
        .ok();
    let mut args = vec!["commit-tree", "--no-gpg-sign", tree.as_str(), "-m", "bench"];
    if let Some(h) = head.as_deref().filter(|h| !h.is_empty()) {
        args.extend(["-p", h]);
    }
    let sha = run(repo, &args, None).await.unwrap();
    run(repo, &["update-ref", "refs/harness/b3/step", &sha], None)
        .await
        .unwrap();
    steps.push(("commit-tree + update-ref", ms(t.elapsed())));
    let _ = run(repo, &["update-ref", "-d", "refs/harness/b3/step"], None).await;
    let _ = std::fs::remove_file(&idx);
    let _ = n_tracked;
    steps
}

fn git_user(dir: &Path, args: &[&str]) {
    let ok = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "b3")
        .env("GIT_AUTHOR_EMAIL", "b3@localhost")
        .env("GIT_COMMITTER_NAME", "b3")
        .env("GIT_COMMITTER_EMAIL", "b3@localhost")
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

// ----------------------------------------------------------- the runs

/// One turn as the session does it: `repo_for`, before, (the turn's edits),
/// after, `changed`, `prune`. Returns the parts in ms.
struct Turn {
    before: f64,
    after: f64,
    changed: f64,
    prune: f64,
    total: f64,
}

async fn turn(
    sn: &Snapper,
    root: &Path,
    base: &Path,
    n: usize,
    tag: &str,
    edits: usize,
    version: u64,
) -> Turn {
    let t0 = Instant::now();
    let repo = repo_for(root, Some(base)).await.unwrap();
    let rb = format!("refs/harness/s/{tag}/before");
    let (b, before) = sn.snap(&repo, &rb).await;
    if edits > 0 {
        edit(root, n, edits, version);
    }
    let ra = format!("refs/harness/s/{tag}/after");
    let t = Instant::now();
    let (a, _) = sn.snap(&repo, &ra).await;
    let after_snap = ms(t.elapsed());
    let t = Instant::now();
    let files = changed(&repo, &b, &a).await.unwrap();
    let ch = ms(t.elapsed());
    assert_eq!(
        files.len(),
        edits.min(n),
        "changed() disagrees with the edits"
    );
    let t = Instant::now();
    prune(&repo, KEEP).await;
    let pr = ms(t.elapsed());
    let total = ms(t0.elapsed());
    Turn {
        before,
        after: after_snap,
        changed: ch,
        prune: pr,
        total,
    }
}

struct RestoreCost {
    total: f64,
    diff: f64,
    save: f64,
    plan: f64,
    secure: f64,
    write: f64,
    after: f64,
    prune: f64,
}

/// The session's restore, step by step (session.rs `restore`).
async fn restore_cycle(
    sn: &Snapper,
    root: &Path,
    base: &Path,
    n: usize,
    count: usize,
    version: u64,
) -> RestoreCost {
    let repo = repo_for(root, Some(base)).await.unwrap();
    let id = harness_core::new_id();
    let rb = format!("refs/harness/s/r{id}/before");
    let ra = format!("refs/harness/s/r{id}/after");
    let (before, _) = sn.snap(&repo, &rb).await;
    let ids = edit(root, n, count, version);
    let (after, _) = sn.snap(&repo, &ra).await;
    // Timed from here: what the person waits for.
    let t0 = Instant::now();
    assert!(exists(&repo, &before).await && exists(&repo, &after).await);
    let t = Instant::now();
    let diff = restore_diff(&repo, &before, &after).await.unwrap();
    let t_diff = ms(t.elapsed());
    let t = Instant::now();
    let saved_ref = format!("refs/harness/s/restore-{id}/before");
    let (now, _) = sn.snap(&repo, &saved_ref).await;
    let t_save = ms(t.elapsed());
    let t = Instant::now();
    let plan = restore_plan(&repo, &diff, &now, true).await.unwrap();
    let t_plan = ms(t.elapsed());
    assert_eq!(plan.touched.len(), count.min(n));
    let t = Instant::now();
    secure_blobs(&repo, &plan).await.unwrap();
    let t_secure = ms(t.elapsed());
    let t = Instant::now();
    restore_paths(&repo, &plan).await.unwrap();
    let t_write = ms(t.elapsed());
    let t = Instant::now();
    let (_s, _) = sn
        .snap(&repo, &format!("refs/harness/s/restore-{id}/after"))
        .await;
    let t_after = ms(t.elapsed());
    let t = Instant::now();
    prune(&repo, KEEP).await;
    let t_prune = ms(t.elapsed());
    let total = ms(t0.elapsed());
    // The folder is back to the generated bytes.
    for &i in &ids {
        assert_eq!(
            std::fs::read(root.join(path_of(i))).unwrap(),
            content(n, i, 0),
            "restore wrote the wrong bytes to {}",
            path_of(i)
        );
    }
    RestoreCost {
        total,
        diff: t_diff,
        save: t_save,
        plan: t_plan,
        secure: t_secure,
        write: t_write,
        after: t_after,
        prune: t_prune,
    }
}

async fn series<F, Fut>(runs: usize, mut f: F) -> Vec<f64>
where
    F: FnMut(usize) -> Fut,
    Fut: std::future::Future<Output = f64>,
{
    // Run 0 is the warm-up and is dropped.
    let mut v = vec![];
    for r in 0..=runs {
        let x = f(r).await;
        if r > 0 {
            v.push(x);
        }
    }
    v
}

fn print_steps(
    res: &mut Results,
    kind: &str,
    n: usize,
    label: &str,
    runs: &[Vec<(&'static str, f64)>],
) {
    for (k, (name, _)) in runs[0].iter().enumerate() {
        let v: Vec<f64> = runs.iter().map(|r| r[k].1).collect();
        res.put(kind, n, &format!("{label}: {name}"), &v, "");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn checkpoint_cost() {
    let Some(parent) = std::env::var_os("B3_DIR").map(PathBuf::from) else {
        eprintln!("set B3_DIR to a directory on a real disk");
        return;
    };
    let sizes: Vec<usize> = std::env::var("B3_SIZES")
        .unwrap_or_else(|_| "1000,10000,100000".into())
        .split(',')
        .map(|s| s.trim().parse().unwrap())
        .collect();
    let kinds = std::env::var("B3_KINDS").unwrap_or_else(|_| "plain,repo".into());
    let runs: usize = std::env::var("B3_RUNS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);
    let runs_big: usize = std::env::var("B3_RUNS_BIG")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(runs);
    let work = parent.join(format!("b3-{}", harness_core::new_id()));
    std::fs::create_dir_all(&work).unwrap();
    let _guard = Cleanup(work.clone());
    eprintln!("B3 start: load {}, dir {}", load(), work.display());
    let mut res = Results::default();

    for &n in &sizes {
        let runs = if n >= 250_000 { runs_big } else { runs };
        let root = work.join(format!("tree{n}"));
        let t = Instant::now();
        gen_tree(&root, n);
        eprintln!("B3 generated {n} files (+{n} ignored) in {:?}", t.elapsed());
        let rd = read_all(&root, n);
        res.text(
            "both",
            n,
            "0 read every file once after creation",
            format!("{rd:.0} ms (8 threads)"),
        );
        let ign = root.join("ignored_deps");
        let ign_away = work.join(format!("ignored_away{n}"));

        for kind in kinds.split(',') {
            let kind = kind.trim();
            let _ = read_all(&root, n);
            if kind == "repo" {
                let t = Instant::now();
                git_user(&root, &["init", "-q", "-b", "main"]);
                git_user(&root, &["add", "-A"]);
                git_user(&root, &["commit", "-q", "-m", "init", "--no-gpg-sign"]);
                eprintln!("B3 committed {n} files in {:?}", t.elapsed());
            }
            let sb = work.join(format!("shadow-{kind}-{n}"));
            let borrow = kind == "repo";
            let sn = Snapper::default();
            eprintln!("B3 ===== {kind} {n} files, {runs} runs, load {}", load());

            // 1. First snapshot, empty shadow each time (shadow creation included).
            let mut warm = 0.0;
            let mut last_first: Option<Repo> = None;
            let v = series(runs, |r| {
                let sb = sb.join(format!("first{r}"));
                let root = root.clone();
                let sn = sn.clone();
                let prev = last_first.take();
                async move {
                    if let Some(p) = prev {
                        // keep the last one for the disk number, drop the rest
                        let _ = std::fs::remove_dir_all(shadow_dir(&p));
                    }
                    let t = Instant::now();
                    let repo = repo_for(&root, Some(&sb)).await.unwrap();
                    let _ = sn.snap(&repo, "refs/harness/s/t/before").await;
                    let e = ms(t.elapsed());
                    let lo = leftovers(&repo);
                    if !lo.is_empty() {
                        eprintln!("LEFTOVER after a first snapshot: {lo:?}");
                    }
                    e
                }
            })
            .await;
            // (warm-up time is not kept by `series`; re-read from the first fresh run below)
            res.put(kind, n, "1 first snapshot", &v, sn.flag());
            let _ = &mut warm;
            // Disk after the first snapshot (the last first-run shadow).
            let repo_last = repo_for(&root, Some(&sb.join(format!("first{runs}"))))
                .await
                .unwrap();
            let (a, l, c) = du(&shadow_dir(&repo_last));
            res.text(
                kind,
                n,
                "7 disk after first snapshot",
                format!(
                    "{} MiB allocated, {} MiB apparent, {c} files",
                    mib(a),
                    mib(l)
                ),
            );
            let _ = std::fs::remove_dir_all(&sb);

            // Same, with the ignored directory out of the way.
            if n <= 100_000 {
                std::fs::rename(&ign, &ign_away).unwrap();
                let v = series(runs.min(3), |r| {
                    let sb = sb.join(format!("noign{r}"));
                    let root = root.clone();
                    let sn = sn.clone();
                    async move {
                        let t = Instant::now();
                        let repo = repo_for(&root, Some(&sb)).await.unwrap();
                        let _ = sn.snap(&repo, "refs/harness/s/t/before").await;
                        ms(t.elapsed())
                    }
                })
                .await;
                res.put(kind, n, "1b first, ignored dir absent", &v, "");
                std::fs::rename(&ign_away, &ign).unwrap();
                let _ = std::fs::remove_dir_all(&sb);
            }

            // The main shadow for everything after.
            let main = sb.join("main");
            let repo = repo_for(&root, Some(&main)).await.unwrap();
            let (_, first_main) = sn.snap(&repo, "refs/harness/s/base/before").await;
            res.text(
                kind,
                n,
                "1c first, main shadow (one run)",
                format!("{first_main:.0}"),
            );

            // 2. Repeat, nothing changed.
            let v = series(runs, |r| {
                let repo = repo.clone();
                let sn = sn.clone();
                async move {
                    sn.snap(&repo, &format!("refs/harness/s/rep{r}/before"))
                        .await
                        .1
                }
            })
            .await;
            res.put(kind, n, "2 repeat, unchanged", &v, sn.flag());
            if n <= 100_000 {
                std::fs::rename(&ign, &ign_away).unwrap();
                let v = series(runs, |r| {
                    let repo = repo.clone();
                    let sn = sn.clone();
                    async move {
                        sn.snap(&repo, &format!("refs/harness/s/repn{r}/before"))
                            .await
                            .1
                    }
                })
                .await;
                res.put(kind, n, "2b repeat, ignored dir absent", &v, "");
                std::fs::rename(&ign_away, &ign).unwrap();
            }
            prune(&repo, 2).await;

            // 3. After changes.
            let mut ver = 0u64;
            for count in [1usize, 10, 1000] {
                let v = series(runs, |r| {
                    ver += 1;
                    edit(&root, n, count, ver);
                    let repo = repo.clone();
                    let sn = sn.clone();
                    async move {
                        sn.snap(&repo, &format!("refs/harness/s/e{count}x{r}/before"))
                            .await
                            .1
                    }
                })
                .await;
                let name = if count > n {
                    format!("3 after {n} edits (all)")
                } else {
                    format!("3 after {count} edited")
                };
                res.put(kind, n, &name, &v, sn.flag());
                prune(&repo, 2).await;
            }
            let mut k = 0;
            let v = series(runs, |r| {
                k += 1;
                add_files(&root, 1000, &format!("{r}"));
                let repo = repo.clone();
                let sn = sn.clone();
                async move {
                    sn.snap(&repo, &format!("refs/harness/s/add{r}/before"))
                        .await
                        .1
                }
            })
            .await;
            let _ = k;
            res.put(kind, n, "3 after 1000 new files", &v, sn.flag());
            for r in 0..=runs {
                let _ = std::fs::remove_dir_all(root.join(format!("added_{r}")));
            }
            // Back to the generated bytes and a settled shadow.
            for i in 0..n.min(2000) {
                let p = root.join(path_of(i));
                if std::fs::read(&p).unwrap() != content(n, i, 0) {
                    std::fs::write(&p, content(n, i, 0)).unwrap();
                }
            }
            for count in [1usize, 10, 1000] {
                edit_back(&root, n, count);
            }
            let _ = sn.snap(&repo, "refs/harness/s/settle/before").await;
            prune(&repo, 2).await;

            // Step breakdown, first and repeat.
            let mut first_steps = vec![];
            for r in 0..=runs.min(3) {
                let r_repo = repo_for(&root, Some(&sb.join(format!("bd{r}"))))
                    .await
                    .unwrap();
                let s = breakdown(&r_repo, false).await;
                if r > 0 {
                    first_steps.push(s);
                }
                let _ = std::fs::remove_dir_all(shadow_dir(&r_repo));
            }
            print_steps(&mut res, kind, n, "step first", &first_steps);
            let mut rep_steps = vec![];
            for r in 0..=runs {
                let s = breakdown(&repo, true).await;
                if r > 0 {
                    rep_steps.push(s);
                }
            }
            print_steps(&mut res, kind, n, "step repeat", &rep_steps);
            // The same with 10 files edited since the shadow's index.
            let mut ed_steps = vec![];
            for r in 0..=runs {
                edit(&root, n, 10, 5000 + r as u64);
                let s = breakdown(&repo, true).await;
                if r > 0 {
                    ed_steps.push(s);
                }
            }
            print_steps(&mut res, kind, n, "step 10 edited", &ed_steps);
            edit_back(&root, n, 10);

            // 6. Index rebuild from an empty index (session: after a failed write-tree).
            let v = series(runs, |_| {
                let repo = repo.clone();
                async move {
                    let t = Instant::now();
                    let _ = worktree_tree_from(&repo, false).await.ok().unwrap();
                    ms(t.elapsed())
                }
            })
            .await;
            res.put(kind, n, "6 index rebuild", &v, "");

            // 4. Whole turns, in a session that already holds KEEP checkpoints
            //    (so every prune drops one and runs gc, as in steady state).
            let tb = sb.join("turns");
            let mut tv = 0u64;
            for p in 0..KEEP {
                tv += 1;
                turn(&sn, &root, &tb, n, &format!("pre{p}"), 10, tv).await;
            }
            let mut ts = vec![];
            for r in 0..=runs {
                tv += 1;
                let t = turn(&sn, &root, &tb, n, &format!("w{r}"), 10, tv).await;
                if r > 0 {
                    ts.push(t);
                }
            }
            put_turn(
                &mut res,
                kind,
                n,
                "4 turn, 10 files changed",
                &ts,
                sn.flag(),
            );
            let mut ts = vec![];
            for r in 0..=runs {
                let t = turn(&sn, &root, &tb, n, &format!("ro{r}"), 0, 0).await;
                if r > 0 {
                    ts.push(t);
                }
            }
            put_turn(&mut res, kind, n, "4 turn, read-only", &ts, sn.flag());
            // A young session: nothing to prune yet.
            let yb = sb.join("young");
            let t = turn(&sn, &root, &yb, n, "y0", 10, 900).await;
            res.text(
                kind,
                n,
                "4 turn, first of a session (10 changed)",
                format!(
                    "total {:.0} (before {:.0}, after {:.0}, changed {:.0}, prune {:.0})",
                    t.total, t.before, t.after, t.changed, t.prune
                ),
            );
            edit_back(&root, n, 10);

            // 5. Restore.
            let mut rv = 1000u64;
            for count in [1usize, 10, 1000] {
                let mut all = vec![];
                for r in 0..=runs.min(5) {
                    rv += 1;
                    let c = restore_cycle(&sn, &root, &tb, n, count, rv).await;
                    if r > 0 {
                        all.push(c);
                    }
                }
                let name = format!("5 restore of {}", count.min(n));
                let v: Vec<f64> = all.iter().map(|c| c.total).collect();
                res.put(kind, n, &name, &v, sn.flag());
                let part =
                    |f: fn(&RestoreCost) -> f64| stats(&all.iter().map(f).collect::<Vec<_>>()).0;
                res.text(
                    kind,
                    n,
                    &format!("5 restore of {} parts (median)", count.min(n)),
                    format!(
                        "diff {:.0}, save-now {:.0}, plan {:.0}, secure {:.0}, write {:.0}, after {:.0}, prune {:.0}",
                        part(|c| c.diff), part(|c| c.save), part(|c| c.plan),
                        part(|c| c.secure), part(|c| c.write), part(|c| c.after), part(|c| c.prune)
                    ),
                );
            }

            // 7. Disk after ten turns of 10 changed files, fresh shadow;
            //    for a repository also without borrowing.
            let variants: Vec<(&str, bool)> = if borrow {
                vec![("borrowing", true), ("not borrowing", false)]
            } else {
                vec![("own objects", true)]
            };
            for (label, b) in variants {
                let db = sb.join(format!("disk-{}", label.replace(' ', "-")));
                let t = Instant::now();
                let drepo = fresh_repo(&root, &db, b).await;
                let (_, first) = sn.snap(&drepo, "refs/harness/s/d/before").await;
                let t_first_total = ms(t.elapsed());
                let (a1, l1, c1) = du(&shadow_dir(&drepo));
                let mut tt = vec![];
                for p in 0..10 {
                    tv += 1;
                    // Not through `repo_for`: that would restore the alternates file.
                    let rb = format!("refs/harness/s/dt{p}/before");
                    let (bsha, _) = sn.snap(&drepo, &rb).await;
                    let t = Instant::now();
                    edit(&root, n, 10, tv);
                    let (asha, _) = sn
                        .snap(&drepo, &format!("refs/harness/s/dt{p}/after"))
                        .await;
                    let _ = changed(&drepo, &bsha, &asha).await.unwrap();
                    prune(&drepo, KEEP).await;
                    tt.push(ms(t.elapsed()));
                }
                let (a2, l2, c2) = du(&shadow_dir(&drepo));
                res.text(
                    kind,
                    n,
                    &format!("7 disk, {label}"),
                    format!(
                        "first snapshot {t_first_total:.0} ms (snap {first:.0}); after first: {} MiB alloc / {} MiB apparent / {c1} files; after 10 turns x 10 files: {} MiB alloc / {} MiB apparent / {c2} files",
                        mib(a1), mib(l1), mib(a2), mib(l2)
                    ),
                );
                if !b {
                    // The no-borrow shadow's first-snapshot time, repeated.
                    let v = series(runs.min(3), |r| {
                        let root = root.clone();
                        let sb = sb.join(format!("nb{r}"));
                        let sn = sn.clone();
                        async move {
                            let t = Instant::now();
                            let repo = fresh_repo(&root, &sb, false).await;
                            let _ = sn.snap(&repo, "refs/harness/s/t/before").await;
                            ms(t.elapsed())
                        }
                    })
                    .await;
                    res.put(kind, n, "1d first, not borrowing", &v, "");
                }
                let lo = leftovers(&drepo);
                if !lo.is_empty() {
                    res.note(format!("{kind} {n} {label}: leftovers {lo:?}"));
                }
                edit_back(&root, n, 10);
                let _ = std::fs::remove_dir_all(shadow_dir(&drepo));
            }

            // Leaks and timeouts.
            let lo = leftovers(&repo);
            let lo2 = leftovers(&repo_for(&root, Some(&tb)).await.unwrap());
            res.note(format!(
                "{kind} {n}: timeouts {}, leftovers main {lo:?} turns {lo2:?}, load {}",
                sn.count(),
                load()
            ));
            let _ = std::fs::remove_dir_all(&sb);
            if kind == "repo" {
                let _ = std::fs::remove_dir_all(root.join(".git"));
            }
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    eprintln!("\nB3 end: load {}", load());
    print_tables(&res, &sizes, &format!("both,{kinds}"));
}

/// Put edited files back to the generated bytes (only those `edit` touches).
fn edit_back(root: &Path, n: usize, count: usize) {
    let count = count.min(n);
    let stride = (n / count).max(1);
    for k in 0..count {
        let i = (k * stride + 7) % n;
        let p = root.join(path_of(i));
        let c = content(n, i, 0);
        if std::fs::read(&p).ok().as_deref() != Some(&c) {
            std::fs::write(p, c).unwrap();
        }
    }
}

fn put_turn(res: &mut Results, kind: &str, n: usize, label: &str, ts: &[Turn], flag: &str) {
    let g = |f: fn(&Turn) -> f64| ts.iter().map(f).collect::<Vec<_>>();
    res.put(kind, n, label, &g(|t| t.total), flag);
    let m = |f: fn(&Turn) -> f64| stats(&g(f)).0;
    res.text(
        kind,
        n,
        &format!("{label}: parts (median)"),
        format!(
            "before {:.0}, after {:.0}, changed {:.0}, prune {:.0}",
            m(|t| t.before),
            m(|t| t.after),
            m(|t| t.changed),
            m(|t| t.prune)
        ),
    );
}

fn print_tables(res: &Results, sizes: &[usize], kinds: &str) {
    eprintln!("\n===== TABLES (ms: median [min-max]) =====");
    for kind in kinds.split(',') {
        eprintln!("\n### {kind}");
        let ops: Vec<&String> = {
            let mut seen: Vec<&String> = vec![];
            for (k, _, op) in res.cells.keys() {
                if k == kind && !seen.contains(&op) {
                    seen.push(op);
                }
            }
            seen
        };
        eprint!("| operation |");
        for n in sizes {
            eprint!(" {n} |");
        }
        eprintln!();
        eprint!("|---|");
        for _ in sizes {
            eprint!("---|");
        }
        eprintln!();
        for op in ops {
            eprint!("| {op} |");
            for n in sizes {
                let c = res
                    .cells
                    .get(&(kind.to_string(), *n, op.clone()))
                    .map_or("-", String::as_str);
                eprint!(" {c} |");
            }
            eprintln!();
        }
    }
    eprintln!("\n### notes");
    for n in &res.notes {
        eprintln!("- {n}");
    }
}
