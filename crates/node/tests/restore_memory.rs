//! The node's memory while an undo writes large files back (R.1, R1b-1 of
//! docs/security-review/2026-11-gate3-fixes-3.md). Alone in its test
//! binary, so the process's peak resident size (`VmHWM` in
//! `/proc/self/status`) is this restore's and no other test's. git runs in
//! processes of its own, so what the node holds is what the measure sees.
//! Before the fix every blob was read whole into a map (and once more in
//! the call's output buffer): two 200 MiB files cost about 800 MiB here.

#[path = "support/rig.rs"]
mod support;

use harness_core::event::*;
use harness_protocol::{MasterToNode, NodeToMaster};

/// The process's peak resident size so far, in bytes.
fn peak() -> u64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap();
    let kb: u64 = s
        .lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))
        .and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
        .unwrap();
    kb << 10
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_undo_of_large_files_holds_none_of_them_in_memory() {
    const BOUND: u64 = 48 << 20;
    let mut rig = support::Rig::new().await;
    // Two 200 MiB files (sparse; distinct first bytes), made in one turn
    // and deleted in the next; undoing the second writes both back.
    rig.turn(
        "truncate -s 200M a && truncate -s 200M b && printf A | dd of=a conv=notrunc \
         status=none && printf B | dd of=b conv=notrunc status=none",
    )
    .await;
    let gone = rig.turn("rm a b").await;
    let turn = gone.id.expect("a checkpoint");
    assert!(!rig.ws.join("a").exists());
    let before = peak();
    rig.send(MasterToNode::Restore {
        req_id: "r".into(),
        session_id: rig_session(&rig),
        turn_id: turn,
        force: false,
        rewind: false,
    });
    loop {
        match rig.recv().await {
            NodeToMaster::Ack {
                req_id, ok, error, ..
            } if req_id == "r" => {
                assert!(ok, "{error:?}");
                break;
            }
            NodeToMaster::Event { event } => {
                if let Ok(EventKind::Error(e)) = event.typed() {
                    eprintln!("note: {}", e.message);
                }
            }
            _ => {}
        }
    }
    let after = peak();
    for (name, first) in [("a", b'A'), ("b", b'B')] {
        let md = std::fs::metadata(rig.ws.join(name)).unwrap();
        assert_eq!(md.len(), 200 << 20, "{name}");
        let mut one = [0u8; 1];
        let f = std::fs::File::open(rig.ws.join(name)).unwrap();
        std::os::unix::fs::FileExt::read_exact_at(&f, &mut one, 0).unwrap();
        assert_eq!(one[0], first);
    }
    let grew = after.saturating_sub(before);
    eprintln!(
        "R1b-1: peak resident size {} MiB before the undo, {} MiB after (+{} KiB) for 400 MiB \
         written",
        before >> 20,
        after >> 20,
        grew >> 10
    );
    assert!(
        grew < BOUND,
        "the undo grew the node's peak memory by {grew} bytes"
    );
}

fn rig_session(rig: &support::Rig) -> harness_core::SessionId {
    rig.session_id()
}
