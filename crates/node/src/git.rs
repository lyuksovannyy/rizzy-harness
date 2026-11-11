//! Turn checkpoints and sub-agent worktrees in git.
//!
//! **No git call of the node that stages, diffs, checks out or applies
//! files reads configuration or attributes a session can write** (R.1,
//! PLAN.md T19). Every such call runs in a harness-owned *shadow*
//! repository in the node's data directory (`checkpoints/<hash>.git` for
//! turn checkpoints, `checkpoints/<hash>-sub.git` for sub-agents), as
//! `--git-dir <shadow> --work-tree <folder>`, with attributes from the
//! empty tree: the user's `.git/config`, hooks, filters, attributes and
//! index are never consulted. In a repository the shadow borrows the
//! user's objects through `objects/info/alternates` (read only,
//! content-addressed) so files already committed are not stored twice, and
//! copies `.git/info/exclude` so the same files are ignored. Only the
//! latest N checkpoints are kept. A git repository nested in the folder is
//! left out of every checkpoint and never touched by an undo (see
//! `stage_without_nested`).
//!
//! **The checkpoint root is found on the file system, never from git's
//! configuration** (R.1, PLAN.md T33): the nearest folder at or above the
//! session folder that holds a `.git` entry, not above the workspace root
//! the session is in; git's own answer must agree, or the folder gets no
//! checkpoints.
//!
//! A sub-agent that can change files works in a worktree of the user's
//! repository, so its own git works there. The node makes it with
//! `worktree add --no-checkout` (no file is converted, no hook runs),
//! fills it from the shadow, snapshots its result in the shadow, writes
//! the changed files into the parent's folder with the restore machinery,
//! and removes it with file system calls. The few calls that still run in
//! the user's repository ([`user_git`]) convert no file.
//!
//! Every git process gets a cleared environment, command-line overrides for
//! the config keys that run commands, a timeout, and is killed (with its
//! process group) when the call is dropped.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::process::Command;

mod upkeep;

/// Where checkpoints for one working folder are stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    /// The folder whose files are snapshotted.
    pub root: PathBuf,
    /// The harness-owned git dir. Always set: no `Repo` is ever made for
    /// the user's own repository (an `Option` only so the tests that look
    /// inside the shadow read the same field).
    shadow: Option<PathBuf>,
    /// The empty tree of the shadow's object format (its attribute source).
    empty_tree: &'static str,
    /// Folders a restore never writes or deletes in (the node's data
    /// directory), canonical.
    protected: Vec<PathBuf>,
    /// A work tree other than the one the shadow is for (a sub-agent's
    /// worktree): snapshots start from an empty index and do not keep it,
    /// so the shadow's stat cache stays the folder's.
    scratch: bool,
}

/// Build output and dependency folders a shadow repository of a plain
/// folder never snapshots (a repository's own ignore rules apply instead).
const SHADOW_EXCLUDES: &str =
    "node_modules/\ntarget/\n.venv/\nvenv/\n__pycache__/\ndist/\nbuild/\n.cache/\n*.pyc\n";

/// A snapshot that takes longer than this is abandoned (huge folders).
const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(60);

/// The longest any single git process may run before it is killed.
pub const GIT_TIMEOUT: Duration = Duration::from_secs(120);

/// Environment variables passed through to git; everything else, every
/// inherited `GIT_*` variable included, is dropped.
const KEEP_ENV: &[&str] = &["PATH", "HOME", "LANG", "TMPDIR"];

/// Command-line config for every call. Command-line config beats every
/// config file, so these win over the repository's `.git/config`.
const OVERRIDES: &[&str] = &[
    // Commands git would run on its own while reading the index or writing it.
    "core.fsmonitor=false",
    "core.hooksPath=/dev/null",
    // Interactive helpers; none should ever start.
    "core.pager=cat",
    "core.editor=false",
    "sequence.editor=false",
    "core.askPass=",
    "credential.helper=",
    // `commit-tree` honours commit.gpgSign and would run gpg.program.
    "commit.gpgSign=false",
    // No transport at all: no ssh (core.sshCommand), no proxy
    // (core.gitProxy), no ext::, no lazy fetch of a partial clone.
    "protocol.allow=never",
    "protocol.ext.allow=never",
    "core.sshCommand=false",
    // Checkout must not descend into submodules (their own configs).
    "submodule.recurse=false",
    // No background gc/maintenance that would outlive the call.
    "gc.auto=0",
    "maintenance.auto=false",
];

/// The empty tree, per object format. Used as the attribute source in a
/// shadow repository, so the work tree's `.gitattributes` (filters, eol,
/// ident, encodings) do not apply and a snapshot is the bytes on disk.
fn empty_tree(format: &str) -> &'static str {
    if format == "sha256" {
        "6ef19b41225c5369f1c104d45d8d85efa9b057b53b14b4b9b939dd74decc5321"
    } else {
        "4b825dc642cb6eb9a060e54bf8d69288fbee4904"
    }
}

impl Repo {
    pub fn is_shadow(&self) -> bool {
        self.shadow.is_some()
    }

    /// The same shadow with another work tree (a sub-agent's worktree).
    fn with_root(&self, root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            ..self.clone()
        }
    }

    /// This, as a scratch work tree (see [`Repo::scratch`]).
    fn scratch(mut self) -> Self {
        self.scratch = true;
        self
    }
}

/// A git command with the hardened base: cleared environment, overrides,
/// no stdin. `program` is `git` except in tests.
fn hardened(program: &Path) -> Command {
    let mut c = Command::new(program);
    c.env_clear();
    for k in KEEP_ENV {
        if let Some(v) = std::env::var_os(k) {
            c.env(k, v);
        }
    }
    c.env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "")
        .env("SSH_ASKPASS", "")
        .env("GIT_PAGER", "cat")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_AUTHOR_NAME", "harness")
        .env("GIT_AUTHOR_EMAIL", "harness@localhost")
        .env("GIT_COMMITTER_NAME", "harness")
        .env("GIT_COMMITTER_EMAIL", "harness@localhost");
    for o in OVERRIDES {
        c.arg("-c").arg(o);
    }
    c
}

/// Kills a git process group when dropped (timeout, cancelled turn), so
/// a filter or hook it started does not outlive it either.
struct Group(Option<u32>);

impl Drop for Group {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.0.take().and_then(|p| i32::try_from(p).ok()) {
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(pid),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
    }
}

/// Run `cmd` to completion, feeding `input` on stdin, within `timeout`.
async fn exec(
    mut cmd: Command,
    input: Option<Vec<u8>>,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    use std::process::Stdio;
    cmd.stdin(if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true);
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = cmd.spawn().map_err(|e| format!("git: {e}"))?;
    let mut group = Group(child.id());
    let stdin = child.stdin.take();
    let work = async move {
        let feed = async move {
            if let (Some(mut s), Some(data)) = (stdin, input) {
                use tokio::io::AsyncWriteExt;
                // A child that exits early closes the pipe; its status says why.
                let _ = s.write_all(&data).await;
            }
        };
        let (_, out) = tokio::join!(feed, child.wait_with_output());
        out.map_err(|e| format!("git: {e}"))
    };
    let out = tokio::time::timeout(timeout, work)
        .await
        .map_err(|_| format!("git did not finish within {} s", timeout.as_secs_f32()))?;
    // The wait finished (and reaped git): nothing left to kill, and its
    // pid must not be signalled any more.
    group.0 = None;
    out
}

fn result(args: &[&str], out: std::process::Output) -> Result<String, String> {
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(result_text(args, out))
    }
}

/// The error message for a failed git call.
fn result_text(args: &[&str], out: std::process::Output) -> String {
    format!(
        "git {}: {}",
        args.iter()
            .find(|a| !a.starts_with('-') && !a.contains('='))
            .unwrap_or(&""),
        String::from_utf8_lossy(&out.stderr).trim()
    )
}

#[cfg(test)]
thread_local! {
    /// A stand-in `git` for the checkpoint calls of this test thread (a
    /// `#[tokio::test]` runs on one thread).
    static TEST_GIT: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// The git program for the checkpoint calls (`git`, found on `PATH`).
fn git_program() -> PathBuf {
    #[cfg(test)]
    if let Some(p) = TEST_GIT.with(|g| g.borrow().clone()) {
        return p;
    }
    PathBuf::from("git")
}

fn command(repo: &Repo, index: Option<&Path>) -> Command {
    let mut c = hardened(&git_program());
    match &repo.shadow {
        Some(dir) => {
            c.arg("--git-dir")
                .arg(dir)
                .arg("--work-tree")
                .arg(&repo.root);
            // No attributes from the work tree or the user's global file:
            // filters stay off even on a git without GIT_ATTR_SOURCE
            // (they have no definition in the config in force).
            c.arg("-c").arg("core.attributesFile=/dev/null");
            // The user's global ignore patterns are copied into the shadow's
            // info/exclude (see `repo_for`); git's own lookup stays off.
            c.arg("-c").arg("core.excludesFile=/dev/null");
            c.env("GIT_ATTR_SOURCE", repo.empty_tree);
            // Paths in and out are relative to the folder's top, and `.`
            // is all of it, wherever the node process itself runs.
            c.current_dir(&repo.root);
        }
        // Never made (see `Repo::shadow`); a git dir that does not exist
        // fails the call rather than fall back to the user's repository.
        None => {
            c.arg("--git-dir").arg("/nonexistent/harness-no-shadow");
            c.current_dir(&repo.root);
        }
    }
    if let Some(i) = index {
        c.env("GIT_INDEX_FILE", i);
    }
    c
}

async fn run(repo: &Repo, args: &[&str], index: Option<&Path>) -> Result<String, String> {
    let mut c = command(repo, index);
    c.args(args);
    result(args, exec(c, None, GIT_TIMEOUT).await?)
}

/// `run`, with bytes on stdin and stdout exactly as git wrote it (paths
/// from `-z` output may be any bytes).
async fn run_raw(
    repo: &Repo,
    args: &[&str],
    index: Option<&Path>,
    input: Option<Vec<u8>>,
) -> Result<Vec<u8>, String> {
    let mut c = command(repo, index);
    c.args(args);
    let out = exec(c, input, GIT_TIMEOUT).await?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(result_text(args, out))
    }
}

/// A git command in the user's repository at `top` (the folder that holds
/// its `.git`), with the hardened base. Only for calls that convert no file
/// (no stage, diff, checkout or apply), so what a session wrote in the
/// repository's config or attributes cannot make them run anything: the
/// overrides switch off what would otherwise run (fsmonitor, hooks, pager,
/// editors, transports, signing), filters and diff drivers only run when
/// file contents are converted, and an alias only when it is named. Git's
/// own search for the repository stops at `top`.
///
/// Callers, each tested against hostile config
/// (`hostile_config_in_the_main_repository_never_runs_in_the_nodes_git`):
/// `rev-parse` (the repository's paths and format, its `HEAD`), and
/// `worktree add --no-checkout` / `--orphan` (a sub-agent's worktree).
fn user_git(top: &Path) -> Command {
    let mut c = hardened(&git_program());
    c.arg("-C").arg(top);
    if let Some(parent) = top.parent() {
        c.env("GIT_CEILING_DIRECTORIES", parent);
    }
    c
}

async fn user_run(top: &Path, args: &[&str]) -> Result<String, String> {
    let mut c = user_git(top);
    c.args(args);
    result(args, exec(c, None, GIT_TIMEOUT).await?)
}

/// The commit, branch and whether tracked files changed, for the folder a
/// published file came from (display only). `rev-parse` in the user's
/// repository; whether anything changed is asked of the shadow (see
/// [`Repo::changed_since`]), never of `git status` there, which would run
/// the repository's clean filters.
pub async fn describe(
    dir: &Path,
    shadow_base: Option<&Path>,
    bounds: &Bounds,
) -> Option<(String, String, bool)> {
    let dir = dir.canonicalize().ok()?;
    let (top, _) = find_repo(&dir, bounds).await.ok()??;
    let commit = user_run(&top, &["rev-parse", "--verify", "-q", "HEAD^{commit}"])
        .await
        .ok()?;
    let branch = user_run(&top, &["rev-parse", "--abbrev-ref", "HEAD"])
        .await
        .unwrap_or_default();
    let dirty = match checkpoint_repo(&dir, shadow_base, bounds).await {
        Ok(Some(repo)) => repo.changed_since(&commit).await,
        _ => false,
    };
    Some((commit, branch, dirty))
}

impl Repo {
    /// Whether tracked files in the folder differ from `commit` (what
    /// `git status --untracked-files=no` would call dirty), asked of the
    /// shadow with a private copy of the user's index (its stat cache, so
    /// unchanged files are not read). Raw bytes are compared, so a file the
    /// repository filters (LFS, eol conversion) may count as changed.
    async fn changed_since(&self, commit: &str) -> bool {
        let Some(dir) = &self.shadow else {
            return false;
        };
        let Ok(user_index) = user_run(
            &self.root,
            &["rev-parse", "--path-format=absolute", "--git-path", "index"],
        )
        .await
        else {
            return false;
        };
        let tmp = dir.join(format!("harness-index-{}", harness_core::new_id()));
        let _files = IndexFiles(tmp.clone());
        if tokio::fs::copy(&user_index, &tmp).await.is_err() {
            return false;
        }
        let mut c = command(self, Some(&tmp));
        c.args([
            "diff-index",
            "--quiet",
            "--no-ext-diff",
            "--no-textconv",
            commit,
        ]);
        matches!(exec(c, None, GIT_TIMEOUT).await, Ok(o) if o.status.code() == Some(1))
    }
}

/// What may bound a checkpoint root or a sub-agent's repository: the
/// workspace root the session folder is in (no walk goes above it) and the
/// folders no checkpoint may hold (the node's data directory). Canonical.
#[derive(Debug, Clone, Default)]
pub struct Bounds {
    /// The innermost workspace root holding the folder; `None` only in
    /// tests (the walk then goes up to `/`).
    pub ceiling: Option<PathBuf>,
    pub protected: Vec<PathBuf>,
}

impl Bounds {
    /// Whether `dir` (canonical) is a protected folder, inside one or holds
    /// one.
    fn exposes(&self, dir: &Path) -> Option<&PathBuf> {
        self.protected
            .iter()
            .find(|p| dir.starts_with(p) || p.starts_with(dir))
    }
}

/// What one `rev-parse` says about the repository at a folder.
struct RepoInfo {
    common_dir: PathBuf,
    objects: PathBuf,
    exclude: PathBuf,
    format: String,
}

/// Whether `dir` holds a `.git` entry git might accept (a directory, a
/// file naming one, or a link): one `lstat`.
fn holds_git(dir: &Path) -> bool {
    std::fs::symlink_metadata(dir.join(".git")).is_ok()
}

/// The repository containing `cwd` (canonical), found on the file system:
/// the nearest folder at or above it that holds a `.git` entry, never
/// above `bounds.ceiling`. A `.git` git does not accept there (rev-parse
/// fails with the search stopped at that folder) is no repository, and the
/// walk goes on upwards. Git is then asked for the repository's paths, and
/// its top level must be that folder: a `core.worktree` (or anything else)
/// that moves it refuses the folder, with the reason (`Err`). `Ok(None)`:
/// no repository within the bounds.
async fn find_repo(cwd: &Path, bounds: &Bounds) -> Result<Option<(PathBuf, RepoInfo)>, String> {
    let mut dir = Some(cwd);
    while let Some(d) = dir {
        if let Some(c) = &bounds.ceiling
            && !d.starts_with(c)
        {
            break;
        }
        if holds_git(d) {
            match repo_info(d).await {
                Ok(Some(info)) => return Ok(Some((d.to_path_buf(), info))),
                Ok(None) => {}
                Err(e) => return Err(e),
            }
        }
        dir = d.parent();
    }
    Ok(None)
}

/// What git says about the repository whose `.git` is in `top`. `Ok(None)`
/// when git does not take it as a repository; `Err` when its top level is
/// not `top`.
async fn repo_info(top: &Path) -> Result<Option<RepoInfo>, String> {
    let Ok(out) = user_run(
        top,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--show-toplevel",
            "--git-common-dir",
            "--git-path",
            "objects",
            "--git-path",
            "info/exclude",
            "--show-object-format",
        ],
    )
    .await
    else {
        return Ok(None);
    };
    let l: Vec<&str> = out.lines().collect();
    let [shown, common, objects, exclude, format] = l[..] else {
        return Ok(None);
    };
    let shown = PathBuf::from(shown);
    let shown = shown.canonicalize().unwrap_or(shown);
    if shown != top {
        return Err(format!(
            "this repository's git configuration puts its top folder at {} instead of {}, \
             where its .git is (core.worktree?); the node takes no checkpoints in it and \
             makes no sub-agent worktree",
            shown.display(),
            top.display()
        ));
    }
    Ok(Some(RepoInfo {
        common_dir: PathBuf::from(common),
        objects: PathBuf::from(objects),
        exclude: PathBuf::from(exclude),
        format: format.to_string(),
    }))
}

/// Write `text` to `path` unless it already holds exactly that (other
/// sessions may be reading it).
async fn write_if_changed(path: &Path, text: &[u8]) {
    if tokio::fs::read(path).await.ok().as_deref() != Some(text) {
        let _ = tokio::fs::write(path, text).await;
    }
}

/// Where to checkpoint `cwd`: a shadow repository under `shadow_base`
/// (created on first use) whose work tree is the repository containing
/// `cwd` (see [`find_repo`]), or `cwd` itself when it is in none within the
/// bounds, or when that repository's folder holds a protected one (the
/// node's data directory in a repository at `$HOME`): then only the session
/// folder is checkpointed, as a plain folder. `Ok(None)` when no shadow
/// base is configured, and for a sub-agent's temporary worktree (it is
/// removed when the sub-agent ends, so its checkpoints could never be
/// used). `Err`: refused, with the reason for the chat.
pub async fn checkpoint_repo(
    cwd: &Path,
    shadow_base: Option<&Path>,
    bounds: &Bounds,
) -> Result<Option<Repo>, String> {
    repo_for_with(
        cwd,
        shadow_base,
        bounds,
        &UserGitEnv::from_process(),
        Shadow::Checkpoints,
    )
    .await
    .map(|r| r.map(|(repo, _)| repo))
}

/// [`checkpoint_repo`] without bounds (tests).
#[cfg(test)]
pub async fn repo_for(cwd: &Path, shadow_base: Option<&Path>) -> Option<Repo> {
    checkpoint_repo(cwd, shadow_base, &Bounds::default())
        .await
        .ok()
        .flatten()
}

/// The repository's top folder containing `cwd`, within `bounds` (tests).
#[cfg(test)]
pub async fn repo_root(cwd: &Path) -> Option<PathBuf> {
    let cwd = cwd.canonicalize().ok()?;
    find_repo(&cwd, &Bounds::default())
        .await
        .ok()
        .flatten()
        .map(|(top, _)| top)
}

/// Which shadow of a folder: turn checkpoints, or sub-agents' snapshots
/// (their own, so a delegation never pushes a turn's checkpoint out).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shadow {
    Checkpoints,
    SubAgents,
}

/// Where the user's own git settings live: only used to find their global
/// ignore file (tests point these at temporary files).
#[derive(Debug, Clone, Default)]
struct UserGitEnv {
    home: Option<PathBuf>,
    xdg_config_home: Option<PathBuf>,
    global_config: Option<PathBuf>,
}

impl UserGitEnv {
    fn from_process() -> Self {
        let var = |k| {
            std::env::var_os(k)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        Self {
            home: var("HOME"),
            xdg_config_home: var("XDG_CONFIG_HOME"),
            global_config: var("GIT_CONFIG_GLOBAL"),
        }
    }
}

/// An ignore file's contents, treated as data: only a regular file of at
/// most 1 MiB is read; anything else counts as empty.
async fn read_ignore_file(path: &Path) -> Vec<u8> {
    let small = tokio::fs::metadata(path)
        .await
        .is_ok_and(|m| m.is_file() && m.len() <= 1 << 20);
    if small {
        tokio::fs::read(path).await.unwrap_or_default()
    } else {
        Vec::new()
    }
}

/// Where the user's global ignore file is, as git finds it:
/// `core.excludesFile` from the user's global config, else
/// `$XDG_CONFIG_HOME/git/ignore`, else `~/.config/git/ignore`. This is the
/// only call that sees the global config, and `git config --get` runs
/// nothing; it runs in `dir` so `includeIf "gitdir:…"` resolves as it
/// would for the user.
async fn global_excludes_path(dir: &Path, env: &UserGitEnv) -> Option<PathBuf> {
    let mut c = hardened(Path::new("git"));
    c.env_remove("GIT_CONFIG_GLOBAL").env_remove("HOME");
    if let Some(g) = &env.global_config {
        c.env("GIT_CONFIG_GLOBAL", g);
    }
    if let Some(h) = &env.home {
        c.env("HOME", h);
    }
    if let Some(x) = &env.xdg_config_home {
        c.env("XDG_CONFIG_HOME", x);
    }
    c.arg("-C").arg(dir).args([
        "config",
        "--global",
        "--includes",
        "--type=path",
        "--get",
        "core.excludesFile",
    ]);
    if let Ok(out) = exec(c, None, GIT_TIMEOUT).await
        && out.status.success()
    {
        let v = String::from_utf8_lossy(&out.stdout)
            .trim_end_matches('\n')
            .to_string();
        if !v.is_empty() {
            // A relative path is relative to the work tree, as in git.
            return Some(dir.join(v));
        }
    }
    match (&env.xdg_config_home, &env.home) {
        (Some(x), _) => Some(x.join("git/ignore")),
        (None, Some(h)) => Some(h.join(".config/git/ignore")),
        (None, None) => None,
    }
}

/// The patterns of the user's global ignore file, ready to append to the
/// shadow's `info/exclude` (starts on a new line).
async fn global_excludes(dir: &Path, env: &UserGitEnv) -> Vec<u8> {
    let Some(path) = global_excludes_path(dir, env).await else {
        return Vec::new();
    };
    let text = read_ignore_file(&path).await;
    if text.is_empty() {
        return text;
    }
    let mut v = b"\n# global ignore file\n".to_vec();
    v.extend(text);
    v.push(b'\n');
    v
}

/// Whether `cwd` is inside a sub-agent worktree (`…/.git/harness-worktrees/
/// <id>/…`), judged from the path alone.
fn in_sub_agent_worktree(cwd: &Path) -> bool {
    harness_tools::sandbox::worktree_git(cwd).is_some()
}

async fn repo_for_with(
    cwd: &Path,
    shadow_base: Option<&Path>,
    bounds: &Bounds,
    user: &UserGitEnv,
    kind: Shadow,
) -> Result<Option<(Repo, Option<RepoInfo>)>, String> {
    // Absolute: git runs in the folder's top, so a relative data directory
    // (`--data-dir data`) would resolve against the user's folder.
    let Some(base) = shadow_base else {
        return Ok(None);
    };
    let base = std::path::absolute(base).map_err(|e| e.to_string())?;
    let base = base.as_path();
    let cwd = cwd
        .canonicalize()
        .map_err(|e| format!("{}: {e}", cwd.display()))?;
    // A sub-agent's worktree is removed when it ends: no turn checkpoints
    // there (its own sub-agents still get a shadow of it).
    if kind == Shadow::Checkpoints && in_sub_agent_worktree(&cwd) {
        return Ok(None);
    }
    if let Some(p) = bounds.exposes(&cwd) {
        return Err(format!(
            "{} holds or is inside the harness's own data directory {}",
            cwd.display(),
            p.display()
        ));
    }
    let (root, info) = match find_repo(&cwd, bounds).await? {
        // A repository whose folder holds the data directory (one at
        // `$HOME`): only the session folder is checkpointed, as a plain one.
        Some((top, _)) if bounds.exposes(&top).is_some() => (cwd, None),
        Some((top, info)) => (top, Some(info)),
        None => (cwd, None),
    };
    // Sub-agents need a repository; no shadow is made for a plain folder.
    if kind == Shadow::SubAgents && info.is_none() {
        return Ok(None);
    }
    let key = {
        use sha2::Digest;
        hex::encode(sha2::Sha256::digest(root.to_string_lossy().as_bytes()))[..16].to_string()
    };
    let dir = match kind {
        Shadow::Checkpoints => base.join(format!("{key}.git")),
        Shadow::SubAgents => base.join(format!("{key}-sub.git")),
    };
    if !dir.join("HEAD").exists() {
        let format = info.as_ref().map_or("sha1", |i| i.format.as_str());
        let mut c = hardened(Path::new("git"));
        c.args(["init", "--bare", "-q", "--object-format", format])
            .arg(&dir);
        let out = exec(c, None, GIT_TIMEOUT).await?;
        if !out.status.success() {
            return Err(result_text(&["init"], out));
        }
        let _ = tokio::fs::write(dir.join("harness-format"), format).await;
        let _ = tokio::fs::write(
            dir.join("harness-folder"),
            root.to_string_lossy().as_bytes(),
        )
        .await;
    }
    let _ = tokio::fs::create_dir_all(dir.join("info")).await;
    let _ = tokio::fs::create_dir_all(dir.join("objects/info")).await;
    let shadow_format = tokio::fs::read_to_string(dir.join("harness-format"))
        .await
        .unwrap_or_else(|_| "sha1".into());
    let alternates = dir.join("objects/info/alternates");
    match &info {
        Some(i) => {
            // The user's objects are only read (content-addressed data); new
            // objects are always written into the shadow.
            if i.format == shadow_format.trim() {
                let mut line = i.objects.to_string_lossy().into_owned().into_bytes();
                line.push(b'\n');
                write_if_changed(&alternates, &line).await;
            } else {
                let _ = tokio::fs::remove_file(&alternates).await;
            }
            // Ignore patterns are data; copy them so the same files are skipped.
            let mut exclude = read_ignore_file(&i.exclude).await;
            exclude.extend(global_excludes(&root, user).await);
            write_if_changed(&dir.join("info/exclude"), &exclude).await;
        }
        None => {
            let _ = tokio::fs::remove_file(&alternates).await;
            let mut exclude = SHADOW_EXCLUDES.as_bytes().to_vec();
            exclude.extend(global_excludes(&root, user).await);
            write_if_changed(&dir.join("info/exclude"), &exclude).await;
        }
    }
    Ok(Some((
        Repo {
            root,
            shadow: Some(dir),
            empty_tree: empty_tree(shadow_format.trim()),
            protected: bounds.protected.clone(),
            scratch: false,
        },
        info,
    )))
}

/// The repository's index and a private one next to it. With `seed`, the
/// private one starts as a copy of the real one (keeps its stat cache, so
/// staging only rehashes what changed), else empty (absent). In a shadow
/// repository the index is the shadow's, never the user's.
async fn seeded_index(repo: &Repo, seed: bool) -> Result<(PathBuf, PathBuf), String> {
    let Some(dir) = &repo.shadow else {
        return Err("no checkpoint repository".into());
    };
    let real = dir.join("index");
    let tmp = real.with_file_name(format!("harness-index-{}", harness_core::new_id()));
    if seed && !repo.scratch && real.exists() {
        tokio::fs::copy(&real, &tmp)
            .await
            .map_err(|e| e.to_string())?;
        // Keep the original mtime: git's "racy clean" check compares file
        // times with the index file's own mtime. A fresh mtime lets a
        // same-size edit made in the same second look unchanged.
        let modified = tokio::fs::metadata(&real)
            .await
            .and_then(|m| m.modified())
            .map_err(|e| e.to_string())?;
        let t = tmp.clone();
        tokio::task::spawn_blocking(move || {
            std::fs::OpenOptions::new()
                .write(true)
                .open(&t)
                .and_then(|f| f.set_modified(modified))
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    }
    Ok((real, tmp))
}

/// A private, empty index (the file does not exist yet).
async fn fresh_index(repo: &Repo) -> Result<PathBuf, String> {
    seeded_index(repo, false).await.map(|(_, tmp)| tmp)
}

/// Removes a private index and the lock git takes on it (`<index>.lock`)
/// when dropped: on every path, including a call dropped at a timeout,
/// whose git was killed holding the lock (A.18). Both names are this
/// call's own (a fresh id each time); after the index was renamed into
/// place, the removal finds nothing.
struct IndexFiles(PathBuf);

impl Drop for IndexFiles {
    fn drop(&mut self) {
        let mut lock = self.0.clone().into_os_string();
        lock.push(".lock");
        let _ = std::fs::remove_file(&self.0);
        let _ = std::fs::remove_file(lock);
    }
}

/// A tree of the folder, and the nested repositories left out of it.
struct WorkTree {
    id: String,
    nested: Vec<Vec<u8>>,
}

/// Tree of the working directory as it is now (tracked + untracked, minus
/// ignored, minus nested repositories; see [`stage_without_nested`]). Used
/// for turn checkpoints (shadow repository) and for a sub-agent's base and
/// result (the user's own repository) alike.
///
/// Repair (A.16): the shadow's index caches the blob id of every file whose
/// stat data did not change, and an id it borrowed from the user's
/// repository can be pruned there; `write-tree` then refuses, and would for
/// every later snapshot. So when `write-tree` fails in a shadow repository,
/// the index is rebuilt once from the folder as it is (an empty index,
/// staged by the same [`stage_without_nested`], so every file is hashed
/// again and what is missing is written into the shadow) and the tree is
/// written again. A second failure is returned with the first one's text.
/// Which step failed decides, never git's message, so a `write-tree` that
/// failed for another reason (a full disk) also costs one rehash of the
/// folder; checkpoints and their refs are not touched.
async fn worktree_tree(repo: &Repo) -> Result<WorkTree, String> {
    match worktree_tree_from(repo, true).await {
        Err(TreeError::WriteTree(first)) if repo.is_shadow() => {
            tracing::warn!(error = %first, "checkpoint index rebuilt from the folder");
            worktree_tree_from(repo, false).await.map_err(|again| {
                format!(
                    "{}; before that, with the kept index: {first}",
                    again.text()
                )
            })
        }
        r => r.map_err(TreeError::text),
    }
}

/// Which step of [`worktree_tree_from`] failed.
enum TreeError {
    Stage(String),
    WriteTree(String),
}

impl TreeError {
    fn text(self) -> String {
        match self {
            Self::Stage(e) | Self::WriteTree(e) => e,
        }
    }
}

/// [`worktree_tree`] with the private index seeded from the repository's
/// (`seed`) or empty.
async fn worktree_tree_from(repo: &Repo, seed: bool) -> Result<WorkTree, TreeError> {
    let (real, idx) = seeded_index(repo, seed).await.map_err(TreeError::Stage)?;
    let _files = IndexFiles(idx.clone());
    let res = async {
        let nested = stage_without_nested(repo, &idx)
            .await
            .map_err(TreeError::Stage)?;
        let id = run(repo, &["write-tree"], Some(&idx))
            .await
            .map_err(TreeError::WriteTree)?;
        Ok(WorkTree { id, nested })
    }
    .await;
    // The shadow keeps the index git just wrote: the next checkpoint only
    // rehashes files whose stat data changed. (Concurrent snapshots each
    // write a complete index; the last rename wins.)
    if res.is_ok()
        && repo.is_shadow()
        && !repo.scratch
        && tokio::fs::rename(&idx, &real).await.is_ok()
    {
        return res;
    }
    let _ = tokio::fs::remove_file(&idx).await;
    res
}

/// Stage the folder into `idx` without nested repositories; returns them
/// (relative, minimal: none inside another, sorted).
///
/// A nested repository is what git itself treats as one: a directory git's
/// walk will not enter (`ls-files -o` lists it as `dir/`; git checks for a
/// valid `.git` directory, gitfile or symlink to one, and opens nothing
/// else), or a gitlink an older snapshot left in the index. It is never
/// staged, not as a gitlink and not as files, and never makes staging fail
/// (plain `add -A` fails outright on one without a commit). A `.git` entry
/// git does not accept (an empty directory, a junk file, a dangling link)
/// changes nothing: the folder is an ordinary one, and git never stages an
/// entry named `.git`, so no snapshot holds one and no restore writes one.
/// A repository made inside a folder whose files the index already tracks
/// is entered by git (the index has paths there), so those files stay
/// recorded and undoable. The folder is walked once, by git; nothing is run
/// or opened inside a nested repository.
async fn stage_without_nested(repo: &Repo, idx: &Path) -> Result<Vec<Vec<u8>>, String> {
    use std::collections::{BTreeSet, HashSet};
    let staged = run_raw(repo, &["ls-files", "-z", "--stage"], Some(idx), None).await?;
    let (tracked, gitlinks) = split_stage(&staged);
    // A gitlink an older snapshot left in the index goes first, so the
    // walk below sees that folder afresh.
    let gitlinks: Vec<Vec<u8>> = gitlinks.into_iter().map(<[u8]>::to_vec).collect();
    force_remove(repo, idx, &gitlinks).await?;
    let others = run_raw(
        repo,
        &["ls-files", "-z", "--others", "--exclude-standard"],
        Some(idx),
        None,
    )
    .await?;
    let mut untracked: Vec<&[u8]> = Vec::new();
    let mut nested: BTreeSet<Vec<u8>> = BTreeSet::new();
    for p in others.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        // Without `--directory`, git names a repository it will not enter
        // as `dir/`, and every other entry is a file or a symlink.
        match p.strip_suffix(b"/") {
            Some(dir) => {
                nested.insert(dir.to_vec());
            }
            None => untracked.push(p),
        }
    }
    let set: HashSet<&[u8]> = nested.iter().map(Vec::as_slice).collect();
    // A tracked file replaced by a repository of the same name: out of the
    // index, or `add -u` would stage a gitlink (or fail on one without a
    // commit).
    let (gone, _) = split_within(&tracked, &set);
    let gone: Vec<Vec<u8>> = gone.into_iter().map(<[u8]>::to_vec).collect();
    force_remove(repo, idx, &gone).await?;
    // Tracked files: changes and deletions, no walk. (No pathspec: with
    // none, `-u` is the whole tree, and `.` fails on an empty index.)
    run(repo, &["add", "-u"], Some(idx)).await?;
    // New files, by name.
    let (_, new) = split_within(&untracked, &set);
    let new: Vec<u8> = new
        .iter()
        .flat_map(|p| p.iter().copied().chain([0]))
        .collect();
    if !new.is_empty() {
        run_raw(
            repo,
            &["update-index", "--add", "--remove", "-z", "--stdin"],
            Some(idx),
            Some(new),
        )
        .await?;
    }
    let after = run_raw(repo, &["ls-files", "-z", "--stage"], Some(idx), None).await?;
    let (kept, late) = split_stage(&after);
    // A folder that became a repository between the walk and the add
    // would have been staged as a gitlink: take it out again.
    let late: Vec<Vec<u8>> = late.into_iter().map(<[u8]>::to_vec).collect();
    force_remove(repo, idx, &late).await?;
    nested.extend(late);
    // A tracked file replaced by a repository of the same name: git counts
    // the name as tracked, so its walk neither enters nor reports it, and
    // `add -u` drops it as if deleted. It is not deleted, it is not
    // recorded: say so (left out), so no change list calls it deleted.
    let kept: HashSet<&[u8]> = kept.into_iter().collect();
    let entered: HashSet<&[u8]> = untracked.iter().flat_map(|p| parents(p)).collect();
    let dropped: Vec<Vec<u8>> = tracked
        .iter()
        .filter(|p| !kept.contains(*p) && !entered.contains(*p))
        .map(|p| p.to_vec())
        .collect();
    let root = repo.root.clone();
    let replaced: Vec<Vec<u8>> = tokio::task::spawn_blocking(move || {
        dropped
            .into_iter()
            .filter(|p| {
                let dir = root.join(rel_path(p));
                std::fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir()) && has_git_entry(&dir)
            })
            .collect()
    })
    .await
    .map_err(|e| e.to_string())?;
    nested.extend(replaced);
    Ok(outermost(nested))
}

/// `ls-files -z --stage` output (`mode id stage TAB path`), as the paths of
/// files and the paths of gitlinks.
fn split_stage(out: &[u8]) -> (Vec<&[u8]>, Vec<&[u8]>) {
    let (mut files, mut gitlinks) = (Vec::new(), Vec::new());
    for r in out.split(|b| *b == 0) {
        let Some(tab) = r.iter().position(|b| *b == b'\t') else {
            continue;
        };
        let path = &r[tab + 1..];
        if r.starts_with(b"160000 ") {
            gitlinks.push(path);
        } else {
            files.push(path);
        }
    }
    (files, gitlinks)
}

/// `paths` split into those at or below one of `dirs` and the rest, in
/// order. Consecutive paths in one directory share one decision.
fn split_within<'a>(
    paths: &[&'a [u8]],
    dirs: &std::collections::HashSet<&[u8]>,
) -> (Vec<&'a [u8]>, Vec<&'a [u8]>) {
    if dirs.is_empty() {
        return (Vec::new(), paths.to_vec());
    }
    let (mut inside, mut rest) = (Vec::new(), Vec::new());
    let mut last: Option<(&[u8], bool)> = None;
    for p in paths {
        let hit = if dirs.contains(p) {
            true
        } else {
            let dir = &p[..p.iter().rposition(|b| *b == b'/').unwrap_or(0)];
            match last {
                Some((d, hit)) if d == dir => hit,
                _ => {
                    let hit = !dir.is_empty() && within(dir, dirs);
                    last = Some((dir, hit));
                    hit
                }
            }
        };
        if hit {
            inside.push(*p);
        } else {
            rest.push(*p);
        }
    }
    (inside, rest)
}

/// Drop `paths` from `idx` (the files stay on disk).
async fn force_remove(repo: &Repo, idx: &Path, paths: &[Vec<u8>]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    let input: Vec<u8> = paths
        .iter()
        .flat_map(|p| p.iter().copied().chain([0]))
        .collect();
    run_raw(
        repo,
        &["update-index", "--force-remove", "-z", "--stdin"],
        Some(idx),
        Some(input),
    )
    .await
    .map(|_| ())
}

/// The sorted paths of `set` that are not inside another one.
fn outermost(set: std::collections::BTreeSet<Vec<u8>>) -> Vec<Vec<u8>> {
    let all: std::collections::HashSet<&[u8]> = set.iter().map(Vec::as_slice).collect();
    set.iter()
        .filter(|p| !parents(p).any(|d| all.contains(d)))
        .cloned()
        .collect()
}

/// Snapshot the working directory into a commit stored at `ref_name`.
/// Nested repositories are left out (see [`snapshot_full`]).
pub async fn snapshot(repo: &Repo, ref_name: &str, msg: &str) -> Result<String, String> {
    snapshot_full(repo, ref_name, msg).await.map(|s| s.sha)
}

/// The most names of nested repositories put into an event or a note; the
/// rest are counted.
pub const SHOWN_MAX: usize = 50;

/// Names for an event or a note: at most [`SHOWN_MAX`], control characters
/// replaced, and how many more there were.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Names {
    pub shown: Vec<String>,
    pub more: usize,
}

impl Names {
    fn of(paths: &[Vec<u8>]) -> Self {
        Self {
            shown: paths.iter().take(SHOWN_MAX).map(|p| shown(p)).collect(),
            more: paths.len().saturating_sub(SHOWN_MAX),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.shown.is_empty()
    }

    /// "`a`, `b` and 3 more".
    pub fn listed(&self) -> String {
        let mut s = self
            .shown
            .iter()
            .map(|n| format!("`{n}`"))
            .collect::<Vec<_>>()
            .join(", ");
        if self.more > 0 {
            s.push_str(&format!(" and {} more", self.more));
        }
        s
    }
}

/// A snapshot and the nested repositories it left out.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub sha: String,
    /// Relative to the checkpoint's folder, sorted, for showing.
    pub nested: Names,
}

/// Snapshot the working directory into a commit stored at `ref_name`, and
/// say which nested repositories it left out (see [`snapshot`]).
pub async fn snapshot_full(repo: &Repo, ref_name: &str, msg: &str) -> Result<Snapshot, String> {
    snapshot_within(repo, ref_name, msg, SNAPSHOT_TIMEOUT).await
}

/// [`snapshot_full`] with its time limit. The caller holds the shadow
/// ([`hold`]): a garbage collection must not run while it writes objects no
/// ref names yet.
async fn snapshot_within(
    repo: &Repo,
    ref_name: &str,
    msg: &str,
    limit: Duration,
) -> Result<Snapshot, String> {
    tokio::time::timeout(limit, async {
        let tree = worktree_tree(repo).await?;
        let head = run(repo, &["rev-parse", "--verify", "-q", "HEAD"], None)
            .await
            .ok();
        // The left-out repositories travel with the snapshot, all of them
        // (a restore needs every one to leave them alone; this is never
        // shown), as hex: any bytes, one per line.
        let mut message = msg.to_string();
        if !tree.nested.is_empty() {
            message.push('\n');
            for n in &tree.nested {
                message.push_str(&format!("\n{NESTED_TRAILER}{}", hex::encode(n)));
            }
        }
        let mut args = vec![
            "commit-tree",
            "--no-gpg-sign",
            tree.id.as_str(),
            "-m",
            message.as_str(),
        ];
        if let Some(h) = head.as_deref().filter(|h| !h.is_empty()) {
            args.extend(["-p", h]);
        }
        let sha = run(repo, &args, None).await?;
        run(repo, &["update-ref", ref_name, &sha], None).await?;
        record_order(repo, ref_name).await;
        Ok(Snapshot {
            sha,
            nested: Names::of(&tree.nested),
        })
    })
    .await
    .map_err(|_| "snapshot took too long (folder too large?)".to_string())?
}

pub use upkeep::{check_kept_packs, gc_in_background, hold, pin_changed, sweep_stale_locks};

/// The line prefix in a snapshot's message that names one left-out
/// repository.
const NESTED_TRAILER: &str = "harness-nested: ";

/// The nested repositories snapshot `sha` left out (none for a snapshot
/// taken before A.15).
async fn nested_in(repo: &Repo, sha: &str) -> Vec<Vec<u8>> {
    run(repo, &["cat-file", "commit", sha], None)
        .await
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.strip_prefix(NESTED_TRAILER))
        .filter_map(|h| hex::decode(h).ok())
        .collect()
}

/// A relative path from git's `-z` output, for showing: lossy UTF-8, and
/// control characters (a newline, an escape sequence) replaced.
fn shown(p: &[u8]) -> String {
    String::from_utf8_lossy(p)
        .chars()
        .map(|c| if c.is_control() { '\u{FFFD}' } else { c })
        .collect()
}

/// A relative path from git's `-z` output, as a path (any bytes on Unix).
#[cfg(unix)]
fn rel_path(p: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(p))
}

#[cfg(not(unix))]
fn rel_path(p: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(p).into_owned())
}

/// Whether `dir` holds a `.git` entry of any kind (one `lstat`, nothing
/// followed or read). Used to tell the person, and to name a tracked file
/// replaced by a repository (which git drops without a word); never to
/// leave out files a snapshot would otherwise hold.
fn has_git_entry(dir: &Path) -> bool {
    std::fs::symlink_metadata(dir.join(".git")).is_ok()
}

/// The directories above `p` (relative, `a/b/c` gives `a` and `a/b`).
fn parents(p: &[u8]) -> impl Iterator<Item = &[u8]> {
    p.iter()
        .enumerate()
        .filter(|(_, b)| **b == b'/')
        .map(move |(i, _)| &p[..i])
}

/// Whether `p` is one of `dirs` or below one.
fn within(p: &[u8], dirs: &std::collections::HashSet<&[u8]>) -> bool {
    dirs.contains(p) || parents(p).any(|d| dirs.contains(d))
}

/// What one restore leaves alone and writes.
#[derive(Debug, Clone, Default)]
pub struct RestorePlan {
    /// The paths it writes, with git's status letter.
    pub touched: Vec<(char, String)>,
    /// Nested repositories it leaves alone: those a snapshot (before, after
    /// or the folder now) left out, and gitlinks of an older snapshot.
    pub nested: Names,
    /// Nested repositories that appeared or went away between `from` and
    /// `expected` (during the undone turn), all left alone.
    pub changed_repos: Names,
    /// Folders it writes into that hold a `.git` no snapshot left out (a
    /// repository made over files the turn's checkpoint holds, or a `.git`
    /// git does not accept): their files are restored, the `.git` stays.
    pub git_inside: Names,
    /// Files a forced restore deletes that no checkpoint holds (ignored
    /// files, say): they are inside a directory the restore replaces by a
    /// file. An unforced restore is refused instead.
    pub unsaved: Names,
    /// What [`restore_paths`] writes: `from`'s mode, blob and path for each
    /// path it puts back, and the paths `from` does not have (deleted).
    writes: Vec<(String, String, Vec<u8>)>,
    deletes: Vec<Vec<u8>>,
}

/// What a test restore did.
#[cfg(test)]
#[derive(Debug)]
pub struct Restored {
    pub files: Vec<String>,
    pub nested: Vec<String>,
    pub plan: RestorePlan,
}

/// [`restore`], also saying what it left alone.
#[cfg(test)]
pub async fn restore_report(
    repo: &Repo,
    from: &str,
    expected: &str,
    force: bool,
) -> Result<Restored, String> {
    let diff = restore_diff(repo, from, expected).await?;
    let now = snapshot(repo, "refs/harness/test/now/before", "now").await?;
    let plan = restore_plan(repo, &diff, &now, !force).await?;
    secure_blobs(repo, &plan).await?;
    let files = restore_paths(repo, &plan).await?;
    Ok(Restored {
        files,
        nested: plan.nested.shown.clone(),
        plan,
    })
}

/// One path of a tree diff.
#[derive(Debug, Clone)]
struct DiffRow {
    /// Git's status letter (`A`: only in `b`).
    status: char,
    path: Vec<u8>,
    /// Either side is a gitlink (a nested repository an older snapshot
    /// recorded).
    gitlink: bool,
    /// The `a` side's mode and object id (all zeros for `A`).
    old_mode: String,
    old_id: String,
    /// The `b` side's mode and object id (all zeros for `D`).
    new_mode: String,
    new_id: String,
}

/// The paths that differ between two snapshots.
async fn diff_raw(repo: &Repo, a: &str, b: &str) -> Result<Vec<DiffRow>, String> {
    let out = run_raw(
        repo,
        &[
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--raw",
            "--no-renames",
            "--no-abbrev",
            "-z",
            a,
            b,
        ],
        None,
        None,
    )
    .await?;
    Ok(parse_raw(&out))
}

/// `--raw -z` output as rows.
fn parse_raw(out: &[u8]) -> Vec<DiffRow> {
    // `:old_mode new_mode old_id new_id status NUL path NUL`
    let parts: Vec<&[u8]> = out.split(|b| *b == 0).filter(|s| !s.is_empty()).collect();
    parts
        .chunks(2)
        .filter_map(|c| match c {
            [meta, path] => {
                let meta = String::from_utf8_lossy(meta);
                let f: Vec<&str> = meta.trim_start_matches(':').split(' ').collect();
                let status = f.get(4)?.chars().next()?;
                Some(DiffRow {
                    status,
                    path: path.to_vec(),
                    gitlink: f[..2].contains(&"160000"),
                    old_mode: f[0].to_string(),
                    old_id: f.get(2)?.to_string(),
                    new_mode: f[1].to_string(),
                    new_id: f.get(3)?.to_string(),
                })
            }
            _ => None,
        })
        .collect()
}

/// Paths that differ between two snapshots, with git's status letter.
/// Nothing at or below a repository either snapshot left out is listed
/// (its files are not recorded, so no letter would be true; the checkpoint
/// names it in `excluded` instead), and a gitlink is not a file.
pub async fn changed(repo: &Repo, a: &str, b: &str) -> Result<Vec<(char, String)>, String> {
    let mut out = nested_in(repo, a).await;
    out.extend(nested_in(repo, b).await);
    let set: std::collections::HashSet<&[u8]> = out.iter().map(Vec::as_slice).collect();
    Ok(diff_raw(repo, a, b)
        .await?
        .into_iter()
        .filter(|r| !r.gitlink && !within(&r.path, &set))
        .map(|r| (r.status, String::from_utf8_lossy(&r.path).into_owned()))
        .collect())
}

/// Whether a snapshot is still kept: a checkpoint ref points at it (old ones
/// are pruned; their objects stay until the next garbage collection, which
/// runs rarely, so the object alone does not say it is kept) and it can be
/// read.
pub async fn exists(repo: &Repo, sha: &str) -> bool {
    let named = run(
        repo,
        &[
            "for-each-ref",
            "--count=1",
            "--format=%(refname)",
            "--points-at",
            sha,
            "refs/harness/",
        ],
        None,
    )
    .await
    .is_ok_and(|r| !r.is_empty());
    named
        && run(
            repo,
            &["cat-file", "-e", &format!("{sha}^{{commit}}")],
            None,
        )
        .await
        .is_ok()
}

/// Put the folder's files back to snapshot `from` for every path that
/// differs between `from` and `expected` (what the folder should look like
/// now). Refuses, listing them, if any of those files changed since
/// `expected`, unless `force`. Undo: `from` = before the turn, `expected`
/// = after it. Rewind: `expected` = the latest state. (The session runs the
/// steps itself, `restore_diff`, `secure_blobs`, a snapshot, `restore_plan`
/// and `restore_paths`, to log between them.)
#[cfg(test)]
pub async fn restore(
    repo: &Repo,
    from: &str,
    expected: &str,
    force: bool,
) -> Result<Vec<String>, String> {
    restore_report(repo, from, expected, force)
        .await
        .map(|r| r.files)
}

/// The difference a restore of `from` over `expected` works from, read once
/// (see [`restore_diff`]).
#[derive(Debug, Clone)]
pub struct RestoreDiff {
    from: String,
    expected: String,
    rows: Vec<DiffRow>,
}

impl RestoreDiff {
    /// Keep only `files` (paths as an event names them). Events carry paths
    /// as lossy UTF-8, so matching is on that form: a name that is not
    /// UTF-8 matches, and so does any other name with the same lossy form.
    pub fn only(&mut self, files: &[String]) {
        let keep: std::collections::HashSet<&str> = files.iter().map(String::as_str).collect();
        self.rows
            .retain(|r| keep.contains(String::from_utf8_lossy(&r.path).as_ref()));
    }
}

/// Why a checkpoint that needs objects that are gone cannot be restored,
/// in words for the person; `paths` are the files whose content is missing.
fn lost_content(repo: &Repo, paths: &[Vec<u8>]) -> String {
    let mut s = if borrows(repo) {
        "nothing was changed: this checkpoint depends on content that the git repository \
         in this folder no longer has (usually because its history was rewritten, by an \
         amend, a rebase, a reset or a deleted branch, and then cleaned up by `git gc`), \
         so it cannot be restored"
            .to_string()
    } else {
        "nothing was changed: this checkpoint is damaged (content it needs is missing \
         from the checkpoint store), so it cannot be restored"
            .to_string()
    };
    if !paths.is_empty() {
        s.push_str(&format!("; missing: {}", Names::of(paths).listed()));
    }
    s
}

/// Whether this checkpoint repository borrows objects from the user's.
fn borrows(repo: &Repo) -> bool {
    repo.shadow
        .as_ref()
        .is_some_and(|d| d.join("objects/info/alternates").exists())
}

/// The trees of `commits` (and every tree below them) that cannot be read.
/// Blobs are not visited (`--filter=blob:none`); used only once a diff of
/// them has failed, to say why in plain words.
async fn trees_missing(repo: &Repo, commits: &[&str]) -> bool {
    let mut trees = Vec::new();
    for c in commits {
        let Ok(text) = run(repo, &["cat-file", "commit", c], None).await else {
            return false;
        };
        match text.lines().next().and_then(|l| l.strip_prefix("tree ")) {
            Some(t) => trees.push(t.to_string()),
            None => return false,
        }
    }
    let mut args = vec![
        "rev-list",
        "--objects",
        "--missing=print",
        "--filter=blob:none",
        "--no-object-names",
    ];
    args.extend(trees.iter().map(String::as_str));
    run(repo, &args, None)
        .await
        .is_ok_and(|out| out.lines().any(|l| l.starts_with('?')))
}

/// The tree diff a restore of `from` over `expected` writes from. If it
/// cannot be read because a tree is gone (borrowed from the user's
/// repository, then pruned there), the error says so in plain words.
pub async fn restore_diff(repo: &Repo, from: &str, expected: &str) -> Result<RestoreDiff, String> {
    match diff_raw(repo, from, expected).await {
        Ok(rows) => Ok(RestoreDiff {
            from: from.to_string(),
            expected: expected.to_string(),
            rows,
        }),
        Err(e) if trees_missing(repo, &[from, expected]).await => {
            tracing::debug!(error = %e, "checkpoint tree missing");
            Err(lost_content(repo, &[]))
        }
        Err(e) => Err(e),
    }
}

/// Make every blob the plan writes safe before the folder is touched: each
/// must be readable (else the restore is refused, in plain words, naming
/// the files), and when the checkpoint repository borrows from the user's,
/// they are all copied into its own object store, so nothing done in the
/// user's repository between this check and the write can take them away.
/// Exactly the blobs the plan writes are read (not those of paths it skips,
/// such as inside a nested repository), never a whole tree.
///
/// `cat-file --batch-check` answers per object (`<id> missing`); the copy
/// is [`copy_into_own_store`]. The restore holds the shadow (see [`hold`])
/// until it ends, so no garbage collection starts before the files are
/// written; the pack then lives until the shadow's next collection, which
/// drops what the user's repository has packed (the restored state's own
/// checkpoint pins what an undo of the restore needs, see [`pin_changed`]).
pub async fn secure_blobs(repo: &Repo, plan: &RestorePlan) -> Result<(), String> {
    let blobs: Vec<(&str, &[u8])> = plan
        .writes
        .iter()
        .map(|(_, id, p)| (id.as_str(), p.as_slice()))
        .collect();
    if blobs.is_empty() {
        return Ok(());
    }
    let mut ids: Vec<&str> = blobs.iter().map(|(id, _)| *id).collect();
    ids.sort_unstable();
    ids.dedup();
    let list: Vec<u8> = ids
        .iter()
        .flat_map(|i| format!("{i}\n").into_bytes())
        .collect();
    let out = run_raw(
        repo,
        &["cat-file", "--batch-check=%(objectname) %(objecttype)"],
        None,
        Some(list.clone()),
    )
    .await?;
    // One line per id: `<id> blob`, or `<id> missing` for one that is gone.
    let text = String::from_utf8_lossy(&out);
    let present: std::collections::HashSet<&str> = text
        .lines()
        .filter_map(|l| l.strip_suffix(" blob"))
        .collect();
    if ids.iter().any(|i| !present.contains(i)) {
        let mut paths: Vec<Vec<u8>> = blobs
            .iter()
            .filter(|(id, _)| !present.contains(id))
            .map(|(_, p)| p.to_vec())
            .collect();
        paths.sort();
        return Err(lost_content(repo, &paths));
    }
    if borrows(repo) {
        copy_into_own_store(repo, list, true)
            .await
            .map_err(|e| format!("nothing was changed: {e}"))?;
    }
    Ok(())
}

/// Copy the objects `list` names (one id per line, each readable) into the
/// shadow's own store as one new pack, and return the pack's name (the
/// hash in `pack-<hash>.pack`). One `pack-objects` reads each object where
/// it is (a borrowed one through the alternates) and writes the pack into
/// the shadow's `objects/pack`; nothing is written or touched in the user's
/// repository (`hash-object -w` would not do: an object the alternate has
/// is "freshened" there and not written locally). A copy that fails leaves
/// none of its files behind. Shared by [`secure_blobs`] and the pinned
/// copies of a turn ([`pin_changed`]).
///
/// With `durable` false the pack is not flushed to disk (`core.fsync=none`):
/// git's default flushes packs, which on a slow rotational disk cost 0.2 to
/// 1.4 s for a pack of one blob (measured, A.17) against about 1 ms without.
/// A turn's copies are then as durable as the checkpoint they serve, whose
/// loose objects and refs git does not flush either; a restore's copy, which
/// protects the user's files during the write, keeps git's default.
async fn copy_into_own_store(repo: &Repo, list: Vec<u8>, durable: bool) -> Result<String, String> {
    let Some(dir) = &repo.shadow else {
        return Err("no checkpoint repository".into());
    };
    let packs = dir.join("objects/pack");
    let had = pack_dir_names(&packs).await;
    let base = packs.join("pack");
    let base = base.to_string_lossy();
    let mut args = vec!["pack-objects", "-q", "--window=0", &base];
    if !durable {
        args.splice(0..0, ["-c", "core.fsync=none"]);
    }
    let res = run_raw(repo, &args, None, Some(list)).await;
    match res {
        Ok(out) => Ok(String::from_utf8_lossy(&out).trim().to_string()),
        Err(e) => {
            remove_failed_copy(&packs, &had).await;
            Err(e)
        }
    }
}

/// The names in the shadow's pack directory.
async fn pack_dir_names(packs: &Path) -> std::collections::HashSet<std::ffi::OsString> {
    let mut names = std::collections::HashSet::new();
    if let Ok(mut rd) = tokio::fs::read_dir(packs).await {
        while let Ok(Some(e)) = rd.next_entry().await {
            names.insert(e.file_name());
        }
    }
    names
}

/// After a failed `pack-objects`: remove what it may have left, and only
/// that: names that were not there before the call and that it writes
/// (`tmp_pack_*`, `tmp_idx_*`, `tmp_rev_*`, and a `pack-*` whose `.idx` is
/// missing, so git never used it). The folder is claimed for the restore,
/// so no other git process of ours writes packs in this shadow meanwhile.
async fn remove_failed_copy(packs: &Path, had: &std::collections::HashSet<std::ffi::OsString>) {
    for name in pack_dir_names(packs).await {
        if had.contains(&name) {
            continue;
        }
        let s = name.to_string_lossy();
        let ours = s.starts_with("tmp_pack_")
            || s.starts_with("tmp_idx_")
            || s.starts_with("tmp_rev_")
            || (s.starts_with("pack-")
                && Path::new(&*s)
                    .file_stem()
                    .is_some_and(|stem| !packs.join(stem).with_extension("idx").exists()));
        if ours {
            let _ = tokio::fs::remove_file(packs.join(&name)).await;
        }
    }
}

/// The paths a restore of `diff` (`from` over `expected`) writes, with
/// their status letter, and what it leaves alone. `now` is a snapshot of
/// the folder as it is; with `check`, refuses, listing them, if any of
/// those files differ there from `expected`.
///
/// A restore skips only what a snapshot recorded as left out (`from`,
/// `expected` or `now`) and gitlinks of an older snapshot: it never writes,
/// deletes or creates anything at or below those. Everything else the
/// checkpoints hold is restored, also inside a folder that holds a `.git`
/// the snapshots did not leave out; no snapshot holds a `.git`, so a
/// restore never writes or deletes one.
pub async fn restore_plan(
    repo: &Repo,
    diff: &RestoreDiff,
    now: &str,
    check: bool,
) -> Result<RestorePlan, String> {
    use std::collections::{BTreeSet, HashSet};
    let (from, expected) = (diff.from.as_str(), diff.expected.as_str());
    let in_from: BTreeSet<Vec<u8>> = nested_in(repo, from).await.into_iter().collect();
    let in_expected: BTreeSet<Vec<u8>> = nested_in(repo, expected).await.into_iter().collect();
    let changed_repos: Vec<Vec<u8>> = in_from
        .symmetric_difference(&in_expected)
        .cloned()
        .collect();
    let mut nested: BTreeSet<Vec<u8>> = in_from.union(&in_expected).cloned().collect();
    nested.extend(nested_in(repo, now).await);
    let mut candidates: Vec<&DiffRow> = Vec::new();
    for r in &diff.rows {
        if r.gitlink {
            nested.insert(r.path.clone());
        } else {
            candidates.push(r);
        }
    }
    let nested = outermost(nested);
    let set: HashSet<&[u8]> = nested.iter().map(Vec::as_slice).collect();
    let candidates: Vec<&DiffRow> = candidates
        .into_iter()
        .filter(|r| !within(&r.path, &set))
        .collect();
    // Whatever the snapshots say: nothing outside the folder, nothing in a
    // protected one (R.1, S-3).
    let paths: Vec<Vec<u8>> = candidates.iter().map(|r| r.path.clone()).collect();
    check_inside(repo, paths).await?;
    // Folders written into that hold a `.git` (to tell the person).
    let root = repo.root.clone();
    let paths: Vec<Vec<u8>> = candidates.iter().map(|r| r.path.clone()).collect();
    let git_inside: BTreeSet<Vec<u8>> = tokio::task::spawn_blocking(move || {
        let mut seen: std::collections::HashMap<Vec<u8>, bool> = Default::default();
        let mut found = BTreeSet::new();
        for p in &paths {
            for d in parents(p) {
                let hit = *seen
                    .entry(d.to_vec())
                    .or_insert_with(|| has_git_entry(&root.join(rel_path(d))));
                if hit {
                    found.insert(d.to_vec());
                }
            }
        }
        found
    })
    .await
    .map_err(|e| e.to_string())?;
    let touched: Vec<(char, String)> = candidates
        .iter()
        .map(|r| (r.status, String::from_utf8_lossy(&r.path).into_owned()))
        .collect();
    let (deleted, written): (Vec<&DiffRow>, Vec<&DiffRow>) =
        candidates.iter().partition(|r| r.status == 'A');
    if check && !touched.is_empty() {
        let mut args = vec![
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--name-only",
            expected,
            now,
            "--",
        ];
        args.extend(touched.iter().map(|(_, p)| p.as_str()));
        let since = run(repo, &args, None).await?;
        if !since.is_empty() {
            return Err(format!(
                "these files changed after the turn; undo would overwrite that: {}",
                since.lines().collect::<Vec<_>>().join(", ")
            ));
        }
    }
    let unsaved = unsaved_in_replaced_dirs(repo, &written, now).await?;
    if !unsaved.is_empty() {
        let (mut dirs, files): (Vec<Vec<u8>>, Vec<Vec<u8>>) = unsaved.into_iter().unzip();
        dirs.dedup(); // sorted by directory first
        let in_dirs = Names::of(&dirs).listed();
        if files
            .iter()
            .any(|f| f.split(|b| *b == b'/').any(|c| c == b".git"))
        {
            return Err(format!(
                "nothing was changed: undo would replace the folder {in_dirs} by a file, and it \
                 holds a git repository, which undo never deletes; move it away first"
            ));
        }
        if check {
            return Err(format!(
                "nothing was changed: undo would replace the folder {in_dirs} by a file, and it \
                 holds files that are in no checkpoint and would be deleted: {}; \
                 \"Undo anyway\" deletes them",
                Names::of(&files).listed()
            ));
        }
        return Ok(RestorePlan {
            unsaved: Names::of(&files),
            ..plan_of(
                touched,
                &nested,
                &changed_repos,
                git_inside,
                &written,
                &deleted,
            )
        });
    }
    Ok(plan_of(
        touched,
        &nested,
        &changed_repos,
        git_inside,
        &written,
        &deleted,
    ))
}

fn plan_of(
    touched: Vec<(char, String)>,
    nested: &[Vec<u8>],
    changed_repos: &[Vec<u8>],
    git_inside: std::collections::BTreeSet<Vec<u8>>,
    written: &[&DiffRow],
    deleted: &[&DiffRow],
) -> RestorePlan {
    RestorePlan {
        touched,
        nested: Names::of(nested),
        changed_repos: Names::of(changed_repos),
        git_inside: Names::of(&outermost(git_inside)),
        unsaved: Names::default(),
        writes: written
            .iter()
            .map(|r| (r.old_mode.clone(), r.old_id.clone(), r.path.clone()))
            .collect(),
        deletes: deleted.iter().map(|r| r.path.clone()).collect(),
    }
}

/// Directories in the folder where the restore writes a file (so
/// `checkout-index -f` removes the directory and everything in it), and in
/// them the files the folder's snapshot `now` does not hold (ignored ones,
/// say), as `(directory, file)`, both relative. Only those directories are
/// walked, without following links.
async fn unsaved_in_replaced_dirs(
    repo: &Repo,
    written: &[&DiffRow],
    now: &str,
) -> Result<Vec<(Vec<u8>, Vec<u8>)>, String> {
    let root = repo.root.clone();
    let paths: Vec<Vec<u8>> = written.iter().map(|r| r.path.clone()).collect();
    let on_disk: Vec<(Vec<u8>, Vec<Vec<u8>>)> = tokio::task::spawn_blocking(move || {
        paths
            .into_iter()
            .filter(|p| std::fs::symlink_metadata(root.join(rel_path(p))).is_ok_and(|m| m.is_dir()))
            .map(|p| {
                let dir = root.join(rel_path(&p));
                let files = walkdir::WalkDir::new(&dir)
                    .follow_links(false)
                    .into_iter()
                    .filter_map(|e| e.ok())
                    .filter(|e| !e.file_type().is_dir())
                    .filter_map(|e| {
                        let rel = e.path().strip_prefix(&root).ok()?;
                        Some(path_bytes(rel))
                    })
                    .collect();
                (p, files)
            })
            .collect()
    })
    .await
    .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for (dir, files) in on_disk {
        let shown_dir = String::from_utf8_lossy(&dir).into_owned();
        let held = run_raw(
            repo,
            &["ls-tree", "-r", "-z", "--name-only", now, "--", &shown_dir],
            None,
            None,
        )
        .await?;
        let held: std::collections::HashSet<&[u8]> =
            held.split(|b| *b == 0).filter(|p| !p.is_empty()).collect();
        for f in files {
            if !held.contains(f.as_slice()) {
                out.push((dir.clone(), f));
            }
        }
    }
    out.sort();
    Ok(out)
}

/// A relative path as git's bytes.
#[cfg(unix)]
fn path_bytes(p: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    p.as_os_str().as_bytes().to_vec()
}

#[cfg(not(unix))]
fn path_bytes(p: &Path) -> Vec<u8> {
    p.to_string_lossy().replace('\\', "/").into_bytes()
}

/// Refuse a restore that would write or delete anything outside the
/// checkpoint's folder or inside a protected one, whatever the snapshots
/// say (R.1, S-3): every path must be relative with plain components only,
/// the folder must still be where it was found, and the deepest existing
/// folder on the way to each path must really be inside it (no link on the
/// way leads out) and outside every protected folder. Nothing is written
/// when one path fails. A link swapped in after this check is the residual
/// of PLAN.md T33; `checkout-index` itself does not write through a linked
/// folder inside the work tree.
async fn check_inside(repo: &Repo, paths: Vec<Vec<u8>>) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    let root = repo.root.clone();
    let protected = repo.protected.clone();
    tokio::task::spawn_blocking(move || {
        use std::path::Component;
        let real_root = root
            .canonicalize()
            .map_err(|e| format!("nothing was changed: the folder {}: {e}", root.display()))?;
        if real_root != root {
            return Err(format!(
                "nothing was changed: the folder {} now leads to {}",
                root.display(),
                real_root.display()
            ));
        }
        let refuse = |p: &[u8], why: &str| {
            Err(format!(
                "nothing was changed: the checkpoint names {}, which is {why}",
                shown(p)
            ))
        };
        let mut seen: std::collections::HashMap<PathBuf, bool> = Default::default();
        for p in &paths {
            let rel = rel_path(p);
            if rel.as_os_str().is_empty()
                || rel.components().any(|c| !matches!(c, Component::Normal(_)))
            {
                return refuse(p, "not a plain path inside the folder");
            }
            let mut dir = root.join(&rel);
            let inside = loop {
                if !dir.pop() {
                    break false;
                }
                if let Some(ok) = seen.get(&dir) {
                    break *ok;
                }
                match dir.canonicalize() {
                    Ok(real) => {
                        let ok = real.starts_with(&root)
                            && !protected.iter().any(|d| real.starts_with(d));
                        seen.insert(dir.clone(), ok);
                        break ok;
                    }
                    Err(e)
                        if e.kind() == std::io::ErrorKind::NotFound
                            || e.kind() == std::io::ErrorKind::NotADirectory => {}
                    Err(_) => break false,
                }
            };
            if !inside {
                return refuse(
                    p,
                    &format!(
                        "outside {} or in the harness's own data directory",
                        root.display()
                    ),
                );
            }
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Write the plan's files into the folder (`from`'s version of each, from
/// the blobs [`secure_blobs`] made safe) and delete the paths `from` does
/// not have.
///
/// The private index holds only the entries written, built from their
/// blob ids (`update-index --index-info`), so no tree is read here and no
/// object other than those blobs is needed. Not atomic across files: git
/// writes them one by one, so a failure partway (a read-only folder, a
/// full disk, a path that became something git cannot replace) leaves the
/// earlier ones written; the caller names the checkpoint that holds the
/// folder as it was.
///
/// Deletes go first, then the writes. A path the turn added is a file or a
/// link in the folder (the folder was just snapshotted); deleting it first
/// empties a directory that a restored file replaces (file `d` over the
/// turn's `d/f`), and a removed file `d` leaves room for a restored `d/f`.
/// A path to delete is already gone when it is not there as a file or link
/// any more: missing, its parent no longer a directory (`ENOTDIR`), or now
/// a directory (someone replaced it; not this entry, left alone). Any other
/// failure to delete is an error.
pub async fn restore_paths(repo: &Repo, plan: &RestorePlan) -> Result<Vec<String>, String> {
    let paths: Vec<String> = plan.touched.iter().map(|(_, p)| p.clone()).collect();
    // Checked again right before writing (the plan was made a moment ago).
    let all: Vec<Vec<u8>> = plan
        .deletes
        .iter()
        .cloned()
        .chain(plan.writes.iter().map(|(_, _, p)| p.clone()))
        .collect();
    check_inside(repo, all).await?;
    for p in &plan.deletes {
        let full = repo.root.join(rel_path(p));
        match tokio::fs::symlink_metadata(&full).await {
            Ok(m) if m.is_dir() => continue,
            Ok(_) => {}
            Err(e)
                if e.kind() == std::io::ErrorKind::NotFound
                    || e.kind() == std::io::ErrorKind::NotADirectory =>
            {
                continue;
            }
            Err(e) => return Err(format!("could not delete {}: {e}", shown(p))),
        }
        match tokio::fs::remove_file(&full).await {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                return Err(format!("could not delete {}: {e}", shown(p)));
            }
            _ => {}
        }
    }
    if !plan.writes.is_empty() {
        let idx = fresh_index(repo).await?;
        let _files = IndexFiles(idx.clone());
        let res = async {
            let mut info = Vec::new();
            for (mode, id, p) in &plan.writes {
                info.extend_from_slice(format!("{mode} {id}\t").as_bytes());
                info.extend_from_slice(p);
                info.push(0);
            }
            run_raw(
                repo,
                &["update-index", "-z", "--index-info"],
                Some(&idx),
                Some(info),
            )
            .await?;
            let names: Vec<u8> = plan
                .writes
                .iter()
                .flat_map(|(_, _, p)| p.iter().copied().chain([0]))
                .collect();
            run_raw(
                repo,
                &["checkout-index", "-f", "-z", "--stdin"],
                Some(&idx),
                Some(names),
            )
            .await
            .map(|_| ())
        }
        .await;
        let _ = tokio::fs::remove_file(&idx).await;
        res?;
    }
    Ok(paths)
}

/// A checkpoint group: a turn's refs (`.../before`, `/after`), a restore's
/// (the same two names: the folder before and after it) or a sub-agent's
/// (`/base`, `/result`).
fn group_of(ref_name: &str) -> &str {
    ref_name
        .rsplit_once('/')
        .map(|(g, _)| g)
        .unwrap_or(ref_name)
}

/// Checkpoints in creation order, kept in the git dir (commit dates only
/// have one-second resolution, too coarse to order turns).
async fn order_file(repo: &Repo) -> Option<PathBuf> {
    run(
        repo,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        None,
    )
    .await
    .ok()
    .map(|d| PathBuf::from(d).join("harness-checkpoints"))
}

static ORDER: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn record_order(repo: &Repo, ref_name: &str) {
    let Some(f) = order_file(repo).await else {
        return;
    };
    let _g = ORDER.lock().await;
    let mut text = tokio::fs::read_to_string(&f).await.unwrap_or_default();
    let g = group_of(ref_name);
    if !text.lines().any(|l| l == g) {
        text.push_str(g);
        text.push('\n');
        let _ = tokio::fs::write(&f, text).await;
    }
}

/// Delete one ref (best effort).
pub async fn delete_ref(repo: &Repo, ref_name: &str) {
    let _ = run(repo, &["update-ref", "-d", ref_name], None).await;
}

/// Keep only the `keep` most recent checkpoints (groups): their refs are
/// deleted (cheap, every time). In a shadow repository the pinned copies of
/// the dropped groups are released and the drop is counted; their objects
/// go at the next garbage collection, which runs rarely and after the turn
/// (see [`gc_in_background`]). In a project repository git's own gc removes
/// them later.
pub async fn prune(repo: &Repo, keep: usize) -> usize {
    let Some(f) = order_file(repo).await else {
        return 0;
    };
    let Ok(refs) = run(
        repo,
        &["for-each-ref", "--format=%(refname)", "refs/harness/"],
        None,
    )
    .await
    else {
        return 0;
    };
    let _g = ORDER.lock().await;
    let order: Vec<String> = tokio::fs::read_to_string(&f)
        .await
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect();
    let kept: Vec<String> = order.iter().rev().take(keep).rev().cloned().collect();
    let doomed: Vec<&str> = refs
        .lines()
        .filter(|r| !kept.iter().any(|k| k == group_of(r)))
        .collect();
    for r in &doomed {
        let _ = run(repo, &["update-ref", "-d", r], None).await;
    }
    let mut text = kept.join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    let _ = tokio::fs::write(&f, text).await;
    if let Some(dir) = &repo.shadow {
        let groups: std::collections::HashSet<&str> = doomed.iter().map(|r| group_of(r)).collect();
        upkeep::release_pins(dir, &kept).await;
        upkeep::count_dropped(dir, groups.len() as u64).await;
    }
    doomed.len()
}

/// Serializes applying sub-agent results to one working directory.
static APPLY: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Where a sub-agent's snapshots live: the shadow of the parent's
/// repository folder (its own, `<hash>-sub.git`, so delegations never push
/// a turn's checkpoint out), and the main repository's git directory, where
/// its worktree is made.
#[derive(Debug, Clone)]
pub struct SubRepo {
    pub repo: Repo,
    common: PathBuf,
}

/// A sub-agent's worktree as the node made it.
#[derive(Debug, Clone)]
pub struct Worktree {
    /// `<git dir>/harness-worktrees/<id>`, canonical.
    pub path: PathBuf,
    /// Its admin folder, `<git dir>/worktrees/<id>` (checked when made).
    admin: PathBuf,
    /// The worktree's folder as made: its result is snapshotted only while
    /// it is still that directory.
    pin: harness_core::folder::FolderPin,
}

/// The [`SubRepo`] for a session working in `cwd`. `Ok(None)`: `cwd` is in
/// no git repository within `bounds`.
pub async fn sub_repo(
    cwd: &Path,
    shadow_base: Option<&Path>,
    bounds: &Bounds,
) -> Result<Option<SubRepo>, String> {
    if shadow_base.is_none() {
        return Err("this node keeps no checkpoint store, which sub-agent worktrees need".into());
    }
    match repo_for_with(
        cwd,
        shadow_base,
        bounds,
        &UserGitEnv::from_process(),
        Shadow::SubAgents,
    )
    .await?
    {
        Some((repo, Some(info))) => Ok(Some(SubRepo {
            repo,
            common: info.common_dir,
        })),
        // Not a repository within the bounds (no shadow was made for it).
        Some((_, None)) | None => Ok(None),
    }
}

impl SubRepo {
    /// Snapshot the parent's folder as the sub-agent's base (uncommitted
    /// work included), at `refs/harness/<child>/base` in the shadow.
    pub async fn base(&self, child: &str) -> Result<String, String> {
        let _busy = hold(&self.repo).await;
        snapshot(
            &self.repo,
            &format!("refs/harness/{child}/base"),
            "harness: sub-agent base",
        )
        .await
    }

    /// Make the worktree for `child` and fill it with `base`.
    ///
    /// In the user's repository only `worktree add --no-checkout` runs (an
    /// unborn `HEAD`: `--orphan`, which has nothing to check out), so no
    /// file is converted and no filter, hook or fsmonitor runs whatever the
    /// config says. The files are then written from the shadow (attributes
    /// from the empty tree), and the worktree's own index is set to `HEAD`'s
    /// tree, so the child's git sees the parent's uncommitted work as its
    /// changes. A worktree that git named otherwise than expected, or whose
    /// folder was replaced meanwhile, is removed and refused.
    pub async fn make_worktree(&self, child: &str, base: &str) -> Result<Worktree, String> {
        let top = &self.repo.root;
        let dir = self.common.join("harness-worktrees");
        let path = dir.join(child);
        let admin = self.common.join("worktrees").join(child);
        let head = user_run(top, &["rev-parse", "--verify", "-q", "HEAD^{commit}"])
            .await
            .ok()
            .filter(|h| !h.is_empty());
        let p = path.to_string_lossy();
        match &head {
            Some(h) => {
                user_run(
                    top,
                    &["worktree", "add", "--detach", "--no-checkout", "-q", &p, h],
                )
                .await?
            }
            None => user_run(top, &["worktree", "add", "--orphan", "-q", &p]).await?,
        };
        let filled = async {
            let gitdir = tokio::fs::read_to_string(admin.join("gitdir"))
                .await
                .map_err(|e| format!("the worktree's git folder: {e}"))?;
            if Path::new(gitdir.trim()) != path.join(".git") {
                return Err("git did not make the worktree where it was asked to".to_string());
            }
            let pin = harness_core::folder::FolderPin::at(&path)?;
            let into = self.repo.with_root(&path).scratch();
            let idx = fresh_index(&into).await?;
            let _files = IndexFiles(idx.clone());
            run(&into, &["read-tree", base], Some(&idx)).await?;
            pin.check()?;
            run(&into, &["checkout-index", "-a", "-f"], Some(&idx)).await?;
            pin.check()?;
            if let Some(h) = &head {
                run(&into, &["read-tree", h], Some(&admin.join("index"))).await?;
            }
            Ok(pin)
        }
        .await;
        match filled {
            Ok(pin) => Ok(Worktree {
                path: path.clone(),
                admin: admin.clone(),
                pin,
            }),
            Err(e) => {
                remove_paths(&path, &admin).await;
                Err(e)
            }
        }
    }

    /// Snapshot the sub-agent's worktree as its result, at
    /// `refs/harness/<child>/result` in the shadow (the worktree's own
    /// config, attributes and index are never read).
    pub async fn result(&self, wt: &Worktree, child: &str) -> Result<String, String> {
        wt.pin.check()?;
        let _busy = hold(&self.repo).await;
        snapshot(
            &self.repo.with_root(&wt.path).scratch(),
            &format!("refs/harness/{child}/result"),
            "harness: sub-agent result",
        )
        .await
    }

    /// Write what changed from `base` to `result` into the parent's folder,
    /// with the restore machinery: the changed files from the shadow's
    /// blobs, deletions as deletions, nested repositories left alone, every
    /// path checked to be inside the folder and outside the protected ones.
    /// All or nothing for the conflict check: when the folder changed any of
    /// those files since `base` (the parent or another sub-agent), nothing
    /// is written. Returns how many files were written or deleted.
    pub async fn apply(&self, base: &str, result: &str, child: &str) -> Result<usize, String> {
        let _guard = APPLY.lock().await;
        let _busy = hold(&self.repo).await;
        let diff = restore_diff(&self.repo, result, base).await?;
        if diff.rows.iter().all(|r| r.gitlink) {
            return Ok(0);
        }
        let now = snapshot(
            &self.repo,
            &format!("refs/harness/{child}/now"),
            "harness: folder before a sub-agent's result",
        )
        .await?;
        let paths: Vec<String> = diff
            .rows
            .iter()
            .filter(|r| !r.gitlink)
            .map(|r| String::from_utf8_lossy(&r.path).into_owned())
            .collect();
        let mut args = vec![
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--name-only",
            base,
            now.as_str(),
            "--",
        ];
        args.extend(paths.iter().map(String::as_str));
        let since = run(&self.repo, &args, None).await?;
        if !since.is_empty() {
            return Err(format!(
                "the folder changed these files while the sub-agent worked: {}",
                since.lines().collect::<Vec<_>>().join(", ")
            ));
        }
        let plan = restore_plan(&self.repo, &diff, &now, true).await?;
        secure_blobs(&self.repo, &plan).await?;
        restore_paths(&self.repo, &plan).await?;
        Ok(plan.touched.len())
    }

    /// Keep the shadow's sub-agent snapshots bounded (`keep` groups) and
    /// collect it when due.
    pub async fn prune(&self, keep: usize) {
        prune(&self.repo, keep).await;
        gc_in_background(self.repo.clone());
    }
}

/// Remove a sub-agent's worktree with file system calls only (no git in
/// the user's repository): its folder and its admin folder. Neither call
/// follows a link (`remove_dir_all` removes a link itself, not its target).
pub async fn remove_worktree(wt: &Worktree) {
    remove_paths(&wt.path, &wt.admin).await;
}

async fn remove_paths(path: &Path, admin: &Path) {
    let (p, a) = (path.to_path_buf(), admin.to_path_buf());
    let _ = tokio::task::spawn_blocking(move || {
        let _ = std::fs::remove_dir_all(&p);
        let _ = std::fs::remove_dir_all(&a);
    })
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn sh(dir: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    /// A throwaway repository plus a folder outside it for markers, and a
    /// script that records it ran (`touch <marks>/<name>`, then `cat`, so it
    /// also works as a pass-through filter).
    pub(super) struct Hostile {
        _d: tempfile::TempDir,
        pub(super) repo: PathBuf,
        marks: PathBuf,
        pub(super) base: PathBuf,
    }

    impl Hostile {
        pub(super) fn new() -> Self {
            let d = tempfile::tempdir().unwrap();
            let repo = d.path().join("repo");
            let marks = d.path().join("marks");
            let base = d.path().join("checkpoints");
            std::fs::create_dir_all(&repo).unwrap();
            std::fs::create_dir_all(&marks).unwrap();
            sh(&repo, &["init", "-q", "-b", "main"]);
            std::fs::write(repo.join("a.txt"), "one\n").unwrap();
            std::fs::write(repo.join("b.txt"), "bee\n").unwrap();
            sh(&repo, &["add", "."]);
            sh(&repo, &["commit", "-qm", "init"]);
            Self {
                _d: d,
                repo,
                marks,
                base,
            }
        }

        /// An executable script that marks `name` when run.
        pub(super) fn script(&self, name: &str) -> PathBuf {
            use std::os::unix::fs::PermissionsExt;
            let p = self.marks.parent().unwrap().join(format!("{name}.sh"));
            let m = self.marks.join(name);
            std::fs::write(&p, format!("#!/bin/sh\ntouch '{}'\ncat\n", m.display())).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            p
        }

        pub(super) fn config(&self, key: &str, value: &str) {
            sh(&self.repo, &["config", "--local", key, value]);
        }

        pub(super) fn hook(&self, dir: &Path, name: &str) {
            use std::os::unix::fs::PermissionsExt;
            std::fs::create_dir_all(dir).unwrap();
            let p = dir.join(name);
            std::fs::copy(self.script(name), &p).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        pub(super) fn marked(&self) -> Vec<String> {
            let mut v: Vec<String> = std::fs::read_dir(&self.marks)
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            v.sort();
            v
        }

        /// What a turn does: pick the checkpoint repository, snapshot,
        /// change files, snapshot, list the changes, undo.
        async fn turn_and_undo(&self) -> Repo {
            let repo = repo_for(&self.repo, Some(&self.base)).await.unwrap();
            let before = snapshot(&repo, "refs/harness/s/t/before", "before")
                .await
                .unwrap();
            std::fs::write(self.repo.join("a.txt"), "two\n").unwrap();
            std::fs::write(self.repo.join("new.txt"), "new\n").unwrap();
            std::fs::remove_file(self.repo.join("b.txt")).unwrap();
            let after = snapshot(&repo, "refs/harness/s/t/after", "after")
                .await
                .unwrap();
            let ch = changed(&repo, &before, &after).await.unwrap();
            assert_eq!(ch.len(), 3, "{ch:?}");
            restore(&repo, &before, &after, false).await.unwrap();
            assert_eq!(
                std::fs::read_to_string(self.repo.join("a.txt")).unwrap(),
                "one\n"
            );
            assert_eq!(
                std::fs::read_to_string(self.repo.join("b.txt")).unwrap(),
                "bee\n"
            );
            assert!(!self.repo.join("new.txt").exists());
            repo
        }
    }

    #[tokio::test]
    async fn checkpoint_does_not_run_repo_fsmonitor() {
        let h = Hostile::new();
        h.config("core.fsmonitor", &h.script("fsmonitor").to_string_lossy());
        h.turn_and_undo().await;
        assert_eq!(h.marked(), Vec::<String>::new());
    }

    #[tokio::test]
    async fn checkpoint_does_not_run_repo_hooks() {
        // Hooks in a configured hooksPath ...
        let h = Hostile::new();
        let dir = h.marks.parent().unwrap().join("hooks");
        for n in [
            "pre-commit",
            "post-checkout",
            "post-index-change",
            "post-commit",
        ] {
            h.hook(&dir, n);
        }
        h.config("core.hooksPath", &dir.to_string_lossy());
        h.turn_and_undo().await;
        assert_eq!(h.marked(), Vec::<String>::new());
        // ... and in .git/hooks.
        let h = Hostile::new();
        for n in [
            "pre-commit",
            "post-checkout",
            "post-index-change",
            "post-commit",
        ] {
            h.hook(&h.repo.join(".git/hooks"), n);
        }
        h.turn_and_undo().await;
        assert_eq!(h.marked(), Vec::<String>::new());
    }

    #[tokio::test]
    async fn checkpoint_diff_and_restore_do_not_run_repo_filters() {
        let h = Hostile::new();
        std::fs::write(h.repo.join(".gitattributes"), "* filter=evil\n").unwrap();
        h.config("filter.evil.clean", &h.script("clean").to_string_lossy());
        h.config("filter.evil.smudge", &h.script("smudge").to_string_lossy());
        h.config("filter.evil.required", "true");
        h.turn_and_undo().await;
        assert_eq!(h.marked(), Vec::<String>::new());
    }

    #[tokio::test]
    async fn harness_diffs_do_not_run_external_diff_or_textconv() {
        let h = Hostile::new();
        std::fs::write(h.repo.join(".gitattributes"), "*.txt diff=tc\n").unwrap();
        h.config("diff.external", &h.script("external").to_string_lossy());
        h.config("diff.tc.textconv", &h.script("textconv").to_string_lossy());
        h.turn_and_undo().await;
        // A sub-agent's base, worktree, result and merge-back: all in the
        // shadow, none of it under the repository's config (R.1, S-1).
        let sub = sub_repo(&h.repo, Some(&h.base), &Bounds::default())
            .await
            .unwrap()
            .unwrap();
        let base = sub.base("c").await.unwrap();
        let wt = sub.make_worktree("c", &base).await.unwrap();
        assert_eq!(
            std::fs::read_to_string(wt.path.join("a.txt")).unwrap(),
            "one\n"
        );
        std::fs::write(wt.path.join("a.txt"), "changed\n").unwrap();
        let result = sub.result(&wt, "c").await.unwrap();
        assert_eq!(sub.apply(&base, &result, "c").await.unwrap(), 1);
        remove_worktree(&wt).await;
        assert!(!wt.path.exists() && !wt.admin.exists());
        assert_eq!(
            std::fs::read_to_string(h.repo.join("a.txt")).unwrap(),
            "changed\n"
        );
        assert_eq!(h.marked(), Vec::<String>::new());
    }

    /// Checkpoints of a repository live outside it, honour its ignore
    /// rules (`.gitignore` and `.git/info/exclude`) and restore bytes exactly.
    #[tokio::test]
    async fn repository_checkpoints_live_outside_and_restore_exact_bytes() {
        let h = Hostile::new();
        std::fs::write(h.repo.join(".gitignore"), "secret.env\n").unwrap();
        std::fs::write(h.repo.join(".git/info/exclude"), "local.log\n").unwrap();
        // Line-ending normalisation must not change what undo writes back.
        std::fs::write(h.repo.join(".gitattributes"), "* text\n").unwrap();
        std::fs::write(h.repo.join("mixed.txt"), "a\r\nb\n").unwrap();
        std::fs::write(h.repo.join("secret.env"), "KEY=1\n").unwrap();
        std::fs::write(h.repo.join("local.log"), "log\n").unwrap();
        let index_before = std::fs::read(h.repo.join(".git/index")).unwrap();
        let repo = repo_for(&h.repo, Some(&h.base)).await.unwrap();
        let before = snapshot(&repo, "refs/harness/s/t/before", "before")
            .await
            .unwrap();
        std::fs::write(h.repo.join("mixed.txt"), "gone\n").unwrap();
        std::fs::write(h.repo.join("secret.env"), "KEY=2\n").unwrap();
        std::fs::write(h.repo.join("local.log"), "log2\n").unwrap();
        let after = snapshot(&repo, "refs/harness/s/t/after", "after")
            .await
            .unwrap();
        let ch = changed(&repo, &before, &after).await.unwrap();
        assert_eq!(ch, [('M', "mixed.txt".to_string())]);
        restore(&repo, &before, &after, false).await.unwrap();
        assert_eq!(
            std::fs::read(h.repo.join("mixed.txt")).unwrap(),
            b"a\r\nb\n"
        );
        // Ignored files are left alone.
        assert_eq!(
            std::fs::read_to_string(h.repo.join("secret.env")).unwrap(),
            "KEY=2\n"
        );
        assert_eq!(
            std::fs::read_to_string(h.repo.join("local.log")).unwrap(),
            "log2\n"
        );
        // Nothing was written into the user's repository.
        assert_eq!(
            std::fs::read(h.repo.join(".git/index")).unwrap(),
            index_before
        );
        let refs = std::process::Command::new("git")
            .arg("-C")
            .arg(&h.repo)
            .args(["for-each-ref", "refs/harness/"])
            .output()
            .unwrap();
        assert!(
            refs.stdout.is_empty(),
            "{}",
            String::from_utf8_lossy(&refs.stdout)
        );
        assert!(!h.repo.join(".git/harness-checkpoints").exists());
    }

    /// Every file under `dir` with its bytes and modification time.
    pub(super) fn dir_state(dir: &Path) -> Vec<(PathBuf, Vec<u8>, std::time::SystemTime)> {
        walkdir::WalkDir::new(dir)
            .sort_by_file_name()
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .map(|e| {
                let m = e.metadata().unwrap().modified().unwrap();
                (e.path().to_path_buf(), std::fs::read(e.path()).unwrap(), m)
            })
            .collect()
    }

    /// A.16: the blobs a restore writes are checked and copied into the
    /// shadow's own objects, in a repository whose config names commands
    /// (fsmonitor, hooks): nothing runs, and no file of the user's `.git`
    /// changes, not even a modification time.
    #[tokio::test]
    async fn securing_blobs_copies_them_and_writes_nothing_in_the_users_repository() {
        let h = Hostile::new();
        sh(&h.repo, &["gc", "-q"]); // the blobs sit in a pack, as in any older repository
        // After the test's own git call, which would run it.
        h.config("core.fsmonitor", &h.script("fsmonitor").to_string_lossy());
        h.hook(&h.repo.join(".git/hooks"), "post-checkout");
        let repo = repo_for(&h.repo, Some(&h.base)).await.unwrap();
        let before = snapshot(&repo, "refs/harness/s/t/before", "before")
            .await
            .unwrap();
        std::fs::write(h.repo.join("a.txt"), "two\n").unwrap();
        let after = snapshot(&repo, "refs/harness/s/t/after", "after")
            .await
            .unwrap();
        let one = run(&repo, &["rev-parse", &format!("{before}:a.txt")], None)
            .await
            .unwrap();
        let alternates = h.base.read_dir().unwrap().next().unwrap().unwrap().path();
        let alternates = alternates.join("objects/info/alternates");
        let owns = |id: String| {
            let alt = alternates.clone();
            let repo = repo.clone();
            async move {
                let text = std::fs::read(&alt).unwrap();
                std::fs::remove_file(&alt).unwrap();
                let ok = run(&repo, &["cat-file", "-e", &id], None).await.is_ok();
                std::fs::write(&alt, text).unwrap();
                ok
            }
        };
        assert!(!owns(one.clone()).await, "a.txt's blob is not borrowed");
        let user_git = dir_state(&h.repo.join(".git"));
        tokio::time::sleep(Duration::from_millis(20)).await;
        let diff = restore_diff(&repo, &before, &after).await.unwrap();
        let plan = restore_plan(&repo, &diff, &after, false).await.unwrap();
        secure_blobs(&repo, &plan).await.unwrap();
        assert!(owns(one).await, "a.txt's blob was not copied");
        assert!(
            dir_state(&h.repo.join(".git")) == user_git,
            "the user's .git changed"
        );
        assert_eq!(h.marked(), Vec::<String>::new());
    }

    /// A blob that is gone is named, in plain words, before anything else
    /// happens, and nothing is copied.
    #[tokio::test]
    async fn securing_a_missing_blob_is_refused_with_its_path() {
        let h = Hostile::new();
        let repo = repo_for(&h.repo, Some(&h.base)).await.unwrap();
        let before = snapshot(&repo, "refs/harness/s/t/before", "before")
            .await
            .unwrap();
        std::fs::write(h.repo.join("a.txt"), "two\n").unwrap();
        let after = snapshot(&repo, "refs/harness/s/t/after", "after")
            .await
            .unwrap();
        let diff = restore_diff(&repo, &before, &after).await.unwrap();
        let mut plan = restore_plan(&repo, &diff, &after, false).await.unwrap();
        plan.writes[0].1 = "1".repeat(40);
        let e = secure_blobs(&repo, &plan).await.unwrap_err();
        assert!(e.starts_with("nothing was changed:"), "{e}");
        assert!(e.contains("missing: `a.txt`"), "{e}");
    }

    /// A `write-tree` that fails for a reason a rebuild does not fix (the
    /// object directories are read-only): one rebuild and retry, and the
    /// error carries both failures.
    #[tokio::test]
    async fn a_snapshot_that_fails_after_the_rebuild_reports_both_failures() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let ws = d.path().join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(ws.join("a.txt"), "a\n").unwrap();
        std::fs::write(ws.join("b.txt"), "b\n").unwrap();
        let repo = repo_for(&ws, Some(&d.path().join("cp"))).await.unwrap();
        snapshot(&repo, "refs/harness/s/t/before", "b")
            .await
            .unwrap();
        // A new tree is needed, no new blob.
        std::fs::remove_file(ws.join("b.txt")).unwrap();
        let objects = repo.shadow.clone().unwrap().join("objects");
        let dirs: Vec<PathBuf> = walkdir::WalkDir::new(&objects)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_dir())
            .map(|e| e.path().to_path_buf())
            .collect();
        let mode = |m| {
            for p in &dirs {
                std::fs::set_permissions(p, std::fs::Permissions::from_mode(m)).unwrap();
            }
        };
        mode(0o555);
        if std::fs::write(objects.join("probe"), "").is_ok() {
            mode(0o755);
            eprintln!("SKIPPED: read-only folders are writable here (root?)");
            return;
        }
        let e = snapshot(&repo, "refs/harness/s/u/before", "b").await;
        mode(0o755);
        let e = e.unwrap_err();
        assert!(
            e.contains("before that, with the kept index: git write-tree"),
            "{e}"
        );
    }

    /// After a failed copy, only what `pack-objects` leaves behind goes:
    /// new temporary files and a new pack without its index. Older files,
    /// and a complete new pack, stay.
    #[tokio::test]
    async fn a_failed_copy_removes_only_its_own_leftovers() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path();
        for f in ["pack-old.pack", "pack-old.idx", "tmp_pack_old", "other"] {
            std::fs::write(p.join(f), "").unwrap();
        }
        let had = pack_dir_names(p).await;
        for f in [
            "tmp_pack_ab12",
            "tmp_idx_cd34",
            "tmp_rev_ef56",
            "pack-half.pack",
            "pack-half.rev",
            "pack-whole.pack",
            "pack-whole.idx",
            "unrelated",
        ] {
            std::fs::write(p.join(f), "").unwrap();
        }
        remove_failed_copy(p, &had).await;
        let mut left: Vec<String> = pack_dir_names(p)
            .await
            .into_iter()
            .map(|n| n.to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(
            left,
            [
                "other",
                "pack-old.idx",
                "pack-old.pack",
                "pack-whole.idx",
                "pack-whole.pack",
                "tmp_pack_old",
                "unrelated"
            ]
        );
    }

    #[tokio::test]
    async fn plain_folder_round_trip() {
        let d = tempfile::tempdir().unwrap();
        let ws = d.path().join("ws");
        std::fs::create_dir_all(ws.join("node_modules")).unwrap();
        std::fs::write(ws.join("a.txt"), "one\n").unwrap();
        std::fs::write(ws.join("node_modules/x.js"), "x\n").unwrap();
        let repo = repo_for(&ws, Some(&d.path().join("cp"))).await.unwrap();
        assert!(repo.is_shadow());
        let before = snapshot(&repo, "refs/harness/s/t/before", "b")
            .await
            .unwrap();
        std::fs::write(ws.join("a.txt"), "two\n").unwrap();
        std::fs::write(ws.join("b.txt"), "b\n").unwrap();
        std::fs::write(ws.join("node_modules/x.js"), "y\n").unwrap();
        let after = snapshot(&repo, "refs/harness/s/t/after", "a")
            .await
            .unwrap();
        restore(&repo, &before, &after, false).await.unwrap();
        assert_eq!(std::fs::read_to_string(ws.join("a.txt")).unwrap(), "one\n");
        assert!(!ws.join("b.txt").exists());
        assert_eq!(
            std::fs::read_to_string(ws.join("node_modules/x.js")).unwrap(),
            "y\n"
        );
        assert!(!ws.join(".git").exists());
    }

    #[tokio::test]
    async fn making_a_worktree_runs_no_hooks() {
        let h = Hostile::new();
        h.hook(&h.repo.join(".git/hooks"), "post-checkout");
        h.hook(&h.repo.join(".git/hooks"), "reference-transaction");
        let sub = sub_repo(&h.repo, Some(&h.base), &Bounds::default())
            .await
            .unwrap()
            .unwrap();
        let base = sub.base("c").await.unwrap();
        let wt = sub.make_worktree("c", &base).await.unwrap();
        assert!(wt.path.join("a.txt").exists());
        // The child's own git sees a clean worktree at `HEAD` (its index
        // is `HEAD`'s tree), and its changes as changes.
        std::fs::write(wt.path.join("b.txt"), "child\n").unwrap();
        let st = std::process::Command::new("git")
            .arg("-C")
            .arg(&wt.path)
            .args(["status", "--porcelain"])
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(st.status.success(), "{st:?}");
        assert_eq!(String::from_utf8_lossy(&st.stdout), " M b.txt\n");
        remove_worktree(&wt).await;
        assert!(!wt.path.exists() && !wt.admin.exists());
        assert_eq!(h.marked(), Vec::<String>::new());
    }

    /// A repository whose config and committed attributes define a filter
    /// gets a worktree all the same (R.1): nothing in it is converted by the
    /// node's git, so the filter never runs, and the files are the bytes of
    /// the folder.
    #[tokio::test]
    async fn making_a_worktree_never_runs_repo_defined_filters() {
        let h = Hostile::new();
        std::fs::write(h.repo.join(".gitattributes"), "* filter=evil\n").unwrap();
        sh(&h.repo, &["add", "."]);
        sh(&h.repo, &["commit", "-qm", "attrs"]);
        h.config("filter.evil.smudge", &h.script("smudge").to_string_lossy());
        h.config("filter.evil.clean", &h.script("clean").to_string_lossy());
        h.config("filter.evil.required", "true");
        let sub = sub_repo(&h.repo, Some(&h.base), &Bounds::default())
            .await
            .unwrap()
            .unwrap();
        let base = sub.base("c").await.unwrap();
        let wt = sub.make_worktree("c", &base).await.unwrap();
        assert_eq!(
            std::fs::read_to_string(wt.path.join("a.txt")).unwrap(),
            "one\n"
        );
        std::fs::write(wt.path.join("b.txt"), "changed\n").unwrap();
        let result = sub.result(&wt, "c").await.unwrap();
        assert_eq!(sub.apply(&base, &result, "c").await.unwrap(), 1);
        remove_worktree(&wt).await;
        assert_eq!(h.marked(), Vec::<String>::new());
    }

    /// A stand-in `git` that runs `body` (a shell script).
    pub(super) fn fake_git(dir: &Path, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let p = dir.join("git");
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    /// `exec` of a script this test just wrote, tolerating ETXTBSY.
    ///
    /// Writing the script and executing it in one process races with other
    /// test threads: a thread that forks while our write descriptor is open
    /// gives the child a copy, held until that child's `exec`, and the kernel
    /// refuses to execute a file with a writer (`Text file busy`). The
    /// descriptor is close-on-exec, so the window is tiny, but it cannot be
    /// closed from here, and renaming does not help (same inode). A failed
    /// spawn ran nothing, so retrying it is safe; only ETXTBSY is retried,
    /// and only for a bounded time.
    async fn exec_script(
        fake: &Path,
        args: &[&str],
        timeout: Duration,
    ) -> Result<std::process::Output, String> {
        let mut tries = 0;
        loop {
            let mut c = hardened(fake);
            c.args(args);
            match exec(c, None, timeout).await {
                Err(e) if e.contains("Text file busy") && tries < 100 => {
                    tries += 1;
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                r => return r,
            }
        }
    }

    #[tokio::test]
    async fn a_hung_git_is_killed_at_the_timeout() {
        let d = tempfile::tempdir().unwrap();
        let pidfile = d.path().join("pid");
        // The hung process is a grandchild, as a filter or hook would be.
        let fake = fake_git(
            d.path(),
            &format!("sleep 30 &\necho $! > '{}'\nwait", pidfile.display()),
        );
        let t = std::time::Instant::now();
        let e = exec_script(&fake, &[], Duration::from_millis(300))
            .await
            .unwrap_err();
        assert!(e.contains("did not finish"), "{e}");
        assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
        // The whole process group went with it.
        let pid: i32 = std::fs::read_to_string(&pidfile)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        let alive = nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_ok()
            && !std::fs::read_to_string(format!("/proc/{pid}/stat"))
                .unwrap_or_default()
                .contains(") Z");
        assert!(!alive, "sleep {pid} survived the timeout");
    }

    #[tokio::test]
    async fn git_gets_a_clean_environment_and_the_overrides() {
        let d = tempfile::tempdir().unwrap();
        let fake = fake_git(d.path(), "env; echo ARGS \"$@\"");
        let out = exec_script(&fake, &["status"], GIT_TIMEOUT).await.unwrap();
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        let allowed = [
            "PATH", "HOME", "LANG", "TMPDIR", "PWD", "SHLVL", "_", "OLDPWD",
        ];
        for line in text.lines().filter(|l| !l.starts_with("ARGS")) {
            let name = line.split('=').next().unwrap();
            let ours = [
                "GIT_CONFIG_NOSYSTEM",
                "GIT_CONFIG_GLOBAL",
                "GIT_TERMINAL_PROMPT",
                "GIT_ASKPASS",
                "SSH_ASKPASS",
                "GIT_PAGER",
                "GIT_NO_LAZY_FETCH",
                "GIT_OPTIONAL_LOCKS",
                "GIT_AUTHOR_NAME",
                "GIT_AUTHOR_EMAIL",
                "GIT_COMMITTER_NAME",
                "GIT_COMMITTER_EMAIL",
            ];
            // Anything else, e.g. this process's own GIT_CONFIG_PARAMETERS
            // or GIT_DIR, must not reach git.
            assert!(
                allowed.contains(&name) || ours.contains(&name),
                "unexpected variable {line}"
            );
        }
        for want in [
            "GIT_CONFIG_NOSYSTEM=1",
            "GIT_CONFIG_GLOBAL=/dev/null",
            "GIT_TERMINAL_PROMPT=0",
            "GIT_ASKPASS=",
        ] {
            assert!(text.lines().any(|l| l == want), "{want} missing:\n{text}");
        }
        let args = text.lines().find(|l| l.starts_with("ARGS")).unwrap();
        for o in [
            "-c core.fsmonitor=false",
            "-c core.hooksPath=/dev/null",
            "-c core.pager=cat",
            "-c protocol.ext.allow=never",
        ] {
            assert!(args.contains(o), "{o} missing: {args}");
        }
    }

    /// A user environment whose global git config (a temp file) names a
    /// global ignore file with `*.swp` and `.idea/`.
    fn user_env(dir: &Path) -> UserGitEnv {
        let home = dir.join("home");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(home.join("my-ignore"), "*.swp\n.idea/\n").unwrap();
        let cfg = dir.join("global.gitconfig");
        std::fs::write(&cfg, "[core]\n\texcludesFile = ~/my-ignore\n").unwrap();
        UserGitEnv {
            home: Some(home),
            xdg_config_home: None,
            global_config: Some(cfg),
        }
    }

    /// The checkpoint repository of `cwd` under `base`, for the user
    /// environment `env`, without bounds.
    async fn with_env(cwd: &Path, base: &Path, env: &UserGitEnv) -> Repo {
        repo_for_with(
            cwd,
            Some(base),
            &Bounds::default(),
            env,
            Shadow::Checkpoints,
        )
        .await
        .unwrap()
        .unwrap()
        .0
    }

    async fn tree_files(repo: &Repo, sha: &str) -> Vec<String> {
        run(repo, &["ls-tree", "-r", "--name-only", sha], None)
            .await
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// Files ignored only by the user's global ignore file are neither
    /// captured nor touched by undo, as before checkpoints moved out.
    #[tokio::test]
    async fn global_ignore_file_is_honoured() {
        let h = Hostile::new();
        let env = user_env(h.marks.parent().unwrap());
        std::fs::write(h.repo.join("old.swp"), "v1\n").unwrap();
        let repo = with_env(&h.repo, &h.base, &env).await;
        let before = snapshot(&repo, "refs/harness/s/t/before", "b")
            .await
            .unwrap();
        // (1) not in the checkpoint tree
        let files = tree_files(&repo, &before).await;
        assert!(!files.iter().any(|f| f.ends_with(".swp")), "{files:?}");
        std::fs::write(h.repo.join("a.txt"), "two\n").unwrap();
        std::fs::write(h.repo.join("old.swp"), "v2\n").unwrap();
        std::fs::write(h.repo.join("new.swp"), "fresh\n").unwrap();
        std::fs::create_dir_all(h.repo.join(".idea")).unwrap();
        std::fs::write(h.repo.join(".idea/ws.xml"), "x\n").unwrap();
        let after = snapshot(&repo, "refs/harness/s/t/after", "a")
            .await
            .unwrap();
        assert_eq!(
            changed(&repo, &before, &after).await.unwrap(),
            [('M', "a.txt".to_string())]
        );
        restore(&repo, &before, &after, false).await.unwrap();
        assert_eq!(
            std::fs::read_to_string(h.repo.join("a.txt")).unwrap(),
            "one\n"
        );
        // (2) created after the checkpoint: survives undo
        assert_eq!(
            std::fs::read_to_string(h.repo.join("new.swp")).unwrap(),
            "fresh\n"
        );
        assert!(h.repo.join(".idea/ws.xml").exists());
        // (3) modified after the checkpoint: not rolled back
        assert_eq!(
            std::fs::read_to_string(h.repo.join("old.swp")).unwrap(),
            "v2\n"
        );
    }

    /// Without `core.excludesFile`, git's default location applies
    /// (`$XDG_CONFIG_HOME/git/ignore`), for plain folders too; an oversized
    /// or non-regular ignore file is skipped.
    #[tokio::test]
    async fn default_global_ignore_location_is_honoured() {
        let d = tempfile::tempdir().unwrap();
        let xdg = d.path().join("xdg");
        std::fs::create_dir_all(xdg.join("git")).unwrap();
        std::fs::write(xdg.join("git/ignore"), "*.swp\n").unwrap();
        let cfg = d.path().join("empty.gitconfig");
        std::fs::write(&cfg, "").unwrap();
        let env = UserGitEnv {
            home: Some(d.path().join("nohome")),
            xdg_config_home: Some(xdg.clone()),
            global_config: Some(cfg),
        };
        let ws = d.path().join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(ws.join("a.txt"), "a\n").unwrap();
        std::fs::write(ws.join("x.swp"), "s\n").unwrap();
        let base = d.path().join("cp");
        let repo = with_env(&ws, &base, &env).await;
        let sha = snapshot(&repo, "refs/harness/s/t/before", "b")
            .await
            .unwrap();
        assert_eq!(tree_files(&repo, &sha).await, ["a.txt"]);
        // A directory where the ignore file should be: skipped, no error.
        std::fs::remove_file(xdg.join("git/ignore")).unwrap();
        std::fs::create_dir_all(xdg.join("git/ignore")).unwrap();
        let repo = with_env(&ws, &base, &env).await;
        let sha = snapshot(&repo, "refs/harness/s/t2/before", "b")
            .await
            .unwrap();
        assert_eq!(tree_files(&repo, &sha).await, ["a.txt", "x.swp"]);
    }

    /// Timing only: `cargo test -p harness-node --lib bench_checkpoint -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn bench_checkpoint_20k_files() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path().join("big");
        for i in 0..200 {
            let dir = r.join(format!("d{i}"));
            std::fs::create_dir_all(&dir).unwrap();
            for j in 0..100 {
                std::fs::write(dir.join(format!("f{j}.txt")), format!("file {i} {j}\n")).unwrap();
            }
        }
        sh(&r, &["init", "-q", "-b", "main"]);
        sh(&r, &["add", "."]);
        sh(&r, &["commit", "-qm", "init", "--no-gpg-sign"]);
        let base = d.path().join("cp");
        for run in 1..=3 {
            let t = std::time::Instant::now();
            let repo = repo_for(&r, Some(&base)).await.unwrap();
            snapshot(&repo, &format!("refs/harness/s/{run}/before"), "b")
                .await
                .unwrap();
            eprintln!("checkpoint run {run}: {:?}", t.elapsed());
            std::fs::write(r.join("d0/f0.txt"), format!("edit {run}\n")).unwrap();
        }
    }

    #[tokio::test]
    async fn snapshot_restore_round_trip_leaves_user_state_alone() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        sh(r, &["init", "-q", "-b", "main"]);
        std::fs::write(r.join("a.txt"), "one\n").unwrap();
        std::fs::write(r.join(".gitignore"), "secret.env\n").unwrap();
        sh(r, &["add", "."]);
        sh(r, &["commit", "-qm", "init"]);
        std::fs::write(r.join("staged.txt"), "user work\n").unwrap();
        sh(r, &["add", "staged.txt"]);
        std::fs::write(r.join("secret.env"), "KEY=1\n").unwrap();
        let cp = tempfile::tempdir().unwrap();
        assert_eq!(repo_root(r).await.unwrap(), r.canonicalize().unwrap());
        let root = repo_for(r, Some(cp.path())).await.unwrap();
        let index_before = std::fs::read(r.join(".git/index")).unwrap();

        let before = snapshot(&root, "refs/harness/s/t/before", "before")
            .await
            .unwrap();
        // The "turn": edit, delete, create.
        std::fs::write(r.join("a.txt"), "two\n").unwrap();
        std::fs::write(r.join("new.txt"), "made by agent\n").unwrap();
        std::fs::remove_file(r.join("staged.txt")).unwrap();
        let after = snapshot(&root, "refs/harness/s/t/after", "after")
            .await
            .unwrap();
        let ch = changed(&root, &before, &after).await.unwrap();
        assert_eq!(ch.len(), 3, "{ch:?}");
        assert!(
            !ch.iter().any(|(_, p)| p == "secret.env"),
            "ignored files are not captured"
        );

        // User's index and HEAD are untouched by snapshots.
        assert_eq!(std::fs::read(r.join(".git/index")).unwrap(), index_before);

        // Someone edits a touched file after the turn: undo refuses.
        std::fs::write(r.join("a.txt"), "three\n").unwrap();
        let e = restore(&root, &before, &after, false).await.unwrap_err();
        assert!(e.contains("a.txt"), "{e}");
        std::fs::write(r.join("a.txt"), "two\n").unwrap();
        // An unrelated later edit does not block it and survives.
        std::fs::write(r.join(".gitignore"), "secret.env\n*.log\n").unwrap();

        let files = restore(&root, &before, &after, false).await.unwrap();
        assert_eq!(files.len(), 3);
        assert_eq!(std::fs::read_to_string(r.join("a.txt")).unwrap(), "one\n");
        assert_eq!(
            std::fs::read_to_string(r.join("staged.txt")).unwrap(),
            "user work\n"
        );
        assert!(!r.join("new.txt").exists());
        assert!(r.join("secret.env").exists());
        assert!(
            std::fs::read_to_string(r.join(".gitignore"))
                .unwrap()
                .contains("*.log")
        );
        assert_eq!(std::fs::read(r.join(".git/index")).unwrap(), index_before);
        assert!(
            repo_root(&std::env::temp_dir().join("definitely-not-a-repo-xyz"))
                .await
                .is_none()
        );
    }
}

/// Experiment B.5: what checkpoints and undo do with a git repository nested
/// in the folder. Test-only; lives in its own file.
#[cfg(test)]
#[path = "git_nested_tests.rs"]
mod nested_tests;

/// Experiment B.3: what a turn's checkpoints cost as the folder grows.
/// Ignored benchmark; lives in its own file.
#[cfg(test)]
#[path = "git_cost_bench.rs"]
mod cost_bench;

/// The checkpoint root and what a restore may write stay inside the
/// session's bounds (R.1, S-3). Test-only; lives in its own file.
#[cfg(test)]
#[path = "git_bounds_tests.rs"]
mod bounds_tests;
