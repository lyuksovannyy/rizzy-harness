//! Shared `ui:` block fixture: `web/src/md/remark-ui.test.tsx` renders the
//! same texts and must find every block here at its ordinal, hash and type,
//! and `web/src/md/fnv.test.ts` checks the hashes.
//! Regenerate with `UPDATE_FIXTURES=1 cargo test -p harness-core --test ui_fixture`.
//!
//! `web_marks` is how many ui fences the browser marks in the text: more
//! than the blocks where it marks what Rust does not make a block (an
//! unterminated fence, the ninth fence).

use harness_core::agent::UiLevel;
use harness_core::ui::{extract, fnv1a32};
use serde_json::{Value, json};

const CHOICES: &str = r#"{"id": "db", "question": "Which database?", "options": [{"id": "pg", "label": "PostgreSQL", "description": "Best default"}, "SQLite"]}"#;
const PROGRESS: &str = r#"{"id": "job", "title": "Migrating", "steps": [{"label": "Read", "status": "done"}, {"label": "Write", "status": "active"}]}"#;
const CHART: &str = r#"{"kind": "bar", "x": {"values": ["core", "web"]}, "series": [{"name": "debug", "values": [12.5, 8]}]}"#;

fn fence(kind: &str, body: &str) -> String {
    format!("```ui:{kind}\n{body}\n```\n")
}

fn cases() -> Vec<(&'static str, String, UiLevel, usize)> {
    let form = r#"{"id": "trip", "title": "Trip", "fields": [
  {"id": "people", "label": "Travellers", "type": "number", "required": true, "min": 1, "max": 12},
  {"id": "key", "label": "API key", "type": "text"},
  {"id": "when", "label": "Start", "type": "date", "default": "2026-12-01"}
]}"#;
    let pretty = "{\n  \"id\": \"p\",\n\n  \"title\": \"Pretty\",\n\t\"steps\": [{\"label\": \"a\", \"status\": \"done\"}]\n}";
    let nine: String = (0..9)
        .map(|i| {
            fence(
                "progress",
                &PROGRESS.replace("\"job\"", &format!("\"j{i}\"")),
            )
        })
        .collect();
    vec![
        (
            "every type",
            format!(
                "Intro.\n\n{}\nText between.\n\n{}{}{}",
                fence("choices", CHOICES),
                fence("progress", PROGRESS),
                fence("chart", CHART),
                fence("progress", pretty)
            ),
            UiLevel::Rich,
            4,
        ),
        (
            "form with a refused field",
            fence("form", form),
            UiLevel::Rich,
            1,
        ),
        (
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
            3,
        ),
        (
            "two input blocks",
            format!("{}{}", fence("choices", CHOICES), fence("form", form)),
            UiLevel::Rich,
            2,
        ),
        (
            "display level",
            format!(
                "{}{}",
                fence("choices", CHOICES),
                fence("progress", PROGRESS)
            ),
            UiLevel::Display,
            2,
        ),
        ("off level", fence("progress", PROGRESS), UiLevel::Off, 1),
        (
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
            0,
        ),
        (
            "fence forms",
            format!("~~~~ ui:progress \n{PROGRESS}\n~~~~~~\n\n````ui:chart\n{CHART}\n   ````\t\n"),
            UiLevel::Rich,
            2,
        ),
        (
            "crlf and hidden characters",
            format!(
                "Line\r\n\r\n```ui:progress\u{200B}\r\n{}\r\n```\r\n",
                PROGRESS.replace("Read", "Re\u{202E}ad")
            ),
            UiLevel::Rich,
            1,
        ),
        (
            "unterminated at the end",
            format!("{}```ui:choices\n{CHOICES}\n", fence("progress", PROGRESS)),
            UiLevel::Rich,
            2,
        ),
        ("nine blocks", nine, UiLevel::Rich, 9),
    ]
}

fn doc() -> Value {
    let texts: Vec<Value> = cases()
        .into_iter()
        .map(|(name, text, level, web_marks)| {
            let blocks = extract(&text, level);
            assert!(blocks.len() <= web_marks, "{name}: more blocks than marks");
            json!({"name": name, "text": text, "level": level, "web_marks": web_marks, "blocks": blocks})
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
    json!({"texts": texts, "hashes": hashes})
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
    assert_eq!(case("unterminated at the end").len(), 1);
    assert_eq!(case("nine blocks").len(), 8);
}
