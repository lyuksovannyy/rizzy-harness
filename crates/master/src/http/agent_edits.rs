//! The Agents page (docs/agent-pages.md): one agent with its instructions,
//! making, changing and deleting agents, and writing a summary from an
//! agent's instructions with the default model.

use std::time::Duration;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use harness_agent_native::{ChatMsg, ChatRequest, Provider};
use harness_agents::Features;
use harness_agents::edits::{Custom, Overlay};
use serde::Deserialize;
use serde_json::json;

use super::{
    ApiError, ApiResult, ScopeQuery, agent_json, require, require_admin, runtime, scoped_resolver,
};
use crate::Master;
use crate::agent_edits::AgentEditError;
use crate::auth::Principal;

/// Whoever may make, change and delete agents: today the server's admins,
/// because edits are server-wide. Workspace agents would add the
/// `settings` permission in the agent's workspace
/// (`workspaces::access(..)?.can(Perm::Settings)`; web: `mayEditAgents` in
/// web/src/lib/agents.ts).
async fn require_agent_editor(m: &Master, headers: &HeaderMap) -> ApiResult<Principal> {
    require_admin(m, headers).await
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

/// An agent as its page shows it: the listing in the asked scope (its
/// model line follows the workspace's chains, like `/api/agents`), plus its
/// instructions as written (a built-in's own prompt, without the additional
/// instructions), the saved summary (empty: the definition's
/// `description_default`).
async fn detail(
    m: &Master,
    who: &Principal,
    q: &ScopeQuery,
    id: &str,
) -> ApiResult<serde_json::Value> {
    let r = scoped_resolver(m, who, q).await?;
    let a = r
        .agents()
        .get(id)
        .ok_or_else(|| ApiError::from(AgentEditError::NotFound(id.into())))?;
    let mut v = agent_json(m, &r, a);
    let edits = m.agent_edits();
    match edits.custom.get(id) {
        Some(c) => {
            v["prompt"] = json!(c.prompt);
            v["summary"] = json!(c.summary);
            v["instructions"] = json!("");
            v["description_default"] = json!("");
        }
        None => {
            let defs = m.agent_definitions();
            let def = defs.get(id);
            let o = edits.overlays.get(id).cloned().unwrap_or_default();
            v["prompt"] = json!(def.map(|d| &d.spec.system_prompt));
            v["summary"] = json!(o.summary);
            v["instructions"] = json!(o.instructions);
            v["description_default"] = json!(def.map(|d| &d.description));
        }
    }
    Ok(v)
}

/// Anyone who can use an agent may read what it is told.
pub(super) async fn get_agent(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<ScopeQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require(&m, &headers).await?;
    Ok(Json(detail(&m, &who, &q, &id).await?))
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
}

pub(super) async fn create_agent(
    State(m): State<Master>,
    headers: HeaderMap,
    Query(q): Query<ScopeQuery>,
    Json(b): Json<NewAgent>,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require_agent_editor(&m, &headers).await?;
    let id = b.id.trim().to_string();
    let c = Custom {
        name: b.name,
        summary: b.summary,
        prompt: b.prompt,
        features: b.features,
        aliases: b.aliases,
        category: b.category,
    };
    m.save_custom_agent(&id, c, true).await?;
    Ok(Json(detail(&m, &who, &q, &id).await?))
}

/// The fields to change; the rest stay. Built-in and file agents take only
/// `summary` and `instructions` (the rest may be sent as they are);
/// custom ones everything but `instructions`.
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
}

pub(super) async fn update_agent(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<ScopeQuery>,
    Json(b): Json<AgentChange>,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require_agent_editor(&m, &headers).await?;
    match m.agent_edits().custom.get(&id).cloned() {
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
            };
            m.save_custom_agent(&id, c, false).await?;
        }
        None => {
            let set = m.agent_definitions();
            let Some(def) = set.get(&id) else {
                return Err(AgentEditError::NotFound(id).into());
            };
            // Aliases as the list shows them: a custom agent's id may have
            // pushed one of the definition's out.
            let shown = m.agents();
            let shown = shown.get(&id).unwrap_or(def);
            if b.name.as_ref().is_some_and(|n| n.trim() != def.name)
                || b.prompt.is_some()
                || b.features.is_some_and(|f| f != def.features)
                || b.aliases.as_ref().is_some_and(|a| *a != shown.aliases)
                || b.category
                    .as_ref()
                    .is_some_and(|c| c.trim() != def.category)
            {
                return Err(m.edit_refusal(&id).into());
            }
            let old = m
                .agent_edits()
                .overlays
                .get(&id)
                .cloned()
                .unwrap_or_default();
            let o = Overlay {
                summary: b.summary.unwrap_or(old.summary),
                instructions: b.instructions.unwrap_or(old.instructions),
            };
            m.save_agent_overlay(&id, o).await?;
        }
    }
    Ok(Json(detail(&m, &who, &q, &id).await?))
}

/// Deletes an agent made here, with its look and model chain, so a new
/// agent with the same handle starts fresh. Chats that used it go on with
/// the default agent.
pub(super) async fn delete_agent(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    require_agent_editor(&m, &headers).await?;
    m.delete_custom_agent(&id).await?;
    if let Some(rt) = m.provider_runtime() {
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
    State(m): State<Master>,
    headers: HeaderMap,
    Json(b): Json<SummaryBody>,
) -> ApiResult<Json<serde_json::Value>> {
    require_agent_editor(&m, &headers).await?;
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
