//! Vectors of the built-in memory in a Qdrant server (REST API, run
//! unmodified as a separate service). Facts, statuses and keyword search
//! stay in `memory.db`; Qdrant only holds one vector per fact, so recall
//! still checks every hit against SQLite (a stale point never surfaces a
//! deleted or unconfirmed fact).
//!
//! * One collection per vector size: `<prefix>_<dims>`. A new embeddings
//!   model of another size starts a new collection instead of failing.
//! * Point id = the fact id (a UUID); payload `owner` (`""` for none) and
//!   `model`, both indexed and always filtered on.

use std::collections::HashSet;
use std::sync::Mutex;
use std::time::Duration;

use harness_core::Secret;
use serde_json::{Value, json};

use crate::{MemoryError, Result};

pub struct QdrantIndex {
    client: reqwest::Client,
    base: String,
    prefix: String,
    key: Option<Secret<String>>,
    /// Collections known to exist (created or seen).
    ready: Mutex<HashSet<String>>,
}

/// A fact id (32 hex digits) as the hyphenated UUID Qdrant expects.
fn point_id(id: &str) -> Option<String> {
    (id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())).then(|| {
        format!(
            "{}-{}-{}-{}-{}",
            &id[..8],
            &id[8..12],
            &id[12..16],
            &id[16..20],
            &id[20..]
        )
    })
}

fn fact_id(point: &Value) -> Option<String> {
    let s = point.as_str()?;
    let id: String = s.chars().filter(|c| *c != '-').collect();
    (id.len() == 32).then(|| id.to_ascii_lowercase())
}

fn owner_value(owner: Option<&str>) -> &str {
    owner.unwrap_or("")
}

impl QdrantIndex {
    pub fn new(base_url: &str, prefix: &str, key: Option<Secret<String>>) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(20))
                // Same as the embeddings client: no redirect may carry the
                // key to another host.
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("static client config"),
            base: base_url.trim_end_matches('/').to_string(),
            prefix: prefix.to_string(),
            key,
            ready: Mutex::new(HashSet::new()),
        }
    }

    fn collection(&self, dims: usize) -> String {
        format!("{}_{dims}", self.prefix)
    }

    async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value> {
        let mut rb = self.client.request(method, format!("{}{path}", self.base));
        if let Some(k) = &self.key {
            rb = rb.header("api-key", k.expose().as_str());
        }
        if let Some(b) = body {
            rb = rb.json(&b);
        }
        let r = rb
            .send()
            .await
            .map_err(|e| MemoryError::Embed(format!("qdrant: {e}")))?;
        let status = r.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(Value::Null);
        }
        if !status.is_success() {
            let body = r.text().await.unwrap_or_default();
            return Err(MemoryError::Embed(format!(
                "qdrant answered {status}: {}",
                crate::embed::error_detail(&body)
            )));
        }
        Ok(r.json().await.unwrap_or(Value::Null))
    }

    /// Create `<prefix>_<dims>` with its payload indexes unless it exists.
    async fn ensure(&self, dims: usize) -> Result<String> {
        let name = self.collection(dims);
        if self
            .ready
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .contains(&name)
        {
            return Ok(name);
        }
        let got = self
            .call(reqwest::Method::GET, &format!("/collections/{name}"), None)
            .await?;
        if got.is_null() {
            self.call(
                reqwest::Method::PUT,
                &format!("/collections/{name}"),
                Some(json!({ "vectors": { "size": dims, "distance": "Cosine" } })),
            )
            .await?;
            for field in ["owner", "model"] {
                self.call(
                    reqwest::Method::PUT,
                    &format!("/collections/{name}/index?wait=true"),
                    Some(json!({ "field_name": field, "field_schema": "keyword" })),
                )
                .await?;
            }
        }
        self.ready
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(name.clone());
        Ok(name)
    }

    /// Store vectors for `(fact id, owner, text vector)` under `model`.
    pub async fn upsert(
        &self,
        model: &str,
        points: Vec<(String, Option<String>, Vec<f32>)>,
    ) -> Result<()> {
        let Some(dims) = points.first().map(|p| p.2.len()).filter(|d| *d > 0) else {
            return Ok(());
        };
        let name = self.ensure(dims).await?;
        let points: Vec<Value> = points
            .into_iter()
            .filter(|p| p.2.len() == dims)
            .filter_map(|(id, owner, v)| {
                Some(json!({
                    "id": point_id(&id)?,
                    "vector": v,
                    "payload": { "owner": owner_value(owner.as_deref()), "model": model },
                }))
            })
            .collect();
        self.call(
            reqwest::Method::PUT,
            &format!("/collections/{name}/points?wait=true"),
            Some(json!({ "points": points })),
        )
        .await?;
        Ok(())
    }

    /// The `limit` nearest facts of `owner` under `model`: `(fact id, score)`.
    pub async fn search(
        &self,
        owner: Option<&str>,
        model: &str,
        vector: &[f32],
        limit: usize,
    ) -> Result<Vec<(String, f32)>> {
        if vector.is_empty() {
            return Ok(vec![]);
        }
        let name = self.ensure(vector.len()).await?;
        let v = self
            .call(
                reqwest::Method::POST,
                &format!("/collections/{name}/points/search"),
                Some(json!({
                    "vector": vector,
                    "limit": limit,
                    "with_payload": false,
                    "filter": { "must": [
                        { "key": "owner", "match": { "value": owner_value(owner) } },
                        { "key": "model", "match": { "value": model } },
                    ]},
                })),
            )
            .await?;
        Ok(v["result"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|p| Some((fact_id(&p["id"])?, p["score"].as_f64()? as f32)))
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Remove the facts' vectors from every collection of this prefix (a
    /// deleted memory must not linger on another server).
    pub async fn delete(&self, ids: &[String]) -> Result<()> {
        let points: Vec<String> = ids.iter().filter_map(|i| point_id(i)).collect();
        if points.is_empty() {
            return Ok(());
        }
        let list = self
            .call(reqwest::Method::GET, "/collections", None)
            .await?;
        let want = format!("{}_", self.prefix);
        let names: Vec<String> = list["result"]["collections"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|c| c["name"].as_str())
                    .filter(|n| {
                        n.strip_prefix(&want)
                            .is_some_and(|d| d.bytes().all(|b| b.is_ascii_digit()))
                    })
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        for name in names {
            self.call(
                reqwest::Method::POST,
                &format!("/collections/{name}/points/delete?wait=true"),
                Some(json!({ "points": points })),
            )
            .await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Against a real Qdrant: `HARNESS_TEST_QDRANT_URL=http://127.0.0.1:6333
    /// HARNESS_TEST_QDRANT_KEY=... cargo test -p harness-memory qdrant_live -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn qdrant_live() {
        let url = std::env::var("HARNESS_TEST_QDRANT_URL").expect("HARNESS_TEST_QDRANT_URL");
        let key = std::env::var("HARNESS_TEST_QDRANT_KEY")
            .ok()
            .map(Secret::new);
        let ix = QdrantIndex::new(&url, "rh_test", key);
        let (a, b, c) = (
            "0192f1c2a3b47c8d9e0f1a2b3c4d5e6a".to_string(),
            "0192f1c2a3b47c8d9e0f1a2b3c4d5e6b".to_string(),
            "0192f1c2a3b47c8d9e0f1a2b3c4d5e6c".to_string(),
        );
        ix.upsert(
            "m1",
            vec![
                (a.clone(), Some("u1".into()), vec![1.0, 0.0, 0.0]),
                (b.clone(), Some("u1".into()), vec![0.0, 1.0, 0.0]),
                (c.clone(), None, vec![1.0, 0.0, 0.0]),
            ],
        )
        .await
        .unwrap();
        let hits = ix
            .search(Some("u1"), "m1", &[0.9, 0.1, 0.0], 10)
            .await
            .unwrap();
        assert_eq!(
            hits.first().map(|h| h.0.as_str()),
            Some(a.as_str()),
            "{hits:?}"
        );
        assert!(
            hits.iter().all(|h| h.0 != c),
            "another owner's fact: {hits:?}"
        );
        assert!(
            ix.search(Some("u1"), "m2", &[1.0, 0.0, 0.0], 10)
                .await
                .unwrap()
                .is_empty(),
            "another model"
        );
        let none = ix.search(None, "m1", &[1.0, 0.0, 0.0], 10).await.unwrap();
        assert_eq!(
            none.iter().map(|h| h.0.clone()).collect::<Vec<_>>(),
            vec![c.clone()]
        );
        // Another vector size lands in its own collection.
        ix.upsert(
            "m4",
            vec![(a.clone(), Some("u1".into()), vec![1.0, 0.0, 0.0, 0.0])],
        )
        .await
        .unwrap();
        ix.delete(&[a.clone(), b.clone(), c.clone()]).await.unwrap();
        assert!(
            ix.search(Some("u1"), "m1", &[1.0, 0.0, 0.0], 10)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            ix.search(Some("u1"), "m4", &[1.0, 0.0, 0.0, 0.0], 10)
                .await
                .unwrap()
                .is_empty()
        );
        // A wrong key is an error, not an empty answer.
        let bad = QdrantIndex::new(&url, "rh_test", Some(Secret::new("nope".into())));
        assert!(
            bad.search(Some("u1"), "m1", &[1.0, 0.0, 0.0], 1)
                .await
                .is_err()
        );
    }

    #[test]
    fn ids_round_trip() {
        let id = "0192f1c2a3b47c8d9e0f1a2b3c4d5e6f";
        let p = point_id(id).unwrap();
        assert_eq!(p, "0192f1c2-a3b4-7c8d-9e0f-1a2b3c4d5e6f");
        assert_eq!(fact_id(&Value::String(p)).as_deref(), Some(id));
        assert!(point_id("not-a-fact-id").is_none());
    }
}
