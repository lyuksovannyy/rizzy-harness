//! Wire snapshots of `session.rewind` and `session.rewind_undo` (X.45):
//! every field is named by every sender (there is no default), and the
//! bytes are what a node of this version reads.

use harness_protocol::MasterToNode;

const REWIND: &str = r#"{"type":"session.rewind","req_id":"r1","session_id":"0193f2a0-0000-7000-8000-000000000001","message_seq":7,"files":true,"force":false}"#;
const UNDO: &str = r#"{"type":"session.rewind_undo","req_id":"r2","session_id":"0193f2a0-0000-7000-8000-000000000001","rewind_seq":12}"#;

#[test]
fn the_rewind_messages_wire_is_stable() {
    for (literal, req) in [(REWIND, "r1"), (UNDO, "r2")] {
        let msg: MasterToNode = serde_json::from_str(literal).unwrap();
        assert_eq!(serde_json::to_string(&msg).unwrap(), literal);
        assert_eq!(msg.req_id(), Some(req));
    }
}

#[test]
fn a_field_left_out_is_an_error_not_a_default() {
    // `files` and `force` decide whether the folder is written over: a
    // sender that forgot one is refused, never read as false.
    for missing in ["files", "force", "message_seq"] {
        let mut v: serde_json::Value = serde_json::from_str(REWIND).unwrap();
        v.as_object_mut().unwrap().remove(missing);
        assert!(
            serde_json::from_value::<MasterToNode>(v).is_err(),
            "{missing} defaulted"
        );
    }
    let mut v: serde_json::Value = serde_json::from_str(UNDO).unwrap();
    v.as_object_mut().unwrap().remove("rewind_seq");
    assert!(serde_json::from_value::<MasterToNode>(v).is_err());
}
