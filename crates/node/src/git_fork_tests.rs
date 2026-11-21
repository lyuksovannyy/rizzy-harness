//! A fork's own copy of the parent's repository (C.2 part 2, T92): the
//! parent's `.git` is hostile (every way git can be made to run a program
//! from a repository's own config, hooks and attributes), and the whole
//! operation must run none of them, write nothing in the parent, and end in
//! a correct copy or a clean refusal. The same test fails, with a marker
//! file, when the clone is replaced by `git worktree add` or by a plain
//! clone (see `the_test_notices_*`).

use std::os::unix::fs::MetadataExt;

use super::fork_copy::{Mutation, Plan, fill, mutate, parse_version, patched, upstream_url};
use super::tests::{Hostile, sh};
use super::*;

/// The hooks git knows, each a marker script when planted.
const HOOKS: &[&str] = &[
    "applypatch-msg",
    "pre-applypatch",
    "post-applypatch",
    "pre-commit",
    "pre-merge-commit",
    "prepare-commit-msg",
    "commit-msg",
    "post-commit",
    "pre-rebase",
    "post-checkout",
    "post-merge",
    "pre-push",
    "pre-receive",
    "update",
    "proc-receive",
    "post-receive",
    "post-update",
    "reference-transaction",
    "push-to-checkout",
    "pre-auto-gc",
    "post-rewrite",
    "sendemail-validate",
    "fsmonitor-watchman",
    "post-index-change",
];

struct Rig {
    h: Hostile,
    top: PathBuf,
    dest: PathBuf,
    outside: PathBuf,
    shadow: Repo,
    /// The commit HEAD was on at the cut.
    head: String,
    /// The commit a later parent commit made.
    later: String,
    snap: String,
    before: String,
}

fn digest(dir: &Path) -> String {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    for e in walkdir::WalkDir::new(dir)
        .sort_by_file_name()
        .follow_links(false)
    {
        let e = e.unwrap();
        h.update(e.path().as_os_str().as_encoded_bytes());
        let m = e.metadata().unwrap();
        h.update(m.mode().to_le_bytes());
        if m.is_file() {
            h.update(std::fs::read(e.path()).unwrap());
        } else if m.is_symlink() {
            h.update(
                std::fs::read_link(e.path())
                    .unwrap()
                    .as_os_str()
                    .as_encoded_bytes(),
            );
        }
    }
    hex::encode(h.finalize())
}

fn out(dir: &Path, args: &[&str]) -> String {
    let o = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("XDG_CONFIG_HOME", "/nonexistent")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}: {o:?}");
    String::from_utf8_lossy(&o.stdout)
        .trim_end_matches('\n')
        .to_string()
}

/// A parent with history, uncommitted work, an untracked and an ignored
/// file, a commit after the cut, hostile ref names, a gitlink, committed
/// attributes; then its `.git` made hostile. No git runs in it after that.
async fn rig(with_snapshot: bool) -> Rig {
    let h = Hostile::new();
    let top = h.repo.parent().unwrap().canonicalize().unwrap();
    let repo = h.repo.clone();
    std::fs::write(
        repo.join(".gitattributes"),
        "* filter=evil diff=evil merge=evil\n*.txt ident export-subst\n",
    )
    .unwrap();
    std::fs::write(repo.join(".gitignore"), "ignored.log\n").unwrap();
    std::fs::write(
        repo.join(".gitmodules"),
        format!(
            "[submodule \"sm\"]\n\tpath = sm\n\turl = file://{}\n\tupdate = !true\n",
            repo.display()
        ),
    )
    .unwrap();
    let first = out(&repo, &["rev-parse", "HEAD"]);
    sh(
        &repo,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{first},sm"),
        ],
    );
    sh(&repo, &["add", "."]);
    sh(&repo, &["commit", "-qm", "attributes, ignore, a submodule"]);
    let head = out(&repo, &["rev-parse", "HEAD"]);
    for name in [
        "refs/heads/$(touch-x)",
        "refs/heads/;touch-y",
        "refs/heads/--upload-pack=touch-z",
        "refs/heads/a`b`",
        "refs/tags/`touch-t`",
        "refs/heads/feat/ok",
    ] {
        sh(&repo, &["update-ref", name, &head]);
    }
    // Work the cut saw: uncommitted, untracked, ignored.
    std::fs::write(repo.join("a.txt"), "two\n").unwrap();
    std::fs::write(repo.join("new.txt"), "new\n").unwrap();
    std::fs::write(repo.join("ignored.log"), "ignored\n").unwrap();
    // A bare repository inside the tree: plain files to every snapshot.
    let inner = repo.join("inner.git");
    std::fs::create_dir_all(inner.join("refs")).unwrap();
    std::fs::write(inner.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::write(
        inner.join("config"),
        format!(
            "[core]\n\tbare = true\n\tfsmonitor = {}\n",
            h.script("inner-fsmonitor").display()
        ),
    )
    .unwrap();
    h.hook(&inner.join("hooks"), "post-checkout");
    let shadow = repo_for(&repo, Some(&h.base)).await.unwrap();
    let snap = snapshot(&shadow, "refs/harness/s/t/after", "after")
        .await
        .unwrap();
    // After the cut the parent goes on: a commit, a deleted file.
    std::fs::write(repo.join("later.txt"), "later\n").unwrap();
    sh(&repo, &["add", "later.txt"]);
    sh(&repo, &["commit", "-qm", "after the cut"]);
    let later = out(&repo, &["rev-parse", "HEAD"]);
    std::fs::remove_file(repo.join("b.txt")).unwrap();
    plant(&h);
    assert_eq!(h.marked(), Vec::<String>::new(), "setup ran something");
    let outside = top.join("outside");
    let dest = top.join("dest");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    // The node's own template directory, which a plain clone would use.
    h.hook(&outside.join("node-template/hooks"), "post-checkout");
    let before = digest(&repo);
    Rig {
        h,
        top,
        dest,
        outside,
        shadow,
        head,
        later,
        snap: if with_snapshot { snap } else { String::new() },
        before,
    }
}

/// Every way a repository's own config, hooks and attributes can name a
/// program. Each is a script that writes a marker when run.
fn plant(h: &Hostile) {
    let git = h.repo.join(".git");
    let root = h.repo.parent().unwrap().to_path_buf();
    for hook in HOOKS {
        h.hook(&git.join("hooks"), hook);
    }
    let hp = root.join("hooks-elsewhere");
    for hook in HOOKS {
        h.hook(&hp, &format!("{hook}-hp"));
        std::fs::copy(hp.join(format!("{hook}-hp")), hp.join(hook)).unwrap();
    }
    let tpl = root.join("template");
    h.hook(&tpl.join("hooks"), "post-checkout");
    let s = |n: &str| h.script(n).to_string_lossy().into_owned();
    let bang = |n: &str| format!("!{}", s(n));
    h.config("core.hooksPath", &hp.to_string_lossy());
    h.config("core.fsmonitor", &s("fsmonitor"));
    h.config("core.sshCommand", &s("sshcommand"));
    h.config("core.pager", &s("pager"));
    h.config("core.editor", &s("editor"));
    h.config("core.gitProxy", &s("gitproxy"));
    h.config("core.askPass", &s("askpass"));
    h.config("core.alternateRefsCommand", &s("altrefs"));
    h.config("core.worktree", &root.join("elsewhere").to_string_lossy());
    h.config("core.untrackedCache", "true");
    h.config("core.sparseCheckout", "true");
    h.config("core.attributesFile", &s("attrsfile"));
    h.config("uploadpack.packObjectsHook", &s("packobjects"));
    h.config("uploadpack.allowFilter", "true");
    h.config("uploadpack.allowAnySHA1InWant", "true");
    h.config("uploadpack.hideRefs", "refs/tags");
    h.config("alias.co", &bang("alias-co"));
    h.config("alias.status", &bang("alias-status"));
    h.config("alias.upload-pack", &bang("alias-uploadpack"));
    h.config("diff.external", &s("diffexternal"));
    h.config("diff.evil.textconv", &s("textconv"));
    h.config("diff.evil.command", &s("diffcommand"));
    h.config("merge.evil.driver", &s("mergedriver"));
    h.config("merge.tool", "evil");
    h.config("mergetool.evil.cmd", &s("mergetool"));
    h.config("filter.evil.clean", &s("clean"));
    h.config("filter.evil.smudge", &s("smudge"));
    h.config("filter.evil.process", &s("process"));
    h.config("filter.evil.required", "true");
    h.config("credential.helper", &bang("credential"));
    h.config("gpg.program", &s("gpg"));
    h.config("gpg.ssh.program", &s("gpgssh"));
    h.config("gpg.x509.program", &s("gpgx509"));
    h.config("commit.gpgsign", "true");
    h.config("tag.gpgsign", "true");
    let extra = root.join("extra.config");
    std::fs::write(
        &extra,
        format!(
            "[core]\n\tfsmonitor = {}\n\thooksPath = {}\n",
            s("include-fsmonitor"),
            hp.display()
        ),
    )
    .unwrap();
    h.config("include.path", &extra.to_string_lossy());
    h.config("includeIf.gitdir:/.path", &extra.to_string_lossy());
    h.config("url.file:///nowhere/.insteadOf", "/");
    h.config("url./nowhere/.pushInsteadOf", "/");
    h.config("protocol.allow", "never");
    h.config("protocol.file.allow", "never");
    h.config("protocol.ext.allow", "always");
    h.config("receive.denyCurrentBranch", "updateInstead");
    h.config("receive.fsckObjects", "true");
    h.config("transfer.hideRefs", "refs/heads");
    h.config("init.templateDir", &tpl.to_string_lossy());
    h.config("submodule.recurse", "true");
    h.config("submodule.sm.url", &format!("file://{}", h.repo.display()));
    h.config("submodule.sm.update", &bang("submodule-update"));
    h.config("remote.origin.uploadpack", &s("remote-uploadpack"));
    h.config("remote.origin.receivepack", &s("remote-receivepack"));
    h.config("remote.origin.vcs", "evil");
    h.config("remote.origin.url", "https://example.invalid/team/proj.git");
    h.config("extensions.worktreeConfig", "true");
    std::fs::create_dir_all(git.join("info")).unwrap();
    std::fs::write(git.join("info/attributes"), "* filter=evil diff=evil\n").unwrap();
    std::fs::write(git.join("info/exclude"), "scratch/\n").unwrap();
    std::fs::create_dir_all(root.join("elsewhere")).unwrap();
}

async fn run(r: &Rig, head: Option<&str>) -> Result<fork_copy::Filled, String> {
    let snap = (!r.snap.is_empty()).then_some(r.snap.as_str());
    fill(&Plan {
        source: &r.h.repo,
        dest: &r.dest,
        outside: &r.outside,
        branch: "fork/ab12cd34",
        head,
        shadow: &r.shadow,
        snapshot: snap,
        timeout: Duration::from_secs(60),
        max_bytes: 1 << 30,
        max_entries: fork_copy::ENTRY_MAX,
    })
    .await
}

/// What a correct own copy must not have done or hold; empty when clean.
fn violations(r: &Rig, res: &Result<fork_copy::Filled, String>) -> Vec<String> {
    let mut v = Vec::new();
    let marked = r.h.marked();
    if !marked.is_empty() {
        v.push(format!("the parent's repository ran: {marked:?}"));
    }
    if digest(&r.h.repo) != r.before {
        v.push("the parent's folder or repository was written".into());
    }
    let git = r.dest.join(".git");
    if git.exists() {
        let hooks = git.join("hooks");
        if hooks.exists() && std::fs::read_dir(&hooks).unwrap().next().is_some() {
            v.push("the copy has hooks".into());
        }
        for e in walkdir::WalkDir::new(git.join("objects"))
            .into_iter()
            .flatten()
        {
            if e.file_type().is_file() && e.metadata().unwrap().nlink() > 1 {
                v.push(format!(
                    "{} is a hard link to a file of the parent",
                    e.path().display()
                ));
                break;
            }
        }
        if let Ok(cfg) = std::fs::read_to_string(git.join("config")) {
            for bad in [
                "hooksPath",
                "fsmonitor",
                "include",
                "filter",
                "alias",
                "sshCommand",
                "pager",
                "editor",
                "credential",
                "gpg",
                "textconv",
                "insteadOf",
                "template",
                "worktree",
                "packObjectsHook",
            ] {
                if cfg.contains(bad) {
                    v.push(format!("the copy's config holds {bad:?}"));
                }
            }
        }
    }
    if let Ok(f) = res
        && f.commit != r.head
    {
        v.push(format!("branch at {} not {}", f.commit, r.head));
    }
    v
}

#[tokio::test]
async fn a_hostile_repository_runs_nothing_and_is_copied_correctly() {
    let r = rig(true).await;
    let res = run(&r, Some(&r.head)).await;
    let f = res.as_ref().unwrap_or_else(|e| panic!("refused: {e}"));
    assert_eq!(violations(&r, &res), Vec::<String>::new());
    assert!(f.head_exact);
    assert_eq!(
        f.upstream.as_deref(),
        Some("https://example.invalid/team/proj.git")
    );
    // The files are the cut's: uncommitted work, the untracked file, the
    // file the parent deleted since, not the ignored file, not the later
    // commit's file.
    let d = &r.dest;
    assert_eq!(std::fs::read_to_string(d.join("a.txt")).unwrap(), "two\n");
    assert_eq!(std::fs::read_to_string(d.join("b.txt")).unwrap(), "bee\n");
    assert_eq!(std::fs::read_to_string(d.join("new.txt")).unwrap(), "new\n");
    assert!(!d.join("ignored.log").exists());
    assert!(!d.join("later.txt").exists());
    // The branch is the cut's commit, not the parent's later one.
    assert_eq!(out(d, &["rev-parse", "HEAD"]), r.head);
    assert_eq!(
        out(d, &["symbolic-ref", "HEAD"]),
        "refs/heads/fork/ab12cd34"
    );
    assert!(
        !out(d, &["rev-list", "--all"]).is_empty()
            && !out(d, &["rev-list", "HEAD"]).contains(&r.later)
    );
    assert_eq!(out(d, &["branch", "--list", "main"]), "");
    assert_eq!(
        out(d, &["remote", "get-url", "origin"]),
        r.h.repo.display().to_string()
    );
    assert_eq!(
        out(d, &["remote", "get-url", "upstream"]),
        "https://example.invalid/team/proj.git"
    );
    // The parent's uncommitted work is uncommitted here.
    assert_eq!(
        out(d, &["status", "--porcelain"]),
        " M a.txt\n?? inner.git/\n?? new.txt"
    );
    // The bare repository inside the tree arrived as plain files.
    assert!(d.join("inner.git/hooks/post-checkout").is_file());
    // Plain git in the copy runs nothing either: its config is ours.
    assert_eq!(r.h.marked(), Vec::<String>::new());
    // The parent's info/exclude came along (patterns only).
    assert_eq!(
        std::fs::read_to_string(d.join(".git/info/exclude")).unwrap(),
        "scratch/\n"
    );
}

#[tokio::test]
async fn without_a_checkpoint_the_branchs_commit_is_written_as_it_is() {
    let r = rig(false).await;
    let res = run(&r, Some(&r.head)).await;
    res.as_ref().unwrap_or_else(|e| panic!("refused: {e}"));
    assert_eq!(violations(&r, &res), Vec::<String>::new());
    let d = &r.dest;
    // The committed files at the cut's commit; none of the parent's
    // uncommitted work.
    assert_eq!(std::fs::read_to_string(d.join("a.txt")).unwrap(), "one\n");
    assert!(!d.join("new.txt").exists() && !d.join("later.txt").exists());
    assert_eq!(out(d, &["status", "--porcelain"]), "");
    assert_eq!(r.h.marked(), Vec::<String>::new());
}

#[tokio::test]
async fn a_commit_the_log_names_that_is_gone_falls_back_to_the_parents_head() {
    let r = rig(true).await;
    let res = run(&r, Some(&"1".repeat(40))).await;
    let f = res.unwrap();
    assert!(!f.head_exact);
    assert_eq!(f.commit, r.later);
    assert_eq!(r.h.marked(), Vec::<String>::new());
}

/// An odd shape of the parent's `.git`: made after the hostile config.
fn shaped(r: &Rig, how: &str) {
    let git = r.h.repo.join(".git");
    let root = r.h.repo.parent().unwrap().to_path_buf();
    use std::os::unix::fs::symlink;
    match how {
        "objects symlink" => {
            std::fs::rename(git.join("objects"), root.join("objstore")).unwrap();
            symlink(root.join("objstore"), git.join("objects")).unwrap();
        }
        "refs symlink" => {
            std::fs::rename(git.join("refs"), root.join("refsstore")).unwrap();
            symlink(root.join("refsstore"), git.join("refs")).unwrap();
        }
        "gitfile" => {
            std::fs::rename(&git, root.join("gitdir")).unwrap();
            std::fs::write(
                r.h.repo.join(".git"),
                format!("gitdir: {}\n", root.join("gitdir").display()),
            )
            .unwrap();
        }
        "commondir" => {
            std::fs::write(git.join("commondir"), "../elsewhere\n").unwrap();
        }
        "alternates outside" => {
            std::fs::create_dir_all(root.join("otherstore")).unwrap();
            std::fs::write(
                git.join("objects/info/alternates"),
                format!("{}\n", root.join("otherstore").display()),
            )
            .unwrap();
        }
        "head nowhere" => {
            std::fs::write(git.join("HEAD"), "ref: refs/heads/nowhere\n").unwrap();
        }
        "head garbage" => {
            std::fs::write(git.join("HEAD"), "garbage\n").unwrap();
        }
        "git symlink to elsewhere" => {
            std::fs::rename(&git, root.join("gitdir")).unwrap();
            symlink(root.join("gitdir"), r.h.repo.join(".git")).unwrap();
        }
        "packed refs huge" => {
            std::fs::write(git.join("packed-refs"), "# pack\n".repeat(400_000)).unwrap();
        }
        other => panic!("{other}"),
    }
}

#[tokio::test]
async fn odd_shapes_end_in_a_clean_refusal_or_a_correct_copy() {
    for (how, must_refuse) in [
        ("objects symlink", true),
        ("refs symlink", true),
        ("gitfile", true),
        ("commondir", true),
        ("alternates outside", true),
        ("head nowhere", true),
        ("head garbage", true),
        ("git symlink to elsewhere", true),
        ("packed refs huge", true),
    ] {
        let r = rig(true).await;
        shaped(&r, how);
        let before = digest(&r.h.repo);
        let res = run(&r, Some(&r.head)).await;
        assert_eq!(r.h.marked(), Vec::<String>::new(), "{how}: ran something");
        assert_eq!(digest(&r.h.repo), before, "{how}: wrote the parent");
        match &res {
            Ok(_) => assert!(!must_refuse, "{how}: was copied"),
            Err(e) => {
                assert!(must_refuse, "{how}: refused: {e}");
                // Refused before the clone touched anything.
                assert_eq!(
                    std::fs::read_dir(&r.dest).unwrap().count(),
                    0,
                    "{how}: left files"
                );
                assert!(!e.is_empty());
            }
        }
        if res.is_ok() {
            // The bare repository's files are plain files in the copy and
            // the copy's git never read them.
            let d = &r.dest;
            assert_eq!(
                out(d, &["rev-parse", "HEAD"]),
                r.head,
                "{how}: not at the branch"
            );
            assert_eq!(
                r.h.marked(),
                Vec::<String>::new(),
                "{how}: git in the copy ran something"
            );
        }
    }
}

/// A repository whose own HEAD, config and hooks are fine, but whose
/// `origin` is something git must not be handed as a remote.
#[tokio::test]
async fn the_parents_origin_url_is_data_and_only_a_plain_one_is_kept() {
    for (url, kept) in [
        ("https://example.invalid/a/b.git", true),
        ("ssh://git@example.invalid/a/b.git", true),
        ("git@example.invalid:a/b.git", true),
        ("/srv/git/b.git", true),
        ("file:///srv/git/b.git", true),
        ("ext::sh -c 'touch x'", false),
        ("ext::touch%x", false),
        ("-oProxyCommand=touch x", false),
        ("--upload-pack=touch x", false),
        ("git://example.invalid/b.git", false),
        ("http://example.invalid/b.git", false),
        ("fd::17", false),
        ("https://exa\u{1b}mple.invalid/b.git", false),
        ("https://example.invalid/a b.git", false),
        ("", false),
        ("relative/path", false),
    ] {
        assert_eq!(upstream_url(url).is_some(), kept, "{url:?}");
    }
    let mut r = rig(true).await;
    // A hostile URL in the parent's config: the copy has no `upstream`.
    let cfg = r.h.repo.join(".git/config");
    let text = std::fs::read_to_string(&cfg).unwrap().replace(
        "https://example.invalid/team/proj.git",
        "ext::sh -c touch-x",
    );
    std::fs::write(&cfg, text).unwrap();
    r.before = digest(&r.h.repo);
    let res = run(&r, Some(&r.head)).await;
    assert_eq!(res.as_ref().unwrap().upstream, None);
    assert_eq!(violations(&r, &res), Vec::<String>::new());
    assert!(
        !std::fs::read_to_string(r.dest.join(".git/config"))
            .unwrap()
            .contains("upstream")
    );
}

#[test]
fn the_minimum_git_is_the_release_with_the_clone_fixes() {
    assert_eq!(parse_version("git version 2.56.1\n"), Some((2, 56, 1)));
    assert_eq!(
        parse_version("git version 2.39.5 (Apple Git-154)"),
        Some((2, 39, 5))
    );
    assert_eq!(parse_version("git version 2.45"), Some((2, 45, 0)));
    assert_eq!(parse_version("nonsense"), None);
    for ok in [
        (2, 45, 1),
        (2, 46, 0),
        (3, 0, 0),
        (2, 39, 4),
        (2, 40, 2),
        (2, 41, 1),
        (2, 42, 2),
        (2, 43, 4),
        (2, 44, 1),
    ] {
        assert!(patched(ok), "{ok:?}");
    }
    for old in [
        (2, 45, 0),
        (2, 39, 3),
        (2, 40, 1),
        (2, 41, 0),
        (2, 38, 9),
        (2, 30, 0),
        (1, 9, 9),
    ] {
        assert!(!patched(old), "{old:?}");
    }
}

#[tokio::test]
async fn an_old_git_is_refused_with_its_version_and_nothing_is_written() {
    let r = rig(true).await;
    fork_copy::pretend_version(Some((2, 34, 1)));
    let res = run(&r, Some(&r.head)).await;
    fork_copy::pretend_version(None);
    let e = res.unwrap_err();
    assert!(e.contains("2.34.1") && e.contains("2.45.1"), "{e}");
    assert_eq!(std::fs::read_dir(&r.dest).unwrap().count(), 0);
}

/// Stage 2 runs in the node's jail when the node has one: the wrapper is
/// the program, and the only folders it is given read-write are the
/// sanitised copy's and the destination, never the parent or the staging
/// folder as a whole. (A real jail is run by
/// `crates/cli/tests/fork_copy_sandbox.rs`.)
#[tokio::test]
async fn the_clone_is_given_the_jail_with_only_its_own_folders() {
    use harness_tools::sandbox::{Sandbox, Settings};
    let r = rig(true).await;
    let mut shadow = r.shadow.clone();
    shadow.jail = Jail(Some(std::sync::Arc::new(
        Sandbox::new("/bin/true".into(), &Settings::default(), None, &[]).unwrap(),
    )));
    let plan = Plan {
        source: &r.h.repo,
        dest: &r.dest,
        outside: &r.outside,
        branch: "fork/ab12cd34",
        head: None,
        shadow: &shadow,
        snapshot: None,
        timeout: Duration::from_secs(60),
        max_bytes: 1 << 30,
        max_entries: fork_copy::ENTRY_MAX,
    };
    let (argv, work) = fork_copy::clone_command_for_test(&plan);
    let all = argv.join("\n");
    assert!(
        argv.iter().any(|a| a == "sandbox-exec"),
        "not through the wrapper: {all}"
    );
    let granted: Vec<String> = argv
        .iter()
        .zip(argv.iter().skip(1))
        .zip(argv.iter().skip(2))
        .filter(|((a, _), _)| *a == "--rw")
        .map(|((_, id), path)| format!("{id} {path}"))
        .collect();
    let writable = |p: &Path| {
        granted
            .iter()
            .any(|g| g.ends_with(&p.display().to_string()))
    };
    assert!(writable(&work) && writable(&r.dest), "{all}");
    assert!(!writable(&r.outside) && !writable(&r.h.repo), "{all}");
    assert!(
        !all.contains(&r.h.repo.display().to_string()),
        "the parent is named: {all}"
    );
    assert!(
        all.contains("--no-local") && all.contains("--no-checkout") && all.contains("--template="),
        "{all}"
    );
}

/// The test above must be able to fail: a copy made by `git -C <parent>
/// worktree add` runs the parent's hooks.
#[tokio::test]
async fn the_test_notices_a_worktree_add() {
    let r = rig(true).await;
    mutate(Mutation::WorktreeAdd);
    let res = run(&r, Some(&r.head)).await;
    mutate(Mutation::None);
    let v = violations(&r, &res);
    assert!(
        v.iter().any(|x| x.contains("ran")),
        "worktree add ran nothing? {v:?} / {res:?}"
    );
}

/// ... and a clone without `--no-local`, `--no-checkout` and `--template=`
/// runs the node's template hook (a template directory a node may have) and
/// hard-links the parent's objects.
#[tokio::test]
async fn the_test_notices_a_plain_clone() {
    let r = rig(true).await;
    mutate(Mutation::PlainClone);
    let res = run(&r, Some(&r.head)).await;
    mutate(Mutation::None);
    let v = violations(&r, &res);
    assert!(v.iter().any(|x| x.contains("ran")), "{v:?} / {res:?}");
    assert!(v.iter().any(|x| x.contains("hard link")), "{v:?} / {res:?}");
}

/// For the notes: the same checks as the first test, with the clone
/// replaced when `HARNESS_FORK_TEST_MUTATION` says so (run by hand).
#[tokio::test]
#[ignore = "run by hand with HARNESS_FORK_TEST_MUTATION=worktree|plain"]
async fn the_real_test_with_a_wrong_clone() {
    let r = rig(true).await;
    match std::env::var("HARNESS_FORK_TEST_MUTATION").as_deref() {
        Ok("worktree") => mutate(Mutation::WorktreeAdd),
        Ok("plain") => mutate(Mutation::PlainClone),
        _ => {}
    }
    let res = run(&r, Some(&r.head)).await;
    assert_eq!(violations(&r, &res), Vec::<String>::new());
}

/// The blocker of the review: git must never open the parent's repository,
/// and the harness must never follow a link inside it. Every case plants
/// something an agent in the parent's folder can write (a link to another
/// repository on the disk, a special file, a huge config) and checks that
/// nothing of the victim reaches the fork and that the end is a plain
/// refusal naming the path.
mod leaks {
    use super::*;
    use std::os::unix::fs::symlink;

    const SECRET: &str = "TOPSECRET-VICTIM";

    fn g(dir: &Path, args: &[&str]) -> String {
        let o = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .output()
            .unwrap();
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }

    /// Another repository on the disk, with a secret in its history and in
    /// its `origin` URL.
    struct Victim {
        dir: PathBuf,
        head: String,
    }

    fn victim(r: &Rig, pack: bool) -> Victim {
        let v = r.top.join("victim");
        std::fs::create_dir_all(&v).unwrap();
        g(&v, &["init", "-q", "-b", "main"]);
        std::fs::write(v.join("secret.txt"), format!("{SECRET}\n")).unwrap();
        g(&v, &["add", "."]);
        g(&v, &["commit", "-qm", "victim"]);
        g(
            &v,
            &[
                "config",
                "remote.origin.url",
                &format!("https://u:{SECRET}@h.invalid/x.git"),
            ],
        );
        if pack {
            g(&v, &["gc", "-q"]);
        }
        let head = g(&v, &["rev-parse", "HEAD"]);
        Victim { dir: v, head }
    }

    impl Victim {
        fn git(&self) -> PathBuf {
            self.dir.join(".git")
        }
    }

    /// The pack files of the victim, linked into the parent's store.
    fn pack_links(r: &Rig, v: &Victim) {
        let pd = r.h.repo.join(".git/objects/pack");
        std::fs::create_dir_all(&pd).unwrap();
        for e in std::fs::read_dir(v.git().join("objects/pack")).unwrap() {
            let p = e.unwrap().path();
            symlink(&p, pd.join(p.file_name().unwrap())).unwrap();
        }
    }

    /// A loose ref (a plain file) naming the victim's commit: an agent that
    /// knows the id can write it.
    fn ref_naming(r: &Rig, v: &Victim, name: &str) {
        std::fs::write(r.h.repo.join(".git").join(name), format!("{}\n", v.head)).unwrap();
    }

    /// The branch-hiding config of the hostile rig would hide the planted
    /// ref from `upload-pack`; the leak does not need it.
    fn unhide(r: &Rig) {
        g(&r.h.repo, &["config", "--unset-all", "transfer.hideRefs"]);
    }

    /// Where anything of the victim is in the fork.
    fn leaked(dest: &Path, v: &Victim) -> Vec<String> {
        let mut hits = Vec::new();
        let o = std::process::Command::new("git")
            .arg("-C")
            .arg(dest)
            .args(["cat-file", "--batch-all-objects", "--batch"])
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output();
        if let Ok(o) = o {
            let text = String::from_utf8_lossy(&o.stdout).into_owned();
            if text.contains(SECRET) || text.contains(&v.head) {
                hits.push("the object store".to_string());
            }
        }
        for e in walkdir::WalkDir::new(dest)
            .follow_links(false)
            .into_iter()
            .flatten()
        {
            if e.file_type().is_file()
                && !e.path().starts_with(dest.join(".git/objects"))
                && let Ok(b) = std::fs::read(e.path())
            {
                let t = String::from_utf8_lossy(&b);
                if t.contains(SECRET) || t.contains(&v.head) {
                    hits.push(e.path().display().to_string());
                }
            }
        }
        hits
    }

    fn append(p: PathBuf, text: &str) {
        use std::io::Write;
        std::fs::OpenOptions::new()
            .append(true)
            .open(p)
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }

    /// Plant, copy, and check: nothing of the victim in the fork, a plain
    /// refusal that names `hint`, nothing left in the destination, nothing
    /// run, the parent untouched.
    async fn refused(name: &str, hint: &str, pack: bool, setup: impl FnOnce(&Rig, &Victim)) {
        let r = rig(true).await;
        unhide(&r);
        let v = victim(&r, pack);
        setup(&r, &v);
        for e in std::fs::read_dir(r.top.join("marks")).unwrap() {
            std::fs::remove_file(e.unwrap().path()).unwrap();
        }
        let before = digest(&r.h.repo);
        let res = run(&r, Some(&r.head)).await;
        let leak = leaked(&r.dest, &v);
        assert!(
            leak.is_empty(),
            "{name}: the victim is in the fork: {leak:?} / {res:?}"
        );
        let e = res
            .err()
            .unwrap_or_else(|| panic!("{name}: was copied, not refused"));
        assert!(
            e.contains(hint),
            "{name}: the refusal does not name {hint:?}: {e}"
        );
        assert!(!e.contains(SECRET), "{name}: {e}");
        assert_eq!(
            std::fs::read_dir(&r.dest).unwrap().count(),
            0,
            "{name}: left files in the destination"
        );
        assert_eq!(r.h.marked(), Vec::<String>::new(), "{name}: ran something");
        assert_eq!(digest(&r.h.repo), before, "{name}: wrote the parent");
    }

    #[tokio::test]
    async fn pack_files_that_are_links_to_another_repository_are_refused() {
        refused("pack links", "objects/pack/pack-", true, |r, v| {
            pack_links(r, v);
            ref_naming(r, v, "refs/heads/leak");
        })
        .await;
    }

    #[tokio::test]
    async fn a_ref_that_is_a_link_to_another_repository_is_refused() {
        refused("ref link", "refs/heads/leak", true, |r, v| {
            pack_links(r, v);
            symlink(
                v.git().join("refs/heads/main"),
                r.h.repo.join(".git/refs/heads/leak"),
            )
            .unwrap();
        })
        .await;
    }

    #[tokio::test]
    async fn relative_links_are_refused_like_absolute_ones() {
        refused("relative links", "refs/heads/leak", true, |r, v| {
            pack_links(r, v);
            symlink(
                "../../../../victim/.git/refs/heads/main",
                r.h.repo.join(".git/refs/heads/leak"),
            )
            .unwrap();
        })
        .await;
    }

    #[tokio::test]
    async fn objects_pack_as_a_link_is_refused() {
        refused("objects/pack link", "objects/pack", true, |r, v| {
            let pd = r.h.repo.join(".git/objects/pack");
            let _ = std::fs::remove_dir_all(&pd);
            symlink(v.git().join("objects/pack"), &pd).unwrap();
            ref_naming(r, v, "refs/heads/leak");
        })
        .await;
    }

    #[tokio::test]
    async fn a_folder_of_refs_that_is_a_link_is_refused() {
        refused("refs/heads/dd link", "refs/heads/dd", true, |r, v| {
            pack_links(r, v);
            let d = v.git().join("refs/heads/d");
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("x"), format!("{}\n", v.head)).unwrap();
            symlink(&d, r.h.repo.join(".git/refs/heads/dd")).unwrap();
        })
        .await;
    }

    #[tokio::test]
    async fn loose_objects_that_are_links_are_refused() {
        refused("loose object links", "objects/", false, |r, v| {
            let git = v.git();
            for o in [
                v.head.clone(),
                g(&v.dir, &["rev-parse", "HEAD^{tree}"]),
                g(&v.dir, &["rev-parse", "HEAD:secret.txt"]),
            ] {
                let d = r.h.repo.join(".git/objects").join(&o[..2]);
                std::fs::create_dir_all(&d).unwrap();
                symlink(
                    git.join("objects").join(&o[..2]).join(&o[2..]),
                    d.join(&o[2..]),
                )
                .unwrap();
            }
            ref_naming(r, v, "refs/heads/leak");
        })
        .await;
    }

    #[tokio::test]
    async fn a_config_that_is_a_link_to_another_repositorys_config_is_refused() {
        refused("config link", "config", true, |r, v| {
            let c = r.h.repo.join(".git/config");
            std::fs::remove_file(&c).unwrap();
            symlink(v.git().join("config"), &c).unwrap();
        })
        .await;
    }

    #[tokio::test]
    async fn packed_refs_that_is_a_link_is_refused() {
        refused("packed-refs link", "packed-refs", true, |r, v| {
            std::fs::write(
                v.git().join("packed-refs"),
                format!("{} refs/heads/zz\n", v.head),
            )
            .unwrap();
            symlink(
                v.git().join("packed-refs"),
                r.h.repo.join(".git/packed-refs"),
            )
            .unwrap();
        })
        .await;
    }

    /// A link at the top of the parent's `.git` entries that are not
    /// copied (hooks, info, logs) does not matter: they are not looked at.
    #[tokio::test]
    async fn links_in_what_is_not_copied_are_not_even_looked_at() {
        let r = rig(true).await;
        let v = victim(&r, true);
        let git = r.h.repo.join(".git");
        for (name, to) in [
            ("logs", v.git().join("objects")),
            ("modules", v.dir.clone()),
            ("config.worktree", v.git().join("config")),
            ("index", v.git().join("config")),
        ] {
            let at = git.join(name);
            let _ = std::fs::remove_dir_all(&at);
            let _ = std::fs::remove_file(&at);
            symlink(to, at).unwrap();
        }
        let res = run(&r, Some(&r.head)).await;
        res.as_ref().unwrap_or_else(|e| panic!("refused: {e}"));
        assert_eq!(leaked(&r.dest, &v), Vec::<String>::new());
    }

    /// `core.attributesFile` and `info/grafts` of the parent are read by
    /// upload-pack in a git process inside the parent; here nothing reads
    /// them, so nothing of them is in any message.
    #[tokio::test]
    async fn the_parents_attributes_file_and_grafts_are_never_read() {
        let r = rig(true).await;
        std::fs::write(r.h.repo.join(".git/info/grafts"), format!("{} \n", r.head)).unwrap();
        let before = digest(&r.h.repo);
        let res = run(&r, Some(&r.head)).await;
        let text = format!("{res:?}");
        assert!(!text.contains("attrsfile"), "{text}");
        assert!(!text.contains("marks"), "{text}");
        assert_eq!(r.h.marked(), Vec::<String>::new());
        assert_eq!(digest(&r.h.repo), before);
        // The grafts file is not copied: the fork's history is the real one.
        let f = res.unwrap_or_else(|e| panic!("refused: {e}"));
        assert_eq!(f.commit, r.head);
        assert!(!r.dest.join(".git/info/grafts").exists());
    }

    /// Something at each path the copy reads, in turn, that is not a plain
    /// file: refused at once (never waited on), naming the path.
    #[tokio::test]
    async fn special_files_at_every_copied_path_are_refused_at_once() {
        use std::os::unix::net::UnixListener;
        let cases: &[(&str, &str)] = &[
            ("HEAD", "fifo"),
            ("packed-refs", "fifo"),
            ("shallow", "fifo"),
            ("config", "fifo"),
            ("info/exclude", "fifo"),
            ("refs/heads/fifo", "fifo"),
            ("refs/tags/sock", "socket"),
            ("objects/ab/cdef", "fifo"),
            ("objects/pack/pack-x.pack", "fifo"),
            ("objects/info/alternates", "fifo"),
            ("objects/info/packs", "fifo"),
        ];
        for (path, kind) in cases {
            let r = rig(true).await;
            let p = r.h.repo.join(".git").join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            let _ = std::fs::remove_file(&p);
            let _sock;
            if *kind == "fifo" {
                nix::unistd::mkfifo(&p, nix::sys::stat::Mode::S_IRWXU).unwrap();
            } else {
                _sock = UnixListener::bind(&p).unwrap();
            }
            let snap = (!r.snap.is_empty()).then_some(r.snap.as_str());
            let t0 = std::time::Instant::now();
            let res = fill(&Plan {
                source: &r.h.repo,
                dest: &r.dest,
                outside: &r.outside,
                branch: "fork/ab12cd34",
                head: Some(&r.head),
                shadow: &r.shadow,
                snapshot: snap,
                timeout: Duration::from_secs(6),
                max_bytes: 1 << 30,
                max_entries: fork_copy::ENTRY_MAX,
            })
            .await;
            let took = t0.elapsed();
            let e = res.err().unwrap_or_else(|| panic!("{path}: was copied"));
            assert!(
                e.contains(path),
                "{path}: the refusal does not name it: {e}"
            );
            assert!(
                took < Duration::from_secs(4),
                "{path}: waited {took:?}: {e}"
            );
            assert!(!e.contains("took longer"), "{path}: {e}");
            assert_eq!(std::fs::read_dir(&r.dest).unwrap().count(), 0, "{path}");
        }
    }

    /// A config of 40 MB is not read: the cap refuses it, quickly.
    #[tokio::test]
    async fn a_config_over_the_cap_is_refused() {
        let r = rig(true).await;
        let line = format!("# {}\n", "x".repeat(78));
        append(r.h.repo.join(".git/config"), &line.repeat(500_000));
        let t0 = std::time::Instant::now();
        let res = run(&r, Some(&r.head)).await;
        let e = res
            .err()
            .unwrap_or_else(|| panic!("a 40 MB config was read"));
        assert!(e.contains("config") && e.contains("larger"), "{e}");
        assert!(t0.elapsed() < Duration::from_secs(5), "{:?}", t0.elapsed());
        assert_eq!(std::fs::read_dir(&r.dest).unwrap().count(), 0);
    }

    /// A repository the copy cannot make (sha256 objects, reftable refs) is
    /// refused with a sentence, from its config read as data.
    #[tokio::test]
    async fn sha256_and_reftable_parents_are_refused() {
        for (key, value, want) in [
            ("extensions.objectformat", "sha256", "sha256"),
            ("extensions.refstorage", "reftable", "reftable"),
        ] {
            let r = rig(true).await;
            g(&r.h.repo, &["config", "core.repositoryformatversion", "1"]);
            g(&r.h.repo, &["config", key, value]);
            let e = run(&r, Some(&r.head)).await.unwrap_err();
            assert!(e.contains(want) && e.contains("not copied"), "{key}: {e}");
            assert_eq!(std::fs::read_dir(&r.dest).unwrap().count(), 0);
        }
    }

    /// `include.path` / `includeIf` of the parent's config is not followed:
    /// a URL only an included file holds is not the fork's `upstream`.
    #[tokio::test]
    async fn the_parents_includes_are_not_followed() {
        let r = rig(true).await;
        let extra = r.top.join("included.config");
        std::fs::write(
            &extra,
            "[remote \"origin\"]\n\turl = https://included.invalid/x.git\n",
        )
        .unwrap();
        g(&r.h.repo, &["config", "--unset-all", "remote.origin.url"]);
        g(
            &r.h.repo,
            &[
                "config",
                "--replace-all",
                "include.path",
                &extra.to_string_lossy(),
            ],
        );
        let res = run(&r, Some(&r.head)).await;
        assert_eq!(res.unwrap().upstream, None);
        assert!(
            !std::fs::read_to_string(r.dest.join(".git/config"))
                .unwrap()
                .contains("included.invalid")
        );
    }

    /// Credentials in an https URL are the parent's, not the fork's.
    #[tokio::test]
    async fn userinfo_is_stripped_from_the_upstream_url() {
        let r = rig(true).await;
        g(
            &r.h.repo,
            &[
                "config",
                "remote.origin.url",
                "https://alice:s3cret-pw@example.invalid/team/proj.git",
            ],
        );
        let res = run(&r, Some(&r.head)).await;
        let f = res.unwrap();
        assert_eq!(
            f.upstream.as_deref(),
            Some("https://example.invalid/team/proj.git")
        );
        let cfg = std::fs::read_to_string(r.dest.join(".git/config")).unwrap();
        assert!(!cfg.contains("s3cret") && !cfg.contains("alice"), "{cfg}");
    }

    /// The mutation the review found uncaught: a git process that reads the
    /// parent's config (`git config` in or pointed at the parent). Every git
    /// the operation starts is logged by a stand-in `git`; none may name
    /// the parent's folder, in an argument, a working directory or the
    /// environment.
    #[tokio::test]
    async fn no_git_process_is_started_in_or_pointed_at_the_parent() {
        let r = rig(true).await;
        let real = String::from_utf8(
            std::process::Command::new("sh")
                .args(["-c", "command -v git"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        let log = r.top.join("git-calls.log");
        let script = r.top.join("fake-git/git");
        std::fs::create_dir_all(script.parent().unwrap()).unwrap();
        crate::test_script::script(
            &script,
            &format!(
                "#!/bin/sh\n{{ echo \"PWD=$PWD\"; echo \"GIT_DIR=$GIT_DIR\"; \
                 echo \"GIT_WORK_TREE=$GIT_WORK_TREE\"; for a in \"$@\"; do echo \"ARG=$a\"; done; \
                 echo ---; }} >> '{}'\nexec {} \"$@\"\n",
                log.display(),
                real.trim()
            ),
        );
        crate::git::use_test_git(Some(script));
        let res = run(&r, Some(&r.head)).await;
        crate::git::use_test_git(None);
        res.as_ref().unwrap_or_else(|e| panic!("refused: {e}"));
        let calls = std::fs::read_to_string(&log).unwrap_or_default();
        assert!(
            calls.contains("ARG=clone"),
            "the stand-in git was not used:\n{calls}"
        );
        let parent = r.h.repo.display().to_string();
        for call in calls.split("---\n").filter(|c| !c.trim().is_empty()) {
            let mut lines: Vec<&str> = call.lines().collect();
            // Setting the fork's `origin` to the parent's path is a value
            // written into the fork's own config, not an access to the
            // parent: the one argument after the key (and never a `--get`).
            if !lines.contains(&"ARG=--get")
                && let Some(i) = lines.iter().position(|l| *l == "ARG=remote.origin.url")
            {
                lines.remove(i + 1);
            }
            for l in lines {
                assert!(
                    !l.contains(&parent),
                    "a git process names the parent: {l}\n{call}"
                );
            }
        }
    }

    /// The bounds of stage 1: bytes and entries, each refused with its own
    /// sentence, and nothing left behind.
    #[tokio::test]
    async fn the_size_and_entry_bounds_refuse_the_copy() {
        for (bytes, entries, want) in [
            (300u64, fork_copy::ENTRY_MAX, "clone_max_mb"),
            (1 << 30, 5, "more than 5"),
        ] {
            let r = rig(true).await;
            let snap = (!r.snap.is_empty()).then_some(r.snap.as_str());
            let res = fill(&Plan {
                source: &r.h.repo,
                dest: &r.dest,
                outside: &r.outside,
                branch: "fork/ab12cd34",
                head: Some(&r.head),
                shadow: &r.shadow,
                snapshot: snap,
                timeout: Duration::from_secs(60),
                max_bytes: bytes,
                max_entries: entries,
            })
            .await;
            let e = res.err().unwrap_or_else(|| panic!("{want}: was copied"));
            assert!(e.contains(want), "{want}: {e}");
            assert_eq!(std::fs::read_dir(&r.dest).unwrap().count(), 0, "{want}");
            assert_eq!(
                std::fs::read_dir(&r.outside)
                    .unwrap()
                    .flatten()
                    .filter(|e| e.file_name().to_string_lossy().starts_with("forksrc-"))
                    .count(),
                0,
                "{want}: the sanitised copy was left in staging"
            );
        }
    }

    /// A parent that goes on working while it is copied (commits, refs,
    /// repacks, packed refs): the fork is a correct repository or the copy
    /// is refused, never a ref without its objects.
    #[tokio::test]
    async fn a_parent_that_keeps_working_gives_a_correct_fork_or_a_refusal() {
        let mut oks = 0;
        for round in 0..4 {
            let h = Hostile::new();
            let repo = h.repo.clone();
            let top = repo.parent().unwrap().canonicalize().unwrap();
            for i in 0..60 {
                std::fs::write(
                    repo.join(format!("f{i}.txt")),
                    format!("{i}\n").repeat(2000),
                )
                .unwrap();
                sh(&repo, &["add", "."]);
                sh(&repo, &["commit", "-qm", &format!("c{i}")]);
            }
            let head = out(&repo, &["rev-parse", "HEAD"]);
            let shadow = repo_for(&repo, Some(&h.base)).await.unwrap();
            let dest = top.join("dest");
            let outside = top.join("outside");
            std::fs::create_dir_all(&dest).unwrap();
            std::fs::create_dir_all(&outside).unwrap();
            let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let worker = {
                let (stop, repo) = (stop.clone(), repo.clone());
                std::thread::spawn(move || {
                    let git = |args: &[&str]| {
                        let _ = std::process::Command::new("git")
                            .arg("-C")
                            .arg(&repo)
                            .args(args)
                            .env("GIT_CONFIG_GLOBAL", "/dev/null")
                            .env("GIT_CONFIG_NOSYSTEM", "1")
                            .env("GIT_AUTHOR_NAME", "t")
                            .env("GIT_AUTHOR_EMAIL", "t@t")
                            .env("GIT_COMMITTER_NAME", "t")
                            .env("GIT_COMMITTER_EMAIL", "t@t")
                            .output();
                    };
                    let mut i = 0;
                    while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                        let _ = std::fs::write(repo.join(format!("g{i}.txt")), format!("{i}\n"));
                        git(&["add", "."]);
                        git(&["commit", "-qm", &format!("w{i}")]);
                        git(&["update-ref", &format!("refs/heads/w{i}"), "HEAD"]);
                        match i % 7 {
                            3 => git(&["pack-refs", "--all"]),
                            5 => git(&["repack", "-adq"]),
                            _ => {}
                        }
                        i += 1;
                    }
                })
            };
            let res = fill(&Plan {
                source: &repo,
                dest: &dest,
                outside: &outside,
                branch: "fork/ab12cd34",
                head: Some(&head),
                shadow: &shadow,
                snapshot: None,
                timeout: Duration::from_secs(60),
                max_bytes: 1 << 30,
                max_entries: fork_copy::ENTRY_MAX,
            })
            .await;
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
            worker.join().unwrap();
            match res {
                Err(e) => println!("round {round}: refused: {e}"),
                Ok(_) => {
                    oks += 1;
                    let refs = out(&dest, &["for-each-ref", "--format=%(objectname)"]);
                    for id in refs.lines() {
                        assert!(
                            std::process::Command::new("git")
                                .arg("-C")
                                .arg(&dest)
                                .args(["cat-file", "-e", id])
                                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                                .status()
                                .unwrap()
                                .success(),
                            "round {round}: a ref names {id}, which the fork lacks"
                        );
                    }
                    let fsck = std::process::Command::new("git")
                        .arg("-C")
                        .arg(&dest)
                        .args(["rev-list", "--objects", "--all"])
                        .env("GIT_CONFIG_GLOBAL", "/dev/null")
                        .output()
                        .unwrap();
                    assert!(fsck.status.success(), "round {round}: {fsck:?}");
                }
            }
        }
        println!("{oks} of 4 copies made while the parent worked");
    }
}
