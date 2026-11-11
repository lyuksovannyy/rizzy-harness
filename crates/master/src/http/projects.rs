//! Projects over HTTP (docs/projects.md, API): projects, which chats they
//! hold, their boards, saved board templates and project memory. Who may do
//! what is decided by the helpers in `crate::projects`.

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use harness_core::SessionId;
use harness_store::{
    CardPatch, NewCard, NewColumn, NewProject, ProjectPatch, ProjectRow, SessionRow, StoreError,
    TemplateRow,
};
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};

use super::{ApiError, ApiResult, CreateBody, memory_err, memory_store, require, visible_session};
use crate::auth::Principal;
use crate::projects::{self as p, ProjectSettings};
use crate::workspaces::{Access, WsError};
use crate::{Live, Master};

pub(super) fn routes() -> Router<Master> {
    Router::new()
        .route("/api/projects", get(list).post(create))
        .route(
            "/api/projects/{id}",
            get(detail).patch(update).delete(remove),
        )
        .route("/api/projects/{id}/board", get(board))
        .route("/api/projects/{id}/board/columns", post(add_column))
        .route(
            "/api/projects/{id}/board/columns/{col}",
            axum::routing::patch(update_column).delete(delete_column),
        )
        .route("/api/projects/{id}/board/cards", post(add_card))
        .route(
            "/api/projects/{id}/board/cards/{card}",
            axum::routing::patch(update_card).delete(delete_card),
        )
        .route("/api/projects/{id}/board/template", post(apply_template))
        .route(
            "/api/projects/{id}/memory",
            get(list_memory).post(add_memory),
        )
        .route(
            "/api/projects/{id}/memory/{fact}",
            axum::routing::patch(update_memory).delete(delete_memory),
        )
        .route("/api/board-templates", get(templates).post(save_template))
        .route("/api/board-templates/{id}", delete(delete_template))
        .route("/api/sessions/{id}/project", put(set_chat_project))
}

fn bad(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}

fn not_found(what: &str) -> ApiError {
    ApiError(StatusCode::NOT_FOUND, format!("unknown {what}"))
}

/// A refused store rule (limits, non-empty board) is the caller's conflict.
fn store_err(e: StoreError) -> ApiError {
    match e {
        StoreError::Refused(msg) => ApiError(StatusCode::CONFLICT, msg),
        other => other.into(),
    }
}

/// `Some(None)` for an explicit `null`, `None` when the field is missing.
fn some<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

fn changed(m: &Master) {
    let _ = m.inner.live.send(Live::ProjectsChanged);
}

/// `?workspace=` on list calls, as on the chat list.
#[derive(Deserialize)]
struct WorkspaceQuery {
    /// Users: defaults to the active workspace. The API token: every
    /// workspace without it.
    #[serde(default)]
    workspace: Option<String>,
}

/// The caller's standing in `workspace`, with a non-member's `NotFound`
/// said as "unknown <what>" (ids of other workspaces' things stay
/// unconfirmed).
async fn member_of(m: &Master, who: &Principal, workspace: &str, what: &str) -> ApiResult<Access> {
    match p::member(m, who, workspace).await {
        Ok(a) => Ok(a),
        Err(WsError::NotFound) => Err(not_found(what)),
        Err(e) => Err(e.into()),
    }
}

fn may_not_change(a: &Access, what: &str) -> ApiError {
    ApiError(
        StatusCode::FORBIDDEN,
        format!("your role in {} cannot change {what}", a.workspace.name),
    )
}

/// The project, if the caller may see it (a member of its workspace; 404
/// otherwise).
async fn visible(
    m: &Master,
    headers: &HeaderMap,
    id: &str,
) -> ApiResult<(Principal, ProjectRow, Access)> {
    let who = require(m, headers).await?;
    let pid = id.to_string();
    let row = m
        .inner
        .mirror
        .call(move |s| s.project(&pid))
        .await?
        .ok_or_else(|| not_found("project"))?;
    let a = member_of(m, &who, &row.workspace_id, "project").await?;
    Ok((who, row, a))
}

/// The project, if the caller may also change it.
async fn managed(m: &Master, headers: &HeaderMap, id: &str) -> ApiResult<(Principal, ProjectRow)> {
    let (who, row, a) = visible(m, headers, id).await?;
    if !p::may_manage(&a) {
        return Err(may_not_change(&a, "projects"));
    }
    Ok((who, row))
}

/// The caller's own top-level chats per project (each sees only their own).
async fn own_chats(m: &Master, who: &Principal) -> ApiResult<HashMap<String, usize>> {
    let rows = m.inner.mirror.call(|s| s.sessions()).await?;
    let links = p::links(m).await?;
    let mut n = HashMap::new();
    for r in rows
        .iter()
        .filter(|r| who.can_see(r.owner_id.as_deref()) && !r.incognito && r.parent_id.is_none())
    {
        if let Some(pid) = p::effective(&links, r) {
            *n.entry(pid.to_string()).or_default() += 1;
        }
    }
    Ok(n)
}

fn summary(r: &ProjectRow, chats: usize) -> Value {
    json!({
        "project_id": r.project_id,
        "workspace_id": r.workspace_id,
        "name": r.name,
        "description": r.description,
        "icon": r.icon,
        "accent": r.accent,
        "chats": chats,
        "updated_ms": r.updated_ms,
    })
}

async fn detail_json(m: &Master, who: &Principal, a: &Access, r: &ProjectRow) -> ApiResult<Value> {
    let chats = own_chats(m, who).await?.remove(&r.project_id).unwrap_or(0);
    let mut v = summary(r, chats);
    v["instructions"] = json!(r.instructions);
    v["settings"] = json!(ProjectSettings::read(&r.settings));
    v["can_manage"] = json!(p::may_manage(a));
    v["created_by"] = json!(r.created_by);
    v["created_ms"] = json!(r.created_ms);
    Ok(v)
}

async fn list(
    State(m): State<Master>,
    headers: HeaderMap,
    Query(q): Query<WorkspaceQuery>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let scope = p::current_workspace(&m, &who, q.workspace.as_deref()).await?;
    let ws = scope.as_ref().map(|a| a.id().to_string());
    let filter = ws.clone();
    // The API token without `?workspace` sees every workspace's projects.
    let rows = m
        .inner
        .mirror
        .call(move |s| s.projects(filter.as_deref()))
        .await?;
    let counts = own_chats(&m, &who).await?;
    let can_create = scope.as_ref().is_some_and(p::may_manage);
    Ok(Json(json!({
        "projects": rows
            .iter()
            .map(|r| summary(r, counts.get(&r.project_id).copied().unwrap_or(0)))
            .collect::<Vec<_>>(),
        "workspace_id": ws,
        "can_create": can_create,
        "icons": p::ICONS,
        "accents": crate::prefs::ACCENTS,
    })))
}

/// The columns a template gives: a built-in one, or one saved in `workspace`.
async fn template_columns(m: &Master, id: &str, workspace: &str) -> ApiResult<Vec<NewColumn>> {
    if let Some(t) = p::builtin_template(id) {
        return Ok(t.new_columns());
    }
    let tid = id.to_string();
    match m.inner.mirror.call(move |s| s.board_template(&tid)).await? {
        Some(t) if t.workspace_id == workspace => serde_json::from_str(&t.columns)
            .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
        _ => Err(not_found("template")),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateProject {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    accent: Option<String>,
    #[serde(default)]
    instructions: String,
    #[serde(default)]
    template_id: Option<String>,
    /// Users default to their current workspace; the API token must pass one.
    #[serde(default)]
    workspace_id: Option<String>,
}

async fn create(
    State(m): State<Master>,
    headers: HeaderMap,
    Json(b): Json<CreateProject>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let a = p::current_workspace(&m, &who, b.workspace_id.as_deref())
        .await?
        .ok_or_else(|| bad("pass workspace_id"))?;
    if !p::may_manage(&a) {
        return Err(may_not_change(&a, "projects"));
    }
    let ws = a.id().to_string();
    let icon = b.icon.unwrap_or_else(|| p::DEFAULT_ICON.into());
    let accent = b.accent.unwrap_or_else(|| p::DEFAULT_ACCENT.into());
    p::check_icon(&icon).map_err(bad)?;
    p::check_accent(&accent).map_err(bad)?;
    let columns = template_columns(
        &m,
        b.template_id.as_deref().unwrap_or(p::TEMPLATES[0].id),
        &ws,
    )
    .await?;
    let new = NewProject {
        project_id: harness_core::new_id(),
        workspace_id: ws,
        name: p::clean_required("name", &b.name, p::NAME_MAX).map_err(bad)?,
        description: p::clean("description", &b.description, p::DESCRIPTION_MAX, false)
            .map_err(bad)?,
        icon,
        accent,
        instructions: p::clean("instructions", &b.instructions, p::INSTRUCTIONS_MAX, true)
            .map_err(bad)?,
        settings: serde_json::to_string(&ProjectSettings::default())
            .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
        created_by: who.user_id.clone(),
        columns,
    };
    let row = m
        .inner
        .mirror
        .call(move |s| s.create_project(&new, p::MAX_PROJECTS))
        .await
        .map_err(store_err)?;
    changed(&m);
    Ok(Json(detail_json(&m, &who, &a, &row).await?))
}

async fn detail(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let (who, row, a) = visible(&m, &headers, &id).await?;
    Ok(Json(detail_json(&m, &who, &a, &row).await?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateProject {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    accent: Option<String>,
    #[serde(default)]
    instructions: Option<String>,
    /// The whole document, strictly read.
    #[serde(default)]
    settings: Option<ProjectSettings>,
}

async fn update(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<UpdateProject>,
) -> ApiResult<Json<Value>> {
    let (who, row, a) = visible(&m, &headers, &id).await?;
    if !p::may_manage(&a) {
        return Err(may_not_change(&a, "projects"));
    }
    if let Some(i) = &b.icon {
        p::check_icon(i).map_err(bad)?;
    }
    if let Some(a) = &b.accent {
        p::check_accent(a).map_err(bad)?;
    }
    let patch = ProjectPatch {
        name: b
            .name
            .map(|n| p::clean_required("name", &n, p::NAME_MAX))
            .transpose()
            .map_err(bad)?,
        description: b
            .description
            .map(|d| p::clean("description", &d, p::DESCRIPTION_MAX, false))
            .transpose()
            .map_err(bad)?,
        icon: b.icon,
        accent: b.accent,
        instructions: b
            .instructions
            .map(|i| p::clean("instructions", &i, p::INSTRUCTIONS_MAX, true))
            .transpose()
            .map_err(bad)?,
        settings: b
            .settings
            .map(|s| s.cleaned())
            .transpose()
            .map_err(bad)?
            .map(|s| serde_json::to_string(&s))
            .transpose()
            .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
    };
    let pid = row.project_id.clone();
    let row = m
        .inner
        .mirror
        .call(move |s| s.update_project(&pid, &patch))
        .await?
        .ok_or_else(|| not_found("project"))?;
    changed(&m);
    Ok(Json(detail_json(&m, &who, &a, &row).await?))
}

/// Delete a project: its chats return to no project, its board and facts go.
async fn remove(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let (_, row) = managed(&m, &headers, &id).await?;
    let pid = row.project_id.clone();
    m.inner.mirror.call(move |s| s.delete_project(&pid)).await?;
    p::delete_memory(&m, &row.project_id).await;
    changed(&m);
    let _ = m.inner.live.send(Live::SessionsChanged);
    let _ = m.inner.live.send(Live::MemoryChanged);
    Ok(Json(json!({ "ok": true })))
}

// ---- chats ----

/// Why `row` cannot be in a project at all, if it cannot.
fn unjoinable(row: &SessionRow) -> Option<&'static str> {
    if row.incognito {
        Some("temporary chats cannot join a project")
    } else if row.parent_id.is_some() {
        Some("a sub-task stays with the chat that started it")
    } else if p::chat_workspace(row).is_none() {
        Some("a chat without a workspace cannot join a project")
    } else {
        None
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChatProject {
    project_id: Option<String>,
}

/// Put one of your chats into a project, or take it out (`null`).
async fn set_chat_project(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<ChatProject>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let row = visible_session(&m, &who, &id).await?;
    let sid = row.session_id;
    let project = match b.project_id {
        None => None,
        Some(pid) => {
            if let Some(why) = unjoinable(&row) {
                return Err(bad(why));
            }
            let target = pid.clone();
            let pr = m
                .inner
                .mirror
                .call(move |s| s.project(&target))
                .await?
                .ok_or_else(|| not_found("project"))?;
            member_of(&m, &who, &pr.workspace_id, "project").await?;
            if p::chat_workspace(&row) != Some(pr.workspace_id.as_str()) {
                return Err(bad("this chat is in another workspace than the project"));
            }
            Some(pid)
        }
    };
    m.inner
        .mirror
        .call(move |s| s.set_session_project(sid, project.as_deref()))
        .await?;
    changed(&m);
    let _ = m.inner.live.send(Live::SessionsChanged);
    Ok(Json(json!({ "ok": true })))
}

/// `POST /api/sessions` with `project_id`: checked before the chat is made,
/// against `scope`, the workspace the chat goes to. An agentic chat without
/// a folder or node gets the project's defaults. The master links the chat
/// when it stamps it (`CreateSpec::project_id`).
pub(super) async fn for_new_chat(
    m: &Master,
    who: &Principal,
    b: &mut CreateBody,
    scope: Option<&Access>,
) -> ApiResult<Option<String>> {
    let Some(pid) = b.project_id.clone() else {
        return Ok(None);
    };
    if b.incognito {
        return Err(bad("temporary chats cannot join a project"));
    }
    let target = pid.clone();
    let project = m
        .inner
        .mirror
        .call(move |s| s.project(&target))
        .await?
        .ok_or_else(|| not_found("project"))?;
    member_of(m, who, &project.workspace_id, "project").await?;
    match scope {
        Some(a) if a.id() == project.workspace_id => {}
        Some(_) => return Err(bad("the project is in another workspace")),
        None => return Err(bad("a chat without a workspace cannot join a project")),
    }
    let agentic = b.profile.unwrap_or(harness_core::event::Profile::Agentic)
        == harness_core::event::Profile::Agentic;
    if agentic {
        let s = ProjectSettings::read(&project.settings);
        if b.cwd.trim().is_empty()
            && let Some(cwd) = s.cwd
        {
            b.cwd = cwd;
        }
        if b.node_id.is_none() {
            b.node_id = s.node_id;
        }
    }
    Ok(Some(pid))
}

// ---- board ----

/// The whole board as the caller may see it: a card's chat shows its title
/// only to whoever may see that chat; others see "private" (T10).
async fn board_json(m: &Master, who: &Principal, project: &str) -> ApiResult<Value> {
    let pid = project.to_string();
    let (cols, cards) = m.inner.mirror.call(move |s| s.board(&pid)).await?;
    let rows: HashMap<String, SessionRow> = m
        .inner
        .mirror
        .call(|s| s.sessions())
        .await?
        .into_iter()
        .map(|r| (r.session_id.to_string(), r))
        .collect();
    let columns: Vec<Value> = cols
        .iter()
        .map(|c| {
            let cards: Vec<Value> = cards
                .iter()
                .filter(|k| k.column_id == c.column_id)
                .map(|k| {
                    let chat = match k.session_id.as_deref().and_then(|s| rows.get(s)) {
                        Some(r) if who.can_see(r.owner_id.as_deref()) => {
                            json!({ "session_id": r.session_id, "title": r.title })
                        }
                        Some(_) => json!("private"),
                        None => Value::Null,
                    };
                    json!({
                        "card_id": k.card_id,
                        "column_id": k.column_id,
                        "title": k.title,
                        "notes": k.notes,
                        "position": k.position,
                        "chat": chat,
                        "created_by": k.created_by,
                        "updated_ms": k.updated_ms,
                    })
                })
                .collect();
            json!({ "column_id": c.column_id, "name": c.name, "done": c.done, "cards": cards })
        })
        .collect();
    Ok(json!({ "columns": columns }))
}

async fn board(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let (who, row, _) = visible(&m, &headers, &id).await?;
    Ok(Json(board_json(&m, &who, &row.project_id).await?))
}

/// Answer a board change: tell every client, return the new board.
async fn board_changed(m: &Master, who: &Principal, project: &str) -> ApiResult<Json<Value>> {
    changed(m);
    Ok(Json(board_json(m, who, project).await?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewColumnBody {
    name: String,
    #[serde(default)]
    done: bool,
}

async fn add_column(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<NewColumnBody>,
) -> ApiResult<Json<Value>> {
    let (who, row) = managed(&m, &headers, &id).await?;
    let col = NewColumn {
        name: p::clean_required("column name", &b.name, p::COLUMN_NAME_MAX).map_err(bad)?,
        done: b.done,
    };
    let pid = row.project_id.clone();
    m.inner
        .mirror
        .call(move |s| s.add_column(&pid, &col, p::MAX_COLUMNS))
        .await
        .map_err(store_err)?;
    board_changed(&m, &who, &row.project_id).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ColumnPatchBody {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    done: Option<bool>,
    #[serde(default)]
    position: Option<usize>,
}

async fn update_column(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, col)): Path<(String, String)>,
    Json(b): Json<ColumnPatchBody>,
) -> ApiResult<Json<Value>> {
    let (who, row) = managed(&m, &headers, &id).await?;
    let name = b
        .name
        .map(|n| p::clean_required("column name", &n, p::COLUMN_NAME_MAX))
        .transpose()
        .map_err(bad)?;
    let pid = row.project_id.clone();
    let found = m
        .inner
        .mirror
        .call(move |s| s.update_column(&pid, &col, name.as_deref(), b.done, b.position))
        .await?;
    if !found {
        return Err(not_found("column"));
    }
    board_changed(&m, &who, &row.project_id).await
}

async fn delete_column(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, col)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let (who, row) = managed(&m, &headers, &id).await?;
    let pid = row.project_id.clone();
    let found = m
        .inner
        .mirror
        .call(move |s| s.delete_column(&pid, &col))
        .await
        .map_err(store_err)?;
    if !found {
        return Err(not_found("column"));
    }
    board_changed(&m, &who, &row.project_id).await
}

/// A chat a card may link: one the caller may see, in this project.
async fn check_card_chat(m: &Master, who: &Principal, project: &str, sid: &str) -> ApiResult<()> {
    let row = visible_session(m, who, sid)
        .await
        .map_err(|_| bad("link one of your chats in this project"))?;
    let links = p::links(m).await?;
    if p::effective(&links, &row) != Some(project) {
        return Err(bad("link one of your chats in this project"));
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewCardBody {
    column_id: String,
    title: String,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    session_id: Option<String>,
}

async fn add_card(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<NewCardBody>,
) -> ApiResult<Json<Value>> {
    let (who, row) = managed(&m, &headers, &id).await?;
    if let Some(sid) = &b.session_id {
        check_card_chat(&m, &who, &row.project_id, sid).await?;
    }
    let card = NewCard {
        card_id: harness_core::new_id(),
        column_id: b.column_id,
        title: p::clean_required("title", &b.title, p::TITLE_MAX).map_err(bad)?,
        notes: p::clean("notes", &b.notes, p::NOTES_MAX, true).map_err(bad)?,
        session_id: b.session_id,
        created_by: who.user_id.clone(),
    };
    let pid = row.project_id.clone();
    let found = m
        .inner
        .mirror
        .call(move |s| s.add_card(&pid, &card, p::MAX_CARDS))
        .await
        .map_err(store_err)?;
    if !found {
        return Err(not_found("column"));
    }
    board_changed(&m, &who, &row.project_id).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CardPatchBody {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    column_id: Option<String>,
    #[serde(default)]
    position: Option<usize>,
    #[serde(default, deserialize_with = "some")]
    session_id: Option<Option<String>>,
}

async fn update_card(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, card)): Path<(String, String)>,
    Json(b): Json<CardPatchBody>,
) -> ApiResult<Json<Value>> {
    let (who, row) = managed(&m, &headers, &id).await?;
    if let Some(Some(sid)) = &b.session_id {
        check_card_chat(&m, &who, &row.project_id, sid).await?;
    }
    let patch = CardPatch {
        title: b
            .title
            .map(|t| p::clean_required("title", &t, p::TITLE_MAX))
            .transpose()
            .map_err(bad)?,
        notes: b
            .notes
            .map(|n| p::clean("notes", &n, p::NOTES_MAX, true))
            .transpose()
            .map_err(bad)?,
        column_id: b.column_id,
        position: b.position,
        session_id: b.session_id,
    };
    let pid = row.project_id.clone();
    let found = m
        .inner
        .mirror
        .call(move |s| s.update_card(&pid, &card, &patch))
        .await?;
    if !found {
        return Err(not_found("card or column"));
    }
    board_changed(&m, &who, &row.project_id).await
}

async fn delete_card(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, card)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let (who, row) = managed(&m, &headers, &id).await?;
    let pid = row.project_id.clone();
    if !m
        .inner
        .mirror
        .call(move |s| s.delete_card(&pid, &card))
        .await?
    {
        return Err(not_found("card"));
    }
    board_changed(&m, &who, &row.project_id).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TemplateChoice {
    template_id: String,
}

/// Replace an empty board's columns with a template's.
async fn apply_template(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<TemplateChoice>,
) -> ApiResult<Json<Value>> {
    let (who, row) = managed(&m, &headers, &id).await?;
    let cols = template_columns(&m, &b.template_id, &row.workspace_id).await?;
    let pid = row.project_id.clone();
    m.inner
        .mirror
        .call(move |s| s.reset_columns(&pid, &cols))
        .await
        .map_err(store_err)?;
    board_changed(&m, &who, &row.project_id).await
}

// ---- board templates ----

fn template_json(id: &str, name: &str, builtin: bool, columns: &[NewColumn]) -> Value {
    json!({ "template_id": id, "name": name, "builtin": builtin, "columns": columns })
}

/// Built-in templates first, then the ones saved in the workspace.
async fn templates(
    State(m): State<Master>,
    headers: HeaderMap,
    Query(q): Query<WorkspaceQuery>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let mut out: Vec<Value> = p::TEMPLATES
        .iter()
        .map(|t| template_json(t.id, t.name, true, &t.new_columns()))
        .collect();
    let scope = p::current_workspace(&m, &who, q.workspace.as_deref()).await?;
    if let Some(ws) = scope.map(|a| a.id().to_string()) {
        for t in m.inner.mirror.call(move |s| s.board_templates(&ws)).await? {
            let cols: Vec<NewColumn> = serde_json::from_str(&t.columns).unwrap_or_default();
            out.push(template_json(&t.template_id, &t.name, false, &cols));
        }
    }
    Ok(Json(json!({ "templates": out })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveTemplate {
    name: String,
    /// The project whose columns become the template.
    project_id: String,
}

async fn save_template(
    State(m): State<Master>,
    headers: HeaderMap,
    Json(b): Json<SaveTemplate>,
) -> ApiResult<Json<Value>> {
    let (_, row) = managed(&m, &headers, &b.project_id).await?;
    let name = p::clean_required("name", &b.name, p::NAME_MAX).map_err(bad)?;
    let pid = row.project_id.clone();
    let (cols, _) = m.inner.mirror.call(move |s| s.board(&pid)).await?;
    let cols: Vec<NewColumn> = cols
        .into_iter()
        .map(|c| NewColumn {
            name: c.name,
            done: c.done,
        })
        .collect();
    let t = TemplateRow {
        template_id: harness_core::new_id(),
        workspace_id: row.workspace_id.clone(),
        name,
        columns: serde_json::to_string(&cols)
            .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
        created_by: None,
        created_ms: harness_core::now_ms(),
    };
    let out = template_json(&t.template_id, &t.name, false, &cols);
    m.inner
        .mirror
        .call(move |s| s.create_board_template(&t, p::MAX_TEMPLATES))
        .await
        .map_err(store_err)?;
    changed(&m);
    Ok(Json(out))
}

async fn delete_template(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    if p::builtin_template(&id).is_some() {
        return Err(bad("built-in templates cannot be deleted"));
    }
    let tid = id.clone();
    let t = m
        .inner
        .mirror
        .call(move |s| s.board_template(&tid))
        .await?
        .ok_or_else(|| not_found("template"))?;
    let a = member_of(&m, &who, &t.workspace_id, "template").await?;
    if !p::may_manage(&a) {
        return Err(may_not_change(&a, "templates"));
    }
    m.inner
        .mirror
        .call(move |s| s.delete_board_template(&id))
        .await?;
    changed(&m);
    Ok(Json(json!({ "ok": true })))
}

// ---- project memory ----

/// Who wrote each chat, for proposals (shown only to their chat's owner).
async fn chat_owners(m: &Master) -> ApiResult<HashMap<SessionId, Option<String>>> {
    Ok(m.inner
        .mirror
        .call(|s| s.sessions())
        .await?
        .into_iter()
        .map(|r| (r.session_id, r.owner_id))
        .collect())
}

/// A proposal came from a private chat: only that chat's owner (or the API
/// token) sees it until they confirm it.
fn proposal_visible(
    who: &Principal,
    f: &harness_memory::Fact,
    owners: &HashMap<SessionId, Option<String>>,
) -> bool {
    match who.user_id.as_deref() {
        None => true,
        Some(me) => f
            .source_session
            .and_then(|s| owners.get(&s).cloned().flatten())
            .is_some_and(|o| o == me),
    }
}

/// A fact as the caller sees it: the source chat only when it is theirs.
fn fact_json(
    who: &Principal,
    f: &harness_memory::Fact,
    owners: &HashMap<SessionId, Option<String>>,
) -> Value {
    let mut v = json!(f);
    let own = f
        .source_session
        .and_then(|s| owners.get(&s))
        .is_some_and(|o| who.can_see(o.as_deref()));
    if !own {
        v["source_session"] = Value::Null;
    }
    v
}

async fn list_memory(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let (who, row, _) = visible(&m, &headers, &id).await?;
    let mem = memory_store(&m)?;
    let facts = mem
        .list(Some(&p::memory_owner(&row.project_id)))
        .await
        .map_err(memory_err)?;
    let owners = chat_owners(&m).await?;
    let shown: Vec<Value> = facts
        .iter()
        .filter(|f| {
            f.status == harness_memory::Status::Active || proposal_visible(&who, f, &owners)
        })
        .map(|f| fact_json(&who, f, &owners))
        .collect();
    Ok(Json(json!({ "memories": shown })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewFactBody {
    text: String,
}

async fn add_memory(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<NewFactBody>,
) -> ApiResult<Json<Value>> {
    let (_, row) = managed(&m, &headers, &id).await?;
    let mem = memory_store(&m)?;
    let f = mem
        .add(harness_memory::NewFact {
            owner_id: Some(p::memory_owner(&row.project_id)),
            text: b.text,
            kind: harness_memory::FactKind::Fact,
            status: harness_memory::Status::Active,
            source_session: None,
            dedupe_key: None,
        })
        .await
        .map_err(memory_err)?;
    let _ = m.inner.live.send(Live::MemoryChanged);
    Ok(Json(json!(f)))
}

/// A project fact the caller may change: a confirmed one needs "may
/// manage"; a proposal is its source chat's owner's to keep, word or
/// discard (any member: the project's memory grows from its chats), and
/// nobody else even sees it. Also whether the caller may confirm it (a
/// proposal becomes shared text for every member's agent: "may manage", or
/// the project's `members_activate_facts` switch).
async fn own_project_fact(
    m: &Master,
    headers: &HeaderMap,
    id: &str,
    fact: &str,
) -> ApiResult<(
    Principal,
    std::sync::Arc<dyn harness_memory::MemoryStore>,
    bool,
)> {
    let (who, row, a) = visible(m, headers, id).await?;
    let mem = memory_store(m)?;
    let f = mem.get(fact).await.map_err(memory_err)?;
    let owners = chat_owners(m).await?;
    let f = match f {
        Some(f)
            if f.owner_id.as_deref() == Some(p::memory_owner(&row.project_id).as_str())
                && (f.status == harness_memory::Status::Active
                    || proposal_visible(&who, &f, &owners)) =>
        {
            f
        }
        _ => return Err(ApiError(StatusCode::NOT_FOUND, "no such memory".into())),
    };
    if f.status == harness_memory::Status::Active && !p::may_manage(&a) {
        return Err(may_not_change(&a, "project memory"));
    }
    let may_activate =
        p::may_manage(&a) || ProjectSettings::read(&row.settings).members_activate_facts;
    Ok((who, mem, may_activate))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FactPatch {
    #[serde(default)]
    text: Option<String>,
    /// `active` confirms a proposal.
    #[serde(default)]
    status: Option<harness_memory::Status>,
}

async fn update_memory(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, fact)): Path<(String, String)>,
    Json(b): Json<FactPatch>,
) -> ApiResult<Json<Value>> {
    let (who, mem, may_activate) = own_project_fact(&m, &headers, &id, &fact).await?;
    if b.status == Some(harness_memory::Status::Active) && !may_activate {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "a project manager confirms facts into the project's memory".into(),
        ));
    }
    let f = mem
        .update(&fact, b.text, b.status)
        .await
        .map_err(memory_err)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "no such memory".into()))?;
    let _ = m.inner.live.send(Live::MemoryChanged);
    let owners = chat_owners(&m).await?;
    Ok(Json(fact_json(&who, &f, &owners)))
}

async fn delete_memory(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, fact)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let (_, mem, _) = own_project_fact(&m, &headers, &id, &fact).await?;
    mem.delete(&fact).await.map_err(memory_err)?;
    let _ = m.inner.live.send(Live::MemoryChanged);
    Ok(Json(json!({ "ok": true })))
}
