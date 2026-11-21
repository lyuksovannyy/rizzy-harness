//! A fork's own copy of the parent's repository (ADOPTION_PLAN C.2 part 2,
//! docs/fork.md, PLAN.md T92).
//!
//! **No git process runs with the parent's repository as its git
//! directory.** The parent's `.git` may have been written by an agent
//! (config, hooks, attributes, filters, `core.fsmonitor`, aliases, a
//! `commondir`, a gitfile), and the node is not sandboxed. `git worktree
//! add` and `git -C <parent>` read that config and run those hooks, so the
//! node never uses them. The one command that touches the parent is
//!
//! ```text
//! git clone --no-local --no-checkout --template= <parent root> <dest>
//! ```
//!
//! run by the hardened runner (cleared environment, the node's command-line
//! overrides, no global or system config) with its working directory
//! outside both folders. git documents this as the safe way to copy an
//! untrusted repository (the sources are quoted in docs/threats/T92.md):
//! `--no-local` reads the source through `upload-pack`, which "tries to
//! avoid any dangerous configuration options or hooks" of the repository it
//! serves (and honours `uploadpack.packObjectsHook` only from protected
//! configuration, which our command-line overrides are); a clone copies no
//! configuration and no hook; `--no-checkout` runs no filter and no
//! `post-checkout`; `--template=` copies no hook from the node's template
//! directory.
//!
//! Everything after the clone is the harness's own: the new repository's
//! config is written by the harness (and the parent's `origin` URL is read
//! AS DATA with `git config --file`, validated), the branch is made with
//! `update-ref`, the working files are written from the checkpoint shadow
//! with the checkpoint writers (attributes from the empty tree), and the
//! index is set to the branch's commit. Plain git in the result is as safe
//! as in any chat folder, because the config in force is the harness's.
//!
//! What is not copied: files git ignores (build output, `.env`,
//! `node_modules`), nested repositories and submodule contents, stashes,
//! and the parent's hooks and local config. Its `info/exclude` is copied
//! (patterns only), so the same files stay ignored.

use std::path::{Path, PathBuf};
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
    /// `.git` the clone reads.
    pub source: &'a Path,
    /// An existing, empty folder the node made and no session can reach.
    pub dest: &'a Path,
    /// The working directory of every git process that has no repository of
    /// its own: neither folder, nothing a session can write.
    pub outside: &'a Path,
    /// The workspace root holding `source` (the search for the repository
    /// does not go above it).
    pub ceiling: Option<&'a Path>,
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
    pub timeout: Duration,
    pub max_bytes: u64,
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
/// remote helpers), no whitespace, not longer than 2000 bytes.
pub(crate) fn upstream_url(u: &str) -> Option<&str> {
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
        Some("https" | "ssh" | "file") => Some(u),
        Some(_) => None,
        None => {
            if u.starts_with('/') {
                return Some(u);
            }
            // scp-like: `[user@]host:path`, the host with no `/` before the colon.
            let (host, path) = u.split_once(':')?;
            (!host.is_empty()
                && !path.is_empty()
                && !host.contains('/')
                && host
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.@[]".contains(&b)))
            .then_some(u)
        }
    }
}

/// A git command outside any repository: the hardened base, the working
/// directory `outside`, and the search for a repository above it stopped
/// at its parent.
fn outside_cmd(program: &Path, outside: &Path) -> Command {
    let mut c = hardened(program);
    c.current_dir(outside);
    if let Some(up) = outside.parent() {
        c.env("GIT_CEILING_DIRECTORIES", up);
    }
    c
}

/// A git command in the new repository: its git directory, its work tree,
/// and no attributes from anywhere (as the shadow's calls: no filter
/// whatever the tree's `.gitattributes` say). Its config is the harness's.
fn dest_cmd(dest: &Path, empty_tree: &str) -> Command {
    let mut c = hardened(&git_program());
    c.arg("--git-dir")
        .arg(dest.join(".git"))
        .arg("--work-tree")
        .arg(dest)
        .arg("-c")
        .arg("core.attributesFile=/dev/null")
        .arg("-c")
        .arg("core.excludesFile=/dev/null");
    c.env("GIT_ATTR_SOURCE", empty_tree).current_dir(dest);
    c
}

async fn run_out(c: Command, what: &str) -> Result<String, String> {
    let out = exec(c, None, GIT_TIMEOUT).await?;
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

/// The parent's repository as the node will read it, or why it is not
/// copied. Everything here reads files the session may write: bounded, no
/// link followed ([`user_repo`]).
async fn check_source(
    source: &Path,
    ceiling: Option<&Path>,
) -> Result<user_repo::UserRepo, String> {
    let (s, c) = (source.to_path_buf(), ceiling.map(Path::to_path_buf));
    tokio::task::spawn_blocking(move || {
        let user = match user_repo::find(&s, c.as_deref()) {
            user_repo::Found::Repo(r) if r.top == s => r,
            user_repo::Found::Repo(_) => {
                return Err("the folder is not the top of a git repository".into());
            }
            user_repo::Found::Unusual(why) => return Err(why),
            user_repo::Found::None => return Err("the folder is not a git repository".into()),
        };
        if user.head.is_none() {
            return Err("the repository has no commit yet, so there is nothing to copy".into());
        }
        if let Some(why) = &user.no_borrow {
            return Err(format!(
                "the repository is not copied: {why} (it would have to be read through a link \
                 or from another store)"
            ));
        }
        // `refs` is read through upload-pack too: a link there leads
        // outside the repository.
        let git = s.join(".git");
        for d in ["refs", "objects"] {
            if !std::fs::symlink_metadata(git.join(d)).is_ok_and(|m| m.is_dir()) {
                return Err(format!(".git/{d} is not a plain folder (a link?)"));
            }
        }
        if std::fs::symlink_metadata(git.join("objects/info/http-alternates")).is_ok() {
            return Err("the repository borrows objects from another store".into());
        }
        Ok(user)
    })
    .await
    .map_err(|e| e.to_string())?
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
    let user = check_source(p.source, p.ceiling).await?;
    clone(p).await?;
    check_dest(p.dest)?;
    let upstream = read_upstream(p).await;
    if let Some(u) = &upstream {
        let cfg = p.dest.join(".git/config");
        for (k, v) in [
            ("remote.upstream.url", u.as_str()),
            (
                "remote.upstream.fetch",
                "+refs/heads/*:refs/remotes/upstream/*",
            ),
        ] {
            let mut c = outside_cmd(&git_program(), p.outside);
            c.arg("config").arg("--file").arg(&cfg).arg(k).arg(v);
            run_out(c, "writing the copy's config").await?;
        }
    }
    let fmt_empty = empty_tree(if user.format == "sha256" {
        "sha256"
    } else {
        "sha1"
    });
    // The commit of the branch: the log's, when the clone has it.
    let (commit, exact) = pick_commit(p, fmt_empty).await?;
    let mut c = dest_cmd(p.dest, fmt_empty);
    c.args(["update-ref"])
        .arg(format!("refs/heads/{}", p.branch))
        .arg(&commit);
    run_out(c, "making the fork's branch").await?;
    let mut c = dest_cmd(p.dest, fmt_empty);
    c.args(["symbolic-ref", "HEAD"])
        .arg(format!("refs/heads/{}", p.branch));
    run_out(c, "pointing the copy at its branch").await?;
    // The clone made a local branch of the parent's current branch: it
    // would carry the parent's later commits. Remote-tracking refs stay.
    drop_clone_branches(p.dest, fmt_empty, p.branch).await;
    copy_exclude(user.exclude.as_deref(), p.dest).await;
    match p.snapshot {
        Some(snap) => {
            write_snapshot(p, snap).await?;
            let mut c = dest_cmd(p.dest, fmt_empty);
            c.args(["read-tree", &commit]);
            run_out(c, "setting the copy's index").await?;
        }
        None => {
            let mut c = dest_cmd(p.dest, fmt_empty);
            c.args(["read-tree", &commit]);
            run_out(c, "setting the copy's index").await?;
            let mut c = dest_cmd(p.dest, fmt_empty);
            c.args(["checkout-index", "-a", "-f"]);
            run_out(c, "writing the copy's files").await?;
        }
    }
    // The index has no stat data yet: one refresh now, not on the first
    // `git status` of the person.
    let mut c = dest_cmd(p.dest, fmt_empty);
    c.args(["update-index", "-q", "--refresh"]);
    let _ = exec(c, None, GIT_TIMEOUT).await;
    Ok(Filled {
        commit,
        head_exact: exact,
        upstream,
    })
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

/// The clone: the one command that touches the parent repository.
async fn clone(p: &Plan<'_>) -> Result<(), String> {
    use std::process::Stdio;
    #[allow(unused_mut)]
    let mut c: Command;
    #[cfg(test)]
    {
        c = match MUTATION.with(|m| m.get()) {
            Mutation::None => real_clone(p),
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
        c = real_clone(p);
    }
    c.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .process_group(0);
    let mut child = c.spawn().map_err(|e| format!("cannot start git: {e}"))?;
    let mut group = Group(child.id());
    let err_out = child.stderr.take().map(|mut s| {
        tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let mut kept = Vec::new();
            let mut buf = [0u8; 4096];
            while let Ok(n) = s.read(&mut buf).await {
                if n == 0 {
                    break;
                }
                let room = (64 * 1024usize).saturating_sub(kept.len());
                kept.extend_from_slice(&buf[..n.min(room)]);
            }
            kept
        })
    });
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

/// `git clone --no-local --no-checkout --template= -- <source> <dest>`,
/// hardened, in `outside`. `protocol.file.allow=always` after the base's
/// `protocol.allow=never`: the file transport is the one this clone needs
/// (the parent's path), and no other. Nothing is recursed into.
fn real_clone(p: &Plan<'_>) -> Command {
    let mut c = outside_cmd(&git_program(), p.outside);
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
        .arg(p.source)
        .arg(p.dest);
    c
}

/// The parent's `origin` URL, read as data: `git config --file` parses a
/// file and runs nothing; no includes are followed.
async fn read_upstream(p: &Plan<'_>) -> Option<String> {
    let mut c = outside_cmd(&git_program(), p.outside);
    c.arg("config")
        .arg("--file")
        .arg(p.source.join(".git/config"))
        .args(["--no-includes", "--get", "remote.origin.url"]);
    let out = exec(c, None, Duration::from_secs(10)).await.ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    upstream_url(&text).map(str::to_string)
}

/// The commit for the branch and whether it is the log's.
async fn pick_commit(p: &Plan<'_>, empty: &str) -> Result<(String, bool), String> {
    if let Some(h) = p.head {
        let mut c = dest_cmd(p.dest, empty);
        c.args(["cat-file", "-e"]).arg(format!("{h}^{{commit}}"));
        if exec(c, None, GIT_TIMEOUT)
            .await
            .is_ok_and(|o| o.status.success())
        {
            return Ok((h.to_string(), true));
        }
    }
    let mut c = dest_cmd(p.dest, empty);
    c.args(["rev-parse", "--verify", "-q", "HEAD^{commit}"]);
    let id = run_out(c, "reading the copy's HEAD").await?;
    if object_id(&id) {
        Ok((id, false))
    } else {
        Err("the copy has no commit to start from".into())
    }
}

/// Delete the local branches `git clone` made (the parent's current
/// branch), leaving `keep`.
async fn drop_clone_branches(dest: &Path, empty: &str, keep: &str) {
    let mut c = dest_cmd(dest, empty);
    c.args(["for-each-ref", "--format=%(refname)", "refs/heads/"]);
    let Ok(list) = run_out(c, "listing branches").await else {
        return;
    };
    for r in list.lines() {
        if r == format!("refs/heads/{keep}") || !r.starts_with("refs/heads/") {
            continue;
        }
        let mut c = dest_cmd(dest, empty);
        c.args(["update-ref", "-d", "--", r]);
        let _ = exec(c, None, GIT_TIMEOUT).await;
    }
}

/// The parent's `info/exclude` (ignore patterns: text git reads as
/// patterns, nothing runs), bounded, no link followed.
async fn copy_exclude(from: Option<&Path>, dest: &Path) {
    let Some(from) = from.map(Path::to_path_buf) else {
        return;
    };
    let text = tokio::task::spawn_blocking(move || user_repo::read_bounded(&from, 1 << 20))
        .await
        .ok()
        .flatten();
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

/// A parent's folder's size: what a copy of its `.git` and its checkpoint
/// tree needs on disk, counted from files (`.git`) and the snapshot's
/// listing (the tree); capped so a huge walk ends.
pub(crate) async fn tree_bytes(shadow: &Repo, snap: &str) -> u64 {
    if !object_id(snap) {
        return 0;
    }
    run_raw(shadow, &["ls-tree", "-r", "-l", "-z", snap], None, None)
        .await
        .map(|out| {
            out.split(|b| *b == 0)
                .filter_map(|l| {
                    let meta = l.split(|b| *b == b'\t').next()?;
                    let size = meta.split(|b| *b == b' ').rfind(|f| !f.is_empty())?;
                    std::str::from_utf8(size).ok()?.parse::<u64>().ok()
                })
                .fold(0u64, u64::saturating_add)
        })
        .unwrap_or(0)
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
/// (no link followed), counting no further than `cap`.
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
