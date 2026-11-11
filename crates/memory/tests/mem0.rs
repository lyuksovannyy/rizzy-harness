//! The mem0 adapter against a fake server shaped like mem0's API.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use harness_memory::*;
use serde_json::{Value, json};

type Db = Arc<Mutex<HashMap<String, Value>>>;

fn authed(h: &HeaderMap) -> bool {
    h.get("x-api-key").map(|v| v.as_bytes()) == Some(b"k")
}

async fn fake() -> (String, Db) {
    let db: Db = Arc::default();
    let app = Router::new()
        .route(
            "/memories",
            post(|State(db): State<Db>, h: HeaderMap, Json(b): Json<Value>| async move {
                if !authed(&h) {
                    return StatusCode::UNAUTHORIZED.into_response();
                }
                assert_eq!(b["infer"], false, "must never ask mem0 to call its LLM");
                let id = format!("m{}", db.lock().unwrap().len() + 1);
                let m = json!({"id": id, "memory": b["messages"][0]["content"], "user_id": b["user_id"],
                    "metadata": b["metadata"], "created_at": "2026-11-03T10:00:00Z", "updated_at": null});
                db.lock().unwrap().insert(id.clone(), m);
                Json(json!({"results": [{"id": id, "memory": "x", "event": "ADD"}]})).into_response()
            })
            .get(|State(db): State<Db>, Query(q): Query<HashMap<String, String>>| async move {
                let u = q.get("user_id").cloned().unwrap_or_default();
                let v: Vec<Value> = db.lock().unwrap().values().filter(|m| m["user_id"] == u.as_str()).cloned().collect();
                Json(json!({"results": v}))
            }),
        )
        .route(
            "/memories/{id}",
            get(|State(db): State<Db>, Path(id): Path<String>| async move {
                match db.lock().unwrap().get(&id) {
                    Some(m) => Json(m.clone()).into_response(),
                    None => StatusCode::NOT_FOUND.into_response(),
                }
            })
            .put(|State(db): State<Db>, Path(id): Path<String>, Json(b): Json<Value>| async move {
                if let Some(m) = db.lock().unwrap().get_mut(&id) {
                    m["memory"] = b["text"].clone();
                    m["metadata"] = b["metadata"].clone();
                    m["updated_at"] = json!("2026-11-03T11:00:00Z");
                }
                Json(json!({"message": "ok"}))
            })
            .delete(|State(db): State<Db>, Path(id): Path<String>| async move {
                db.lock().unwrap().remove(&id);
                Json(json!({"message": "Memory deleted successfully"}))
            }),
        )
        .route(
            "/search",
            post(|State(db): State<Db>, Json(b): Json<Value>| async move {
                let q = b["query"].as_str().unwrap().to_lowercase();
                let u = b["filters"]["user_id"].clone();
                let v: Vec<Value> = db
                    .lock()
                    .unwrap()
                    .values()
                    .filter(|m| m["user_id"] == u)
                    .filter(|m| q.split(' ').any(|w| w.len() > 2 && m["memory"].as_str().unwrap().to_lowercase().contains(w)))
                    .cloned()
                    .collect();
                Json(json!({"results": v}))
            }),
        )
        .with_state(db.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let a = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (format!("http://{a}"), db)
}

#[tokio::test]
async fn mem0_adapter_round_trip() {
    let (base, _db) = fake().await;
    let m = Mem0Memory::new(&base, Some(harness_core::Secret::new("k".into())));
    let nf = |text: &str, status, key: Option<&str>| NewFact {
        owner_id: Some("alex".into()),
        text: text.into(),
        kind: FactKind::Location,
        status,
        source_session: None,
        dedupe_key: key.map(str::to_string),
    };
    let p = m
        .add(nf("Alex lives in Riga", Status::Proposed, Some("c1")))
        .await
        .unwrap()
        .unwrap();
    assert_eq!((p.status, p.kind), (Status::Proposed, FactKind::Location));
    assert!(
        m.add(nf("Alex lives in Riga", Status::Proposed, Some("c1")))
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        m.recall(Some("alex"), "where does alex live riga", 5)
            .await
            .unwrap()
            .is_empty()
    );
    let c = m
        .update(&p.id, None, Some(Status::Active))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(c.status, Status::Active);
    assert_eq!(
        m.recall(Some("alex"), "riga trams", 5).await.unwrap().len(),
        1
    );
    assert!(m.recall(Some("bob"), "riga", 5).await.unwrap().is_empty());
    assert_eq!(m.list(Some("alex")).await.unwrap().len(), 1);
    assert!(m.delete(&p.id).await.unwrap());
    assert!(m.get(&p.id).await.unwrap().is_none());
    let bad = Mem0Memory::new(&base, Some(harness_core::Secret::new("wrong".into())));
    assert!(bad.add(nf("x y z", Status::Active, None)).await.is_err());
}

/// Records every request line, answers 404 (so the adapter reads "no such
/// memory").
async fn recorder() -> (String, Arc<Mutex<Vec<String>>>) {
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let s = seen.clone();
    let app = Router::new().fallback(move |req: axum::extract::Request| {
        let s = s.clone();
        async move {
            let pq = req
                .uri()
                .path_and_query()
                .map(|p| p.to_string())
                .unwrap_or_default();
            s.lock().unwrap().push(format!("{} {pq}", req.method()));
            StatusCode::NOT_FOUND
        }
    });
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let a = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (format!("http://{a}"), seen)
}

/// An id is one path segment, whatever it contains (audit L13): `/`, `?`,
/// `#` and `%` cannot change the path or reach another endpoint, and `.`
/// and `..` are not sent at all.
#[tokio::test]
async fn mem0_ids_are_percent_encoded() {
    let (base, seen) = recorder().await;
    let m = Mem0Memory::new(&base, Some(harness_core::Secret::new("k".into())));
    for id in [
        "../search",
        "a/b",
        "x?user_id=bob",
        "x#frag",
        "%2fsearch",
        "a b",
    ] {
        assert!(m.get(id).await.unwrap().is_none(), "{id}");
        assert!(!m.delete(id).await.unwrap(), "{id}");
        assert!(
            m.update(id, Some("t".into()), None)
                .await
                .unwrap()
                .is_none(),
            "{id}"
        );
    }
    for id in [".", ".."] {
        assert!(m.get(id).await.unwrap().is_none(), "{id}");
        assert!(!m.delete(id).await.unwrap(), "{id}");
    }
    let seen = seen.lock().unwrap().clone();
    assert!(!seen.is_empty());
    for line in &seen {
        let path = line.split_once(' ').unwrap().1;
        let rest = path
            .strip_prefix("/memories/")
            .unwrap_or_else(|| panic!("left /memories/: {line}"));
        assert!(!rest.contains(['/', '?', '#']), "{line}");
        assert!(rest != "." && rest != "..", "{line}");
    }
    assert!(
        seen.contains(&"GET /memories/..%2Fsearch".to_string()),
        "{seen:?}"
    );
    assert!(
        seen.contains(&"GET /memories/x%3Fuser_id=bob".to_string()),
        "{seen:?}"
    );
    assert!(
        seen.contains(&"GET /memories/%252fsearch".to_string()),
        "{seen:?}"
    );
}

/// An owner id with query syntax stays one `user_id` value.
#[tokio::test]
async fn mem0_owner_id_is_one_query_value() {
    let (base, seen) = recorder().await;
    let m = Mem0Memory::new(&base, None);
    let _ = m.list(Some("a&user_id=bob#x")).await;
    let seen = seen.lock().unwrap().clone();
    assert_eq!(
        seen,
        ["GET /memories?user_id=a%26user_id%3Dbob%23x&top_k=1000"]
    );
}

/// A mem0 server cannot move the harness, and its API key, to another host.
#[tokio::test]
async fn mem0_does_not_follow_cross_origin_redirects() {
    let (other, hits) = recorder().await;
    let target = format!("{other}/steal");
    let app = Router::new().fallback(move || {
        let t = target.clone();
        async move { (StatusCode::TEMPORARY_REDIRECT, [("location", t)]) }
    });
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let a = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    let m = Mem0Memory::new(
        &format!("http://{a}"),
        Some(harness_core::Secret::new("k".into())),
    );
    assert!(m.get("m1").await.is_err());
    assert!(m.list(Some("alex")).await.is_err());
    assert!(
        hits.lock().unwrap().is_empty(),
        "followed: {:?}",
        hits.lock().unwrap()
    );
}
