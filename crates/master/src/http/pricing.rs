//! The server's one currency for model prices (`[pricing]`), set by server
//! admins. The prices themselves are saved with each model on the provider
//! (`providers.rs`); everyone signed in reads both with `/api/models`.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::json;

use super::caller::Admin;
use super::{ApiError, ApiResult, runtime};
use crate::Master;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PricingBody {
    currency: String,
}

pub(super) async fn put_pricing(
    _: Admin,
    State(m): State<Master>,
    Json(b): Json<PricingBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let currency = crate::pricing::clean_currency(&b.currency)
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e))?;
    let rt = runtime(&m)?;
    let _edits = rt.lock_edits().await;
    let mut cfg = rt.config();
    cfg.pricing.currency = currency;
    super::providers::apply(&m, &rt, cfg).await?;
    tracing::info!(currency = %rt.config().pricing.currency, "model price currency set");
    Ok(Json(json!({ "currency": rt.config().pricing.currency })))
}
