//! The node's own git reads a session's folder in the Landlock sandbox
//! (R.1, R1b-2 of docs/security-review/2026-11-gate3-fixes-3.md), through
//! the real `rizzy-harness sandbox-exec` wrapper. On a kernel without
//! Landlock the tests skip with a printed reason.
//!
//! The attack: a process the session left running swaps a folder inside
//! the session's folder for a link to a folder outside it, over and over,
//! while the node snapshots the folder (a turn's checkpoint; a sub-agent's
//! base, which is written straight into the sub-agent's folder). Each try
//! counts as a hit when the outside file's content is in the snapshot or in
//! the sub-agent's folder. Run unconfined (as before R1b-2) and confined.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use harness_tools::sandbox::{Sandbox, Settings};

const EXE: &str = env!("CARGO_BIN_EXE_rizzy-harness");

fn git(dir: &Path, args: &[&str]) {
    let o = std::process::Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}

struct Fixture {
    _dir: tempfile::TempDir,
    data: PathBuf,
    proj: PathBuf,
    victim: PathBuf,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let data = root.join("data");
    let proj = root.join("home/proj");
    let victim = root.join("home/victim");
    std::fs::create_dir_all(proj.join("d")).unwrap();
    std::fs::create_dir_all(&victim).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(victim.join("x"), "CANARY-OUTSIDE\n").unwrap();
    std::fs::write(data.join("x"), "CANARY-DATA-DIRECTORY\n").unwrap();
    std::fs::write(proj.join("d/x"), "inside\n").unwrap();
    git(&proj, &["init", "-q"]);
    git(&proj, &["add", "."]);
    git(&proj, &["commit", "-qm", "base"]);
    Fixture {
        _dir: dir,
        data,
        proj,
        victim,
    }
}

impl Fixture {
    fn sandbox(&self) -> Arc<Sandbox> {
        Arc::new(
            Sandbox::new(
                PathBuf::from(EXE),
                &Settings::default(),
                Some(self.data.join("sandbox-tmp")),
                std::slice::from_ref(&self.data),
            )
            .unwrap(),
        )
    }

    fn landlock(&self) -> bool {
        match self
            .sandbox()
            .probe(&self.data.join("sandbox-tmp/probe"), &self.data)
        {
            Ok(_) => true,
            Err(e) => {
                eprintln!("SKIPPED: Landlock does not work here: {e}");
                false
            }
        }
    }

    /// A thread that swaps `d` for a link to the outside folder or the data
    /// directory, in turn, until told to stop.
    fn swapper(&self, stop: Arc<AtomicBool>) -> std::thread::JoinHandle<()> {
        let (d, away) = (self.proj.join("d"), self.proj.join("d.r"));
        let targets = [self.victim.clone(), self.data.clone()];
        std::thread::spawn(move || {
            let mut i = 0usize;
            while !stop.load(Ordering::Relaxed) {
                if std::fs::rename(&d, &away).is_ok() {
                    let _ = std::os::unix::fs::symlink(&targets[i % 2], &d);
                    for _ in 0..50 {
                        std::thread::yield_now();
                    }
                    let _ = std::fs::remove_file(&d);
                    let _ = std::fs::rename(&away, &d);
                }
                i += 1;
                std::thread::yield_now();
            }
            if !d.exists() {
                let _ = std::fs::remove_file(&d);
                let _ = std::fs::rename(&away, &d);
            }
        })
    }

    /// How many of `tries` snapshots and sub-agent folders hold a canary.
    async fn attack(&self, sandbox: Option<Arc<Sandbox>>, tries: usize) -> (usize, usize, usize) {
        let base = self.data.join("checkpoints");
        let protected = vec![self.data.clone()];
        let stop = Arc::new(AtomicBool::new(false));
        let swapper = self.swapper(stop.clone());
        let (mut snap_hits, mut child_hits, mut failed) = (0, 0, 0);
        for i in 0..tries {
            match harness_node::testing::snapshot_holds(
                &self.proj,
                &base,
                sandbox.clone(),
                &protected,
                &format!("refs/harness/t/{i}/after"),
                "d/x",
            )
            .await
            {
                Ok(Some(b)) if String::from_utf8_lossy(&b).contains("CANARY") => snap_hits += 1,
                Ok(_) => {}
                Err(_) => failed += 1,
            }
            if i % 3 == 0 {
                match harness_node::testing::sub_agent_folder(
                    &self.proj,
                    &base,
                    sandbox.clone(),
                    &protected,
                    &format!("c{i}"),
                )
                .await
                {
                    Ok(child) => {
                        // What the node wrote there, not through a link
                        // (a base that recorded `d` as the link holds the
                        // link, which the sandboxed child cannot follow
                        // out; this test process could).
                        let d = child.join("d");
                        let real_dir =
                            std::fs::symlink_metadata(&d).is_ok_and(|m| m.file_type().is_dir());
                        let got = if real_dir {
                            std::fs::read_to_string(d.join("x")).unwrap_or_default()
                        } else {
                            String::new()
                        };
                        if got.contains("CANARY") {
                            child_hits += 1;
                        }
                    }
                    Err(_) => failed += 1,
                }
            }
        }
        stop.store(true, Ordering::Relaxed);
        swapper.join().unwrap();
        (snap_hits, child_hits, failed)
    }
}

/// R1b-2: the attack, unconfined and confined. Confined, no try lands.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_folder_swapped_for_a_link_during_a_snapshot_reads_nothing_outside() {
    const TRIES: usize = 300;
    let f = fixture();
    if !f.landlock() {
        return;
    }
    let (s, c, e) = f.attack(None, TRIES).await;
    eprintln!(
        "R1b-2 unconfined: {s} of {TRIES} snapshots and {c} of {} sub-agent folders held a \
         canary ({e} calls failed)",
        TRIES.div_ceil(3)
    );
    let (s, c, e) = f.attack(Some(f.sandbox()), TRIES).await;
    eprintln!(
        "R1b-2 confined: {s} of {TRIES} snapshots and {c} of {} sub-agent folders held a \
         canary ({e} calls failed)",
        TRIES.div_ceil(3)
    );
    assert_eq!((s, c), (0, 0), "the confined git read outside the folder");
}

/// A link inside the folder that leads outside it (to the data directory,
/// to another folder) is stored as a link, never followed, confined or not;
/// and an ordinary snapshot works in the sandbox.
#[tokio::test]
async fn links_in_the_folder_are_stored_as_links_and_never_followed() {
    let f = fixture();
    if !f.landlock() {
        return;
    }
    std::os::unix::fs::symlink(&f.data, f.proj.join("to-data")).unwrap();
    std::os::unix::fs::symlink(&f.victim, f.proj.join("to-victim")).unwrap();
    std::fs::write(f.proj.join("new.txt"), "new\n").unwrap();
    for sandbox in [None, Some(f.sandbox())] {
        let files = harness_node::testing::snapshot_files(
            &f.proj,
            &f.data.join("checkpoints"),
            sandbox.clone(),
            std::slice::from_ref(&f.data),
        )
        .await
        .unwrap();
        let mut names: Vec<&str> = files.iter().map(|(_, p)| p.as_str()).collect();
        names.sort();
        assert_eq!(
            names,
            ["d/x", "new.txt", "to-data", "to-victim"],
            "{files:?}"
        );
        for (mode, p) in &files {
            if p.starts_with("to-") {
                assert_eq!(mode, "120000", "{p} was not stored as a link");
            }
        }
    }
}

/// What the jail costs: a turn's checkpoint of a 100,000-file folder, the
/// first one and a second one after one change, unconfined and confined.
/// A measurement, not a check:
/// `cargo test -p harness --test snapshot_sandbox cost -- --ignored --nocapture`.
#[tokio::test]
#[ignore]
async fn cost_of_the_jail_on_a_large_folder() {
    let f = fixture();
    if !f.landlock() {
        return;
    }
    for i in 0..100_000 {
        let d = f.proj.join(format!("t/{}/{}", i / 1000, (i / 100) % 10));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join(format!("f{i}.txt")), format!("{i}\n")).unwrap();
    }
    for (name, sandbox) in [("unconfined", None), ("confined", Some(f.sandbox()))] {
        let base = f.data.join(format!("checkpoints-{name}"));
        let protected = vec![f.data.clone()];
        let t = std::time::Instant::now();
        harness_node::testing::snapshot_files(&f.proj, &base, sandbox.clone(), &protected)
            .await
            .unwrap();
        let first = t.elapsed();
        std::fs::write(f.proj.join("t/0/0/f0.txt"), "changed\n").unwrap();
        let t = std::time::Instant::now();
        harness_node::testing::snapshot_files(&f.proj, &base, sandbox, &protected)
            .await
            .unwrap();
        eprintln!("COST {name}: first {first:?}, next {:?}", t.elapsed());
    }
}

/// Without any race, everything the node's git does for a session works
/// in the jail: a turn's checkpoints and its undo in a repository whose
/// objects are borrowed, and a sub-agent's folder with its own index.
#[tokio::test]
async fn checkpoints_undo_and_a_sub_agent_folder_work_confined() {
    let f = fixture();
    if !f.landlock() {
        return;
    }
    let base = f.data.join("checkpoints");
    let protected = vec![f.data.clone()];
    let proj = f.proj.clone();
    let files = harness_node::testing::turn_and_undo(
        &f.proj,
        &base,
        Some(f.sandbox()),
        &protected,
        move || {
            std::fs::write(proj.join("d/x"), "changed\n").unwrap();
            std::fs::write(proj.join("made.txt"), "made\n").unwrap();
        },
    )
    .await
    .unwrap();
    assert_eq!(files.len(), 2, "{files:?}");
    assert_eq!(
        std::fs::read_to_string(f.proj.join("d/x")).unwrap(),
        "inside\n"
    );
    assert!(!f.proj.join("made.txt").exists());
    let child = harness_node::testing::sub_agent_folder(
        &f.proj,
        &base,
        Some(f.sandbox()),
        &protected,
        "c1",
    )
    .await
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(child.join("d/x")).unwrap(),
        "inside\n"
    );
    assert!(child.join(".git/index").is_file());
}
