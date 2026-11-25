//! Renaming a chat. Owner only, as every per-chat route: anyone else,
//! admins included, gets the 404 a chat that does not exist gets (T10).
//!
//! The title the lists show is the master's row (`sessions.title`), filled
//! from `session_created` and read by every list, search hit and page; the
//! node and the model never read it. So a rename is a master-side update of
//! that row and its search entry, and appends no event: the log keeps the
//! title the chat was created with, and its one writer stays the node.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use harness_core::SessionId;
use harness_core::policy::ApprovalMode;
use serde::Deserialize;
use serde_json::{Value, json};

use super::caller::User;
use super::{ApiError, ApiResult, acting_session, projects, query_flag, visible_session};
use crate::workspaces as ws;
use crate::{Audience, Live, Master};

pub(super) fn routes() -> Router<Master> {
    Router::new()
        .route("/api/attention", get(attention))
        .route("/api/sessions", get(list_sessions).post(create_session))
        .route("/api/sessions/{id}/seen", post(mark_seen))
        .route(
            "/api/sessions/{id}",
            get(get_session).delete(delete_session),
        )
        .route("/api/sessions/{id}/events", get(events))
        .route("/api/sessions/{id}/title", put(rename))
}

#[derive(Deserialize)]
struct RenameBody {
    title: String,
}

/// Control and hidden characters are dropped, surrounding blanks trimmed;
/// empty or over `MAX_TITLE_CHARS` characters is refused.
async fn rename(
    User(who): User,
    State(m): State<Master>,
    Path(id): Path<String>,
    Json(b): Json<RenameBody>,
) -> ApiResult<Json<Value>> {
    let sid = acting_session(&m, &who, &id).await?.session_id;
    let title = crate::projects::clean_required("title", &b.title, harness_store::MAX_TITLE_CHARS)
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e))?;
    let t2 = title.clone();
    if !m
        .inner
        .mirror
        .call(move |s| s.rename_session(sid, &t2))
        .await?
    {
        return Err(ApiError(StatusCode::NOT_FOUND, "unknown session".into()));
    }
    m.nudge(Live::SessionsChanged(Audience::Chat(sid)));
    Ok(Json(json!({ "ok": true, "title": title })))
}

/// A chat as the UI lists it. `blocked`: why the caller may not make this
/// agentic chat do work (always `null` for chats).
fn session_json(r: &harness_store::SessionRow, blocked: Option<&str>) -> serde_json::Value {
    let blocked = blocked.filter(|_| r.profile.as_deref() != Some("chat"));
    json!({
        "session_id": r.session_id,
        "node_id": r.node_id,
        "title": r.title,
        "cwd": r.cwd,
        "model": r.model,
        "created_ms": r.created_ms,
        "updated_ms": r.updated_ms,
        "head_seq": r.head_seq,
        "running": r.running,
        "owner_id": r.owner_id,
        "agent_id": r.agent_id,
        "profile": r.profile,
        "incognito": r.incognito,
        "parent_id": r.parent_id,
        "workspace_id": r.workspace_id,
        "blocked": blocked,
    })
}

#[derive(Deserialize)]
struct SessionsQuery {
    #[serde(default)]
    workspace: Option<String>,
    /// Only this project's chats (one the caller may see, in the
    /// workspace): each project group of the sidebar pages on its own.
    #[serde(default)]
    project: Option<String>,
    /// Only chats in no project (the project page's "Add a chat" list);
    /// not with `project`.
    #[serde(default, deserialize_with = "query_flag")]
    loose: bool,
    /// Also the routine run chats the list leaves out once seen.
    #[serde(default, deserialize_with = "query_flag")]
    routine_runs: bool,
    /// Rows per page: default [`DEFAULT_PAGE`], clamped to
    /// `1..=MAX_PAGE`; not a number (or past `i64`) is 400.
    #[serde(default)]
    limit: Option<i64>,
    /// The `next` of the previous page; malformed is 400.
    #[serde(default)]
    after: Option<String>,
}

/// Rows of a page when the request does not say.
const DEFAULT_PAGE: i64 = 50;

/// One page of the caller's own top-level chats in one workspace, newest
/// first (the API token: every owner's, optionally in one workspace), with
/// every sub-agent chat below them in `children` and where the next page
/// starts in `next` (`docs/store-readers.md` §4). The owner comes from the
/// caller only; the cursor and the project only narrow what that owner
/// sees (T10).
async fn list_sessions(
    User(who): User,
    State(m): State<Master>,
    Query(q): Query<SessionsQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    use harness_store::session_pages::{Cursor, MAX_PAGE, Owner, PageQuery};
    let after = match q.after.as_deref() {
        None => None,
        Some(text) => Some(
            Cursor::parse(text)
                .ok_or_else(|| ApiError(StatusCode::BAD_REQUEST, "invalid cursor".into()))?,
        ),
    };
    let limit = q.limit.unwrap_or(DEFAULT_PAGE).clamp(1, MAX_PAGE as i64) as usize;
    if q.loose && q.project.is_some() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "loose lists chats in no project: name no project".into(),
        ));
    }
    let loose = q.loose;
    let scope = ws::scope(&m, &who, q.workspace.as_deref()).await?;
    let project = match q.project.as_deref() {
        Some(id) => Some(projects::listed_in(&m, &who, scope.as_ref().map(|a| a.id()), id).await?),
        None => None,
    };
    let local_ok = ws::may_use_local_node(&m, &who);
    let owner_id = who.user_id.clone();
    let viewer = owner_id.clone().unwrap_or_default();
    let workspace = scope.as_ref().map(|a| a.id().to_string());
    let hide_seen_runs = !q.routine_runs;
    let page = m
        .inner
        .mirror
        .call(move |s| {
            let owner = match owner_id.as_deref() {
                Some(u) => Owner::User(u),
                None => Owner::Everyone,
            };
            let q = PageQuery {
                owner,
                workspace: workspace.as_deref(),
                top_level: true,
                project: project.as_deref(),
                hide_seen_runs,
                after,
                limit,
            };
            let (rows, next) = if loose {
                s.loose_sessions_page(&q)?
            } else {
                s.sessions_page(&q)?
            };
            let ids: Vec<SessionId> = rows.iter().map(|r| r.session_id).collect();
            let children = s.children_of(owner, &ids)?;
            let all: Vec<SessionId> = ids
                .into_iter()
                .chain(children.rows.iter().map(|r| r.session_id))
                .collect();
            Ok::<_, harness_store::StoreError>(Page {
                unread: s.unread_of(owner, &viewer, &all)?,
                links: s
                    .project_links_of(owner, &all)?
                    .into_iter()
                    .map(|l| (l.session_id, l))
                    .collect(),
                marks: s.routine_marks_of(owner, &all)?,
                rows,
                children,
                next,
            })
        })
        .await?;
    let json_of = |r: &harness_store::SessionRow| {
        let on_local = (!local_ok && m.is_local_node(&r.node_id))
            .then(|| ws::LOCAL_NODE_ADMINS_ONLY.to_string());
        let blocked = ws::row_refusal(&who, scope.as_ref(), r);
        let mut v = session_json(r, blocked.or(on_local).as_deref());
        v["project_id"] = json!(crate::projects::effective(&page.links, r));
        let mark = page.marks.get(&r.session_id);
        v["routine"] = json!(mark);
        // A routine's run says it is done through the routine's own results.
        v["unread"] = json!(mark.is_none() && page.unread.contains(&r.session_id));
        v
    };
    // The store already answered for the caller's chats only; this is the
    // second line.
    let mut list: Vec<Value> = page
        .rows
        .iter()
        .filter(|r| who.can_see(r.owner_id.as_deref()))
        .map(json_of)
        .collect();
    let mut children: Vec<Value> = page
        .children
        .rows
        .iter()
        .filter(|r| who.can_see(r.owner_id.as_deref()))
        .map(json_of)
        .collect();
    m.annotate_background(&mut list);
    m.annotate_background(&mut children);
    Ok(Json(json!({
        "sessions": list,
        "children": children,
        "children_truncated": page.children.truncated,
        "next": page.next.map(|c| c.encode()),
    })))
}

/// What one store call gathers for a page.
struct Page {
    rows: Vec<harness_store::SessionRow>,
    children: harness_store::session_pages::Descendants,
    next: Option<harness_store::session_pages::Cursor>,
    unread: std::collections::HashSet<SessionId>,
    links: std::collections::HashMap<SessionId, harness_store::ProjectLink>,
    marks: std::collections::HashMap<SessionId, harness_store::routines::RoutineMark>,
}

/// Any of the caller's chats, whatever its workspace (the UI follows a
/// link to a chat into its workspace).
async fn get_session(
    User(who): User,
    State(m): State<Master>,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let row = visible_session(&m, &who, &id).await?;
    let blocked = ws::agentic_blocked(&m, &who, &row).await;
    let mut v = session_json(&row, blocked.as_deref());
    v["project_id"] = json!(crate::projects::effective_of(&m, &row).await?);
    crate::routines::annotate_one(&m, &mut v).await?;
    m.annotate_background(std::slice::from_mut(&mut v));
    // Whether custom connectors are off in this chat and why (`this_chat`,
    // `parent_chat`, `event_data`, `no_owner`, `unknown`), so a screen can
    // say it (PLAN.md T85).
    v["connectors_off"] = json!(m.connectors_off(&row).await);
    Ok(Json(v))
}

#[derive(Deserialize)]
struct DeleteSessionQuery {
    /// Also delete the caller's Media items that came from this chat.
    #[serde(default, deserialize_with = "query_flag")]
    media: bool,
}

async fn delete_session(
    User(who): User,
    State(m): State<Master>,
    Path(id): Path<String>,
    Query(q): Query<DeleteSessionQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let sid = acting_session(&m, &who, &id).await?.session_id;
    // Before the chat goes, while its items still name it.
    if q.media {
        crate::media::on_chat_deleted(&m, &who, sid).await?;
    }
    m.delete_session(sid).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct EventsQuery {
    #[serde(default)]
    after: u64,
    #[serde(default)]
    limit: Option<usize>,
}

async fn events(
    User(who): User,
    State(m): State<Master>,
    Path(id): Path<String>,
    Query(q): Query<EventsQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let sid = visible_session(&m, &who, &id).await?.session_id;
    let limit = q.limit.unwrap_or(1000).min(5000);
    let evs = m
        .inner
        .mirror
        .call(move |s| s.events_after(sid, q.after, limit))
        .await?;
    // No signature or redacted data reaches a browser (X.25, T73).
    let evs: Vec<serde_json::Value> = evs
        .iter()
        .map(harness_core::RawEvent::for_browser)
        .collect();
    Ok(Json(json!({ "events": evs })))
}

#[derive(Deserialize)]
pub(super) struct CreateBody {
    #[serde(default)]
    title: Option<String>,
    /// Required for agentic sessions; ignored for chat.
    #[serde(default)]
    pub(super) cwd: String,
    #[serde(default)]
    pub(super) profile: Option<harness_core::event::Profile>,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    pub(super) node_id: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    approval_mode: Option<ApprovalMode>,
    #[serde(default)]
    pub(super) incognito: bool,
    /// Users: defaults to the active workspace; members only. The API
    /// token: any existing workspace, or none.
    #[serde(default)]
    workspace_id: Option<String>,
    /// Start the chat in this project (docs/projects.md).
    #[serde(default)]
    pub(super) project_id: Option<String>,
    /// How its commands run (N.10a): a request, checked against the
    /// caller's level (`levels::create_mode`) and then by the node.
    #[serde(default)]
    exec_mode: Option<harness_core::policy::ExecMode>,
}

async fn create_session(
    User(who): User,
    State(m): State<Master>,
    Json(b): Json<CreateBody>,
) -> ApiResult<Json<serde_json::Value>> {
    // Every request for the host shell is audited, whatever refuses it.
    let host = b.exec_mode == Some(harness_core::policy::ExecMode::Host);
    let mut seen = ws::HostAudit::default();
    let out = create_checked(&m, &who, b, &mut seen).await;
    if host {
        let res = out.as_ref().map(|s| *s).map_err(|e| e.1.as_str());
        ws::audit_host_create(&m, &who, &seen, res).await;
    }
    Ok(Json(json!({ "session_id": out? })))
}

pub(super) async fn create_checked(
    m: &Master,
    who: &crate::auth::Principal,
    mut b: CreateBody,
    seen: &mut ws::HostAudit,
) -> ApiResult<SessionId> {
    let (m, who) = (m.clone(), who.clone());
    seen.node = b.node_id.clone();
    if b.incognito && b.profile != Some(harness_core::event::Profile::Chat) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "incognito is only available for chats".into(),
        ));
    }
    let scope = ws::scope(&m, &who, b.workspace_id.as_deref()).await?;
    seen.workspace = scope.as_ref().map(|a| a.id().to_string());
    // Agentic sessions run shell commands on a worker node: the mode asked
    // for (or the most confined one) must be within the caller's level
    // here (N.10a part 2). `Master::create_session` checks it again.
    if b.profile.unwrap_or(harness_core::event::Profile::Agentic)
        == harness_core::event::Profile::Agentic
    {
        let mode = crate::levels::create_mode(&who, scope.as_ref(), b.exec_mode);
        b.exec_mode = Some(mode.map_err(|why| ApiError(StatusCode::FORBIDDEN, why))?);
    }
    let project = projects::for_new_chat(&m, &who, &mut b, scope.as_ref()).await?;
    // And the node must be one they may use (the embedded node is for
    // admins by default): checked after the project may have chosen it;
    // without a choice, only nodes they may use are candidates.
    let agentic = b.profile.unwrap_or(harness_core::event::Profile::Agentic)
        == harness_core::event::Profile::Agentic;
    if agentic {
        let local_ok = ws::may_use_local_node(&m, &who);
        if b.node_id.is_none() && !local_ok {
            b.node_id = m.default_node_where(harness_core::event::Profile::Agentic, false);
            if b.node_id.is_none() {
                return Err(ApiError(
                    StatusCode::FORBIDDEN,
                    ws::LOCAL_NODE_ADMINS_ONLY.into(),
                ));
            }
        }
        if let Some(why) = b
            .node_id
            .as_deref()
            .and_then(|n| ws::node_refusal(&m, &who, n))
        {
            return Err(ApiError(StatusCode::FORBIDDEN, why));
        }
        // The node as `create_session` will pick it, for the audit line.
        b.node_id = b
            .node_id
            .or_else(|| m.default_node(harness_core::event::Profile::Agentic));
        seen.node = b.node_id.clone();
    } else if b.exec_mode.is_some() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "a chat runs no commands and has no execution mode".into(),
        ));
    }
    let title = b
        .title
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| "New session".into());
    // The master stamps owner, workspace and project once the node has the
    // chat.
    let sid = m
        .create_session(crate::CreateSpec {
            profile: b.profile,
            node_id: b.node_id,
            title,
            cwd: b.cwd,
            model: b.model,
            approval_mode: b.approval_mode,
            agent_id: b.agent_id,
            incognito: b.incognito,
            owner_id: who.user_id.clone(),
            workspace_id: scope.map(|a| a.workspace.workspace_id),
            project_id: project,
            no_callbacks: false,
            exec_mode: b.exec_mode,
        })
        .await?;
    Ok(sid)
}

/// Chats waiting for the caller (approvals, unanswered questions).
async fn attention(User(who): User, State(m): State<Master>) -> ApiResult<Json<serde_json::Value>> {
    let owner = who.user_id.clone();
    let viewer = owner.clone().unwrap_or_default();
    let (o, v) = (owner.clone(), viewer.clone());
    let items = m
        .inner
        .mirror
        .call(move |s| s.attention(o.as_deref(), &v))
        .await?;
    // Callback turns not seen yet, and why the agent was not resumed (T.8d
    // step 3): rows of the same shape, after the chats that wait.
    let mut items = serde_json::to_value(items).unwrap_or_default();
    if let Some(list) = items.as_array_mut() {
        list.extend(m.callback_attention(owner, viewer).await?);
    }
    let routines = crate::routines::attention(&m, &who).await?;
    Ok(Json(json!({ "items": items, "routines": routines })))
}

#[derive(Deserialize)]
struct SeenBody {
    seq: u64,
}

async fn mark_seen(
    User(who): User,
    State(m): State<Master>,
    Path(id): Path<String>,
    Json(b): Json<SeenBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let sid = visible_session(&m, &who, &id).await?.session_id;
    let viewer = who.user_id.clone().unwrap_or_default();
    m.inner
        .mirror
        .call(move |s| s.mark_seen(&viewer, sid, b.seq))
        .await?;
    m.nudge(crate::Live::SessionsChanged(crate::Audience::Chat(sid)));
    Ok(Json(json!({ "ok": true })))
}
