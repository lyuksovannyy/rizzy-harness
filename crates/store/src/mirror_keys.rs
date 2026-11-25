//! The `kv` keys the master keeps per chat in `master.db`, and the rules
//! about them that the master and a backup's copy step share (one place,
//! so the copy can never apply a rule the live server does not).

use harness_core::SessionId;

/// A create's intended owner, recorded before the node hears of the chat
/// (Gate 3 review P-6).
pub const CREATE_INTENT_PREFIX: &str = "create-intent:";
/// How long an intent waits for its chat; an older one is dropped and the
/// chat, if it ever lands, stays ownerless.
pub const CREATE_INTENT_TTL_MS: i64 = 60 * 60 * 1000;
/// The mark that a chat follows the default model.
pub const AUTO_MODEL_PREFIX: &str = "session-model-auto:";
/// A callback turn not seen yet.
pub const CALLBACK_ATTENTION_PREFIX: &str = "callback-attention:";

pub fn create_intent_key(sid: SessionId) -> String {
    format!("{CREATE_INTENT_PREFIX}{sid}")
}

pub fn auto_model_key(sid: SessionId) -> String {
    format!("{AUTO_MODEL_PREFIX}{sid}")
}

pub fn callback_attention_key(sid: SessionId) -> String {
    format!("{CALLBACK_ATTENTION_PREFIX}{sid}")
}

/// Whether a stored intent is older than [`CREATE_INTENT_TTL_MS`] at
/// `now` (one that cannot be read counts as old).
pub fn intent_expired(v: &str, now: i64) -> bool {
    serde_json::from_str::<serde_json::Value>(v)
        .ok()
        .and_then(|v| v["at"].as_i64())
        .is_none_or(|at| now - at > CREATE_INTENT_TTL_MS)
}
