//! A new project's folder (ADOPTION_PLAN X.26, PLAN.md T77).
//!
//! The master sends a display name and nothing else (`project.create`).
//! The node makes the folder's name from it, `<slug>-<4 random
//! characters>`, inside the one folder its node.toml names
//! (`[projects] dir`, checked at start: inside `workspace_roots`, clear of
//! the data directory), makes it a git repository with one empty commit,
//! and answers with the folder's path.
//!
//! Nothing here resolves a path the master chose, and nothing follows a
//! link placed in the way: the projects folder is opened once without
//! following a link at its name and compared with what was checked; the
//! new folder is made relative to that open folder with `mkdirat`, which
//! fails when anything (a folder, a file, a link) holds the name, and is
//! then opened without following a link; git starts in that open folder
//! (`/proc/self/fd/<n>`), not at a name. On any failure after the folder
//! was made, it is removed again, and only when it holds nothing but the
//! `.git` this call made.

use std::collections::VecDeque;
use std::ffi::OsStr;
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use nix::errno::Errno;
use nix::fcntl::{AtFlags, OFlag};
use nix::sys::stat::Mode;

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
const TRIES: usize = 8;

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
/// asks in a loop (`[limits] projects_per_hour`).
#[derive(Default)]
pub(crate) struct Made(std::sync::Mutex<VecDeque<Instant>>);

impl Made {
    /// Take one place in the hour's allowance, or say why there is none.
    fn take(&self, per_hour: usize) -> Result<(), String> {
        let mut q = self.0.lock().unwrap_or_else(|p| p.into_inner());
        let now = Instant::now();
        while q
            .front()
            .is_some_and(|t| now.duration_since(*t) >= Duration::from_secs(3600))
        {
            q.pop_front();
        }
        if q.len() >= per_hour {
            return Err(format!(
                "this node has made {per_hour} project folders in the last hour, its limit \
                 ([limits] projects_per_hour in its node.toml); try again later"
            ));
        }
        q.push_back(now);
        Ok(())
    }
}

/// Why a node makes no project folder at all.
pub const OFF: &str = "this node makes no project folders: its node.toml turns them off \
     ([projects] dir), or the folder it names is not usable (see the node's log)";

/// Make a new project's folder for `name` (a display name), with the
/// node's `policy`; its absolute path. `made` bounds how many per hour.
pub(crate) async fn create(
    policy: &NodePolicy,
    made: &Made,
    name: &str,
    make_suffix: &(dyn Fn() -> String + Sync),
) -> Result<PathBuf, String> {
    let Some(dir) = policy.projects.located.clone() else {
        return Err(OFF.into());
    };
    if policy.limits.projects_per_hour == 0 {
        return Err(OFF.into());
    }
    if name.len() > harness_protocol::MAX_PROJECT_NAME {
        return Err(format!(
            "a project's name is at most {} bytes",
            harness_protocol::MAX_PROJECT_NAME
        ));
    }
    if name.trim().is_empty() {
        return Err("a project needs a name".into());
    }
    made.take(policy.limits.projects_per_hour)?;
    let base = slug(name);
    let p = policy.clone();
    let names: Vec<String> = (0..TRIES)
        .map(|_| format!("{base}-{}", make_suffix()))
        .collect();
    let (top, child, path) = tokio::task::spawn_blocking(move || make_folder(&p, &dir, &names))
        .await
        .map_err(|e| format!("making the folder: {e}"))??;
    let proc_path = PathBuf::from(format!("/proc/self/fd/{}", child.as_raw_fd()));
    let made_repo = crate::git::init_project(&proc_path).await;
    let checked = made_repo.and_then(|()| check_made(policy, &child, &path));
    match checked {
        Ok(()) => Ok(path),
        Err(e) => {
            let name = path.file_name().map(OsStr::to_os_string);
            tokio::task::spawn_blocking(move || {
                if let Some(n) = name {
                    remove_made(&top, &child, &n);
                }
            })
            .await
            .ok();
            Err(format!("making the project's repository failed: {e}"))
        }
    }
}

fn dir_flags() -> OFlag {
    OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC
}

fn same(a: &nix::sys::stat::FileStat, b: &std::fs::Metadata) -> bool {
    a.st_dev == b.dev() && a.st_ino == b.ino()
}

/// Open the projects folder `dir` (making it first if it is missing), check
/// it is still the folder the rules allow, and make the first free name of
/// `names` in it. The open projects folder, the open new folder and its
/// path.
fn make_folder(
    policy: &NodePolicy,
    dir: &Path,
    names: &[String],
) -> Result<(OwnedFd, OwnedFd, PathBuf), String> {
    let io = |what: &str, e: std::io::Error| format!("{what} {}: {e}", dir.display());
    if std::fs::symlink_metadata(dir).is_err() {
        std::fs::create_dir_all(dir).map_err(|e| io("cannot make the projects folder", e))?;
    }
    // Its name must not be a link now (it was not one at start: `located`
    // is resolved), and must still be the folder found there by path.
    let top = nix::fcntl::open(dir, dir_flags(), Mode::empty()).map_err(|e| {
        io(
            "the projects folder cannot be opened as a plain folder (a link put in its place is \
             not followed)",
            e.into(),
        )
    })?;
    let real = dir
        .canonicalize()
        .map_err(|e| io("the projects folder", e))?;
    let meta = std::fs::metadata(&real).map_err(|e| io("the projects folder", e))?;
    let st = nix::sys::stat::fstat(&top).map_err(|e| io("the projects folder", e.into()))?;
    if real != dir || !same(&st, &meta) {
        return Err(format!(
            "the projects folder {} was moved or replaced since the node started; restart the \
             node to check it again",
            dir.display()
        ));
    }
    if let Some(why) = policy.projects_refusal(&real) {
        return Err(format!("the projects folder {}: {why}", dir.display()));
    }
    for n in names {
        match nix::sys::stat::mkdirat(&top, n.as_str(), Mode::from_bits_truncate(0o755)) {
            Ok(()) => {
                let child = nix::fcntl::openat(&top, n.as_str(), dir_flags(), Mode::empty())
                    .map_err(|e| {
                        format!("opening the new folder {n}: {}", std::io::Error::from(e))
                    })?;
                return Ok((top, child, real.join(n)));
            }
            // Taken, by anything: a folder, a file, a link. Never reused.
            Err(Errno::EEXIST) => continue,
            Err(e) => {
                return Err(format!(
                    "cannot make a folder in {}: {}",
                    dir.display(),
                    std::io::Error::from(e)
                ));
            }
        }
    }
    Err(format!(
        "every name tried in {} was taken; try again",
        dir.display()
    ))
}

/// After git: the folder at `path` is still the one made (no link or
/// other folder put at its name), holds a `.git` folder, and a session may
/// run there (`check_cwd`).
fn check_made(policy: &NodePolicy, child: &OwnedFd, path: &Path) -> Result<(), String> {
    let st = nix::sys::stat::fstat(child).map_err(|e| e.to_string())?;
    let meta = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_dir() || !same(&st, &meta) {
        return Err(format!("{} was replaced while it was made", path.display()));
    }
    nix::sys::stat::fstatat(child, ".git", AtFlags::AT_SYMLINK_NOFOLLOW)
        .map_err(|e| format!("no .git: {}", std::io::Error::from(e)))?;
    let real = policy.check_cwd(&path.to_string_lossy())?;
    if real != path {
        return Err(format!("{} resolves elsewhere", path.display()));
    }
    Ok(())
}

/// Remove the folder this call made, `name` in `top`: only when it is
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
            "a new project folder whose repository failed holds more than .git; left in place"
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
    if now.st_dev == made.st_dev && now.st_ino == made.st_ino {
        let _ = nix::unistd::unlinkat(top, name, nix::unistd::UnlinkatFlags::RemoveDir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(policy.locate_projects().unwrap(), None);
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
        assert_eq!(std::fs::read_dir(&r.data).unwrap().count(), 0);
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
                assert_eq!(std::fs::read_dir(&r.data).unwrap().count(), 0);
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
        off.policy.locate_projects().unwrap();
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
        std::fs::write(
            &fake,
            "#!/bin/sh\nfor a in \"$@\"; do [ \"$a\" = commit ] && exit 1; done\nexec git \"$@\"\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        crate::git::use_test_git(Some(fake));
        let res = r.make("Broken").await;
        crate::git::use_test_git(None);
        let e = res.unwrap_err();
        assert!(e.contains("repository failed"), "{e}");
        assert_eq!(
            std::fs::read_dir(&r.projects).unwrap().count(),
            0,
            "removed again"
        );
    }

    #[test]
    fn the_projects_folder_is_checked_at_start() {
        let r = rig();
        let at = |dir: Option<PathBuf>| {
            let mut p = r.policy.clone();
            p.projects.dir = dir;
            p.locate_projects().map(|w| (w, p.projects.located.clone()))
        };
        // Named, outside the roots, in or over the data directory, with
        // `..`: the node does not start.
        let outside = r.ws.parent().unwrap().join("outside");
        assert!(at(Some(outside)).unwrap_err().contains("workspace_roots"));
        assert!(at(Some(r.data.join("p"))).unwrap_err().contains("data"));
        let mut over = r.policy.clone();
        over.workspace_roots = vec![r.ws.parent().unwrap().to_path_buf()];
        over.projects.dir = Some(r.ws.parent().unwrap().to_path_buf());
        assert!(over.locate_projects().unwrap_err().contains("data"));
        assert!(at(Some(r.ws.join("a/../b"))).unwrap_err().contains(".."));
        // Off.
        assert_eq!(at(Some(PathBuf::new())).unwrap(), (None, None));
        // The default, outside the roots: off with a reason, the node starts.
        let mut d = r.policy.clone();
        d.projects.dir = None;
        d.workspace_roots = vec![r.ws.clone()];
        if dirs::home_dir().is_some_and(|h| !h.starts_with(&r.ws)) {
            let (w, located) = d
                .locate_projects()
                .map(|w| (w, d.projects.located.clone()))
                .unwrap();
            assert!(w.unwrap().contains("off"));
            assert_eq!(located, None);
        }
    }

    #[test]
    fn the_hourly_allowance_is_counted() {
        let m = Made::default();
        for _ in 0..3 {
            m.take(3).unwrap();
        }
        assert!(m.take(3).unwrap_err().contains("projects_per_hour"));
    }
}
