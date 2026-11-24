//! The background-output read (X.62) on a node whose chat has no running
//! command: the places it answers in are its own (an open row cannot refuse
//! a person's `@` completion, nor the reverse), it is busy (not final) when
//! they are taken, and it refuses where node.toml denies `bash` or
//! `bash_output`, in a sentence.

#[path = "support/rig.rs"]
mod support;

use harness_protocol::{MasterToNode, NodeToMaster};
use support::Rig;

async fn ask(r: &mut Rig) -> (Option<String>, bool) {
    r.n += 1;
    let req = format!("o{}", r.n);
    r.send(MasterToNode::BackgroundOutput {
        req_id: req.clone(),
        session_id: r.session_id(),
        bg_id: "bg-1".into(),
    });
    loop {
        match r.recv().await {
            NodeToMaster::BackgroundOutputResult {
                req_id,
                error,
                busy,
                ..
            } if req_id == req => return (error, busy),
            NodeToMaster::Event { event } => r.seen.push(event),
            _ => {}
        }
    }
}

#[tokio::test]
async fn output_reads_and_file_completion_have_places_of_their_own() {
    let mut r = Rig::new().await;
    let (out, files) = r.places();
    assert!(!std::sync::Arc::ptr_eq(&out, &files));
    // Every place of the output reads taken: the next read is busy, and the
    // completion's places are all still free.
    let held: Vec<_> = (0..out.available_permits())
        .map(|_| out.clone().try_acquire_owned().unwrap())
        .collect();
    assert_eq!(held.len(), 2);
    assert_eq!(
        files.available_permits(),
        2,
        "completion lost a place to output reads"
    );
    let (error, busy) = ask(&mut r).await;
    assert!(busy && error.is_none(), "busy is not an error: {error:?}");
    // And the other way: completions holding theirs do not make a read busy.
    drop(held);
    let held: Vec<_> = (0..2)
        .map(|_| files.clone().try_acquire_owned().unwrap())
        .collect();
    let (error, busy) = ask(&mut r).await;
    assert!(
        !busy,
        "an output read was refused for a completion's places"
    );
    assert!(
        error.is_some_and(|e| e.contains("no background")),
        "an answer about the command"
    );
    drop(held);
}

#[tokio::test]
async fn where_the_node_denies_bash_or_bash_output_the_read_is_a_sentence() {
    for denied in ["bash", "bash_output"] {
        let mut r = Rig::with_policy(|p| p.denied_tools = vec![denied.into()]).await;
        let (error, busy) = ask(&mut r).await;
        assert!(!busy);
        assert_eq!(
            error.as_deref(),
            Some("this node does not allow reading a command's output"),
            "{denied}"
        );
    }
}
