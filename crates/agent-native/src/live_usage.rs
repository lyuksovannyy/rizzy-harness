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

#[cfg(test)]
mod tests {
    use super::*;
    use harness_core::policy::{Decision, ToolRisk};
    use std::sync::Mutex;

    #[derive(Default)]
    struct Rec(Mutex<Vec<EventKind>>);

    #[async_trait::async_trait]
    impl TurnHost for Rec {
        async fn emit(&self, kind: EventKind) -> Result<(), LogError> {
            self.0.lock().unwrap().push(kind);
            Ok(())
        }
        async fn approve(&self, _: &str, _: &str, _: &serde_json::Value, _: ToolRisk) -> Decision {
            Decision::Allow
        }
    }

    fn used(n: u64) -> Usage {
        Usage {
            input_tokens: n,
            output_tokens: 1,
            ..Default::default()
        }
    }

    /// Only when the total changed: the same total writes nothing, a larger
    /// one writes the running total once, as the turn's own.
    #[tokio::test]
    async fn only_a_changed_total_is_written() {
        let host = Rec::default();
        let (before, more) = (used(10), used(25));
        report(&host, "t", &before, &before).await.unwrap();
        assert!(host.0.lock().unwrap().is_empty());
        report(&host, "t", &before, &more).await.unwrap();
        let got = host.0.lock().unwrap().clone();
        assert_eq!(
            got,
            [EventKind::TurnUsage(TurnUsage {
                turn_id: "t".into(),
                usage: more
            })]
        );
    }
}
