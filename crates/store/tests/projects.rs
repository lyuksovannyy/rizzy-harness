//! Projects storage: projects, chat links, the board and saved templates.

use harness_core::{RawEvent, SessionId};
use harness_store::{
    CardPatch, EarlierSource, NewCard, NewColumn, NewProject, NewUser, ProjectPatch, Role, Store,
    StoreError, TemplateRow,
};
use serde_json::json;

/// A store with one account, "u1", and a team workspace it owns.
fn store() -> (Store, String) {
    let s = Store::open_in_memory().unwrap();
    s.create_user(&NewUser {
        user_id: "u1".into(),
        username: "u1".into(),
        password_hash: "$argon2id$x".into(),
        role: Role::Member,
        exec_ceiling: harness_core::policy::ExecLevel::None,
        must_change_password: false,
    })
    .unwrap();
    let w = s.create_workspace("Team", "u1").unwrap().workspace_id;
    (s, w)
}

fn cols(names: &[&str]) -> Vec<NewColumn> {
    names
        .iter()
        .map(|n| NewColumn {
            name: n.to_string(),
            done: *n == "Done",
        })
        .collect()
}

fn new_in(id: &str, ws: &str) -> NewProject {
    NewProject {
        project_id: id.into(),
        workspace_id: ws.into(),
        name: format!("Project {id}"),
        description: String::new(),
        icon: "folder".into(),
        accent: "blue".into(),
        instructions: String::new(),
        settings: "{}".into(),
        created_by: Some("u1".into()),
        columns: cols(&["To do", "In progress", "Done"]),
        source: None,
    }
}

fn project(s: &Store, id: &str, ws: &str) {
    s.create_project(&new_in(id, ws), 10).unwrap();
}

fn chat(s: &Store, sid: SessionId) {
    chat_of(s, sid, None);
}

fn chat_of(s: &Store, sid: SessionId, owner: Option<&str>) {
    s.append(
        "n1",
        &RawEvent {
            session_id: sid,
            seq: 1,
            ts_ms: 1,
            kind: "session_created".into(),
            payload: json!({"title": "t", "cwd": "", "model": "m"}),
        },
    )
    .unwrap();
    if let Some(o) = owner {
        s.claim_session(sid, Some(o), None).unwrap();
    }
}

fn card(s: &Store, p: &str, column: &str, title: &str) -> String {
    let id = harness_core::new_id();
    assert!(
        s.add_card(
            p,
            &NewCard {
                card_id: id.clone(),
                column_id: column.into(),
                title: title.into(),
                notes: String::new(),
                session_id: None,
                created_by: None,
                agent: None,
            },
            100,
        )
        .unwrap()
    );
    id
}

/// Card titles per column, in board order.
fn titles(s: &Store, p: &str) -> Vec<Vec<String>> {
    let (cols, cards) = s.board(p).unwrap();
    cols.iter()
        .map(|c| {
            let mut v: Vec<_> = cards
                .iter()
                .filter(|k| k.column_id == c.column_id)
                .collect();
            v.sort_by_key(|k| k.position);
            v.into_iter().map(|k| k.title.clone()).collect()
        })
        .collect()
}

#[test]
fn projects_are_listed_per_workspace_and_capped() {
    let (s, w1) = store();
    let w2 = s.create_workspace("Other", "u1").unwrap().workspace_id;
    let w3 = s.personal_workspace("u1").unwrap();
    project(&s, "b", &w1);
    project(&s, "a", &w1);
    project(&s, "c", &w2);
    let names: Vec<_> = s
        .projects(Some(&w1))
        .unwrap()
        .into_iter()
        .map(|p| p.project_id)
        .collect();
    assert_eq!(names, ["a", "b"], "by name");
    assert_eq!(s.projects(None).unwrap().len(), 3);

    let mut p = NewProject {
        project_id: "x".into(),
        workspace_id: w3,
        name: "x".into(),
        description: String::new(),
        icon: "folder".into(),
        accent: "blue".into(),
        instructions: String::new(),
        settings: "{}".into(),
        created_by: None,
        columns: vec![],
        source: None,
    };
    s.create_project(&p, 1).unwrap();
    p.project_id = "y".into();
    assert!(matches!(
        s.create_project(&p, 1),
        Err(StoreError::Refused(_))
    ));
}

#[test]
fn update_changes_only_given_fields() {
    let (s, w) = store();
    project(&s, "p", &w);
    let before = s.project("p").unwrap().unwrap();
    let after = s
        .update_project(
            "p",
            &ProjectPatch {
                name: Some("Renamed".into()),
                instructions: Some("Use metric units.".into()),
                ..Default::default()
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(after.name, "Renamed");
    assert_eq!(after.instructions, "Use metric units.");
    assert_eq!(after.icon, before.icon);
    assert!(after.updated_ms >= before.updated_ms);
    assert!(
        s.update_project("nope", &ProjectPatch::default())
            .unwrap()
            .is_none()
    );
}

#[test]
fn chats_join_and_leave_and_deleting_keeps_them() {
    let (s, w) = store();
    project(&s, "p", &w);
    project(&s, "q", &w);
    let (a, b) = (SessionId::new(), SessionId::new());
    chat(&s, a);
    chat(&s, b);
    s.set_session_project(a, Some("p")).unwrap();
    s.set_session_project(b, Some("p")).unwrap();
    // A chat is in one project at most: moving replaces the link.
    s.set_session_project(b, Some("q")).unwrap();
    let link = s.session_project(b).unwrap().unwrap();
    assert_eq!(
        (link.project_id.as_str(), link.workspace_id.as_str()),
        ("q", w.as_str())
    );
    assert_eq!(s.project_links(None).unwrap().len(), 2);
    s.set_session_project(b, None).unwrap();
    assert!(s.session_project(b).unwrap().is_none());

    // Deleting the project drops its links; the chat stays.
    assert!(s.delete_project("p").unwrap());
    assert!(s.session_project(a).unwrap().is_none());
    assert!(s.session(a).unwrap().is_some());
    assert!(s.board("p").unwrap().0.is_empty(), "board went with it");
}

#[test]
fn deleting_a_chat_drops_its_link_and_card_links() {
    let (s, w) = store();
    project(&s, "p", &w);
    let sid = SessionId::new();
    chat(&s, sid);
    s.set_session_project(sid, Some("p")).unwrap();
    let (cols, _) = s.board("p").unwrap();
    let c = card(&s, "p", &cols[0].column_id, "linked");
    s.update_card(
        "p",
        &c,
        &CardPatch {
            session_id: Some(Some(sid.to_string())),
            ..Default::default()
        },
    )
    .unwrap();
    s.delete_session(sid, true).unwrap();
    assert!(s.project_links(None).unwrap().is_empty());
    assert_eq!(s.card("p", &c).unwrap().unwrap().session_id, None);
}

#[test]
fn cards_move_within_and_across_columns() {
    let (s, w) = store();
    project(&s, "p", &w);
    let (cols, _) = s.board("p").unwrap();
    let (todo, doing) = (cols[0].column_id.clone(), cols[1].column_id.clone());
    let a = card(&s, "p", &todo, "a");
    card(&s, "p", &todo, "b");
    card(&s, "p", &todo, "c");
    assert_eq!(titles(&s, "p"), [vec!["a", "b", "c"], vec![], vec![]]);

    // To the top of its own column.
    let c = s
        .board("p")
        .unwrap()
        .1
        .into_iter()
        .find(|k| k.title == "c")
        .unwrap();
    s.update_card(
        "p",
        &c.card_id,
        &CardPatch {
            position: Some(0),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(titles(&s, "p")[0], ["c", "a", "b"]);

    // Across: the old column closes the gap, a big index clamps to the end.
    s.update_card(
        "p",
        &a,
        &CardPatch {
            column_id: Some(doing.clone()),
            position: Some(99),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(titles(&s, "p"), [vec!["c", "b"], vec!["a"], vec![]]);
    let positions: Vec<_> = s
        .board("p")
        .unwrap()
        .1
        .into_iter()
        .filter(|k| k.column_id == todo)
        .map(|k| k.position)
        .collect();
    assert_eq!(positions, [0, 1]);

    // A column of another project is refused (false), and nothing moves.
    project(&s, "q", &w);
    let other = s.board("q").unwrap().0[0].column_id.clone();
    let moved = s
        .update_card(
            "p",
            &a,
            &CardPatch {
                column_id: Some(other),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(!moved);
    assert_eq!(titles(&s, "p")[1], ["a"]);
    // So is a card of another project.
    assert!(!s.delete_card("q", &a).unwrap());
    assert!(s.delete_card("p", &a).unwrap());
}

#[test]
fn columns_rules() {
    let (s, w) = store();
    project(&s, "p", &w);
    let (cols, _) = s.board("p").unwrap();
    let first = cols[0].column_id.clone();
    card(&s, "p", &first, "x");
    assert!(matches!(
        s.delete_column("p", &first),
        Err(StoreError::Refused(_))
    ));
    // Move the last column to the front, rename it.
    let last = cols[2].column_id.clone();
    assert!(
        s.update_column("p", &last, Some("Shipped"), None, Some(0))
            .unwrap()
    );
    let names: Vec<_> = s
        .board("p")
        .unwrap()
        .0
        .into_iter()
        .map(|c| c.name)
        .collect();
    assert_eq!(names, ["Shipped", "To do", "In progress"]);
    assert!(s.delete_column("p", &last).unwrap());
    assert!(!s.update_column("q", &first, Some("x"), None, None).unwrap());
    let id = s
        .add_column(
            "p",
            &NewColumn {
                name: "Later".into(),
                done: false,
            },
            3,
        )
        .unwrap();
    assert_eq!(s.board("p").unwrap().0.last().unwrap().column_id, id);
    assert!(matches!(
        s.add_column(
            "p",
            &NewColumn {
                name: "One too many".into(),
                done: false
            },
            3
        ),
        Err(StoreError::Refused(_))
    ));
}

#[test]
fn a_template_replaces_only_an_empty_board() {
    let (s, w) = store();
    project(&s, "p", &w);
    s.reset_columns("p", &cols(&["Ideas", "Done"])).unwrap();
    let names: Vec<_> = s
        .board("p")
        .unwrap()
        .0
        .into_iter()
        .map(|c| c.name)
        .collect();
    assert_eq!(names, ["Ideas", "Done"]);
    let first = s.board("p").unwrap().0[0].column_id.clone();
    card(&s, "p", &first, "keep me");
    assert!(matches!(
        s.reset_columns("p", &cols(&["A"])),
        Err(StoreError::Refused(_))
    ));
    assert_eq!(titles(&s, "p")[0], ["keep me"]);
}

#[test]
fn templates_per_workspace() {
    let (s, w) = store();
    let v = s.create_workspace("Other", "u1").unwrap().workspace_id;
    let t = |id: &str, ws: &str| TemplateRow {
        template_id: id.into(),
        workspace_id: ws.into(),
        name: format!("T {id}"),
        columns: r#"[{"name":"A","done":false}]"#.into(),
        created_by: None,
        created_ms: 1,
    };
    s.create_board_template(&t("1", &w), 2).unwrap();
    s.create_board_template(&t("2", &w), 2).unwrap();
    s.create_board_template(&t("3", &v), 2).unwrap();
    assert!(matches!(
        s.create_board_template(&t("4", &w), 2),
        Err(StoreError::Refused(_))
    ));
    assert_eq!(s.board_templates(&w).unwrap().len(), 2);
    assert_eq!(s.board_template("3").unwrap().unwrap().workspace_id, v);
    assert!(s.delete_board_template("1").unwrap());
    assert!(!s.delete_board_template("1").unwrap());
}

#[test]
fn deleting_a_workspace_takes_its_projects_and_templates() {
    let (s, w) = store();
    let home = s.personal_workspace("u1").unwrap();
    project(&s, "p", &w);
    project(&s, "mine", &home);
    let sid = SessionId::new();
    chat_of(&s, sid, Some("u1"));
    assert!(s.claim_session(sid, None, Some(&w)).unwrap());
    assert!(s.link_new_session(sid, "p").unwrap());
    let first = s.board("p").unwrap().0[0].column_id.clone();
    card(&s, "p", &first, "x");
    s.create_board_template(
        &TemplateRow {
            template_id: "t".into(),
            workspace_id: w.clone(),
            name: "T".into(),
            columns: "[]".into(),
            created_by: None,
            created_ms: 1,
        },
        5,
    )
    .unwrap();
    assert!(s.delete_workspace(&w).unwrap());
    assert!(s.project("p").unwrap().is_none());
    assert!(s.board("p").unwrap().1.is_empty(), "cards went too");
    assert!(s.project_links(None).unwrap().is_empty());
    assert!(s.board_template("t").unwrap().is_none());
    // The chat stays, back in Personal; Personal's project stays.
    assert_eq!(s.session(sid).unwrap().unwrap().workspace_id, Some(home));
    assert!(s.project("mine").unwrap().is_some());
    // A project needs an existing workspace.
    assert!(s.create_project(&new_in("x", "nope"), 10).is_err());
}

#[test]
fn a_new_chat_joins_only_a_project_of_its_workspace() {
    let (s, w) = store();
    let home = s.personal_workspace("u1").unwrap();
    project(&s, "team", &w);
    project(&s, "mine", &home);
    // Stamped into the team: the team's project, not Personal's.
    let a = SessionId::new();
    chat_of(&s, a, Some("u1"));
    s.claim_session(a, None, Some(&w)).unwrap();
    assert!(!s.link_new_session(a, "mine").unwrap());
    assert!(s.link_new_session(a, "team").unwrap());
    // Left in Personal (no mapping): Personal's project only.
    let b = SessionId::new();
    chat_of(&s, b, Some("u1"));
    assert!(!s.link_new_session(b, "team").unwrap());
    assert!(s.link_new_session(b, "mine").unwrap());
    assert!(!s.link_new_session(b, "gone").unwrap());
    assert_eq!(s.project_links(None).unwrap().len(), 2);
}

/// A chat of `owner` in team workspace `w`, created at `ts`, its later
/// events one millisecond apart; in project `p` when given. `created` adds
/// to the `session_created` payload.
fn chat_with(
    s: &Store,
    owner: &str,
    w: &str,
    p: Option<&str>,
    ts: i64,
    created: serde_json::Value,
    events: &[(&str, serde_json::Value)],
) -> SessionId {
    let sid = SessionId::new();
    let mut payload = json!({"title": format!("chat {ts}"), "cwd": "", "model": "m"});
    for (k, v) in created.as_object().unwrap() {
        payload[k] = v.clone();
    }
    s.append(
        "n1",
        &RawEvent {
            session_id: sid,
            seq: 1,
            ts_ms: ts,
            kind: "session_created".into(),
            payload,
        },
    )
    .unwrap();
    s.claim_session(sid, Some(owner), Some(w)).unwrap();
    if let Some(p) = p {
        s.set_session_project(sid, Some(p)).unwrap();
    }
    for (i, (kind, payload)) in events.iter().enumerate() {
        s.append(
            "n1",
            &RawEvent {
                session_id: sid,
                seq: i as u64 + 2,
                ts_ms: ts + i as i64 + 1,
                kind: kind.to_string(),
                payload: payload.clone(),
            },
        )
        .unwrap();
    }
    sid
}

fn said(text: &str) -> (&'static str, serde_json::Value) {
    (
        "assistant_message",
        json!({"turn_id": "t", "message_id": harness_core::new_id(), "text": text}),
    )
}

/// What a new project chat's first turn reads of earlier chats: only its
/// owner's own chats in that project, newest first, never incognito or
/// sub-agent chats nor the chat itself; each with its latest summary, else
/// its last assistant message with text; a chat with neither is left out.
#[test]
fn earlier_chats_are_the_owners_own_in_the_project_newest_first() {
    let (s, w) = store();
    s.create_user(&NewUser {
        user_id: "u2".into(),
        username: "u2".into(),
        password_hash: "$argon2id$x".into(),
        role: Role::Member,
        exec_ceiling: harness_core::policy::ExecLevel::None,
        must_change_password: false,
    })
    .unwrap();
    assert!(s.add_member(&w, "u2", "member").unwrap());
    project(&s, "p", &w);
    project(&s, "q", &w);
    let none = json!({});
    let summary = (
        "history_summarized",
        json!({"turn_id": "t", "through_seq": 2, "text": "Goal: alpha"}),
    );
    // Summarised, then talked on: the summary is what it gives.
    let a = chat_with(
        &s,
        "u1",
        &w,
        Some("p"),
        100,
        none.clone(),
        &[said("alpha reply"), summary, said("alpha later")],
    );
    // No summary: the last assistant message with text (a tool-call step
    // with none comes after it).
    let b = chat_with(
        &s,
        "u1",
        &w,
        Some("p"),
        200,
        none.clone(),
        &[said("bravo reply"), said("")],
    );
    // Another member's chat in the same project.
    chat_with(
        &s,
        "u2",
        &w,
        Some("p"),
        300,
        none.clone(),
        &[said("charlie secret")],
    );
    // Incognito, a sub-agent of `a`, no reply yet, another project.
    chat_with(
        &s,
        "u1",
        &w,
        Some("p"),
        400,
        json!({"incognito": true}),
        &[said("delta hidden")],
    );
    let sub = chat_with(
        &s,
        "u1",
        &w,
        Some("p"),
        500,
        json!({"parent_session": a.to_string()}),
        &[said("echo delegate")],
    );
    assert_eq!(
        s.session(sub).unwrap().unwrap().parent_id,
        Some(a.to_string())
    );
    chat_with(&s, "u1", &w, Some("p"), 600, none.clone(), &[]);
    chat_with(
        &s,
        "u1",
        &w,
        Some("q"),
        700,
        none.clone(),
        &[said("foxtrot elsewhere")],
    );
    let me = chat_with(
        &s,
        "u1",
        &w,
        Some("p"),
        800,
        none.clone(),
        &[said("golf myself")],
    );

    let got = s.earlier_chats("p", "u1", me, 5).unwrap();
    let ids: Vec<SessionId> = got.iter().map(|c| c.session_id).collect();
    assert_eq!(ids, vec![b, a], "{got:?}");
    assert_eq!(got[0].text.as_deref(), Some("bravo reply"));
    assert_eq!(got[0].source, EarlierSource::LastReply);
    assert!(got[0].facts.is_empty(), "nothing to hand off");
    assert_eq!(got[0].title, "chat 200");
    assert_eq!(got[0].updated_ms, 202);
    assert_eq!(got[1].text.as_deref(), Some("Goal: alpha"));
    assert_eq!(got[1].source, EarlierSource::Summary);
    // The limit counts chats with text.
    let got = s.earlier_chats("p", "u1", me, 1).unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].session_id, b);
    // The other member reads only their own.
    let theirs = s.earlier_chats("p", "u2", me, 5).unwrap();
    assert_eq!(theirs.len(), 1);
    assert_eq!(theirs[0].text.as_deref(), Some("charlie secret"));
}

/// A hand-off note (X.16) is preferred over the summary and the last
/// reply, with the facts its log shows; a note no model wrote leaves the
/// summary in place but the facts still go; a note older than the chat's
/// last turn is marked stale; a chat with only facts is still listed.
#[test]
fn earlier_chats_prefer_the_hand_off_note_and_carry_the_facts() {
    let (s, w) = store();
    project(&s, "p", &w);
    let none = json!({});
    let ended = ("turn_ended", json!({"turn_id": "t", "reason": "completed"}));
    let started = ("turn_started", json!({"turn_id": "t"}));
    let asked = (
        "user_message",
        json!({"turn_id": "t", "text": "add a --json flag"}),
    );
    let changed = (
        "checkpoint",
        json!({"turn_id": "t", "phase": "after", "git_ref": "r", "files": ["src/cli.rs"]}),
    );
    let todo = (
        "todo_updated",
        json!({"turn_id": "t", "call_id": "c", "items": [{"content": "Docs", "status": "pending"}]}),
    );
    let summary = (
        "history_summarized",
        json!({"turn_id": "t", "through_seq": 2, "text": "Goal: json"}),
    );
    let noted = chat_with(
        &s,
        "u1",
        &w,
        Some("p"),
        100,
        none.clone(),
        &[
            started.clone(),
            asked.clone(),
            changed.clone(),
            said("Done, tests pass."),
            summary.clone(),
            ended.clone(),
            (
                "handoff_written",
                json!({"through_seq": 7, "text": "Status:\nDocs left."}),
            ),
        ],
    );
    let failed = chat_with(
        &s,
        "u1",
        &w,
        Some("p"),
        200,
        none.clone(),
        &[
            started.clone(),
            asked.clone(),
            todo.clone(),
            said("Done."),
            summary.clone(),
            ended.clone(),
            (
                "handoff_written",
                json!({"through_seq": 7, "failed": [{"model": "x::y", "reason": "failed"}]}),
            ),
        ],
    );
    let stale = chat_with(
        &s,
        "u1",
        &w,
        Some("p"),
        300,
        none.clone(),
        &[
            started.clone(),
            asked.clone(),
            changed.clone(),
            ended.clone(),
            (
                "handoff_written",
                json!({"through_seq": 5, "text": "Status:\nold"}),
            ),
            started.clone(),
            ended.clone(),
        ],
    );
    let facts_only = chat_with(
        &s,
        "u1",
        &w,
        Some("p"),
        400,
        none.clone(),
        &[started, asked, changed, ended],
    );
    let me = chat_with(&s, "u1", &w, Some("p"), 900, none, &[]);
    let got = s.earlier_chats("p", "u1", me, 5).unwrap();
    let by = |sid| got.iter().find(|c| c.session_id == sid).unwrap();
    let n = by(noted);
    assert_eq!(n.source, EarlierSource::Note);
    assert_eq!(n.text.as_deref(), Some("Status:\nDocs left."));
    assert!(!n.stale);
    // The note does not make the chat "last active": its turn's end does.
    assert_eq!(n.updated_ms, 106);
    assert_eq!(n.facts[0], "Asked first: add a --json flag");
    assert!(
        n.facts.iter().any(|l| l == "Files changed (1): src/cli.rs"),
        "{:?}",
        n.facts
    );
    let f = by(failed);
    assert_eq!(f.source, EarlierSource::Summary);
    assert!(
        f.facts
            .iter()
            .any(|l| l.starts_with("Task list: 0 of 1 done")),
        "{:?}",
        f.facts
    );
    let st = by(stale);
    assert_eq!((st.source, st.stale), (EarlierSource::Note, true));
    let fo = by(facts_only);
    assert_eq!((fo.source, fo.text.clone()), (EarlierSource::None, None));
    assert!(!fo.facts.is_empty());
}

/// A chat whose owner left the project's workspace is back in Personal:
/// its link no longer applies, so it is not one of the project's chats.
#[test]
fn earlier_chats_follow_the_chats_workspace() {
    let (s, w) = store();
    project(&s, "p", &w);
    let home = s.personal_workspace("u1").unwrap();
    project(&s, "mine", &home);
    let a = chat_with(
        &s,
        "u1",
        &w,
        Some("p"),
        100,
        json!({}),
        &[said("alpha reply")],
    );
    // A stale link to a project of another workspace.
    s.set_session_project(a, Some("mine")).unwrap();
    let me = SessionId::new();
    assert!(s.earlier_chats("mine", "u1", me, 5).unwrap().is_empty());
    s.set_session_project(a, Some("p")).unwrap();
    assert_eq!(s.earlier_chats("p", "u1", me, 5).unwrap().len(), 1);
}

/// An appended note goes after the notes people wrote, never over them;
/// one that would pass the limit is refused and changes nothing (T78).
#[test]
fn notes_are_appended_and_refused_past_the_limit() {
    let (s, w) = store();
    project(&s, "p", &w);
    let (cols, _) = s.board("p").unwrap();
    let k = card(&s, "p", &cols[0].column_id, "Task");
    assert!(s.append_card_note("p", &k, "first", 20).unwrap());
    assert!(s.append_card_note("p", &k, "second", 20).unwrap());
    assert_eq!(s.card("p", &k).unwrap().unwrap().notes, "first\nsecond");
    let e = s
        .append_card_note("p", &k, "this is too long", 20)
        .unwrap_err();
    assert!(matches!(e, harness_store::StoreError::Refused(_)), "{e:?}");
    assert_eq!(s.card("p", &k).unwrap().unwrap().notes, "first\nsecond");
    assert!(
        !s.append_card_note("other", &k, "x", 20).unwrap(),
        "another project's card"
    );
    assert!(!s.append_card_note("p", "nope", "x", 20).unwrap());
}

/// A cloned project keeps where its folder came from (T86): host and
/// owner/name, read back as written; a project made otherwise has none;
/// the table refuses one without the other.
#[test]
fn a_cloned_project_keeps_its_source() {
    let (s, ws) = store();
    let mut p = new_in("c", &ws);
    p.source = Some(("github.com".into(), "ada/harness".into()));
    let row = s.create_project(&p, 10).unwrap();
    assert_eq!(
        row.source,
        Some(("github.com".to_string(), "ada/harness".to_string()))
    );
    assert_eq!(s.project("c").unwrap().unwrap().source, row.source);
    project(&s, "plain", &ws);
    assert_eq!(s.project("plain").unwrap().unwrap().source, None);
}

fn clone_in(id: &str, ws: &str, full: &str) -> harness_store::project_clones::NewClone {
    harness_store::project_clones::NewClone {
        project_id: id.into(),
        workspace_id: ws.into(),
        name: format!("Project {id}"),
        node_id: "n1".into(),
        host: "github.com".into(),
        full_name: full.into(),
        created_by: "u1".into(),
    }
}

/// A running clone (X.27, T86) counts against the project limit, for a
/// clone and for a plain project; a failed one does not. It becomes its
/// project with the same id in one step; it is never a project before.
#[test]
fn running_clones_count_against_the_limit_and_become_their_project() {
    let (s, w) = store();
    project(&s, "p1", &w);
    s.start_clone(&clone_in("c1", &w, "ada/a"), 2).unwrap();
    assert!(s.project("c1").unwrap().is_none(), "a clone is a project");
    assert!(matches!(
        s.start_clone(&clone_in("c2", &w, "ada/b"), 2),
        Err(StoreError::Refused(_))
    ));
    assert!(matches!(
        s.create_project(&new_in("p2", &w), 2),
        Err(StoreError::Refused(_))
    ));
    // The same repository again, in another case: refused while running.
    assert!(matches!(
        s.start_clone(&clone_in("c3", &w, "ADA/A"), 9),
        Err(StoreError::Refused(_))
    ));
    assert!(s.fail_clone("c1", "the node refused").unwrap());
    // Failed: no longer counted, and the repository may be tried again.
    s.start_clone(&clone_in("c4", &w, "ada/a"), 2).unwrap();
    assert_eq!(s.clones(Some(&w)).unwrap().len(), 2);
    let row = s.finish_clone(&new_in("c4", &w), 2).unwrap();
    assert_eq!(row.project_id, "c4");
    assert!(s.clone_of("c4").unwrap().is_none());
    // Ended: finishing again is refused, failing it does nothing.
    assert!(s.finish_clone(&new_in("c4", &w), 9).is_err());
    assert!(!s.fail_clone("c4", "late").unwrap());
    // Dismissed by its starter only, and only once failed.
    assert!(!s.dismiss_clone("c1", "someone else").unwrap());
    assert!(s.dismiss_clone("c1", "u1").unwrap());
    s.start_clone(&clone_in("c5", &w, "ada/c"), 9).unwrap();
    assert!(
        !s.dismiss_clone("c5", "u1").unwrap(),
        "a running clone was dismissed"
    );
    assert_eq!(s.fail_running_clones("restarted").unwrap(), 1);
    assert_eq!(
        s.clone_of("c5").unwrap().unwrap().failed.as_deref(),
        Some("restarted")
    );
}

/// A rewound turn's reply or note is not what another chat of the project
/// is shown (X.46, T90): the earlier chat reads as it does without them,
/// and again with them after the undo.
#[test]
fn earlier_chats_skip_what_a_rewind_took_out() {
    let (s, w) = store();
    project(&s, "p", &w);
    let none = json!({});
    let started = |t: &str| ("turn_started", json!({"turn_id": t}));
    let asked = |t: &str, text: &str| ("user_message", json!({"turn_id": t, "text": text}));
    let ended = |t: &str| ("turn_ended", json!({"turn_id": t, "reason": "completed"}));
    // seqs: 1 created, 2 start t1, 3 msg, 4 reply, 5 end, 6 start t2,
    // 7 msg, 8 reply, 9 summary-free note, 10 end.
    let first = chat_with(
        &s,
        "u1",
        &w,
        Some("p"),
        100,
        none.clone(),
        &[
            started("t1"),
            asked("t1", "first"),
            said("kept reply"),
            ended("t1"),
            started("t2"),
            asked("t2", "second"),
            said("rewound zebrafish reply"),
            ended("t2"),
        ],
    );
    let noted = chat_with(
        &s,
        "u1",
        &w,
        Some("p"),
        200,
        none.clone(),
        &[
            started("t1"),
            asked("t1", "first"),
            said("kept note reply"),
            ended("t1"),
            started("t2"),
            asked("t2", "second"),
            ended("t2"),
            (
                "handoff_written",
                json!({"through_seq": 8, "text": "Status:\nrewound zebrafish note"}),
            ),
        ],
    );
    let me = chat_with(&s, "u1", &w, Some("p"), 900, none, &[]);
    let rewind = |sid: SessionId, seq: u64, msg: u64| {
        s.append(
            "n1",
            &RawEvent {
                session_id: sid,
                seq,
                ts_ms: 1000 + seq as i64,
                kind: "conversation_rewound".into(),
                payload: json!({"message_seq": msg, "turn_id": "t2", "files": false}),
            },
        )
        .unwrap();
    };
    let text_of = |sid: SessionId| {
        let got = s.earlier_chats("p", "u1", me, 5).unwrap();
        got.into_iter()
            .find(|c| c.session_id == sid)
            .map(|c| c.text)
    };
    assert_eq!(text_of(first), Some(Some("rewound zebrafish reply".into())));
    rewind(first, 10, 7);
    rewind(noted, 10, 7);
    assert_eq!(text_of(first), Some(Some("kept reply".into())));
    // The note is rewound with the turn that held it: the chat has no
    // note, and its last visible reply stands in.
    let n = text_of(noted).expect("listed").unwrap_or_default();
    assert!(!n.contains("zebrafish"), "{n}");
    assert_eq!(n, "kept note reply");
    for (sid, seq) in [(first, 11), (noted, 11)] {
        s.append(
            "n1",
            &RawEvent {
                session_id: sid,
                seq,
                ts_ms: 2000,
                kind: "conversation_rewind_undone".into(),
                payload: json!({"rewind_seq": 10}),
            },
        )
        .unwrap();
    }
    assert_eq!(text_of(first), Some(Some("rewound zebrafish reply".into())));
    assert_eq!(
        text_of(noted),
        Some(Some("Status:\nrewound zebrafish note".into()))
    );
}
