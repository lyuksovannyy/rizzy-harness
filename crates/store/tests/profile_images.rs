//! Profile pictures (PLAN.md T74): one per slot, replaced in one
//! transaction, the old blob released unless something else holds it, the
//! quota checked, and everything gone with the account, settings included.

use harness_store::blobs::{Limits, NewBlob};
use harness_store::media::{MediaInsert, NewItem};
use harness_store::profile_images::ProfileImageSet;
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

fn blob(id: &str, owner: &str, sha: &str, size: u64) -> NewBlob {
    NewBlob {
        blob_id: id.into(),
        owner_id: Some(owner.into()),
        sha256: sha.into(),
        size,
        kind: "image".into(),
        format: "png".into(),
        mime: "image/png".into(),
        display: "img".into(),
        key: vec![7; 32],
        width: Some(256),
        height: Some(256),
    }
}

const NO_LIMIT: Limits = Limits {
    account: 0,
    server: 0,
};

fn stored(r: ProfileImageSet) -> (String, bool, Vec<String>) {
    match r {
        ProfileImageSet::Stored {
            sha256,
            new_blob,
            gone,
        } => (sha256, new_blob, gone),
        other => panic!("not stored: {other:?}"),
    }
}

#[test]
fn a_new_picture_replaces_the_old_and_releases_its_blob() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "u1");
    let (sha, new, gone) = stored(
        s.profile_image_set("u1", "avatar", &blob("b1", "u1", "aa", 10), NO_LIMIT)
            .unwrap(),
    );
    assert_eq!((sha.as_str(), new, gone.len()), ("aa", true, 0));
    assert_eq!(
        s.profile_image("u1", "avatar").unwrap().unwrap().blob_id,
        "b1"
    );
    assert!(s.profile_image("u1", "banner").unwrap().is_none());
    // Replaced: the old blob goes in the same transaction.
    let (_, _, gone) = stored(
        s.profile_image_set("u1", "avatar", &blob("b2", "u1", "bb", 10), NO_LIMIT)
            .unwrap(),
    );
    assert_eq!(gone, vec!["b1".to_string()]);
    assert!(s.blob("b1").unwrap().is_none());
    // The same bytes again: the blob already held, nothing new to keep.
    let (_, new, gone) = stored(
        s.profile_image_set("u1", "avatar", &blob("b3", "u1", "bb", 10), NO_LIMIT)
            .unwrap(),
    );
    assert!(!new && gone.is_empty());
    // Removed.
    assert_eq!(
        s.profile_image_delete("u1", "avatar").unwrap(),
        vec!["b2".to_string()]
    );
    assert!(s.profile_image("u1", "avatar").unwrap().is_none());
    assert!(s.profile_image_delete("u1", "avatar").unwrap().is_empty());
}

#[test]
fn a_blob_media_also_holds_stays_and_the_sweep_leaves_pictures_alone() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "u1");
    let item = NewItem {
        media_id: "m1".into(),
        owner_id: Some("u1".into()),
        workspace_id: None,
        name: "me.png".into(),
        origin: "upload".into(),
        session_id: None,
    };
    assert!(matches!(
        s.media_insert(&item, &blob("b1", "u1", "aa", 10), NO_LIMIT)
            .unwrap(),
        MediaInsert::Added { .. }
    ));
    stored(
        s.profile_image_set("u1", "avatar", &blob("bx", "u1", "aa", 10), NO_LIMIT)
            .unwrap(),
    );
    assert_eq!(
        s.profile_image("u1", "avatar").unwrap().unwrap().blob_id,
        "b1"
    );
    assert!(s.profile_image_delete("u1", "avatar").unwrap().is_empty());
    assert!(s.blob("b1").unwrap().is_some());
    // A picture's blob is referenced: the hourly sweep keeps it.
    stored(
        s.profile_image_set("u1", "banner", &blob("b9", "u1", "cc", 10), NO_LIMIT)
            .unwrap(),
    );
    assert!(s.blobs_unreferenced(i64::MAX).unwrap().is_empty());
}

#[test]
fn the_quota_counts_pictures() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "u1");
    let limits = Limits {
        account: 15,
        server: 0,
    };
    stored(
        s.profile_image_set("u1", "avatar", &blob("b1", "u1", "aa", 10), limits)
            .unwrap(),
    );
    assert!(matches!(
        s.profile_image_set("u1", "banner", &blob("b2", "u1", "bb", 10), limits)
            .unwrap(),
        ProfileImageSet::OverQuota(_)
    ));
    assert!(s.profile_image("u1", "banner").unwrap().is_none());
}

#[test]
fn deleting_the_account_takes_its_pictures_and_settings() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "u1");
    user(&s, "u2");
    stored(
        s.profile_image_set("u1", "avatar", &blob("b1", "u1", "aa", 10), NO_LIMIT)
            .unwrap(),
    );
    s.kv_set("settings:u1", r#"{"description":"x"}"#).unwrap();
    s.kv_set("settings:u2", "{}").unwrap();
    s.delete_user("u1").unwrap();
    assert!(s.profile_image("u1", "avatar").unwrap().is_none());
    assert!(s.blob("b1").unwrap().is_none());
    assert!(s.kv_get("settings:u1").unwrap().is_none());
    assert!(s.kv_get("settings:u2").unwrap().is_some());
}

#[test]
fn sharing_a_workspace_is_membership_in_one_together() {
    let s = Store::open_in_memory().unwrap();
    for u in ["u1", "u2", "u3"] {
        user(&s, u);
    }
    let w = s.create_workspace("Acme", "u1").unwrap();
    assert!(s.add_member(&w.workspace_id, "u2", "member").unwrap());
    assert!(s.share_a_workspace("u1", "u2").unwrap());
    assert!(s.share_a_workspace("u2", "u1").unwrap());
    assert!(!s.share_a_workspace("u1", "u3").unwrap());
    assert!(s.remove_member(&w.workspace_id, "u2", None).unwrap());
    assert!(!s.share_a_workspace("u1", "u2").unwrap());
}
