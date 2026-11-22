//! Files waiting to be sent with a message that the model cannot read as
//! text (PLAN.md T94): owner-keyed rows on blobs, taken at send, given
//! back when the node certainly did not take the message, purged when
//! never sent, and keeping their blob alive until then.

use harness_store::blobs::{Limits, NewBlob};
use harness_store::chat_uploads::{NewUpload, Put};
use harness_store::{NewUser, Role, Store};

fn user(s: &Store, id: &str) {
    s.create_user(&NewUser {
        user_id: id.into(),
        username: id.into(),
        password_hash: "$argon2id$x".into(),
        role: Role::Member,
        exec_ceiling: harness_core::policy::ExecLevel::None,
        must_change_password: false,
    })
    .unwrap();
}

fn blob(id: &str, owner: Option<&str>, sha: &str, size: u64) -> NewBlob {
    NewBlob {
        blob_id: id.into(),
        owner_id: owner.map(str::to_string),
        sha256: sha.into(),
        size,
        kind: "other".into(),
        format: "bin".into(),
        mime: "application/octet-stream".into(),
        display: "download".into(),
        key: vec![7; 32],
        width: None,
        height: None,
    }
}

fn up(id: &str, owner: Option<&str>) -> NewUpload {
    NewUpload {
        upload_id: id.into(),
        owner_id: owner.map(str::to_string),
        name: format!("{id}.zip"),
        mime: "application/zip".into(),
        session_id: None,
    }
}

const FREE: Limits = Limits {
    account: 0,
    server: 0,
};

fn put(s: &Store, id: &str, owner: &str, sha: &str, size: u64) -> Put {
    s.chat_upload_put(
        &up(id, Some(owner)),
        &blob(&format!("b{id}"), Some(owner), sha, size),
        FREE,
        3,
    )
    .unwrap()
}

#[test]
fn an_upload_is_taken_once_by_its_owner_only() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "u1");
    user(&s, "u2");
    assert!(matches!(
        put(&s, "a", "u1", "s1", 10),
        Put::Added { new_blob: true }
    ));
    // Another account cannot take it; nothing is consumed by the try.
    assert!(
        s.chat_uploads_take(&["a".into()], Some("u2"))
            .unwrap()
            .is_none()
    );
    let got = s
        .chat_uploads_take(&["a".into()], Some("u1"))
        .unwrap()
        .unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(
        (got[0].name.as_str(), got[0].size, got[0].sha256.as_str()),
        ("a.zip", 10, "s1")
    );
    assert_eq!(got[0].blob_id, "ba");
    // Taken: not again, by anyone (an id used twice).
    assert!(
        s.chat_uploads_take(&["a".into()], Some("u1"))
            .unwrap()
            .is_none()
    );
    // An unknown id, and a mix with an unknown id: all or nothing.
    assert!(put(&s, "b", "u1", "s2", 5).added());
    assert!(
        s.chat_uploads_take(&["b".into(), "nope".into()], Some("u1"))
            .unwrap()
            .is_none()
    );
    assert!(
        s.chat_uploads_take(&["b".into()], Some("u1"))
            .unwrap()
            .is_some()
    );
}

/// The API token (no owner) may take any, as attachments do.
#[test]
fn the_api_token_takes_anyones() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "u1");
    assert!(put(&s, "a", "u1", "s1", 10).added());
    assert!(s.chat_uploads_take(&["a".into()], None).unwrap().is_some());
}

/// Done: the row goes. Refused or never sent: given back, and takeable
/// again. After the row is gone the blob is unreferenced.
#[test]
fn done_deletes_and_give_back_makes_it_takeable_again() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "u1");
    assert!(put(&s, "a", "u1", "s1", 10).added());
    let ids = vec!["a".to_string()];
    s.chat_uploads_take(&ids, Some("u1")).unwrap().unwrap();
    s.chat_uploads_give_back(&ids).unwrap();
    s.chat_uploads_take(&ids, Some("u1")).unwrap().unwrap();
    // The blob is still referenced while the row is.
    assert!(s.blobs_unreferenced(i64::MAX).unwrap().is_empty());
    s.chat_uploads_done(&ids).unwrap();
    assert_eq!(
        s.blobs_unreferenced(i64::MAX).unwrap(),
        vec!["ba".to_string()]
    );
    // A deleted row is not given back.
    s.chat_uploads_give_back(&ids).unwrap();
    assert!(s.chat_uploads_take(&ids, Some("u1")).unwrap().is_none());
}

/// An unsent upload keeps its blob out of the unreferenced sweep; the
/// purge by age removes the row and then the sweep takes the blob.
#[test]
fn an_unsent_upload_keeps_its_blob_until_purged() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "u1");
    assert!(put(&s, "a", "u1", "s1", 10).added());
    assert!(s.blobs_unreferenced(i64::MAX).unwrap().is_empty());
    assert_eq!(s.chat_uploads_purge(0).unwrap(), 0);
    assert_eq!(s.chat_uploads_purge(i64::MAX).unwrap(), 1);
    assert_eq!(
        s.blobs_unreferenced(i64::MAX).unwrap(),
        vec!["ba".to_string()]
    );
    // A purge of rows being sent leaves them: they are being used.
    assert!(put(&s, "b", "u1", "s2", 5).added());
    s.chat_uploads_take(&["b".into()], Some("u1"))
        .unwrap()
        .unwrap();
    assert_eq!(
        s.chat_uploads_purge(i64::MAX).unwrap(),
        1,
        "a stale one goes either way"
    );
}

/// An account holds at most `max_unsent` uploads; the same bytes twice
/// share one blob and cost no quota.
#[test]
fn the_count_and_the_quota_are_checked_inside_the_insert() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "u1");
    for (i, id) in ["a", "b", "c"].iter().enumerate() {
        assert!(put(&s, id, "u1", &format!("s{i}"), 10).added());
    }
    assert!(matches!(put(&s, "d", "u1", "s9", 10), Put::TooMany));
    // The same bytes again (same sha): one blob, a second row, once room.
    s.chat_uploads_take(&["a".into()], Some("u1"))
        .unwrap()
        .unwrap();
    s.chat_uploads_done(&["a".into()]).unwrap();
    let again = put(&s, "e", "u1", "s1", 10);
    assert!(matches!(again, Put::Added { new_blob: false }), "{again:?}");
    // Quota.
    let tight = Limits {
        account: 25,
        server: 0,
    };
    let r = s
        .chat_upload_put(
            &up("q", Some("u1")),
            &blob("bq", Some("u1"), "sq", 100),
            tight,
            10,
        )
        .unwrap();
    assert!(matches!(r, Put::OverQuota(_)), "{r:?}");
}

/// Deleting the account removes its rows with its blobs.
#[test]
fn deleting_an_account_removes_its_uploads() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "u1");
    assert!(put(&s, "a", "u1", "s1", 10).added());
    s.delete_user("u1").unwrap();
    assert!(s.chat_uploads_take(&["a".into()], None).unwrap().is_none());
}
