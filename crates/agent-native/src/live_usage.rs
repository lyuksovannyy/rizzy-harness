//! A turn's usage while it runs (X.39): after a model call (or a summary, a
//! sub-agent's result, a parallel answer) added to the turn's usage, the
//! loop hands the running total to the host, which appends it as a
//! `turn_usage` event like every other event (one writer, contiguous seq).
//! Only when the total changed; the total is the turn's own sum, so the
//! event holds numbers and the model's id, nothing the model wrote.

use harness_core::backend::{LogError, TurnHost};
use harness_core::event::{EventKind, TurnUsage, Usage};

/// Report `now` as the turn's running usage when it differs from `before`
/// (the total before the call that was just added).
pub async fn report(
    host: &dyn TurnHost,
    turn_id: &str,
    before: &Usage,
    now: &Usage,
) -> Result<(), LogError> {
    if before == now {
        return Ok(());
    }
    host.emit(EventKind::TurnUsage(TurnUsage {
        turn_id: turn_id.to_string(),
        usage: now.clone(),
    }))
    .await
}
