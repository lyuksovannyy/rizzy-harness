//! Shared `ui:` block fixture: `web/src/md/remark-ui.test.tsx` renders the
//! same texts and must find every block here at its ordinal, hash and type,
//! `web/src/md/fnv.test.ts` checks the hashes, and `web/src/ui/form.test.ts`
//! must accept and reject every reply exactly as `ui::answer_reply` does.
//! Regenerate with `UPDATE_FIXTURES=1 cargo test -p harness-core --test ui_fixture`.
//!
//! Each case records, by hand, what the browser must do with it: how many
//! ui fences it marks (`web_marks`), and how many cards and error notes it
//! shows (`cards`, `errors`). Where micromark and the Rust scanner disagree
//! about a fence (an HTML block or math swallowing it, a list fence closed
//! at column 0), the browser's ordinals and hashes do not line up with the
//! stored blocks, and it must show no card at all: plain code instead.

use harness_core::agent::UiLevel;
use harness_core::event::{FormReply, UiBlock};
use harness_core::ui::{MAX_REPLY, answer_reply, extract, fnv1a32};
use serde_json::{Value, json};

const CHOICES: &str = r#"{"id": "db", "question": "Which database?", "options": [{"id": "pg", "label": "PostgreSQL", "description": "Best default"}, "SQLite"]}"#;
const PROGRESS: &str = r#"{"id": "job", "title": "Migrating", "steps": [{"label": "Read", "status": "done"}, {"label": "Write", "status": "active"}]}"#;
const CHART: &str = r#"{"kind": "bar", "x": {"values": ["core", "web"]}, "series": [{"name": "debug", "values": [12.5, 8]}]}"#;

fn fence(kind: &str, body: &str) -> String {
    format!("```ui:{kind}\n{body}\n```\n")
}

struct Case {
    name: &'static str,
    text: String,
    level: UiLevel,
    web_marks: usize,
    cards: usize,
    errors: usize,
}

fn case(
    name: &'static str,
    text: String,
    level: UiLevel,
    (web_marks, cards, errors): (usize, usize, usize),
) -> Case {
    Case {
        name,
        text,
        level,
        web_marks,
        cards,
        errors,
    }
}

fn cases() -> Vec<Case> {
    let form = r#"{"id": "trip", "title": "Trip", "fields": [
  {"id": "people", "label": "Travellers", "type": "number", "required": true, "min": 1, "max": 12},
  {"id": "key", "label": "API key", "type": "text"},
  {"id": "when", "label": "Start", "type": "date", "default": "2026-12-01"}
]}"#;
    let pretty = "{\n  \"id\": \"p\",\n\n  \"title\": \"Pretty\",\n\t\"steps\": [{\"label\": \"a\", \"status\": \"done\"}]\n}";
    let job = |i: usize| PROGRESS.replace("\"job\"", &format!("\"j{i}\""));
    let nine: String = (0..9).map(|i| fence("progress", &job(i))).collect();
    // Two different progress fences: if the browser matched by hash alone it
    // would show the second, by ordinal and type alone the first.
    let pair = format!(
        "{}\n{}",
        fence("progress", &job(1)),
        fence("progress", &job(2))
    );
    vec![
        case(
            "every type",
            format!(
                "Intro.\n\n{}\nText between.\n\n{}{}{}",
                fence("choices", CHOICES),
                fence("progress", PROGRESS),
                fence("chart", CHART),
                fence("progress", pretty)
            ),
            UiLevel::Rich,
            (4, 4, 0),
        ),
        case(
            "form with a refused field",
            fence("form", form),
            UiLevel::Rich,
            (1, 1, 0),
        ),
        case(
            "errors",
            format!(
                "{}{}{}",
                fence("progress", "{\"id\": \"p\", \"title\": \"T\"}"),
                fence("chart", "{not json"),
                fence(
                    "choices",
                    &CHOICES.replace("Best default", &"x".repeat(201))
                )
            ),
            UiLevel::Rich,
            (3, 0, 3),
        ),
        case(
            "two input blocks",
            format!("{}{}", fence("choices", CHOICES), fence("form", form)),
            UiLevel::Rich,
            (2, 1, 1),
        ),
        case(
            "display level",
            format!(
                "{}{}",
                fence("choices", CHOICES),
                fence("progress", PROGRESS)
            ),
            UiLevel::Display,
            (2, 1, 1),
        ),
        case(
            "off level",
            fence("progress", PROGRESS),
            UiLevel::Off,
            (1, 0, 0),
        ),
        case(
            "not blocks",
            [
                format!("````md\n{}````\n", fence("choices", CHOICES)),
                fence("widget", "{}"),
                "```ui:progress extra\n{}\n```\n".to_string(),
                format!("  {}", fence("progress", PROGRESS)),
                format!("- item\n\n  ```ui:progress\n  {PROGRESS}\n  ```\n"),
                format!("> ```ui:progress\n> {PROGRESS}\n> ```\n"),
                "Inline ```ui:progress``` code.\n".to_string(),
            ]
            .join("\n"),
            UiLevel::Rich,
            (0, 0, 0),
        ),
        case(
            "fence forms",
            format!("~~~~ ui:progress \n{PROGRESS}\n~~~~~~\n\n````ui:chart\n{CHART}\n   ````\t\n"),
            UiLevel::Rich,
            (2, 2, 0),
        ),
        case(
            "crlf and hidden characters",
            format!(
                "Line\r\n\r\n```ui:progress\u{200B}\r\n{}\r\n```\r\n",
                PROGRESS.replace("Read", "Re\u{202E}ad")
            ),
            UiLevel::Rich,
            (1, 1, 0),
        ),
        // The browser marks an unterminated fence (code to the end); Rust
        // makes no block of it.
        case(
            "unterminated at the end",
            format!("{}```ui:choices\n{CHOICES}\n", fence("progress", PROGRESS)),
            UiLevel::Rich,
            (2, 1, 0),
        ),
        case("nine blocks", nine, UiLevel::Rich, (9, 8, 0)),
        // Rust counts fences micromark does not: each case shows no card.
        case(
            "an html block swallows a fence",
            format!("<div>\n{pair}"),
            UiLevel::Rich,
            (1, 0, 0),
        ),
        case(
            "a tag line swallows a fence",
            format!("<br>\n{pair}"),
            UiLevel::Rich,
            (1, 0, 0),
        ),
        case(
            "display math holds a fence",
            format!(
                "$$\n{}$$\n\n{}",
                fence("choices", CHOICES),
                fence("progress", PROGRESS)
            ),
            UiLevel::Rich,
            (1, 0, 0),
        ),
        case(
            "a list fence closed at column 0",
            format!("- a\n  ```\n  code\n```\n{pair}"),
            UiLevel::Rich,
            (1, 0, 0),
        ),
    ]
}

/// The one valid block of a `ui:<kind>` fence.
fn block(kind: &str, body: Value) -> UiBlock {
    let b = extract(&fence(kind, &body.to_string()), UiLevel::Rich).remove(0);
    assert!(b.error.is_none(), "{:?}", b.error);
    b
}

/// Answers to a question and a form, each valid or invalid, for the
/// browser's checks to agree with.
fn replies() -> Vec<Value> {
    let single = block(
        "choices",
        json!({"id": "colour", "question": "Which colour?", "options": [{"id": "blue", "label": "Blue"}, {"id": "green", "label": "Green"}, {"id": "red", "label": "Red"}]}),
    );
    let other = block(
        "choices",
        json!({"id": "db", "question": "Which database?", "options": ["PostgreSQL", "SQLite"], "other": true}),
    );
    let multi = block(
        "choices",
        json!({"id": "lint", "question": "Which linters?", "options": ["eslint", "prettier", "biome"], "multi": true, "other": true, "min": 2, "max": 2}),
    );
    let form = block(
        "form",
        json!({"id": "trip", "title": "Trip", "fields": [
            {"id": "people", "label": "Travellers", "type": "number", "required": true, "min": 1, "max": 12, "step": 1, "unit": "people"},
            {"id": "dest", "label": "Destination", "type": "choice", "options": [{"id": "lis", "label": "Lisbon"}, {"id": "rom", "label": "Rome"}]},
            {"id": "extras", "label": "Extras", "type": "choice", "multi": true, "options": ["Breakfast", "Parking", "Late checkout"], "max": 2},
            {"id": "start", "label": "Start", "type": "date", "min": "2026-11-08", "max": "2027-11-08"},
            {"id": "flex", "label": "Flexible", "type": "toggle"},
            {"id": "notes", "label": "Notes", "type": "text", "multiline": true, "max_length": 20},
            {"id": "name", "label": "Name", "type": "text", "max_length": 10},
            {"id": "budget", "label": "Budget", "type": "number", "step": 0.5},
            {"id": "key", "label": "API key", "type": "text"}
        ]}),
    );
    let with = |extra: Value| {
        let mut v = json!({"people": 2});
        v.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        v
    };
    // Bounds that JS would print with an exponent; the errors name them.
    let wide = block(
        "form",
        json!({"id": "wide", "title": "Wide", "fields": [
            {"id": "n", "label": "N", "type": "number", "min": 1e-7, "max": 1e21, "step": 1e-7}
        ]}),
    );
    // Nine long text fields: every answer within its field, together at
    // the size cap or one byte over it.
    let fields: Vec<Value> = (1..=9)
        .map(|i| json!({"id": format!("p{i}"), "label": format!("Part {i}"), "type": "text", "multiline": true, "max_length": 4000}))
        .collect();
    let essay = block(
        "form",
        json!({"id": "essay", "title": "Essay", "fields": fields}),
    );
    let sized = |over: usize| {
        // A two-byte letter, and a quote and a line break that each take
        // two bytes once escaped, so both sides count encoded bytes.
        let mut v = json!({"p1": "é\"\n".repeat(1000)});
        for i in 2..=8 {
            v[format!("p{i}")] = json!("x".repeat(3700));
        }
        let fill = |n: usize| {
            let mut v = v.clone();
            v["p9"] = json!("y".repeat(n));
            v
        };
        let base = serde_json::to_vec(&reply_to(&essay, fill(0)))
            .unwrap()
            .len();
        fill(MAX_REPLY - base + over)
    };
    let cases: Vec<(&UiBlock, Value)> = vec![
        (&single, json!({"selected": ["blue"]})),
        (&single, json!({"selected": []})),
        (&single, json!({"selected": ["blue", "red"]})),
        (&single, json!({"selected": ["pink"]})),
        (&single, json!({"selected": ["blue", "blue"]})),
        (&single, json!({"selected": "blue"})),
        (&single, json!({})),
        (&single, json!({"selected": ["blue"], "other": "teal"})),
        (&single, json!({"selected": ["blue"], "shade": "dark"})),
        (&other, json!({"selected": [], "other": "MariaDB 11"})),
        (&other, json!({"selected": ["o1"], "other": "MariaDB"})),
        (&other, json!({"selected": [], "other": "  "})),
        (&other, json!({"selected": [], "other": "a\nb"})),
        (&other, json!({"selected": [], "other": "é".repeat(500)})),
        (&other, json!({"selected": [], "other": "x".repeat(501)})),
        (&multi, json!({"selected": ["o3", "o1"]})),
        (&multi, json!({"selected": ["o1"], "other": "stylelint"})),
        (
            &multi,
            json!({"selected": ["o1", "o2"], "other": "stylelint"}),
        ),
        (&multi, json!({"selected": ["o1"]})),
        (&multi, json!({"selected": ["o1", "o2", "o3"]})),
        (
            &form,
            json!({"people": 3, "dest": "lis", "extras": ["o2", "o1"], "start": "2026-12-01", "flex": true, "notes": "Window seat,\nplease", "name": "Ana", "budget": 120.5}),
        ),
        (&form, json!({"people": 12.0, "flex": false})),
        (&form, json!({})),
        (&form, json!({"people": "2"})),
        (&form, json!({"people": 0})),
        (&form, json!({"people": 13})),
        (&form, json!({"people": 2.5})),
        (&form, with(json!({"budget": 0.25}))),
        (&form, with(json!({"dest": "par"}))),
        (&form, with(json!({"dest": ["lis"]}))),
        (&form, with(json!({"extras": []}))),
        (&form, with(json!({"extras": ["o1", "o2", "o3"]}))),
        (&form, with(json!({"extras": ["o1", "o1"]}))),
        (&form, with(json!({"start": "2026-02-30"}))),
        (&form, with(json!({"start": "2026-11-07"}))),
        (&form, with(json!({"start": "2027-11-09"}))),
        (&form, with(json!({"flex": "yes"}))),
        (&form, with(json!({"notes": ""}))),
        (&form, with(json!({"notes": "x".repeat(21)}))),
        (&form, with(json!({"name": "a\nb"}))),
        (&form, with(json!({"name": 7}))),
        (&form, with(json!({"key": "sk-123"}))),
        (&form, with(json!({"colour": 1}))),
        (&form, with(json!({"Bad Key": 1}))),
        (&form, json!({"zz": 1})),
        (&wide, json!({"n": 1e22})),
        (&wide, json!({"n": 1e-8})),
        (&wide, json!({"n": 1.5e-7})),
        (&wide, json!({"n": 5e20})),
        (&essay, sized(0)),
        (&essay, sized(1)),
    ];
    cases
        .into_iter()
        .map(|(b, values)| {
            let reply = reply_to(b, values);
            let got = answer_reply(b, &reply);
            json!({
                "type": b.kind, "spec": b.spec, "reply": reply, "ok": got.is_ok(),
                "canonical": got.as_ref().ok(), "error": got.as_ref().err(),
            })
        })
        .collect()
}

fn reply_to(b: &UiBlock, values: Value) -> FormReply {
    FormReply {
        message_id: "01JC6Z8Q2V3W4X5Y6Z7A8B9C0D".into(),
        block_id: b.id.clone(),
        values: values.as_object().unwrap().clone(),
    }
}

fn doc() -> Value {
    let texts: Vec<Value> = cases()
        .into_iter()
        .map(|c| {
            let blocks = extract(&c.text, c.level);
            // Only a stored block can show, and only as what it is.
            let specs = blocks.iter().filter(|b| b.spec.is_some()).count();
            assert!(c.cards <= specs, "{}: more cards than valid blocks", c.name);
            assert!(
                c.errors <= blocks.len() - specs,
                "{}: more errors than error blocks",
                c.name
            );
            json!({
                "name": c.name, "text": c.text, "level": c.level, "blocks": blocks,
                "web_marks": c.web_marks, "cards": c.cards, "errors": c.errors,
            })
        })
        .collect();
    let hashes: Vec<Value> = [
        "",
        "a",
        "foobar",
        "é",
        "日本語",
        "🦀 crab",
        "line\nbreak",
        CHOICES,
    ]
    .into_iter()
    .map(|s| json!({"input": s, "hash": format!("{:08x}", fnv1a32(s.as_bytes()))}))
    .collect();
    json!({"texts": texts, "hashes": hashes, "replies": replies()})
}

#[test]
fn fixture_is_current() {
    let doc = doc();
    let text = serde_json::to_string_pretty(&doc).unwrap() + "\n";
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/src/__fixtures__/ui.json");
    if std::env::var_os("UPDATE_FIXTURES").is_some() {
        std::fs::write(&path, &text).unwrap();
    }
    let on_disk =
        std::fs::read_to_string(&path).expect("fixture missing; run with UPDATE_FIXTURES=1");
    let on_disk: Value = serde_json::from_str(&on_disk).unwrap();
    assert_eq!(on_disk, doc, "fixture is stale; run with UPDATE_FIXTURES=1");
}

/// What the fixture records, spot-checked by hand, so a regression in
/// `extract` cannot just be regenerated into it unnoticed.
#[test]
fn fixture_cases_say_what_they_mean() {
    let doc = doc();
    let case = |name: &str| -> Vec<Value> {
        doc["texts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == name)
            .unwrap()["blocks"]
            .as_array()
            .unwrap()
            .clone()
    };
    let kinds = |bs: &[Value]| {
        bs.iter()
            .map(|b| b["type"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    let errs = |bs: &[Value]| {
        bs.iter()
            .map(|b| b["error"].as_str().map(str::to_string))
            .collect::<Vec<_>>()
    };
    let every = case("every type");
    assert_eq!(kinds(&every), ["choices", "progress", "chart", "progress"]);
    assert!(errs(&every).iter().all(Option::is_none));
    assert_eq!(every[2]["id"], "b2");
    assert_eq!(every[3]["hash"], format!("{:08x}", fnv1a32(b"{\n  \"id\": \"p\",\n\n  \"title\": \"Pretty\",\n\t\"steps\": [{\"label\": \"a\", \"status\": \"done\"}]\n}")));
    let form = case("form with a refused field");
    assert_eq!(
        form[0]["spec"]["fields"][1],
        json!({"id": "key", "label": "API key", "type": "text", "refused": true})
    );
    assert_eq!(
        errs(&case("errors")),
        [
            Some("steps: missing".to_string()),
            Some("body: not valid JSON (key must be a string at line 1 column 2)".to_string()),
            Some("options[0].description: longer than 200 characters".to_string()),
        ]
    );
    assert_eq!(
        errs(&case("two input blocks"))[1].as_deref(),
        Some("only one choices or form block per message")
    );
    assert_eq!(
        errs(&case("display level"))[0].as_deref(),
        Some("interactive blocks are off for this agent")
    );
    assert!(case("off level").is_empty() && case("not blocks").is_empty());
    assert_eq!(kinds(&case("fence forms")), ["progress", "chart"]);
    let hidden = case("crlf and hidden characters");
    assert_eq!(hidden[0]["spec"]["steps"][0]["label"], "Read");
    assert_eq!(
        hidden[0]["hash"],
        format!("{:08x}", fnv1a32(PROGRESS.as_bytes()))
    );
    let replies = doc["replies"].as_array().unwrap();
    assert_eq!(replies[0]["canonical"], "Choice answer · colour: blue");
    assert_eq!(replies[2]["error"], "selected: pick only one");
    let essay: Vec<&Value> = replies
        .iter()
        .filter(|r| r["reply"]["block_id"] == "essay")
        .collect();
    assert_eq!(essay[0]["ok"], true, "exactly at the cap");
    assert_eq!(essay[1]["error"], harness_core::ui::TOO_LARGE);
    let wide = |n: usize| {
        replies
            .iter()
            .filter(|r| r["reply"]["block_id"] == "wide")
            .nth(n)
            .unwrap()
    };
    assert_eq!(
        wide(0)["error"],
        "n: must be at most 1000000000000000000000"
    );
    assert_eq!(wide(1)["error"], "n: must be at least 0.0000001");
    assert_eq!(
        wide(3)["canonical"],
        "Form answer · wide\n- n: 500000000000000000000"
    );
    assert!(replies.iter().filter(|r| r["ok"] == true).count() >= 6);
    assert!(replies.iter().filter(|r| r["ok"] == false).count() >= 30);
    assert_eq!(case("unterminated at the end").len(), 1);
    assert_eq!(case("nine blocks").len(), 8);
    // Where the parsers disagree, Rust does make valid blocks: no card is
    // the browser declining them, not Rust rejecting them.
    for name in [
        "an html block swallows a fence",
        "a tag line swallows a fence",
        "display math holds a fence",
        "a list fence closed at column 0",
    ] {
        let blocks = case(name);
        assert_eq!(blocks.len(), 2, "{name}");
        assert!(errs(&blocks).iter().all(Option::is_none), "{name}");
    }
}
