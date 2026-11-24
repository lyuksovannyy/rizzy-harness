//! The sidebar search panel (`docs/sidebar-search.md`) and the list of search
//! tabs a caller has. Search routes go here.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use super::caller::User;
use super::{ApiError, ApiResult, query_flag};
use crate::workspaces as ws;
use crate::{Master, auth};

pub(super) fn routes() -> Router<Master> {
    Router::new().route("/api/search", get(search))
}

#[derive(Deserialize)]
struct SearchQuery {
    /// Empty: the newest items.
    #[serde(default)]
    q: String,
    /// `all`, `chats`, `projects`, `media` or `artifacts`; `chats` when left
    /// out (the panel always names it).
    #[serde(default, rename = "in")]
    kind: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    /// The previous page's `next`.
    #[serde(default)]
    cursor: Option<String>,
    /// Similar-chat suggestions for the bar (strict; may return nothing).
    #[serde(default, deserialize_with = "query_flag")]
    suggest: bool,
    /// Users: defaults to the active workspace. The API token: no filter.
    #[serde(default)]
    workspace: Option<String>,
}

/// The search panel (docs/sidebar-search.md): one tab's page, or All. Chats
/// are the caller's own for everyone, admins included (T10); everything is
/// in the one workspace asked for (members only).
async fn search(
    User(who): User,
    State(m): State<Master>,
    Query(q): Query<SearchQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let scope = ws::scope(&m, &who, q.workspace.as_deref()).await?;
    let kinds = search_kinds(&m, &who, scope.as_ref()).await;
    let owner = who.user_id.clone();
    let workspace = scope.map(|a| a.workspace.workspace_id);
    let text: String = q.q.chars().take(500).collect();
    if q.suggest {
        let results = m
            .inner
            .mirror
            .call(move |s| crate::search::suggest(s, &text, owner.as_deref(), workspace.as_deref()))
            .await?;
        return Ok(Json(
            json!({ "results": results, "next": null, "kinds": kinds }),
        ));
    }
    let asked = q.kind.as_deref().unwrap_or("chats");
    let all = asked == "all";
    if !all && !crate::search::KINDS.contains(&asked) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "in must be all, chats, projects, media or artifacts".into(),
        ));
    }
    let offset = match q.cursor.as_deref() {
        None | Some("") => 0,
        Some(c) => c
            .parse::<u32>()
            .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "bad cursor".into()))?
            as usize,
    };
    // A kind whose area the viewer does not have answers nothing.
    let searched = kinds
        .iter()
        .copied()
        .filter(|k| all || *k == asked)
        .collect();
    let r = crate::search::Request {
        q: text,
        kinds: searched,
        all,
        owner,
        workspace,
        limit: q.limit.unwrap_or(20).clamp(1, 50),
        offset,
    };
    let page = m
        .inner
        .mirror
        .call(move |s| crate::search::run(s, &r))
        .await?;
    Ok(Json(json!({
        "results": page.results,
        "next": page.next.map(|n| n.to_string()),
        "kinds": kinds,
    })))
}

/// The search tabs `who` has in `scope`: chats and projects always, media
/// and artifacts while their areas show (the same flags `/api/features`
/// sends).
async fn search_kinds(
    m: &Master,
    who: &auth::Principal,
    scope: Option<&ws::Access>,
) -> Vec<&'static str> {
    let media = crate::media::features(m, who, scope).await;
    let artifacts = crate::artifacts::features(m, who, scope).await;
    kinds_shown(&media, &artifacts)
}

pub(super) fn kinds_shown(
    media: &serde_json::Value,
    artifacts: &serde_json::Value,
) -> Vec<&'static str> {
    let mut kinds = vec!["chats", "projects"];
    for (kind, block) in [("media", media), ("artifacts", artifacts)] {
        if block["show"] == true {
            kinds.push(kind);
        }
    }
    kinds
}
