//! `coalesce_led` while an append waits (A.21, review A21-3).

use std::sync::Mutex;
use std::time::Duration;

use harness_core::backend::TurnHost;
use harness_core::event::*;
use harness_core::policy::{Decision, ToolRisk};
use serde_json::Value;
use tokio::sync::{mpsc, watch};

use crate::*;

/// Appends wait until `open` (a full link); records what was appended.
struct Gate {
    open: watch::Receiver<bool>,
    got: Mutex<Vec<String>>,
}

#[async_trait::async_trait]
impl TurnHost for Gate {
    async fn emit(&self, kind: EventKind) -> Result<(), harness_core::backend::LogError> {
        let _ = self.open.clone().wait_for(|o| *o).await;
        if let EventKind::ToolOutputDelta(d) = kind {
            self.got.lock().unwrap().push(d.text);
        }
        Ok(())
    }
    async fn approve(&self, _c: &str, _t: &str, _i: &Value, _r: ToolRisk) -> Decision {
        Decision::Allow
    }
}

/// A source that finishes while the first append waits leaves its rest in
/// the channel (a model's deltas are not held back: the channel is
/// unbounded). That rest goes out in batches of the usual size, not as one
/// event the size of everything that piled up.
///
/// Failed before the fix: 40 KiB of 8-byte chunks went out as one event.
#[tokio::test]
async fn what_piled_up_behind_a_waiting_append_goes_out_in_batches() {
    let (open_tx, open) = watch::channel(false);
    let host = Gate {
        open,
        got: Mutex::new(vec![]),
    };
    let (tx, rx) = mpsc::unbounded_channel::<String>();
    let mut sent = String::new();
    let mut chunks = vec!["x".repeat(FLUSH_BYTES)];
    for i in 0..5_000 {
        chunks.push(format!("{i:07} "));
    }
    for c in &chunks {
        sent.push_str(c);
    }
    let source = async move {
        // The first chunk starts an append, which waits.
        tx.send(chunks[0].clone()).unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        for c in &chunks[1..] {
            tx.send(c.clone()).unwrap();
        }
    };
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        let _ = open_tx.send(true);
        // Keep the gate open until the test ends.
        tokio::time::sleep(Duration::from_secs(60)).await;
    });
    let cancel = tokio_util::sync::CancellationToken::new();
    let (step, emitted) = coalesce_led(
        source,
        rx,
        &cancel,
        &host,
        &mut None,
        &SecretSet::default(),
        |text| {
            EventKind::ToolOutputDelta(ToolOutputDelta {
                call_id: "c".into(),
                stream: OutputStream::Stdout,
                text,
            })
        },
    )
    .await;
    assert!(matches!(step, Step::Done(())) && emitted);
    let got = host.got.lock().unwrap().clone();
    let biggest = got.iter().map(String::len).max().unwrap_or(0);
    assert_eq!(got.concat(), sent, "lost or reordered output");
    assert!(
        biggest <= 2 * FLUSH_BYTES,
        "{} batches, the largest {biggest} bytes",
        got.len()
    );
}
