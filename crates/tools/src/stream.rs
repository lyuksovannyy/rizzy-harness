//! Live output of one tool call, with a total budget (PLAN.md T9).
//!
//! Every chunk a tool streams becomes a stored, replicated event, so the
//! stream has a byte budget per call. Up to it, chunks pass unchanged; the
//! chunk that crosses it is cut and ends with [`TRUNCATED_MARKER`], and
//! nothing is sent after that. The channel is bounded: a sender waits while
//! the consumer is behind, which pushes back onto whatever reads the pipes.

use std::sync::Mutex;

use harness_core::event::OutputStream;
use tokio::sync::mpsc;

/// Default budget for one call's streamed output (`[limits]
/// tool_stream_bytes` in node.toml).
pub const DEFAULT_STREAM_BYTES: usize = 1 << 20;
/// Smallest budget node.toml may set.
pub const MIN_STREAM_BYTES: usize = 4096;
/// Largest budget node.toml may set.
pub const MAX_STREAM_BYTES: usize = 1 << 30;
/// Chunks in flight between a tool and the consumer.
pub const STREAM_CHANNEL: usize = 32;
/// Ends the last chunk once the budget is spent.
pub const TRUNCATED_MARKER: &str = "\n[output truncated: further output is not shown or stored]\n";

/// The sending end handed to a tool as `ToolCtx::output`.
pub struct OutputSink {
    tx: mpsc::Sender<(OutputStream, String)>,
    budget: usize,
    /// Bytes sent so far, and whether the marker went out.
    state: Mutex<(usize, bool)>,
}

impl OutputSink {
    /// A sink with `budget` bytes and the receiver for its chunks.
    pub fn channel(budget: usize) -> (Self, mpsc::Receiver<(OutputStream, String)>) {
        let (tx, rx) = mpsc::channel(STREAM_CHANNEL);
        (Self::new(tx, budget), rx)
    }

    pub fn new(tx: mpsc::Sender<(OutputStream, String)>, budget: usize) -> Self {
        Self {
            tx,
            budget,
            state: Mutex::new((0, false)),
        }
    }

    /// Send a chunk, waiting for room in the channel. Past the budget it
    /// returns at once and sends nothing. Returns false once the budget is
    /// spent (or the consumer is gone).
    pub async fn send(&self, stream: OutputStream, mut s: String) -> bool {
        let cut = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            if st.1 {
                return false;
            }
            let room = self.budget.saturating_sub(st.0);
            if s.len() <= room {
                st.0 += s.len();
                false
            } else {
                let mut at = room;
                while !s.is_char_boundary(at) {
                    at -= 1;
                }
                s.truncate(at);
                s.push_str(TRUNCATED_MARKER);
                *st = (self.budget, true);
                true
            }
        };
        self.tx.send((stream, s)).await.is_ok() && !cut
    }

    /// Whether the budget was spent and output was dropped.
    pub fn truncated(&self) -> bool {
        self.state.lock().unwrap_or_else(|p| p.into_inner()).1
    }

    pub fn budget(&self) -> usize {
        self.budget
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cuts_once_at_the_budget() {
        let (sink, mut rx) = OutputSink::channel(10);
        assert!(sink.send(OutputStream::Stdout, "12345".into()).await);
        assert!(!sink.send(OutputStream::Stdout, "6789abcdef".into()).await);
        assert!(!sink.send(OutputStream::Stdout, "more".into()).await);
        assert!(sink.truncated());
        assert_eq!(rx.recv().await.unwrap().1, "12345");
        assert_eq!(
            rx.recv().await.unwrap().1,
            format!("6789a{TRUNCATED_MARKER}")
        );
        drop(sink);
        assert!(rx.recv().await.is_none(), "nothing after the marker");
    }

    #[tokio::test]
    async fn cut_respects_char_boundaries() {
        let (sink, mut rx) = OutputSink::channel(4);
        sink.send(OutputStream::Stdout, "aéé".into()).await;
        assert_eq!(rx.recv().await.unwrap().1, format!("aé{TRUNCATED_MARKER}"));
    }
}
