//! Links for what the browser fetches by itself (Gate 3 P-1,
//! `auth::links`): `POST /api/links` mints them for the caller's login, and
//! the byte handlers accept them through [`require_bytes`].

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

use super::caller::User;
use super::{ApiError, ApiResult};
use crate::{Master, auth};

/// Merged into the API router by `http::router`.
pub(super) fn routes() -> Router<Master> {
    Router::new().route("/api/links", post(mint))
}

#[derive(Deserialize)]
struct MintBody {
    /// Paths with an optional query, each one an element's or a
    /// download's URL on this server.
    urls: Vec<String>,
}

/// `{"links": {url: link}}`; a URL no link is made for is left out. Only
/// for a browser login: the API token sends its header on every request.
async fn mint(
    User(who): User,
    State(m): State<Master>,
    headers: HeaderMap,
    Json(b): Json<MintBody>,
) -> ApiResult<Json<Value>> {
    if who.user_id.is_none() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "the API token needs no links".into(),
        ));
    }
    if b.urls.len() > auth::links::MAX_LINKS {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            format!("at most {} links at once", auth::links::MAX_LINKS),
        ));
    }
    let mut links = serde_json::Map::new();
    for url in b.urls {
        if let Some(link) = auth::relink(&m, &headers, &url) {
            links.insert(url, Value::String(link));
        }
    }
    Ok(Json(json!({ "links": links })))
}
