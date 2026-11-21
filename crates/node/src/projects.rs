//! A new project's folder (ADOPTION_PLAN X.26, PLAN.md T77).
//!
//! The master sends a display name and nothing else (`project.create`).
//! The node makes the folder's name from it, `<slug>-<4 random
//! characters>`, inside the one folder its node.toml names
//! (`[projects] dir`, checked at start: inside `workspace_roots`, clear of
//! the data directory, its parent existing, on the data directory's file
//! system), and answers with the folder's path.
//!
//! **git never runs where anything but the node can write.** A session
//! that can write in the projects folder could otherwise put a `.git`
//! file (`gitdir: <another repository>`) into the new folder between its
//! making and `git init`, and the node's git would initialise and commit
//! into that other repository (PR 143 review). So the repository is built
//! in `<data>/projects-staging/<random>` (mode 0700, protected: no session
//! has it in view), checked there (a real `.git` folder, no `commondir`,
//! one commit by the node's identity, nothing else in the folder), and
//! only then moved into the projects folder under its final name with
//! `renameat2(RENAME_NOREPLACE)`, which fails when anything (a folder, a
//! file, a link) holds the name: a taken name gets new characters. After
//! the move the entry is checked again from the open projects folder.
//!
//! Nothing here resolves a path the master chose, and nothing follows a
//! link placed in the way: the projects folder's parent is opened without
//! following a link at its name and compared (device and inode) with what
//! was recorded at start; the projects folder is made (only its last
//! component) and opened relative to it, also without following, and
//! compared with its identity at start when it existed then.

use std::collections::VecDeque;
use std::ffi::OsStr;
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use harness_core::folder::Identity;
use nix::errno::Errno;
use nix::fcntl::{AtFlags, OFlag};
use nix::sys::stat::{Mode, SFlag};

use crate::policy::NodePolicy;

/// The four characters after the slug come from this alphabet: lower-case
/// ASCII letters and digits, as the slug itself (36^4, about 1.7 million
/// names per slug). Not a secret: they only keep names apart.
pub const SUFFIX_ALPHABET: &[u8; 36] = b"abcdefghijklmnopqrstuvwxyz0123456789";
/// How many characters the suffix has.
pub const SUFFIX_LEN: usize = 4;
/// The longest slug, in characters (all ASCII).
pub const SLUG_MAX: usize = 48;
/// The slug of a name with no letter or digit in it.
pub const EMPTY_SLUG: &str = "project";
/// New suffixes tried when the name is taken, before giving up.
pub(crate) const TRIES: usize = 8;

/// The folder name's first part, from a display name: ASCII letters
/// (lower-cased) and digits kept, every other run of characters (spaces,
/// dots, slashes, control characters, NUL, any non-ASCII character,
/// whatever it normalises to) one `-`, none at either end, at most
/// [`SLUG_MAX`] characters; [`EMPTY_SLUG`] when nothing is left. So no
/// dot and no separator survive, and no name is `.`, `..` or hidden.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    let mut gap = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            let dash = gap && !out.is_empty();
            if out.len() + usize::from(dash) + 1 > SLUG_MAX {
                break;
            }
            if dash {
                out.push('-');
            }
            gap = false;
            out.push(c.to_ascii_lowercase());
        } else {
            gap = true;
        }
    }
    let out = out.trim_end_matches('-');
    if out.is_empty() {
        EMPTY_SLUG.to_string()
    } else {
        out.to_string()
    }
}

/// [`SUFFIX_LEN`] characters of [`SUFFIX_ALPHABET`], from the operating
/// system's randomness, without bias (bytes at or above 252 are drawn
/// again).
pub fn suffix() -> String {
    let mut out = String::with_capacity(SUFFIX_LEN);
    while out.len() < SUFFIX_LEN {
        let mut b = [0u8; 8];
        getrandom::fill(&mut b).expect("OS randomness");
        for x in b {
            if out.len() < SUFFIX_LEN && x < 252 {
                out.push(SUFFIX_ALPHABET[usize::from(x) % 36] as char);
            }
        }
    }
    out
}

/// How many folders were made in the last hour, to bound a master that
/// asks in a loop (`[limits] projects_per_hour`). Only folders made count;
/// the count lives in memory (a restart starts it again).
#[derive(Default)]
pub(crate) struct Made(std::sync::Mutex<VecDeque<Instant>>);

impl Made {
    fn recent(&self) -> std::sync::MutexGuard<'_, VecDeque<Instant>> {
        let mut q = self.0.lock().unwrap_or_else(|p| p.into_inner());
        let now = Instant::now();
        while q
            .front()
            .is_some_and(|t| now.duration_since(*t) >= Duration::from_secs(3600))
        {
            q.pop_front();
        }
        q
    }

    /// Whether the hour's allowance has room, or why not.
    pub(crate) fn room(&self, per_hour: usize) -> Result<(), String> {
        if self.recent().len() >= per_hour {
            return Err(format!(
                "this node has made {per_hour} project folders in the last hour, its limit \
                 ([limits] projects_per_hour in its node.toml); try again later"
            ));
        }
        Ok(())
    }

    /// One folder made.
    pub(crate) fn record(&self) {
        self.recent().push_back(Instant::now());
    }

    /// Take a place in the hour's allowance now, under the lock that counts
    /// it, so copies that run at once cannot all pass [`Made::room`] before
    /// any records: the place is given back when the [`Slot`] is dropped
    /// unless [`Slot::keep`] says a folder was made.
    pub(crate) fn reserve(&self, per_hour: usize) -> Result<Slot<'_>, String> {
        let mut q = self.recent();
        if q.len() >= per_hour {
            return Err(format!(
                "this node has made {per_hour} project folders in the last hour, its limit \
                 ([limits] projects_per_hour in its node.toml); try again later"
            ));
        }
        let at = Instant::now();
        q.push_back(at);
        Ok(Slot {
            made: self,
            at,
            kept: false,
        })
    }
}

/// A place in the hour's allowance ([`Made::reserve`]).
pub(crate) struct Slot<'a> {
    made: &'a Made,
    at: Instant,
    kept: bool,
}

impl Slot<'_> {
    /// A folder was made: the place stays used.
    pub(crate) fn keep(mut self) {
        self.kept = true;
    }
}

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        if !self.kept {
            let mut q = self.made.recent();
            if let Some(i) = q.iter().position(|t| *t == self.at) {
                q.remove(i);
            }
        }
    }
}

/// Why a node makes no project folder at all.
pub const OFF: &str = "this node makes no project folders: its node.toml turns them off \
     ([projects] dir), or the folder it names is not usable (see the node's log)";

/// A display name the folder's name may be made from: at most
/// [`harness_protocol::MAX_PROJECT_NAME`] bytes, not blank.
pub(crate) fn check_name(name: &str) -> Result<(), String> {
    if name.len() > harness_protocol::MAX_PROJECT_NAME {
        return Err(format!(
            "a project's name is at most {} bytes",
            harness_protocol::MAX_PROJECT_NAME
        ));
    }
    if name.trim().is_empty() {
        return Err("a project needs a name".into());
    }
    Ok(())
}

/// Make a new project's folder for `name` (a display name), with the
/// node's `policy`; its absolute path. `made` bounds how many per hour.
pub(crate) async fn create(
    policy: &NodePolicy,
    made: &Made,
    name: &str,
    make_suffix: &(dyn Fn() -> String + Sync),
) -> Result<PathBuf, String> {
    let pp = &policy.projects;
    let (Some(dir), Some(_), Some(_)) = (pp.located.clone(), pp.parent_id, pp.staging.clone())
    else {
        return Err(OFF.into());
    };
    if policy.limits.projects_per_hour == 0 {
        return Err(OFF.into());
    }
    check_name(name)?;
    made.room(policy.limits.projects_per_hour)?;
    let base = slug(name);
    let names: Vec<String> = (0..TRIES)
        .map(|_| format!("{base}-{}", make_suffix()))
        .collect();
    // The projects folder first, then the build folder: a projects folder
    // swapped while the repository is built is still the one opened here.
    let p = policy.clone();
    let (top, build) = tokio::task::spawn_blocking(move || {
        let top = open_projects(&p, &dir)?;
        let build = Build::new(&p)?;
        Ok::<_, String>((top, build))
    })
    .await
    .map_err(|e| format!("making the folder: {e}"))??;
    let built = async {
        let at = build.proc_path();
        crate::git::init_project(&at).await?;
        crate::git::check_first_commit(&at).await?;
        build.check(true)
    }
    .await;
    if let Err(e) = built {
        tokio::task::spawn_blocking(move || build.discard())
            .await
            .ok();
        return Err(format!("making the project's repository failed: {e}"));
    }
    let p = policy.clone();
    let path = tokio::task::spawn_blocking(move || build.move_into(&p, &top, &names))
        .await
        .map_err(|e| format!("moving the folder: {e}"))??;
    made.record();
    Ok(path)
}

fn dir_flags() -> OFlag {
    OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC
}

fn id_of(st: &nix::sys::stat::FileStat) -> Identity {
    Identity {
        dev: st.st_dev,
        ino: st.st_ino,
    }
}

fn io(what: &str, p: &Path, e: Errno) -> String {
    format!("{what} {}: {}", p.display(), std::io::Error::from(e))
}

/// Open the projects folder `dir`: its parent without following a link at
/// its name, compared with the parent recorded at start; the folder made
/// there if missing (its last component only: nothing is made through a
/// link in a missing parent) and opened without following, compared with
/// the folder recorded at start when it existed then. The rules are
/// checked again on its path.
pub(crate) fn open_projects(policy: &NodePolicy, dir: &Path) -> Result<OwnedFd, String> {
    let pp = &policy.projects;
    let (Some(parent), Some(name)) = (dir.parent(), dir.file_name()) else {
        return Err(OFF.into());
    };
    let moved = || {
        format!(
            "the projects folder {} or its parent was moved or replaced since the node \
             started; restart the node to check it again",
            dir.display()
        )
    };
    let up = nix::fcntl::open(parent, dir_flags(), Mode::empty()).map_err(|_| moved())?;
    let st = nix::sys::stat::fstat(&up).map_err(|e| io("the projects folder", dir, e))?;
    if Some(id_of(&st)) != pp.parent_id {
        return Err(moved());
    }
    match nix::sys::stat::mkdirat(&up, name, Mode::from_bits_truncate(0o755)) {
        Ok(()) | Err(Errno::EEXIST) => {}
        Err(e) => return Err(io("cannot make the projects folder", dir, e)),
    }
    let top = nix::fcntl::openat(&up, name, dir_flags(), Mode::empty()).map_err(|_| moved())?;
    let st = nix::sys::stat::fstat(&top).map_err(|e| io("the projects folder", dir, e))?;
    if pp.dir_id.is_some_and(|want| want != id_of(&st)) {
        return Err(moved());
    }
    if let Some(why) = policy.projects_refusal(dir) {
        return Err(format!("the projects folder {}: {why}", dir.display()));
    }
    Ok(top)
}

/// A new project's folder while it is built: `<staging>/<name>`, made
/// 0700 by the node in its own data directory and held open.
pub(crate) struct Build {
    staging: OwnedFd,
    name: String,
    fd: OwnedFd,
    path: PathBuf,
    /// The folder was moved into the projects folder: nothing of it is
    /// left in staging to remove.
    moved: bool,
}

/// A build that is dropped (a failure, a cancelled request) removes its
/// folder at once; the sweep at the next start is the second line only.
impl Drop for Build {
    fn drop(&mut self) {
        if !self.moved {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

impl Build {
    pub(crate) fn new(policy: &NodePolicy) -> Result<Self, String> {
        let Some(at) = policy.projects.staging.clone() else {
            return Err(OFF.into());
        };
        let staging = nix::fcntl::open(&at, dir_flags(), Mode::empty())
            .map_err(|e| io("the node's staging folder", &at, e))?;
        let mut b = [0u8; 8];
        getrandom::fill(&mut b).expect("OS randomness");
        let name = format!("build-{}", hex::encode(b));
        nix::sys::stat::mkdirat(&staging, name.as_str(), Mode::from_bits_truncate(0o700))
            .map_err(|e| io("cannot make a folder in", &at, e))?;
        let fd = nix::fcntl::openat(&staging, name.as_str(), dir_flags(), Mode::empty())
            .map_err(|e| io("cannot open a folder in", &at, e))?;
        Ok(Self {
            path: at.join(&name),
            staging,
            name,
            fd,
            moved: false,
        })
    }

    /// The folder for a child process: exactly the one held open.
    pub(crate) fn proc_path(&self) -> PathBuf {
        PathBuf::from(format!("/proc/self/fd/{}", self.fd.as_raw_fd()))
    }

    /// The folder in the staging folder, for the node's own reads.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// `.git` is a real folder, not a git file, and names no other
    /// repository (`commondir`); with `only_git` (a new empty repository)
    /// the folder holds nothing else.
    pub(crate) fn check(&self, only_git: bool) -> Result<(), String> {
        if only_git {
            let entries: Vec<_> = std::fs::read_dir(self.proc_path())
                .map_err(|e| e.to_string())?
                .filter_map(|e| e.ok())
                .map(|e| e.file_name())
                .collect();
            if entries != [std::ffi::OsString::from(".git")] {
                return Err(format!("the new folder holds {entries:?}"));
            }
        }
        let st = nix::sys::stat::fstatat(&self.fd, ".git", AtFlags::AT_SYMLINK_NOFOLLOW)
            .map_err(|e| format!(".git: {}", std::io::Error::from(e)))?;
        if SFlag::from_bits_truncate(st.st_mode) & SFlag::S_IFMT != SFlag::S_IFDIR {
            return Err(".git is not a folder".into());
        }
        if nix::sys::stat::fstatat(&self.fd, ".git/commondir", AtFlags::AT_SYMLINK_NOFOLLOW).is_ok()
        {
            return Err(".git names another repository (commondir)".into());
        }
        Ok(())
    }

    /// Remove the build folder (in the node's own staging folder: nothing
    /// else writes there).
    pub(crate) fn discard(self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }

    /// Move the built folder into the projects folder `top` under the
    /// first of `names` that nothing holds, never replacing what is there;
    /// then check, from `top`, that the name holds this very folder with
    /// its real `.git`, and that a session may run at its path. Its path.
    pub(crate) fn move_into(
        mut self,
        policy: &NodePolicy,
        top: &OwnedFd,
        names: &[String],
    ) -> Result<PathBuf, String> {
        let dir = policy.projects.located.clone().ok_or(OFF)?;
        // The person's folder gets ordinary permissions, as `mkdir` gives.
        let _ = nix::sys::stat::fchmod(&self.fd, Mode::from_bits_truncate(0o755));
        let mut placed = None;
        for n in names {
            match nix::fcntl::renameat2(
                &self.staging,
                self.name.as_str(),
                top,
                n.as_str(),
                nix::fcntl::RenameFlags::RENAME_NOREPLACE,
            ) {
                Ok(()) => {
                    placed = Some(n.clone());
                    break;
                }
                // Taken, by anything: a folder, a file, a link. Never
                // replaced, never followed.
                Err(Errno::EEXIST) => continue,
                Err(Errno::EXDEV) => {
                    self.discard();
                    return Err(format!(
                        "the projects folder {} is not on the node's data directory's file \
                         system any more; restart the node",
                        dir.display()
                    ));
                }
                Err(e) => {
                    self.discard();
                    return Err(io("cannot move the new folder into", &dir, e));
                }
            }
        }
        if placed.is_some() {
            self.moved = true;
        }
        let Some(n) = placed else {
            self.discard();
            return Err(format!(
                "every name tried in {} was taken; try again",
                dir.display()
            ));
        };
        let path = dir.join(&n);
        let checked = (|| {
            let made = nix::sys::stat::fstat(&self.fd).map_err(|e| e.to_string())?;
            let now = nix::sys::stat::fstatat(top, n.as_str(), AtFlags::AT_SYMLINK_NOFOLLOW)
                .map_err(|e| e.to_string())?;
            if id_of(&now) != id_of(&made) {
                return Err(format!(
                    "{} was replaced as it was moved in",
                    path.display()
                ));
            }
            let git = nix::sys::stat::fstatat(&self.fd, ".git", AtFlags::AT_SYMLINK_NOFOLLOW)
                .map_err(|e| format!(".git: {}", std::io::Error::from(e)))?;
            if SFlag::from_bits_truncate(git.st_mode) & SFlag::S_IFMT != SFlag::S_IFDIR {
                return Err(".git is not a folder".into());
            }
            let real = policy.check_cwd(&path.to_string_lossy())?;
            if real != path || Identity::of(&real).ok() != Some(id_of(&made)) {
                return Err(format!(
                    "{} does not lead to the folder made (the projects folder was moved or \
                     replaced)",
                    path.display()
                ));
            }
            Ok(())
        })();
        if let Err(e) = checked {
            remove_made(top, &self.fd, OsStr::new(&n));
            return Err(format!("making the project's folder failed: {e}"));
        }
        Ok(path)
    }
}

/// The marker file of a fork's copy between its move into the projects
/// folder and the writing of its chat: `<staging>/pending-fork-<build>`
/// holds the folder's device and inode. A kill in that window leaves the
/// marker (and a complete, unused folder); the next start removes the
/// folder it names ([`sweep_pending`]). Dropping this removes the marker.
#[derive(Debug)]
pub(crate) struct Pending {
    file: PathBuf,
}

impl Drop for Pending {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.file);
    }
}

/// What starts a marker's file name.
pub(crate) const PENDING_PREFIX: &str = "pending-fork-";

impl Build {
    /// Write the marker for this folder, before it is moved in.
    pub(crate) fn mark_pending(&self) -> Result<Pending, String> {
        let st = nix::sys::stat::fstat(&self.fd).map_err(|e| e.to_string())?;
        let file = self
            .path
            .with_file_name(format!("{PENDING_PREFIX}{}", self.name));
        std::fs::write(&file, format!("{} {}\n", st.st_dev, st.st_ino))
            .map_err(|e| format!("cannot write {}: {e}", file.display()))?;
        Ok(Pending { file })
    }
}

/// At node start, before the staging folder is emptied: remove the folder a
/// leftover marker names, if it is still a folder in the projects folder
/// `projects` (its direct children, compared by device and inode, a link
/// never matches), and the marker. A fork's copy whose chat was never
/// written is complete but unused; the chat's own folder is never this: its
/// marker is removed as soon as its chat is stored. Returns how many folders
/// were removed.
pub(crate) fn sweep_pending(staging: &Path, projects: &Path) -> usize {
    use std::os::unix::fs::MetadataExt;
    let Ok(entries) = std::fs::read_dir(staging) else {
        return 0;
    };
    let mut removed = 0;
    for e in entries.flatten() {
        let name = e.file_name();
        if !name.to_string_lossy().starts_with(PENDING_PREFIX)
            || !e.file_type().is_ok_and(|t| t.is_file())
        {
            continue;
        }
        let text = std::fs::File::open(e.path())
            .ok()
            .map(|f| {
                use std::io::Read;
                let mut t = String::new();
                let _ = f.take(64).read_to_string(&mut t);
                t
            })
            .unwrap_or_default();
        let _ = std::fs::remove_file(e.path());
        let mut it = text.split_whitespace().map(|n| n.parse::<u64>().ok());
        let (Some(Some(dev)), Some(Some(ino))) = (it.next(), it.next()) else {
            continue;
        };
        let Ok(children) = std::fs::read_dir(projects) else {
            continue;
        };
        for c in children.flatten() {
            let Ok(md) = std::fs::symlink_metadata(c.path()) else {
                continue;
            };
            if md.is_dir() && (md.dev(), md.ino()) == (dev, ino) {
                tracing::warn!(
                    folder = ?c.path(),
                    "removing a fork's copy whose chat was never written (the node was stopped)"
                );
                if std::fs::remove_dir_all(c.path()).is_ok() {
                    removed += 1;
                }
            }
        }
    }
    removed
}

/// Remove the folder this call moved in, `name` in `top`: only when it is
/// still the one made and holds nothing but `.git` (removed with it);
/// otherwise it is left as it is and logged.
fn remove_made(top: &OwnedFd, child: &OwnedFd, name: &OsStr) {
    let inside = PathBuf::from(format!("/proc/self/fd/{}", child.as_raw_fd()));
    let entries: Vec<_> = match std::fs::read_dir(&inside) {
        Ok(r) => r.filter_map(|e| e.ok()).map(|e| e.file_name()).collect(),
        Err(_) => return,
    };
    if entries.iter().any(|e| e != ".git") {
        tracing::warn!(
            folder = ?name,
            "a new project folder that failed its checks holds more than .git; left in place"
        );
        return;
    }
    if !entries.is_empty() {
        // `remove_dir_all` opens each level without following links.
        let _ = std::fs::remove_dir_all(inside.join(".git"));
    }
    let (Ok(now), Ok(made)) = (
        nix::sys::stat::fstatat(top, name, AtFlags::AT_SYMLINK_NOFOLLOW),
        nix::sys::stat::fstat(child),
    ) else {
        return;
    };
    if id_of(&now) == id_of(&made) {
        let _ = nix::unistd::unlinkat(top, name, nix::unistd::UnlinkatFlags::RemoveDir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A place is taken under the lock before the work starts, so two
    /// requests at once cannot both pass a limit of one; a dropped slot gives
    /// it back, a kept one stays used.
    #[test]
    fn a_reserved_place_is_given_back_unless_kept() {
        let made = Made::default();
        let a = made.reserve(1).unwrap();
        assert!(made.reserve(1).is_err(), "the limit was passed by a second");
        drop(a);
        let b = made.reserve(1).unwrap();
        b.keep();
        assert!(made.reserve(1).is_err());
        assert!(made.room(1).is_err());
        assert!(made.reserve(2).is_ok());
    }

    /// The sweep removes the folder a marker names, by device and inode, in
    /// the projects folder's direct children: not a link to it, not another
    /// folder, not what a malformed marker says.
    #[test]
    fn the_sweep_removes_only_the_folder_a_marker_names() {
        use std::os::unix::fs::MetadataExt;
        let d = tempfile::tempdir().unwrap();
        let top = d.path().canonicalize().unwrap();
        let (staging, projects) = (top.join("staging"), top.join("projects"));
        for p in [
            &staging,
            &projects,
            &projects.join("a"),
            &projects.join("b"),
        ] {
            std::fs::create_dir_all(p).unwrap();
        }
        std::fs::write(projects.join("a/f"), "x").unwrap();
        std::os::unix::fs::symlink(projects.join("b"), projects.join("link-to-b")).unwrap();
        let id = |p: &Path| {
            let m = std::fs::symlink_metadata(p).unwrap();
            format!("{} {}\n", m.dev(), m.ino())
        };
        std::fs::write(staging.join("pending-fork-1"), id(&projects.join("a"))).unwrap();
        std::fs::write(staging.join("pending-fork-2"), "garbage").unwrap();
        std::fs::write(staging.join("pending-fork-3"), "1 2\n").unwrap();
        std::fs::write(staging.join("other"), id(&projects.join("b"))).unwrap();
        assert_eq!(sweep_pending(&staging, &projects), 1);
        assert!(!projects.join("a").exists());
        assert!(projects.join("b").is_dir() && projects.join("link-to-b").exists());
        assert!(
            staging.join("other").is_file(),
            "a file that is no marker was touched"
        );
        assert!(!staging.join("pending-fork-1").exists());
        assert!(!staging.join("pending-fork-2").exists());
    }

    /// A build folder that is dropped (a failed or cancelled request) is
    /// removed at once; one that was moved in is not touched.
    #[test]
    fn a_dropped_build_removes_its_folder_at_once() {
        let d = tempfile::tempdir().unwrap();
        let data = d.path().canonicalize().unwrap().join("data");
        let ws = d.path().canonicalize().unwrap().join("ws");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&ws).unwrap();
        let mut policy = NodePolicy {
            workspace_roots: vec![ws.clone()],
            ..Default::default()
        };
        policy.projects.dir = Some(ws.join("projects"));
        policy.protect(&data);
        policy.locate_projects(&data).unwrap();
        let staging = data.join("projects-staging");
        let b = Build::new(&policy).unwrap();
        let at = b.path().to_path_buf();
        std::fs::write(at.join("part"), "x").unwrap();
        assert!(at.is_dir());
        drop(b);
        assert!(
            !at.exists(),
            "left in staging: {:?}",
            std::fs::read_dir(&staging).map(|d| d.count())
        );
    }
    use crate::policy::ProjectsPolicy;

    #[test]
    fn slugs_keep_ascii_letters_and_digits_only() {
        for (name, want) in [
            ("Project Name Example", "project-name-example"),
            ("../x", "x"),
            ("..", EMPTY_SLUG),
            ("/etc", "etc"),
            ("/etc/passwd", "etc-passwd"),
            ("....///\\\\", EMPTY_SLUG),
            ("", EMPTY_SLUG),
            ("a\0b\nc\x1bd", "a-b-c-d"),
            // Fullwidth solidus, division slash, one dot leader, a
            // fullwidth full stop: each would normalise (NFKC) to `/` or
            // `.`; none is ASCII, so each is a gap.
            ("a\u{FF0F}b\u{2215}c\u{2024}d\u{FF0E}e", "a-b-c-d-e"),
            ("Café Ünïcode", "caf-n-code"),
            ("  --Trim--  ", "trim"),
            (".hidden", "hidden"),
        ] {
            assert_eq!(slug(name), want, "{name:?}");
        }
        let long = "x".repeat(10_000);
        assert_eq!(slug(&long).len(), SLUG_MAX);
        // A cut never ends on a dash.
        let s = slug(&format!("{} y", "a".repeat(SLUG_MAX - 1)));
        assert!(!s.ends_with('-') && s.len() <= SLUG_MAX, "{s}");
        for s in [slug("../x"), slug("..")] {
            assert!(!s.contains(['.', '/', '\\', '\0']));
        }
    }

    #[test]
    fn suffixes_come_from_the_alphabet() {
        for _ in 0..200 {
            let s = suffix();
            assert_eq!(s.len(), SUFFIX_LEN);
            assert!(s.bytes().all(|b| SUFFIX_ALPHABET.contains(&b)), "{s}");
        }
    }

    /// A node with its projects folder at `<tmp>/ws/projects`, the data
    /// directory at `<tmp>/data`, `workspace_roots = [<tmp>/ws]`.
    struct Rig {
        _tmp: tempfile::TempDir,
        ws: PathBuf,
        data: PathBuf,
        projects: PathBuf,
        policy: NodePolicy,
        made: Made,
    }

    fn rig() -> Rig {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let (ws, data) = (base.join("ws"), base.join("data"));
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        let projects = ws.join("projects");
        let mut policy = NodePolicy {
            workspace_roots: vec![ws.clone()],
            ..Default::default()
        };
        policy.projects.dir = Some(projects.clone());
        policy.protect(&data);
        assert_eq!(policy.locate_projects(&data).unwrap(), None);
        assert_eq!(policy.projects.located.as_deref(), Some(projects.as_path()));
        Rig {
            _tmp: tmp,
            ws,
            data,
            projects,
            policy,
            made: Made::default(),
        }
    }

    impl Rig {
        /// The data directory holds the staging folder only, and it is empty.
        fn data_holds_nothing_made(&self) {
            let names: Vec<_> = std::fs::read_dir(&self.data)
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect();
            assert_eq!(names, [std::ffi::OsString::from(ProjectsPolicy::STAGING)]);
            let staging = self.data.join(ProjectsPolicy::STAGING);
            assert_eq!(std::fs::read_dir(staging).unwrap().count(), 0);
        }
        async fn make(&self, name: &str) -> Result<PathBuf, String> {
            create(&self.policy, &self.made, name, &suffix).await
        }
        async fn make_with(&self, name: &str, next: &[&str]) -> Result<PathBuf, String> {
            let left =
                std::sync::Mutex::new(next.iter().map(|s| s.to_string()).collect::<VecDeque<_>>());
            let f = move || {
                left.lock()
                    .unwrap()
                    .pop_front()
                    .unwrap_or_else(|| "zzzz".into())
            };
            create(&self.policy, &self.made, name, &f).await
        }
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// The folder: `<slug>-<4 of the alphabet>` directly in the projects
    /// folder (made on first use), a repository on `main` with one empty
    /// commit by the node's identity, no hooks copied, and a session may
    /// run there.
    #[tokio::test]
    async fn a_project_folder_is_a_repository_with_one_commit() {
        let r = rig();
        assert!(!r.projects.exists());
        let p = r.make("Project Name Example").await.unwrap();
        assert_eq!(p.parent(), Some(r.projects.as_path()));
        let name = p.file_name().unwrap().to_str().unwrap();
        let tail = name.strip_prefix("project-name-example-").unwrap();
        assert_eq!(tail.len(), SUFFIX_LEN);
        assert!(tail.bytes().all(|b| SUFFIX_ALPHABET.contains(&b)));
        assert_eq!(git(&p, &["rev-list", "--count", "HEAD"]), "1");
        assert_eq!(git(&p, &["branch", "--show-current"]), "main");
        assert_eq!(
            git(&p, &["log", "-1", "--format=%an <%ae>|%s"]),
            format!("harness <harness@localhost>|{}", crate::git::FIRST_COMMIT)
        );
        assert_eq!(git(&p, &["ls-tree", "HEAD"]), "", "an empty commit");
        let hooks = p.join(".git/hooks");
        assert!(
            !hooks.exists() || std::fs::read_dir(&hooks).unwrap().next().is_none(),
            "no template hooks"
        );
        assert_eq!(r.policy.check_cwd(&p.to_string_lossy()).unwrap(), p);
        let entries: Vec<_> = std::fs::read_dir(&p)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(entries, vec![std::ffi::OsString::from(".git")]);
    }

    /// Hostile names: each makes a folder directly inside the projects
    /// folder, with a name of ASCII letters, digits and dashes; one longer
    /// than the protocol's bound is refused; nothing appears elsewhere.
    #[tokio::test]
    async fn hostile_names_stay_inside_the_projects_folder() {
        let mut r = rig();
        r.policy.limits.projects_per_hour = 100;
        for name in [
            "../x",
            "..",
            "/etc",
            ".",
            "....",
            "///",
            "a\0b",
            "a\nb\rc\x07d",
            "\u{FF0F}etc\u{FF0F}passwd",
            "\u{2024}\u{2024}\u{2215}x",
            "x/../../../../tmp/evil",
        ] {
            let p = r
                .make(name)
                .await
                .unwrap_or_else(|e| panic!("{name:?}: {e}"));
            assert_eq!(p.parent(), Some(r.projects.as_path()), "{name:?}");
            let n = p.file_name().unwrap().to_str().unwrap();
            assert!(
                n.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
                "{name:?} gave {n:?}"
            );
            assert!(!n.starts_with('-') && !n.starts_with('.'));
        }
        let long = "y".repeat(10_000);
        let e = r.make(&long).await.unwrap_err();
        assert!(e.contains("at most"), "{e}");
        let blank = r.make(" \t ").await.unwrap_err();
        assert!(blank.contains("needs a name"), "{blank}");
        // Nothing was made beside the projects folder or in the data
        // directory.
        let beside: Vec<_> = std::fs::read_dir(&r.ws)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(beside, vec![std::ffi::OsString::from("projects")]);
        r.data_holds_nothing_made();
    }

    /// The projects folder replaced by a link after start (to the data
    /// directory, to `/`): refused, nothing made through the link.
    #[tokio::test]
    async fn a_projects_folder_replaced_by_a_link_is_refused() {
        for to in ["data", "/"] {
            let r = rig();
            let target = if to == "data" {
                r.data.clone()
            } else {
                PathBuf::from("/")
            };
            std::os::unix::fs::symlink(&target, &r.projects).unwrap();
            let e = r.make("x").await.unwrap_err();
            assert!(e.contains("projects folder"), "{to}: {e}");
            if to == "data" {
                r.data_holds_nothing_made();
            }
        }
        // And a real folder moved away for another one: refused too.
        let r = rig();
        std::fs::create_dir(&r.projects).unwrap();
        r.make("first").await.unwrap();
        std::fs::rename(&r.projects, r.ws.join("old")).unwrap();
        std::os::unix::fs::symlink(r.ws.join("old"), &r.projects).unwrap();
        assert!(r.make("second").await.is_err());
    }

    /// Whatever holds the chosen name (a link to elsewhere, a folder, a
    /// file) is never reused or followed: the next suffix is taken.
    #[tokio::test]
    async fn a_taken_name_is_never_reused_or_followed() {
        let r = rig();
        std::fs::create_dir(&r.projects).unwrap();
        let elsewhere = r.ws.join("elsewhere");
        std::fs::create_dir(&elsewhere).unwrap();
        std::os::unix::fs::symlink(&elsewhere, r.projects.join("x-aaaa")).unwrap();
        std::fs::create_dir(r.projects.join("x-bbbb")).unwrap();
        std::fs::write(r.projects.join("x-cccc"), "file").unwrap();
        let p = r
            .make_with("x", &["aaaa", "bbbb", "cccc", "dddd"])
            .await
            .unwrap();
        assert_eq!(p, r.projects.join("x-dddd"));
        assert_eq!(
            std::fs::read_dir(&elsewhere).unwrap().count(),
            0,
            "the link was not followed"
        );
        assert_eq!(
            std::fs::read_dir(r.projects.join("x-bbbb"))
                .unwrap()
                .count(),
            0
        );
        assert_eq!(
            std::fs::read_to_string(r.projects.join("x-cccc")).unwrap(),
            "file"
        );
        // Every try taken: refused, nothing overwritten.
        let e = r.make_with("x", &["aaaa"; 8]).await.unwrap_err();
        assert!(e.contains("taken"), "{e}");
    }

    /// Two requests for the same name at once: two folders.
    #[tokio::test]
    async fn two_requests_at_once_make_two_folders() {
        let r = rig();
        let (a, b) = tokio::join!(r.make("Same"), r.make("Same"));
        let (a, b) = (a.unwrap(), b.unwrap());
        assert_ne!(a, b);
        for p in [&a, &b] {
            assert_eq!(git(p, &["rev-list", "--count", "HEAD"]), "1");
        }
        // Forced onto one name: the second gets the next suffix.
        let (a, b) = tokio::join!(
            r.make_with("Same", &["abcd", "efgh"]),
            r.make_with("Same", &["abcd", "efgh"])
        );
        let mut got = vec![a.unwrap(), b.unwrap()];
        got.sort();
        assert_eq!(
            got,
            vec![r.projects.join("same-abcd"), r.projects.join("same-efgh")]
        );
    }

    /// `[limits] projects_per_hour`, and a node without a projects folder.
    #[tokio::test]
    async fn the_cap_and_the_switch_hold() {
        let mut r = rig();
        r.policy.limits.projects_per_hour = 2;
        r.make("a").await.unwrap();
        r.make("b").await.unwrap();
        let e = r.make("c").await.unwrap_err();
        assert!(e.contains("projects_per_hour"), "{e}");
        let mut off = rig();
        off.policy.projects.dir = Some(PathBuf::new());
        off.policy.locate_projects(&off.data).unwrap();
        assert_eq!(off.make("a").await.unwrap_err(), OFF);
        assert!(!off.projects.exists());
        off.policy = NodePolicy::default();
        assert_eq!(off.make("a").await.unwrap_err(), OFF, "never located");
    }

    /// git failing after the folder was made: the folder goes again (only
    /// it, only with nothing but `.git` in it).
    #[tokio::test]
    async fn a_failed_repository_leaves_no_folder() {
        let r = rig();
        let bin = r.ws.parent().unwrap().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let fake = bin.join("git");
        // `init` runs for real; `commit` fails.
        crate::test_script::script(
            &fake,
            "#!/bin/sh\nfor a in \"$@\"; do [ \"$a\" = commit ] && exit 1; done\nexec git \"$@\"\n",
        );
        crate::git::use_test_git(Some(fake));
        let res = r.make("Broken").await;
        crate::git::use_test_git(None);
        let e = res.unwrap_err();
        assert!(e.contains("repository failed"), "{e}");
        assert_eq!(
            std::fs::read_dir(&r.projects).unwrap().count(),
            0,
            "nothing moved in"
        );
        r.data_holds_nothing_made();
    }

    #[test]
    fn the_projects_folder_is_checked_at_start() {
        let r = rig();
        let at = |dir: Option<PathBuf>| {
            let mut p = r.policy.clone();
            p.projects.dir = dir;
            p.locate_projects(&r.data)
                .map(|w| (w, p.projects.located.clone()))
        };
        // Named, outside the roots, in or over the data directory, with
        // `..`: the node does not start.
        let outside = r.ws.parent().unwrap().join("outside");
        assert!(at(Some(outside)).unwrap_err().contains("workspace_roots"));
        assert!(at(Some(r.data.join("p"))).unwrap_err().contains("data"));
        let mut over = r.policy.clone();
        over.workspace_roots = vec![r.ws.parent().unwrap().to_path_buf()];
        over.projects.dir = Some(r.ws.parent().unwrap().to_path_buf());
        assert!(over.locate_projects(&r.data).unwrap_err().contains("data"));
        assert!(at(Some(r.ws.join("a/../b"))).unwrap_err().contains(".."));
        // Off.
        assert_eq!(at(Some(PathBuf::new())).unwrap(), (None, None));
        // The default, outside the roots: off with a reason, the node starts.
        let mut d = r.policy.clone();
        d.projects.dir = None;
        d.workspace_roots = vec![r.ws.clone()];
        if dirs::home_dir().is_some_and(|h| !h.starts_with(&r.ws)) {
            let (w, located) = d
                .locate_projects(&r.data)
                .map(|w| (w, d.projects.located.clone()))
                .unwrap();
            assert!(w.unwrap().contains("off"));
            assert_eq!(located, None);
        }
    }

    /// A git program for this test thread: `commit` first runs `before`
    /// (a shell snippet, in the folder git was started in), then the real
    /// git does everything.
    fn fake_git(r: &Rig, before: &str) -> PathBuf {
        let bin = r.ws.parent().unwrap().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let fake = bin.join("git");
        crate::test_script::script(
            &fake,
            &format!(
                "#!/bin/sh\nfor a in \"$@\"; do [ \"$a\" = commit ] && {{ {before}; }}; done\nexec git \"$@\"\n"
            ),
        );
        fake
    }

    /// A repository outside the workspace roots with one commit.
    fn victim(r: &Rig) -> PathBuf {
        let v = r.ws.parent().unwrap().join("victim");
        std::fs::create_dir_all(&v).unwrap();
        let g = |args: &[&str]| {
            let ok = std::process::Command::new("git")
                .arg("-C")
                .arg(&v)
                .args(args)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_AUTHOR_NAME", "v")
                .env("GIT_AUTHOR_EMAIL", "v@v")
                .env("GIT_COMMITTER_NAME", "v")
                .env("GIT_COMMITTER_EMAIL", "v@v")
                .status()
                .unwrap()
                .success();
            assert!(ok);
        };
        g(&["init", "-q", "-b", "trunk"]);
        g(&["commit", "-q", "--allow-empty", "-m", "victim"]);
        v
    }

    /// PR 143 review, turned around: an adversary that can write in the
    /// projects folder (a sandboxed chat opened on it or on a folder that
    /// holds it) polls it and writes `.git` = `gitdir: <victim>/.git` into
    /// every new entry the moment it appears. Before the fix it won 20 of
    /// 20 and the node's git committed into the victim. Now git runs in the
    /// node's staging folder and a folder arrives in the projects folder
    /// with its real `.git` already in place: 200 of 200, the victim keeps
    /// its one commit, every folder made has a `.git` folder of its own.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_gitfile_planted_in_a_new_folder_never_redirects_the_nodes_git() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        let mut r = rig();
        r.policy.limits.projects_per_hour = 200;
        std::fs::create_dir_all(&r.projects).unwrap();
        let victim = victim(&r);
        let stop = Arc::new(AtomicBool::new(false));
        let (s2, dir, vgit) = (stop.clone(), r.projects.clone(), victim.join(".git"));
        let attacker = std::thread::spawn(move || {
            let mut seen = std::collections::HashSet::new();
            let mut planted = 0usize;
            while !s2.load(Ordering::Relaxed) {
                if let Ok(rd) = std::fs::read_dir(&dir) {
                    for e in rd.flatten() {
                        if seen.insert(e.file_name()) {
                            let g = dir.join(e.file_name()).join(".git");
                            if std::fs::write(&g, format!("gitdir: {}\n", vgit.display())).is_ok() {
                                planted += 1;
                            }
                        }
                    }
                }
            }
            planted
        });
        let mut made = Vec::new();
        for i in 0..200 {
            made.push(r.make(&format!("Race {i}")).await.unwrap());
        }
        stop.store(true, Ordering::Relaxed);
        let planted = attacker.join().unwrap();
        assert_eq!(planted, 0, "a gitfile was written into a new folder");
        assert_eq!(git(&victim, &["rev-list", "--count", "HEAD"]), "1");
        for p in &made {
            assert!(std::fs::symlink_metadata(p.join(".git")).unwrap().is_dir());
            assert_eq!(git(p, &["rev-list", "--count", "HEAD"]), "1");
        }
        r.data_holds_nothing_made();
    }

    /// Planted at the final names while the repository is built (the
    /// commit step plants them): a link to another repository, a folder
    /// with a repository of its own, a gitfile. None is replaced, followed
    /// or committed into; the folder lands under the next free name.
    #[tokio::test]
    async fn plants_at_the_final_name_during_the_build_are_never_replaced() {
        let r = rig();
        std::fs::create_dir_all(&r.projects).unwrap();
        let victim = victim(&r);
        let p = r.projects.display().to_string();
        let v = victim.display().to_string();
        let fake = fake_git(
            &r,
            &format!(
                "ln -s {v} {p}/x-aaaa; mkdir {p}/x-bbbb && git -C {p}/x-bbbb init -q; \
                 printf 'gitdir: {v}/.git\\n' > {p}/x-cccc"
            ),
        );
        crate::git::use_test_git(Some(fake));
        let res = r.make_with("x", &["aaaa", "bbbb", "cccc", "dddd"]).await;
        crate::git::use_test_git(None);
        assert_eq!(res.unwrap(), r.projects.join("x-dddd"));
        assert!(
            std::fs::symlink_metadata(r.projects.join("x-aaaa"))
                .unwrap()
                .is_symlink()
        );
        assert_eq!(git(&victim, &["rev-list", "--count", "HEAD"]), "1");
        let own = std::process::Command::new("git")
            .arg("-C")
            .arg(r.projects.join("x-bbbb"))
            .args(["rev-list", "--count", "HEAD"])
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(!own.status.success(), "the planted repository got a commit");
        assert!(
            std::fs::read_to_string(r.projects.join("x-cccc"))
                .unwrap()
                .starts_with("gitdir:"),
            "the gitfile was replaced"
        );
        assert_eq!(
            git(&r.projects.join("x-dddd"), &["rev-list", "--count", "HEAD"]),
            "1"
        );
        r.data_holds_nothing_made();
    }

    /// The projects folder itself swapped while the repository is built
    /// (moved away, a link to another folder put at its name): the folder
    /// is moved into the folder opened before (wherever it went now), its
    /// path no longer leads to it, so it is removed again and the request
    /// refused; nothing reaches the link's target.
    #[tokio::test]
    async fn a_projects_folder_swapped_during_the_build_gets_nothing() {
        let r = rig();
        std::fs::create_dir_all(&r.projects).unwrap();
        let elsewhere = r.ws.join("elsewhere");
        let old = r.ws.join("old");
        std::fs::create_dir(&elsewhere).unwrap();
        let fake = fake_git(
            &r,
            &format!(
                "mv {p} {o} && ln -s {e} {p}",
                p = r.projects.display(),
                o = old.display(),
                e = elsewhere.display()
            ),
        );
        crate::git::use_test_git(Some(fake));
        let res = r.make("Swap").await;
        crate::git::use_test_git(None);
        let e = res.unwrap_err();
        assert!(e.contains("making the project's folder failed"), "{e}");
        assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0);
        assert_eq!(std::fs::read_dir(&old).unwrap().count(), 0, "removed again");
        r.data_holds_nothing_made();
    }

    /// PR 143 review, turned around: missing folders on the way to the
    /// projects folder are never made, so a link planted at one is never
    /// followed. A missing parent stops the node at start (named) or turns
    /// the default off; a parent replaced by a link after start is refused.
    #[tokio::test]
    async fn missing_parents_are_never_made_through_a_planted_link() {
        let r = rig();
        let outside = r.ws.parent().unwrap().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        // At start: a link at the first missing component.
        std::os::unix::fs::symlink(&outside, r.ws.join("a")).unwrap();
        let mut p = r.policy.clone();
        p.projects.dir = Some(r.ws.join("a/b/projects"));
        assert!(p.locate_projects(&r.data).is_err());
        // Without a link: the missing parent is named, not made.
        p.projects.dir = Some(r.ws.join("c/projects"));
        assert!(
            p.locate_projects(&r.data)
                .unwrap_err()
                .contains("make it first")
        );
        assert!(!r.ws.join("c").exists());
        // After start: the parent exists, then becomes a link outside.
        std::fs::create_dir(r.ws.join("p")).unwrap();
        p.projects.dir = Some(r.ws.join("p/projects"));
        p.locate_projects(&r.data).unwrap();
        std::fs::rename(r.ws.join("p"), r.ws.join("p-away")).unwrap();
        std::os::unix::fs::symlink(&outside, r.ws.join("p")).unwrap();
        let e = create(&p, &r.made, "x", &suffix).await.unwrap_err();
        assert!(e.contains("moved or replaced"), "{e}");
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
        assert!(!r.ws.join("p-away/projects").exists());
    }

    /// PR 143 review: a commit step that fails after planting a file next
    /// to `.git`. The build folder is the node's own, in staging: it goes
    /// whole, and nothing reaches the projects folder.
    #[tokio::test]
    async fn a_failed_build_leaves_nothing_anywhere() {
        let r = rig();
        let fake = fake_git(&r, "echo keep > planted; exit 1");
        crate::git::use_test_git(Some(fake));
        let res = r.make("Keep").await;
        crate::git::use_test_git(None);
        assert!(res.unwrap_err().contains("repository failed"));
        assert_eq!(std::fs::read_dir(&r.projects).unwrap().count(), 0);
        r.data_holds_nothing_made();
    }

    /// A projects folder the node cannot write (a full or read-only disk, a
    /// chat that ran `chmod 555` on it): refused, nothing made, the build
    /// folder gone, and the failed try does not count against the hour.
    #[tokio::test]
    async fn a_read_only_projects_folder_refuses_and_counts_nothing() {
        let r = rig();
        std::fs::create_dir_all(&r.projects).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&r.projects, std::fs::Permissions::from_mode(0o555)).unwrap();
        let e = r.make("x").await.unwrap_err();
        std::fs::set_permissions(&r.projects, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(e.contains("cannot move"), "{e}");
        assert_eq!(std::fs::read_dir(&r.projects).unwrap().count(), 0);
        r.data_holds_nothing_made();
        assert_eq!(r.made.recent().len(), 0);
    }

    /// PR 143 review: slugs of hostile Unicode and punctuation, and of every
    /// ASCII byte alone and around letters.
    #[test]
    fn slugs_of_hostile_names() {
        for (n, want) in [
            ("İstanbul", "stanbul"),
            ("ſecret", "ecret"),
            ("\u{212A}elvin", "elvin"),
            ("ＡＢＣ", EMPTY_SLUG),
            ("-rf", "rf"),
            ("--template=x", "template-x"),
            ("con", "con"),
            ("\u{202E}evil", "evil"),
            ("a\u{0301}", "a"),
        ] {
            assert_eq!(slug(n), want, "{n:?}");
        }
        for b in 0u8..128 {
            let c = b as char;
            for n in [c.to_string(), format!("{c}a{c}"), format!("a{c}b")] {
                let s = slug(&n);
                assert!(!s.is_empty() && s.len() <= SLUG_MAX);
                assert!(
                    !s.starts_with(['-', '.']) && !s.ends_with('-'),
                    "{n:?} -> {s}"
                );
                assert!(
                    s.bytes()
                        .all(|x| x.is_ascii_lowercase() || x.is_ascii_digit() || x == b'-'),
                    "{n:?} -> {s}"
                );
                assert!(!s.contains("--"), "{n:?} -> {s}");
            }
        }
        assert_eq!(slug(&"😀".repeat(64)), EMPTY_SLUG);
        assert!(slug(&"ab ".repeat(85)).len() <= SLUG_MAX);
    }

    #[test]
    fn the_hourly_allowance_is_counted() {
        let m = Made::default();
        for _ in 0..3 {
            m.room(3).unwrap();
            m.record();
        }
        assert!(m.room(3).unwrap_err().contains("projects_per_hour"));
    }
}
