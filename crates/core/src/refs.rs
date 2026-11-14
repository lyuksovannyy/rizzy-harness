//! Citation labels for web results (`[S1]`, `[S2]`, ...).
//!
//! A label names one URL for the whole session: the book is rebuilt from the
//! log at the start of every turn, so a page found three turns ago keeps its
//! number and a new one gets the next. A delegated sub-task shares its
//! parent's book (`TurnExtra::refs`), so the labels in the answer it hands
//! back mean the same pages in the parent. The UI resolves the model's
//! `[S<n>]` through the sources recorded on `tool_call_finished`, never
//! through text the model wrote.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::Deserialize;

use crate::RawEvent;
use crate::event::Source;

/// One book shared by every call of a turn and by its sub-tasks.
pub type SharedRefs = Arc<Mutex<RefBook>>;

#[derive(Debug, Default)]
pub struct RefBook {
    by_url: HashMap<String, u32>,
    next: u32,
}

impl RefBook {
    /// The labels already given in this session's log.
    pub fn seed(history: &[RawEvent]) -> Self {
        let mut book = Self::default();
        for s in sources_in(history) {
            if let Some(n) = s.number() {
                book.by_url.insert(s.url, n);
                book.next = book.next.max(n);
            }
        }
        book
    }

    /// The label for `url`: the one it already has, or the next free one.
    pub fn label(&mut self, url: &str) -> String {
        let n = match self.by_url.get(url) {
            Some(n) => *n,
            None => {
                self.next += 1;
                self.by_url.insert(url.to_string(), self.next);
                self.next
            }
        };
        format!("S{n}")
    }
}

/// Every source recorded on the log's `tool_call_finished` events, in log
/// order, once per label (the latest copy wins).
pub fn sources_in(history: &[RawEvent]) -> Vec<Source> {
    let mut out: Vec<Source> = Vec::new();
    for e in history.iter().filter(|e| e.kind == "tool_call_finished") {
        // Only the one field: deserializing whole payloads of a long log
        // every turn would copy every tool output.
        let Some(v) = e.payload.get("sources") else {
            continue;
        };
        let Ok(sources) = Vec::<Source>::deserialize(v) else {
            continue;
        };
        for s in sources {
            match out.iter_mut().find(|o| o.id == s.id) {
                Some(o) => *o = s,
                None => out.push(s),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SessionId;
    use crate::event::{EventKind, ToolCallFinished};

    fn finished(seq: u64, sources: &[(&str, &str)]) -> RawEvent {
        let (kind, payload) = EventKind::ToolCallFinished(ToolCallFinished {
            seen: None,
            call_id: format!("c{seq}"),
            ok: true,
            output: String::new(),
            exit_code: None,
            truncated: false,
            sources: sources
                .iter()
                .map(|(id, url)| Source {
                    id: id.to_string(),
                    url: url.to_string(),
                    title: format!("t{seq}"),
                })
                .collect(),
            note: None,
        })
        .into_parts();
        RawEvent {
            session_id: SessionId::new(),
            seq,
            ts_ms: 0,
            kind,
            payload,
        }
    }

    #[test]
    fn empty_book_starts_at_one_and_reuses_labels() {
        let mut b = RefBook::default();
        assert_eq!(b.label("https://a.example/"), "S1");
        assert_eq!(b.label("https://b.example/"), "S2");
        assert_eq!(b.label("https://a.example/"), "S1");
    }

    #[test]
    fn seeded_book_continues_after_the_highest_label() {
        let history = vec![
            finished(
                1,
                &[("S1", "https://a.example/"), ("S2", "https://b.example/")],
            ),
            finished(2, &[]),
            finished(
                3,
                &[
                    ("S5", "https://c.example/"),
                    ("bogus", "https://d.example/"),
                ],
            ),
        ];
        let mut b = RefBook::seed(&history);
        assert_eq!(b.label("https://b.example/"), "S2");
        assert_eq!(b.label("https://c.example/"), "S5");
        assert_eq!(b.label("https://new.example/"), "S6");
        // A malformed label is not a label: its URL gets a new one.
        assert_eq!(b.label("https://d.example/"), "S7");
    }

    #[test]
    fn sources_come_once_per_label_in_log_order() {
        let history = vec![
            finished(
                1,
                &[("S1", "https://a.example/"), ("S2", "https://b.example/")],
            ),
            finished(
                2,
                &[("S3", "https://c.example/"), ("S1", "https://a.example/")],
            ),
        ];
        let got: Vec<(String, String)> = sources_in(&history)
            .into_iter()
            .map(|s| (s.id, s.title))
            .collect();
        assert_eq!(
            got,
            [
                ("S1".to_string(), "t2".to_string()),
                ("S2".to_string(), "t1".to_string()),
                ("S3".to_string(), "t2".to_string()),
            ]
        );
    }
}
