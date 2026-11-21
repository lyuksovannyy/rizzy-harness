//! Where a sub-agent that can write works (the owner, 2026-11-21, PLAN.md
//! T32): in the chat's own folder unless the delegating agent asks for a
//! copy (`folder: "own"`) or the node says `sub_agent_folder = "own"`.

#[path = "support/rig.rs"]
mod support;

use harness_core::SessionId;
use harness_core::event::EventKind;
use support::Rig;

/// The sub-agents of the rig's chat that were made: id and folder.
fn children(r: &Rig) -> Vec<(SessionId, String)> {
    r.seen
        .iter()
        .filter(|e| e.session_id != r.session_id())
        .filter_map(|e| match e.typed() {
            Ok(EventKind::SessionCreated(c)) => Some((e.session_id, c.cwd)),
            _ => None,
        })
        .collect()
}

/// Folders the node made for sub-agents (`<data>/sessions/<id>`).
fn session_folders(r: &Rig) -> usize {
    std::fs::read_dir(r.dir.path().join("sessions"))
        .map(|d| d.count())
        .unwrap_or(0)
}

/// Checkpoint repositories on the node: the chat's, and the sub-agents'
/// when one was made (`<data>/checkpoints/*.git`).
fn shadows(r: &Rig) -> usize {
    std::fs::read_dir(r.dir.path().join("checkpoints"))
        .map(|d| {
            d.filter_map(Result::ok)
                .filter(|e| e.path().extension().is_some_and(|x| x == "git"))
                .count()
        })
        .unwrap_or(0)
}

async fn repo_rig() -> Rig {
    let r = Rig::new().await;
    r.git(&["init", "-q"]);
    r.write("keep.txt", "v1\n");
    r.git(&["add", "."]);
    r.git(&["commit", "-qm", "init"]);
    r
}

/// The default: no `folder` at all. The sub-agent writes straight into the
/// chat's folder, no copy is made, no base ref exists, and its result ends
/// with the one harness line naming the files that changed there.
#[tokio::test]
async fn a_delegation_with_no_folder_writes_straight_into_the_chats_folder() {
    let mut r = repo_rig().await;
    let (_, outs) = r
        .lead_turn("delegate: dev=run: echo v2 > keep.txt; echo new > made.txt")
        .await;
    assert_eq!(outs.len(), 1, "{outs:#?}");
    let out = &outs[0];
    assert_eq!(r.read("keep.txt").unwrap(), "v2\n");
    assert_eq!(r.read("made.txt").unwrap(), "new\n");
    // It worked in the chat's folder: same cwd, no folder of its own.
    let kids = children(&r);
    assert_eq!(kids.len(), 1);
    assert_eq!(
        std::path::Path::new(&kids[0].1).canonicalize().unwrap(),
        r.ws.canonicalize().unwrap(),
        "the sub-agent got another folder"
    );
    assert_eq!(session_folders(&r), 0);
    // No copy, no merge: no sub-agent snapshots, no merge words.
    assert_eq!(shadows(&r), 1, "a sub-agent snapshot repository was made");
    assert!(!out.contains("applied") && !out.contains("merged"), "{out}");
    // The lead is told where it worked and which files changed.
    let line = out
        .lines()
        .find(|l| l.starts_with("[it worked in this chat's folder"))
        .unwrap_or_else(|| panic!("no shared line: {out}"));
    assert!(
        line.contains("`keep.txt`") && line.contains("`made.txt`"),
        "{line}"
    );
    assert!(line.contains("read them again"), "{line}");
}

/// `shared` asked for by name is the same as leaving it out.
#[tokio::test]
async fn shared_asked_for_by_name_is_the_default() {
    let mut r = repo_rig().await;
    let (_, outs) = r
        .lead_turn("delegate: dev#shared=run: echo new > made.txt")
        .await;
    assert!(
        outs[0].contains("worked in this chat's folder"),
        "{outs:#?}"
    );
    assert_eq!(r.read("made.txt").unwrap(), "new\n");
    assert_eq!(session_folders(&r), 0);
}

/// A writing sub-agent runs in a folder that is not a git repository.
#[tokio::test]
async fn a_shared_writer_works_outside_a_git_repository() {
    let mut r = Rig::new().await;
    r.write("keep.txt", "v1\n");
    let (_, outs) = r
        .lead_turn("delegate: dev=run: echo v2 > keep.txt; echo new > made.txt")
        .await;
    assert!(
        !outs[0].contains("not started") && outs[0].contains("worked in this chat's folder"),
        "{outs:#?}"
    );
    assert_eq!(r.read("keep.txt").unwrap(), "v2\n");
    assert_eq!(r.read("made.txt").unwrap(), "new\n");
    assert_eq!(children(&r).len(), 1);
}

/// `own` is today's behaviour: a copy of its own, merged back per file, no
/// shared line. Outside a repository it is refused, and the sentence says
/// leaving `folder` out works.
#[tokio::test]
async fn own_is_a_copy_merged_back_and_needs_a_repository() {
    let mut r = repo_rig().await;
    let (_, outs) = r
        .lead_turn("delegate: dev#own=run: echo v2 > keep.txt; echo new > made.txt")
        .await;
    assert!(outs[0].contains("2 changed file(s) applied"), "{outs:#?}");
    assert!(
        !outs[0].contains("worked in this chat's folder"),
        "{outs:#?}"
    );
    assert_eq!(r.read("keep.txt").unwrap(), "v2\n");
    let kids = children(&r);
    assert!(
        std::path::Path::new(&kids[0].1).starts_with(r.dir.path().join("sessions")),
        "own: the sub-agent did not work in a folder of its own: {kids:?}"
    );
    let mut p = Rig::new().await;
    p.write("keep.txt", "v1\n");
    let (_, outs) = p
        .lead_turn("delegate: dev#own=run: echo v2 > keep.txt")
        .await;
    assert!(
        outs[0].contains("not started")
            && outs[0].contains("not a git repository")
            && outs[0].contains("leave `folder` out"),
        "{outs:#?}"
    );
    assert_eq!(p.read("keep.txt").unwrap(), "v1\n");
    assert_eq!(children(&p).len(), 0, "a child session was made");
}

/// The node's `own` wins over what a model asks: every writer gets a copy,
/// outside a repository it is refused, and when `shared` was asked for by
/// name the refusal names the setting.
#[tokio::test]
async fn the_nodes_own_setting_gives_every_writer_a_copy() {
    use harness_core::subagent::SubFolder;
    let mut r = Rig::with_policy(|p| p.limits.sub_agent_folder = SubFolder::Own).await;
    r.git(&["init", "-q"]);
    r.write("keep.txt", "v1\n");
    r.git(&["add", "."]);
    r.git(&["commit", "-qm", "init"]);
    for ask in ["dev", "dev#shared", "dev#own"] {
        let (_, outs) = r
            .lead_turn(&format!("delegate: {ask}=run: echo v2 > keep.txt"))
            .await;
        assert!(
            outs[0].contains("1 changed file(s) applied"),
            "{ask}: {outs:#?}"
        );
        assert!(
            !outs[0].contains("worked in this chat's folder"),
            "{ask}: {outs:#?}"
        );
        r.write("keep.txt", "v1\n");
    }
    for (_, cwd) in children(&r) {
        assert!(
            std::path::Path::new(&cwd).starts_with(r.dir.path().join("sessions")),
            "{cwd}"
        );
    }
    let mut p = Rig::with_policy(|p| p.limits.sub_agent_folder = SubFolder::Own).await;
    p.write("keep.txt", "v1\n");
    let (_, outs) = p
        .lead_turn("delegate: dev#shared=run: echo v2 > keep.txt")
        .await;
    assert!(
        outs[0].contains("not started") && outs[0].contains("sub_agent_folder = \"own\""),
        "{outs:#?}"
    );
    let (_, outs) = p.lead_turn("delegate: dev=run: echo v2 > keep.txt").await;
    assert!(
        outs[0].contains("not started") && outs[0].contains("not a git repository"),
        "{outs:#?}"
    );
    assert_eq!(p.read("keep.txt").unwrap(), "v1\n");
}

/// A sub-agent that only reads is the same on every setting: it works in
/// the chat's folder and `folder` changes nothing for it.
#[tokio::test]
async fn a_reader_ignores_folder() {
    let mut r = repo_rig().await;
    for ask in ["look", "look#own", "look#shared"] {
        let (_, outs) = r
            .lead_turn(&format!("delegate: {ask}=read: keep.txt"))
            .await;
        assert!(outs[0].contains("finished: done"), "{ask}: {outs:#?}");
        assert!(
            !outs[0].contains("worked in this chat's folder"),
            "{outs:#?}"
        );
    }
    assert_eq!(session_folders(&r), 0);
}
