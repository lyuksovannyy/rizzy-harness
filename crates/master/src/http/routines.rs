//! Routines over HTTP (docs/routines.md). Owned by the routines backend.

use axum::Router;

use crate::Master;

/// Merged into the API router by `http::router`.
pub(super) fn routes() -> Router<Master> {
    Router::new()
}
