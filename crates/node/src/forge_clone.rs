//! A new project's folder cloned from a forge repository (`project.clone`,
//! ADOPTION_PLAN X.27 step 3, PLAN.md T86).
//!
//! **The node decides where from.** The master sends a display name, a host
//! and a repository name (`owner/name`, GitLab `group/…/name`), and for a
//! private repository the login and token of the person's connection. The
//! host must be one of node.toml's `[forge] hosts`, exactly as written; the
//! name must have the shape every forge kind shares ([`valid_full_name`]);
//! the node builds the URL itself, `https://<host>/<full_name>.git`. No
//! scheme, port, user or path of the master's reaches git.
//!
//! **The token reaches one git process only, through a pipe.** It is never
//! in an argument, the environment, a file or git's config. The node writes
//! it into a pipe it made close-on-exec, closes the writing end, and hands
//! the reading end to git as its standard input; the standard library puts
//! it at descriptor 0 in the child after the fork, so no other child of the
//! node can inherit it. git reads nothing from its standard input during a
//! clone. Its credential helper (this module's shell function, [`helper`])
//! answers `get` only for exactly this scheme, host and path (with
//! `credential.useHttpPath`), finds the git it was started under by walking
//! up its parents until one has that very pipe (`pipe:[<inode>]`) at
//! descriptor 0, and reads the token from `/proc/<that git>/fd/0` with the
//! shell's own `read`, printing it with the shell's own `printf`: no program
//! is started with it. It relies on Linux `/proc` and on reading a
//! descriptor of a process of the same user (ptrace read access: any
//! process of the node's user can do the same while a clone runs, which
//! PLAN.md T86 states).
//!
//! The design note's first plan, the token as an `http.extraHeader` in git's
//! environment, leaked it to another host on a redirect and sat in
//! `/proc/<git>/environ` for the whole clone (the experiment in
//! `.claude/handoff-2026-11-15/x27-step3/`); redirects are refused here
//! (`http.followRedirects=false`), and the helper checks the host besides.
//!
//! **Bounds.** The clone is built in the node's staging folder like
//! `project.create`'s repository and moved in with the same no-replace
//! rename; `[limits] clone_timeout_s` and `clone_max_mb` (the staging
//! folder's size is measured every [`SIZE_EVERY`] while git runs and once
//! after) end git's whole process group and remove what it wrote; a clone
//! counts against `[limits] projects_per_hour`; one runs at a time per node.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use harness_core::Secret;

use crate::policy::NodePolicy;
use crate::projects::{self, Build, Made};

/// Why a node clones nothing.
pub const OFF: &str = "this node clones no repositories: its node.toml turns cloning off \
     ([forge] clone, [forge] hosts) or makes no project folders ([projects] dir)";

/// How often the staging folder's size is measured while git runs: one walk
/// of the folder (a stat of each entry; a few thousand entries take a few
/// milliseconds), so a fast download overshoots the cap by at most what
/// arrives in this time.
pub const SIZE_EVERY: Duration = Duration::from_millis(500);

/// The URL's scheme. Tests clone over plain HTTP from a fake server on
/// 127.0.0.1; nothing a person sets changes it.
const PROD_SCHEME: &str = "https";
const SCHEME: &str = if cfg!(test) { "http" } else { PROD_SCHEME };

/// The most of git's error output kept, and passed on (in characters).
const STDERR_READ: usize = 64 * 1024;
const ERROR_MAX: usize = 300;

/// The first git version with `http.proactiveAuth` (git 2.46.0).
const PROACTIVE_SINCE: (u32, u32) = (2, 46);

/// What the master asks for.
pub(crate) struct CloneRequest {
    pub name: String,
    pub host: String,
    pub full_name: String,
    pub username: Option<String>,
    pub token: Option<Secret<String>>,
}

/// `owner/name` in the shape every forge kind shares (the strictest of
/// GitHub, GitLab, Gitea and Forgejo, since the node does not know which
/// one a host runs): at most [`harness_protocol::MAX_CLONE_NAME`] bytes, 2
/// to 21 parts of `[A-Za-z0-9._-]`, no part empty, `.` or `..`, starting
/// with `-`, or ending in `.git` or `.atom`.
pub fn valid_full_name(s: &str) -> bool {
    if s.is_empty() || s.len() > harness_protocol::MAX_CLONE_NAME {
        return false;
    }
    let parts: Vec<&str> = s.split('/').collect();
    (2..=21).contains(&parts.len())
        && parts.iter().all(|p| {
            !p.is_empty()
                && *p != "."
                && *p != ".."
                && !p.starts_with('-')
                && !p.ends_with(".git")
                && !p.ends_with(".atom")
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        })
}

/// A forge login: 1 to 100 of `[A-Za-z0-9._-]`, not starting with `-`.
pub fn valid_username(s: &str) -> bool {
    (1..=100).contains(&s.len())
        && !s.starts_with('-')
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// A token: 1 to [`harness_protocol::MAX_CLONE_TOKEN`] visible ASCII
/// characters (no space, no line break: it cannot add a line to git's
/// credential exchange).
fn valid_token(t: &str) -> bool {
    (1..=harness_protocol::MAX_CLONE_TOKEN).contains(&t.len())
        && t.bytes().all(|b| b.is_ascii_graphic())
}

/// The URL the node clones, and the one the repository's `origin` keeps:
/// no credential, ever.
fn url(scheme: &str, host: &str, full_name: &str) -> String {
    format!("{scheme}://{host}/{full_name}.git")
}

/// The request checked against this node's policy, before anything is made
/// or started. Every refusal names the rule, never the value sent.
fn check(policy: &NodePolicy, req: &CloneRequest) -> Result<(), String> {
    let pp = &policy.projects;
    if !policy.forge.clones()
        || pp.located.is_none()
        || pp.parent_id.is_none()
        || pp.staging.is_none()
        || policy.limits.projects_per_hour == 0
    {
        return Err(OFF.into());
    }
    projects::check_name(&req.name)?;
    if !crate::policy::valid_forge_host(&req.host) || !policy.forge.allows(&req.host) {
        return Err(
            "this node does not clone from that host: only from the hosts its node.toml lists \
             ([forge] hosts)"
                .into(),
        );
    }
    if !valid_full_name(&req.full_name) {
        return Err(
            "that is not a repository name this node clones (owner/name, letters, digits, . _ -)"
                .into(),
        );
    }
    match (&req.username, &req.token) {
        (None, None) => {}
        (Some(u), Some(t)) if valid_username(u) && valid_token(t.expose()) => {}
        _ => {
            return Err("the repository's credential is malformed; nothing was cloned".into());
        }
    }
    Ok(())
}

/// The bounds of one clone.
pub(crate) struct Bounds {
    pub timeout: Duration,
    pub max_bytes: u64,
    pub every: Duration,
}

impl Bounds {
    pub(crate) fn of(policy: &NodePolicy) -> Self {
        Self {
            timeout: Duration::from_secs(policy.limits.clone_timeout_s),
            max_bytes: policy.limits.clone_max_mb.saturating_mul(1024 * 1024),
            every: SIZE_EVERY,
        }
    }
}

/// Clone `req` into a new project's folder; its absolute path. `made`
/// bounds how many folders per hour (shared with `project.create`).
pub(crate) async fn clone(
    policy: &NodePolicy,
    made: &Made,
    req: CloneRequest,
    bounds: Bounds,
    make_suffix: &(dyn Fn() -> String + Sync),
) -> Result<PathBuf, String> {
    check(policy, &req)?;
    made.room(policy.limits.projects_per_hour)?;
    let dir = policy.projects.located.clone().ok_or(OFF)?;
    let base = projects::slug(&req.name);
    let names: Vec<String> = (0..projects::TRIES)
        .map(|_| format!("{base}-{}", make_suffix()))
        .collect();
    let p = policy.clone();
    let (top, build) = tokio::task::spawn_blocking(move || {
        let top = projects::open_projects(&p, &dir)?;
        let build = Build::new(&p)?;
        Ok::<_, String>((top, build))
    })
    .await
    .map_err(|e| format!("making the folder: {e}"))??;
    let fetched = run_git(&build, &req, &bounds).await.and_then(|()| {
        build
            .check(false)
            .map_err(|e| format!("the cloned repository is not usable: {e}"))
    });
    if let Err(e) = fetched {
        tokio::task::spawn_blocking(move || build.discard())
            .await
            .ok();
        return Err(e);
    }
    let p = policy.clone();
    let path = tokio::task::spawn_blocking(move || build.move_into(&p, &top, &names))
        .await
        .map_err(|e| format!("moving the folder: {e}"))??;
    made.record();
    Ok(path)
}

/// Whether this git sends the credential with its first request
/// (`http.proactiveAuth`, git 2.46): asked once.
async fn proactive_auth() -> bool {
    #[cfg(test)]
    if OLD_GIT.with(|o| o.get()) {
        return false;
    }
    static KNOWN: tokio::sync::OnceCell<bool> = tokio::sync::OnceCell::const_new();
    *KNOWN
        .get_or_init(|| async {
            let mut c = crate::git::hardened(&crate::git::git_program());
            c.arg("version")
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true);
            match tokio::time::timeout(Duration::from_secs(10), c.output()).await {
                Ok(Ok(o)) if o.status.success() => {
                    version_at_least(&String::from_utf8_lossy(&o.stdout), PROACTIVE_SINCE)
                }
                _ => false,
            }
        })
        .await
}

/// `git version 2.56.1` (or `… (Apple Git-143)`) at `min` or later.
fn version_at_least(out: &str, min: (u32, u32)) -> bool {
    let Some(v) = out.split_whitespace().nth(2) else {
        return false;
    };
    let mut it = v.split('.').map(|n| n.parse::<u32>().ok());
    match (it.next().flatten(), it.next().flatten()) {
        (Some(a), Some(b)) => (a, b) >= min,
        _ => false,
    }
}

/// The credential helper: a shell function git runs as `sh -c '… "$@"' …
/// <operation>`. It answers `get` only, only when git asks for exactly
/// `scheme`, `host` and `path`; it then walks up at most six parents to the
/// one whose descriptor 0 is the pipe `pipe_ino`, and reads one line, the
/// token, from it. Every value it names is checked by the caller to be of
/// `[A-Za-z0-9._:/-]` (none needs quoting inside `'…'`).
fn helper(
    scheme: &str,
    host: &str,
    path: &str,
    username: &str,
    pipe_ino: u64,
) -> Result<String, String> {
    let plain = |v: &str| {
        !v.is_empty()
            && v.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b':' | b'/'))
    };
    if ![scheme, host, path, username].into_iter().all(plain) {
        return Err("an unchecked value for the credential helper".into());
    }
    Ok(format!(
        "!f() {{ test \"$1\" = get || exit 0; p=; h=; u=; \
         while IFS= read -r l; do case \"$l\" in \
         protocol=*) p=${{l#protocol=}};; host=*) h=${{l#host=}};; path=*) u=${{l#path=}};; \
         '') break;; esac; done; \
         test \"$p\" = '{scheme}' && test \"$h\" = '{host}' && test \"$u\" = '{path}' || exit 0; \
         x=$PPID; n=0; \
         while test $n -lt 6; do \
         test \"$(readlink /proc/$x/fd/0)\" = 'pipe:[{pipe_ino}]' && break; \
         read -r s < /proc/$x/stat || exit 0; s=${{s##*) }}; set -- $s; x=$2; n=$((n+1)); \
         done; test $n -lt 6 || exit 0; \
         IFS= read -r t < /proc/$x/fd/0 || exit 0; test -n \"$t\" || exit 0; \
         printf 'username=%s\\npassword=%s\\n' '{username}' \"$t\"; }}; f"
    ))
}

/// git's configuration for a clone, on the command line (where it beats
/// every file): only `scheme` as a transport, no redirect, no credential
/// but `helper`'s, nothing that runs a command, objects checked.
fn clone_config(scheme: &str, helper: Option<&str>, proactive: bool) -> Vec<String> {
    let mut c: Vec<String> = [
        // The hardened base forbids every transport; this one alone again.
        format!("protocol.{scheme}.allow=always").as_str(),
        "protocol.file.allow=never",
        "protocol.ext.allow=never",
        "protocol.ssh.allow=never",
        "protocol.git.allow=never",
        // A redirect to another host would take the credential with it
        // (git re-bases on the redirect target; the experiment).
        "http.followRedirects=false",
        // Off by git's default today; off here whatever a later git does:
        // a bundle or packfile URI the server advertises is another URL.
        "transfer.bundleURI=false",
        "fetch.bundleURI=",
        "fetch.uriProtocols=",
        "http.proxy=",
        "credential.useHttpPath=true",
        "credential.interactive=false",
        "core.hooksPath=/dev/null",
        "core.fsmonitor=false",
        "submodule.recurse=false",
        "transfer.fsckObjects=true",
        "fetch.fsckObjects=true",
        "init.templateDir=",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    // `credential.helper=` (reset) is in the hardened base; then ours.
    if let Some(h) = helper {
        c.push(format!("credential.helper={h}"));
        if proactive {
            c.push("http.proactiveAuth=basic".into());
        }
    }
    c
}

/// The pipe that carries the token: written and closed here, its reading
/// end for git's standard input, and its inode for the helper.
fn token_pipe(token: &Secret<String>) -> Result<(std::os::fd::OwnedFd, u64), String> {
    let (r, w) = nix::unistd::pipe2(nix::fcntl::OFlag::O_CLOEXEC)
        .map_err(|e| format!("cannot make a pipe: {}", std::io::Error::from(e)))?;
    let mut line = Vec::with_capacity(token.expose().len() + 1);
    line.extend_from_slice(token.expose().as_bytes());
    line.push(b'\n');
    // At most 513 bytes: below PIPE_BUF, one write, never blocks.
    let n = nix::unistd::write(&w, &line)
        .map_err(|e| format!("cannot fill a pipe: {}", std::io::Error::from(e)))?;
    line.fill(0);
    if n != line.len() {
        return Err("cannot fill a pipe".into());
    }
    drop(w);
    let ino = nix::sys::stat::fstat(&r)
        .map_err(|e| format!("cannot read a pipe: {}", std::io::Error::from(e)))?
        .st_ino;
    Ok((r, ino))
}

#[cfg(test)]
thread_local! {
    /// Tests: behave as a git without `http.proactiveAuth`.
    static OLD_GIT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Tests: the `HOME` git gets instead of the node's empty one.
    static TEST_HOME: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn pretend_old_git(on: bool) {
    OLD_GIT.with(|o| o.set(on));
}

#[cfg(test)]
pub(crate) fn use_test_home(p: Option<PathBuf>) {
    TEST_HOME.with(|h| *h.borrow_mut() = p);
}

/// Run `git clone` into `build`, within `bounds`.
/// An empty folder the node makes for one clone, in its staging folder
/// (0700, made new so it is empty), given to git as `HOME`,
/// `XDG_CONFIG_HOME` and `CURL_HOME`; removed when dropped. With `HOME`
/// unset libcurl would find the node user's home through the password
/// database and read its `.netrc` (review of PR 163).
struct EmptyHome(PathBuf);

impl EmptyHome {
    fn new(staging: &Path) -> Result<Self, String> {
        use std::os::unix::fs::DirBuilderExt;
        let mut b = [0u8; 8];
        getrandom::fill(&mut b).expect("OS randomness");
        let p = staging.join(format!("home-{}", hex::encode(b)));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&p)
            .map_err(|e| format!("cannot make a folder in the node's staging folder: {e}"))?;
        let home = Self(p);
        if std::fs::read_dir(&home.0).map_or(true, |mut d| d.next().is_some()) {
            return Err("the clone's empty home folder is not empty".into());
        }
        Ok(home)
    }
}

impl Drop for EmptyHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// What git's error output means when a private clone's credential never
/// reached it (git asked for a user name or password, and nothing answered).
fn credential_missing(stderr: &str) -> bool {
    [
        "could not read Username",
        "could not read Password",
        "unable to get password",
        "terminal prompts disabled",
    ]
    .iter()
    .any(|m| stderr.contains(m))
}

/// The message for that case.
pub const CREDENTIAL_MISSING: &str = "git did not receive the credential the node passed it for \
     this private repository, so nothing was cloned. If this happens again, another process of \
     this node's user may be reading it: replace the token under Plugins → Connectors";

async fn run_git(build: &Build, req: &CloneRequest, bounds: &Bounds) -> Result<(), String> {
    let path = format!("{}.git", req.full_name);
    let (stdin, helper) = match (&req.username, &req.token) {
        (Some(u), Some(t)) => {
            let (fd, ino) = token_pipe(t)?;
            (
                Stdio::from(fd),
                Some(helper(SCHEME, &req.host, &path, u, ino)?),
            )
        }
        _ => (Stdio::null(), None),
    };
    let proactive = helper.is_some() && proactive_auth().await;
    // The environment is an allow-list (the hardened base clears it and
    // passes PATH, LANG, TMPDIR and its own GIT_* values; no inherited
    // GIT_*, SSL_*, CURL_* or proxy variable reaches git); the home, where
    // libcurl reads `.netrc` and git a user config, is an empty folder of
    // the node's own, never the node user's.
    let staging = build
        .path()
        .parent()
        .ok_or("the node's staging folder is missing")?;
    let home = EmptyHome::new(staging)?;
    let mut c = crate::git::hardened(&crate::git::git_program());
    #[allow(unused_mut)]
    let mut home_dir = home.0.clone();
    #[cfg(test)]
    if let Some(h) = TEST_HOME.with(|h| h.borrow().clone()) {
        home_dir = h;
    }
    c.env("HOME", &home_dir)
        .env("XDG_CONFIG_HOME", &home_dir)
        .env("CURL_HOME", &home_dir);
    for o in clone_config(SCHEME, helper.as_deref(), proactive) {
        c.arg("-c").arg(o);
    }
    c.args([
        "clone",
        "--no-recurse-submodules",
        "--template=",
        "--quiet",
        "--",
        &url(SCHEME, &req.host, &req.full_name),
        ".",
    ])
    .current_dir(build.proc_path())
    .stdin(stdin)
    .stdout(Stdio::null())
    .stderr(Stdio::piped())
    .kill_on_drop(true)
    .process_group(0);
    let mut child = c.spawn().map_err(|e| format!("cannot start git: {e}"))?;
    let mut group = crate::git::Group(child.id());
    let err_out = child.stderr.take().map(|mut s| {
        tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let mut kept = Vec::new();
            let mut buf = [0u8; 4096];
            while let Ok(n) = s.read(&mut buf).await {
                if n == 0 {
                    break;
                }
                let room = STDERR_READ.saturating_sub(kept.len());
                kept.extend_from_slice(&buf[..n.min(room)]);
            }
            kept
        })
    });
    let deadline = Instant::now() + bounds.timeout;
    let at = build.path().to_path_buf();
    let ended: Result<std::process::ExitStatus, String> = loop {
        let next = (Instant::now() + bounds.every).min(deadline);
        tokio::select! {
            st = child.wait() => break st.map_err(|e| format!("git: {e}")),
            _ = tokio::time::sleep_until(next.into()) => {
                if Instant::now() >= deadline {
                    break Err(format!(
                        "the clone took longer than this node allows ([limits] clone_timeout_s = \
                         {} s); git was stopped and nothing was kept",
                        bounds.timeout.as_secs()
                    ));
                }
                if over(&at, bounds.max_bytes).await {
                    break Err(too_big(bounds));
                }
            }
        }
    };
    let status = match ended {
        Ok(st) => {
            // Reaped: its pid must not be signalled any more.
            group.0 = None;
            st
        }
        Err(e) => {
            // The whole group (git, its remote helper, index-pack), then
            // git itself reaped.
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
        let text = String::from_utf8_lossy(&stderr);
        if req.token.is_some() && credential_missing(&text) {
            return Err(CREDENTIAL_MISSING.into());
        }
        return Err(format!("git clone failed: {}", clean_error(&text, req)));
    }
    if over(&at, bounds.max_bytes).await {
        return Err(too_big(bounds));
    }
    drop(home);
    // An empty repository gets `project.create`'s first commit (on the
    // branch the remote names as its default), so the harness can work in
    // it; refs without a commit at `HEAD` are refused.
    match crate::git::head_of(&build.proc_path()).await? {
        crate::git::Head::Commit => Ok(()),
        crate::git::Head::Empty => crate::git::first_commit(&build.proc_path())
            .await
            .map_err(|e| format!("making the empty repository's first commit failed: {e}")),
        crate::git::Head::Dangling => Err(
            "the repository's HEAD names no commit (its default branch is missing on the \
             forge); nothing was kept"
                .into(),
        ),
    }
}

fn too_big(bounds: &Bounds) -> String {
    format!(
        "the clone grew past what this node allows ([limits] clone_max_mb = {} MB); git was \
         stopped and nothing was kept",
        bounds.max_bytes / (1024 * 1024)
    )
}

/// Whether the files under `at` (not following links) hold more than `max`
/// bytes.
async fn over(at: &Path, max: u64) -> bool {
    let at = at.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let mut n = 0u64;
        for e in walkdir::WalkDir::new(&at)
            .follow_links(false)
            .into_iter()
            .flatten()
        {
            if let Ok(m) = e.metadata()
                && !m.is_dir()
            {
                n = n.saturating_add(m.len());
                if n > max {
                    return true;
                }
            }
        }
        false
    })
    .await
    .unwrap_or(true)
}

/// git's last error lines, one line, without the token (in any form a
/// server could echo: plain or as the Basic credential), control
/// characters or more than [`ERROR_MAX`] characters.
fn clean_error(stderr: &str, req: &CloneRequest) -> String {
    let mut s = stderr.to_string();
    if let (Some(u), Some(t)) = (&req.username, &req.token) {
        use base64::Engine as _;
        let basic = base64::engine::general_purpose::STANDARD.encode(format!("{u}:{}", t.expose()));
        s = s
            .replace(t.expose().as_str(), "<redacted>")
            .replace(&basic, "<redacted>");
    }
    let lines: Vec<&str> = s.lines().filter(|l| !l.trim().is_empty()).collect();
    let tail = lines[lines.len().saturating_sub(3)..].join(" / ");
    let out: String = tail
        .chars()
        .filter(|c| !c.is_control())
        .take(ERROR_MAX)
        .collect();
    if out.is_empty() {
        "git said nothing".into()
    } else {
        out
    }
}

#[cfg(test)]
#[path = "forge_clone_tests.rs"]
mod tests;
