//! The agent's own task list (the `todo` tool): a short plan the agent
//! replaces whole with each call. It lives in the chat's log as
//! `todo_updated` events, the latest one being the list, so it survives
//! compaction, a node restart and a reload; the UI folds it like the rest.
//! It changes nothing outside the chat. Its items are the model's text:
//! checked to one line each here, and quoted wherever the harness sends
//! them back to a model.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::event::{EventKind, OUTPUT_QUOTE, RawEvent, quoted_line};

/// Items one list may hold.
pub const MAX_ITEMS: usize = 50;
/// Characters (not bytes) of one item.
pub const MAX_CHARS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Done,
}

impl TodoStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Done => "done",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoItem {
    /// One line, trimmed, at most [`MAX_CHARS`] characters.
    pub content: String,
    pub status: TodoStatus,
}

/// The list a `todo` call sets, or why it is refused (the words go back to
/// the model as the call's result): `{"items": [{"content", "status"}]}`,
/// at most [`MAX_ITEMS`] items of one line and at most [`MAX_CHARS`]
/// characters each, at most one `in_progress`. `[]` clears the list.
pub fn parse(input: &Value) -> Result<Vec<TodoItem>, String> {
    let Some(items) = input.get("items").and_then(Value::as_array) else {
        return Err(
            "`items` must be the whole list: an array of {\"content\", \"status\"} \
                    objects ([] clears it)"
                .into(),
        );
    };
    if items.len() > MAX_ITEMS {
        return Err(format!(
            "{} items: a list holds at most {MAX_ITEMS}; merge small steps or drop finished ones",
            items.len()
        ));
    }
    let mut out = Vec::with_capacity(items.len());
    let mut working: Option<usize> = None;
    for (i, it) in items.iter().enumerate() {
        let n = i + 1;
        let content = it
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("item {n}: `content` must be a string"))?
            .trim();
        if content.is_empty() {
            return Err(format!("item {n}: `content` is empty"));
        }
        if content
            .chars()
            .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
        {
            return Err(format!(
                "item {n}: `content` must be one line (no line breaks, tabs or control characters)"
            ));
        }
        let chars = content.chars().count();
        if chars > MAX_CHARS {
            return Err(format!(
                "item {n}: `content` is {chars} characters; at most {MAX_CHARS} (one short line)"
            ));
        }
        let status = match it.get("status").and_then(Value::as_str) {
            Some("pending") => TodoStatus::Pending,
            Some("in_progress") => TodoStatus::InProgress,
            Some("done") => TodoStatus::Done,
            _ => {
                return Err(format!(
                    "item {n}: `status` must be \"pending\", \"in_progress\" or \"done\""
                ));
            }
        };
        if status == TodoStatus::InProgress {
            if let Some(w) = working {
                return Err(format!(
                    "items {w} and {n} are both in_progress: at most one item is in progress at \
                     a time; mark the other pending or done"
                ));
            }
            working = Some(n);
        }
        out.push(TodoItem {
            content: content.to_string(),
            status,
        });
    }
    Ok(out)
}

/// `3 items: 1 done, 1 in progress, 1 pending` (the call's result).
pub fn tally(items: &[TodoItem]) -> String {
    let count = |s: TodoStatus| items.iter().filter(|i| i.status == s).count();
    format!(
        "{} item{}: {} done, {} in progress, {} pending",
        items.len(),
        if items.len() == 1 { "" } else { "s" },
        count(TodoStatus::Done),
        count(TodoStatus::InProgress),
        count(TodoStatus::Pending),
    )
}

/// The list an item a line, `[in_progress] Write the tests`, each one line
/// even for an event this node did not check (`quoted_line`).
pub fn lines(items: &[TodoItem]) -> String {
    items
        .iter()
        .map(|i| format!("[{}] {}", i.status.as_str(), quoted_line(&i.content)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// [`lines`] as the harness sends them to a model: each behind
/// [`OUTPUT_QUOTE`] (`| [in_progress] Write the tests`).
pub fn quoted(items: &[TodoItem]) -> String {
    lines(items)
        .split('\n')
        .map(|l| format!("{OUTPUT_QUOTE}{l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The list as it stood at `seq` (inclusive) in `history`, if one is set
/// and not empty.
pub fn as_of(history: &[RawEvent], seq: u64) -> Option<Vec<TodoItem>> {
    latest(&history[..history.partition_point(|e| e.seq <= seq)]).filter(|l| !l.is_empty())
}

/// The list `events` leave: the latest `todo_updated`'s, if any.
pub fn latest(events: &[RawEvent]) -> Option<Vec<TodoItem>> {
    events
        .iter()
        .rev()
        .filter(|e| e.kind == "todo_updated")
        .find_map(|e| match e.typed() {
            Ok(EventKind::TodoUpdated(t)) => Some(t.items),
            _ => None,
        })
}
