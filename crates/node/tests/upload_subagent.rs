//! A sub-agent's chat takes no attached file, whatever the master sends
//! (PLAN.md T94): the node's own check, in `Session::accepts_uploads`.

#[path = "support/rig.rs"]
mod support;

use base64::Engine as _;
use harness_core::SessionId;
use harness_core::event::EventKind;
use harness_protocol::{MasterToNode, NodeToMaster};
use sha2::{Digest, Sha256};
use support::Rig;

#[tokio::test]
async fn a_sub_agents_chat_refuses_a_chunk() {
    let mut r = Rig::new().await;
    r.lead_turn("delegate: dev=run: echo hi").await;
    let child: SessionId = r
        .seen
        .iter()
        .filter(|e| e.session_id != r.session_id())
        .find_map(|e| match e.typed() {
            Ok(EventKind::SessionCreated(_)) => Some(e.session_id),
            _ => None,
        })
        .expect("the delegation made a chat");
    let bytes = b"bytes";
    r.send(MasterToNode::UploadChunk {
        req_id: "u1".into(),
        session_id: child,
        upload_id: "a".repeat(32),
        offset: 0,
        size: bytes.len() as u64,
        sha256: hex::encode(Sha256::digest(bytes)),
        data: base64::engine::general_purpose::STANDARD.encode(bytes),
    });
    loop {
        if let NodeToMaster::Ack { req_id, ok, error } = r.recv().await
            && req_id == "u1"
        {
            assert!(!ok);
            let e = error.unwrap_or_default();
            assert!(e.contains("sub-agent"), "{e}");
            return;
        }
    }
}
