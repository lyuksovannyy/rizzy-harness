//! The skills store's endpoints (ADOPTION_PLAN X.34 step B, PLAN.md T81;
//! `crate::stored_skills`). Every route is under a workspace the caller is
//! a member of (404 otherwise, as for every workspace read); `{scope}` is
//! `server` (the server's own skills: written by the server's admins, read
//! by any member, offered in every workspace) or `workspace` (this
//! workspace's: written by whoever holds its `settings` permission).
//!
//! * `POST /api/workspaces/{id}/skills`: `{"scope", "name", "description",
//!   "body"}`, or `{"scope", "skill_md"}` with a whole `SKILL.md` (read by
//!   the node's own reader; keys other than `name` and `description` are
//!   listed back as ignored).
//! * `GET /api/workspaces/{id}/skills/{scope}/{name}`: one skill with its
//!   body, as text.
//! * `PUT` the same path: `{"description", "body"}` or `{"skill_md"}`
//!   (its name must be the skill's). A skill's name is fixed once made.
//! * `DELETE` the same path.
//!
//! Every write passes `harness_tools::skills::check_stored`: refused with
//! the reason, never stored cut.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use harness_store::stored_skills::SkillScope;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiError, ApiResult, require};
use crate::Master;
use crate::auth::Principal;
use crate::stored_skills;
use crate::workspaces::{self as ws, Perm};

/// The largest SKILL.md taken: the body's cap and room for front matter.
/// The browser refuses a larger file before sending it, with the same
/// number (`web/src/components/Plugins.tsx`).
pub(super) const MAX_SKILL_MD: usize = harness_tools::skills::BODY_CAP + 16 * 1024;

fn bad(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}

/// The scope a path or a body names, `server` or `workspace`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    Server,
    Workspace,
}

fn scope_of(s: &str) -> ApiResult<Scope> {
    match s {
        "server" => Ok(Scope::Server),
        "workspace" => Ok(Scope::Workspace),
        _ => Err(bad("the scope is server or workspace")),
    }
}

/// Whether `who`, with access `a` to the workspace, may write `scope`.
fn may_write(who: &Principal, a: &ws::Access, scope: Scope) -> bool {
    match scope {
        Scope::Server => who.is_admin(),
        Scope::Workspace => a.can(Perm::Settings),
    }
}

fn require_write(who: &Principal, a: &ws::Access, scope: Scope) -> ApiResult<()> {
    if may_write(who, a, scope) {
        return Ok(());
    }
    Err(ApiError(
        StatusCode::FORBIDDEN,
        match scope {
            Scope::Server => "only the server's admins change the server's skills",
            Scope::Workspace => "only this workspace's admins change its skills",
        }
        .into(),
    ))
}

fn store_scope<'a>(scope: Scope, ws: &'a str) -> SkillScope<'a> {
    match scope {
        Scope::Server => SkillScope::Server,
        Scope::Workspace => SkillScope::Workspace(ws),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NewSkill {
    scope: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    skill_md: Option<String>,
}

/// A skill as checked: its name, description and body, and what the person
/// is told about what was not used.
struct Checked {
    name: String,
    description: String,
    body: String,
    ignored: Vec<String>,
    notes: Vec<String>,
}

/// Front-matter keys that would name a program, a tool or a model on a
/// node: none has any effect on a stored skill, and the answer says so.
const PROGRAM_KEYS: &[&str] = &[
    "allowed-tools",
    "hooks",
    "model",
    "context",
    "agent",
    "compatibility",
];

/// Check the fields of a write (`skill_md`, or the three fields); `name`
/// is the path's on an edit.
fn check(
    name: Option<String>,
    description: Option<String>,
    body: Option<String>,
    skill_md: Option<String>,
) -> ApiResult<Checked> {
    let (name, description, body, ignored, mut notes) = match skill_md {
        Some(md) => {
            if description.is_some() || body.is_some() {
                return Err(bad("send a SKILL.md or the description and body, not both"));
            }
            // The whole file must fit what a skill may hold, with room for
            // its front matter: a larger one is refused before it is read.
            if md.len() > MAX_SKILL_MD {
                return Err(bad(format!(
                    "the file is {} KiB; a SKILL.md is at most {} KiB (its instructions at most \
                     {} KiB)",
                    md.len().div_ceil(1024),
                    MAX_SKILL_MD / 1024,
                    harness_tools::skills::BODY_CAP / 1024
                )));
            }
            let read = harness_tools::skills::read_skill_md(&md).map_err(bad)?;
            let mut notes = Vec::new();
            if !read.yaml {
                notes.push(
                    "its front matter is not valid YAML: it was read line by line, as a node \
                     reads it"
                        .to_string(),
                );
            }
            let file_name = read.name.unwrap_or_default();
            let name = match name {
                Some(n) if !file_name.is_empty() && file_name != n => {
                    return Err(bad(format!(
                        "the file's name is `{}`, not `{n}`: a skill's name is fixed once made",
                        harness_core::text::clean_line(&file_name, 64)
                    )));
                }
                Some(n) => n,
                None if file_name.is_empty() => {
                    return Err(bad("the SKILL.md has no `name` in its front matter"));
                }
                None => file_name,
            };
            let description = read
                .description
                .unwrap_or_default()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            (name, description, read.body, read.ignored, notes)
        }
        None => (
            name.ok_or_else(|| bad("a skill needs a name"))?,
            description.unwrap_or_default().trim().to_string(),
            body.unwrap_or_default(),
            Vec::new(),
            Vec::new(),
        ),
    };
    let body = harness_tools::skills::check_stored(&name, &description, &body).map_err(bad)?;
    if !ignored.is_empty() {
        notes.push(format!(
            "not used: {} (a stored skill has only a name, a description and its text)",
            ignored.join(", ")
        ));
    }
    let programs: Vec<&str> = ignored
        .iter()
        .map(String::as_str)
        .filter(|k| PROGRAM_KEYS.contains(k))
        .collect();
    if !programs.is_empty() {
        notes.push(format!(
            "{} has no effect: a stored skill is text only and runs nothing; a skill that \
             brings programs is installed on a node by its owner",
            programs.join(", ")
        ));
    }
    Ok(Checked {
        name,
        description,
        body,
        ignored,
        notes,
    })
}

/// The check off the async workers: a YAML alias bomb in an upload costs
/// about a second of parsing (review of PR 148, L3).
async fn checked(f: impl FnOnce() -> ApiResult<Checked> + Send + 'static) -> ApiResult<Checked> {
    tokio::task::spawn_blocking(f).await.map_err(|e| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("the skill could not be checked: {e}"),
        )
    })?
}

pub(super) async fn create(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<NewSkill>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let a = ws::access(&m, &who, &id).await?;
    let scope = scope_of(&b.scope)?;
    require_write(&who, &a, scope)?;
    let c = checked(move || check(b.name, b.description, b.body, b.skill_md)).await?;
    let w = a.id().to_string();
    let (name, description, body) = (c.name.clone(), c.description.clone(), c.body.clone());
    let made = m
        .inner
        .mirror
        .call(move |s| {
            s.stored_skill_create(
                store_scope(scope, &w),
                &name,
                &description,
                &body,
                stored_skills::caps(),
            )
        })
        .await?;
    if !made {
        return Err(ApiError(StatusCode::NOT_FOUND, "no such workspace".into()));
    }
    tracing::info!(workspace = %a.id(), scope = ?scope, name = %c.name, by = %who.username, "stored skill made");
    Ok(Json(json!({
        "ok": true,
        "name": c.name,
        "ignored": c.ignored,
        "notes": c.notes,
    })))
}

pub(super) async fn get_one(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, scope, name)): Path<(String, String, String)>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let a = ws::access(&m, &who, &id).await?;
    let scope = scope_of(&scope)?;
    let w = a.id().to_string();
    let n = name.clone();
    let row = m
        .inner
        .mirror
        .call(move |s| s.stored_skill(store_scope(scope, &w), &n))
        .await?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "no such skill".into()))?;
    Ok(Json(json!({
        "scope": if scope == Scope::Server { "server" } else { "workspace" },
        "name": row.name,
        "description": row.description,
        "body": row.body,
        "updated_ms": row.updated_ms,
        "can_edit": may_write(&who, &a, scope),
    })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EditSkill {
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    skill_md: Option<String>,
}

pub(super) async fn put(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, scope, name)): Path<(String, String, String)>,
    Json(b): Json<EditSkill>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let a = ws::access(&m, &who, &id).await?;
    let scope = scope_of(&scope)?;
    require_write(&who, &a, scope)?;
    let c = checked(move || check(Some(name), b.description, b.body, b.skill_md)).await?;
    let w = a.id().to_string();
    let (n, description, body) = (c.name.clone(), c.description.clone(), c.body.clone());
    let found = m
        .inner
        .mirror
        .call(move |s| {
            s.stored_skill_update(
                store_scope(scope, &w),
                &n,
                &description,
                &body,
                stored_skills::caps(),
            )
        })
        .await?;
    if !found {
        return Err(ApiError(StatusCode::NOT_FOUND, "no such skill".into()));
    }
    tracing::info!(workspace = %a.id(), scope = ?scope, name = %c.name, by = %who.username, "stored skill changed");
    Ok(Json(json!({
        "ok": true,
        "name": c.name,
        "ignored": c.ignored,
        "notes": c.notes,
    })))
}

pub(super) async fn delete(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, scope, name)): Path<(String, String, String)>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let a = ws::access(&m, &who, &id).await?;
    let scope = scope_of(&scope)?;
    require_write(&who, &a, scope)?;
    let w = a.id().to_string();
    let n = name.clone();
    let gone = m
        .inner
        .mirror
        .call(move |s| s.stored_skill_delete(store_scope(scope, &w), &n))
        .await?;
    if !gone {
        return Err(ApiError(StatusCode::NOT_FOUND, "no such skill".into()));
    }
    tracing::info!(workspace = %a.id(), scope = ?scope, name = %name, by = %who.username, "stored skill deleted");
    Ok(Json(json!({ "ok": true })))
}
