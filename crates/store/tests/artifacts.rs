//! Artifacts in the store: lineages and their sequence, idempotent ingest,
//! caps, retention with the name index in step, the lineage's one Media
//! item, deletes, the hourly sweep, visibility, and account deletion.

use harness_store::artifacts::{ArtifactQuery, Committed, IngestCaps, Ingested, NewVersion};
use harness_store::blobs::{Limits, NewBlob};
use harness_store::{NewUser, Role, Store};

fn user(s: &Store, id: &str) {
    s.create_user(&NewUser {
        user_id: id.into(),
        username: id.into(),
        password_hash: "$argon2id$x".into(),
        role: Role::Member,
        can_agentic: true,
        must_change_password: false,
    })
    .unwrap();
}

const CAPS: IngestCaps = IngestCaps {
    pending_per_node: 8,
    per_owner: 200,
    per_workspace: 1000,
};

fn version(owner: &str, ws: &str, name: &str, n: u32) -> NewVersion {
    NewVersion {
        owner_id: owner.into(),
        workspace_id: ws.into(),
        project_id: None,
        name: name.into(),
        node_id: "local".into(),
        stage_id: format!("{n:032x}"),
        size: 10,
        sha256: format!("{n:064x}"),
        filename: format!("{name}-{n}.bin"),
        packed: None,
        by_user: false,
        session_id: Some("0193f2a0-0000-7000-8000-000000000001".into()),
        call_id: Some(format!("c{n}")),
        meta: "{}".into(),
        notes: String::new(),
    }
}

fn blob(owner: &str, n: u32, kind: &str) -> NewBlob {
    NewBlob {
        blob_id: format!("{:032x}", n + 50_000),
        owner_id: Some(owner.into()),
        sha256: format!("{n:064x}"),
        size: 10,
        kind: kind.into(),
        format: "x".into(),
        mime: "application/octet-stream".into(),
        display: "download".into(),
        key: vec![n as u8; 32],
        width: None,
        height: None,
    }
}

/// Ingest and commit version `n` of `name`; returns (artifact, version).
fn publish(s: &Store, owner: &str, ws: &str, name: &str, n: u32, kind: &str) -> (String, String) {
    let Ingested::Added {
        artifact_id,
        version_id,
        failed: None,
        ..
    } = s
        .artifact_ingest(&version(owner, ws, name, n), None, CAPS)
        .unwrap()
    else {
        panic!("not added")
    };
    let c = s
        .artifact_commit(&version_id, &blob(owner, n, kind), Limits::default(), 5)
        .unwrap();
    assert!(matches!(c, Committed::Ready { .. }), "{c:?}");
    (artifact_id, version_id)
}

fn fts_hits(s: &Store, owner: &str, ws: &str, word: &str) -> usize {
    s.artifact_list(&ArtifactQuery {
        viewer: Some(owner.into()),
        workspace: Some(ws.into()),
        fts: Some(format!("\"{word}\"")),
        limit: 50,
        ..Default::default()
    })
    .unwrap()
    .len()
}

#[test]
fn lineages_sequence_and_idempotent_ingest() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "alex");
    let ws = s.personal_workspace("alex").unwrap();
    let (a1, _) = publish(&s, "alex", &ws, "myapp", 1, "binary");
    let (a2, v2) = publish(&s, "alex", &ws, "myapp", 2, "binary");
    assert_eq!(a1, a2, "same name, same lineage");
    let (other, _) = publish(&s, "alex", &ws, "tool", 3, "binary");
    assert_ne!(other, a1);
    let versions = s.artifact_versions(&a1).unwrap();
    assert_eq!(versions.iter().map(|v| v.seq).collect::<Vec<_>>(), [2, 1]);
    assert_eq!(versions[0].version_id, v2);
    // The same node's stage again (a replayed event) adds nothing.
    let again = s
        .artifact_ingest(&version("alex", &ws, "myapp", 2), None, CAPS)
        .unwrap();
    assert_eq!(again, Ingested::Duplicate);
    // A failure is recorded as a failed version, not lost.
    let Ingested::Added { failed, .. } = s
        .artifact_ingest(&version("alex", &ws, "myapp", 9), Some("too old"), CAPS)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(failed.as_deref(), Some("too old"));
    let usage = s.blob_usage(Some("alex")).unwrap();
    assert_eq!((usage.artifact_versions, usage.artifact_bytes), (3, 30));
}

#[test]
fn caps_count_ready_lineages_and_pending_per_node() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "alex");
    let ws = s.personal_workspace("alex").unwrap();
    let caps = IngestCaps {
        pending_per_node: 2,
        per_owner: 2,
        per_workspace: 1000,
    };
    for (n, name) in [(1, "a"), (2, "b")] {
        let Ingested::Added { version_id, .. } = s
            .artifact_ingest(&version("alex", &ws, name, n), None, caps)
            .unwrap()
        else {
            panic!()
        };
        s.artifact_commit(
            &version_id,
            &blob("alex", n, "binary"),
            Limits::default(),
            5,
        )
        .unwrap();
    }
    // A third lineage is refused; a new version of an existing one is not.
    let Ingested::Added { failed, .. } = s
        .artifact_ingest(&version("alex", &ws, "c", 3), None, caps)
        .unwrap()
    else {
        panic!()
    };
    assert!(failed.unwrap().contains("2 artifacts"));
    let Ingested::Added { failed, .. } = s
        .artifact_ingest(&version("alex", &ws, "a", 4), None, caps)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(failed, None);
    let Ingested::Added { failed, .. } = s
        .artifact_ingest(&version("alex", &ws, "b", 5), None, caps)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(failed, None, "2 pending now");
    let Ingested::Added { failed, .. } = s
        .artifact_ingest(&version("alex", &ws, "b", 6), None, caps)
        .unwrap()
    else {
        panic!()
    };
    assert!(failed.unwrap().contains("waiting to upload"));
}

#[test]
fn retention_keeps_five_plus_pinned_and_the_index_follows() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "alex");
    let ws = s.personal_workspace("alex").unwrap();
    let (a, first) = publish(&s, "alex", &ws, "app", 1, "binary");
    assert!(s.artifact_pin(&a, &first, true, 10).unwrap());
    for n in 2..=8 {
        publish(&s, "alex", &ws, "app", n, "binary");
    }
    let seqs: Vec<u64> = s
        .artifact_versions(&a)
        .unwrap()
        .iter()
        .map(|v| v.seq)
        .collect();
    assert_eq!(seqs, [8, 7, 6, 5, 4, 1], "five newest and the pinned first");
    // The name index holds only the remaining versions' file names.
    assert_eq!(fts_hits(&s, "alex", &ws, "app-1.bin"), 1);
    assert_eq!(fts_hits(&s, "alex", &ws, "app-8.bin"), 1);
    assert_eq!(fts_hits(&s, "alex", &ws, "app-3.bin"), 0);
    assert_eq!(fts_hits(&s, "alex", &ws, "app-2.bin"), 0);
    // The dropped versions' blobs went with them.
    assert_eq!(s.known_blob_ids().unwrap().len(), 6);
    // Pins are capped.
    let v = s.artifact_versions(&a).unwrap();
    assert!(s.artifact_pin(&a, &v[0].version_id, true, 1).is_err());
}

#[test]
fn one_media_item_per_lineage_follows_the_newest_version() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "alex");
    let ws = s.personal_workspace("alex").unwrap();
    let (a, _) = publish(&s, "alex", &ws, "render", 1, "image");
    let (_, v2) = publish(&s, "alex", &ws, "render", 2, "image");
    let items = s
        .media_list(&harness_store::media::MediaQuery {
            owner: Some("alex".into()),
            limit: 10,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert_eq!(item.origin, "agent");
    assert_eq!(item.artifact_id.as_deref(), Some(a.as_str()));
    assert_eq!(item.version_id.as_deref(), Some(v2.as_str()));
    assert_eq!(
        (item.artifact_seq, item.artifact_name.as_deref()),
        (Some(2), Some("render"))
    );
    assert_eq!(item.blob.sha256, format!("{:064x}", 2));
    // Deleting the item leaves the artifact; the next version adds it back.
    s.media_delete(Some("alex"), std::slice::from_ref(&item.media_id))
        .unwrap();
    assert_eq!(s.artifact_versions(&a).unwrap().len(), 2);
    publish(&s, "alex", &ws, "render", 3, "image");
    let usage = s.blob_usage(Some("alex")).unwrap();
    assert_eq!(usage.media_items, 1);
    // Deleting the artifact keeps the item, and the blob it shows.
    let gone = s.artifact_delete(&a).unwrap();
    assert_eq!(gone.len(), 2, "the two older blobs");
    assert_eq!(s.blob_usage(Some("alex")).unwrap().media_items, 1);
}

#[test]
fn deleting_the_last_version_deletes_the_lineage() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "alex");
    let ws = s.personal_workspace("alex").unwrap();
    let (a, v1) = publish(&s, "alex", &ws, "app", 1, "binary");
    let (_, v2) = publish(&s, "alex", &ws, "app", 2, "binary");
    assert_eq!(
        s.artifact_version_delete(&a, &v1).unwrap().unwrap().len(),
        1
    );
    assert!(s.artifact_get(&a).unwrap().is_some());
    assert!(s.artifact_version_delete(&a, "nope").unwrap().is_none());
    s.artifact_version_delete(&a, &v2).unwrap();
    assert!(s.artifact_get(&a).unwrap().is_none());
    assert_eq!(fts_hits(&s, "alex", &ws, "app"), 0);
    assert!(s.known_blob_ids().unwrap().is_empty());
}

#[test]
fn the_hourly_sweep_fails_stale_pending_and_drops_empty_lineages() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "alex");
    let ws = s.personal_workspace("alex").unwrap();
    let Ingested::Added { artifact_id: a, .. } = s
        .artifact_ingest(&version("alex", &ws, "app", 1), None, CAPS)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(s.artifact_pending(Some("local")).unwrap().len(), 1);
    // A TTL of zero: the pending version is stale at once.
    std::thread::sleep(std::time::Duration::from_millis(3));
    let swept = s.artifacts_hourly(0).unwrap();
    assert_eq!(swept.expired.len(), 1);
    assert!(s.artifact_pending(None).unwrap().is_empty());
    // It had no ready version left: the lineage is gone too.
    assert!(s.artifact_get(&a).unwrap().is_none());
    // A lineage with a ready version stays; its failed ones beyond three go.
    let (b, _) = publish(&s, "alex", &ws, "tool", 2, "binary");
    for n in 3..8 {
        s.artifact_ingest(&version("alex", &ws, "tool", n), Some("x"), CAPS)
            .unwrap();
    }
    s.artifacts_hourly(3_600_000).unwrap();
    let statuses: Vec<String> = s
        .artifact_versions(&b)
        .unwrap()
        .into_iter()
        .map(|v| v.status)
        .collect();
    assert_eq!(statuses, ["failed", "failed", "failed", "ready"]);
}

#[test]
fn visibility_private_shared_and_fallback() {
    let s = Store::open_in_memory().unwrap();
    for u in ["alex", "bob", "carol"] {
        user(&s, u);
    }
    let team = s.create_workspace("Team", "alex").unwrap().workspace_id;
    s.add_member(&team, "bob", "member").unwrap();
    let (a, _) = publish(&s, "alex", &team, "app", 1, "binary");
    let list = |viewer: &str, ws: &str| {
        s.artifact_list(&ArtifactQuery {
            viewer: Some(viewer.into()),
            workspace: Some(ws.into()),
            limit: 50,
            ..Default::default()
        })
        .unwrap()
        .len()
    };
    assert_eq!(list("alex", &team), 1);
    assert_eq!(list("bob", &team), 0, "private");
    s.artifact_update(&a, None, Some("workspace")).unwrap();
    assert_eq!(list("bob", &team), 1, "shared");
    assert_eq!(list("carol", &team), 0, "not a member");
    assert_eq!(
        s.artifact_search(Some("bob"), Some(&team), None, 10, 0)
            .unwrap()
            .len(),
        1,
        "search uses the same rule"
    );
    // A name clash in the same place is refused.
    publish(&s, "alex", &team, "other", 2, "binary");
    assert!(s.artifact_update(&a, Some("other"), None).is_err());
    // The owner leaves: the artifact falls back to their Personal
    // workspace and is private again.
    s.add_member(&team, "carol", "owner").unwrap();
    s.remove_member(&team, "alex", None).unwrap();
    assert_eq!(list("bob", &team), 0);
    let personal = s.personal_workspace("alex").unwrap();
    assert_eq!(list("alex", &personal), 2);
    assert!(!s.artifact_get(&a).unwrap().unwrap().shared());
    // The API token sees all.
    let all = s
        .artifact_list(&ArtifactQuery {
            limit: 50,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(all.len(), 2);
}

#[test]
fn account_deletion_leaves_no_index_rows() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("m.db");
    let s = Store::open(&db, harness_store::Durability::Normal).unwrap();
    user(&s, "alex");
    let ws = s.personal_workspace("alex").unwrap();
    publish(&s, "alex", &ws, "secretproject", 1, "binary");
    assert_eq!(fts_hits(&s, "alex", &ws, "secretproject"), 1);
    s.delete_user("alex").unwrap();
    assert!(s.known_blob_ids().unwrap().is_empty());
    let all = s
        .artifact_search(None, None, Some("\"secretproject\""), 10, 0)
        .unwrap();
    assert!(all.is_empty());
    s.checkpoint().unwrap();
    let raw = rusqlite::Connection::open(&db).unwrap();
    let rows: i64 = raw
        .query_row("SELECT COUNT(*) FROM artifact_fts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 0);
}

#[test]
fn the_same_bytes_share_one_blob_until_the_last_version_goes() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "alex");
    let ws = s.personal_workspace("alex").unwrap();
    let (a, _) = publish(&s, "alex", &ws, "one", 1, "binary");
    // Another lineage with the same bytes: a dedupe hit, no new blob.
    let Ingested::Added {
        artifact_id: b,
        version_id,
        ..
    } = s
        .artifact_ingest(&version("alex", &ws, "two", 2), None, CAPS)
        .unwrap()
    else {
        panic!()
    };
    let c = s
        .artifact_commit(
            &version_id,
            &blob("alex", 1, "binary"),
            Limits::default(),
            5,
        )
        .unwrap();
    assert!(
        matches!(
            c,
            Committed::Ready {
                new_blob: false,
                ..
            }
        ),
        "{c:?}"
    );
    assert_eq!(s.known_blob_ids().unwrap().len(), 1);
    assert!(
        s.artifact_delete(&a).unwrap().is_empty(),
        "still held by the other"
    );
    assert_eq!(s.artifact_delete(&b).unwrap().len(), 1);
    assert!(s.known_blob_ids().unwrap().is_empty());
}
