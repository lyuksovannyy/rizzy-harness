//! Memory over HTTP (README, Memory): a person's own facts and their
//! Personal workspace's memory (Settings → Account → Memory), a workspace's
//! shared memory (Settings → Workspace → Memory: every member reads it; its
//! admins, or every member behind one switch, change it; proposals wait
//! there for whoever may decide them), and what one `remember` call of a
//! chat did, with Keep, Dismiss and Undo. A person's own facts are theirs
//! alone; a workspace's memory reaches its members only (404 for anyone
//! else); a chat's calls answer only its owner (PLAN.md T49, T54).

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use harness_memory::{Fact, MemoryStore, NewFact, Status};
use harness_store::SessionRow;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiError, ApiResult, require, visible_session};
use crate::auth::Principal;
use crate::memory::{
    Place, audience_of, fact_place, is_own, memory_changed, place_audience, workspace_owner,
};
use crate::workspaces::{Perm, access};
use crate::{Audience, Master};

type Store = std::sync::Arc<dyn MemoryStore>;

pub(super) fn routes() -> Router<Master> {
    Router::new()
        .route("/api/memory", get(list).post(add))
        .route(
            "/api/memory/{id}",
            axum::routing::patch(update).delete(remove),
        )
        .route("/api/workspaces/{id}/facts", get(ws_list).post(ws_add))
        .route("/api/workspaces/{id}/facts/settings", put(ws_settings))
        .route(
            "/api/workspaces/{id}/facts/{fact}",
            axum::routing::patch(ws_update).delete(ws_remove),
        )
        .route("/api/sessions/{id}/memory/{call_id}", get(call_state))
        .route("/api/sessions/{id}/memory/{call_id}/undo", post(undo))
        .route("/api/sessions/{id}/memory/{call_id}/keep", post(keep))
        .route("/api/sessions/{id}/memory/{call_id}/dismiss", post(dismiss))
}

pub(super) fn memory_store(m: &Master) -> ApiResult<Store> {
    m.memory().ok_or_else(|| {
        ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "memory is not set up on this server".into(),
        )
    })
}

pub(super) fn memory_err(e: harness_memory::MemoryError) -> ApiError {
    match e {
        harness_memory::MemoryError::Invalid(msg) => ApiError(StatusCode::BAD_REQUEST, msg),
        other => ApiError(StatusCode::INTERNAL_SERVER_ERROR, other.to_string()),
    }
}

fn not_found() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "no such memory".into())
}

fn forbidden(msg: &str) -> ApiError {
    ApiError(StatusCode::FORBIDDEN, msg.into())
}

/// The caller's Personal workspace (`None` for the API token).
async fn home(m: &Master, who: &Principal) -> Option<String> {
    crate::memory::personal_of(m, who.user_id.as_deref()?).await
}

/// A fact as the Account page shows it: where it lives too.
fn fact_json(f: &Fact) -> Value {
    let mut v = json!(f);
    match fact_place(f) {
        Some(Place::Workspace(ws)) => {
            v["place"] = json!("workspace");
            v["workspace_id"] = json!(ws);
            v["workspace_personal"] = json!(true);
        }
        _ => v["place"] = json!("user"),
    }
    v
}

/// The caller's own facts and their Personal workspace's memory (where
/// their private details stay); newest first. A team's memory is on that
/// workspace's own Memory tab.
async fn list(State(m): State<Master>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let mem = memory_store(&m)?;
    let mut facts = mem.list(who.user_id.as_deref()).await.map_err(memory_err)?;
    if let Some(ws) = home(&m, &who).await {
        facts.extend(
            mem.list(Some(&workspace_owner(&ws)))
                .await
                .map_err(memory_err)?,
        );
    }
    facts.sort_by_key(|f| std::cmp::Reverse(f.updated_ms));
    let metas = crate::memory::proposal_metas(&m).await;
    let out: Vec<Value> = facts
        .iter()
        .map(|f| {
            let mut v = fact_json(f);
            if let Some(meta) = metas.get(&f.id) {
                v["proposal"] = json!(meta);
            }
            v
        })
        .collect();
    Ok(Json(json!({ "memories": out, "backend": mem.name() })))
}

/// The owner key for a fact of the caller: their own facts, or (`workspace`
/// set) their Personal workspace's memory; any other workspace is 404 here.
async fn owner_key(m: &Master, who: &Principal, workspace: Option<&str>) -> ApiResult<String> {
    let Some(uid) = who.user_id.clone() else {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "the API token has no workspaces".into(),
        ));
    };
    match workspace {
        None => Ok(uid),
        Some(ws) if home(m, who).await.as_deref() == Some(ws) => Ok(workspace_owner(ws)),
        Some(_) => Err(ApiError(StatusCode::NOT_FOUND, "no such workspace".into())),
    }
}

#[derive(Deserialize)]
struct NewMemoryBody {
    text: String,
    #[serde(default)]
    kind: harness_memory::FactKind,
    /// Keep it in the caller's Personal workspace's memory (its id).
    #[serde(default)]
    workspace_id: Option<String>,
}

/// Added by the person: active at once.
async fn add(
    State(m): State<Master>,
    headers: HeaderMap,
    Json(b): Json<NewMemoryBody>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let mem = memory_store(&m)?;
    let owner = match (&who.user_id, b.workspace_id.as_deref()) {
        (None, None) => None,
        (_, ws) => Some(owner_key(&m, &who, ws).await?),
    };
    let to = place_audience(owner.as_deref());
    let f = mem
        .add(NewFact {
            owner_id: owner,
            text: b.text,
            kind: b.kind,
            status: Status::Active,
            source_session: None,
            dedupe_key: None,
        })
        .await
        .map_err(memory_err)?;
    memory_changed(&m, to);
    Ok(Json(f.map(|f| fact_json(&f)).unwrap_or(Value::Null)))
}

/// The fact, if the Account page shows it to the caller: their own, or
/// their Personal workspace's (404 otherwise, never 403).
async fn own_fact(m: &Master, who: &Principal, id: &str) -> ApiResult<(Store, Fact)> {
    let mem = memory_store(m)?;
    let personal = home(m, who).await;
    match mem.get(id).await.map_err(memory_err)? {
        Some(f)
            if is_own(who.user_id.as_deref(), &f)
                || (who.user_id.is_none() && f.owner_id.is_none())
                || (personal.is_some() && crate::memory::kept_in(&f) == personal.as_deref()) =>
        {
            Ok((mem, f))
        }
        _ => Err(not_found()),
    }
}

#[derive(Deserialize)]
struct MemoryPatch {
    #[serde(default)]
    text: Option<String>,
    /// `active` keeps a proposal or restores a retired fact.
    #[serde(default)]
    status: Option<Status>,
    /// `user` (their own facts) or `workspace` (their Personal workspace,
    /// `workspace_id`): moves it.
    #[serde(default)]
    place: Option<String>,
    #[serde(default)]
    workspace_id: Option<String>,
}

/// Keep `cur` if the patch keeps a proposal (what it replaces or drops
/// applies too); `true` when it did.
async fn kept(
    m: &Master,
    who: &Principal,
    mem: &dyn MemoryStore,
    cur: &Fact,
    status: Option<Status>,
) -> ApiResult<bool> {
    if status == Some(Status::Active) && cur.status == Status::Proposed {
        crate::memory::keep_proposal(m, mem, cur, who.user_id.as_deref())
            .await
            .map_err(memory_err)?;
        return Ok(true);
    }
    Ok(false)
}

async fn update(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<MemoryPatch>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let (mem, cur) = own_fact(&m, &who, &id).await?;
    let before = audience_of(&m, &cur).await;
    if b.text.is_some() {
        mem.update(&id, b.text.clone(), None)
            .await
            .map_err(memory_err)?;
    }
    let status = if kept(&m, &who, &*mem, &cur, b.status).await? {
        None
    } else {
        b.status
    };
    let Some(mut f) = mem.update(&id, None, status).await.map_err(memory_err)? else {
        // A kept drop proposal is gone: it only retired its fact.
        memory_changed(&m, before);
        return Ok(Json(json!({ "ok": true })));
    };
    let to = match b.place.as_deref() {
        None => None,
        Some("user") => Some(owner_key(&m, &who, None).await?),
        Some("workspace") => {
            let ws = b.workspace_id.as_deref().ok_or_else(|| {
                ApiError(
                    StatusCode::BAD_REQUEST,
                    "place workspace needs workspace_id".into(),
                )
            })?;
            Some(owner_key(&m, &who, Some(ws)).await?)
        }
        Some(_) => {
            return Err(ApiError(
                StatusCode::BAD_REQUEST,
                "place must be user or workspace".into(),
            ));
        }
    };
    // A move is a copy in the new place and the old one deleted: the
    // back ends key facts by owner and none moves one.
    if let Some(owner) = to.filter(|o| cur.owner_id.as_deref() != Some(o.as_str())) {
        let moved = mem
            .add(NewFact {
                owner_id: Some(owner),
                text: f.text.clone(),
                kind: f.kind,
                status: f.status,
                source_session: f.source_session,
                dedupe_key: None,
            })
            .await
            .map_err(memory_err)?
            .ok_or_else(not_found)?;
        mem.delete(&id).await.map_err(memory_err)?;
        crate::memory::clear_meta(&m, &id).await;
        f = moved;
    }
    told(&m, before, audience_of(&m, &f).await);
    Ok(Json(fact_json(&f)))
}

async fn remove(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let (mem, cur) = own_fact(&m, &who, &id).await?;
    let to = audience_of(&m, &cur).await;
    mem.delete(&id).await.map_err(memory_err)?;
    crate::memory::clear_meta(&m, &id).await;
    memory_changed(&m, to);
    Ok(Json(json!({ "ok": true })))
}

// ---- a workspace's shared memory ----

/// The caller's standing for workspace `id`'s memory: a member (404
/// otherwise), whether they may change it ([`crate::memory::may_edit`]),
/// whether they may set its switch and purge retired facts (the `settings`
/// permission). The API token acts as an owner of a team workspace, but a
/// Personal workspace's memory is its person's alone: 404 (review F12).
async fn ws_standing(m: &Master, who: &Principal, id: &str) -> ApiResult<(String, bool, bool)> {
    let a = access(m, who, id).await?;
    if who.user_id.is_none() && a.personal {
        return Err(not_found());
    }
    let ws = a.id().to_string();
    let may = match &who.user_id {
        Some(u) => crate::memory::may_edit(m, u, &ws).await,
        None => true,
    };
    Ok((ws, may, a.can(Perm::Settings)))
}

/// Who started each chat, for a proposal's "from your chat".
async fn chat_owners(
    m: &Master,
) -> ApiResult<std::collections::HashMap<harness_core::SessionId, Option<String>>> {
    Ok(m.inner
        .mirror
        .call(|s| s.sessions())
        .await?
        .into_iter()
        .map(|r| (r.session_id, r.owner_id))
        .collect())
}

/// Every account's name, for "added by"; one whose account is gone reads
/// "a former member".
pub(super) async fn names(m: &Master) -> ApiResult<std::collections::HashMap<String, String>> {
    Ok(m.inner
        .mirror
        .call(|s| s.list_users())
        .await?
        .into_iter()
        .map(|u| (u.user_id, u.username))
        .collect())
}

/// Who added, kept and retired `f`, and who proposed it, as a tab shows it.
pub(super) fn credit(
    v: &mut Value,
    f: &Fact,
    authors: &std::collections::HashMap<String, crate::memory::Authorship>,
    metas: &std::collections::HashMap<String, crate::memory::ProposalMeta>,
    names: &std::collections::HashMap<String, String>,
) {
    let name = |id: &Option<String>| match id {
        Some(id) => json!(
            names
                .get(id)
                .cloned()
                .unwrap_or_else(|| "a former member".into())
        ),
        None => Value::Null,
    };
    if let Some(a) = authors.get(&f.id) {
        v["added_by"] = name(&a.by);
        v["added_ms"] = json!(if a.at > 0 { a.at } else { f.created_ms });
        v["kept_by"] = name(&a.kept_by);
        v["kept_ms"] = json!(a.kept_at);
        v["retired_by"] = name(&a.retired_by);
        v["retired_ms"] = json!(a.retired_at);
    } else {
        v["added_ms"] = json!(f.created_ms);
    }
    if let Some(meta) = metas.get(&f.id) {
        v["proposal"] = json!({
            "replaces": meta.replaces,
            "drop": meta.drop,
            "submitted": meta.submitted,
        });
        v["proposed_by"] = name(&meta.by);
    }
}

/// Every member reads the workspace's memory (active and retired facts,
/// with who added, kept and retired each). A waiting change is shown to the
/// person whose chat made it, and to those who may decide it only once that
/// person sent it on (review F2: their chat is private). The source chat is
/// named only to its owner.
async fn ws_list(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let (ws, may, admin) = ws_standing(&m, &who, &id).await?;
    let mem = memory_store(&m)?;
    let facts = mem
        .list(Some(&workspace_owner(&ws)))
        .await
        .map_err(memory_err)?;
    let owners = chat_owners(&m).await?;
    let metas = crate::memory::proposal_metas(&m).await;
    let authors = crate::memory::authorships(&m).await;
    let names = names(&m).await?;
    let mine = |f: &Fact| {
        f.source_session
            .and_then(|s| owners.get(&s).cloned().flatten())
            .is_some_and(|o| who.user_id.as_deref() == Some(o.as_str()))
    };
    let mut waiting = 0;
    let mut out = Vec::new();
    for f in &facts {
        let own = mine(f);
        if f.status == Status::Proposed {
            let sent = metas.get(&f.id).is_some_and(|x| x.submitted);
            if !(own || (may && sent)) {
                continue;
            }
            if may && sent {
                waiting += 1;
            }
        }
        let mut v = json!(f);
        if !own {
            v["source_session"] = Value::Null;
        }
        v["mine"] = json!(own);
        credit(&mut v, f, &authors, &metas, &names);
        out.push(v);
    }
    let members_edit = crate::memory::members_edit(&m, &ws).await;
    Ok(Json(json!({
        "memories": out,
        "can_edit": may,
        "can_set": admin,
        "members_edit": members_edit,
        "waiting": waiting,
    })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewWsFact {
    text: String,
}

/// Added by hand: those who may change the memory; active at once, with
/// its author recorded.
async fn ws_add(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<NewWsFact>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let (ws, may, _) = ws_standing(&m, &who, &id).await?;
    if !may {
        return Err(forbidden(
            "only the workspace's admins change its memory here",
        ));
    }
    let mem = memory_store(&m)?;
    let f = mem
        .add(NewFact {
            owner_id: Some(workspace_owner(&ws)),
            text: b.text,
            kind: harness_memory::FactKind::Fact,
            status: Status::Active,
            source_session: None,
            dedupe_key: None,
        })
        .await
        .map_err(memory_err)?;
    if let Some(f) = &f {
        crate::memory::note_added(&m, f, who.user_id.as_deref()).await;
    }
    memory_changed(&m, Audience::Workspace(ws));
    Ok(Json(json!(f)))
}

/// A fact of workspace `ws`'s memory and what the caller may do with it:
/// `Edit` (those who may change the memory: any fact, and a proposal once
/// it was sent on), `Own` (the person whose chat made a proposal: reword or
/// withdraw it while it waits).
#[derive(PartialEq, Eq)]
enum WsRight {
    Edit,
    Own,
}

async fn ws_fact(
    m: &Master,
    who: &Principal,
    id: &str,
    fact: &str,
) -> ApiResult<(Store, Fact, WsRight, bool, String)> {
    let (ws, may, admin) = ws_standing(m, who, id).await?;
    let mem = memory_store(m)?;
    let f = mem
        .get(fact)
        .await
        .map_err(memory_err)?
        .filter(|f| f.owner_id.as_deref() == Some(workspace_owner(&ws).as_str()))
        .ok_or_else(not_found)?;
    let owners = chat_owners(m).await?;
    let own = f.status == Status::Proposed
        && f.source_session
            .and_then(|s| owners.get(&s).cloned().flatten())
            .is_some_and(|o| who.user_id.as_deref() == Some(o.as_str()));
    let sent = crate::memory::proposal_meta(m, &f.id).await.submitted;
    if own {
        return Ok((mem, f, WsRight::Own, admin, ws));
    }
    // Another person's proposal that they have not sent on is not shown,
    // even to an admin: 404 as for any fact one may not see.
    if f.status == Status::Proposed && !sent {
        return Err(not_found());
    }
    if may {
        Ok((mem, f, WsRight::Edit, admin, ws))
    } else {
        Err(forbidden(
            "only the workspace's admins change its memory here",
        ))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WsPatch {
    #[serde(default)]
    text: Option<String>,
    /// `active` keeps a proposal or restores a retired fact; `retired`
    /// retires one.
    #[serde(default)]
    status: Option<Status>,
}

async fn ws_update(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, fact)): Path<(String, String)>,
    Json(b): Json<WsPatch>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let (mem, cur, right, _, _) = ws_fact(&m, &who, &id, &fact).await?;
    let before = audience_of(&m, &cur).await;
    if right == WsRight::Own && b.status.is_some() {
        return Err(forbidden(
            "a workspace admin keeps proposals into its memory",
        ));
    }
    if b.text.is_some() {
        mem.update(&fact, b.text, None).await.map_err(memory_err)?;
    }
    if b.status == Some(Status::Active) && cur.status == Status::Proposed {
        crate::memory::keep_proposal(&m, &*mem, &cur, who.user_id.as_deref())
            .await
            .map_err(memory_err)?;
    } else if let Some(st) = b.status {
        mem.update(&fact, None, Some(st))
            .await
            .map_err(memory_err)?;
        if st == Status::Retired || cur.status == Status::Retired {
            crate::memory::note_retired(&m, &cur, who.user_id.as_deref(), st == Status::Retired)
                .await;
        }
    }
    let f = mem.get(&fact).await.map_err(memory_err)?;
    let after = match &f {
        Some(f) => audience_of(&m, f).await,
        None => before.clone(),
    };
    told(&m, before, after);
    Ok(Json(f.map(|f| json!(f)).unwrap_or(json!({ "ok": true }))))
}

/// Removing from a workspace's memory (review F5): a waiting change may be
/// withdrawn by its proposer or dismissed by one who may decide it; a fact
/// is hard-deleted only once retired, and only by the workspace's admins
/// and owners. Everyone else retires (PATCH `retired`), which keeps it
/// listed with who retired it.
async fn ws_remove(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, fact)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let (mem, cur, _, admin, _) = ws_fact(&m, &who, &id, &fact).await?;
    let to = audience_of(&m, &cur).await;
    match cur.status {
        Status::Proposed => {}
        Status::Retired if admin => {}
        Status::Retired => {
            return Err(forbidden(
                "only the workspace's admins delete retired facts",
            ));
        }
        Status::Active => {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "retire a fact first; only a retired fact can be deleted".into(),
            ));
        }
    }
    mem.delete(&fact).await.map_err(memory_err)?;
    crate::memory::clear_meta(&m, &fact).await;
    crate::memory::clear_authorship(&m, &fact).await;
    memory_changed(&m, to);
    Ok(Json(json!({ "ok": true })))
}

/// Tell who saw a fact before a change and who sees it after (T58).
fn told(m: &Master, before: Audience, after: Audience) {
    if after != before {
        memory_changed(m, after);
    }
    memory_changed(m, before);
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WsMemorySettings {
    members_edit: bool,
}

/// The one switch (admins and owners): every member may change the
/// workspace's memory, and their agents' saves there apply at once.
async fn ws_settings(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<WsMemorySettings>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let (ws, _, admin) = ws_standing(&m, &who, &id).await?;
    if !admin {
        return Err(forbidden("only the workspace's admins and owners set this"));
    }
    crate::memory::set_members_edit(&m, &ws, b.members_edit).await?;
    memory_changed(&m, Audience::Workspace(ws));
    Ok(Json(json!({ "members_edit": b.members_edit })))
}

// ---- a chat's `remember` calls ----

/// The caller's own chat `sid` and its `remember` call `call_id` (404 for
/// anyone else's chat, admins and the API token included, and for any
/// other tool's call). Returns the chat and the call's input.
async fn own_call(
    m: &Master,
    who: &Principal,
    sid: &str,
    call_id: &str,
) -> ApiResult<(SessionRow, Value)> {
    let row = visible_session(m, who, sid).await?;
    let none = || ApiError(StatusCode::NOT_FOUND, "no such call".into());
    if who.user_id.is_none() || row.owner_id != who.user_id {
        return Err(none());
    }
    let (s, c) = (row.session_id, call_id.to_string());
    let (tool, input) = m
        .inner
        .mirror
        .call(move |st| st.tool_call(s, &c))
        .await?
        .ok_or_else(none)?;
    if tool != "remember" {
        return Err(none());
    }
    Ok((row, input))
}

/// What the chat's memory block shows for a call: its recorded state and
/// the facts it names as they are now. `waiting` when nothing is recorded
/// yet (the master files a call when the node reports it; the block asks
/// again).
async fn state_json(
    m: &Master,
    who: &Principal,
    row: &SessionRow,
    call_id: &str,
    input: &Value,
) -> ApiResult<Value> {
    let Some(r) = crate::memory::call_record(m, row.session_id, call_id).await else {
        return Ok(json!({ "state": "waiting" }));
    };
    let mem = memory_store(m)?;
    let uid = who.user_id.clone().unwrap_or_default();
    let personal = home(m, who).await;
    // Facts the block may show: the owner's own, any of a workspace they
    // are a member of (their Personal one included), the chat's project's.
    let project = crate::projects::project_of(m, row)
        .await
        .map(|p| crate::projects::memory_owner(&p.project_id));
    let mut shown: Vec<Option<Fact>> = Vec::new();
    for id in [&r.added, &r.target] {
        let f = match id {
            Some(id) => mem.get(id).await.map_err(memory_err)?,
            None => None,
        };
        let ok = match f.as_ref().and_then(fact_place) {
            Some(Place::User(u)) => u == uid,
            Some(Place::Workspace(ws)) => crate::memory::is_member(m, &uid, ws).await,
            Some(Place::Project(_)) => {
                project.is_some() && f.as_ref().is_some_and(|f| f.owner_id == project)
            }
            None => false,
        };
        shown.push(f.filter(|_| ok));
    }
    let (fact, target) = (shown[0].take(), shown[1].take());
    let forget = input.get("forget").is_some() && input.get("text").is_none();
    use crate::memory::CallState as S;
    // Sent on by the person: it waits for those who may decide it.
    let sent = match &fact {
        Some(f) => crate::memory::proposal_meta(m, &f.id).await.submitted,
        None => false,
    };
    let state = match r.state {
        S::Saved | S::Kept if r.added.is_some() && fact.is_none() => "gone",
        S::Saved if forget => "forgot",
        S::Kept if forget => "forgot",
        S::Saved => "saved",
        S::Kept => "kept",
        // Kept or discarded on a Memory tab meanwhile.
        S::Pending => match fact.as_ref().map(|f| f.status) {
            Some(Status::Proposed) if sent => "sent",
            Some(Status::Proposed) => "pending",
            Some(_) => "kept",
            None if forget && target.as_ref().is_some_and(|t| t.status == Status::Retired) => {
                "forgot"
            }
            None => "dismissed",
        },
        S::Dismissed => "dismissed",
        S::Undone => "undone",
        S::Refused => "refused",
    };
    let place_of = |f: &Fact| match fact_place(f) {
        Some(Place::Project(_)) => ("project", false),
        Some(Place::Workspace(ws)) => ("workspace", personal.as_deref() == Some(ws)),
        _ => ("user", false),
    };
    let mut v = json!({
        "state": state,
        "fact": fact,
        "target": target,
        "forget": forget,
        "reason": r.why,
        "decides": r.decides,
        // Keep applies it where the person may, else sends it on (once).
        "keep": r.decides == "you" || !sent,
        "text": input.get("text"),
        // The chat (or one above it) is one someone other than its owner
        // acted in, or holds external event data: the card says so.
        "from_marked_chat": m.chat_foreign(row.session_id).await,
    });
    if let Some(f) = fact.as_ref().or(target.as_ref()) {
        let (place, personal) = place_of(f);
        v["place"] = json!(place);
        v["workspace_personal"] = json!(personal);
    }
    Ok(v)
}

async fn call_state(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, call_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let (row, input) = own_call(&m, &who, &id, &call_id).await?;
    Ok(Json(state_json(&m, &who, &row, &call_id, &input).await?))
}

async fn decide(
    m: Master,
    headers: HeaderMap,
    id: String,
    call_id: String,
    d: crate::memory::Decision,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let (row, input) = own_call(&m, &who, &id, &call_id).await?;
    let uid = who.user_id.clone().unwrap_or_default();
    crate::memory::decide(&m, &uid, row.session_id, &call_id, d)
        .await
        .map_err(|why| ApiError(StatusCode::CONFLICT, why))?;
    Ok(Json(state_json(&m, &who, &row, &call_id, &input).await?))
}

/// Keep what the call asked for (a save, a replacement, a forgetting), for
/// one who may decide it.
async fn keep(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, call_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    decide(m, headers, id, call_id, crate::memory::Decision::Keep).await
}

/// Withdraw what the call asked for (the chat's owner always may).
async fn dismiss(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, call_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    decide(m, headers, id, call_id, crate::memory::Decision::Dismiss).await
}

/// Undo what the call did, as recorded: delete the fact it saved, restore
/// the one it retired; nothing else.
async fn undo(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, call_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    decide(m, headers, id, call_id, crate::memory::Decision::Undo).await
}
