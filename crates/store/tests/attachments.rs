//! Unsent uploads: capped per owner, one owner's uploads never count against
//! another's, taken all or nothing, swept by age (FIX_PLAN 3.4).

use harness_core::event::Attachment;
use harness_store::Store;

fn att(text: &str) -> Attachment {
    Attachment {
        name: "n.txt".into(),
        mime: "text/plain".into(),
        text: text.into(),
        truncated: false,
    }
}

#[test]
fn unsent_uploads_are_capped_per_owner() {
    let s = Store::open_in_memory().unwrap();
    for i in 0..3 {
        assert!(
            s.attachment_put(&format!("a{i}"), Some("alex"), &att("x"), 3, 1000)
                .unwrap()
        );
    }
    // Count: the fourth is refused and not stored.
    assert!(
        !s.attachment_put("a3", Some("alex"), &att("x"), 3, 1000)
            .unwrap()
    );
    assert!(
        s.attachments_take(&["a3".into()], Some("alex"))
            .unwrap()
            .is_none()
    );
    // Another account (and the API token) has its own cap.
    assert!(
        s.attachment_put("b0", Some("sam"), &att("x"), 3, 1000)
            .unwrap()
    );
    assert!(s.attachment_put("t0", None, &att("x"), 3, 1000).unwrap());
    // Sending frees room.
    assert_eq!(
        s.attachments_take(&["a0".into()], Some("alex"))
            .unwrap()
            .unwrap()
            .len(),
        1
    );
    assert!(
        s.attachment_put("a4", Some("alex"), &att("x"), 3, 1000)
            .unwrap()
    );
    // Bytes (name + text): 5 + 990 fits under 1000 for sam, one more does not.
    let big = "y".repeat(990);
    assert!(
        !s.attachment_put("b1", Some("sam"), &att(&big), 3, 1000)
            .unwrap()
    );
    let fits = "y".repeat(1000 - 5 - 5 - 1);
    assert!(
        s.attachment_put("b1", Some("sam"), &att(&fits), 3, 1000)
            .unwrap()
    );
    assert!(
        !s.attachment_put("b2", Some("sam"), &att("zz"), 3, 1000)
            .unwrap()
    );
}

#[test]
fn the_sweep_drops_only_old_uploads() {
    let s = Store::open_in_memory().unwrap();
    assert!(
        s.attachment_put("old", Some("alex"), &att("x"), 5, 1000)
            .unwrap()
    );
    let cut = harness_core::now_ms() + 1;
    std::thread::sleep(std::time::Duration::from_millis(5));
    assert!(
        s.attachment_put("new", Some("alex"), &att("x"), 5, 1000)
            .unwrap()
    );
    assert_eq!(s.purge_attachments(cut).unwrap(), 1);
    assert!(
        s.attachments_take(&["new".into()], Some("alex"))
            .unwrap()
            .is_some()
    );
}

/// Review R-8: an upload the API token took for a message that was then
/// refused goes back to the account that uploaded it, not to the token,
/// so its owner can still send it.
#[test]
fn a_given_back_upload_keeps_its_owner() {
    let s = Store::open_in_memory().unwrap();
    assert!(
        s.attachment_put("u1", Some("alex"), &att("x"), 3, 1000)
            .unwrap()
    );
    let taken = s
        .attachments_take_owned(&["u1".into()], None)
        .unwrap()
        .unwrap();
    assert_eq!(taken[0].1.as_deref(), Some("alex"));
    s.attachments_give_back(&["u1".into()], &taken).unwrap();
    assert!(
        s.attachments_take(&["u1".into()], Some("alex"))
            .unwrap()
            .is_some(),
        "the owner cannot send their own upload"
    );
}
