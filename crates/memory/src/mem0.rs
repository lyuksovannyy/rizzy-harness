//! Adapter for a self-hosted mem0 server (Apache-2.0, run unmodified as a
//! separate service). We send `infer: false`, so mem0 stores our text as is
//! and never calls its own LLM: no second model bill or data path. Our
//! fields (status, kind, source chat, dedupe key) live in `metadata`.
//!
//! Shapes follow mem0's server (`server/main.py`, checked 2026-11-03);
//! verify against a running instance's `/docs` before relying on it.

use std::time::Duration;

use harness_core::{Secret, SessionId};
use serde_json::{Value, json};

use crate::{Fact, FactKind, MemoryError, MemoryStore, NewFact, Result, Status};

/// mem0 requires a user id; facts without an owner (API token) use this.
const NO_OWNER: &str = "_harness_api";

pub struct Mem0Memory {
    client: reqwest::Client,
    base: String,
    key: Option<Secret<String>>,
}

impl Mem0Memory {
    pub fn new(base_url: &str, key: Option<Secret<String>>) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(20))
                .build()
                .expect("static client config"),
            base: base_url.trim_end_matches('/').to_string(),
            key,
        }
    }

    async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value> {
        let mut rb = self.client.request(method, format!("{}{path}", self.base));
        if let Some(k) = &self.key {
            rb = rb.header("X-API-Key", k.expose().as_str());
        }
        if let Some(b) = body {
            rb = rb.json(&b);
        }
        let r = rb
            .send()
            .await
            .map_err(|e| MemoryError::Embed(format!("mem0: {e}")))?;
        let status = r.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(Value::Null);
        }
        if !status.is_success() {
            return Err(MemoryError::Embed(format!("mem0 answered {status}")));
        }
        Ok(r.json().await.unwrap_or(Value::Null))
    }
}

fn user(owner: Option<&str>) -> &str {
    owner.unwrap_or(NO_OWNER)
}

fn ms(v: &Value) -> i64 {
    v.as_str()
        .and_then(|s| s.parse::<jiff::Timestamp>().ok())
        .map(|t| t.as_millisecond())
        .unwrap_or(0)
}

fn parse(v: &Value) -> Option<Fact> {
    let md = &v["metadata"];
    let owner = v["user_id"]
        .as_str()
        .filter(|u| *u != NO_OWNER)
        .map(str::to_string);
    Some(Fact {
        id: v["id"].as_str()?.to_string(),
        owner_id: owner,
        text: v["memory"].as_str()?.to_string(),
        kind: match md["kind"].as_str() {
            Some("preference") => FactKind::Preference,
            Some("location") => FactKind::Location,
            _ => FactKind::Fact,
        },
        status: if md["status"].as_str() == Some("proposed") {
            Status::Proposed
        } else {
            Status::Active
        },
        source_session: md["source_session"]
            .as_str()
            .and_then(|s| s.parse::<SessionId>().ok()),
        created_ms: ms(&v["created_at"]),
        updated_ms: ms(&v["updated_at"]).max(ms(&v["created_at"])),
    })
}

fn results(v: &Value) -> Vec<Value> {
    v["results"]
        .as_array()
        .or_else(|| v.as_array())
        .cloned()
        .unwrap_or_default()
}

fn metadata(kind: FactKind, status: Status, source: Option<SessionId>, key: Option<&str>) -> Value {
    json!({
        "kind": match kind { FactKind::Fact => "fact", FactKind::Preference => "preference", FactKind::Location => "location" },
        "status": match status { Status::Proposed => "proposed", Status::Active => "active" },
        "source_session": source.map(|s| s.to_string()),
        "dedupe_key": key,
    })
}

#[async_trait::async_trait]
impl MemoryStore for Mem0Memory {
    fn name(&self) -> &'static str {
        "mem0"
    }

    async fn add(&self, f: NewFact) -> Result<Option<Fact>> {
        let text = crate::clean_text(&f.text)?;
        let uid = user(f.owner_id.as_deref()).to_string();
        if let Some(k) = &f.dedupe_key {
            let all = self
                .call(
                    reqwest::Method::GET,
                    &format!("/memories?user_id={uid}&top_k=1000"),
                    None,
                )
                .await?;
            if results(&all)
                .iter()
                .any(|m| m["metadata"]["dedupe_key"].as_str() == Some(k))
            {
                return Ok(None);
            }
        }
        let v = self
            .call(
                reqwest::Method::POST,
                "/memories",
                Some(json!({
                    "messages": [{"role": "user", "content": text}],
                    "user_id": uid,
                    "metadata": metadata(f.kind, f.status, f.source_session, f.dedupe_key.as_deref()),
                    "infer": false,
                })),
            )
            .await?;
        let id = results(&v)
            .first()
            .and_then(|r| r["id"].as_str().map(str::to_string))
            .ok_or_else(|| MemoryError::Embed("mem0 returned no id".into()))?;
        self.get(&id).await
    }

    async fn get(&self, id: &str) -> Result<Option<Fact>> {
        let v = self
            .call(reqwest::Method::GET, &format!("/memories/{id}"), None)
            .await?;
        Ok(parse(&v))
    }

    async fn list(&self, owner: Option<&str>) -> Result<Vec<Fact>> {
        let v = self
            .call(
                reqwest::Method::GET,
                &format!("/memories?user_id={}&top_k=1000", user(owner)),
                None,
            )
            .await?;
        let mut out: Vec<Fact> = results(&v).iter().filter_map(parse).collect();
        out.sort_by_key(|f| std::cmp::Reverse(f.updated_ms));
        Ok(out)
    }

    async fn update(
        &self,
        id: &str,
        text: Option<String>,
        status: Option<Status>,
    ) -> Result<Option<Fact>> {
        let Some(cur) = self.get(id).await? else {
            return Ok(None);
        };
        let text = match text {
            Some(t) => crate::clean_text(&t)?,
            None => cur.text.clone(),
        };
        let raw = self
            .call(reqwest::Method::GET, &format!("/memories/{id}"), None)
            .await?;
        let md = metadata(
            cur.kind,
            status.unwrap_or(cur.status),
            cur.source_session,
            raw["metadata"]["dedupe_key"].as_str(),
        );
        self.call(
            reqwest::Method::PUT,
            &format!("/memories/{id}"),
            Some(json!({ "text": text, "metadata": md })),
        )
        .await?;
        self.get(id).await
    }

    async fn delete(&self, id: &str) -> Result<bool> {
        let existed = self.get(id).await?.is_some();
        if existed {
            self.call(reqwest::Method::DELETE, &format!("/memories/{id}"), None)
                .await?;
        }
        Ok(existed)
    }

    async fn recall(&self, owner: Option<&str>, query: &str, k: usize) -> Result<Vec<Fact>> {
        let v = self
            .call(
                reqwest::Method::POST,
                "/search",
                Some(json!({
                    "query": query,
                    "filters": { "user_id": user(owner) },
                    "top_k": k * 3,
                    "threshold": 0.3,
                })),
            )
            .await?;
        Ok(results(&v)
            .iter()
            .filter_map(parse)
            .filter(|f| f.status == Status::Active && f.owner_id.as_deref() == owner)
            .take(k)
            .collect())
    }
}
