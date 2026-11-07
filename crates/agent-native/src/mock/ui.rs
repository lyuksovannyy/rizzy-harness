//! Canned `ui:` blocks for `ui: <name>` (rich_plan §12.3), written the way
//! a model writes them: a line of prose, then a pretty-printed fence.

/// The reply for `ui: <name>`, if it is one.
pub(super) fn canned(name: &str) -> Option<String> {
    let block = |kind: &str, body: &str| format!("```ui:{kind}\n{body}\n```\n");
    Some(match name {
        // The line after the fence lets a test see the body has streamed
        // while the message is still streaming.
        "choices" => format!("Pick a colour:\n\n{}\n{AFTER}", block("choices", CHOICES)),
        "form" => format!("A few details first:\n\n{}", block("form", FORM)),
        "progress" => format!("Working on it.\n\n{}", block("progress", PROGRESS)),
        "chart" => format!("Build times:\n\n{}", block("chart", CHART)),
        "secret" => format!("To connect:\n\n{}", block("form", SECRET)),
        "bad" => format!("Pick one:\n\n{}", block("choices", &bad())),
        "unknown" => format!(
            "A widget:\n\n{}",
            block("widget", r#"{"id": "w", "title": "Widget"}"#)
        ),
        "two" => format!(
            "Two questions:\n\n{}\n{}",
            block("choices", CHOICES),
            block("choices", &CHOICES.replace("\"colour\"", "\"shade\""))
        ),
        _ => return None,
    })
}

const AFTER: &str = "Tap the one you like best, or tell me another colour in your own words and I will use that one instead.";

const CHOICES: &str = r#"{
  "id": "colour",
  "question": "Which colour do you like best?",
  "options": [
    {"id": "blue", "label": "Blue"},
    {"id": "green", "label": "Green"},
    {"id": "red", "label": "Red"}
  ]
}"#;

const FORM: &str = r#"{
  "id": "trip",
  "title": "Trip details",
  "fields": [
    {"id": "people", "label": "Travellers", "type": "number", "required": true, "min": 1, "max": 12, "step": 1},
    {"id": "dest", "label": "Destination", "type": "choice", "options": [{"id": "lis", "label": "Lisbon"}, {"id": "rom", "label": "Rome"}]},
    {"id": "notes", "label": "Anything else?", "type": "text", "multiline": true, "placeholder": "Optional"}
  ]
}"#;

const PROGRESS: &str = r#"{
  "id": "migrate",
  "title": "Migrating config",
  "steps": [
    {"label": "Read config", "status": "done"},
    {"label": "Write schema", "status": "active", "detail": "3 of 7 tables"},
    {"label": "Run tests", "status": "pending"}
  ]
}"#;

const CHART: &str = r#"{
  "id": "builds",
  "kind": "bar",
  "title": "Build time by crate",
  "x": {"label": "crate", "values": ["core", "web", "master"]},
  "y": {"label": "time", "unit": "s"},
  "series": [
    {"name": "debug", "values": [12.5, 8, 30]},
    {"name": "release", "values": [40, 22, null]}
  ]
}"#;

/// A form that asks for a key: that field is refused, the other one stays.
const SECRET: &str = r#"{
  "id": "connect",
  "title": "Connect the service",
  "fields": [
    {"id": "key", "label": "API key", "type": "text"},
    {"id": "name", "label": "Name", "type": "text"}
  ]
}"#;

/// A question whose first label is one character over the limit.
fn bad() -> String {
    let label = format!("A{}", "a".repeat(80));
    format!(
        "{{\n  \"id\": \"bad\",\n  \"question\": \"Pick one\",\n  \"options\": [{{\"label\": \"{label}\"}}, \"Short\"]\n}}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_core::agent::UiLevel;
    use harness_core::ui::extract;

    /// Every canned block validates as the e2e tests expect.
    #[test]
    fn canned_blocks_validate() {
        let errors = |name: &str| -> Vec<Option<String>> {
            extract(&canned(name).unwrap(), UiLevel::Rich)
                .into_iter()
                .map(|b| b.error)
                .collect()
        };
        for name in ["choices", "form", "progress", "chart", "secret"] {
            assert_eq!(errors(name), [None], "{name}");
        }
        assert_eq!(
            errors("bad"),
            [Some("options[0].label: longer than 80 characters".into())]
        );
        assert_eq!(errors("unknown"), Vec::<Option<String>>::new());
        assert_eq!(
            errors("two"),
            [
                None,
                Some("only one choices or form block per message".into())
            ]
        );
        let secret = extract(&canned("secret").unwrap(), UiLevel::Rich);
        let fields = &secret[0].spec.as_ref().unwrap()["fields"];
        assert_eq!(fields[0]["refused"], true);
        assert!(fields[1].get("refused").is_none());
        assert!(canned("nope").is_none());
    }
}
