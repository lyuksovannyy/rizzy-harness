//! A.18: the checkpoint repository's garbage collection runs rarely, after
//! the turn has ended, and bounded growth still holds.
//!
//! Every git call here runs with a fixed identity, no system or global
//! config, and a cleared environment, in a temporary directory.

#[path = "support/rig.rs"]
mod rig;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use rig::Rig;

/// How many dropped checkpoint groups make a garbage collection due.
const GC_EVERY: u64 = 16;

/// "Six minutes later": the time of the last garbage collection is moved
/// back, so the rule's minimum interval does not hold the next one back.
/// The test cannot wait; the state file is the rule's only memory of time.
fn pretend_the_last_gc_was_long_ago(rig: &Rig) {
    if !rig.dir.path().join("checkpoints").exists() {
        return;
    }
    let f = rig.shadow().join("harness-gc");
    let Ok(text) = std::fs::read_to_string(&f) else {
        return;
    };
    let kept: String = text
        .lines()
        .filter(|l| !l.starts_with("last "))
        .map(|l| format!("{l}\n"))
        .collect();
    let tmp = f.with_extension("test");
    std::fs::write(&tmp, format!("last 0\n{kept}")).unwrap();
    std::fs::rename(tmp, f).unwrap();
}

/// Turns of the bounded-growth tests.
const TURNS: u64 = 60;

/// The most checkpoint groups whose objects can be in the store at once:
/// the kept ones, those dropped since the last collection (up to
/// `GC_EVERY`), and those a collection keeps because they are younger than
/// its start (in this test turns are about 0.1 s apart, so up to about
/// `GC_EVERY` more), plus two.
fn waiting_groups(keep: usize) -> u64 {
    keep as u64 + 2 * GC_EVERY + 2
}

/// Wait while a collection runs (its `gc.pid`), as time between turns
/// would; a collection that is spawned but not yet started is not waited
/// for.
async fn no_gc_running(rig: &Rig) {
    tokio::time::sleep(Duration::from_millis(20)).await;
    while rig.shadow().join("gc.pid").exists() {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

// ---- A.18 ----

/// Wait until no garbage collection runs in the checkpoint repository and
/// one has finished (the state file says no group was dropped since).
async fn wait_for_a_finished_gc(rig: &Rig) {
    let f = rig.shadow().join("harness-gc");
    for _ in 0..600 {
        let text = std::fs::read_to_string(&f).unwrap_or_default();
        let collected = text
            .lines()
            .any(|l| l.starts_with("last ") && l != "last 0");
        if collected && !rig.shadow().join("gc.pid").exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!(
        "no garbage collection finished: {:?}",
        std::fs::read_to_string(&f)
    );
}

/// A plain folder (the shadow stores everything): over 60 turns that each
/// write a new large file, the store stays bounded; loose objects are
/// packed and those of dropped checkpoints freed, by collections that run
/// once per many dropped checkpoints.
#[tokio::test]
async fn the_checkpoint_repository_stays_bounded_in_a_plain_folder() {
    const KEEP: usize = 3;
    const SIZE: u64 = 256 * 1024;
    let mut rig = Rig::with_keep(KEEP).await;
    rig.write("small.txt", "s\n");
    let cmd = format!("head -c {SIZE} /dev/urandom > big.bin");
    let mut peak = 0;
    for _ in 0..TURNS {
        pretend_the_last_gc_was_long_ago(&rig);
        rig.turn(&cmd).await;
        peak = peak.max(rig.shadow_object_bytes());
        no_gc_running(&rig).await;
    }
    // Without collection: TURNS * SIZE (15 MiB).
    let bound = SIZE * waiting_groups(KEEP);
    eprintln!("peak {peak}, bound {bound}");
    assert!(peak <= bound, "peak {peak} > bound {bound}");
    let counts = rig.git_in(&rig.shadow(), &["count-objects", "-v"]);
    let loose: u64 = counts
        .lines()
        .find_map(|l| l.strip_prefix("count: "))
        .unwrap()
        .parse()
        .unwrap();
    // Per turn: one blob, a tree or two, a commit, twice.
    assert!(
        loose < 8 * (KEEP as u64 + GC_EVERY),
        "{loose} loose objects: nothing was packed"
    );
}

/// After a collection, every kept checkpoint still restores: the user
/// commits and packs before each turn, the node's collection runs, and a
/// rewind to the oldest kept turn puts back exactly the folder as it was
/// before that turn.
#[tokio::test]
async fn after_gc_every_kept_checkpoint_restores() {
    const KEEP: usize = 4;
    let mut rig = Rig::with_keep(KEEP).await;
    rig.git(&["init", "-q", "-b", "main"]);
    rig.write("f.txt", "v0\n");
    let mut turns = Vec::new();
    for i in 1..=GC_EVERY as usize + KEEP + 2 {
        rig.git(&["add", "-A"]);
        rig.git(&["commit", "-qm", &format!("before turn {i}")]);
        rig.git(&["gc", "-q"]);
        let before = rig.files();
        let t = rig
            .turn(&format!("echo v{i} > f.txt; echo n > n{i}.txt"))
            .await;
        turns.push((t.id.unwrap(), before));
    }
    wait_for_a_finished_gc(&rig).await;
    let (oldest, folder) = &turns[turns.len() - KEEP];
    let r = rig.restore_with(oldest, false, true).await;
    assert_eq!(r.result, Ok(()));
    assert_eq!(&r.folder_after, folder);
}

/// The node's garbage collection runs rarely: 30 turns with two kept
/// checkpoints drop 28 groups, and the old code ran `git gc` in the
/// checkpoint repository after each of them. Counted by a stand-in `git`
/// first on `PATH` that logs every call it passes on; runs in a child
/// process, as this one's `PATH` cannot be changed without `unsafe`.
#[test]
fn gc_runs_rarely() {
    if let Some(log) = std::env::var_os(COUNT_CHILD) {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(count_scenario(PathBuf::from(log)));
        return;
    }
    let path = std::env::var("PATH").unwrap_or_default();
    let real = std::env::split_paths(&path)
        .map(|d| d.join("git"))
        .find(|g| g.is_file())
        .expect("git is needed for this test");
    let bin = tempfile::tempdir().unwrap();
    let log = bin.path().join("calls");
    let wrapper = bin.path().join("git");
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\n\
             d=0; g=0\n\
             for a in \"$@\"; do [ \"$a\" = --git-dir ] && d=1; [ \"$a\" = gc ] && g=1; done\n\
             [ $d = 1 ] && [ $g = 1 ] && echo gc >> '{log}'\n\
             exec '{r}' \"$@\"\n",
            log = log.display(),
            r = real.display()
        ),
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["gc_runs_rarely", "--exact", "--nocapture"])
        .env(COUNT_CHILD, &log)
        .env(
            "PATH",
            std::env::join_paths(
                std::iter::once(bin.path().to_path_buf()).chain(std::env::split_paths(&path)),
            )
            .unwrap(),
        )
        .status()
        .unwrap();
    assert!(status.success(), "the scenario failed: {status}");
}

const COUNT_CHILD: &str = "HARNESS_TEST_A18_GC_LOG";

async fn count_scenario(log: PathBuf) {
    let mut rig = Rig::with_keep(2).await;
    rig.write("f.txt", "0\n");
    for i in 1..=30 {
        rig.turn(&format!("echo {i} > f.txt")).await;
    }
    // A collection started after the last turn ended has time to log.
    tokio::time::sleep(Duration::from_secs(1)).await;
    let n = count_lines(&log);
    eprintln!("{n} garbage collections in 30 turns");
    assert!((1..=2).contains(&n), "{n} garbage collections in 30 turns");
}

fn count_lines(p: &Path) -> usize {
    std::fs::read_to_string(p)
        .unwrap_or_default()
        .lines()
        .count()
}
