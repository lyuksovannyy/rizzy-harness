//! The Plugins screen's endpoints (ADOPTION_PLAN X.34 step A, PLAN.md
//! T80; `crate::plugins`):
//!
//! * `GET /api/workspaces/{id}/plugins`: for any member, the skills and
//!   connectors (MCP servers) the nodes announce, each with the nodes that
//!   announce it and this workspace's choice (Shared, or Assigned and to
//!   which of its agents); the workspace's agents for the picker; whether
//!   the caller may change it.
//!   A skill stored on the server (`crate::stored_skills`, step B) is an
//!   item too, with `stored`: its origin (`builtin`: the server's,
//!   `workspace`: this one's), description and whether the caller may
//!   change it; `hidden_skills` names, for editors only, this workspace's
//!   own skills a server skill's name hides.
//! * `PUT /api/workspaces/{id}/plugins/{kind}/{name}`: for whoever holds
//!   the workspace's `settings` permission, `{"access": "shared"}` or
//!   `{"access": "assigned", "agents": [ids]}`. Ids must be agents of this
//!   workspace's set (the server's and its own), by id; names follow the
//!   node's own rules.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};

use super::caller::User;
use super::{ApiError, ApiResult};
use crate::Master;
use crate::plugins::{self, MAX_AGENTS, MAX_ASSIGNED};
use crate::workspaces::{self as ws, Perm, access};

fn bad(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}

pub(super) async fn list(
    User(who): User,
    State(m): State<Master>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
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
    // The skills stored on the server (X.34 step B): the server's
    // (Built-in) and this workspace's, one row per name with a node's of
    // that name (which a node uses instead of the stored one there).
    let (stored, hidden) = match m.visible_skills(a.id()).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, workspace = %a.id(), "could not read the stored skills");
            (Vec::new(), Vec::new())
        }
    };
    let mut stored_by_name: BTreeMap<String, Value> = BTreeMap::new();
    for (scope, r) in &stored {
        items.entry((plugins::SKILL, r.name.clone())).or_default();
        stored_by_name.insert(
            r.name.clone(),
            json!({
                "origin": match scope {
                    harness_core::skills::StoredScope::Server => "builtin",
                    harness_core::skills::StoredScope::Workspace => "workspace",
                },
                "description": r.description,
                "updated_ms": r.updated_ms,
                "can_edit": match scope {
                    harness_core::skills::StoredScope::Server => who.is_admin(),
                    harness_core::skills::StoredScope::Workspace => a.can(Perm::Settings),
                },
            }),
        );
    }
    // A workspace skill a server skill's name hides: only its editors are
    // told, so they can delete or remake it.
    let hidden: Vec<Value> = if a.can(Perm::Settings) {
        hidden
            .iter()
            .map(|r| json!({"name": r.name, "description": r.description}))
            .collect()
    } else {
        Vec::new()
    };
    // The workspace's custom connectors (X.35): an item by name like a
    // node's server, so Shared / Assigned holds for them too (agents use
    // them from the next version on).
    let w = a.id().to_string();
    let custom: BTreeMap<String, Value> = match m
        .inner
        .mirror
        .call(move |s| s.connectors_of(&w))
        .await
    {
        Ok(rows) => rows
            .into_iter()
            .map(|r| {
                let host = reqwest::Url::parse(&r.url)
                    .ok()
                    .and_then(|u| u.host_str().map(str::to_string));
                (
                    r.name.clone(),
                    json!({"id": r.connector_id, "host": host, "auth": r.auth}),
                )
            })
            .collect(),
        Err(e) => {
            tracing::warn!(error = %e, workspace = %a.id(), "could not read the custom connectors");
            BTreeMap::new()
        }
    };
    for name in custom.keys() {
        items.entry((plugins::MCP, name.clone())).or_default();
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
            let stored = (kind == plugins::SKILL)
                .then(|| stored_by_name.get(&name).cloned())
                .flatten();
            let custom = (kind == plugins::MCP)
                .then(|| custom.get(&name).cloned())
                .flatten();
            json!({
                "kind": kind,
                "name": name,
                "stored": stored,
                "custom": custom,
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
        "can_edit_server": who.is_admin(),
        "hidden_skills": hidden,
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
    User(who): User,
    State(m): State<Master>,
    Path((id, kind, name)): Path<(String, String, String)>,
    Json(b): Json<PutBody>,
) -> ApiResult<Json<Value>> {
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
