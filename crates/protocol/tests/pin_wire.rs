//! Wire snapshot of `session.pin` (docs/pins.md): every field is named by
//! every sender (there is no default), and the bytes are what a node of
//! this version reads.

use harness_protocol::MasterToNode;

const PIN: &str = r#"{"type":"session.pin","req_id":"p1","session_id":"0193f2a0-0000-7000-8000-000000000001","message_seq":7,"pinned":true}"#;

#[test]
fn the_pin_message_wire_is_stable() {
    let msg: MasterToNode = serde_json::from_str(PIN).unwrap();
    assert_eq!(serde_json::to_string(&msg).unwrap(), PIN);
    assert_eq!(msg.req_id(), Some("p1"));
    assert_eq!(harness_protocol::CAP_PIN, "pin");
}

#[test]
fn a_field_left_out_is_an_error_not_a_default() {
    // `pinned` decides whether a mark is put or taken off: a sender that
    // forgot it is refused, never read as false.
    for missing in ["pinned", "message_seq", "session_id", "req_id"] {
        let mut v: serde_json::Value = serde_json::from_str(PIN).unwrap();
        v.as_object_mut().unwrap().remove(missing);
        assert!(
            serde_json::from_value::<MasterToNode>(v).is_err(),
            "{missing} defaulted"
        );
    }
}
