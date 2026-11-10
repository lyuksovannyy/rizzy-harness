//! Turn checkpoints in git, and the git calls for sub-agent worktrees.
//!
//! Checkpoints always live in a harness-owned *shadow* repository in the
//! node's data directory (`checkpoints/<hash>.git`, run as `--git-dir
//! <shadow> --work-tree <folder>`), for git repositories and plain folders
//! alike. The user's `.git/config`, hooks, filters and index are never
//! consulted for a checkpoint, a diff or an undo; in a repository the shadow
//! borrows the user's objects through `objects/info/alternates` so files
//! already committed are not stored twice, and copies `.git/info/exclude`
//! so the same files are ignored. Only the latest N checkpoints are kept.
//!
//! Sub-agent worktrees must run in the user's repository. Those calls get
//! the same hardened base (no system/global config, hooks off, fsmonitor
//! off, no transports, timeouts), and a worktree is refused when the
//! repository's own config defines a filter command, which would otherwise
//! run during checkout.
//!
//! Every git process gets a cleared environment, command-line overrides for
//! the config keys that run commands, a timeout, and is killed (with its
//! process group) when the call is dropped.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::process::Command;

/// Where checkpoints for one working folder are stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    /// The folder whose files are snapshotted.
    pub root: PathBuf,
    /// The harness-owned git dir (always set for turn checkpoints; `None`
    /// only for the sub-agent calls that must use the user's repository).
    shadow: Option<PathBuf>,
    /// The empty tree of the shadow's object format (its attribute source).
    empty_tree: &'static str,
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
    pub fn project(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            shadow: None,
            empty_tree: empty_tree("sha1"),
        }
    }

    pub fn is_shadow(&self) -> bool {
        self.shadow.is_some()
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

fn command(repo: &Repo, index: Option<&Path>) -> Command {
    let mut c = hardened(Path::new("git"));
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
        }
        None => {
            c.arg("-C").arg(&repo.root);
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

async fn git(dir: &Path, args: &[&str], index: Option<&Path>) -> Result<String, String> {
    run(&Repo::project(dir), args, index).await
}

/// The repository's own config, as `(key, value)`, read without running
/// anything (`git config --list` executes nothing; includes are followed
/// because the other git calls follow them too).
async fn local_config(root: &Path) -> Result<Vec<(String, String)>, String> {
    let out = git(
        root,
        &["config", "--list", "--includes", "--show-scope", "-z"],
        None,
    )
    .await?;
    // With -z: `scope NUL key LF value NUL` per entry.
    let fields: Vec<&str> = out.split('\0').collect();
    Ok(fields
        .chunks(2)
        .filter_map(|c| match c {
            // Our own overrides show up as "command".
            [scope, entry] if *scope != "command" && !entry.is_empty() => {
                let (k, v) = entry.split_once('\n').unwrap_or((entry, ""));
                Some((k.to_string(), v.to_string()))
            }
            _ => None,
        })
        .collect())
}

/// git-lfs's own filter commands, the only filter definitions allowed.
const LFS_STANDARD: &[(&str, &str)] = &[
    ("clean", "git-lfs clean -- %f"),
    ("smudge", "git-lfs smudge -- %f"),
    ("smudge", "git-lfs smudge --skip -- %f"),
    ("process", "git-lfs filter-process"),
    ("process", "git-lfs filter-process --skip"),
];

/// Whether a config entry would run a command during a worktree checkout,
/// `add` or `apply` despite the overrides: filter drivers, and git-lfs's
/// own command hooks (custom transfer agents, extensions).
fn runs_command(key: &str, value: &str) -> bool {
    let k = key.to_ascii_lowercase();
    if let Some(rest) = k.strip_prefix("filter.") {
        let Some((name, var)) = rest.rsplit_once('.') else {
            return false;
        };
        if !matches!(var, "clean" | "smudge" | "process") {
            return false;
        }
        return !(name == "lfs" && LFS_STANDARD.contains(&(var, value.trim())));
    }
    k.starts_with("lfs.customtransfer.")
        || k.starts_with("lfs.extension.")
        || k == "lfs.standalonetransferagent"
}

/// Config keys in the repository at `root` that would run a command in a
/// sub-agent worktree. Empty means a worktree may be created.
pub async fn worktree_hazards(root: &Path) -> Result<Vec<String>, String> {
    let mut bad: Vec<String> = local_config(root)
        .await?
        .into_iter()
        .filter(|(k, v)| runs_command(k, v))
        .map(|(k, _)| k)
        .collect();
    bad.sort();
    bad.dedup();
    Ok(bad)
}

/// Refuse sub-agent worktrees in a repository whose own config defines
/// commands that checkout would run.
pub async fn check_worktree_safe(root: &Path) -> Result<(), String> {
    let bad = worktree_hazards(root).await?;
    if bad.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "this repository's own git config defines commands that a checkout would run ({}); \
             no separate worktree is created in it",
            bad.join(", ")
        ))
    }
}

/// The commit, branch and whether tracked files changed, for the folder a
/// published file came from (display only).
pub async fn describe(dir: &Path) -> Option<(String, String, bool)> {
    let commit = git(dir, &["rev-parse", "HEAD"], None).await.ok()?;
    let branch = git(dir, &["rev-parse", "--abbrev-ref", "HEAD"], None)
        .await
        .unwrap_or_default();
    // `status` may run clean filters: switch off every filter the
    // repository defines (an empty command is a no-op).
    let mut args: Vec<String> = Vec::new();
    let mut names: Vec<String> = local_config(dir)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(k, _)| {
            let rest = k.strip_prefix("filter.")?;
            let (name, _) = rest.rsplit_once('.')?;
            Some(name.to_string())
        })
        .collect();
    names.sort();
    names.dedup();
    for n in names {
        for (var, val) in [
            ("clean", ""),
            ("smudge", ""),
            ("process", ""),
            ("required", "false"),
        ] {
            args.push("-c".into());
            args.push(format!("filter.{n}.{var}={val}"));
        }
    }
    args.extend(["status", "--porcelain", "--untracked-files=no"].map(String::from));
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let status = git(dir, &refs, None).await.unwrap_or_default();
    Some((commit, branch, !status.is_empty()))
}

/// What one `rev-parse` says about the repository containing a folder.
struct RepoInfo {
    root: PathBuf,
    common_dir: PathBuf,
    objects: PathBuf,
    exclude: PathBuf,
    format: String,
}

async fn repo_info(cwd: &Path) -> Option<RepoInfo> {
    let out = git(
        cwd,
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
        None,
    )
    .await
    .ok()?;
    let l: Vec<&str> = out.lines().collect();
    let [root, common, objects, exclude, format] = l[..] else {
        return None;
    };
    let root = PathBuf::from(root);
    // A `core.worktree` pointing elsewhere does not move the checkpoint
    // root away from the folder the session works in.
    if root.as_os_str().is_empty() || !cwd.starts_with(&root) {
        return None;
    }
    Some(RepoInfo {
        root,
        common_dir: PathBuf::from(common),
        objects: PathBuf::from(objects),
        exclude: PathBuf::from(exclude),
        format: format.to_string(),
    })
}

/// The repository containing `cwd`, if any.
pub async fn repo_root(cwd: &Path) -> Option<PathBuf> {
    let cwd = cwd.canonicalize().ok()?;
    repo_info(&cwd).await.map(|i| i.root)
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
/// `cwd`, or `cwd` itself when it is not in one. `None` when no shadow base
/// is configured, and for a sub-agent's temporary worktree (it is removed
/// when the sub-agent ends, so its checkpoints could never be used).
pub async fn repo_for(cwd: &Path, shadow_base: Option<&Path>) -> Option<Repo> {
    repo_for_with(cwd, shadow_base, &UserGitEnv::from_process()).await
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

async fn repo_for_with(cwd: &Path, shadow_base: Option<&Path>, user: &UserGitEnv) -> Option<Repo> {
    let base = shadow_base?;
    let cwd = cwd.canonicalize().ok()?;
    let info = repo_info(&cwd).await;
    if let Some(i) = &info
        && i.root.starts_with(i.common_dir.join("harness-worktrees"))
    {
        return None;
    }
    let root = info.as_ref().map(|i| i.root.clone()).unwrap_or(cwd);
    let key = {
        use sha2::Digest;
        hex::encode(sha2::Sha256::digest(root.to_string_lossy().as_bytes()))[..16].to_string()
    };
    let dir = base.join(format!("{key}.git"));
    if !dir.join("HEAD").exists() {
        let format = info.as_ref().map_or("sha1", |i| i.format.as_str());
        let mut c = hardened(Path::new("git"));
        c.args(["init", "--bare", "-q", "--object-format", format])
            .arg(&dir);
        let ok = exec(c, None, GIT_TIMEOUT).await.ok()?.status.success();
        if !ok {
            return None;
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
    Some(Repo {
        root,
        shadow: Some(dir),
        empty_tree: empty_tree(shadow_format.trim()),
    })
}

/// A private index seeded from the repository's own (keeps its stat cache,
/// so `add -A` only rehashes what changed). In a shadow repository the
/// index is the shadow's, never the user's.
async fn temp_index(repo: &Repo) -> Result<(PathBuf, PathBuf), String> {
    let real = match &repo.shadow {
        Some(dir) => dir.join("index"),
        None => PathBuf::from(
            run(
                repo,
                &["rev-parse", "--path-format=absolute", "--git-path", "index"],
                None,
            )
            .await?,
        ),
    };
    let tmp = real.with_file_name(format!("harness-index-{}", harness_core::new_id()));
    if real.exists() {
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

/// Tree of the working directory as it is now (tracked + untracked, minus ignored).
async fn worktree_tree(repo: &Repo) -> Result<String, String> {
    let (real, idx) = temp_index(repo).await?;
    let res = async {
        run(repo, &["add", "-A", "--", "."], Some(&idx)).await?;
        run(repo, &["write-tree"], Some(&idx)).await
    }
    .await;
    // The shadow keeps the index git just wrote: the next checkpoint only
    // rehashes files whose stat data changed. (Concurrent snapshots each
    // write a complete index; the last rename wins.)
    if res.is_ok() && repo.is_shadow() && tokio::fs::rename(&idx, &real).await.is_ok() {
        return res;
    }
    let _ = tokio::fs::remove_file(&idx).await;
    res
}

/// Snapshot the working directory into a commit stored at `ref_name`.
pub async fn snapshot(repo: &Repo, ref_name: &str, msg: &str) -> Result<String, String> {
    tokio::time::timeout(SNAPSHOT_TIMEOUT, async {
        let tree = worktree_tree(repo).await?;
        let head = run(repo, &["rev-parse", "--verify", "-q", "HEAD"], None)
            .await
            .ok();
        let mut args = vec!["commit-tree", "--no-gpg-sign", tree.as_str(), "-m", msg];
        if let Some(h) = head.as_deref().filter(|h| !h.is_empty()) {
            args.extend(["-p", h]);
        }
        let sha = run(repo, &args, None).await?;
        run(repo, &["update-ref", ref_name, &sha], None).await?;
        record_order(repo, ref_name).await;
        Ok(sha)
    })
    .await
    .map_err(|_| "snapshot took too long (folder too large?)".to_string())?
}

/// Paths that differ between two snapshots, with git's status letter.
pub async fn changed(repo: &Repo, a: &str, b: &str) -> Result<Vec<(char, String)>, String> {
    let out = run(
        repo,
        &[
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--name-status",
            "--no-renames",
            "-z",
            a,
            b,
        ],
        None,
    )
    .await?;
    let parts: Vec<&str> = out.split('\0').filter(|s| !s.is_empty()).collect();
    Ok(parts
        .chunks(2)
        .filter_map(|c| match c {
            [st, path] => Some((st.chars().next().unwrap_or('M'), path.to_string())),
            _ => None,
        })
        .collect())
}

/// Whether a snapshot still exists (old ones are pruned).
pub async fn exists(repo: &Repo, sha: &str) -> bool {
    run(
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
/// = after it. Rewind: `expected` = the latest state.
pub async fn restore(
    repo: &Repo,
    from: &str,
    expected: &str,
    force: bool,
) -> Result<Vec<String>, String> {
    let touched = changed(repo, from, expected).await?;
    if touched.is_empty() {
        return Ok(vec![]);
    }
    let paths: Vec<String> = touched.iter().map(|(_, p)| p.clone()).collect();
    if !force {
        let now = worktree_tree(repo).await?;
        let mut args = vec![
            "diff".to_string(),
            "--no-ext-diff".into(),
            "--no-textconv".into(),
            "--name-only".into(),
            expected.into(),
            now,
            "--".into(),
        ];
        args.extend(paths.iter().cloned());
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let since = run(repo, &refs, None).await?;
        if !since.is_empty() {
            return Err(format!(
                "these files changed after the turn; undo would overwrite that: {}",
                since.lines().collect::<Vec<_>>().join(", ")
            ));
        }
    }
    // Write the old versions through a private index.
    let (_, idx) = temp_index(repo).await?;
    let res = async {
        run(repo, &["read-tree", from], Some(&idx)).await?;
        let restore: Vec<&str> = touched
            .iter()
            .filter(|(st, _)| *st != 'A')
            .map(|(_, p)| p.as_str())
            .collect();
        if !restore.is_empty() {
            let mut args = vec!["checkout-index", "-f", "--"];
            args.extend(restore);
            run(repo, &args, Some(&idx)).await?;
        }
        for (st, p) in &touched {
            if *st == 'A' {
                let _ = tokio::fs::remove_file(repo.root.join(p)).await;
            }
        }
        Ok::<_, String>(())
    }
    .await;
    let _ = tokio::fs::remove_file(&idx).await;
    res?;
    Ok(paths)
}

/// A checkpoint group: a turn's refs (`.../before`, `/after`, `/restored`)
/// or a sub-agent's (`/base`, `/result`).
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

/// Keep only the `keep` most recent checkpoints (groups). In a shadow
/// repository the dropped snapshots are garbage-collected right away; in a
/// project repository git's own gc removes them later.
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
    if !doomed.is_empty() && repo.is_shadow() {
        let _ = run(repo, &["gc", "--prune=now", "--quiet"], None).await;
    }
    doomed.len()
}

/// Serializes applying sub-agent results to one working directory.
static APPLY: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A detached worktree at `path` checked out at `commit`. Refused when the
/// repository's config defines commands the checkout would run (see
/// [`check_worktree_safe`]); hooks (`post-checkout`) never run.
pub async fn worktree_add(root: &Path, path: &Path, commit: &str) -> Result<(), String> {
    check_worktree_safe(root).await?;
    let p = path.to_string_lossy();
    git(
        root,
        &["worktree", "add", "--detach", "-q", &p, commit],
        None,
    )
    .await
    .map(|_| ())
}

pub async fn worktree_remove(root: &Path, path: &Path) {
    let p = path.to_string_lossy();
    let _ = git(root, &["worktree", "remove", "--force", &p], None).await;
    let _ = git(root, &["worktree", "prune"], None).await;
}

/// Where sub-agent worktrees live: inside the repository's git directory,
/// so they never show up as untracked files.
pub async fn worktrees_dir(root: &Path) -> Result<PathBuf, String> {
    Ok(PathBuf::from(
        git(
            root,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            None,
        )
        .await?,
    )
    .join("harness-worktrees"))
}

/// Apply the change `base..result` to the working directory at `root`
/// (files only; index and HEAD untouched). All or nothing.
pub async fn apply(root: &Path, base: &str, result: &str) -> Result<usize, String> {
    let _guard = APPLY.lock().await;
    let n = changed(&Repo::project(root), base, result).await?.len();
    if n == 0 {
        return Ok(0);
    }
    // The patch is passed on byte for byte (not through `run`, which trims).
    let diff = [
        "diff",
        "--no-ext-diff",
        "--no-textconv",
        "--binary",
        base,
        result,
    ];
    let mut c = command(&Repo::project(root), None);
    c.args(diff);
    let out = exec(c, None, GIT_TIMEOUT).await?;
    if !out.status.success() {
        return Err(result_text(&diff, out));
    }
    let args = ["apply", "--whitespace=nowarn", "-"];
    let mut c = command(&Repo::project(root), None);
    c.args(args);
    let res = exec(c, Some(out.stdout), GIT_TIMEOUT).await?;
    if res.status.success() {
        Ok(n)
    } else {
        Err(result_text(&args, res))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(dir: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
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
    struct Hostile {
        _d: tempfile::TempDir,
        repo: PathBuf,
        marks: PathBuf,
        base: PathBuf,
    }

    impl Hostile {
        fn new() -> Self {
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
        fn script(&self, name: &str) -> PathBuf {
            use std::os::unix::fs::PermissionsExt;
            let p = self.marks.parent().unwrap().join(format!("{name}.sh"));
            let m = self.marks.join(name);
            std::fs::write(&p, format!("#!/bin/sh\ntouch '{}'\ncat\n", m.display())).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            p
        }

        fn config(&self, key: &str, value: &str) {
            sh(&self.repo, &["config", "--local", key, value]);
        }

        fn hook(&self, dir: &Path, name: &str) {
            use std::os::unix::fs::PermissionsExt;
            std::fs::create_dir_all(dir).unwrap();
            let p = dir.join(name);
            std::fs::copy(self.script(name), &p).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        fn marked(&self) -> Vec<String> {
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
        // The sub-agent merge-back diff runs in the user's repository.
        let project = Repo::project(&h.repo);
        let base = snapshot(&project, "refs/harness/c/base", "b")
            .await
            .unwrap();
        std::fs::write(h.repo.join("a.txt"), "changed\n").unwrap();
        let result = snapshot(&project, "refs/harness/c/result", "r")
            .await
            .unwrap();
        std::fs::write(h.repo.join("a.txt"), "one\n").unwrap();
        assert_eq!(apply(&h.repo, &base, &result).await.unwrap(), 1);
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
    async fn worktree_add_runs_no_hooks() {
        let h = Hostile::new();
        h.hook(&h.repo.join(".git/hooks"), "post-checkout");
        let head = git(&h.repo, &["rev-parse", "HEAD"], None).await.unwrap();
        let wt = h.marks.parent().unwrap().join("wt");
        worktree_add(&h.repo, &wt, &head).await.unwrap();
        assert!(wt.join("a.txt").exists());
        worktree_remove(&h.repo, &wt).await;
        assert_eq!(h.marked(), Vec::<String>::new());
    }

    #[tokio::test]
    async fn worktree_add_refuses_repo_defined_filters() {
        let h = Hostile::new();
        std::fs::write(h.repo.join(".gitattributes"), "* filter=evil\n").unwrap();
        sh(&h.repo, &["add", "."]);
        sh(&h.repo, &["commit", "-qm", "attrs"]);
        h.config("filter.evil.smudge", &h.script("smudge").to_string_lossy());
        let head = git(&h.repo, &["rev-parse", "HEAD"], None).await.unwrap();
        let wt = h.marks.parent().unwrap().join("wt");
        let e = worktree_add(&h.repo, &wt, &head).await.unwrap_err();
        assert!(e.contains("filter.evil.smudge"), "{e}");
        assert!(!wt.exists());
        assert_eq!(h.marked(), Vec::<String>::new());
    }

    /// A stand-in `git` that runs `body` (a shell script).
    fn fake_git(dir: &Path, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let p = dir.join("git");
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
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
        let e = exec(hardened(&fake), None, Duration::from_millis(300))
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
        let mut c = hardened(&fake);
        c.arg("status");
        let out = exec(c, None, GIT_TIMEOUT).await.unwrap();
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

    #[test]
    fn only_standard_lfs_filters_are_allowed() {
        assert!(!runs_command("filter.lfs.clean", "git-lfs clean -- %f"));
        assert!(!runs_command(
            "filter.lfs.process",
            "git-lfs filter-process"
        ));
        assert!(!runs_command("filter.lfs.required", "true"));
        assert!(runs_command(
            "filter.lfs.clean",
            "git-lfs clean -- %f; evil"
        ));
        assert!(runs_command("filter.LFS2.smudge", "git-lfs smudge -- %f"));
        assert!(runs_command("filter.x.process", "anything"));
        assert!(runs_command("lfs.customtransfer.x.path", "evil"));
        assert!(
            !runs_command("core.fsmonitor", "evil"),
            "overridden instead"
        );
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
        let repo = repo_for_with(&h.repo, Some(&h.base), &env).await.unwrap();
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
        let repo = repo_for_with(&ws, Some(&base), &env).await.unwrap();
        let sha = snapshot(&repo, "refs/harness/s/t/before", "b")
            .await
            .unwrap();
        assert_eq!(tree_files(&repo, &sha).await, ["a.txt"]);
        // A directory where the ignore file should be: skipped, no error.
        std::fs::remove_file(xdg.join("git/ignore")).unwrap();
        std::fs::create_dir_all(xdg.join("git/ignore")).unwrap();
        let repo = repo_for_with(&ws, Some(&base), &env).await.unwrap();
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
        let root = Repo::project(&repo_root(r).await.unwrap());
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
