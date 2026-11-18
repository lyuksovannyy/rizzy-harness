//! The project task board for agents (ADOPTION_PLAN X.32, PLAN.md T78,
//! docs/projects.md "Task board"). The board is on the master and the
//! master never runs tools: the node's `board` and `board_edit` tools file
//! a [`BoardRequest`] for the session they run in over the link, and the
//! master takes the project from that session's own row, the person from
//! its owner, and answers with a [`BoardAnswer`]. Nothing in a request
//! names a user, a workspace, a project or a chat.
//!
//! Two tools, not one with an action: the approval path classes a call by
//! its tool's name and risk (plan mode refuses by name, Ask asks by risk,
//! the read-only allow-list is of names), so `board` (list) is read-only
//! and `board_edit` (add, move, note) changes shared state.
//!
//! Agents add and append; they never delete a card or overwrite a title or
//! notes a person wrote.

use serde::{Deserialize, Serialize};

/// A card's notes as `list` shows them, at most (characters).
pub const LIST_NOTES_CHARS: usize = 500;
/// Every title and note of one `list` answer together, at most
/// (characters); cards past it are counted, not shown.
pub const LIST_TOTAL_CHARS: usize = 12_000;
/// Cards one `list` answer shows, at most.
pub const LIST_CARDS: usize = 100;
/// A card title (the board's own limit).
pub const TITLE_MAX: usize = 200;
/// A card's whole notes (the board's own limit). An append that would pass
/// it is refused, never cut.
pub const NOTES_MAX: usize = 4000;
/// One appended note, at most (characters, before its mark).
pub const NOTE_MAX: usize = 1000;
/// Writes (`add`, `move`, `note`) one turn may make, its sub-agents'
/// included.
pub const WRITES_PER_TURN: u32 = 10;
/// `list` calls one turn may make, its sub-agents' included.
pub const READS_PER_TURN: u32 = 20;
/// A column or card id as a request carries it, at most.
pub const ID_MAX: usize = 64;
/// A column named in a request, at most (an id or a name).
pub const COLUMN_MAX: usize = 64;

/// One call, as the model asked for it. Only what to do: the board is the
/// master's to find from the session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum BoardRequest {
    /// The open cards (cards of columns not marked done), with ids and
    /// their notes cut; every column by name and id.
    List {
        /// Only this column (its id or name).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        column: Option<String>,
    },
    /// A new card, at the bottom of `column` (default: the first column
    /// not marked done).
    Add {
        title: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        notes: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        column: Option<String>,
    },
    /// A card to `column` (its id or name), at `position` there (0 is the
    /// top; default the bottom).
    Move {
        card_id: String,
        column: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<u32>,
    },
    /// A line appended to a card's notes, marked with the agent.
    Note { card_id: String, text: String },
}

impl BoardRequest {
    /// Whether this changes the board.
    pub fn writes(&self) -> bool {
        !matches!(self, BoardRequest::List { .. })
    }

    /// The tool a call of this request is made with.
    pub fn tool(&self) -> &'static str {
        if self.writes() { EDIT_TOOL } else { READ_TOOL }
    }
}

/// The read tool's name.
pub const READ_TOOL: &str = "board";
/// The write tool's name.
pub const EDIT_TOOL: &str = "board_edit";

/// A card's linked chat, as the asking chat's owner may see it (T10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CardChat {
    /// The chat this call runs in (or its parent, for a sub-agent).
    This,
    /// Another chat of the same owner, by its title.
    Own { title: String },
    /// A chat of another member: no title, no id.
    Private,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardView {
    pub card_id: String,
    pub title: String,
    /// Cut at [`LIST_NOTES_CHARS`].
    #[serde(default)]
    pub notes: String,
    /// Characters of the notes left out.
    #[serde(default)]
    pub notes_cut: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat: Option<CardChat>,
    /// The agent that made the card, by name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnView {
    pub column_id: String,
    pub name: String,
    pub done: bool,
    /// Cards here; a done column's are counted only.
    #[serde(default)]
    pub cards: Vec<CardView>,
    /// Cards here not shown (a done column's, or past the caps).
    #[serde(default)]
    pub hidden: usize,
}

/// What a call gets back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BoardAnswer {
    /// `list`: the project's name and its columns.
    Board {
        project: String,
        columns: Vec<ColumnView>,
    },
    /// A write that was made: which card, and a line saying what happened.
    Done { card_id: String, said: String },
}

/// Where a session's board calls go: the node's link to the master, bound
/// to the session the call runs for.
#[async_trait::async_trait]
pub trait BoardSource: Send + Sync {
    async fn board(&self, r: BoardRequest) -> Result<BoardAnswer, String>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A request carries what to do and nothing that names a place: a
    /// project, workspace, user or chat field does not parse.
    #[test]
    fn requests_name_no_place() {
        let r: BoardRequest =
            serde_json::from_value(json!({"action": "add", "title": "Fix login"})).unwrap();
        assert_eq!(
            r,
            BoardRequest::Add {
                title: "Fix login".into(),
                notes: String::new(),
                column: None
            }
        );
        assert!(r.writes() && r.tool() == EDIT_TOOL);
        for extra in [
            "project_id",
            "workspace_id",
            "user_id",
            "session_id",
            "owner",
        ] {
            for v in [
                json!({"action": "list", extra: "x"}),
                json!({"action": "add", "title": "t", extra: "x"}),
                json!({"action": "note", "card_id": "c", "text": "t", extra: "x"}),
            ] {
                assert!(
                    serde_json::from_value::<BoardRequest>(v).is_err(),
                    "{extra}"
                );
            }
        }
        assert!(
            serde_json::from_value::<BoardRequest>(json!({"action": "delete", "card_id": "c"}))
                .is_err()
        );
        let l: BoardRequest = serde_json::from_value(json!({"action": "list"})).unwrap();
        assert!(!l.writes() && l.tool() == READ_TOOL);
    }
}
