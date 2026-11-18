//! The Plugins screen's endpoints (ADOPTION_PLAN X.34 step A, PLAN.md
//! T80; `crate::plugins`):
//!
//! * `GET /api/workspaces/{id}/plugins`: for any member, the skills and
//!   connectors (MCP servers) the nodes announce, each with the nodes that
//!   announce it and this workspace's choice (Shared, or Assigned and to
//!   which of its agents); the workspace's agents for the picker; whether
//!   the caller may change it.
//! * `PUT /api/workspaces/{id}/plugins/{kind}/{name}`: for whoever holds
//!   the workspace's `settings` permission, `{"access": "shared"}` or
//!   `{"access": "assigned", "agents": [ids]}`. Ids must be agents of this
//!   workspace's set (the server's and its own), by id; names follow the
//!   node's own rules.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiError, ApiResult, require};
use crate::Master;
use crate::plugins::{self, MAX_AGENTS, MAX_ASSIGNED};
use crate::workspaces::{self as ws, Perm, access};

fn bad(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}

pub(super) async fn list(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let a = access(&m, &who, &id).await?;
    let agents = m.agents_in(Some(a.id())).await;
    let chosen = m.assignments(a.id()).await;
    let local_ok = ws::may_use_local_node(&m, &who);
    // (kind, name) -> the nodes that announce it, with each one's text.
    let mut items: BTreeMap<(&'static str, String), Vec<Value>> = BTreeMap::new();
    let mut unenforced = Vec::new();
    let mut left_out = Vec::new();
    let announcers = m.announcers();
    for n in announcers
        .iter()
        .filter(|n| local_ok || !m.is_local_node(&n.node_id))
    {
        let node = |description: Option<&str>| {
            json!({
                "node_id": n.node_id,
                "hostname": n.hostname,
                "connected": n.connected,
                "description": description,
            })
        };
        for s in &n.plugins.skills {
            items
                .entry((plugins::SKILL, s.name.clone()))
                .or_default()
                .push(node(Some(&s.description)));
        }
        for name in &n.plugins.mcp_servers {
            items
                .entry((plugins::MCP, name.clone()))
                .or_default()
                .push(node(None));
        }
        if n.connected && !n.enforces {
            unenforced.push(json!({"node_id": n.node_id, "hostname": n.hostname}));
        }
        if n.plugins.left_out > 0 {
            left_out.push(json!({
                "node_id": n.node_id,
                "hostname": n.hostname,
                "count": n.plugins.left_out,
            }));
        }
    }
    // Settings for names no node announces now stay listed: they still
    // hold for any node that has the name.
    for (kind, name, _) in chosen.iter() {
        let kind = if kind == plugins::SKILL {
            plugins::SKILL
        } else {
            plugins::MCP
        };
        items.entry((kind, name.to_string())).or_default();
    }
    let list: Vec<Value> = items
        .into_iter()
        .map(|((kind, name), nodes)| {
            let assigned = chosen.assigned(kind, &name).map(|ids| {
                // An id no agent of this set has any more counts for none.
                ids.iter()
                    .filter(|i| agents.get(i).is_some())
                    .cloned()
                    .collect::<Vec<_>>()
            });
            json!({
                "kind": kind,
                "name": name,
                "nodes": nodes,
                "access": if assigned.is_some() { "assigned" } else { "shared" },
                "agents": assigned.unwrap_or_default(),
            })
        })
        .collect();
    let agent_list: Vec<Value> = agents
        .list()
        .iter()
        .map(|g| {
            json!({
                "id": g.id,
                "name": g.name,
                "skills": g.features.skills,
                "connectors": g.features.mcp,
            })
        })
        .collect();
    Ok(Json(json!({
        "items": list,
        "agents": agent_list,
        "can_edit": a.can(Perm::Settings),
        "unenforced": unenforced,
        "left_out": left_out,
    })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PutBody {
    access: String,
    #[serde(default)]
    agents: Vec<String>,
}

pub(super) async fn put(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, kind, name)): Path<(String, String, String)>,
    Json(b): Json<PutBody>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let a = access(&m, &who, &id).await?;
    a.require(Perm::Settings)?;
    if kind != plugins::SKILL && kind != plugins::MCP {
        return Err(bad("the kind is skill or mcp"));
    }
    if !plugins::valid(&kind, &name) {
        return Err(bad(if kind == plugins::SKILL {
            "a skill's name is 1 to 64 letters, digits, '.', '_' or '-', starting with a letter \
             or digit"
        } else {
            "a connector's name is 1 to 32 of a-z, 0-9, '-' and '_', starting and ending with a \
             letter or digit, without '__'"
        }));
    }
    let ws = a.id().to_string();
    match b.access.as_str() {
        "shared" => {
            if !b.agents.is_empty() {
                return Err(bad("a Shared item names no agents"));
            }
            let (w, k, n) = (ws, kind, name);
            m.inner
                .mirror
                .call(move |s| s.plugin_access_delete(&w, &k, &n))
                .await?;
        }
        "assigned" => {
            if b.agents.len() > MAX_AGENTS {
                return Err(bad(format!(
                    "an item is assigned to at most {MAX_AGENTS} agents"
                )));
            }
            let set = m.agents_in(Some(&ws)).await;
            let mut ids: Vec<String> = Vec::new();
            for g in b.agents {
                // By id only: an alias or another workspace's agent is not
                // one of this set's ids.
                if set.get(&g).is_none() {
                    let shown: String = harness_core::text::clean_line(&g, 40);
                    return Err(bad(format!("no agent `{shown}` in this workspace")));
                }
                if !ids.contains(&g) {
                    ids.push(g);
                }
            }
            let (w, k, n) = (ws, kind, name);
            let found = m
                .inner
                .mirror
                .call(move |s| s.plugin_access_put(&w, &k, &n, &ids, MAX_ASSIGNED))
                .await?;
            if !found {
                return Err(ApiError(StatusCode::NOT_FOUND, "no such workspace".into()));
            }
        }
        _ => return Err(bad("access is shared or assigned")),
    }
    Ok(Json(json!({ "ok": true })))
}
