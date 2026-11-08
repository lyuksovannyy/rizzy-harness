//! Media over HTTP (docs/media.md). Owned by the files backend.

use axum::Router;

use crate::Master;

/// Merged into the API router by `http::router`.
pub(super) fn routes() -> Router<Master> {
    Router::new()
}
