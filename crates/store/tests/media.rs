//! Media items: private lists filtered by the effective workspace, kinds,
//! paging, the name index, and the fallback when an owner leaves.

use harness_store::blobs::{Limits, NewBlob};
use harness_store::media::{MediaQuery, NewItem};
use harness_store::{HIT_END, HIT_START, NewUser, Role, Store};

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

/// Add `name` for `owner` in `ws`; `kind`/`display` as the sniffer said.
fn add(
    s: &Store,
    n: u32,
    owner: &str,
    ws: Option<&str>,
    name: &str,
    kind: &str,
    display: &str,
) -> String {
    let id = format!("{n:032x}");
    let item = NewItem {
        media_id: id.clone(),
        owner_id: Some(owner.into()),
        workspace_id: ws.map(str::to_string),
        name: name.into(),
        origin: "upload".into(),
        session_id: None,
    };
    let blob = NewBlob {
        blob_id: format!("{:032x}", n + 1000),
        owner_id: Some(owner.into()),
        sha256: format!("{n:064x}"),
        size: 10,
        kind: kind.into(),
        format: "x".into(),
        mime: "x/y".into(),
        display: display.into(),
        key: vec![1; 32],
        width: None,
        height: None,
    };
    s.media_insert(&item, &blob, Limits::default()).unwrap();
    id
}

fn q(owner: Option<&str>, ws: Option<&str>) -> MediaQuery {
    MediaQuery {
        owner: owner.map(str::to_string),
        workspace: ws.map(str::to_string),
        limit: 100,
        ..Default::default()
    }
}

fn names(s: &Store, q: &MediaQuery) -> Vec<String> {
    s.media_list(q)
        .unwrap()
        .into_iter()
        .map(|r| r.name)
        .collect()
}

#[test]
fn lists_are_private_and_filtered_by_workspace_and_kind() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "alex");
    user(&s, "bob");
    let personal = s.personal_workspace("alex").unwrap();
    let team = s.create_workspace("Team", "alex").unwrap().workspace_id;
    s.add_member(&team, "bob", "member").unwrap();
    add(&s, 1, "alex", Some(&personal), "cat.png", "image", "inline");
    add(&s, 2, "alex", Some(&team), "talk.mp4", "video", "inline");
    add(&s, 3, "alex", Some(&team), "song.mp3", "audio", "inline");
    add(&s, 4, "alex", Some(&team), "raw.tiff", "image", "download");
    add(&s, 5, "bob", Some(&team), "bobs.png", "image", "inline");

    assert_eq!(names(&s, &q(Some("alex"), Some(&personal))), ["cat.png"]);
    assert_eq!(
        names(&s, &q(Some("alex"), Some(&team))),
        ["raw.tiff", "song.mp3", "talk.mp4"]
    );
    // The workspace filters; it never shares: bob sees only his own there.
    assert_eq!(names(&s, &q(Some("bob"), Some(&team))), ["bobs.png"]);
    // The API token, without a workspace: everything.
    assert_eq!(s.media_list(&q(None, None)).unwrap().len(), 5);

    let kind = |k: &str| MediaQuery {
        kind: Some(k.into()),
        ..q(Some("alex"), Some(&team))
    };
    assert_eq!(names(&s, &kind("video")), ["talk.mp4"]);
    assert_eq!(names(&s, &kind("other")), ["raw.tiff"]);
    assert!(names(&s, &kind("image")).is_empty(), "a tiff is Other");
    let raw = s.media_list(&kind("other")).unwrap().remove(0);
    assert_eq!((raw.kind(), raw.blob.kind.as_str()), ("other", "image"));
}

#[test]
fn paging_by_cursor() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "alex");
    for n in 1..=5 {
        add(&s, n, "alex", None, &format!("f{n}.png"), "image", "inline");
    }
    let mut page = MediaQuery {
        limit: 2,
        ..q(Some("alex"), None)
    };
    let mut seen = Vec::new();
    loop {
        let rows = s.media_list(&page).unwrap();
        if rows.is_empty() {
            break;
        }
        let last = rows.last().unwrap();
        page.before = Some((last.created_ms, last.media_id.clone()));
        seen.extend(rows.into_iter().map(|r| r.name));
    }
    assert_eq!(seen, ["f5.png", "f4.png", "f3.png", "f2.png", "f1.png"]);
}

#[test]
fn an_owner_who_leaves_takes_their_items_to_personal() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "alex");
    user(&s, "bob");
    let team = s.create_workspace("Team", "alex").unwrap().workspace_id;
    s.add_member(&team, "bob", "member").unwrap();
    let id = add(&s, 1, "bob", Some(&team), "holiday.jpg", "image", "inline");
    assert_eq!(names(&s, &q(Some("bob"), Some(&team))), ["holiday.jpg"]);
    s.remove_member(&team, "bob", None).unwrap();
    let personal = s.personal_workspace("bob").unwrap();
    assert!(names(&s, &q(Some("bob"), Some(&team))).is_empty());
    assert_eq!(names(&s, &q(Some("bob"), Some(&personal))), ["holiday.jpg"]);
    assert_eq!(
        s.media_get(&id).unwrap().unwrap().workspace_id.as_deref(),
        Some(personal.as_str())
    );
    // A deleted workspace (SET NULL) falls back the same way.
    let team2 = s.create_workspace("Other", "alex").unwrap().workspace_id;
    add(&s, 2, "alex", Some(&team2), "plan.png", "image", "inline");
    s.delete_workspace(&team2).unwrap();
    let alex_personal = s.personal_workspace("alex").unwrap();
    assert_eq!(
        names(&s, &q(Some("alex"), Some(&alex_personal))),
        ["plan.png"]
    );
}

#[test]
fn names_are_searched_marked_and_renamed() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "alex");
    user(&s, "bob");
    let a = add(
        &s,
        1,
        "alex",
        None,
        "Sunset over Riga.jpg",
        "image",
        "inline",
    );
    add(&s, 2, "alex", None, "budget.png", "image", "inline");
    add(&s, 3, "bob", None, "sunset copy.jpg", "image", "inline");
    let hits = s
        .media_search(Some("alex"), None, Some("\"sunset\"*"), 10, 0)
        .unwrap();
    assert_eq!(hits.len(), 1, "bob's sunset is not alex's");
    assert_eq!(
        hits[0].1,
        format!("{HIT_START}Sunset{HIT_END} over Riga.jpg")
    );
    // The list's q uses the same index.
    let found = MediaQuery {
        fts: Some("\"riga\"*".into()),
        ..q(Some("alex"), None)
    };
    assert_eq!(names(&s, &found), ["Sunset over Riga.jpg"]);
    // Without a query: the newest first.
    let newest = s.media_search(Some("alex"), None, None, 10, 0).unwrap();
    assert_eq!(newest.len(), 2);
    assert!(s.media_rename(&a, "Dawn in Tallinn.jpg").unwrap());
    assert!(
        s.media_search(Some("alex"), None, Some("\"riga\"*"), 10, 0)
            .unwrap()
            .is_empty()
    );
    let hits = s
        .media_search(Some("alex"), None, Some("\"tallinn\"*"), 10, 0)
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert!(!s.media_rename("missing", "x").unwrap());
}
