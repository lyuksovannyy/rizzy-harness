//! The Agents page (docs/agent-pages.md): one agent with its instructions,
//! making, changing and deleting agents at the server's scope or in one
//! workspace (X.33, PLAN.md T79), and writing a summary from an agent's
//! instructions with the default model.

use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use harness_agent_native::{ChatMsg, ChatRequest, Provider};
use harness_agents::Features;
use harness_agents::edits::{Custom, Overlay};
use serde::Deserialize;
use serde_json::json;

use super::agent_edits;
use super::caller::User;
use super::{ApiError, ApiResult, runtime};
use crate::Master;
use crate::agent_edits::{AgentEditError, Place};
use crate::auth::{self, Principal};
use crate::workspaces::{self as ws, Perm};

pub(super) fn routes() -> Router<Master> {
    Router::new()
        .route("/api/models", get(models))
        .route("/api/agents", get(agents).post(create_agent))
        .route(
            "/api/agents/{id}",
            get(get_agent).patch(update_agent).delete(delete_agent),
        )
        .route("/api/agent-summary", post(generate_summary))
}

/// Where a write goes and whether `who` may make it (PLAN.md T79): a write
/// names its scope. `?scope=server` is the server's set, for the server's
/// admins; `?workspace=<id>` is that workspace's own, for whoever holds its
/// `settings` permission (its owners and admins; everyone in their own
/// Personal workspace; the API token, which acts as an owner of any
/// workspace). A non-member, a server admin included, gets 404 as for every
/// workspace read (`workspaces::access`). The web asks the same through
/// `may_edit` on the listing.
async fn require_agent_editor(
    m: &Master,
    who: Principal,
    q: &ScopeQuery,
) -> ApiResult<(Principal, Place)> {
    let place = match (q.scope.as_deref(), q.workspace.as_deref()) {
        (Some("server"), None) => {
            if !who.is_admin() {
                return Err(ApiError(
                    StatusCode::FORBIDDEN,
                    "only the server's admins change the server's agents".into(),
                ));
            }
            Place::Server
        }
        (None, Some(id)) => {
            let a = ws::access(m, &who, id).await?;
            a.require(Perm::Settings)?;
            Place::Workspace(a.id().to_string())
        }
        (None, None) => {
            return Err(ApiError(
                StatusCode::BAD_REQUEST,
                "say where the change goes: ?workspace=<id> or ?scope=server".into(),
            ));
        }
        _ => {
            return Err(ApiError(
                StatusCode::BAD_REQUEST,
                "ask for a workspace or for the server scope, not both".into(),
            ));
        }
    };
    Ok((who, place))
}

/// Whether `who` may change the agents of the scope a read resolved to
/// (`None`: the server's).
pub(super) fn may_edit(who: &Principal, scope: Option<&ws::Access>) -> bool {
    match scope {
        None => who.is_admin(),
        Some(a) => a.can(Perm::Settings),
    }
}

/// A workspace's own agents that a server agent with the same id hides
/// there (made at the server's scope after them, or a later file): the
/// server's agent wins and theirs is skipped (PLAN.md T79). Only the
/// workspace's editors are told, so they can delete or remake it.
pub(super) async fn hidden(
    m: &Master,
    who: &Principal,
    scope: Option<&ws::Access>,
) -> Vec<serde_json::Value> {
    let Some(a) = scope.filter(|a| may_edit(who, Some(a))) else {
        return Vec::new();
    };
    let layer = m.workspace_layer(a.id()).await;
    layer
        .edits
        .custom
        .iter()
        .filter(|(id, _)| {
            layer
                .set
                .get(id)
                .is_none_or(|x| x.scope != harness_agents::AgentScope::Workspace)
        })
        .map(|(id, c)| json!({ "id": id, "name": c.name }))
        .collect()
}

impl From<AgentEditError> for ApiError {
    fn from(e: AgentEditError) -> Self {
        let code = match &e {
            AgentEditError::NotFound(_) => StatusCode::NOT_FOUND,
            AgentEditError::Refused(_) => StatusCode::CONFLICT,
            AgentEditError::Bad(_) => StatusCode::BAD_REQUEST,
            AgentEditError::Store(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        ApiError(code, e.to_string())
    }
}

/// An agent as its page shows it in `place`: the listing in that scope
/// (its model line follows the workspace's chains, like `/api/agents`),
/// plus its instructions as written (a built-in's own prompt, without any
/// additional instructions), `place`'s own summary and additional
/// instructions (empty summary: `description_default`), in a workspace the
/// server's additional instructions (`server_instructions`, read-only
/// there), and whether `place` made it (`own`: edited whole).
async fn detail(
    m: &Master,
    r: &crate::Resolver,
    place: &Place,
    id: &str,
) -> ApiResult<serde_json::Value> {
    let a = r
        .agents()
        .get(id)
        .ok_or_else(|| ApiError::from(AgentEditError::NotFound(id.into())))?;
    let mut v = agent_json(m, r, a);
    let (own, _) = m.place_edits(place).await;
    let server = m.agent_edits();
    v["own"] = json!(own.custom.contains_key(id));
    // The stored "may delegate to" list (the resolved delegates are
    // `delegates`); entries may name agents that are gone.
    v["delegate_to"] = json!(own.custom.get(id).and_then(|c| c.delegates.as_ref()));
    v["server_instructions"] = json!("");
    match own.custom.get(id) {
        Some(c) => {
            v["prompt"] = json!(c.prompt);
            v["summary"] = json!(c.summary);
            v["instructions"] = json!("");
            v["description_default"] = json!("");
        }
        None => {
            // What the wider scope made: a definition, or a server agent
            // seen from a workspace.
            let defs = m.agent_definitions();
            let wider = m.agents();
            let (prompt, default) = match (defs.get(id), server.custom.get(id)) {
                (Some(d), _) => (d.spec.system_prompt.clone(), d.description.clone()),
                (None, Some(c)) => (c.prompt.clone(), c.summary.clone()),
                (None, None) => (a.spec.system_prompt.clone(), a.description.clone()),
            };
            let o = own.overlays.get(id).cloned().unwrap_or_default();
            v["prompt"] = json!(prompt);
            v["summary"] = json!(o.summary);
            v["instructions"] = json!(o.instructions);
            v["description_default"] = json!(default);
            if let Place::Workspace(_) = place {
                let so = server.overlays.get(id).cloned().unwrap_or_default();
                v["server_instructions"] = json!(so.instructions);
                v["description_default"] =
                    json!(wider.get(id).map_or(default, |w| w.description.clone()));
            }
        }
    }
    Ok(v)
}

/// The resolver and place a write's answer is read in.
async fn written(m: &Master, place: &Place) -> crate::Resolver {
    match place {
        Place::Server => m.resolver(None),
        Place::Workspace(ws) => m.resolver_for(Some(ws)).await,
    }
}

/// Anyone who can use an agent may read what it is told.
pub(super) async fn get_agent(
    User(who): User,
    State(m): State<Master>,
    Path(id): Path<String>,
    Query(q): Query<ScopeQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let scope = settings_scope(&m, &who, &q).await?;
    let (place, r) = match &scope {
        Some(a) => (Place::Workspace(a.id().to_string()), a.resolver(&m).await),
        None => (Place::Server, m.resolver(None)),
    };
    let mut v = detail(&m, &r, &place, &id).await?;
    v["may_edit"] = json!(may_edit(&who, scope.as_ref()));
    Ok(Json(v))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NewAgent {
    /// The handle (`@id`), fixed once made.
    id: String,
    name: String,
    #[serde(default)]
    summary: String,
    prompt: String,
    features: Features,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    category: Option<String>,
    /// Who it may delegate to; absent or null: every sub-agent; `[]`: none.
    #[serde(default)]
    delegate_to: Option<Vec<String>>,
}

pub(super) async fn create_agent(
    User(who): User,
    State(m): State<Master>,
    Query(q): Query<ScopeQuery>,
    Json(b): Json<NewAgent>,
) -> ApiResult<Json<serde_json::Value>> {
    let (_, place) = require_agent_editor(&m, who, &q).await?;
    let id = b.id.trim().to_string();
    let c = Custom {
        name: b.name,
        summary: b.summary,
        prompt: b.prompt,
        features: b.features,
        aliases: b.aliases,
        category: b.category,
        delegates: b.delegate_to,
    };
    m.save_custom_agent(&place, &id, c, true).await?;
    Ok(Json(
        detail(&m, &written(&m, &place).await, &place, &id).await?,
    ))
}

/// The fields to change; the rest stay. Agents the scope did not make
/// (built-in, file, and in a workspace the server's) take only `summary`
/// and `instructions` (the rest may be sent as they are); its own agents
/// everything but `instructions`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AgentChange {
    name: Option<String>,
    summary: Option<String>,
    prompt: Option<String>,
    instructions: Option<String>,
    features: Option<Features>,
    aliases: Option<Vec<String>>,
    category: Option<String>,
    /// Absent: keep; null: every sub-agent again; an array: only these (`[]`: none).
    #[serde(default, deserialize_with = "present")]
    delegate_to: Option<Option<Vec<String>>>,
}

/// A field that is there (null or not) as `Some`, so absent and null differ.
fn present<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<Option<Vec<String>>>, D::Error> {
    Option::<Vec<String>>::deserialize(d).map(Some)
}

pub(super) async fn update_agent(
    User(who): User,
    State(m): State<Master>,
    Path(id): Path<String>,
    Query(q): Query<ScopeQuery>,
    Json(b): Json<AgentChange>,
) -> ApiResult<Json<serde_json::Value>> {
    let (_, place) = require_agent_editor(&m, who, &q).await?;
    let (own, seen) = m.place_edits(&place).await;
    match own.custom.get(&id).cloned() {
        Some(old) => {
            if b.instructions
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty())
            {
                return Err(ApiError(
                    StatusCode::BAD_REQUEST,
                    "an agent made here has no additional instructions: change its instructions"
                        .into(),
                ));
            }
            let c = Custom {
                name: b.name.unwrap_or(old.name),
                summary: b.summary.unwrap_or(old.summary),
                prompt: b.prompt.unwrap_or(old.prompt),
                features: b.features.unwrap_or(old.features),
                aliases: b.aliases.unwrap_or(old.aliases),
                category: b.category.or(old.category),
                delegates: b.delegate_to.unwrap_or(old.delegates),
            };
            m.save_custom_agent(&place, &id, c, false).await?;
        }
        None => {
            let set = m.wider(&place);
            let Some(def) = set.get(&id) else {
                return Err(AgentEditError::NotFound(id).into());
            };
            // Aliases as the list shows them: a custom agent's id may have
            // pushed one of the definition's out.
            let shown = seen.get(&id).unwrap_or(def);
            if b.name.as_ref().is_some_and(|n| n.trim() != def.name)
                || b.prompt.is_some()
                || b.features.is_some_and(|f| f != def.features)
                || b.aliases.as_ref().is_some_and(|a| *a != shown.aliases)
                || b.category
                    .as_ref()
                    .is_some_and(|c| c.trim() != def.category)
                // Any array means something now (`[]`: none), so it is refused here too.
                || matches!(b.delegate_to, Some(Some(_)))
            {
                return Err(m.edit_refusal(&place, &id).await.into());
            }
            let old = own.overlays.get(&id).cloned().unwrap_or_default();
            let o = Overlay {
                summary: b.summary.unwrap_or(old.summary),
                instructions: b.instructions.unwrap_or(old.instructions),
            };
            m.save_agent_overlay(&place, &id, o).await?;
        }
    }
    Ok(Json(
        detail(&m, &written(&m, &place).await, &place, &id).await?,
    ))
}

/// Deletes an agent made at the scope. At the server's scope its look and
/// model chain go too (config.toml), in a workspace its chain there, so a
/// new agent with the same handle starts fresh. Chats that used it refuse
/// their next prompt until another agent is chosen (`AgentGone`).
pub(super) async fn delete_agent(
    User(who): User,
    State(m): State<Master>,
    Path(id): Path<String>,
    Query(q): Query<ScopeQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let (_, place) = require_agent_editor(&m, who, &q).await?;
    m.delete_custom_agent(&place, &id).await?;
    if place == Place::Server
        && let Some(rt) = m.provider_runtime()
    {
        let _edits = rt.lock_edits().await;
        let mut cfg = rt.config();
        let looks = cfg.agent_avatars.remove(&id).is_some();
        let chain = cfg.agent_models.remove(&id).is_some();
        if looks || chain {
            super::providers::apply(&m, &rt, cfg)?;
        }
    }
    Ok(Json(json!({ "ok": true })))
}

/// What the default model is asked to do with an agent's instructions.
const SUMMARY_PROMPT: &str = "You write the one-line summary shown under an AI agent's name in \
a list of agents. Given the agent's name and its instructions, say in one or two sentences (at \
most 200 characters) what the agent is for. Start with a noun phrase, like \"Software engineer \
that reads, edits and tests code.\" or \"Read-only researcher that…\". Plain text only: no \
quotes, no markdown, no preamble.";
const MAX_GENERATED: usize = 300;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SummaryBody {
    #[serde(default)]
    name: String,
    prompt: String,
}

/// A summary written by the default model from the instructions in the
/// editor (saved or not); nothing is stored. One short request.
pub(super) async fn generate_summary(
    User(who): User,
    State(m): State<Master>,
    Query(q): Query<ScopeQuery>,
    Json(b): Json<SummaryBody>,
) -> ApiResult<Json<serde_json::Value>> {
    require_agent_editor(&m, who, &q).await?;
    let prompt = b.prompt.trim();
    if prompt.is_empty() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "write the instructions first".into(),
        ));
    }
    let rt = runtime(&m)?;
    let model = m
        .agent_spec(None, None)
        .ok()
        .and_then(|s| s.model)
        .ok_or_else(|| ApiError(StatusCode::BAD_REQUEST, "no default model is set".into()))?;
    let req = ChatRequest {
        model,
        messages: vec![
            ChatMsg::System(SUMMARY_PROMPT.into()),
            ChatMsg::User(format!(
                "Name: {}\n\nInstructions:\n{}",
                b.name.trim(),
                prompt.chars().take(40_000).collect::<String>()
            )),
        ],
        tools: vec![],
        effort: None,
        cache: false,
        limits: Some(Default::default()),
        no_tool_calls: false,
        max_tokens: None,
    };
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let done = tokio::time::timeout(Duration::from_secs(60), rt.provider.complete(&req, tx))
        .await
        .map_err(|_| {
            ApiError(
                StatusCode::GATEWAY_TIMEOUT,
                "the model took too long".into(),
            )
        })?
        .map_err(|e| ApiError(StatusCode::BAD_GATEWAY, format!("the model failed: {e}")))?;
    let summary = tidy(&done.text);
    if summary.is_empty() {
        return Err(ApiError(
            StatusCode::BAD_GATEWAY,
            "the model returned no summary".into(),
        ));
    }
    Ok(Json(json!({ "summary": summary })))
}

/// The reply's first paragraph on one line, without quotes or a label,
/// cut at a word to fit.
fn tidy(reply: &str) -> String {
    let first = reply.trim().split("\n\n").next().unwrap_or_default();
    // One line of plain words: no control or direction-changing characters.
    let line: String = first
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}'))
        .collect();
    let line = line
        .strip_prefix("Summary:")
        .unwrap_or(&line)
        .trim()
        .trim_matches(|c| matches!(c, '"' | '\'' | '“' | '”' | '*' | '`'))
        .trim();
    if line.chars().count() <= MAX_GENERATED {
        return line.to_string();
    }
    let cut: String = line.chars().take(MAX_GENERATED - 1).collect();
    let cut = cut.rsplit_once(' ').map_or(cut.as_str(), |(head, _)| head);
    format!("{}…", cut.trim_end_matches([',', ';', ':']))
}

/// Which settings a read of models, agents or notices follows.
#[derive(Deserialize)]
pub(super) struct ScopeQuery {
    /// A workspace the caller is a member of (users: defaults to the
    /// active one; the API token: the server's without it).
    #[serde(default)]
    workspace: Option<String>,
    /// `server`: the server's own values, without any workspace's.
    #[serde(default)]
    scope: Option<String>,
}

/// The workspace whose settings apply, or `None` for the server's own.
pub(super) async fn settings_scope(
    m: &Master,
    who: &auth::Principal,
    q: &ScopeQuery,
) -> ApiResult<Option<ws::Access>> {
    match q.scope.as_deref() {
        None => Ok(ws::scope(m, who, q.workspace.as_deref()).await?),
        Some("server") if q.workspace.is_none() => Ok(None),
        Some("server") => Err(ApiError(
            StatusCode::BAD_REQUEST,
            "ask for a workspace or for the server scope, not both".into(),
        )),
        Some(_) => Err(ApiError(
            StatusCode::BAD_REQUEST,
            "scope must be server".into(),
        )),
    }
}

async fn scoped_resolver(
    m: &Master,
    who: &auth::Principal,
    q: &ScopeQuery,
) -> ApiResult<crate::Resolver> {
    Ok(match settings_scope(m, who, q).await? {
        Some(a) => a.resolver(m).await,
        None => m.resolver(None),
    })
}

/// The listed models; `default` is the scope's default model.
async fn models(
    User(who): User,
    State(m): State<Master>,
    Query(q): Query<ScopeQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let r = scoped_resolver(&m, &who, &q).await?;
    Ok(Json(json!({ "models": r.models() })))
}

/// An agent as the UI lists it in a scope: its definition, the model
/// assigned to it there, the model it runs on and the fallbacks it has
/// there (its own chain's, else the ones it inherits with the default),
/// which of those are unavailable, and its avatar (server-wide).
pub(super) fn agent_json(
    m: &Master,
    r: &crate::Resolver,
    a: &harness_agents::Agent,
) -> serde_json::Value {
    let chain = r.agent_chain(&a.id);
    let main = r.agent_main_model(a);
    let fallbacks = r.agent_fallbacks(&a.id, main.as_deref());
    let mut unavailable: Vec<String> = Vec::new();
    for x in main.iter().chain(&fallbacks) {
        if r.model_unavailable(x) && !unavailable.contains(x) {
            unavailable.push(x.clone());
        }
    }
    let mut v = json!(a);
    v["default_model"] = json!(chain.map(|c| c.model));
    v["main_model"] = json!(main);
    v["fallbacks"] = json!(fallbacks);
    v["unavailable"] = json!(unavailable);
    v["avatar"] = json!(m.agent_avatar(&a.id));
    v
}

async fn agents(
    User(who): User,
    State(m): State<Master>,
    Query(q): Query<ScopeQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let scope = settings_scope(&m, &who, &q).await?;
    let r = match &scope {
        Some(a) => a.resolver(&m).await,
        None => m.resolver(None),
    };
    let agents: Vec<serde_json::Value> = r
        .agents()
        .list()
        .iter()
        .map(|a| agent_json(&m, &r, a))
        .collect();
    Ok(Json(json!({
        "agents": agents,
        // The ordinary-mode default, as before.
        "default": r.default_agent(),
        // The default per mode, for the picker to preselect; null when
        // the setting names an agent that cannot work there.
        "defaults": {
            "chat": r.default_for(harness_core::event::Profile::Chat).ok(),
            "agentic": r.default_for(harness_core::event::Profile::Agentic).ok(),
        },
        // Whether the caller may make and change agents in this scope.
        "may_edit": agent_edits::may_edit(&who, scope.as_ref()),
        // For the workspace's editors: its own agents a server agent with
        // the same id hides (PLAN.md T79), to delete or remake.
        "hidden": agent_edits::hidden(&m, &who, scope.as_ref()).await,
    })))
}

#[cfg(test)]
mod tests {
    use super::tidy;

    #[test]
    fn tidy_keeps_one_plain_line() {
        assert_eq!(
            tidy("Summary: \"Planner that\nsplits work.\"\n\nMore."),
            "Planner that splits work."
        );
        let long = "word ".repeat(100);
        let t = tidy(&long);
        assert!(t.chars().count() <= 300 && t.ends_with("word…"), "{t}");
    }
}
