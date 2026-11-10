//! Full outputs of filtered or truncated tool calls, kept on the node so the
//! model can fetch what the filter dropped (`tool_output`). In memory only,
//! bounded, gone after a restart.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use harness_core::SessionId;
use harness_core::policy::ToolRisk;
use serde_json::{Value, json};

use crate::{Tool, ToolCtx, ToolOutcome, str_arg};

/// Outputs kept per session (its oldest goes first).
pub const KEEP: usize = 64;
/// Bounds over all sessions of the node. Past one, the session holding the
/// most bytes loses its oldest output, so one busy session cannot push out
/// everyone else's.
const KEEP_TOTAL: usize = 512;
const MAX_TOTAL_BYTES: usize = 128 << 20;
/// Largest output stored (characters).
pub const MAX_STORED: usize = 2_000_000;

/// Full outputs by session. Every access names the session, and an entry
/// is only ever returned to the session that stored it.
#[derive(Default)]
pub struct OutputStore {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    sessions: HashMap<SessionId, Kept>,
    entries: usize,
    bytes: usize,
}

#[derive(Default)]
struct Kept {
    items: VecDeque<(String, String)>,
    bytes: usize,
}

impl Inner {
    fn pop_oldest(&mut self, sid: SessionId) {
        let Some(kept) = self.sessions.get_mut(&sid) else {
            return;
        };
        if let Some((_, full)) = kept.items.pop_front() {
            kept.bytes -= full.len();
            self.bytes -= full.len();
            self.entries -= 1;
        }
        if kept.items.is_empty() {
            self.sessions.remove(&sid);
        }
    }
}

/// A fresh id: 64 bits from the OS, as 16 hex characters (short enough for
/// a model to copy). `None` if the OS has no randomness to give.
fn random_id() -> Option<String> {
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).ok()?;
    Some(b.iter().map(|x| format!("{x:02x}")).collect())
}

impl OutputStore {
    /// Keep `full` for `session` and return its id; `None` when no id
    /// could be made (the caller then shows the output without one).
    pub fn put(&self, session: SessionId, full: String) -> Option<String> {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let id = loop {
            let id = random_id()?;
            let taken = inner
                .sessions
                .get(&session)
                .is_some_and(|k| k.items.iter().any(|(i, _)| *i == id));
            if !taken {
                break id;
            }
        };
        if inner
            .sessions
            .get(&session)
            .is_some_and(|k| k.items.len() >= KEEP)
        {
            inner.pop_oldest(session);
        }
        let len = full.len();
        let kept = inner.sessions.entry(session).or_default();
        kept.items.push_back((id.clone(), full));
        kept.bytes += len;
        inner.entries += 1;
        inner.bytes += len;
        while inner.entries > KEEP_TOTAL || inner.bytes > MAX_TOTAL_BYTES {
            let heaviest = inner
                .sessions
                .iter()
                .max_by_key(|(_, k)| (k.bytes, k.items.len()))
                .map(|(s, _)| *s);
            match heaviest {
                Some(s) => inner.pop_oldest(s),
                None => break,
            }
        }
        Some(id)
    }

    /// The output `id` that `session` stored, if it is still kept.
    pub fn get(&self, session: SessionId, id: &str) -> Option<String> {
        self.inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .sessions
            .get(&session)?
            .items
            .iter()
            .find(|(i, _)| i == id)
            .map(|(_, s)| s.clone())
    }

    /// The store as one session sees it (what a tool call is given).
    pub fn session(self: &Arc<Self>, session: SessionId) -> SessionOutputs {
        SessionOutputs {
            store: self.clone(),
            session,
        }
    }
}

/// One session's view of an [`OutputStore`]: a tool call can only store
/// and read the outputs of the session it runs for.
#[derive(Clone)]
pub struct SessionOutputs {
    store: Arc<OutputStore>,
    session: SessionId,
}

impl SessionOutputs {
    pub fn put(&self, full: String) -> Option<String> {
        self.store.put(self.session, full)
    }

    pub fn get(&self, id: &str) -> Option<String> {
        self.store.get(self.session, id)
    }
}

/// Read a stored full output: a line range, or only the lines containing a
/// text.
pub struct ToolOutput;

#[async_trait::async_trait]
impl Tool for ToolOutput {
    fn name(&self) -> &'static str {
        "tool_output"
    }

    fn description(&self) -> &'static str {
        "Read the full output of an earlier command whose result was filtered or \
         shortened (its result names the id). Give `contains` to get only matching lines, \
         or `offset`/`limit` for a line range."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {"type": "string"},
                "contains": {"type": "string", "description": "Only lines containing this text"},
                "offset": {"type": "integer", "description": "1-based first line (default 1)"},
                "limit": {"type": "integer", "description": "Max lines (default 200)"}
            },
            "required": ["id"]
        })
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }

    fn enabled(&self, s: &harness_core::savings::Savings) -> bool {
        s.filter_shell_output
    }

    async fn run(&self, ctx: &ToolCtx, input: Value) -> ToolOutcome {
        let id = match str_arg(&input, "id") {
            Ok(v) => v,
            Err(e) => return ToolOutcome::err(e),
        };
        let Some(full) = ctx.outputs.as_ref().and_then(|o| o.get(id)) else {
            return ToolOutcome::err(format!(
                "no stored output {id:?} (this chat's last {KEEP} filtered commands \
                 are kept until the node restarts); run the command again"
            ));
        };
        let offset = input
            .get("offset")
            .and_then(|v| v.as_u64())
            .unwrap_or(1)
            .max(1) as usize;
        let limit = input
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(200)
            .clamp(1, 2000) as usize;
        let contains = input.get("contains").and_then(|v| v.as_str());
        let lines: Vec<(usize, &str)> = full
            .lines()
            .enumerate()
            .filter(|(_, l)| contains.is_none_or(|c| l.contains(c)))
            .collect();
        let total = lines.len();
        let mut out = String::new();
        for (n, l) in lines.iter().skip(offset - 1).take(limit) {
            out.push_str(&format!("{}\t{l}\n", n + 1));
        }
        let shown_end = (offset - 1 + limit).min(total);
        if shown_end < total {
            out.push_str(&format!(
                "[{} more matching lines; use offset={}]\n",
                total - shown_end,
                shown_end + 1
            ));
        }
        if out.is_empty() {
            out = "(no matching lines)".into();
        }
        ToolOutcome::ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_from_a_tight_loop_are_distinct() {
        let store = OutputStore::default();
        let sid = SessionId::new();
        let ids: std::collections::HashSet<String> = (0..1000)
            .map(|i| store.put(sid, format!("out {i}")).unwrap())
            .collect();
        assert_eq!(ids.len(), 1000);
        assert!(ids.iter().all(|i| i.len() == 16));
    }

    fn ctx(store: &Arc<OutputStore>, sid: SessionId) -> ToolCtx {
        ToolCtx {
            outputs: Some(store.session(sid)),
            ..ToolCtx::new("/")
        }
    }

    #[tokio::test]
    async fn another_session_cannot_read_an_output() {
        let store = Arc::new(OutputStore::default());
        let (a, b) = (SessionId::new(), SessionId::new());
        let id = store.session(a).put("secret output of A".into()).unwrap();
        let out = ToolOutput.run(&ctx(&store, b), json!({"id": id})).await;
        assert!(!out.ok, "session B read A's output: {}", out.output);
        assert!(out.output.contains("no stored output"), "{}", out.output);
        let out = ToolOutput.run(&ctx(&store, a), json!({"id": id})).await;
        assert!(
            out.ok && out.output.contains("secret output of A"),
            "{}",
            out.output
        );
    }

    #[test]
    fn a_busy_session_evicts_only_its_own_outputs() {
        let store = OutputStore::default();
        let (a, b) = (SessionId::new(), SessionId::new());
        let b_id = store.put(b, "b's".into()).unwrap();
        let a_first = store.put(a, "a's first".into()).unwrap();
        for i in 0..KEEP {
            store.put(a, format!("a {i}")).unwrap();
        }
        assert_eq!(store.get(a, &a_first), None, "a's oldest goes past KEEP");
        assert_eq!(store.get(b, &b_id).as_deref(), Some("b's"));
    }

    #[test]
    fn the_global_bound_takes_from_the_heaviest_session() {
        let store = OutputStore::default();
        let small = SessionId::new();
        let small_id = store.put(small, "small".into()).unwrap();
        // A heavy session over the byte bound, then many sessions that are
        // each under their own cap but together over the entry bound.
        let big = "x".repeat(MAX_STORED + 200_000);
        let heavy = SessionId::new();
        for _ in 0..KEEP {
            store.put(heavy, big.clone()).unwrap();
        }
        for _ in 0..(KEEP_TOTAL / KEEP + 2) {
            let s = SessionId::new();
            for _ in 0..KEEP {
                store.put(s, "y".into()).unwrap();
            }
        }
        {
            let inner = store.inner.lock().unwrap();
            assert!(inner.entries <= KEEP_TOTAL && inner.bytes <= MAX_TOTAL_BYTES);
        }
        assert_eq!(store.get(small, &small_id).as_deref(), Some("small"));
    }
}
