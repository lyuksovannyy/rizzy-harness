//! A fork's own copy of the parent's repository, with the node's git in the
//! Landlock sandbox through the real `rizzy-harness sandbox-exec` wrapper
//! (PLAN.md T92). The clone of stage 2 runs in the jail with only the
//! folders of this copy visible read-write; the parent's repository is not
//! one of them (and git is never given its path). On a kernel without
//! Landlock the test skips with a printed reason.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use harness_tools::sandbox::{Sandbox, Settings};

const EXE: &str = env!("CARGO_BIN_EXE_rizzy-harness");

fn git(dir: &Path, args: &[&str]) -> String {
    let o = std::process::Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("XDG_CONFIG_HOME", "/nonexistent")
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn sandbox(data: &Path) -> Arc<Sandbox> {
    Arc::new(
        Sandbox::new(
            PathBuf::from(EXE),
            &Settings::default(),
            Some(data.join("sandbox-tmp")),
            std::slice::from_ref(&data.to_path_buf()),
        )
        .unwrap(),
    )
}

#[tokio::test]
async fn the_clone_of_a_fork_copy_works_in_the_jail_and_holds_only_the_parents_history() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let (data, proj, victim) = (
        root.join("data"),
        root.join("home/proj"),
        root.join("home/victim"),
    );
    for d in [&data, &proj, &victim] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(proj.join("a.txt"), "one\n").unwrap();
    git(&proj, &["init", "-q", "-b", "main"]);
    git(&proj, &["add", "."]);
    git(&proj, &["commit", "-qm", "base"]);
    let head = git(&proj, &["rev-parse", "HEAD"]);
    std::fs::write(victim.join("secret.txt"), "CANARY-VICTIM\n").unwrap();
    git(&victim, &["init", "-q", "-b", "main"]);
    git(&victim, &["add", "."]);
    git(&victim, &["commit", "-qm", "victim"]);
    git(&victim, &["gc", "-q"]);
    // A planted link to the victim's packs, and a ref naming its commit.
    let victim_head = git(&victim, &["rev-parse", "HEAD"]);
    let sb = sandbox(&data);
    if let Err(e) = sb.probe(&data.join("sandbox-tmp/probe"), &data) {
        eprintln!("SKIPPED: Landlock does not work here: {e}");
        return;
    }
    let base = data.join("checkpoints");
    let protected = vec![data.clone()];
    let (dest, outside) = (root.join("dest"), data.join("staging"));
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    // The honest copy: made in the jail, the branch at the parent's HEAD.
    let commit = harness_node::testing::fork_copy(
        &proj,
        &base,
        Some(sb.clone()),
        &protected,
        &dest,
        &outside,
    )
    .await
    .unwrap_or_else(|e| panic!("refused in the jail: {e}"));
    assert_eq!(commit, head);
    assert_eq!(
        std::fs::read_to_string(dest.join("a.txt")).unwrap(),
        "one\n"
    );
    assert_eq!(git(&dest, &["rev-parse", "HEAD"]), head);
    // The sanitised copy is gone from staging.
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
    // Planted links: refused, nothing of the victim in a second fork.
    let pd = proj.join(".git/objects/pack");
    std::fs::create_dir_all(&pd).unwrap();
    for e in std::fs::read_dir(victim.join(".git/objects/pack")).unwrap() {
        let p = e.unwrap().path();
        std::os::unix::fs::symlink(&p, pd.join(p.file_name().unwrap())).unwrap();
    }
    std::fs::write(
        proj.join(".git/refs/heads/leak"),
        format!("{victim_head}\n"),
    )
    .unwrap();
    let dest2 = root.join("dest2");
    std::fs::create_dir_all(&dest2).unwrap();
    let e = harness_node::testing::fork_copy(&proj, &base, Some(sb), &protected, &dest2, &outside)
        .await
        .unwrap_err();
    assert!(e.contains("objects/pack/"), "{e}");
    assert_eq!(std::fs::read_dir(&dest2).unwrap().count(), 0);
}
