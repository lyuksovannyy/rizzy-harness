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

use std::os::fd::{AsFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::process::Stdio;

use harness_core::folder::Identity;
use nix::errno::Errno;
use nix::fcntl::{AtFlags, OFlag};
use nix::sys::stat::{Mode, SFlag};
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

/// The chat's folder, opened once for a whole push (both walks, both reads
/// of the ref): every later name is opened relative to this descriptor,
/// one component at a time, with no link followed (F1 of the review). The
/// path is resolved exactly once, here; a folder that is not the session's
/// pinned directory (`expect`: its device and inode) is refused.
pub(crate) struct Folder(OwnedFd);

/// Flags for a folder opened by descriptor: no link followed, never kept
/// across an `exec`.
fn dir_flags() -> OFlag {
    OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC
}

const CHANGED: &str = "the folder's repository changed while it was read (a folder became a link); nothing was \
     pushed";

impl Folder {
    pub(crate) fn open(path: &Path, expect: Option<Identity>) -> Result<Self, String> {
        let fd = nix::fcntl::open(path, dir_flags(), Mode::empty())
            .map_err(|e| format!("the chat's folder cannot be opened: {e}"))?;
        if let Some(want) = expect {
            let st = nix::sys::stat::fstat(&fd).map_err(|e| e.to_string())?;
            if (st.st_dev, st.st_ino) != (want.dev, want.ino) {
                return Err(format!(
                    "{}: the chat's folder is not the directory it was created in",
                    harness_core::folder::REPLACED
                ));
            }
        }
        Ok(Self(fd))
    }
}

/// `name` in `dir`, as a directory, with no link followed. `Ok(None)` when
/// it is gone or is not a directory; `Err` when it is a link (or anything
/// else that is not plain).
fn sub(dir: &impl AsFd, name: &str) -> Result<Option<OwnedFd>, String> {
    match nix::fcntl::openat(dir, name, dir_flags(), Mode::empty()) {
        Ok(fd) => Ok(Some(fd)),
        Err(Errno::ENOENT) => Ok(None),
        // `O_DIRECTORY | O_NOFOLLOW` on a link answers `ENOTDIR` or
        // `ELOOP`: a link refuses (fail closed); a plain file is skipped.
        Err(Errno::ENOTDIR | Errno::ELOOP) => {
            match nix::sys::stat::fstatat(dir, name, AtFlags::AT_SYMLINK_NOFOLLOW) {
                Ok(st)
                    if SFlag::from_bits_truncate(st.st_mode) & SFlag::S_IFMT == SFlag::S_IFREG =>
                {
                    Ok(None)
                }
                Err(Errno::ENOENT) => Ok(None),
                _ => Err(CHANGED.into()),
            }
        }
        Err(e) => Err(format!("cannot read the folder's repository: {e}")),
    }
}

/// The names in directory `dir` (not `.` or `..`), read through a fresh
/// descriptor of that same directory.
fn names(dir: &impl AsFd) -> Result<Vec<String>, String> {
    let fd = nix::fcntl::openat(dir, ".", dir_flags(), Mode::empty())
        .map_err(|e| format!("cannot read the folder's repository: {e}"))?;
    let mut d = nix::dir::Dir::from_fd(fd).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for e in d.iter() {
        let e = e.map_err(|e| e.to_string())?;
        if let Ok(n) = e.file_name().to_str()
            && n != "."
            && n != ".."
        {
            out.push(n.to_string());
        }
    }
    Ok(out)
}

/// `name` in `dir` opened for reading: no link followed, never waiting
/// (a FIFO), a regular file on the opened descriptor.
fn open_file(dir: &impl AsFd, name: &str) -> Option<std::fs::File> {
    let fd = nix::fcntl::openat(
        dir,
        name,
        OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .ok()?;
    let f = std::fs::File::from(fd);
    f.metadata().ok()?.is_file().then_some(f)
}

/// [`open_file`] read whole, at most `cap` bytes (more counts as unreadable).
fn read_at(dir: &impl AsFd, name: &str, cap: u64) -> Option<Vec<u8>> {
    use std::io::Read;
    let f = open_file(dir, name)?;
    let mut out = Vec::new();
    f.take(cap + 1).read_to_end(&mut out).ok()?;
    (out.len() as u64 <= cap).then_some(out)
}

/// A commit id: 40 or 64 lower-case hex digits.
fn object_id(s: &str) -> Option<String> {
    let s = s.trim();
    ((s.len() == 40 || s.len() == 64) && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| s.to_ascii_lowercase())
}

/// The plain `.git` directory of the folder: a real directory, not a
/// gitfile, a link or one that names another (`commondir`, `gitdir`).
fn git_dir(folder: &Folder) -> Result<OwnedFd, String> {
    let unreadable = "the folder's repository could not be read (its .git must be a plain repository \
         directory)";
    let git = sub(&folder.0, ".git")?.ok_or(unreadable)?;
    for redirect in ["commondir", "gitdir"] {
        if nix::sys::stat::fstatat(&git, redirect, AtFlags::AT_SYMLINK_NOFOLLOW).is_ok() {
            return Err(unreadable.into());
        }
    }
    Ok(git)
}

/// The commit `refs/heads/<branch>` names: the loose ref read component by
/// component through descriptors (a component that is a link refuses),
/// else its line in `packed-refs`.
fn resolve(git: &OwnedFd, branch: &str) -> Result<Option<String>, String> {
    let name = format!("refs/heads/{branch}");
    let parts: Vec<&str> = name.split('/').collect();
    let (last, dirs) = parts.split_last().expect("a ref has parts");
    let mut at: Option<OwnedFd> = None;
    let mut found = true;
    for d in dirs {
        let next = match &at {
            None => sub(git, d)?,
            Some(fd) => sub(fd, d)?,
        };
        match next {
            Some(fd) => at = Some(fd),
            None => {
                found = false;
                break;
            }
        }
    }
    if found && let Some(d) = &at {
        if nix::sys::stat::fstatat(d, *last, AtFlags::AT_SYMLINK_NOFOLLOW)
            .is_ok_and(|st| SFlag::from_bits_truncate(st.st_mode) & SFlag::S_IFMT == SFlag::S_IFLNK)
        {
            return Err(CHANGED.into());
        }
        if let Some(id) = read_at(d, last, 512)
            .and_then(|b| String::from_utf8(b).ok())
            .and_then(|t| object_id(&t))
        {
            return Ok(Some(id));
        }
    }
    Ok(read_at(git, "packed-refs", 1 << 20)
        .and_then(|b| String::from_utf8(b).ok())
        .and_then(|t| {
            t.lines()
                .filter(|l| !l.starts_with('#') && !l.starts_with('^'))
                .find_map(|l| match l.split_once(' ') {
                    Some((id, r)) if r == name => object_id(id),
                    _ => None,
                })
        }))
}

/// Read what `branch` (default: the branch `HEAD` names) points at in the
/// folder's repository, by descriptor from `folder`. Every refusal is a
/// plain sentence.
pub(crate) fn pin(folder: &Folder, branch: Option<&str>) -> Result<Pin, String> {
    let git = git_dir(folder)?;
    let branch = match branch {
        Some(b) => b.to_string(),
        None => {
            let head = read_at(&git, "HEAD", 8192)
                .and_then(|b| String::from_utf8(b).ok())
                .ok_or("the folder's repository has no readable HEAD")?;
            let head = head.trim_end_matches('\n');
            if object_id(head).is_some() {
                return Err(
                    "the folder's repository is not on a branch (a detached HEAD): \
                     name the branch to push"
                        .into(),
                );
            }
            match head
                .strip_prefix("ref: ")
                .and_then(|r| r.trim().strip_prefix("refs/heads/"))
            {
                Some(b) => b.to_string(),
                None => return Err("the folder's HEAD names no branch".into()),
            }
        }
    };
    if !valid_branch(&branch) {
        return Err(
            "the folder's current branch has a name push does not take; name \
             another branch"
                .into(),
        );
    }
    race("ref");
    match resolve(&git, &branch)? {
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
thread_local! {
    /// Tests: a step run at named points of the walk (`start`, `descend
    /// <name>`, `link <name>`, `ref`), where a racing agent would swap a
    /// directory for a link: the race made deterministic.
    static RACE: std::cell::RefCell<Option<RaceStep>> =
        std::cell::RefCell::new(None);
}

/// A test's step at a race point.
#[cfg(test)]
pub(crate) type RaceStep = Box<dyn FnMut(&str)>;

#[cfg(test)]
pub(crate) fn set_race(f: Option<RaceStep>) {
    RACE.with(|r| *r.borrow_mut() = f);
}

/// The race point `at` (tests only; nothing otherwise).
#[allow(unused_variables)]
fn race(at: &str) {
    #[cfg(test)]
    RACE.with(|r| {
        if let Some(f) = r.borrow_mut().as_mut() {
            f(at)
        }
    });
}

#[cfg(test)]
pub(crate) fn refuse_links(on: bool) {
    NO_LINKS.with(|n| n.set(on));
}

fn link(src: &impl AsFd, name: &str, dst: &impl AsFd) -> nix::Result<()> {
    #[cfg(test)]
    if NO_LINKS.with(|n| n.get()) {
        return Err(Errno::EPERM);
    }
    // No flag: a link at `name` itself would be linked as a link, never
    // followed (and refused by the check after).
    nix::unistd::linkat(src, name, dst, name, AtFlags::empty())
}

const TOO_BIG: &str = "the folder's objects are larger than this node copies for a push \
     ([limits] clone_max_mb); nothing was pushed";

/// A file of the node's own repository, made new (0600).
fn create_in(dir: &impl AsFd, name: &str) -> Result<std::fs::File, String> {
    nix::fcntl::openat(
        dir,
        name,
        OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::from_bits_truncate(0o600),
    )
    .map(std::fs::File::from)
    .map_err(|e| format!("cannot write the node's copy: {e}"))
}

/// Make the node's repository for a push of the folder's repository in
/// `staging` (the node's staging folder): a bare repository of `format`
/// (`sha1`, `sha256`) holding links to (or bounded copies of) the folder's
/// loose objects and packs, all reached by descriptor from `folder` with
/// no link followed. Blocking; run it off the async threads.
pub(crate) fn stage(
    staging: &Path,
    folder: &Folder,
    format: &str,
    b: &Bounds,
) -> Result<Staged, String> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    let started = Instant::now();
    race("start");
    let git = git_dir(folder)?;
    let objects =
        sub(&git, "objects")?.ok_or("the folder's object store (.git/objects) is missing")?;
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
    let out_objects = nix::fcntl::open(&repo.join("objects"), dir_flags(), Mode::empty())
        .map_err(|e| format!("cannot open the node's repository: {e}"))?;
    let hexlen = if format == "sha256" { 64 } else { 40 };
    let is_hex = |s: &str| {
        s.bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    };
    let take = |src: &OwnedFd, name: &str, dst: &OwnedFd, st: &mut Staged| -> Result<(), String> {
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
        // Gone since it was listed (a `git gc`): not needed, or the push
        // says which object is missing. Not a regular file: not an object.
        let Ok(before) = nix::sys::stat::fstatat(src, name, AtFlags::AT_SYMLINK_NOFOLLOW) else {
            return Ok(());
        };
        if SFlag::from_bits_truncate(before.st_mode) & SFlag::S_IFMT != SFlag::S_IFREG {
            return Ok(());
        }
        let was = (before.st_dev, before.st_ino);
        race(&format!("link {name}"));
        match link(src, name, dst) {
            Ok(()) => {
                // What was linked must be the file that was checked: read
                // through the node's side, a regular file of that very
                // device and inode. Anything else refuses the push.
                let ok = nix::sys::stat::fstatat(dst, name, AtFlags::AT_SYMLINK_NOFOLLOW)
                    .is_ok_and(|a| {
                        guard_post_link(
                            SFlag::from_bits_truncate(a.st_mode) & SFlag::S_IFMT == SFlag::S_IFREG
                                && (a.st_dev, a.st_ino) == was,
                        )
                    });
                if !ok {
                    return Err(CHANGED.into());
                }
                st.linked += 1;
            }
            Err(_) => {
                let f = open_file(src, name).ok_or(CHANGED)?;
                let md = f.metadata().map_err(|e| e.to_string())?;
                if (md.dev(), md.ino()) != was {
                    return Err(CHANGED.into());
                }
                let room = b.max_copy.saturating_sub(st.copied_bytes);
                if md.len() > room {
                    return Err(TOO_BIG.into());
                }
                let mut out = create_in(dst, name)?;
                let n = std::io::copy(
                    &mut std::io::Read::take(f, room.saturating_add(1)),
                    &mut out,
                )
                .map_err(|e| format!("cannot copy an object file: {e}"))?;
                if n > room {
                    return Err(TOO_BIG.into());
                }
                st.copied += 1;
                st.copied_bytes += n;
            }
        }
        Ok(())
    };
    for name in names(&objects)? {
        race(&format!("descend {name}"));
        let loose = name.len() == 2 && is_hex(&name);
        if !loose && name != "pack" {
            // Anything else (`info/`: alternates, http-alternates, packs,
            // commit-graph; `incoming-*`) is never taken.
            continue;
        }
        let Some(src) = sub(&objects, &name)? else {
            continue;
        };
        let dst_name = if loose { name.as_str() } else { "pack" };
        let mut dst: Option<OwnedFd> = None;
        for f in names(&src)? {
            let wanted = if loose {
                f.len() == hexlen - 2 && is_hex(&f)
            } else {
                f.strip_suffix(".pack")
                    .or_else(|| f.strip_suffix(".idx"))
                    .and_then(|stem| stem.strip_prefix("pack-"))
                    .is_some_and(|id| (id.len() == 40 || id.len() == 64) && is_hex(id))
            };
            if !wanted {
                continue;
            }
            if dst.is_none() {
                if loose {
                    nix::sys::stat::mkdirat(
                        &out_objects,
                        dst_name,
                        Mode::from_bits_truncate(0o700),
                    )
                    .map_err(|e| format!("cannot make the node's repository: {e}"))?;
                }
                dst =
                    Some(sub(&out_objects, dst_name)?.ok_or("cannot open the node's repository")?);
            }
            take(&src, &f, dst.as_ref().expect("made"), &mut staged)?;
        }
    }
    Ok(staged)
}

/// The post-link check's verdict (tests may turn the check off to prove a
/// test needs it: `skip_post_link_check`).
fn guard_post_link(ok: bool) -> bool {
    #[cfg(test)]
    if SKIP_POST_LINK.with(|s| s.get()) {
        return true;
    }
    ok
}

#[cfg(test)]
thread_local! {
    static SKIP_POST_LINK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Tests: run without the post-link check (a mutation the tests must see).
#[cfg(test)]
pub(crate) fn skip_post_link_check(on: bool) {
    SKIP_POST_LINK.with(|s| s.set(on));
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

/// The subject shown when the commit's bytes do not hash to its id.
pub const UNVERIFIED: &str = "(subject not shown: the commit could not be verified)";

/// One git command in `staged` with `input` on its standard input, within
/// [`READ_TIME`]; its standard output (at most [`COMMIT_READ`] + 1 bytes
/// kept), or `None` when it failed.
async fn capture(
    staged: &Staged,
    jail: Option<&harness_tools::sandbox::Sandbox>,
    args: &[&str],
    input: Option<Vec<u8>>,
) -> Option<Vec<u8>> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut c = git_in(staged, jail);
    c.args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .process_group(0);
    let mut child = c.spawn().ok()?;
    let group = crate::git::Group(child.id());
    let run = async {
        if let (Some(data), Some(mut w)) = (input, child.stdin.take()) {
            w.write_all(&data).await.ok()?;
        }
        let mut out = Vec::new();
        child
            .stdout
            .take()?
            .take(COMMIT_READ as u64 + 1)
            .read_to_end(&mut out)
            .await
            .ok()?;
        child.wait().await.ok()?.success().then_some(out)
    };
    let out = tokio::time::timeout(READ_TIME, run).await.ok().flatten();
    drop(group);
    out
}

/// The pinned commit's subject from the node's repository: `Err` when it is
/// not a commit there; [`UNVERIFIED`] when the bytes git returned do not
/// hash to `commit` (a linked object rewritten in place: `cat-file` does
/// not check) or are too long to check; otherwise its first line (agent
/// text: one line, cleaned, at most [`SUBJECT_MAX`] characters), bound to
/// the pinned id by that hash.
pub(crate) async fn subject(
    staged: &Staged,
    commit: &str,
    jail: Option<&harness_tools::sandbox::Sandbox>,
) -> Result<String, String> {
    let Some(out) = capture(staged, jail, &["cat-file", "commit", commit], None).await else {
        return Err(format!(
            "the commit {} is not in the folder's repository (or not a commit)",
            &commit[..12.min(commit.len())]
        ));
    };
    if out.len() > COMMIT_READ {
        return Ok(UNVERIFIED.into());
    }
    let hashed = capture(
        staged,
        jail,
        &["hash-object", "-t", "commit", "--stdin"],
        Some(out.clone()),
    )
    .await
    .and_then(|h| String::from_utf8(h).ok());
    if hashed.as_deref().map(str::trim) != Some(commit) {
        return Ok(UNVERIFIED.into());
    }
    let text = String::from_utf8_lossy(&out).to_string();
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
    let mut child = spawn_without_core(&mut c)?;
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

/// Start `c` with a core-file limit of 0, which git and every process it
/// starts (`git-remote-https`, `send-pack`, the helper's shell) inherit: the
/// token is in their memory, and the host's core pattern often hands a dump
/// to a handler outside every sandbox (review of PR 125, F7). Without
/// `unsafe` there is no hook in the child between `fork` and `exec`, so the
/// node lowers its own soft limit for the instant of the spawn and puts it
/// back (a spawn of another thread in that instant gets 0 too, which only
/// takes a core dump away). The hard limit is never touched.
fn spawn_without_core(c: &mut tokio::process::Command) -> Result<tokio::process::Child, String> {
    use nix::sys::resource::{Resource, getrlimit, setrlimit};
    static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _one = ONE.lock().unwrap_or_else(|p| p.into_inner());
    let (soft, hard) = getrlimit(Resource::RLIMIT_CORE)
        .map_err(|e| format!("cannot read the core-dump limit; nothing was pushed: {e}"))?;
    setrlimit(Resource::RLIMIT_CORE, 0, hard)
        .map_err(|e| format!("cannot turn core dumps off; nothing was pushed: {e}"))?;
    let child = c.spawn().map_err(|e| format!("cannot start git: {e}"));
    let _ = setrlimit(Resource::RLIMIT_CORE, soft, hard);
    child
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
