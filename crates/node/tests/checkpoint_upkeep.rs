//! A.17: undo does not depend on the user's repository keeping old content.
//! At the end of every turn in a repository folder, the old versions of the
//! files the turn changed (and the trees a diff of the turn reads) are
//! copied into the checkpoint repository's own store, in a pack with a
//! `.keep` file, released when the turn's checkpoint is pruned.
//!
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

use harness_core::event::*;
use rig::{AGENT_GIT, Rig, X};

/// The prefix of the chat note for a turn whose undo cannot be complete.
const INCOMPLETE: &str = "This turn's undo is incomplete";

/// How many dropped checkpoint groups make a garbage collection due.
const GC_EVERY: u64 = 16;

/// Two committed files, `a.txt` and `b.txt`, in one commit (and nothing
/// else in the folder, so the checkpoint's trees equal the user's). Returns
/// their blob ids.
fn two_committed_files(rig: &Rig) -> (String, String) {
    rig.git(&["init", "-q", "-b", "main"]);
    rig.write("a.txt", "A1: committed\n");
    rig.write("b.txt", "B1: committed\n");
    rig.git(&["add", "."]);
    rig.git(&["commit", "-qm", "base"]);
    (
        rig.git(&["hash-object", "a.txt"]),
        rig.git(&["hash-object", "b.txt"]),
    )
}

/// The agent's own history rewrite: amend the commit with what the folder
/// holds now, expire the reflog, prune everything unreachable.
fn agent_rewrites_and_prunes() -> String {
    format!(
        "{AGENT_GIT}; git commit -qa --amend -m rewritten && \
         git reflog expire --expire=now --all && git gc --prune=now -q"
    )
}

fn turn_ended_normally(rig: &Rig, mark: usize) -> bool {
    rig.seen[mark..].iter().any(
        |e| matches!(e.typed(), Ok(EventKind::TurnEnded(t)) if t.reason != TurnEndReason::Error),
    )
}

// ---- A.17 ----

/// The owner's case: the turn changes two committed files; afterwards the
/// user rewrites history and prunes, so the old versions are gone from the
/// user's repository. The undo works and puts back the exact bytes.
#[tokio::test]
async fn undo_works_after_the_user_rewrites_history_and_prunes() {
    let mut rig = Rig::new().await;
    let (a1, b1) = two_committed_files(&rig);
    let t = rig.turn("echo a2 > a.txt; echo b2 > b.txt").await;
    assert_eq!(t.phases, ["before", "after"]);
    rig.write("a.txt", "A3: amended\n");
    rig.git(&["commit", "-qa", "--amend", "-m", "amended"]);
    rig.expire_reflog_and_gc_now();
    assert!(!rig.user_has(&a1) && !rig.user_has(&b1), "not pruned");
    rig.write("a.txt", "a2\n"); // the turn's result, as the turn left it
    let r = rig.restore(t.id.as_deref().unwrap(), false).await;
    assert_eq!(r.result, Ok(()));
    assert_eq!(
        r.folder_after.get("a.txt"),
        Some(&b"A1: committed\n".to_vec())
    );
    assert_eq!(
        r.folder_after.get("b.txt"),
        Some(&b"B1: committed\n".to_vec())
    );
}

/// The abuse case: the agent itself rewrites history and prunes inside its
/// turn, after changing the files, so the old versions are gone before the
/// turn ends and cannot be copied. The turn ends normally, and the chat
/// says this turn's undo is incomplete, naming both files. Undo is then
/// refused with nothing changed (A.16).
#[tokio::test]
async fn history_pruned_inside_the_turn_is_named_in_the_chat_and_the_turn_ends() {
    let mut rig = Rig::new().await;
    let (a1, b1) = two_committed_files(&rig);
    // Untracked, so the checkpoint's root tree is its own (see the clean
    // variant below).
    rig.write("scratch.txt", "untracked\n");
    let mark = rig.seen.len();
    let t = rig
        .turn(&format!(
            "echo a2 > a.txt; echo b2 > b.txt; {}",
            agent_rewrites_and_prunes()
        ))
        .await;
    assert!(
        !rig.user_has(&a1) && !rig.user_has(&b1),
        "the agent's gc ran"
    );
    assert_eq!(t.phases, ["before", "after"]);
    assert!(turn_ended_normally(&rig, mark));
    let notes = rig.notes_since(mark);
    assert!(
        notes
            .iter()
            .any(|n| n.starts_with(INCOMPLETE) && n.contains("`a.txt`") && n.contains("`b.txt`")),
        "{notes:?}"
    );
    let r = rig.restore(t.id.as_deref().unwrap(), false).await;
    let e = r.result.unwrap_err();
    assert!(e.starts_with("nothing was changed:"), "{e}");
    assert_eq!(r.folder_after, r.folder_before);
}

/// The same inside a clean folder: there the checkpoint's root tree is the
/// user's own, so the agent's prune takes the tree too, the turn's changes
/// cannot even be listed, and the existing note says the turn has no
/// checkpoint. Visible, not closed (residual).
#[tokio::test]
async fn history_pruned_inside_a_clean_turn_says_the_turn_has_no_checkpoint() {
    let mut rig = Rig::new().await;
    two_committed_files(&rig);
    let mark = rig.seen.len();
    rig.turn(&format!("echo a2 > a.txt; {}", agent_rewrites_and_prunes()))
        .await;
    assert!(turn_ended_normally(&rig, mark));
    let notes = rig.notes_since(mark);
    assert!(
        notes
            .iter()
            .any(|n| n.starts_with("This turn has no checkpoint")),
        "{notes:?}"
    );
}

/// The variant the copy does close: the agent changes the files, the turn
/// ends, and only a later turn rewrites history and prunes. The first
/// turn's undo works.
#[tokio::test]
async fn history_pruned_by_a_later_turn_does_not_take_the_undo() {
    let mut rig = Rig::new().await;
    let (a1, b1) = two_committed_files(&rig);
    let t = rig.turn("echo a2 > a.txt; echo b2 > b.txt").await;
    let mark = rig.seen.len();
    rig.turn(&agent_rewrites_and_prunes()).await;
    assert!(
        !rig.user_has(&a1) && !rig.user_has(&b1),
        "the agent's gc ran"
    );
    assert!(
        !rig.notes_since(mark)
            .iter()
            .any(|n| n.starts_with(INCOMPLETE)),
        "{:?}",
        rig.notes_since(mark)
    );
    let r = rig.restore(t.id.as_deref().unwrap(), false).await;
    assert_eq!(r.result, Ok(()));
    assert_eq!(
        r.folder_after.get("a.txt"),
        Some(&b"A1: committed\n".to_vec())
    );
    assert_eq!(
        r.folder_after.get("b.txt"),
        Some(&b"B1: committed\n".to_vec())
    );
}

/// The copies survive the checkpoint repository's own garbage collection
/// (the command the node runs, and a plain `git gc`), so a prune in the
/// user's repository afterwards still does not take the undo.
#[tokio::test]
async fn copies_survive_the_checkpoint_repositorys_own_gc() {
    let mut rig = Rig::new().await;
    let (a1, _) = two_committed_files(&rig);
    let t = rig.turn("echo a2 > a.txt; echo b2 > b.txt").await;
    let shadow = rig.shadow();
    let at = format!(
        "--prune={}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 1
    );
    for args in [
        &["gc", "--prune=now", "--quiet"][..],
        &["gc", &at, "--quiet"],
        &["gc", "--quiet"],
    ] {
        rig.git_in(&shadow, args);
    }
    assert!(rig.shadow_owns(&a1), "the copy of a.txt went with gc");
    rig.replace_history_and_prune();
    assert!(!rig.user_has(&a1));
    rig.write("a.txt", "a2\n");
    rig.write("b.txt", "b2\n");
    let r = rig.restore(t.id.as_deref().unwrap(), false).await;
    assert_eq!(r.result, Ok(()));
    assert_eq!(
        r.folder_after.get("a.txt"),
        Some(&b"A1: committed\n".to_vec())
    );
    assert_eq!(
        r.folder_after.get("b.txt"),
        Some(&b"B1: committed\n".to_vec())
    );
}

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

/// What the copies cost: after a turn that changed one large committed
/// file, the checkpoint repository's own store holds that file's old and
/// new versions and no other blob; over 60 turns that each rewrite it, no
/// more copies are kept than there are kept checkpoints, and the store
/// stays bounded (copies of pruned checkpoints are released and collected).
#[tokio::test]
async fn only_changed_files_are_copied_and_copies_go_with_their_checkpoint() {
    const KEEP: usize = 3;
    const SIZE: u64 = 256 * 1024;
    let mut rig = Rig::with_keep(KEEP).await;
    rig.git(&["init", "-q", "-b", "main"]);
    let big = |rig: &Rig| {
        let mut f = std::fs::File::open("/dev/urandom").unwrap();
        let mut buf = vec![0u8; SIZE as usize];
        std::io::Read::read_exact(&mut f, &mut buf).unwrap();
        std::fs::write(rig.ws.join("big.bin"), buf).unwrap();
    };
    big(&rig);
    for i in 0..5 {
        rig.write(&format!("small{i}.txt"), &format!("small {i}\n"));
    }
    rig.git(&["add", "."]);
    rig.git(&["commit", "-qm", "base"]);
    let b0 = rig.git(&["hash-object", "big.bin"]);
    let cmd = format!("head -c {SIZE} /dev/urandom > big.bin");
    rig.turn(&cmd).await;
    let b1 = rig.git(&["hash-object", "big.bin"]);
    let own = rig.shadow_own_blobs();
    assert!(own.contains(&b0), "the old version was not copied: {own:?}");
    assert_eq!(
        own,
        [b0.clone(), b1].into_iter().collect(),
        "blobs of unchanged files were copied"
    );
    let mut peak = 0;
    for _ in 0..TURNS {
        pretend_the_last_gc_was_long_ago(&rig);
        rig.turn(&cmd).await;
        assert!(
            rig.shadow_keeps().len() <= KEEP,
            "copies outlive their checkpoints: {:?}",
            rig.shadow_keeps()
        );
        peak = peak.max(rig.shadow_object_bytes());
        no_gc_running(&rig).await;
    }
    // Two versions a turn: without release or collection the store would
    // reach 2 * TURNS * SIZE (30 MiB).
    let bound = 2 * SIZE * waiting_groups(KEEP);
    eprintln!("peak {peak}, bound {bound}");
    assert!(peak <= bound, "peak {peak} > bound {bound}");
}

/// A folder that is not a repository borrows nothing, so nothing is copied:
/// no pack and no `.keep` file appear, and undo works as before.
#[tokio::test]
async fn a_plain_folder_gets_no_copies() {
    let mut rig = Rig::new().await;
    rig.write("keep.txt", X);
    let t = rig.turn("echo v2 > keep.txt").await;
    assert!(!rig.shadow().join("objects/info/alternates").exists());
    let packs: Vec<_> = std::fs::read_dir(rig.shadow().join("objects/pack"))
        .unwrap()
        .collect();
    assert!(packs.is_empty(), "{packs:?}");
    let r = rig.restore(t.id.as_deref().unwrap(), false).await;
    assert_eq!(r.result, Ok(()));
    assert_eq!(r.folder_after.get("keep.txt"), Some(&X.as_bytes().to_vec()));
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

/// After a collection, every kept checkpoint still restores, including
/// those whose old versions exist only as copies: the user commits and
/// packs before each turn, the node's collection runs, the user throws its
/// history away, and a rewind to the oldest kept turn puts back exactly the
/// folder as it was before that turn.
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
    rig.replace_history_and_prune();
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
    run_with_gc_wrapper("gc_runs_rarely", COUNT_CHILD, 0);
}

const COUNT_CHILD: &str = "HARNESS_TEST_A18_GC_LOG";

/// Run test `name` again in a child process with a stand-in `git` first on
/// `PATH`: for the checkpoint repository's `gc` it logs `start <epoch s>`,
/// sleeps `sleep` seconds, runs the real gc, logs `end <epoch s>`; every
/// other call goes straight to the real git. The log's path is in `env`.
fn run_with_gc_wrapper(name: &str, env: &str, sleep: u32) {
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
             if [ $d = 1 ] && [ $g = 1 ]; then\n\
             echo \"start $(date +%s.%N)\" >> '{log}'\n\
             sleep {sleep}\n\
             '{r}' \"$@\"; s=$?\n\
             echo \"end $(date +%s.%N)\" >> '{log}'\n\
             exit $s\n\
             fi\n\
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
        .args([name, "--exact", "--nocapture"])
        .env(env, &log)
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

/// The times the stand-in logged for `what` (`start` or `end`).
fn logged(p: &Path, what: &str) -> Vec<f64> {
    std::fs::read_to_string(p)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.strip_prefix(what)?.trim().parse().ok())
        .collect()
}

fn epoch() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

/// A.18 review F2: a collection holds the checkpoint repository alone. The
/// turn whose end makes it due has ended before it starts (its end is not
/// held back), and the next turn's "before" snapshot, started while the
/// collection runs (a stand-in `git` makes it take 3 s), is taken only
/// after the collection has finished; its tree is fully readable.
#[test]
fn a_snapshot_waits_for_a_running_collection() {
    if let Some(log) = std::env::var_os(SLOW_CHILD) {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(slow_gc_scenario(PathBuf::from(log)));
        return;
    }
    run_with_gc_wrapper("a_snapshot_waits_for_a_running_collection", SLOW_CHILD, 3);
}

const SLOW_CHILD: &str = "HARNESS_TEST_A18_SLOW_GC_LOG";

async fn slow_gc_scenario(log: PathBuf) {
    let mut rig = Rig::with_keep(1).await;
    rig.write("f.txt", "0\n");
    let mut ended = 0.0;
    for i in 1..=40 {
        rig.turn(&format!("echo {i} > f.txt")).await;
        ended = epoch();
        tokio::time::sleep(Duration::from_millis(300)).await;
        if !logged(&log, "start").is_empty() {
            break;
        }
    }
    let start = *logged(&log, "start")
        .first()
        .expect("no collection started");
    assert!(
        start >= ended,
        "the collection started {:.2} s before the turn ended",
        ended - start
    );
    // The next turn, while the collection sleeps.
    let mark = rig.seen.len();
    rig.start_turn("echo next > f.txt");
    rig.until(|k| matches!(k, EventKind::Checkpoint(c) if c.phase == CheckpointPhase::Before))
        .await;
    let before_at = epoch();
    rig.until(|k| matches!(k, EventKind::TurnEnded(_))).await;
    let end = *logged(&log, "end")
        .first()
        .expect("the collection did not end");
    assert!(
        before_at >= end,
        "the snapshot was taken {:.2} s before the collection ended",
        end - before_at
    );
    let sha = rig.seen[mark..]
        .iter()
        .find_map(|e| match e.typed() {
            Ok(EventKind::Checkpoint(c)) if c.phase == CheckpointPhase::Before => Some(c.git_ref),
            _ => None,
        })
        .unwrap();
    let objects = rig.git_in(
        &rig.shadow(),
        &["rev-list", "--objects", "--missing=print", &sha],
    );
    assert!(
        !objects.lines().any(|l| l.starts_with('?')),
        "missing objects"
    );
}

/// A.18 review F1: a collection killed (timeout, shutdown) while git wrote
/// the commit graph left `objects/info/commit-graph.lock`; the sweep did
/// not look there, so every later collection failed and the count of
/// dropped groups never reset. (Reviewer's proof.)
#[tokio::test]
async fn a_stale_commit_graph_lock_does_not_stop_collection() {
    let mut rig = Rig::with_keep(1).await;
    rig.write("f.txt", "0\n");
    rig.turn("echo 1 > f.txt").await;
    let lock = rig.shadow().join("objects/info/commit-graph.lock");
    std::fs::write(&lock, "").unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&lock)
        .unwrap()
        .set_modified(std::time::SystemTime::now() - Duration::from_secs(3600))
        .unwrap();
    for i in 2..40 {
        pretend_the_last_gc_was_long_ago(&rig);
        rig.turn(&format!("echo {i} > f.txt")).await;
        no_gc_running(&rig).await;
    }
    let state = std::fs::read_to_string(rig.shadow().join("harness-gc")).unwrap();
    let dropped: u64 = state
        .lines()
        .find_map(|l| l.strip_prefix("dropped "))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(
        dropped < GC_EVERY,
        "no collection succeeded: {state:?}, lock left: {}",
        lock.exists()
    );
}

/// A.17 review F5: a rewind across a change the person made between two
/// turns. Turn 1 changes `a.txt`; the person edits and commits `b.txt`;
/// turn 2 changes `a.txt` again; the person replaces history and prunes.
/// A rewind to before turn 1 needs `b.txt`'s first version, which no turn
/// changed; it is copied at the start of turn 2. (Reviewer's proof.)
#[tokio::test]
async fn rewind_across_a_change_made_between_turns_survives_a_prune() {
    let mut rig = Rig::with_keep(10).await;
    rig.git(&["init", "-q", "-b", "main"]);
    rig.write("a.txt", "A1\n");
    rig.write("b.txt", "B1: committed\n");
    rig.git(&["add", "."]);
    rig.git(&["commit", "-qm", "base"]);
    rig.git(&["gc", "-q"]);
    let folder = rig.files();
    let t1 = rig.turn("echo a2 > a.txt").await;
    rig.write("b.txt", "B2: by hand\n");
    rig.git(&["commit", "-qam", "by hand"]);
    rig.turn("echo a3 > a.txt").await;
    rig.replace_history_and_prune();
    let r = rig
        .restore_with(t1.id.as_deref().unwrap(), false, true)
        .await;
    assert_eq!(r.result, Ok(()), "rewind refused");
    assert_eq!(r.folder_after, folder);
}

/// A.17 review F6: a turn that changed nothing copies nothing (no pack, no
/// `.keep` file).
#[tokio::test]
async fn a_turn_that_changes_nothing_copies_nothing() {
    let mut rig = Rig::new().await;
    two_committed_files(&rig);
    let t = rig.turn("cat a.txt").await;
    assert_eq!(t.phases, ["before", "after"]);
    let packs: Vec<_> = std::fs::read_dir(rig.shadow().join("objects/pack"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert!(packs.is_empty(), "{packs:?}");
}

/// A.17 review F6: a restore that fails after its `before_restore` event
/// (a read-only folder) leaves that saved checkpoint as the way back; its
/// content is copied before the write, so the way back still works after
/// the person commits, packs, and then throws history away.
#[tokio::test]
async fn the_way_back_from_a_failed_restore_survives_a_prune() {
    let mut rig = Rig::new().await;
    rig.git(&["init", "-q", "-b", "main"]);
    std::fs::create_dir(rig.ws.join("ro")).unwrap();
    rig.write("a.txt", "a1\n");
    rig.write("ro/b.txt", "b1\n");
    rig.git(&["add", "."]);
    rig.git(&["commit", "-qm", "base"]);
    let t = rig.turn("echo a2 > a.txt; echo b2 > ro/b.txt").await;
    // The person commits the turn's work; their gc packs it, and the
    // checkpoint repository's own gc then drops its loose copies.
    rig.git(&["commit", "-qam", "agent work"]);
    rig.git(&["gc", "-q"]);
    rig.git_in(&rig.shadow(), &["gc", "--prune=now", "--quiet"]);
    let before = rig.files();
    set_mode(&rig.ws.join("ro"), 0o555);
    if std::fs::write(rig.ws.join("ro/.probe"), "").is_ok() {
        set_mode(&rig.ws.join("ro"), 0o755);
        eprintln!("SKIPPED: read-only folders are writable here (root?)");
        return;
    }
    let r = rig.restore(t.id.as_deref().unwrap(), false).await;
    set_mode(&rig.ws.join("ro"), 0o755);
    let saved = r.saved.clone().expect("a before_restore checkpoint");
    assert!(r.result.is_err());
    rig.replace_history_and_prune();
    let back = rig.restore(&saved, false).await;
    assert_eq!(back.result, Ok(()));
    assert_eq!(back.folder_after, before);
}

fn set_mode(p: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).unwrap();
}

async fn count_scenario(log: PathBuf) {
    let mut rig = Rig::with_keep(2).await;
    rig.write("f.txt", "0\n");
    for i in 1..=30 {
        rig.turn(&format!("echo {i} > f.txt")).await;
    }
    // A collection started after the last turn ended has time to log.
    tokio::time::sleep(Duration::from_secs(1)).await;
    let n = logged(&log, "start").len();
    eprintln!("{n} garbage collections in 30 turns");
    assert!((1..=2).contains(&n), "{n} garbage collections in 30 turns");
}
