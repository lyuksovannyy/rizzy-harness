//! The Project popup's preview of a new project's folder name (X.26,
//! `web/src/lib/projectSlug.ts`) against the real path: the name as the
//! master cleans it (`projects::clean_required`, which drops control and
//! hidden characters), then the node's slug (`harness_node::projects::slug`).
//! The pairs are written to `web/src/lib/projectSlug.fixture.json`, which
//! the web test reads; regenerate with `HARNESS_WRITE_FIXTURE=1` after a
//! change to either side (review of PR 143: the preview had said `a-b`
//! where the folder became `ab-xxxx`).

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../web/src/lib/projectSlug.fixture.json"
);

fn names() -> Vec<String> {
    let mut v: Vec<String> = [
        // The review's.
        "a\u{200B}b",
        "a\tb",
        "Foo\u{202E}bar",
        "a\0b\nc",
        "İstanbul",
        "ſecret",
        "\u{212A}elvin",
        "ＡＢＣ",
        "-rf",
        "--template=x",
        "\u{202E}evil",
        "a\u{0301}b",
        "😀 x 😀",
        "Ǆemal",
        "ß straße",
        "\u{0130}\u{0131}",
        "a\u{FF0F}b",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa bbbb",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa bbbb",
        "\0\0a",
        // Ours.
        "Project Name Example",
        "../x",
        "..",
        "/etc/passwd",
        "a\u{FF0F}b\u{2215}c\u{2024}d\u{FF0E}e",
        "Café Ünïcode",
        "  --Trim--  ",
        ".hidden",
        "tag\u{E0041}ged",
        "zero\u{FEFF}width\u{2060}joiner",
        "bell\u{7}char",
        "line\u{2028}sep",
        "nbsp\u{A0}space",
        "soft\u{AD}hyphen",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    v.push("x".repeat(60));
    v
}

#[test]
fn the_preview_fixture_is_the_real_path() {
    let pairs: Vec<(String, String)> = names()
        .into_iter()
        .filter_map(|n| {
            // A name the master refuses makes no folder: no preview to check.
            let clean =
                harness_master::projects::clean_required("name", &n, usize::MAX).ok()?;
            Some((n, harness_node::projects::slug(&clean)))
        })
        .collect();
    let text = serde_json::to_string_pretty(&pairs).unwrap() + "\n";
    if std::env::var_os("HARNESS_WRITE_FIXTURE").is_some() {
        std::fs::write(FIXTURE, &text).unwrap();
    }
    assert_eq!(
        std::fs::read_to_string(FIXTURE).unwrap_or_default(),
        text,
        "web/src/lib/projectSlug.fixture.json is stale: run with HARNESS_WRITE_FIXTURE=1"
    );
    // The review's three, said plainly.
    let get = |n: &str| pairs.iter().find(|p| p.0 == n).unwrap().1.clone();
    assert_eq!(get("a\u{200B}b"), "ab");
    assert_eq!(get("a\tb"), "ab");
    assert_eq!(get("Foo\u{202E}bar"), "foobar");
}
