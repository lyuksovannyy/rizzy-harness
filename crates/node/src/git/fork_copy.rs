//! A fork's own copy of the parent's repository (ADOPTION_PLAN C.2 part 2,
//! docs/fork.md, PLAN.md T92).
//!
//! **No git process ever opens the parent's repository, and the harness
//! never follows a link inside it.** The parent's `.git` may have been
//! written by an agent (config, hooks, attributes, filters,
//! `core.fsmonitor`, aliases, a `commondir`, a gitfile, and links to other
//! repositories on the disk), and the node is not sandboxed. `git worktree
//! add` and `git -C <parent>` read that config and run those hooks; a
//! `git clone --no-local` of the parent runs `git upload-pack` there, which
//! follows every link planted in `refs/` and `objects/` and reads
//! `core.attributesFile`. So the copy is made in two stages, both in the
//! node's staging folder (0700, protected, emptied at every start):
//!
//! 1. **A sanitised copy by our own code** ([`fork_sanitise`]): a walk with
//!    file descriptors (`openat` with `O_NOFOLLOW`, `fstat` on the opened
//!    descriptor), copying only `HEAD`, `refs/`, `packed-refs`, `shallow`
//!    and `objects/` as new files; a link or a special file anywhere in
//!    those ends the copy in a refusal naming the path; the parent's
//!    `config` and `info/exclude` are read as data. The harness writes the
//!    copy's own `config`.
//! 2. **The clone**, from that copy and never from the parent:
//!
//! ```text
//! git clone --no-local --no-checkout --template= <sanitised copy> <dest>
//! ```
//!
//!    by the hardened runner (cleared environment, the node's command-line
//!    overrides, no global or system config), inside the sandbox jail the
//!    checkpoint calls use when the node has one, with only the staging
//!    folders of this copy visible read-write and nothing of the parent
//!    mounted. Without a jail (a node with `[sandbox] mode = "off"`) it runs
//!    hardened but unjailed, which is acceptable because git no longer
//!    touches anything an agent's links could redirect. `--no-local` makes
//!    git read the source through `upload-pack` (no hard links into the
//!    source); `--no-checkout` runs no filter and no `post-checkout`;
//!    `--template=` copies no hook from the node's template directory. The
//!    sanitised copy is deleted afterwards.
//!
//! Everything after the clone is the harness's own: the new repository's
//! config is written by the harness (`origin` is the parent's path with
//! pushing disabled, and the parent's `origin` URL, read AS DATA from the
//! copy of its config with `git config --file`, validated and stripped of
//! credentials, becomes `upstream`), the branch is made with `update-ref`,
//! the working files are written from the checkpoint shadow with the
//! checkpoint writers (attributes from the empty tree), and the index is
//! set to the branch's commit. Plain git in the result is as safe as in any
//! chat folder, because the config in force is the harness's.
//!
//! What is not copied: files git ignores (build output, `.env`,
//! `node_modules`), nested repositories and submodule contents, stashes,
//! and the parent's hooks and local config. Its `info/exclude` is copied
//! (patterns only), so the same files stay ignored.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::*;

/// The oldest git the own-copy fork runs on, by release line: the first
/// release of each line with the local-clone fixes of May 2024
/// (CVE-2024-32002, -32004, -32020, -32021, -32465; release notes of
/// v2.39.4 to v2.45.1). A line not listed needs 2.45.1 or newer.
pub(crate) const MIN_GIT: &str = "2.45.1 (or 2.39.4, 2.40.2, 2.41.1, 2.42.2, 2.43.4, 2.44.1)";

/// Whether `version` (`major.minor.patch`) has the fixes [`MIN_GIT`] names.
pub(crate) fn patched(v: (u32, u32, u32)) -> bool {
    match (v.0, v.1) {
        (2, 39) => v.2 >= 4,
        (2, 40) => v.2 >= 2,
        (2, 41) => v.2 >= 1,
        (2, 42) => v.2 >= 2,
        (2, 43) => v.2 >= 4,
        (2, 44) => v.2 >= 1,
        (a, b) => (a, b, v.2) >= (2, 45, 1),
    }
}

/// `git version 2.56.1`, `git version 2.39.5 (Apple Git-154)`.
pub(crate) fn parse_version(out: &str) -> Option<(u32, u32, u32)> {
    let v = out.split_whitespace().nth(2)?;
    let mut it = v.split('.').map(|n| {
        n.chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse::<u32>()
            .ok()
    });
    Some((it.next()??, it.next()??, it.next().flatten().unwrap_or(0)))
}

#[cfg(test)]
thread_local! {
    /// Tests: the version git reports.
    static VERSION: std::cell::Cell<Option<(u32, u32, u32)>> =
        const { std::cell::Cell::new(None) };
    /// Tests: a wrong way to copy, to prove the hostile-repository test
    /// notices it.
    static MUTATION: std::cell::Cell<Mutation> = const { std::cell::Cell::new(Mutation::None) };
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mutation {
    None,
    /// `git -C <parent> worktree add`.
    WorktreeAdd,
    /// `git clone <parent> <dest>` with the node's template directory.
    PlainClone,
}

#[cfg(test)]
pub(crate) fn pretend_version(v: Option<(u32, u32, u32)>) {
    VERSION.with(|c| c.set(v));
}

#[cfg(test)]
pub(crate) fn mutate(m: Mutation) {
    MUTATION.with(|c| c.set(m));
}

/// The git on this node is new enough (see [`MIN_GIT`]); the refusal says
/// which version it found.
pub(crate) async fn version_ok() -> Result<(), String> {
    #[cfg(test)]
    if let Some(v) = VERSION.with(|c| c.get()) {
        return check_version(v);
    }
    let mut c = hardened(&git_program());
    c.arg("version");
    let out = exec(c, None, Duration::from_secs(10)).await?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    match parse_version(&text) {
        Some(v) => check_version(v),
        None => Err(format!(
            "this node could not tell which git it has; a fork's own copy needs git {MIN_GIT}"
        )),
    }
}

fn check_version(v: (u32, u32, u32)) -> Result<(), String> {
    if patched(v) {
        Ok(())
    } else {
        Err(format!(
            "this node's git is {}.{}.{}, older than the {MIN_GIT} a copy of a repository \
             needs (its fixes for cloning untrusted repositories)",
            v.0, v.1, v.2
        ))
    }
}

/// What [`fill`] works from.
pub(crate) struct Plan<'a> {
    /// The parent's repository root (canonical): the one folder whose
    /// `.git` stage 1 walks. No git process is ever given this path.
    pub source: &'a Path,
    /// An existing, empty folder the node made and no session can reach.
    pub dest: &'a Path,
    /// The node's staging folder: the sanitised copy is made in it.
    pub outside: &'a Path,
    /// `fork/<short id>`.
    pub branch: &'a str,
    /// The commit the parent's HEAD was on at the cut, when the chat's log
    /// says; otherwise the parent's current HEAD is used.
    pub head: Option<&'a str>,
    /// The parent's checkpoint repository (its shadow), for the writers.
    pub shadow: &'a Repo,
    /// The cut's snapshot (`after` checkpoint); `None`: the branch's commit
    /// is written as it is.
    pub snapshot: Option<&'a str>,
    /// How long each stage may take.
    pub timeout: Duration,
    pub max_bytes: u64,
    /// The most files and folders stage 1 copies.
    pub max_entries: u64,
}

/// What [`fill`] made.
#[derive(Debug, Clone)]
pub(crate) struct Filled {
    /// The commit the fork's branch is on.
    pub commit: String,
    /// Whether it is the commit the parent's log says HEAD was on at the
    /// cut (`false`: the parent's current HEAD).
    pub head_exact: bool,
    /// The parent's `origin` URL as the fork's `upstream`, when it had a
    /// usable one.
    pub upstream: Option<String>,
}

fn object_id(s: &str) -> bool {
    (s.len() == 40 || s.len() == 64)
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A branch name the harness makes: `fork/` and letters, digits, `-`, `_`.
fn branch_ok(b: &str) -> bool {
    b.strip_prefix("fork/").is_some_and(|r| {
        !r.is_empty()
            && r.len() <= 40
            && r.bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    })
}

/// The URL of a remote the parent had, accepted as `upstream` only when it
/// is plain text of a transport a person would use: https, ssh (a URL or
/// scp-like `user@host:path`) or a file (a path or `file://`). No control
/// character, not starting with `-`, no `::` (git's `ext::` and other
/// remote helpers), no whitespace, not longer than 2000 bytes. The user
/// name and password of an https URL are dropped (they are the parent's
/// credentials, not the fork's); an https URL with no host is no URL.
pub(crate) fn upstream_url(u: &str) -> Option<String> {
    let u = u.trim_end_matches('\n');
    if u.is_empty()
        || u.len() > 2000
        || u.starts_with('-')
        || u.contains("::")
        || u.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return None;
    }
    let url_scheme = u.split_once("://").map(|(s, _)| s.to_ascii_lowercase());
    match url_scheme.as_deref() {
        Some("https") => {
            let rest = &u["https://".len()..];
            let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
            let (auth, tail) = rest.split_at(end);
            let host = auth.rsplit_once('@').map_or(auth, |(_, h)| h);
            (!host.is_empty() && !host.starts_with(':')).then(|| format!("https://{host}{tail}"))
        }
        Some("ssh" | "file") => Some(u.to_string()),
        Some(_) => None,
        None => {
            if u.starts_with('/') {
                return Some(u.to_string());
            }
            // scp-like: `[user@]host:path`, the host with no `/` before the colon.
            let (host, path) = u.split_once(':')?;
            (!host.is_empty()
                && !path.is_empty()
                && !host.contains('/')
                && host
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.@[]".contains(&b)))
            .then(|| u.to_string())
        }
    }
}

pub(crate) use super::fork_sanitise::ENTRY_MAX;

/// The most stdout and stderr a git call of the copy keeps in memory.
const OUT_CAP: usize = 1 << 20;
const ERR_CAP: usize = 64 * 1024;

/// A git command of the copy: the hardened base, in the node's sandbox jail
/// when it has one, with exactly the folders `rw` visible read-write and
/// nothing else of the disk beyond the system's read-only folders (the
/// parent is never among them); working directory `cwd`, with the search for
/// a repository above it stopped at the staging folder.
fn git_cmd(p: &Plan<'_>, cwd: &Path, rw: &[&Path]) -> Result<Command, String> {
    let base = match &p.shadow.jail.0 {
        Some(s) => {
            use harness_tools::sandbox::Grant;
            let grants = rw
                .iter()
                .map(|d| Grant::made(d, "a folder of the fork's copy"))
                .collect::<Result<Vec<_>, _>>()?;
            let mut c = s.helper(&grants, &[]);
            c.arg(git_program());
            c
        }
        None => std::process::Command::new(git_program()),
    };
    let mut c = hardened_std(base);
    overrides(&mut c);
    c.current_dir(cwd).env("GIT_CEILING_DIRECTORIES", p.outside);
    Ok(Command::from(c))
}

/// A git command in the new repository: its git directory, its work tree,
/// and no attributes from anywhere (as the shadow's calls: no filter
/// whatever the tree's `.gitattributes` say). Its config is the harness's.
fn dest_cmd(p: &Plan<'_>, empty_tree: &str) -> Result<Command, String> {
    let mut c = git_cmd(p, p.dest, &[p.dest])?;
    c.arg("--git-dir")
        .arg(p.dest.join(".git"))
        .arg("--work-tree")
        .arg(p.dest)
        .arg("-c")
        .arg("core.attributesFile=/dev/null")
        .arg("-c")
        .arg("core.excludesFile=/dev/null");
    c.env("GIT_ATTR_SOURCE", empty_tree);
    Ok(c)
}

/// What a bounded git call left.
struct Captured {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

/// Keep the first `cap` bytes of `r` and drain the rest.
async fn keep<R: tokio::io::AsyncRead + Unpin>(mut r: R, cap: usize) -> Vec<u8> {
    use tokio::io::AsyncReadExt;
    let mut kept = Vec::new();
    let mut buf = [0u8; 8192];
    while let Ok(n) = r.read(&mut buf).await {
        if n == 0 {
            break;
        }
        let room = cap.saturating_sub(kept.len());
        kept.extend_from_slice(&buf[..n.min(room)]);
    }
    kept
}

/// Run `cmd` within `timeout` and keep at most [`OUT_CAP`] and [`ERR_CAP`]
/// bytes of what it prints (a hostile repository cannot make the node
/// buffer without bound); the process group is killed on drop.
async fn exec_capped(mut cmd: Command, timeout: Duration) -> Result<Captured, String> {
    use std::process::Stdio;
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .process_group(0);
    let mut child = cmd.spawn().map_err(|e| {
        harness_tools::sandbox::image_spawn_error(cmd.as_std().get_program(), &e)
            .unwrap_or_else(|| format!("git: {e}"))
    })?;
    let mut group = Group(child.id());
    let out = child.stdout.take().map(|s| tokio::spawn(keep(s, OUT_CAP)));
    let err = child.stderr.take().map(|s| tokio::spawn(keep(s, ERR_CAP)));
    let status = tokio::time::timeout(timeout, child.wait())
        .await
        .map_err(|_| format!("git did not finish within {} s", timeout.as_secs_f32()))?
        .map_err(|e| format!("git: {e}"))?;
    group.0 = None;
    let stdout = match out {
        Some(t) => t.await.unwrap_or_default(),
        None => Vec::new(),
    };
    let stderr = match err {
        Some(t) => t.await.unwrap_or_default(),
        None => Vec::new(),
    };
    Ok(Captured {
        status,
        stdout,
        stderr,
    })
}

async fn run_out(c: Command, what: &str) -> Result<String, String> {
    let out = exec_capped(c, GIT_TIMEOUT).await?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(format!(
            "{what}: {}",
            clean(&String::from_utf8_lossy(&out.stderr))
        ))
    }
}

/// The last lines of git's complaint, control characters out.
fn clean(stderr: &str) -> String {
    let lines: Vec<&str> = stderr.lines().filter(|l| !l.trim().is_empty()).collect();
    let tail = lines[lines.len().saturating_sub(3)..].join(" / ");
    let out: String = tail.chars().filter(|c| !c.is_control()).take(300).collect();
    if out.is_empty() {
        "git said nothing".into()
    } else {
        out
    }
}

/// The sanitised copy's folder in the staging folder, removed when this is
/// dropped (done, failed or cancelled): nothing waits for the next start.
struct Scratch {
    dir: PathBuf,
    /// The bare repository inside it.
    repo: PathBuf,
}

impl Scratch {
    fn new(outside: &Path) -> Result<Self, String> {
        use std::os::unix::fs::DirBuilderExt;
        let base = outside
            .canonicalize()
            .map_err(|e| format!("the node's staging folder: {e}"))?;
        let mut b = [0u8; 8];
        getrandom::fill(&mut b).expect("OS randomness");
        let dir = base.join(format!("forksrc-{}", hex::encode(b)));
        let mk = |p: &Path| {
            std::fs::DirBuilder::new()
                .mode(0o700)
                .create(p)
                .map_err(|e| format!("cannot make {}: {e}", p.display()))
        };
        mk(&dir)?;
        let this = Self {
            repo: dir.join("repo.git"),
            dir,
        };
        mk(&this.repo)?;
        Ok(this)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Tells the walk of stage 1 to stop when the copy is cancelled.
struct StopOnDrop(Arc<std::sync::atomic::AtomicBool>);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// What the parent's config said, from the copy of its bytes (read as data).
#[derive(Debug, Default, PartialEq, Eq)]
struct ParentConfig {
    origin_url: Option<String>,
}

/// Parse `git config --list -z` of the copy of the parent's config; refuse
/// what stage 2 cannot copy.
fn parent_config(list: &[u8]) -> Result<ParentConfig, String> {
    let mut out = ParentConfig::default();
    for rec in list.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        let (key, value) = match rec.iter().position(|b| *b == b'\n') {
            Some(i) => (&rec[..i], Some(&rec[i + 1..])),
            None => (rec, None),
        };
        let key = String::from_utf8_lossy(key).to_ascii_lowercase();
        let value = value.map(|v| String::from_utf8_lossy(v).into_owned());
        match (key.as_str(), value) {
            ("extensions.objectformat", Some(v)) if v.eq_ignore_ascii_case("sha256") => {
                return Err(
                    "the repository is not copied: it uses the sha256 object format, which a \
                     fork's own copy does not support yet"
                        .into(),
                );
            }
            ("extensions.refstorage", Some(v)) if v.eq_ignore_ascii_case("reftable") => {
                return Err(
                    "the repository is not copied: it uses the reftable ref storage, which a \
                     fork's own copy does not support yet"
                        .into(),
                );
            }
            ("core.repositoryformatversion", Some(v))
                if !v.trim().parse::<u32>().is_ok_and(|n| n <= 1) =>
            {
                return Err(
                    "the repository is not copied: its repository format version is one this \
                     node does not know"
                        .into(),
                );
            }
            ("remote.origin.url", Some(v)) => out.origin_url = Some(v),
            _ => {}
        }
    }
    Ok(out)
}

/// Copy the parent's repository into `dest` and fill it (see the module
/// comment). `dest` is left as it is on failure: the caller removes it.
pub(crate) async fn fill(p: &Plan<'_>) -> Result<Filled, String> {
    if !branch_ok(p.branch) {
        return Err("not a fork branch name".into());
    }
    if let Some(h) = p.head
        && !object_id(h)
    {
        return Err("not a commit id".into());
    }
    version_ok().await?;
    // Stage 1: a sanitised copy, no git process.
    let work = Scratch::new(p.outside)?;
    let data = sanitise(p, &work).await?;
    let git_dir = work.repo.clone();
    let (head, _branch) = tokio::task::spawn_blocking(move || user_repo::head_of(&git_dir))
        .await
        .map_err(|e| e.to_string())?;
    let Some(head) = head else {
        return Err("the repository has no commit yet, so there is nothing to copy".into());
    };
    if head.len() == 64 {
        return Err(
            "the repository is not copied: it uses the sha256 object format, which a \
             fork's own copy does not support yet"
                .into(),
        );
    }
    let upstream = read_parent_config(p, &work, data.config.as_deref()).await?;
    std::fs::write(
        work.repo.join("config"),
        "[core]\n\trepositoryformatversion = 0\n\tbare = true\n",
    )
    .map_err(|e| format!("cannot write the copy's config: {e}"))?;
    // Stage 2: the clone, from the copy. The parent is not mounted.
    clone(p, &work).await?;
    drop(work);
    check_dest(p.dest)?;
    let src = p.source.to_string_lossy().into_owned();
    let mut sets = vec![("remote.origin.url", src)];
    if let Some(u) = &upstream {
        sets.push(("remote.upstream.url", u.clone()));
        sets.push((
            "remote.upstream.fetch",
            "+refs/heads/*:refs/remotes/upstream/*".to_string(),
        ));
    }
    for (k, v) in sets {
        let mut c = git_cmd(p, p.dest, &[p.dest])?;
        c.arg("config")
            .arg("--file")
            .arg(p.dest.join(".git/config"))
            .arg(k)
            .arg(v);
        run_out(c, "writing the copy's config").await?;
    }
    let fmt_empty = empty_tree("sha1");
    // The commit of the branch: the log's, when the clone has it.
    let (commit, exact) = pick_commit(p, fmt_empty).await?;
    let mut c = dest_cmd(p, fmt_empty)?;
    c.args(["update-ref"])
        .arg(format!("refs/heads/{}", p.branch))
        .arg(&commit);
    run_out(c, "making the fork's branch").await?;
    let mut c = dest_cmd(p, fmt_empty)?;
    c.args(["symbolic-ref", "HEAD"])
        .arg(format!("refs/heads/{}", p.branch));
    run_out(c, "pointing the copy at its branch").await?;
    // The clone made a local branch of the parent's current branch: it
    // would carry the parent's later commits. Remote-tracking refs stay.
    drop_clone_branches(p, fmt_empty).await;
    copy_exclude(data.exclude.as_deref(), p.dest).await;
    match p.snapshot {
        Some(snap) => {
            write_snapshot(p, snap).await?;
            let mut c = dest_cmd(p, fmt_empty)?;
            c.args(["read-tree", &commit]);
            run_out(c, "setting the copy's index").await?;
        }
        None => {
            let mut c = dest_cmd(p, fmt_empty)?;
            c.args(["read-tree", &commit]);
            run_out(c, "setting the copy's index").await?;
            let mut c = dest_cmd(p, fmt_empty)?;
            c.args(["checkout-index", "-a", "-f"]);
            run_out(c, "writing the copy's files").await?;
        }
    }
    // The working files are written: the whole folder against the bound.
    if crate::forge_clone::over(p.dest, p.max_bytes).await {
        return Err(too_big(p.max_bytes));
    }
    // The index has no stat data yet: one refresh now, not on the first
    // `git status` of the person.
    let mut c = dest_cmd(p, fmt_empty)?;
    c.args(["update-index", "-q", "--refresh"]);
    let _ = exec_capped(c, GIT_TIMEOUT).await;
    Ok(Filled {
        commit,
        head_exact: exact,
        upstream,
    })
}

/// Stage 1 on a blocking thread, within the stage's time; the walk is told
/// to stop if this future is dropped.
async fn sanitise(p: &Plan<'_>, work: &Scratch) -> Result<fork_sanitise::Data, String> {
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let _stop = StopOnDrop(cancel.clone());
    let lim = fork_sanitise::Limits {
        max_bytes: p.max_bytes,
        max_entries: p.max_entries,
        deadline: Instant::now() + p.timeout,
        cancel,
    };
    let (root, into) = (p.source.to_path_buf(), work.repo.clone());
    tokio::task::spawn_blocking(move || fork_sanitise::copy(&root, &into, &lim))
        .await
        .map_err(|e| format!("copying the repository: {e}"))?
}

/// The parent's `origin` URL (validated, no credentials), from the bytes of
/// its config read as data: the bytes go to a file in the copy's folder and
/// `git config --file` parses that file, with no includes. The copy of the
/// parent's repository is not involved.
async fn read_parent_config(
    p: &Plan<'_>,
    work: &Scratch,
    config: Option<&[u8]>,
) -> Result<Option<String>, String> {
    let Some(bytes) = config else {
        return Ok(None);
    };
    let file = work.dir.join("parent-config");
    std::fs::write(&file, bytes)
        .map_err(|e| format!("cannot copy the repository's config: {e}"))?;
    let mut c = git_cmd(p, &work.dir, &[&work.dir])?;
    c.arg("config")
        .arg("--file")
        .arg(&file)
        .args(["--no-includes", "--list", "-z"]);
    let out = exec_capped(c, Duration::from_secs(10)).await?;
    if !out.status.success() {
        return Err(
            "the repository is not copied: its .git/config is not a git config file".into(),
        );
    }
    let cfg = parent_config(&out.stdout)?;
    Ok(cfg.origin_url.as_deref().and_then(upstream_url))
}

/// `.git` of the copy is a plain folder git made, with no `commondir`.
fn check_dest(dest: &Path) -> Result<(), String> {
    let git = dest.join(".git");
    if !std::fs::symlink_metadata(&git).is_ok_and(|m| m.is_dir()) {
        return Err("the copy has no .git folder".into());
    }
    if std::fs::symlink_metadata(git.join("commondir")).is_ok() {
        return Err("the copy's .git names another repository (commondir)".into());
    }
    Ok(())
}

/// The clone: stage 2, from the sanitised copy.
async fn clone(p: &Plan<'_>, work: &Scratch) -> Result<(), String> {
    use std::process::Stdio;
    #[allow(unused_mut)]
    let mut c: Command;
    #[cfg(test)]
    {
        c = match MUTATION.with(|m| m.get()) {
            Mutation::None => real_clone(p, work)?,
            Mutation::WorktreeAdd => {
                let mut c = tokio::process::Command::new("git");
                c.arg("-C")
                    .arg(p.source)
                    .args(["worktree", "add", "--detach", "--force"])
                    .arg(p.dest)
                    .env("GIT_CONFIG_GLOBAL", "/dev/null")
                    .env("GIT_CONFIG_NOSYSTEM", "1");
                c
            }
            Mutation::PlainClone => {
                // The node's template directory, as a node that sets one
                // would have it, and a clone with no safe options.
                let mut c = tokio::process::Command::new("git");
                c.args(["clone", "--"])
                    .arg(p.source)
                    .arg(p.dest)
                    .current_dir(p.outside)
                    .env("GIT_CONFIG_GLOBAL", "/dev/null")
                    .env("GIT_CONFIG_NOSYSTEM", "1")
                    .env("GIT_TEMPLATE_DIR", p.outside.join("node-template"));
                c
            }
        };
    }
    #[cfg(not(test))]
    {
        c = real_clone(p, work)?;
    }
    c.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .process_group(0);
    let mut child = c.spawn().map_err(|e| {
        harness_tools::sandbox::image_spawn_error(c.as_std().get_program(), &e)
            .unwrap_or_else(|| format!("cannot start git: {e}"))
    })?;
    let mut group = Group(child.id());
    let err_out = child.stderr.take().map(|s| tokio::spawn(keep(s, ERR_CAP)));
    let deadline = Instant::now() + p.timeout;
    let every = Duration::from_millis(500);
    let ended: Result<std::process::ExitStatus, String> = loop {
        let next = (Instant::now() + every).min(deadline);
        tokio::select! {
            st = child.wait() => break st.map_err(|e| format!("git: {e}")),
            _ = tokio::time::sleep_until(next.into()) => {
                if Instant::now() >= deadline {
                    break Err(format!(
                        "copying the repository took longer than this node allows ([limits] \
                         clone_timeout_s = {} s); nothing was kept",
                        p.timeout.as_secs()
                    ));
                }
                if crate::forge_clone::over(p.dest, p.max_bytes).await {
                    break Err(too_big(p.max_bytes));
                }
            }
        }
    };
    let status = match ended {
        Ok(st) => {
            group.0 = None;
            st
        }
        Err(e) => {
            drop(group);
            let _ = child.wait().await;
            return Err(e);
        }
    };
    let stderr = match err_out {
        Some(t) => t.await.unwrap_or_default(),
        None => Vec::new(),
    };
    if !status.success() {
        return Err(format!(
            "git could not copy the repository: {}",
            clean(&String::from_utf8_lossy(&stderr))
        ));
    }
    if crate::forge_clone::over(p.dest, p.max_bytes).await {
        return Err(too_big(p.max_bytes));
    }
    Ok(())
}

fn too_big(max: u64) -> String {
    format!(
        "the copy grew past what this node allows ([limits] clone_max_mb = {} MB); git was \
         stopped and nothing was kept",
        max / (1024 * 1024)
    )
}

/// `git clone --no-local --no-checkout --template= -- <sanitised copy>
/// <dest>`, hardened, in the jail when the node has one. `protocol.file.
/// allow=always` after the base's `protocol.allow=never`: the file
/// transport is the one this clone needs (the sanitised copy's path), and
/// no other. Nothing is recursed into.
fn real_clone(p: &Plan<'_>, work: &Scratch) -> Result<Command, String> {
    let mut c = git_cmd(p, p.dest, &[&work.dir, p.dest])?;
    c.arg("-c")
        .arg("protocol.file.allow=always")
        .args([
            "clone",
            "--no-local",
            "--no-checkout",
            "--no-recurse-submodules",
            "--template=",
            "--quiet",
            "--",
        ])
        .arg(&work.repo)
        .arg(p.dest);
    Ok(c)
}

/// Tests: the arguments of the clone command (program and arguments), and
/// the folders of the copy it was given.
#[cfg(test)]
pub(crate) fn clone_command_for_test(p: &Plan<'_>) -> (Vec<String>, PathBuf) {
    let work = Scratch::new(p.outside).unwrap();
    let c = real_clone(p, &work).unwrap();
    let std = c.as_std();
    let mut v = vec![std.get_program().to_string_lossy().into_owned()];
    v.extend(std.get_args().map(|a| a.to_string_lossy().into_owned()));
    (v, work.dir.clone())
}

/// The commit for the branch and whether it is the log's.
async fn pick_commit(p: &Plan<'_>, empty: &str) -> Result<(String, bool), String> {
    if let Some(h) = p.head {
        let mut c = dest_cmd(p, empty)?;
        c.args(["cat-file", "-e"]).arg(format!("{h}^{{commit}}"));
        if exec_capped(c, GIT_TIMEOUT)
            .await
            .is_ok_and(|o| o.status.success())
        {
            return Ok((h.to_string(), true));
        }
    }
    let mut c = dest_cmd(p, empty)?;
    c.args(["rev-parse", "--verify", "-q", "HEAD^{commit}"]);
    let id = run_out(c, "reading the copy's HEAD").await?;
    if object_id(&id) {
        Ok((id, false))
    } else {
        Err("the copy has no commit to start from".into())
    }
}

/// Delete the local branches `git clone` made (the parent's current
/// branch), leaving the fork's.
async fn drop_clone_branches(p: &Plan<'_>, empty: &str) {
    let Ok(mut c) = dest_cmd(p, empty) else {
        return;
    };
    c.args(["for-each-ref", "--format=%(refname)", "refs/heads/"]);
    let Ok(list) = run_out(c, "listing branches").await else {
        return;
    };
    for r in list.lines() {
        if r == format!("refs/heads/{}", p.branch) || !r.starts_with("refs/heads/") {
            continue;
        }
        if let Ok(mut c) = dest_cmd(p, empty) {
            c.args(["update-ref", "-d", "--", r]);
            let _ = exec_capped(c, GIT_TIMEOUT).await;
        }
    }
}

/// The parent's `info/exclude` (ignore patterns: text git reads as
/// patterns, nothing runs), as read by stage 1.
async fn copy_exclude(text: Option<&[u8]>, dest: &Path) {
    if let Some(t) = text {
        let info = dest.join(".git/info");
        if tokio::fs::create_dir_all(&info).await.is_ok() {
            let _ = tokio::fs::write(info.join("exclude"), t).await;
        }
    }
}

/// The cut's files, written by the checkpoint writers from the shadow: a
/// private index, `read-tree`, `checkout-index` (attributes from the empty
/// tree, so no filter), as a sub-agent's folder is filled.
async fn write_snapshot(p: &Plan<'_>, snap: &str) -> Result<(), String> {
    if !object_id(snap) {
        return Err("not a checkpoint id".into());
    }
    // No prune or collection of the shadow while it is read.
    let _busy = hold(p.shadow).await;
    let mut into = p.shadow.with_root(p.dest).scratch();
    into.tree_rw = true;
    let idx = fresh_index(&into).await?;
    let _files = IndexFiles(idx.clone());
    run(&into, &["read-tree", snap], Some(&idx)).await?;
    run(&into, &["checkout-index", "-a", "-f"], Some(&idx)).await?;
    Ok(())
}

/// The bytes the cut's snapshot holds, counted from its listing as it
/// streams (nothing is buffered), capped by [`fork_sanitise::ENTRY_MAX`]
/// entries and [`GIT_TIMEOUT`]. `Err` when the listing fails or is too
/// long: a copy is never started on a size nobody could read.
pub(crate) async fn tree_bytes(shadow: &Repo, snap: &str) -> Result<u64, String> {
    use tokio::io::AsyncReadExt;
    if !object_id(snap) {
        return Err("not a checkpoint id".into());
    }
    let mut c = command(shadow, None);
    c.args(["ls-tree", "-r", "-l", "-z", snap])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .process_group(0);
    let mut child = c.spawn().map_err(|e| {
        harness_tools::sandbox::image_spawn_error(c.as_std().get_program(), &e)
            .unwrap_or_else(|| format!("git: {e}"))
    })?;
    let mut group = Group(child.id());
    let err = child.stderr.take().map(|s| tokio::spawn(keep(s, ERR_CAP)));
    let mut stdout = child.stdout.take().ok_or("git: no output")?;
    let read = async {
        let (mut total, mut entries) = (0u64, 0u64);
        let mut carry: Vec<u8> = Vec::new();
        let mut buf = [0u8; 16 * 1024];
        loop {
            let n = stdout
                .read(&mut buf)
                .await
                .map_err(|e| format!("git: {e}"))?;
            if n == 0 {
                break;
            }
            carry.extend_from_slice(&buf[..n]);
            let mut from = 0;
            while let Some(i) = carry[from..].iter().position(|b| *b == 0) {
                let rec = &carry[from..from + i];
                from += i + 1;
                entries += 1;
                if entries > fork_sanitise::ENTRY_MAX {
                    return Err(format!(
                        "this chat's files are more than {} entries, which a copy does not \
                         handle",
                        fork_sanitise::ENTRY_MAX
                    ));
                }
                let size = rec
                    .split(|b| *b == b'\t')
                    .next()
                    .and_then(|meta| meta.split(|b| *b == b' ').rfind(|f| !f.is_empty()))
                    .and_then(|f| std::str::from_utf8(f).ok())
                    .and_then(|f| f.parse::<u64>().ok());
                // A submodule's line has `-` for a size: no bytes.
                total = total.saturating_add(size.unwrap_or(0));
            }
            carry.drain(..from);
            // A record has a path (at most a few KiB), never megabytes.
            if carry.len() > 1 << 20 {
                return Err("the checkpoint's file list has a record that is too long".into());
            }
        }
        Ok(total)
    };
    let total = tokio::time::timeout(GIT_TIMEOUT, read)
        .await
        .map_err(|_| "reading the checkpoint's file list took too long".to_string())??;
    let status = child.wait().await.map_err(|e| format!("git: {e}"))?;
    group.0 = None;
    if !status.success() {
        let stderr = match err {
            Some(t) => t.await.unwrap_or_default(),
            None => Vec::new(),
        };
        return Err(format!(
            "the checkpoint's file list could not be read: {}",
            clean(&String::from_utf8_lossy(&stderr))
        ));
    }
    Ok(total)
}

/// What the parent's folder is, to a fork that wants its own copy.
pub(crate) enum Source {
    /// The top of the git repository holding the folder (canonical).
    Repo(PathBuf),
    /// No git repository holds it (or none within the bounds).
    NotRepo,
}

/// The repository holding `cwd`, found on the file system and read as data
/// (see [`user_repo`]); `Err` with the reason when the `.git` there is not a
/// plain repository directory (a gitfile, a link, a `commondir`).
pub(crate) async fn source_of(cwd: &Path, bounds: &Bounds) -> Result<Source, String> {
    match find_repo(cwd, bounds).await {
        user_repo::Found::Repo(r) => Ok(Source::Repo(r.top)),
        user_repo::Found::Unusual(why) => Err(why),
        user_repo::Found::None => Ok(Source::NotRepo),
    }
}

/// The disk a copy of `source`'s `.git` needs, counted from its files
/// (no link followed, only sizes: nothing is opened), counting no further
/// than `cap`. An estimate for the room check; stage 1 bounds the real copy.
pub(crate) async fn git_dir_bytes(source: &Path, cap: u64) -> u64 {
    let git = source.join(".git");
    tokio::task::spawn_blocking(move || {
        let mut n = 0u64;
        for e in walkdir::WalkDir::new(&git)
            .follow_links(false)
            .into_iter()
            .flatten()
        {
            if let Ok(m) = e.metadata()
                && !m.is_dir()
            {
                n = n.saturating_add(m.len());
                if n > cap {
                    break;
                }
            }
        }
        n
    })
    .await
    .unwrap_or(cap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_of_an_https_url_are_dropped_and_a_url_without_a_host_is_none() {
        for (url, want) in [
            (
                "https://u:p@example.invalid/a/b.git",
                Some("https://example.invalid/a/b.git"),
            ),
            (
                "https://token@example.invalid/a.git",
                Some("https://example.invalid/a.git"),
            ),
            (
                "https://u:p@example.invalid:8443/a.git?x=1",
                Some("https://example.invalid:8443/a.git?x=1"),
            ),
            ("https://u:p@host", Some("https://host")),
            (
                "https://example.invalid/a@b.git",
                Some("https://example.invalid/a@b.git"),
            ),
            ("https://u:p@/a.git", None),
            ("https://@/a.git", None),
            ("https:///a.git", None),
            ("https://:8443/a.git", None),
            (
                "ssh://git@example.invalid/a.git",
                Some("ssh://git@example.invalid/a.git"),
            ),
            (
                "git@example.invalid:a/b.git",
                Some("git@example.invalid:a/b.git"),
            ),
        ] {
            assert_eq!(upstream_url(url).as_deref(), want, "{url}");
        }
    }

    #[test]
    fn the_parents_config_is_read_for_four_keys_and_what_cannot_be_copied_is_refused() {
        // `git config --list -z`: `key\nvalue\0` records; `|` stands for NUL.
        let list = |s: &str| s.replace('|', "\u{0}").into_bytes();
        let cfg = parent_config(&list(
            "core.repositoryformatversion\n0|remote.origin.url\nhttps://h/x.git|remote.origin.url\nhttps://h/y.git|uploadpack.packobjectshook\nevil|",
        ))
        .unwrap();
        assert_eq!(cfg.origin_url.as_deref(), Some("https://h/y.git"));
        // A key with no `=` (a boolean) has no value.
        assert!(parent_config(&list("core.bare|")).is_ok());
        for (text, want) in [
            ("extensions.objectformat\nSHA256|", "sha256"),
            ("extensions.refstorage\nreftable|", "reftable"),
            ("core.repositoryformatversion\n2|", "format version"),
            ("core.repositoryformatversion\nx|", "format version"),
        ] {
            let e = parent_config(&list(text)).unwrap_err();
            assert!(e.contains(want), "{text}: {e}");
        }
        assert!(parent_config(&list("extensions.objectformat\nsha1|")).is_ok());
    }
}
