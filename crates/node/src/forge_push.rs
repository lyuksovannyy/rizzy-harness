//! The mechanics of the `push` tool (ADOPTION_PLAN X.27 step 4, PLAN.md
//! T101, `docs/forge-push.md`): which commit, the node's own repository it
//! is pushed from, and the one `git push`.
//!
//! **No git process of the node runs in the folder's repository** (T19):
//! its `.git/config` (`url.*.insteadOf`, `include.path`, hooks, helpers)
//! would steer a push there, and `-c` cannot remove such keys. The commit
//! is read from the refs as files ([`crate::git::user_repo`]); the objects
//! are hard-linked (copied where a link is refused) by this module's own
//! walk into a bare repository the node makes in its staging folder: only
//! loose objects (`objects/??/<hex>`) and `objects/pack/pack-<hex>.pack` /
//! `.idx`, regular files only, no link followed, nothing else of
//! `objects/` (no `info/alternates`, `http-alternates`, `commit-graph`,
//! `multi-pack-index`, `.promisor`, `.keep`). git reads nothing of the
//! folder but those bytes. The experiment that chose this over
//! `objects/info/alternates`: an alternates chain the agent writes pushed
//! another repository's commit (`docs/forge-push.md` §3).
//!
//! **A file the agent rewrites after it was linked** shares its inode with
//! the staged one: git then reads the new bytes. The forge names every
//! object by the hash of what it received (`index-pack`) and refuses a ref
//! update whose objects it cannot all reach (`receive-pack`'s connectivity
//! check), so such a rewrite can make the push fail, never put a commit
//! other than the pinned id, or other content under that id, on the forge
//! (`a_rewritten_object_only_fails_the_push`).
//!
//! **The token** reaches the one `git push` as the clone's does
//! (`forge_clone::helper`, `token_pipe`): a close-on-exec pipe at its
//! standard input, a helper that answers only that scheme, host and path.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use harness_core::Secret;
use harness_core::forge::valid_branch;

use crate::forge_clone::{self, SCHEME};

/// The most files one push links or copies into its repository: a store
/// of more loose objects than this is refused with the advice to pack it.
pub const MAX_FILES: usize = 200_000;

/// How long the link step may take.
pub const STAGE_TIME: Duration = Duration::from_secs(120);

/// How long the reads of the commit before the card may take.
const READ_TIME: Duration = Duration::from_secs(20);

/// The most of a commit object read for the card.
const COMMIT_READ: usize = 64 * 1024;

/// The longest subject shown on the card.
pub const SUBJECT_MAX: usize = 200;

/// The bounds of one push.
#[derive(Debug, Clone)]
pub(crate) struct Bounds {
    /// The push itself (`[limits] clone_timeout_s`).
    pub timeout: Duration,
    /// The most bytes copied where a link is refused (`[limits]
    /// clone_max_mb`).
    pub max_copy: u64,
    pub max_files: usize,
    pub stage_time: Duration,
}

impl Bounds {
    pub(crate) fn of(policy: &crate::policy::NodePolicy) -> Self {
        Self {
            timeout: Duration::from_secs(policy.limits.clone_timeout_s),
            max_copy: policy.limits.clone_max_mb.saturating_mul(1024 * 1024),
            max_files: MAX_FILES,
            stage_time: STAGE_TIME,
        }
    }
}

/// The branch and commit to push, read from the folder's files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Pin {
    pub branch: String,
    pub commit: String,
}

/// Read what `branch` (default: the branch `HEAD` names) points at in the
/// repository whose top folder is `top`. Every refusal is a plain sentence.
pub(crate) fn pin(top: &Path, branch: Option<&str>) -> Result<Pin, String> {
    use crate::git::user_repo;
    let branch = match branch {
        Some(b) => b.to_string(),
        None => match user_repo::head_label(top) {
            Some((Some(b), _)) => b,
            Some((None, _)) => {
                return Err(
                    "the folder's repository is not on a branch (a detached HEAD): \
                     name the branch to push"
                        .into(),
                );
            }
            None => {
                return Err(
                    "the folder's repository could not be read (its .git must be a \
                     plain repository directory)"
                        .into(),
                );
            }
        },
    };
    if !valid_branch(&branch) {
        return Err(
            "the folder's current branch has a name push does not take; name \
             another branch"
                .into(),
        );
    }
    match user_repo::branch_commit(top, &branch) {
        Some(commit) => Ok(Pin { branch, commit }),
        None => Err(format!(
            "the branch `{branch}` has no commit in the folder's repository: commit first"
        )),
    }
}

/// The node's repository for one push, removed when dropped.
pub(crate) struct Staged {
    /// `<staging>/push-<random>`: the bare repository is `repo`, git's
    /// empty home `home`.
    dir: PathBuf,
    pub repo: PathBuf,
    pub linked: usize,
    pub copied: usize,
    pub copied_bytes: u64,
}

impl Drop for Staged {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Staged {
    pub(crate) fn dir(&self) -> &Path {
        &self.dir
    }
}

#[cfg(test)]
thread_local! {
    /// Tests: behave as a node whose links are refused (another owner's
    /// files under `fs.protected_hardlinks`, a cross-device folder).
    static NO_LINKS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
pub(crate) fn refuse_links(on: bool) {
    NO_LINKS.with(|n| n.set(on));
}

fn link(src: &Path, dst: &Path) -> std::io::Result<()> {
    #[cfg(test)]
    if NO_LINKS.with(|n| n.get()) {
        return Err(std::io::Error::from_raw_os_error(nix::libc::EPERM));
    }
    std::fs::hard_link(src, dst)
}

/// `src` copied into `dst` (made new), at most `room` bytes: opened with no
/// link followed and never waiting (a FIFO), a regular file on the opened
/// descriptor. The bytes copied.
fn copy_bounded(src: &Path, dst: &Path, room: u64) -> Result<u64, String> {
    use nix::fcntl::OFlag;
    use std::io::Read;
    let fd = nix::fcntl::open(
        src,
        OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map_err(|e| format!("cannot read an object file: {e}"))?;
    let f = std::fs::File::from(fd);
    if !f.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("an object file is not a regular file".into());
    }
    let mut out = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dst)
        .map_err(|e| format!("cannot write the node's copy: {e}"))?;
    let n = std::io::copy(&mut f.take(room.saturating_add(1)), &mut out)
        .map_err(|e| format!("cannot copy an object file: {e}"))?;
    if n > room {
        return Err(TOO_BIG.into());
    }
    Ok(n)
}

const TOO_BIG: &str = "the folder's objects are larger than this node copies for a push \
     ([limits] clone_max_mb); nothing was pushed";

/// Make the node's repository for a push of the repository at `top` in
/// `staging` (the node's staging folder): a bare repository of `format`
/// (`sha1`, `sha256`) holding links to (or bounded copies of) the folder's
/// loose objects and packs. Blocking; run it off the async threads.
pub(crate) fn stage(
    staging: &Path,
    top: &Path,
    format: &str,
    b: &Bounds,
) -> Result<Staged, String> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    let started = Instant::now();
    let objects = top.join(".git").join("objects");
    // The store itself: a real directory, no link anywhere on its path.
    let real = objects
        .canonicalize()
        .map_err(|_| "the folder's object store (.git/objects) is missing".to_string())?;
    if real != objects || !std::fs::symlink_metadata(&objects).is_ok_and(|m| m.is_dir()) {
        return Err(
            "the folder's object store (.git/objects) is a link; nothing was pushed".into(),
        );
    }
    let mut r = [0u8; 8];
    getrandom::fill(&mut r).expect("OS randomness");
    let dir = staging.join(format!("push-{}", hex::encode(r)));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&dir)
        .map_err(|e| format!("cannot make a folder in the node's staging folder: {e}"))?;
    let mut staged = Staged {
        repo: dir.join("repo.git"),
        dir,
        linked: 0,
        copied: 0,
        copied_bytes: 0,
    };
    let repo = staged.repo.clone();
    for d in ["objects/pack", "refs/heads", "refs/tags", "home"] {
        let p = if d == "home" {
            staged.dir.join(d)
        } else {
            repo.join(d)
        };
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&p)
            .map_err(|e| format!("cannot make the node's repository: {e}"))?;
    }
    std::fs::write(repo.join("HEAD"), "ref: refs/heads/main\n")
        .map_err(|e| format!("cannot make the node's repository: {e}"))?;
    let config = if format == "sha256" {
        "[core]\n\trepositoryformatversion = 1\n\tbare = true\n[extensions]\n\tobjectformat = sha256\n"
    } else {
        "[core]\n\trepositoryformatversion = 0\n\tbare = true\n"
    };
    std::fs::write(repo.join("config"), config)
        .map_err(|e| format!("cannot make the node's repository: {e}"))?;
    let hexlen = if format == "sha256" { 64 } else { 40 };
    let is_hex = |s: &str| {
        s.bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    };
    let dev = std::fs::symlink_metadata(&objects)
        .map_err(|e| e.to_string())?
        .dev();
    let take = |src: PathBuf, dst: PathBuf, st: &mut Staged| -> Result<(), String> {
        if st.linked + st.copied >= b.max_files {
            return Err(format!(
                "the folder's repository has more than {} object files; pack them first (`git \
                 gc` in the folder) and push again",
                b.max_files
            ));
        }
        if (st.linked + st.copied).is_multiple_of(100) && started.elapsed() > b.stage_time {
            return Err(format!(
                "reading the folder's objects took longer than {} s; pack them first (`git gc` \
                 in the folder) and push again",
                b.stage_time.as_secs()
            ));
        }
        let md = match std::fs::symlink_metadata(&src) {
            Ok(m) => m,
            // Gone since it was listed (a `git gc`): not needed, or the
            // push says which object is missing.
            Err(_) => return Ok(()),
        };
        if !md.is_file() {
            return Ok(());
        }
        match link(&src, &dst) {
            Ok(()) => {
                // A link names the inode it found: if the agent swapped a
                // symbolic link in between, that is what was linked, and
                // git would follow it. Refused, never followed.
                if !std::fs::symlink_metadata(&dst).is_ok_and(|m| m.is_file() && m.dev() == dev) {
                    return Err(
                        "an object file of the folder changed into a link while it was read; \
                         nothing was pushed"
                            .into(),
                    );
                }
                st.linked += 1;
            }
            Err(_) => {
                let room = b.max_copy.saturating_sub(st.copied_bytes);
                let n = copy_bounded(&src, &dst, room)?;
                st.copied += 1;
                st.copied_bytes += n;
            }
        }
        Ok(())
    };
    let entries = std::fs::read_dir(&objects)
        .map_err(|e| format!("cannot read the folder's object store: {e}"))?;
    for e in entries.flatten() {
        let name = e.file_name();
        let Some(name) = name.to_str() else { continue };
        let Ok(ft) = e.file_type() else { continue };
        if !ft.is_dir() {
            continue;
        }
        if name.len() == 2 && is_hex(name) {
            let sub = repo.join("objects").join(name);
            let mut made = false;
            for f in std::fs::read_dir(e.path()).into_iter().flatten().flatten() {
                let fname = f.file_name();
                let Some(fname) = fname.to_str() else {
                    continue;
                };
                if fname.len() != hexlen - 2 || !is_hex(fname) {
                    continue;
                }
                if !made {
                    std::fs::create_dir(&sub).map_err(|e| e.to_string())?;
                    made = true;
                }
                take(f.path(), sub.join(fname), &mut staged)?;
            }
        } else if name == "pack" {
            for f in std::fs::read_dir(e.path()).into_iter().flatten().flatten() {
                let fname = f.file_name();
                let Some(fname) = fname.to_str() else {
                    continue;
                };
                let Some(stem) = fname
                    .strip_suffix(".pack")
                    .or_else(|| fname.strip_suffix(".idx"))
                else {
                    continue;
                };
                let Some(id) = stem.strip_prefix("pack-") else {
                    continue;
                };
                if !(id.len() == 40 || id.len() == 64) || !is_hex(id) {
                    continue;
                }
                take(f.path(), repo.join("objects/pack").join(fname), &mut staged)?;
            }
        }
        // Anything else (`info/`: alternates, http-alternates, packs,
        // commit-graph; `incoming-*`) is never taken.
    }
    Ok(staged)
}

/// A git command in the node's repository `staged`, with the hardened base,
/// in the node's helper jail when there is one (read-write on the push's
/// own folder only, the network open), its home the push's empty folder.
fn git_in(
    staged: &Staged,
    jail: Option<&harness_tools::sandbox::Sandbox>,
) -> tokio::process::Command {
    let program = crate::git::git_program();
    let base = match jail {
        Some(s) => {
            let mut c = s.helper_net(&[harness_tools::sandbox::Grant::system(staged.dir())], &[]);
            c.arg(&program);
            c
        }
        None => std::process::Command::new(&program),
    };
    let mut c = crate::git::hardened_on(base);
    let home = staged.dir().join("home");
    c.env("HOME", &home)
        .env("XDG_CONFIG_HOME", &home)
        .env("CURL_HOME", &home)
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .arg("--git-dir")
        .arg(&staged.repo)
        .current_dir(staged.dir());
    c
}

/// The pinned commit's subject from the node's repository (agent-written
/// text: one line, cleaned, at most [`SUBJECT_MAX`] characters); `Err` when
/// it is not a commit there.
pub(crate) async fn subject(
    staged: &Staged,
    commit: &str,
    jail: Option<&harness_tools::sandbox::Sandbox>,
) -> Result<String, String> {
    let mut c = git_in(staged, jail);
    c.args(["cat-file", "commit", commit])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .process_group(0);
    let child = c.spawn().map_err(|e| format!("cannot start git: {e}"))?;
    let group = crate::git::Group(child.id());
    let out = tokio::time::timeout(READ_TIME, child.wait_with_output()).await;
    drop(group);
    let out = match out {
        Ok(Ok(o)) if o.status.success() => o.stdout,
        _ => {
            return Err(format!(
                "the commit {} is not in the folder's repository (or not a commit)",
                &commit[..12.min(commit.len())]
            ));
        }
    };
    let text = String::from_utf8_lossy(&out[..out.len().min(COMMIT_READ)]).to_string();
    let body = text.split_once("\n\n").map(|(_, b)| b).unwrap_or("");
    let line = body.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    Ok(harness_core::text::clean_line(line, SUBJECT_MAX))
}

/// What one push did, from git's porcelain line (the forge's answer).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Pushed {
    /// git's flag: ` ` fast-forward, `*` new branch, `=` up to date, `!`
    /// rejected, anything else as given.
    pub flag: char,
    /// The summary (`abc..def`, `[new branch]`, `[rejected] (…)`), cleaned.
    pub summary: String,
}

/// git's configuration for a push, on the command line (it beats every
/// file; the node's repository has none of the folder's anyway).
pub(crate) fn push_config(helper: &str, proactive: bool) -> Vec<String> {
    let mut c: Vec<String> = [
        format!("protocol.{SCHEME}.allow=always").as_str(),
        "protocol.file.allow=never",
        "protocol.ext.allow=never",
        "protocol.ssh.allow=never",
        "protocol.git.allow=never",
        "http.followRedirects=false",
        "http.proxy=",
        "credential.useHttpPath=true",
        "credential.interactive=false",
        "push.recurseSubmodules=no",
        "push.gpgSign=false",
        "push.followTags=false",
        "push.pushOption=",
        "push.negotiate=false",
        "advice.pushUpdateRejected=false",
        "advice.pushNonFFCurrent=false",
        "advice.pushNonFFMatching=false",
        "advice.pushFetchFirst=false",
        "advice.pushNeedsForce=false",
        "core.bigFileThreshold=1m",
        "pack.windowMemory=64m",
        "pack.deltaCacheSize=16m",
        "pack.threads=2",
        "transfer.bundleURI=false",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    c.push(format!("credential.helper={helper}"));
    if proactive {
        c.push("http.proactiveAuth=basic".into());
    }
    c
}

/// Push `commit` from `staged` to `refs/heads/<branch>` of
/// `<scheme>://<host>/<full_name>.git`, with `login` and `token` for that
/// host and path only. `Ok` with the forge's answer, a rejection included;
/// `Err` when git could not ask (a connection, a credential, a time limit),
/// its text cleaned of the token, escapes and hints.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn push(
    staged: &Staged,
    jail: Option<&harness_tools::sandbox::Sandbox>,
    host: &str,
    full_name: &str,
    branch: &str,
    commit: &str,
    login: &str,
    token: &Secret<String>,
    bounds: &Bounds,
) -> Result<Pushed, String> {
    if !crate::policy::valid_forge_host(host)
        || !forge_clone::valid_full_name(full_name)
        || !valid_branch(branch)
        || !(commit.len() == 40 || commit.len() == 64)
        || !commit.bytes().all(|b| b.is_ascii_hexdigit())
        || !forge_clone::valid_username(login)
        || !forge_clone::valid_token(token.expose())
    {
        return Err("the push's target or credential is malformed; nothing was pushed".into());
    }
    let path = format!("{full_name}.git");
    let (stdin, ino) = forge_clone::token_pipe(token)?;
    let helper = forge_clone::helper(SCHEME, host, &path, login, ino)?;
    let proactive = forge_clone::proactive_auth().await;
    let mut c = git_in(staged, jail);
    for o in push_config(&helper, proactive) {
        c.arg("-c").arg(o);
    }
    c.args([
        "push",
        "--porcelain",
        "--no-verify",
        "--no-follow-tags",
        "--recurse-submodules=no",
        "--",
        &format!("{SCHEME}://{host}/{full_name}.git"),
        &format!("{commit}:refs/heads/{branch}"),
    ])
    .stdin(Stdio::from(stdin))
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true)
    .process_group(0);
    let mut child = c.spawn().map_err(|e| format!("cannot start git: {e}"))?;
    let mut group = crate::git::Group(child.id());
    let bounded = |s: Option<tokio::process::ChildStdout>| {
        s.map(|mut s| {
            tokio::spawn(async move {
                use tokio::io::AsyncReadExt;
                let mut kept = Vec::new();
                let mut buf = [0u8; 4096];
                while let Ok(n) = s.read(&mut buf).await {
                    if n == 0 {
                        break;
                    }
                    let room = forge_clone::STDERR_READ.saturating_sub(kept.len());
                    kept.extend_from_slice(&buf[..n.min(room)]);
                }
                kept
            })
        })
    };
    let out_task = bounded(child.stdout.take());
    let err_task = child.stderr.take().map(|mut s| {
        tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let mut kept = Vec::new();
            let mut buf = [0u8; 4096];
            while let Ok(n) = s.read(&mut buf).await {
                if n == 0 {
                    break;
                }
                let room = forge_clone::STDERR_READ.saturating_sub(kept.len());
                kept.extend_from_slice(&buf[..n.min(room)]);
            }
            kept
        })
    });
    let status = match tokio::time::timeout(bounds.timeout, child.wait()).await {
        Ok(Ok(st)) => {
            group.0 = None;
            st
        }
        Ok(Err(e)) => return Err(format!("git: {e}")),
        Err(_) => {
            drop(group);
            let _ = child.wait().await;
            return Err(format!(
                "the push took longer than this node allows ([limits] clone_timeout_s = {} s); \
                 git was stopped. The forge may or may not have the branch now",
                bounds.timeout.as_secs()
            ));
        }
    };
    let out = match out_task {
        Some(t) => t.await.unwrap_or_default(),
        None => Vec::new(),
    };
    let err = match err_task {
        Some(t) => t.await.unwrap_or_default(),
        None => Vec::new(),
    };
    let out = clean_text(&String::from_utf8_lossy(&out), login, token);
    if let Some(p) = porcelain(&out, branch) {
        return Ok(p);
    }
    let err = clean_text(&String::from_utf8_lossy(&err), login, token);
    if forge_clone::credential_missing(&err) {
        return Err(forge_clone::CREDENTIAL_MISSING.replace("cloned", "pushed"));
    }
    let _ = status;
    Err(format!("git push failed: {}", last_lines(&err)))
}

/// git's text as the model and the log may see it: terminal escapes
/// stripped (the shell's own stripper), the token in every form removed,
/// `hint:` lines dropped.
pub(crate) fn clean_text(s: &str, login: &str, token: &Secret<String>) -> String {
    let mut strip = harness_tools::filter::AnsiStripper::new();
    let mut t = strip.push(s);
    t.push_str(&strip.finish());
    let t = forge_clone::without_token(&t, login, token);
    t.lines()
        .filter(|l| !l.trim_start().starts_with("hint:"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The last three non-empty lines, one line, controls dropped, at most
/// [`forge_clone::ERROR_MAX`] characters.
pub(crate) fn last_lines(s: &str) -> String {
    let lines: Vec<&str> = s.lines().filter(|l| !l.trim().is_empty()).collect();
    let tail = lines[lines.len().saturating_sub(3)..].join(" / ");
    let out = harness_core::text::clean_line(&tail, forge_clone::ERROR_MAX);
    if out.is_empty() {
        "git said nothing".into()
    } else {
        out
    }
}

/// The porcelain line for `refs/heads/<branch>`: `<flag>\t<from>:<to>\t<summary>`.
fn porcelain(out: &str, branch: &str) -> Option<Pushed> {
    let to = format!(":refs/heads/{branch}");
    out.lines().find_map(|l| {
        let mut parts = l.splitn(3, '\t');
        let flag = parts.next()?;
        let refs = parts.next()?;
        let summary = parts.next().unwrap_or("");
        let mut fc = flag.chars();
        let f = fc.next()?;
        (fc.next().is_none() && refs.ends_with(&to)).then(|| Pushed {
            flag: f,
            summary: harness_core::text::clean_line(summary, 300),
        })
    })
}

#[cfg(test)]
#[path = "forge_push_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "forge_push_runtime_tests.rs"]
mod runtime_tests;
