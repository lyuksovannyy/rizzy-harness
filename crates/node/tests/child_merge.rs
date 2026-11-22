//! Two writing sub-agents of one step change the same file (fix-plan A.4,
//! the first live test of 2026-11-15): their changes are merged per file,
//! three ways; overlapping ones write nothing of that child, and the parent
//! gets the child's diff, quoted, instead of a ref it cannot read.

#[path = "support/rig.rs"]
mod support;

use support::Rig;

/// Ten lines, two "functions" far apart.
fn two_functions(first: &str, last: &str) -> String {
    format!("fn first() {{ {first} }}\n2\n3\n4\n5\n6\n7\n8\n9\nfn last() {{ {last} }}\n")
}

async fn rig_with_file() -> Rig {
    let r = Rig::new().await;
    r.git(&["init", "-q"]);
    r.write("f.rs", &two_functions("a", "z"));
    r.git(&["add", "."]);
    r.git(&["commit", "-qm", "init"]);
    r
}

/// Two children change different functions of one file: both changes are
/// in the folder afterwards, the notes say so truthfully, and one Undo of
/// the turn puts the folder back exactly.
#[tokio::test]
async fn two_children_changing_different_functions_both_land_and_undo_restores() {
    let mut r = rig_with_file().await;
    let before = r.files();
    let (turn, outs) = r
        .lead_turn(
            "delegate: dev#own=run: sed -i 's/{ a }/{ ONE }/' f.rs | \
             dev#own=run: sed -i 's/{ z }/{ TWO }/' f.rs",
        )
        .await;
    assert_eq!(outs.len(), 2, "{outs:#?}");
    assert!(
        outs.iter().all(|o| o.contains("1 changed file(s) applied")),
        "{outs:#?}"
    );
    assert_eq!(
        outs.iter()
            .filter(|o| o.contains("merged with changes the folder had"))
            .count(),
        1,
        "{outs:#?}"
    );
    assert_eq!(r.read("f.rs").unwrap(), two_functions("ONE", "TWO"));
    let undo = r.restore(&turn.id.expect("a turn checkpoint"), false).await;
    assert_eq!(undo.result, Ok(()));
    assert_eq!(r.files(), before);
}

/// Overlapping changes: the second child's work is not applied, the
/// folder holds the first one's exactly, and the parent's result names the
/// file and carries the second child's diff, every line quoted.
#[tokio::test]
async fn overlapping_children_leave_the_folder_alone_and_hand_back_the_diff() {
    let mut r = rig_with_file().await;
    let (_, outs) = r
        .lead_turn(
            "delegate: dev#own=run: sed -i 's/{ a }/{ ONE }/' f.rs | \
             dev#own=run: sed -i 's/{ a }/{ TWO }/' f.rs; echo extra > also.txt",
        )
        .await;
    let applied: Vec<&String> = outs
        .iter()
        .filter(|o| o.contains("applied to the folder"))
        .collect();
    let refused: Vec<&String> = outs.iter().filter(|o| o.contains("NOT applied")).collect();
    assert_eq!((applied.len(), refused.len()), (1, 1), "{outs:#?}");
    let out = refused[0];
    let now = r.read("f.rs").unwrap();
    assert!(
        now == two_functions("ONE", "z") || now == two_functions("TWO", "z"),
        "{now}"
    );
    let lost = if now.contains("ONE") { "TWO" } else { "ONE" };
    assert!(
        out.contains("`f.rs` (both changed the same lines)"),
        "{out}"
    );
    assert!(
        out.ends_with(harness_core::subagent::NOT_APPLIED_LAST),
        "{out}"
    );
    let lines: Vec<&str> = out.lines().collect();
    assert!(
        lines.contains(&format!("| +fn first() {{ {lost} }}").as_str()),
        "{out}"
    );
    // Every line of the diff is behind the quote: no line after the note
    // stands on its own.
    let note = lines
        .iter()
        .position(|l| l.starts_with("[its changes were NOT applied"))
        .expect("the note");
    let last = lines.len() - 1;
    assert!(
        lines[note + 1..last].iter().all(|l| l.starts_with("| ")),
        "{out}"
    );
    // Nothing of that child was written, not even its other file (when the
    // refused one was the child that made it).
    if lost == "TWO" {
        assert!(r.read("also.txt").is_none());
    }
}

/// A child whose changes were not applied is not reported ok, whatever
/// it says of itself (review of PR 118, M1): the call is `ok: false`
/// (the card is not green), its own status line is kept, and the result
/// ends with the harness's instruction to apply the changes.
#[tokio::test]
async fn a_refused_childs_delegate_result_is_not_ok_and_ends_with_what_to_do() {
    let mut r = rig_with_file().await;
    let (_, outs) = r
        .lead_turn(
            "delegate: dev#own=run: sed -i 's/{ a }/{ ONE }/' f.rs | \
             dev#own=run: sed -i 's/{ a }/{ TWO }/' f.rs",
        )
        .await;
    let refused = outs
        .iter()
        .find(|o| o.contains("NOT applied"))
        .expect("one refused");
    let applied = outs
        .iter()
        .find(|o| !o.contains("NOT applied"))
        .expect("one applied");
    let ok_of = |out: &String| {
        r.seen
            .iter()
            .filter_map(|e| e.typed().ok())
            .find_map(|k| match k {
                harness_core::event::EventKind::ToolCallFinished(f) if &f.output == out => {
                    Some(f.ok)
                }
                _ => None,
            })
            .expect("its event")
    };
    assert!(!ok_of(refused), "{refused}");
    assert!(ok_of(applied), "{applied}");
    assert!(
        refused.lines().any(|l| l.contains("finished: done")),
        "{refused}"
    );
    assert!(
        refused.ends_with(harness_core::subagent::NOT_APPLIED_LAST),
        "{refused}"
    );
    assert!(!applied.contains(harness_core::subagent::NOT_APPLIED_LAST));
}

/// What the child made that the repository ignores is named in the note,
/// not dropped without a word.
#[tokio::test]
async fn a_childs_ignored_files_are_named_in_the_note() {
    let mut r = rig_with_file().await;
    r.write(".gitignore", "*.log\n");
    r.git(&["add", "."]);
    r.git(&["commit", "-qm", "ignore logs"]);
    let (_, outs) = r
        .lead_turn("delegate: dev#own=run: echo work > report.log")
        .await;
    assert!(
        outs[0].contains(
            "1 path(s) the sub-agent made that the repository ignores were not brought back: \
             `report.log`"
        ),
        "{}",
        outs[0]
    );
    assert!(r.read("report.log").is_none());
}
