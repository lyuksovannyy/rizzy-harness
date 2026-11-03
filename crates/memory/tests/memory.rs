//! Built-in memory store: CRUD, proposals, owner scoping, recall.

use axum::Json;
use axum::routing::post;
use harness_memory::*;
use serde_json::{Value, json};

fn fact(owner: &str, text: &str, status: Status) -> NewFact {
    NewFact {
        owner_id: Some(owner.into()),
        text: text.into(),
        kind: FactKind::Fact,
        status,
        source_session: None,
        dedupe_key: None,
    }
}

#[tokio::test]
async fn crud_proposals_and_scoping() {
    let m = SqliteMemory::open_in_memory().unwrap();
    let a = m
        .add(fact("alex", "Alex has a dog named Pixel", Status::Active))
        .await
        .unwrap()
        .unwrap();
    let p = m
        .add(fact("alex", "Alex prefers metric units", Status::Proposed))
        .await
        .unwrap()
        .unwrap();
    m.add(fact("bob", "Bob has a dog named Rex", Status::Active))
        .await
        .unwrap();

    // Recall: only active, only the owner's.
    let r = m
        .recall(Some("alex"), "what is my dog called?", 5)
        .await
        .unwrap();
    assert_eq!(
        r.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(),
        vec![a.id.as_str()]
    );
    assert!(
        m.recall(Some("alex"), "metric units", 5)
            .await
            .unwrap()
            .is_empty(),
        "proposals unused"
    );
    // Confirm the proposal: now it is recalled.
    m.update(&p.id, None, Some(Status::Active)).await.unwrap();
    assert_eq!(
        m.recall(Some("alex"), "which units", 5).await.unwrap()[0].id,
        p.id
    );
    // Edit re-indexes.
    m.update(&a.id, Some("Alex has a cat named Pixel".into()), None)
        .await
        .unwrap();
    assert!(m.recall(Some("alex"), "dog", 5).await.unwrap().is_empty());
    assert_eq!(m.recall(Some("alex"), "cat", 5).await.unwrap().len(), 1);
    assert_eq!(m.list(Some("alex")).await.unwrap().len(), 2);
    assert!(m.delete(&a.id).await.unwrap());
    assert!(m.get(&a.id).await.unwrap().is_none());
    // FTS syntax is inert; empty or oversized text is refused.
    m.recall(Some("alex"), "\"units* NEAR( OR", 5)
        .await
        .unwrap();
    assert!(m.add(fact("alex", "  ", Status::Active)).await.is_err());
    assert!(
        m.add(fact(
            "alex",
            &"x".repeat(MAX_FACT_CHARS + 1),
            Status::Active
        ))
        .await
        .is_err()
    );
}

#[tokio::test]
async fn dedupe_key_blocks_replays() {
    let m = SqliteMemory::open_in_memory().unwrap();
    let mut f = fact("alex", "Lives in Riga", Status::Proposed);
    f.dedupe_key = Some("call-1".into());
    assert!(m.add(f.clone()).await.unwrap().is_some());
    assert!(m.add(f).await.unwrap().is_none());
    assert_eq!(m.list(Some("alex")).await.unwrap().len(), 1);
}

/// Fake /embeddings: a 3-d vector by topic word, so "puppy" finds "dog".
async fn fake_embeddings() -> String {
    let app = axum::Router::new().route(
        "/v1/embeddings",
        post(|Json(b): Json<Value>| async move {
            let inputs: Vec<String> = serde_json::from_value(b["input"].clone()).unwrap();
            let data: Vec<Value> = inputs
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    let t = t.to_lowercase();
                    let v = if t.contains("dog") || t.contains("puppy") {
                        [1.0, 0.0, 0.0]
                    } else if t.contains("tram") || t.contains("bus") {
                        [0.0, 1.0, 0.0]
                    } else {
                        [0.0, 0.0, 1.0]
                    };
                    json!({"index": i, "embedding": v})
                })
                .collect();
            Json(json!({"data": data}))
        }),
    );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let a = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    format!("http://{a}/v1")
}

#[tokio::test]
async fn embeddings_find_meaning_and_degrade_gracefully() {
    let m = SqliteMemory::open_in_memory().unwrap();
    let dog = m
        .add(fact("alex", "Alex has a dog named Pixel", Status::Active))
        .await
        .unwrap()
        .unwrap();
    m.add(fact("alex", "Alex takes the tram to work", Status::Active))
        .await
        .unwrap();
    // Keywords alone cannot connect "puppy" to "dog".
    assert!(
        m.recall(Some("alex"), "my puppy is sick", 3)
            .await
            .unwrap()
            .is_empty()
    );
    m.set_embedder(Some(Embedder::new(&fake_embeddings().await, "fake", None)));
    let r = m.recall(Some("alex"), "my puppy is sick", 3).await.unwrap();
    assert_eq!(r[0].id, dog.id);
    // A dead endpoint falls back to keywords instead of failing the chat.
    m.set_embedder(Some(Embedder::new("http://127.0.0.1:1/v1", "fake", None)));
    let r = m.recall(Some("alex"), "dog", 3).await.unwrap();
    assert_eq!(r[0].id, dog.id);
}
