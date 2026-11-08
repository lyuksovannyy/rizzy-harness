//! Blob rows: per-owner dedupe, the quota inside the insert, previews,
//! release of unreferenced blobs, and account deletion.

use harness_store::blobs::{Limits, NewBlob, OverQuota};
use harness_store::media::{MediaInsert, NewItem};
use harness_store::{NewUser, Role, Store};

fn user(s: &Store, id: &str) {
    s.create_user(&NewUser {
        user_id: id.into(),
        username: id.into(),
        password_hash: "$argon2id$x".into(),
        role: Role::Member,
        can_agentic: false,
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
        kind: "image".into(),
        format: "png".into(),
        mime: "image/png".into(),
        display: "inline".into(),
        key: vec![7; 32],
        width: Some(640),
        height: Some(480),
    }
}

fn item(id: &str, owner: Option<&str>, ws: Option<&str>) -> NewItem {
    NewItem {
        media_id: id.into(),
        owner_id: owner.map(str::to_string),
        workspace_id: ws.map(str::to_string),
        name: format!("{id}.png"),
        origin: "upload".into(),
        session_id: None,
    }
}

const NO_LIMIT: Limits = Limits {
    account: 0,
    server: 0,
};

#[test]
fn quota_is_checked_inside_the_insert_and_dedupe_is_free() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "u1");
    user(&s, "u2");
    let limits = Limits {
        account: 100,
        server: 150,
    };
    let add = |mid: &str, bid: &str, owner: &str, sha: &str, size| {
        s.media_insert(
            &item(mid, Some(owner), None),
            &blob(bid, Some(owner), sha, size),
            limits,
        )
        .unwrap()
    };
    assert!(matches!(
        add("m1", "b1", "u1", "aa", 80),
        MediaInsert::Added { new_blob: true, .. }
    ));
    // Over the account's limit: nothing is written.
    assert_eq!(
        add("m2", "b2", "u1", "bb", 30),
        MediaInsert::OverQuota(OverQuota {
            used: 80,
            limit: 100,
            server: false
        })
    );
    assert!(s.media_get("m2").unwrap().is_none());
    assert!(s.blob("b2").unwrap().is_none());
    // The same bytes again cost nothing (and are the same item here).
    assert_eq!(
        add("m3", "b3", "u1", "aa", 80),
        MediaInsert::Duplicate {
            media_id: "m1".into()
        }
    );
    // Another account holding the same bytes gets its own blob: dedupe never
    // crosses accounts.
    assert!(matches!(
        add("m4", "b4", "u2", "aa", 60),
        MediaInsert::Added { new_blob: true, .. }
    ));
    // Now the server's limit: 140 stored, 15 more is over 150.
    assert_eq!(
        add("m5", "b5", "u2", "cc", 15),
        MediaInsert::OverQuota(OverQuota {
            used: 140,
            limit: 150,
            server: true
        })
    );
    let u = s.blob_usage(Some("u1")).unwrap();
    assert_eq!((u.used, u.media_bytes, u.media_items), (80, 80, 1));
    assert_eq!(s.blob_total().unwrap(), 140);
    // The API token is limited only by the server.
    let token = s
        .media_insert(
            &item("m6", None, None),
            &blob("b6", None, "dd", 5),
            Limits {
                account: 1,
                server: 150,
            },
        )
        .unwrap();
    assert!(matches!(token, MediaInsert::Added { .. }));
    assert_eq!(s.blob_usage(None).unwrap().used, 5);
}

#[test]
fn image_dimensions_and_preview_hints() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "u1");
    s.media_insert(
        &item("m1", Some("u1"), None),
        &blob("b1", Some("u1"), "aa", 10),
        NO_LIMIT,
    )
    .unwrap();
    let p = s.preview_get("b1").unwrap();
    assert_eq!(
        (p.width, p.height, p.has_thumb, p.no_thumb),
        (Some(640), Some(480), false, false)
    );
    s.preview_set_hints("b1", None, None, Some(1500)).unwrap();
    s.preview_set_no_thumb("b1", true).unwrap();
    let p = s.preview_get("b1").unwrap();
    assert_eq!(
        (p.width, p.duration_ms, p.no_thumb),
        (Some(640), Some(1500), true)
    );
    s.preview_put_thumb("b1", "image/webp", b"RIFF....WEBP")
        .unwrap();
    let p = s.preview_get("b1").unwrap();
    assert!(p.has_thumb && !p.no_thumb, "a thumbnail clears no_thumb");
    let (mime, bytes) = s.preview_thumb("b1").unwrap().unwrap();
    assert_eq!(
        (mime.as_str(), bytes.as_slice()),
        ("image/webp", &b"RIFF....WEBP"[..])
    );
    let row = s.media_get("m1").unwrap().unwrap();
    assert!(row.preview.has_thumb);
    assert_eq!(row.preview.duration_ms, Some(1500));
}

#[test]
fn a_blob_goes_with_its_last_item() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "u1");
    let personal = s.personal_workspace("u1").unwrap();
    let team = s.create_workspace("Team", "u1").unwrap().workspace_id;
    s.media_insert(
        &item("m1", Some("u1"), Some(&personal)),
        &blob("b1", Some("u1"), "aa", 10),
        NO_LIMIT,
    )
    .unwrap();
    // The same bytes in another workspace: a second item on the same blob.
    let r = s
        .media_insert(
            &item("m2", Some("u1"), Some(&team)),
            &blob("b2", Some("u1"), "aa", 10),
            NO_LIMIT,
        )
        .unwrap();
    assert_eq!(
        r,
        MediaInsert::Added {
            media_id: "m2".into(),
            new_blob: false
        }
    );
    assert_eq!(s.media_get("m2").unwrap().unwrap().blob.blob_id, "b1");
    let d = s.media_delete(Some("u1"), &["m1".into()]).unwrap();
    assert_eq!((d.items, d.blobs.len()), (1, 0), "still referenced");
    // Someone else's id deletes nothing.
    user(&s, "u2");
    let d = s.media_delete(Some("u2"), &["m2".into()]).unwrap();
    assert_eq!(d.items, 0);
    let d = s.media_delete(Some("u1"), &["m2".into()]).unwrap();
    assert_eq!((d.items, d.blobs), (1, vec!["b1".to_string()]));
    assert!(s.blob("b1").unwrap().is_none());
    assert!(s.blob_key("b1").unwrap().is_none());
    assert!(s.known_blob_ids().unwrap().is_empty());
}

#[test]
fn leftover_blobs_are_found_and_released() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "u1");
    s.media_insert(
        &item("m1", Some("u1"), None),
        &blob("b1", Some("u1"), "aa", 10),
        NO_LIMIT,
    )
    .unwrap();
    assert!(s.blobs_unreferenced(i64::MAX).unwrap().is_empty());
    // Only rows that are still unreferenced go.
    assert!(s.blob_delete(&["b1".into()]).unwrap().is_empty());
    s.media_delete(None, &["m1".into()]).unwrap();
    assert!(s.blob("b1").unwrap().is_none());
    let mut plain = blob("b9", Some("u1"), "zz", 1);
    plain.width = None;
    plain.height = None;
    assert!(matches!(
        s.media_insert(&item("m9", Some("u1"), None), &plain, NO_LIMIT)
            .unwrap(),
        MediaInsert::Added { .. }
    ));
    assert_eq!(
        s.known_blob_ids().unwrap().into_iter().collect::<Vec<_>>(),
        ["b9"]
    );
    assert_eq!(s.preview_get("b9").unwrap(), Default::default());
}

#[test]
fn account_deletion_takes_rows_keys_and_fts() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "u1");
    user(&s, "u2");
    for (m, b, sha) in [("m1", "b1", "aa"), ("m2", "b2", "bb")] {
        s.media_insert(
            &item(m, Some("u1"), None),
            &blob(b, Some("u1"), sha, 10),
            NO_LIMIT,
        )
        .unwrap();
    }
    s.media_insert(
        &item("m3", Some("u2"), None),
        &blob("b3", Some("u2"), "aa", 10),
        NO_LIMIT,
    )
    .unwrap();
    s.delete_user("u1").unwrap();
    assert!(s.media_get("m1").unwrap().is_none());
    assert!(s.blob_key("b1").unwrap().is_none());
    assert_eq!(
        s.known_blob_ids().unwrap().into_iter().collect::<Vec<_>>(),
        ["b3"]
    );
    // Only u2's name is left in the index.
    let hits = s.media_search(None, None, Some("\"m1\"*"), 10, 0).unwrap();
    assert!(hits.is_empty());
    let hits = s.media_search(None, None, Some("\"m3\"*"), 10, 0).unwrap();
    assert_eq!(hits.len(), 1);
    let (used, _) = s.storage_usage_all().unwrap();
    assert_eq!(used.len(), 1);
    assert_eq!((used[0].username.as_str(), used[0].used), ("u2", 10));
}
