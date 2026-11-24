//! The push mechanics (X.27 step 4, PLAN.md T101) against the clone's
//! fake git server on 127.0.0.1 (`git http-backend` behind a few lines of
//! HTTP that ask for a Basic credential): a made-up token, the canary, is
//! searched for wherever it must not be. No real forge, no real token.

use super::*;
use crate::forge_clone::tests::{
    CANARY, Mode, USER, bare, basic, files_with_token, git_in, pids_with, server,
};
use std::path::PathBuf;

struct Rig {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    staging: PathBuf,
    folder: PathBuf,
    /// The forge's bare repository.
    remote: PathBuf,
    markers: PathBuf,
}

fn rig() -> Rig {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    bare(&root, "o/r", |_| {});
    let staging = root.join("staging");
    let markers = root.join("markers");
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::create_dir_all(&markers).unwrap();
    let folder = root.join("ws/proj");
    std::fs::create_dir_all(folder.parent().unwrap()).unwrap();
    git_in(
        &root,
        &[
            "clone",
            "-q",
            root.join("o/r.git").to_str().unwrap(),
            folder.to_str().unwrap(),
        ],
    );
    let r = Rig {
        remote: root.join("o/r.git"),
        _tmp: tmp,
        root,
        staging,
        folder,
        markers,
    };
    r.commit("second", "agent's work\n");
    r
}

fn bounds() -> Bounds {
    Bounds {
        timeout: Duration::from_secs(60),
        max_copy: 64 * 1024 * 1024,
        max_files: MAX_FILES,
        stage_time: STAGE_TIME,
    }
}

fn token() -> Secret<String> {
    Secret::new(CANARY.into())
}

impl Rig {
    /// A commit in the folder, made with the folder's config ignored as far
    /// as it can be (the test's own git, not the node's).
    fn commit(&self, msg: &str, text: &str) -> String {
        std::fs::write(self.folder.join("work.txt"), text).unwrap();
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", msg]);
        self.git(&["rev-parse", "HEAD"])
    }

    fn git(&self, args: &[&str]) -> String {
        let mut a = vec![
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "include.path=/dev/null",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.commitGraph=false",
        ];
        a.extend_from_slice(args);
        git_in(&self.folder, &a)
    }

    fn remote_ref(&self, branch: &str) -> Option<String> {
        let out = std::process::Command::new("git")
            .args(["--git-dir"])
            .arg(&self.remote)
            .args([
                "rev-parse",
                "--verify",
                "-q",
                &format!("refs/heads/{branch}"),
            ])
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    fn open(&self) -> Folder {
        Folder::open(&self.folder, None).unwrap()
    }

    fn stage(&self) -> Result<Staged, String> {
        stage(&self.staging, &self.open(), "sha1", &bounds())
    }

    async fn push(&self, host: &str, branch: &str, commit: &str) -> Result<Pushed, String> {
        let st = self.stage()?;
        push(
            &st,
            None,
            host,
            "o/r",
            branch,
            commit,
            USER,
            &token(),
            &bounds(),
        )
        .await
    }

    /// A program that records that it ran.
    fn marker(&self, name: &str) -> String {
        let p = self.root.join(format!("mark-{name}.sh"));
        std::fs::write(
            &p,
            format!(
                "#!/bin/sh\necho {name} >> {}/ran\ncat >/dev/null\n",
                self.markers.display()
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p.display().to_string()
    }

    fn ran(&self) -> String {
        std::fs::read_to_string(self.markers.join("ran")).unwrap_or_default()
    }

    fn staging_empty(&self) -> bool {
        std::fs::read_dir(&self.staging).unwrap().next().is_none()
    }
}

fn ask() -> Mode {
    Mode {
        ask: true,
        ..Default::default()
    }
}

/// The plain case: the pinned commit reaches the forge's branch, the token
/// only through the helper, and the node's repository is gone afterwards.
#[tokio::test]
async fn the_pinned_commit_reaches_the_branch() {
    let r = rig();
    let forge = server(&r.root, ask());
    let pin = pin(&r.open(), None).unwrap();
    assert_eq!(pin.branch, "main");
    let p = r.push(&forge.host(), "feature", &pin.commit).await.unwrap();
    assert_eq!(p.flag, '*', "{p:?}");
    assert_eq!(
        r.remote_ref("feature").as_deref(),
        Some(pin.commit.as_str())
    );
    assert!(r.staging_empty(), "the node's repository was not removed");
    assert!(
        forge
            .seen()
            .iter()
            .any(|s| s.auth.as_deref() == Some(basic().as_str())),
        "the credential never reached the forge"
    );
    // Again: up to date, nothing sent.
    let p = r.push(&forge.host(), "feature", &pin.commit).await.unwrap();
    assert_eq!(p.flag, '=', "{p:?}");
}

/// The folder's config, include, hooks and helpers name the thief and
/// marker programs: nothing of them is read or run, the thief gets nothing,
/// and the push goes where the node built the URL for (T19).
#[tokio::test]
async fn nothing_of_the_folders_config_or_hooks_is_read_or_run() {
    let r = rig();
    let forge = server(&r.root, ask());
    let thief = server(&r.root, Mode::default());
    let t = thief.base();
    let inc = r.root.join("inc.cfg");
    std::fs::write(
        &inc,
        format!("[url \"{t}/\"]\n\tinsteadOf = {}/\n", forge.base()),
    )
    .unwrap();
    let hooks = r.folder.join(".git/hooks");
    for h in [
        "pre-push",
        "reference-transaction",
        "post-update",
        "push-to-checkout",
    ] {
        let m = r.marker(h);
        std::fs::copy(&m, hooks.join(h)).unwrap();
    }
    let mut cfg = std::fs::read_to_string(r.folder.join(".git/config")).unwrap();
    cfg.push_str(&format!(
        "[include]\n\tpath = {inc}\n\
         [url \"{t}/\"]\n\tpushInsteadOf = {b}/\n\tinsteadOf = {b}/\n\
         [remote \"origin\"]\n\tpushurl = {t}/o/r.git\n\treceivepack = {rp}\n\
         [http]\n\tproxy = {t}\n\textraHeader = X-Leak: folder\n\tsslVerify = false\n\tfollowRedirects = true\n\
         [credential]\n\thelper = !{ch}\n\
         [core]\n\thooksPath = {hp}\n\tsshCommand = {sc}\n\tgitProxy = {gp}\n\tfsmonitor = {fm}\n\talternateRefsCommand = {ar}\n\
         [push]\n\tpushOption = leak\n\tfollowTags = true\n\trecurseSubmodules = on-demand\n\
         [gpg]\n\tprogram = {gpg}\n",
        inc = inc.display(),
        b = forge.base(),
        rp = r.marker("receivepack"),
        ch = r.marker("credential"),
        hp = hooks.display(),
        sc = r.marker("ssh"),
        gp = r.marker("gitproxy"),
        fm = r.marker("fsmonitor"),
        ar = r.marker("alternaterefs"),
        gpg = r.marker("gpg"),
    ));
    std::fs::write(r.folder.join(".git/config"), cfg).unwrap();
    // A gitlink and a .gitmodules naming the thief: data, nothing runs.
    std::fs::write(
        r.folder.join(".gitmodules"),
        format!("[submodule \"s\"]\n\tpath = s\n\turl = {t}/o/r.git\n"),
    )
    .unwrap();
    r.git(&["add", ".gitmodules"]);
    let other = r.git(&["rev-parse", "HEAD~1"]);
    r.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("160000,{other},s"),
    ]);
    r.git(&["commit", "-q", "-m", "gitlink"]);
    let pin = pin(&r.open(), None).unwrap();
    let p = r.push(&forge.host(), "main", &pin.commit).await.unwrap();
    assert_eq!(p.flag, ' ', "{p:?}");
    assert_eq!(r.remote_ref("main").as_deref(), Some(pin.commit.as_str()));
    assert_eq!(r.ran(), "", "a program the folder names ran");
    assert!(thief.seen().is_empty(), "{:?}", thief.seen());
    for s in forge.seen() {
        assert!(!s.head.contains("X-Leak"), "{}", s.head);
        assert!(!s.path.contains("leak"), "{}", s.path);
    }
}

/// An alternates chain the agent wrote into the folder's store, naming
/// another repository on the node: that repository's commit cannot be
/// pushed (the experiment's E2, which pushed it with alternates).
#[tokio::test]
async fn an_alternates_chain_never_reaches_another_repository() {
    let r = rig();
    let forge = server(&r.root, ask());
    let other = r.root.join("other");
    std::fs::create_dir_all(&other).unwrap();
    git_in(&other, &["init", "-q", "-b", "main"]);
    std::fs::write(other.join("secret"), "OTHER-REPO-SECRET-CANARY\n").unwrap();
    git_in(&other, &["add", "-A"]);
    git_in(&other, &["commit", "-q", "-m", "other"]);
    let theirs = git_in(&other, &["rev-parse", "HEAD"]);
    std::fs::write(
        r.folder.join(".git/objects/info/alternates"),
        format!("{}\n", other.join(".git/objects").display()),
    )
    .unwrap();
    // Also as the branch's own ref: what an agent would write.
    std::fs::write(r.folder.join(".git/refs/heads/leak"), format!("{theirs}\n")).unwrap();
    let pin = pin(&r.open(), Some("leak")).unwrap();
    assert_eq!(pin.commit, theirs);
    let st = r.stage().unwrap();
    assert!(subject(&st, &theirs, None).await.is_err());
    let e = r.push(&forge.host(), "leak", &theirs).await.unwrap_err();
    assert!(e.contains("git push failed"), "{e}");
    assert_eq!(r.remote_ref("leak"), None);
    // A loose object file that is a link to the other repository's object
    // is not taken either.
    let (d, f) = theirs.split_at(2);
    std::fs::create_dir_all(r.folder.join(".git/objects").join(d)).unwrap();
    std::os::unix::fs::symlink(
        other.join(".git/objects").join(d).join(f),
        r.folder.join(".git/objects").join(d).join(f),
    )
    .unwrap();
    let st = r.stage().unwrap();
    assert!(!st.repo.join("objects").join(d).join(f).exists());
    assert!(subject(&st, &theirs, None).await.is_err());
}

/// What the node's repository holds: loose objects and pack/idx files of
/// the folder, nothing of `info/` (alternates, http-alternates, the
/// commit-graph), no multi-pack-index, promisor or keep file.
#[tokio::test]
async fn only_loose_objects_and_packs_are_taken() {
    let r = rig();
    let forge = server(&r.root, ask());
    r.git(&["gc", "-q"]);
    let objects = r.folder.join(".git/objects");
    std::fs::write(
        objects.join("info/http-alternates"),
        format!("{}/x\n", forge.base()),
    )
    .unwrap();
    std::fs::write(objects.join("info/commit-graph"), b"junk-graph").unwrap();
    std::fs::write(objects.join("pack/multi-pack-index"), b"junk-midx").unwrap();
    let pack = std::fs::read_dir(objects.join("pack"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .find(|n| n.ends_with(".pack"))
        .unwrap();
    let stem = pack.trim_end_matches(".pack");
    std::fs::write(objects.join(format!("pack/{stem}.promisor")), b"").unwrap();
    std::fs::write(objects.join(format!("pack/{stem}.keep")), b"").unwrap();
    let st = r.stage().unwrap();
    let mut names: Vec<String> = walkdir::WalkDir::new(st.repo.join("objects"))
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .map(|e| {
            e.path()
                .strip_prefix(&st.repo)
                .unwrap()
                .display()
                .to_string()
        })
        .collect();
    names.sort();
    assert!(
        names.iter().all(|n| (n.starts_with("objects/pack/pack-")
            && (n.ends_with(".pack") || n.ends_with(".idx")))
            || (n.len() == "objects/xx/".len() + 38)),
        "{names:?}"
    );
    assert!(names.iter().any(|n| n.ends_with(".pack")), "{names:?}");
    let pin = pin(&r.open(), None).unwrap();
    drop(st);
    let p = r.push(&forge.host(), "packed", &pin.commit).await.unwrap();
    assert_eq!(p.flag, '*');
}

/// A forge that redirects: not followed; the other host gets nothing.
#[tokio::test]
async fn a_redirect_is_not_followed() {
    let r = rig();
    let thief = server(&r.root, Mode::default());
    for on_post in [false, true] {
        let forge = server(
            &r.root,
            Mode {
                ask: true,
                redirect: Some((302, thief.base(), on_post)),
                ..Default::default()
            },
        );
        let pin = pin(&r.open(), None).unwrap();
        let res = r.push(&forge.host(), "redir", &pin.commit).await;
        assert!(res.is_err(), "{res:?}");
        assert!(!res.unwrap_err().contains(CANARY));
    }
    assert!(thief.seen().is_empty(), "{:?}", thief.seen());
    assert_eq!(r.remote_ref("redir"), None);
}

/// A push the forge refuses (non-fast-forward) is reported as the forge's
/// porcelain line, without git's hints; the forge's branch is unchanged.
#[tokio::test]
async fn a_rejected_push_is_the_forges_answer_without_hints() {
    let r = rig();
    let forge = server(&r.root, ask());
    let first = pin(&r.open(), None).unwrap();
    r.push(&forge.host(), "main", &first.commit).await.unwrap();
    r.git(&["reset", "-q", "--hard", "HEAD~1"]);
    let diverged = r.commit("diverged", "other work\n");
    let p = r.push(&forge.host(), "main", &diverged).await.unwrap();
    assert_eq!(p.flag, '!', "{p:?}");
    assert!(p.summary.contains("rejected"), "{p:?}");
    assert!(!p.summary.contains("hint"), "{p:?}");
    assert_eq!(r.remote_ref("main").as_deref(), Some(first.commit.as_str()));
}

/// (a) A linked object file the agent rewrites after the link: git reads
/// the new bytes, and the forge, which names objects by the hash of what
/// it received and checks that the new ref reaches only objects it has,
/// refuses the push. The forge never gets the pinned id with other content.
#[tokio::test]
async fn a_rewritten_object_only_fails_the_push() {
    let r = rig();
    let forge = server(&r.root, ask());
    let pin = pin(&r.open(), None).unwrap();
    let st = r.stage().unwrap();
    // The commit's tree's blob `work.txt`, rewritten in place (same inode)
    // as a valid loose object of other content.
    let blob = r.git(&["rev-parse", "HEAD:work.txt"]);
    let (d, f) = blob.split_at(2);
    let path = r.folder.join(".git/objects").join(d).join(f);
    let staged_path = st.repo.join("objects").join(d).join(f);
    use std::os::unix::fs::MetadataExt;
    assert_eq!(
        std::fs::metadata(&path).unwrap().ino(),
        std::fs::metadata(&staged_path).unwrap().ino(),
        "linked"
    );
    let forged = {
        let other = r.root.join("forged");
        std::fs::create_dir_all(&other).unwrap();
        git_in(&other, &["init", "-q"]);
        std::fs::write(other.join("x"), "FORGED CONTENT\n").unwrap();
        let id = git_in(&other, &["hash-object", "-w", "x"]);
        let (a, b) = id.split_at(2);
        std::fs::read(other.join(".git/objects").join(a).join(b)).unwrap()
    };
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    {
        use std::io::Write;
        let mut w = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&path)
            .unwrap();
        w.write_all(&forged).unwrap();
    }
    let res = push(
        &st,
        None,
        &forge.host(),
        "o/r",
        "rewritten",
        &pin.commit,
        USER,
        &token(),
        &bounds(),
    )
    .await;
    assert!(
        res.is_err() || res.as_ref().is_ok_and(|p| p.flag == '!'),
        "{res:?}"
    );
    assert_eq!(r.remote_ref("rewritten"), None);
    // The forge has no object of that id with the forged content.
    let out = std::process::Command::new("git")
        .arg("--git-dir")
        .arg(&r.remote)
        .args(["cat-file", "-p", &blob])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&out.stdout).contains("FORGED"));
}

/// (a) Where a link is refused (another owner's files under
/// `fs.protected_hardlinks`, another file system), the objects are copied,
/// with no link followed and within the copy cap; past the cap, a plain
/// sentence and nothing pushed.
#[tokio::test]
async fn refused_links_fall_back_to_a_bounded_copy() {
    let r = rig();
    let forge = server(&r.root, ask());
    let pin = pin(&r.open(), None).unwrap();
    refuse_links(true);
    let st = r.stage().unwrap();
    assert!(
        st.linked == 0 && st.copied > 0,
        "{} {}",
        st.linked,
        st.copied
    );
    let p = push(
        &st,
        None,
        &forge.host(),
        "o/r",
        "copied",
        &pin.commit,
        USER,
        &token(),
        &bounds(),
    )
    .await
    .unwrap();
    assert_eq!(p.flag, '*');
    drop(st);
    let small = Bounds {
        max_copy: 100,
        ..bounds()
    };
    let e = stage(&r.staging, &r.open(), "sha1", &small).err().unwrap();
    assert!(e.contains("clone_max_mb"), "{e}");
    refuse_links(false);
    assert!(r.staging_empty());
}

/// (b) More object files than the cap: a plain sentence before anything is
/// sent, and the node's repository removed.
#[tokio::test]
async fn too_many_object_files_are_refused_plainly() {
    let r = rig();
    let few = Bounds {
        max_files: 2,
        ..bounds()
    };
    let e = stage(&r.staging, &r.open(), "sha1", &few).err().unwrap();
    assert!(e.contains("git gc"), "{e}");
    assert!(r.staging_empty());
}

/// While a push waits on a stalled forge, no process has the token in its
/// command line or environment (it is only in the pipe). The limit is long
/// and the test releases the forge once it has looked, so no time limit
/// races the look; then git's whole group is gone.
#[tokio::test]
async fn a_stalled_push_has_no_token_in_proc() {
    let r = rig();
    let forge = server(
        &r.root,
        Mode {
            stall: true,
            ..Default::default()
        },
    );
    let pin = pin(&r.open(), None).unwrap();
    let st = r.stage().unwrap();
    let long = Bounds {
        timeout: Duration::from_secs(120),
        ..bounds()
    };
    let look = async {
        let mut seen = Vec::new();
        // Until the forge has the request and git is seen, then a few
        // more looks, then the forge lets go.
        let mut after = 0;
        for _ in 0..3000 {
            tokio::time::sleep(Duration::from_millis(20)).await;
            let pids = pids_with(&forge.host());
            for pid in &pids {
                for f in ["cmdline", "environ"] {
                    if let Ok(b) = std::fs::read(format!("/proc/{pid}/{f}")) {
                        seen.push(String::from_utf8_lossy(&b).to_string());
                    }
                }
            }
            if !forge.seen().is_empty() && !pids.is_empty() {
                after += 1;
                if after >= 10 {
                    break;
                }
            }
        }
        forge.release();
        seen
    };
    let (host, tok) = (forge.host(), token());
    let run = push(
        &st,
        None,
        &host,
        "o/r",
        "stall",
        &pin.commit,
        USER,
        &tok,
        &long,
    );
    let (res, seen) = tokio::join!(run, look);
    let e = res.unwrap_err();
    assert!(!e.contains(CANARY) && !e.contains("clone_timeout_s"), "{e}");
    assert!(!seen.is_empty(), "no git process was seen");
    for s in seen {
        assert!(!s.contains(CANARY), "{s}");
    }
    for _ in 0..250 {
        if pids_with(&forge.host()).is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(pids_with(&forge.host()), Vec::<i32>::new());
}

/// A stalled forge: the push ends at its time limit with its process
/// group, nothing else in the test racing the limit.
#[tokio::test]
async fn a_stalled_push_ends_at_its_limit() {
    let r = rig();
    let forge = server(
        &r.root,
        Mode {
            stall: true,
            ..Default::default()
        },
    );
    let pin = pin(&r.open(), None).unwrap();
    let st = r.stage().unwrap();
    let short = Bounds {
        timeout: Duration::from_secs(2),
        ..bounds()
    };
    let e = push(
        &st,
        None,
        &forge.host(),
        "o/r",
        "stall",
        &pin.commit,
        USER,
        &token(),
        &short,
    )
    .await
    .unwrap_err();
    assert!(e.contains("clone_timeout_s"), "{e}");
    assert!(!e.contains(CANARY), "{e}");
    assert_eq!(pids_with(&forge.host()), Vec::<i32>::new());
}

/// What is read before the card: the branch from `HEAD` or by name, from
/// loose refs and `packed-refs`; a detached or unborn `HEAD`, a hostile
/// name and a missing branch are plain sentences.
#[test]
fn pins() {
    let r = rig();
    let head = r.git(&["rev-parse", "HEAD"]);
    assert_eq!(
        pin(&r.open(), None).unwrap(),
        Pin {
            branch: "main".into(),
            commit: head.clone()
        }
    );
    r.git(&["branch", "feature/x"]);
    r.git(&["pack-refs", "--all"]);
    assert_eq!(pin(&r.open(), Some("feature/x")).unwrap().commit, head);
    assert!(
        pin(&r.open(), Some("nope"))
            .unwrap_err()
            .contains("no commit")
    );
    r.git(&["checkout", "-q", "--detach"]);
    assert!(pin(&r.open(), None).unwrap_err().contains("detached"));
    r.git(&["checkout", "-q", "--orphan", "empty"]);
    assert!(pin(&r.open(), None).unwrap_err().contains("no commit"));
    // A `.git` that is a file (a linked worktree) is not read.
    let wt = r.root.join("wt");
    std::fs::create_dir_all(&wt).unwrap();
    std::fs::write(
        wt.join(".git"),
        format!("gitdir: {}\n", r.folder.join(".git").display()),
    )
    .unwrap();
    assert!(Folder::open(&wt, None).and_then(|f| pin(&f, None)).is_err());
}

/// git's text loses terminal escapes, the token in every form and hints.
#[test]
fn gits_text_is_cleaned() {
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(format!("{USER}:{CANARY}"));
    let raw = format!(
        "\u{1b}[31merror\u{1b}[0m: {CANARY} at https://{USER}:{CANARY}@h/x\n\
         hint: use git pull\nremote: {b64}\n\u{1b}]0;title\u{7}tail"
    );
    let t = clean_text(&raw, USER, &token());
    assert!(!t.contains(CANARY) && !t.contains(&b64), "{t}");
    assert!(!t.contains('\u{1b}') && !t.contains("hint:"), "{t}");
    assert!(t.contains("error") && t.contains("tail"), "{t}");
    let l = last_lines(&format!("a\n\nb\nc\nd\u{202e}e\n{}", "x".repeat(1000)));
    assert!(l.chars().count() <= crate::forge_clone::ERROR_MAX && !l.contains('\u{202e}'));
}

/// Nothing the push leaves holds the token: no file under the test's tree
/// (the forge's own repository aside, which never stores it either).
#[tokio::test]
async fn no_file_holds_the_token() {
    let r = rig();
    let forge = server(&r.root, ask());
    let pin = pin(&r.open(), None).unwrap();
    r.push(&forge.host(), "t", &pin.commit).await.unwrap();
    assert_eq!(files_with_token(&r.root), Vec::<PathBuf>::new());
}

/// (b) Measured by hand, not in CI: the link step over the repository at
/// `HARNESS_PUSH_MEASURE_REPO` (its top folder), into a staging folder
/// beside it, with the shipped bounds; prints the count and the time.
#[test]
#[ignore]
fn measure_the_link_step() {
    let Some(top) = std::env::var_os("HARNESS_PUSH_MEASURE_REPO") else {
        return;
    };
    let top = PathBuf::from(top).canonicalize().unwrap();
    let staging = top.parent().unwrap().join("measure-staging");
    std::fs::create_dir_all(&staging).unwrap();
    let b = Bounds {
        timeout: Duration::from_secs(600),
        max_copy: 2048 * 1024 * 1024,
        max_files: MAX_FILES,
        stage_time: STAGE_TIME,
    };
    for links in [true, false] {
        refuse_links(!links);
        let t = std::time::Instant::now();
        let r = stage(&staging, &Folder::open(&top, None).unwrap(), "sha1", &b);
        let took = t.elapsed();
        match &r {
            Ok(s) => eprintln!(
                "MEASURE links={links}: linked {} copied {} ({} bytes) in {took:?}",
                s.linked, s.copied, s.copied_bytes
            ),
            Err(e) => eprintln!("MEASURE links={links}: refused after {took:?}: {e}"),
        }
        let t = std::time::Instant::now();
        drop(r);
        eprintln!("MEASURE links={links}: removed in {:?}", t.elapsed());
    }
    refuse_links(false);
    let _ = std::fs::remove_dir_all(&staging);
}

// ------------------------------------------- F1: directories swapped mid-walk

/// Another repository on the node (the one the agent wants leaked), with
/// its commit id; `packed`: in one pack, else loose.
fn other_repo(r: &Rig, packed: bool) -> (PathBuf, String) {
    let other = r.root.join("other");
    std::fs::create_dir_all(&other).unwrap();
    git_in(&other, &["init", "-q", "-b", "main"]);
    std::fs::write(other.join("secret"), "OTHER-REPO-SECRET-CANARY\n").unwrap();
    git_in(&other, &["add", "-A"]);
    git_in(&other, &["commit", "-q", "-m", "other"]);
    if packed {
        git_in(&other, &["gc", "-q"]);
    }
    let id = git_in(&other, &["rev-parse", "HEAD"]);
    (other, id)
}

/// At race point `at` (once), `dir` is swapped for a symbolic link to
/// `to`, as a racing agent would.
fn swap_at(at: &str, dir: PathBuf, to: PathBuf) {
    let at = at.to_string();
    let mut done = false;
    set_race(Some(Box::new(move |p: &str| {
        if done || p != at {
            return;
        }
        done = true;
        let mut real = dir.clone().into_os_string();
        real.push(".real");
        std::fs::rename(&dir, &real).unwrap();
        std::os::unix::fs::symlink(&to, &dir).unwrap();
    })));
}

/// The forge has neither the other repository's commit nor its secret.
fn forge_clean(r: &Rig, theirs: &str) {
    let out = std::process::Command::new("git")
        .arg("--git-dir")
        .arg(&r.remote)
        .args(["cat-file", "-e", theirs])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "the forge has the other repository's commit"
    );
    assert_eq!(r.remote_ref("leak"), None);
}

/// Stage with the swap armed, then try what a lying folder would want:
/// show and push the other repository's commit. Nothing of it may reach
/// the node's repository or the forge.
async fn swap_case(r: &Rig, theirs: &str) -> Result<(), String> {
    let forge = server(&r.root, ask());
    let staged = r.stage();
    set_race(None);
    if let Ok(st) = &staged {
        assert!(
            subject(st, theirs, None).await.is_err(),
            "the other repository's commit is in the node's repository"
        );
        let _ = push(
            st,
            None,
            &forge.host(),
            "o/r",
            "leak",
            theirs,
            USER,
            &token(),
            &bounds(),
        )
        .await;
    }
    forge_clean(r, theirs);
    staged.map(|_| ())
}

#[tokio::test]
async fn a_swapped_git_folder_is_not_followed() {
    let r = rig();
    let (other, theirs) = other_repo(&r, true);
    swap_at("start", r.folder.join(".git"), other.join(".git"));
    let res = swap_case(&r, &theirs).await;
    assert!(res.is_err(), "the walk went on through a swapped .git");
}

#[tokio::test]
async fn a_swapped_objects_folder_is_not_followed() {
    let r = rig();
    let (other, theirs) = other_repo(&r, true);
    swap_at(
        "start",
        r.folder.join(".git/objects"),
        other.join(".git/objects"),
    );
    let res = swap_case(&r, &theirs).await;
    assert!(res.is_err(), "the walk went on through a swapped objects");
}

#[tokio::test]
async fn a_swapped_pack_folder_is_not_followed() {
    let r = rig();
    r.git(&["gc", "-q"]);
    let (other, theirs) = other_repo(&r, true);
    swap_at(
        "descend pack",
        r.folder.join(".git/objects/pack"),
        other.join(".git/objects/pack"),
    );
    let res = swap_case(&r, &theirs).await;
    assert!(
        res.is_err(),
        "the walk descended into a swapped pack folder"
    );
}

#[tokio::test]
async fn a_swapped_loose_folder_is_not_followed() {
    let r = rig();
    let (other, theirs) = other_repo(&r, false);
    let xx = &theirs[..2];
    std::fs::create_dir_all(r.folder.join(".git/objects").join(xx)).unwrap();
    swap_at(
        &format!("descend {xx}"),
        r.folder.join(".git/objects").join(xx),
        other.join(".git/objects").join(xx),
    );
    let res = swap_case(&r, &theirs).await;
    assert!(
        res.is_err(),
        "the walk descended into a swapped loose folder"
    );
}

/// Between the file's check and its link, the pack folder is swapped for
/// a link to a folder holding the other repository's pack under the
/// folder's own pack's names: what is linked must be what was checked.
#[tokio::test]
async fn a_folder_swapped_between_check_and_link_is_not_followed() {
    let r = rig();
    r.git(&["gc", "-q"]);
    let (other, theirs) = other_repo(&r, true);
    let ours = r.folder.join(".git/objects/pack");
    let mine: Vec<String> = std::fs::read_dir(&ours)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".pack") || n.ends_with(".idx"))
        .collect();
    let theirs_dir = other.join(".git/objects/pack");
    let fake = r.root.join("fakepack");
    std::fs::create_dir_all(&fake).unwrap();
    for e in std::fs::read_dir(&theirs_dir).unwrap().flatten() {
        let n = e.file_name().to_string_lossy().to_string();
        for m in &mine {
            if (n.ends_with(".pack") && m.ends_with(".pack"))
                || (n.ends_with(".idx") && m.ends_with(".idx"))
            {
                std::fs::copy(e.path(), fake.join(m)).unwrap();
            }
        }
    }
    let first = mine.iter().find(|m| m.ends_with(".pack")).unwrap().clone();
    swap_at(&format!("link {first}"), ours, fake);
    let _ = swap_case(&r, &theirs).await;
}

/// A ref's folder swapped for a link to a folder naming the other
/// repository's commit: the pin is the folder's own commit or nothing.
#[test]
fn a_swapped_ref_folder_is_not_followed() {
    let r = rig();
    let (_other, theirs) = other_repo(&r, true);
    r.git(&["branch", "feat/x"]);
    let fake = r.root.join("fakeref");
    std::fs::create_dir_all(&fake).unwrap();
    std::fs::write(fake.join("x"), format!("{theirs}\n")).unwrap();
    swap_at("ref", r.folder.join(".git/refs/heads/feat"), fake);
    let got = pin(&r.open(), Some("feat/x"));
    set_race(None);
    if let Ok(p) = got {
        assert_ne!(p.commit, theirs, "the pin followed a swapped ref folder");
    }
}

/// The reviewer's threaded probe, kept as a stress test (run by hand): a
/// thread swaps `pack` with a link to another repository's pack while the
/// walk runs; it must never stage the other repository's commit.
#[tokio::test]
#[ignore]
async fn stress_a_racing_pack_swap_never_stages_another_repository() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    let r = rig();
    let (other, theirs) = other_repo(&r, true);
    std::fs::write(r.folder.join(".git/refs/heads/leak"), format!("{theirs}\n")).unwrap();
    for i in 0..3000 {
        let p = r.root.join("blob");
        std::fs::write(&p, format!("pad {i}\n")).unwrap();
        r.git(&["hash-object", "-w", p.to_str().unwrap()]);
    }
    let objs = r.folder.join(".git/objects");
    std::fs::create_dir_all(objs.join("pack")).unwrap();
    std::os::unix::fs::symlink(other.join(".git/objects/pack"), objs.join("pack.lnk")).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let (s2, o2) = (stop.clone(), objs.clone());
    let racer = std::thread::spawn(move || {
        let mut n = 0u64;
        while !s2.load(Ordering::Relaxed) {
            let _ = std::fs::rename(o2.join("pack"), o2.join("pack.real"));
            let _ = std::fs::rename(o2.join("pack.lnk"), o2.join("pack"));
            std::thread::sleep(Duration::from_micros(300 + (n % 7) * 100));
            let _ = std::fs::rename(o2.join("pack"), o2.join("pack.lnk"));
            let _ = std::fs::rename(o2.join("pack.real"), o2.join("pack"));
            std::thread::sleep(Duration::from_micros(50 + (n % 5) * 40));
            n += 1;
        }
    });
    let mut won = 0;
    for _ in 0..3000 {
        if let Ok(st) = r.stage()
            && subject(&st, &theirs, None).await.is_ok()
        {
            won += 1;
        }
    }
    stop.store(true, Ordering::Relaxed);
    racer.join().unwrap();
    assert_eq!(
        won, 0,
        "the other repository's commit was staged {won} times"
    );
}

// ------------------------------------------- F6: the card's subject is bound

/// The pinned commit's loose object rewritten in place with another
/// commit's bytes (git's `cat-file` does not check the hash): the card
/// must not show the other commit's subject as the pinned commit's.
#[tokio::test]
async fn a_rewritten_commit_object_shows_no_subject() {
    let r = rig();
    let pin = pin(&r.open(), None).unwrap();
    let st = r.stage().unwrap();
    assert_eq!(subject(&st, &pin.commit, None).await.unwrap(), "second");
    // Another commit, with a subject of the agent's choosing.
    let other = r.root.join("forged");
    std::fs::create_dir_all(&other).unwrap();
    git_in(&other, &["init", "-q"]);
    git_in(
        &other,
        &["commit", "-q", "--allow-empty", "-m", "FORGED SUBJECT"],
    );
    let fid = git_in(&other, &["rev-parse", "HEAD"]);
    let forged = std::fs::read(other.join(".git/objects").join(&fid[..2]).join(&fid[2..])).unwrap();
    let path = r
        .folder
        .join(".git/objects")
        .join(&pin.commit[..2])
        .join(&pin.commit[2..]);
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    {
        use std::io::Write;
        let mut w = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&path)
            .unwrap();
        w.write_all(&forged).unwrap();
    }
    let shown = subject(&st, &pin.commit, None).await;
    assert!(
        !shown.as_deref().unwrap_or("").contains("FORGED"),
        "the card would show another commit's subject: {shown:?}"
    );
}

/// Between a file's check and its link, the folder's pack and index are
/// replaced by links to the other repository's: the link made would be a
/// link, which the check after the link refuses (and git would follow).
#[tokio::test]
async fn a_file_swapped_for_a_link_before_its_link_is_refused() {
    let r = rig();
    r.git(&["gc", "-q"]);
    let (other, theirs) = other_repo(&r, true);
    let ours = r.folder.join(".git/objects/pack");
    let mine: Vec<String> = std::fs::read_dir(&ours)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".pack") || n.ends_with(".idx"))
        .collect();
    let theirs_files: Vec<PathBuf> = std::fs::read_dir(other.join(".git/objects/pack"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "pack" || x == "idx"))
        .collect();
    let (o2, m2) = (ours.clone(), mine.clone());
    // At each file's link, that file becomes a link to the other
    // repository's file of its kind.
    set_race(Some(Box::new(move |p: &str| {
        let Some(m) = p
            .strip_prefix("link ")
            .filter(|m| m2.iter().any(|x| x == m))
        else {
            return;
        };
        let ext = if m.ends_with(".pack") { "pack" } else { "idx" };
        let t = theirs_files
            .iter()
            .find(|f| f.extension().is_some_and(|x| x == ext))
            .unwrap();
        std::fs::rename(o2.join(m), o2.join(format!("{m}.real"))).unwrap();
        std::os::unix::fs::symlink(t, o2.join(m)).unwrap();
    })));
    let res = swap_case(&r, &theirs).await;
    assert!(res.is_err(), "a link was staged");
}

/// The same swap with the check after the link turned off stages the
/// other repository's commit: that check is what refuses it.
#[tokio::test]
async fn the_post_link_check_is_what_refuses_a_swapped_file() {
    let r = rig();
    r.git(&["gc", "-q"]);
    let (other, theirs) = other_repo(&r, true);
    let ours = r.folder.join(".git/objects/pack");
    let mine: Vec<String> = std::fs::read_dir(&ours)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".pack") || n.ends_with(".idx"))
        .collect();
    let theirs_files: Vec<PathBuf> = std::fs::read_dir(other.join(".git/objects/pack"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "pack" || x == "idx"))
        .collect();
    // At each file's link, that file becomes a link to the other
    // repository's file of its kind.
    set_race(Some(Box::new(move |p: &str| {
        let Some(m) = p
            .strip_prefix("link ")
            .filter(|m| mine.iter().any(|x| x == m))
        else {
            return;
        };
        let ext = if m.ends_with(".pack") { "pack" } else { "idx" };
        let t = theirs_files
            .iter()
            .find(|f| f.extension().is_some_and(|x| x == ext))
            .unwrap();
        std::fs::rename(ours.join(m), ours.join(format!("{m}.real"))).unwrap();
        std::os::unix::fs::symlink(t, ours.join(m)).unwrap();
    })));
    skip_post_link_check(true);
    let st = r.stage();
    skip_post_link_check(false);
    set_race(None);
    let st = st.expect("staged without the check");
    assert!(
        subject(&st, &theirs, None).await.is_ok(),
        "without the check the other repository's commit is staged"
    );
}

/// (F7) The git processes that hold the token may not dump core: their
/// core limit is 0 (soft and what they may raise it to stays the node's
/// hard limit, never above it).
#[tokio::test]
async fn the_pushs_git_dumps_no_core() {
    let r = rig();
    let forge = server(
        &r.root,
        Mode {
            stall: true,
            ..Default::default()
        },
    );
    let pin = pin(&r.open(), None).unwrap();
    let st = r.stage().unwrap();
    // A long limit; the forge is released once git was looked at, so no
    // time limit races the look.
    let long = Bounds {
        timeout: Duration::from_secs(120),
        ..bounds()
    };
    let look = async {
        let mut limits = Vec::new();
        for _ in 0..3000 {
            tokio::time::sleep(Duration::from_millis(20)).await;
            for pid in pids_with(&forge.host()) {
                if let Ok(t) = std::fs::read_to_string(format!("/proc/{pid}/limits"))
                    && let Some(l) = t.lines().find(|l| l.starts_with("Max core file size"))
                {
                    limits.push(l.split_whitespace().nth(4).unwrap_or("?").to_string());
                }
            }
            if !forge.seen().is_empty() && limits.len() >= 2 {
                break;
            }
        }
        forge.release();
        limits
    };
    let (host, tok) = (forge.host(), token());
    let run = push(
        &st,
        None,
        &host,
        "o/r",
        "core",
        &pin.commit,
        USER,
        &tok,
        &long,
    );
    let (_, limits) = tokio::join!(run, look);
    assert!(!limits.is_empty(), "no git process was seen");
    assert!(limits.iter().all(|l| l == "0"), "{limits:?}");
}

/// (N2) Junk names count against the bound too: a folder of many entries
/// that are not objects is refused with the bound's sentence, not listed
/// whole before any bound.
#[test]
fn junk_names_count_against_the_bound() {
    let r = rig();
    let junk = r.folder.join(".git/objects/pack");
    std::fs::create_dir_all(&junk).unwrap();
    for i in 0..500 {
        std::fs::write(junk.join(format!("junk-{i}")), b"").unwrap();
    }
    let few = Bounds {
        max_files: 100,
        ..bounds()
    };
    let e = stage(&r.staging, &r.open(), "sha1", &few)
        .err()
        .expect("refused");
    assert!(e.contains("git gc"), "{e}");
    assert!(r.staging_empty());
}

/// (N2) Measured by hand: a pack folder of `HARNESS_PUSH_JUNK` (default
/// 300,000) junk names, with the shipped bounds; prints the time.
#[test]
#[ignore]
fn measure_a_junk_folder() {
    let n: usize = std::env::var("HARNESS_PUSH_JUNK")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300_000);
    let r = rig();
    let junk = r.folder.join(".git/objects/pack");
    std::fs::create_dir_all(&junk).unwrap();
    for i in 0..n {
        std::fs::File::create(junk.join(format!("junk-{i}"))).unwrap();
    }
    let hwm = || {
        std::fs::read_to_string("/proc/self/status")
            .unwrap()
            .lines()
            .find(|l| l.starts_with("VmHWM"))
            .unwrap_or("")
            .to_string()
    };
    eprintln!("MEASURE before: {}", hwm());
    let t = std::time::Instant::now();
    let res = stage(&r.staging, &r.open(), "sha1", &bounds());
    eprintln!(
        "MEASURE junk={n}: {:?} in {:?}; after: {}",
        res.as_ref().map(|_| ()),
        t.elapsed(),
        hwm()
    );
}
