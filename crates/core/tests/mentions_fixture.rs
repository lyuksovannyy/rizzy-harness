//! The trigger rule of `harness_core::mention`, against the cases the
//! browser's mirror (`web/src/lib/mentionScan.ts`) reads too
//! (`web/src/__fixtures__/mentions.json`, written by hand): a case added
//! there is checked by both.

use harness_core::mention::{MAX_CALLED, Who, scan};
use serde_json::Value;

fn fixture() -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../web/src/__fixtures__/mentions.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn every_case_matches_the_fixture() {
    let f = fixture();
    let agents = f["agents"].as_object().unwrap().clone();
    let tags = f["tags"].as_object().unwrap().clone();
    let person = f["person"].as_str().unwrap().to_string();
    let resolve = move |n: &str| -> Option<Who> {
        if let Some(id) = agents.get(n) {
            return Some(Who::Agent(id.as_str().unwrap().into()));
        }
        if let Some(id) = tags.get(n) {
            return Some(Who::Tag(id.as_str().unwrap().into()));
        }
        (n == person).then_some(Who::Person)
    };
    let cases = f["cases"].as_array().unwrap();
    assert!(cases.len() >= 50);
    for c in cases {
        let text = c["text"].as_str().unwrap();
        let name = c["name"].as_str().unwrap();
        let m = scan(text, &resolve);
        let called: Vec<&str> = c["called"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(m.called, called, "{name}");
        let found: Vec<(String, String, bool)> = m
            .found
            .iter()
            .map(|x| {
                (
                    text[x.start..x.end].to_string(),
                    match &x.who {
                        Who::Agent(id) => format!("agent:{id}"),
                        Who::Person => "person".into(),
                        Who::Tag(id) => format!("tag:{id}"),
                    },
                    x.trigger,
                )
            })
            .collect();
        let want: Vec<(String, String, bool)> = c["found"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| {
                (
                    x["text"].as_str().unwrap().into(),
                    x["who"].as_str().unwrap().into(),
                    x["trigger"].as_bool().unwrap(),
                )
            })
            .collect();
        assert_eq!(found, want, "{name}");
    }
}

/// 10,000 mentions, or a million tokens, are one linear pass; and the kept
/// list never grows past the limit and one.
#[test]
fn work_is_linear_and_the_list_is_bounded() {
    let resolve = |n: &str| (n == "a").then(|| Who::Agent("a".into()));
    let t = std::time::Instant::now();
    let text = "@a ".repeat(10_000);
    let m = scan(&text, &resolve);
    assert_eq!(m.called, ["a"]);
    assert_eq!(m.found.len(), 10_000);
    let many = format!("{}\n{}", "word @a ".repeat(200_000), "@a ".repeat(100_000));
    scan(&many, &resolve);
    let ticks = "` ".repeat(100_000);
    scan(&ticks, &resolve);
    assert!(
        t.elapsed() < std::time::Duration::from_secs(5),
        "{:?}",
        t.elapsed()
    );
    let five = |n: &str| Some(Who::Agent(n.into()));
    let m = scan("@a @b @c @d @e @f @g", &five);
    assert_eq!(m.called.len(), MAX_CALLED + 1);
}

/// Long runs of white space (review of PR 166, M1): the same answers as the
/// browser's mirror gives (it reads the same `big` cases), in linear time.
#[test]
fn long_runs_of_white_space_are_linear_and_agree() {
    let f = fixture();
    let agents = f["agents"].as_object().unwrap().clone();
    let resolve = move |n: &str| {
        agents
            .get(n)
            .map(|id| Who::Agent(id.as_str().unwrap().into()))
    };
    for c in f["big"].as_array().unwrap() {
        let text: String = c["parts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                p[0].as_str()
                    .unwrap()
                    .repeat(p[1].as_u64().unwrap() as usize)
            })
            .collect();
        let t = std::time::Instant::now();
        let m = scan(&text, &resolve);
        assert!(
            t.elapsed() < std::time::Duration::from_secs(2),
            "{}",
            c["name"]
        );
        let want: Vec<&str> = c["called"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(m.called, want, "{}", c["name"]);
    }
}
