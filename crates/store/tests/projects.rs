//! Projects storage: projects, chat links, the board and saved templates.

use harness_core::{RawEvent, SessionId};
use harness_store::{
    CardPatch, NewCard, NewColumn, NewProject, NewUser, ProjectPatch, Role, Store, StoreError,
    TemplateRow,
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
        can_agentic: false,
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
