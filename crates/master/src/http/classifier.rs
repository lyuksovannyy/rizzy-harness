//! The bar's mode classifier, stored per user: weights and accuracy windows
//! only, never typed text. Classifier routes go here.

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use super::caller::User;
use super::{ApiError, ApiResult};
use crate::{Master, auth};

pub(super) fn routes() -> Router<Master> {
    Router::new().route(
        "/api/me/classifier",
        get(get_classifier).put(put_classifier),
    )
}

/// The bar's mode classifier, per user. Only weights and accuracy windows:
/// no typed text is ever stored.
#[derive(Debug, Clone, serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassifierState {
    version: u32,
    weights: Vec<f32>,
    bias: Vec<f32>,
    labels: u32,
    /// Last <= 100 outcomes (1 = right) for the rules and for the model.
    rules_window: Vec<u8>,
    model_window: Vec<u8>,
}

const CLASSIFIER_BUCKETS: usize = 4096;

const CLASSIFIER_CLASSES: usize = 3;

impl ClassifierState {
    fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("unsupported classifier version".into());
        }
        if self.weights.len() != CLASSIFIER_BUCKETS * CLASSIFIER_CLASSES
            || self.bias.len() != CLASSIFIER_CLASSES
        {
            return Err("classifier weights have the wrong shape".into());
        }
        if !self
            .weights
            .iter()
            .chain(&self.bias)
            .all(|w| w.is_finite() && w.abs() < 1e4)
        {
            return Err("classifier weights must be finite and bounded".into());
        }
        for w in [&self.rules_window, &self.model_window] {
            if w.len() > 100 || w.iter().any(|x| *x > 1) {
                return Err("accuracy windows hold at most 100 values of 0 or 1".into());
            }
        }
        Ok(())
    }
}

fn classifier_key(who: &auth::Principal) -> String {
    format!(
        "classifier:{}",
        who.user_id.as_deref().unwrap_or("api-token")
    )
}

async fn get_classifier(
    User(who): User,
    State(m): State<Master>,
) -> ApiResult<Json<serde_json::Value>> {
    let key = classifier_key(&who);
    let v = m.inner.mirror.call(move |s| s.kv_get(&key)).await?;
    Ok(Json(
        match v.and_then(|v| serde_json::from_str::<serde_json::Value>(&v).ok()) {
            Some(state) => state,
            None => serde_json::Value::Null,
        },
    ))
}

async fn put_classifier(
    User(who): User,
    State(m): State<Master>,
    Json(state): Json<ClassifierState>,
) -> ApiResult<Json<serde_json::Value>> {
    state
        .validate()
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e))?;
    let (key, v) = (
        classifier_key(&who),
        serde_json::to_string(&state).expect("serialize"),
    );
    m.inner.mirror.call(move |s| s.kv_set(&key, &v)).await?;
    Ok(Json(json!({ "ok": true })))
}
