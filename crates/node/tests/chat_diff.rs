//! The whole chat's changes in one diff (ADOPTION_PLAN X.61, PLAN.md T53):
//! from the oldest checkpoint the node still keeps to the latest finished
//! state, read from the chat's own `checkpoint` events; the range rule for
//! undone, rewound, running and pruned turns; the caps of a turn's diff hold
//! for the range; nothing is appended to the log.

#[path = "support/rig.rs"]
mod support;

use harness_protocol::{DiffStatus, MAX_DIFF_JSON, TurnDiff};
use support::Rig;

/// `MAX_FILES` of `crates/node/src/git/diff.rs`: files listed by name.
const MAX_FILES: usize = 1000;

fn names(d: &TurnDiff) -> Vec<&str> {
    let mut v: Vec<&str> = d.files.iter().map(|f| f.path.as_str()).collect();
    v.sort();
    v
}

/// Two turns, one file changed by both: the range shows the net change of
/// each file once, and reading it writes nothing to the log.
#[tokio::test]
async fn the_range_is_the_chats_net_change_and_logs_nothing() {
    let mut rig = Rig::new().await;
    assert!(rig.caps.iter().any(|c| c == harness_protocol::CAP_DIFF_ALL));
    rig.write("keep.txt", "v0\n");
    rig.turn("echo v1 > keep.txt; echo new > a.txt").await;
    rig.turn("echo v2 > keep.txt; echo new > b.txt").await;
    let seen = rig.seen.len();
    let d = rig.diff_all().await.unwrap();
    assert_eq!(names(&d), ["a.txt", "b.txt", "keep.txt"]);
    let keep = d.files.iter().find(|f| f.path == "keep.txt").unwrap();
    let lines: Vec<&str> = keep.hunks[0].lines.iter().map(String::as_str).collect();
    assert_eq!(lines, ["-v0", "+v2"], "the net change, not each turn's");
    assert!(!d.since_pruned);
    assert_eq!(rig.seen.len(), seen, "a diff appends no event");
}

/// An undone turn is not in the range: the range ends at the latest state.
#[tokio::test]
async fn an_undone_turn_drops_out_of_the_range() {
    let mut rig = Rig::new().await;
    rig.turn("echo 1 > a.txt").await;
    let second = rig.turn("echo 2 > b.txt").await.id.unwrap();
    assert!(rig.restore(&second, false).await.result.is_ok());
    assert_eq!(names(&rig.diff_all().await.unwrap()), ["a.txt"]);
}

/// A rewind to before a turn drops it and every later turn.
#[tokio::test]
async fn a_rewound_range_ends_before_the_rewound_turn() {
    let mut rig = Rig::new().await;
    rig.turn("echo 1 > a.txt").await;
    let second = rig.turn("echo 2 > b.txt").await.id.unwrap();
    rig.turn("echo 3 > c.txt").await;
    assert!(rig.restore_with(&second, false, true).await.result.is_ok());
    assert_eq!(names(&rig.diff_all().await.unwrap()), ["a.txt"]);
}

/// A running turn is not in the range yet: the diff is to the last finished
/// turn, and it is an answer, not an error.
#[tokio::test]
async fn a_running_turn_is_left_out_until_it_ends() {
    let mut rig = Rig::new().await;
    rig.turn("echo 1 > a.txt").await;
    rig.start_turn("sleep 3; echo 2 > b.txt");
    // Its `before` checkpoint is taken first.
    rig.until(|k| matches!(k, harness_core::event::EventKind::Checkpoint(_)))
        .await;
    let d = rig.diff_all().await.unwrap();
    assert_eq!(names(&d), ["a.txt"]);
    rig.until(|k| matches!(k, harness_core::event::EventKind::TurnEnded(_)))
        .await;
    assert_eq!(names(&rig.diff_all().await.unwrap()), ["a.txt", "b.txt"]);
}

/// The very first turn running: nothing is finished, so nothing to show.
#[tokio::test]
async fn a_first_turn_still_running_shows_no_changes() {
    let mut rig = Rig::new().await;
    rig.start_turn("sleep 2; echo 1 > a.txt");
    rig.until(|k| matches!(k, harness_core::event::EventKind::Checkpoint(_)))
        .await;
    let d = rig.diff_all().await.unwrap();
    assert!(d.files.is_empty());
    rig.until(|k| matches!(k, harness_core::event::EventKind::TurnEnded(_)))
        .await;
}

/// A chat with no turn, and one whose turns changed nothing: an empty diff
/// (the panel says "No file changes"), not an error.
#[tokio::test]
async fn a_chat_with_no_file_change_is_an_empty_answer() {
    let mut rig = Rig::new().await;
    let d = rig.diff_all().await.unwrap();
    assert!(d.files.is_empty() && !d.since_pruned);
    rig.turn("true").await;
    let d = rig.diff_all().await.unwrap();
    assert!(d.files.is_empty() && !d.since_pruned);
}

/// A checkpoint past `checkpoints_keep` starts the range later, and the
/// answer says so; the files are those since the oldest kept one.
#[tokio::test]
async fn a_pruned_start_moves_the_range_and_says_so() {
    let mut rig = Rig::with_keep(2).await;
    rig.turn("echo 1 > a.txt").await;
    rig.turn("echo 2 > b.txt").await;
    rig.turn("echo 3 > c.txt").await;
    let d = rig.diff_all().await.unwrap();
    assert!(d.since_pruned);
    assert_eq!(names(&d), ["b.txt", "c.txt"]);
}

/// Nothing is kept at all (`checkpoints_keep = 0`): a sentence.
#[tokio::test]
async fn checkpoints_off_is_a_sentence() {
    let mut rig = Rig::with_keep(0).await;
    rig.turn("echo 1 > a.txt").await;
    let e = rig.diff_all().await.unwrap_err();
    assert!(e.contains("checkpoints"), "{e}");
}

/// Secrets are scrubbed from lines and names of the range, as of a turn's.
#[tokio::test]
async fn a_secret_in_a_changed_file_is_scrubbed_in_the_range() {
    let mut rig = Rig::new().await;
    // The command is built from halves: the node scrubs a command's own text
    // before it runs, so the whole secret must only come to exist on disk.
    let (a, b) = support::RIG_SECRET.split_at(10);
    rig.turn(&format!(
        "S={a}\"{b_}\"; echo \"token=$S\" > cfg.txt; echo x > \"$S.txt\"",
        b_ = b
    ))
    .await;
    assert!(
        rig.files().keys().any(|k| k.contains(support::RIG_SECRET)),
        "the secret is on disk (else this test proves nothing)"
    );
    let d = rig.diff_all().await.unwrap();
    let json = serde_json::to_string(&d).unwrap();
    assert!(!json.contains(support::RIG_SECRET), "{json}");
    assert!(json.contains("token=[REDACTED]"), "{json}");
}

/// The caps of a turn's diff hold for the range, which can be far larger:
/// files past the list are counted, a large file is named with its size, and
/// the whole answer stays inside the link's bound.
#[tokio::test]
async fn the_range_keeps_the_turn_diffs_caps() {
    let mut rig = Rig::new().await;
    let each = MAX_FILES / 2 + 100;
    for part in 0..2 {
        rig.turn(&format!(
            "for i in $(seq 1 {each}); do echo {part}$i > p{part}_$i.txt; done"
        ))
        .await;
    }
    rig.turn("seq 1 600000 > big.txt").await;
    let d = rig.diff_all().await.unwrap();
    assert_eq!(d.files.len(), MAX_FILES);
    assert_eq!(d.more_files as usize, 2 * each + 1 - MAX_FILES);
    assert!(d.cut);
    let json = serde_json::to_vec(&d).unwrap();
    assert!(json.len() <= MAX_DIFF_JSON, "{} bytes", json.len());
    // The large file, if listed, is not compared line by line.
    if let Some(big) = d.files.iter().find(|f| f.path == "big.txt") {
        assert_eq!(big.status, DiffStatus::Added);
        assert!(big.hunks.is_empty());
    }
}

/// A fork's log holds its parent's checkpoint events and its folder is the
/// parent's: the range holds the inherited turn, the parent's later turn in
/// the shared folder (documented: the range is the folder's), and its own.
#[tokio::test]
async fn a_forks_range_holds_the_inherited_turns_and_the_shared_folder() {
    let mut rig = Rig::new().await;
    let parent = rig.session_id();
    let first = rig.turn("echo 1 > a.txt").await.id.unwrap();
    let fork = rig.fork(parent, &first).await;
    rig.turn("echo 2 > b.txt").await; // the parent goes on
    rig.turn_in(fork, "echo 3 > c.txt").await;
    let d = rig.diff_all_of(fork).await.unwrap();
    assert_eq!(names(&d), ["a.txt", "b.txt", "c.txt"]);
    assert!(!d.since_pruned);
    // The parent's own range ends at its own last turn: no c.txt.
    assert_eq!(names(&rig.diff_all().await.unwrap()), ["a.txt", "b.txt"]);
}

/// Checkpoints are pruned per folder, so a sibling chat's turns can push this
/// chat's oldest out: the range starts at its oldest kept one and says so.
#[tokio::test]
async fn a_sibling_chats_turns_can_prune_the_start() {
    let mut rig = Rig::with_keep(3).await;
    let other = rig.new_chat().await;
    rig.turn("echo 1 > a.txt").await;
    rig.turn_in(other, "echo 2 > b.txt").await;
    rig.turn("echo 3 > c.txt").await;
    rig.turn_in(other, "echo 4 > d.txt").await;
    let d = rig.diff_all().await.unwrap();
    assert!(d.since_pruned, "the first turn's checkpoints are gone");
    assert_eq!(
        names(&d),
        ["c.txt"],
        "from its oldest kept start to its latest end"
    );
}

/// The chat's folder is a subfolder of the repository the checkpoints follow:
/// the paths stay relative to the repository (as the diff has always shown
/// them) and the answer says where the folder is in it, for a turn's diff and
/// the whole chat's alike. A folder that is the repository's own says nothing.
#[tokio::test]
async fn a_folder_inside_a_repository_says_where_it_is() {
    use harness_core::event::EventKind;
    use harness_core::policy::ApprovalMode;
    use harness_protocol::MasterToNode;
    let mut rig = Rig::new().await;
    let top = rig.ws.clone();
    rig.git_in(&top, &["init", "-q"]);
    std::fs::create_dir_all(top.join("pkg")).unwrap();
    std::fs::write(top.join("a.txt"), "the repository's own\n").unwrap();
    let sid = harness_core::SessionId::new();
    rig.send(MasterToNode::Create {
        exec_mode: None,
        no_callbacks: false,
        profile: None,
        incognito: false,
        agent: None,
        req_id: "c-sub".into(),
        session_id: sid,
        title: "t".into(),
        cwd: top.join("pkg").display().to_string(),
        model: None,
        approval_mode: Some(ApprovalMode::Auto),
    });
    rig.until(|k| matches!(k, EventKind::SessionCreated(_)))
        .await;
    rig.turn_in(sid, "echo hi > a.txt").await;
    let d = rig.diff_all_of(sid).await.unwrap();
    assert_eq!(names(&d), ["pkg/a.txt"], "paths are the repository's");
    assert_eq!(d.folder, "pkg");
    let turn = rig
        .seen
        .iter()
        .rev()
        .find_map(|e| match e.typed() {
            Ok(EventKind::Checkpoint(c)) if e.session_id == sid => Some(c.turn_id),
            _ => None,
        })
        .unwrap();
    let one = rig.diff_of(sid, &turn, false).await.unwrap();
    assert_eq!(
        (names(&one), one.folder.as_str()),
        (vec!["pkg/a.txt"], "pkg")
    );
    // A folder that is the repository's own: nothing to strip.
    rig.turn("echo hi > b.txt").await;
    let d = rig.diff_all().await.unwrap();
    assert_eq!(d.folder, "", "{:?}", names(&d));
    assert!(names(&d).contains(&"b.txt"));
}
