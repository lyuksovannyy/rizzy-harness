//! A secret a command prints with a terminal escape sequence inside it
//! (`grep --color=always` colours its match) must still be scrubbed, on
//! every path command output takes: the result given to the model, the full
//! output `tool_output` returns later, and the live pieces. Scrubbing is
//! done here as the agent loop does it (`SecretSet::scrub` on a result,
//! a `StreamScrubber` on the live pieces).

use std::sync::Arc;

use harness_core::secret::StreamScrubber;
use harness_core::{Secret, SecretSet, SessionId};
use harness_tools::stream::OutputSink;
use harness_tools::{Bash, OutputStore, Tool, ToolCtx, ToolOutput};
use serde_json::json;

const S: &str = "sk-test-0123456789abcdef";
const TAIL: &str = "456789abcdef";

fn secrets() -> SecretSet {
    SecretSet::new(vec![Secret::new(S.to_string())])
}

/// What a reader of `text` could piece together, after the scrub: the
/// second half of the key, or the key once the escapes are taken out.
fn leaked(set: &SecretSet, text: &str) -> bool {
    let c = set.scrub(text);
    c.contains(TAIL) || harness_tools::filter::strip_ansi(&c).contains(S)
}

/// Escape sequences that can sit inside a key.
const SPLITS: &[(&str, &str)] = &[
    ("sgr", "\x1b[0m"),
    ("grep", "\x1b[01;31m\x1b[K"),
    ("osc_bel", "\x1b]0;t\x07"),
    ("osc_st", "\x1b]8;;http://x\x1b\\"),
    ("other", "\x1bX"),
    ("csi_c1", "\u{9b}0m"),
];

struct Run {
    result: String,
    live: String,
    kept: Option<String>,
}

async fn run(filter: bool, cmd: &str) -> Run {
    let dir = tempfile::tempdir().unwrap();
    let sid = SessionId::new();
    let store = Arc::new(OutputStore::default());
    let mut ctx = ToolCtx::new(dir.path());
    ctx.savings.filter_shell_output = filter;
    ctx.outputs = Some(store.session(sid));
    let (sink, mut rx) = OutputSink::channel(harness_tools::stream::DEFAULT_STREAM_BYTES);
    ctx.output = Some(sink);
    let reader = tokio::spawn(async move {
        let set = secrets();
        let mut sc = StreamScrubber::new(set);
        let mut out = String::new();
        while let Some((_, s)) = rx.recv().await {
            out.push_str(&sc.push(&s));
        }
        out.push_str(&sc.finish());
        out
    });
    let r = Bash.run(&ctx, json!({ "command": cmd })).await;
    let kept = match r.output.split("tool_output id=").nth(1) {
        Some(rest) => {
            let id: String = rest.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
            Some(ToolOutput.run(&ctx, json!({ "id": id })).await.output)
        }
        None => None,
    };
    drop(ctx);
    Run {
        result: r.output,
        live: reader.await.unwrap(),
        kept,
    }
}

fn report(what: &str, set: &SecretSet, text: &str) -> Option<String> {
    leaked(set, text).then(|| format!("{what}: {:?}", set.scrub(text)))
}

/// A key split by an escape, in one write.
#[tokio::test]
async fn a_key_split_by_an_escape_is_scrubbed_on_every_path() {
    let set = secrets();
    let mut bad = vec![];
    for filter in [false, true] {
        for (name, esc) in SPLITS {
            // "Compiling" is noise the filter drops, so the full output is kept.
            let cmd = format!(
                "echo 'Compiling x'; printf 'key sk-test-0123%s456789abcdef\\n' '{esc}'; echo done"
            );
            let r = run(filter, &cmd).await;
            let tag = format!("filter={filter} {name}");
            bad.extend(report(&format!("{tag} result"), &set, &r.result));
            bad.extend(report(&format!("{tag} live"), &set, &r.live));
            if let Some(k) = &r.kept {
                bad.extend(report(&format!("{tag} tool_output"), &set, k));
            } else if filter {
                bad.push(format!("{tag}: nothing was kept: {:?}", r.result));
            }
        }
    }
    assert!(bad.is_empty(), "leaks:\n{}", bad.join("\n"));
}

/// A key split across two writes with the escape at the seam, so the two
/// pipe reads each hold half of it.
#[tokio::test]
async fn a_key_split_across_writes_at_an_escape_is_scrubbed_on_every_path() {
    let set = secrets();
    let mut bad = vec![];
    for filter in [false, true] {
        for (name, esc) in SPLITS {
            // Escape wholly after the first write; and the escape itself cut
            // between the writes (after its first byte).
            let (a, b) = esc.split_at(esc.chars().next().unwrap().len_utf8());
            for (case, first, second) in [
                ("after", S_A.to_string(), format!("{esc}{S_B}")),
                ("before", format!("{S_A}{esc}"), S_B.to_string()),
                ("within", format!("{S_A}{a}"), format!("{b}{S_B}")),
            ] {
                let cmd = format!(
                    "echo 'Compiling x'; printf '%s' 'key {first}'; sleep 0.3; printf '%s\\n' '{second}'; echo done"
                );
                let r = run(filter, &cmd).await;
                let tag = format!("filter={filter} {name} {case}");
                bad.extend(report(&format!("{tag} result"), &set, &r.result));
                bad.extend(report(&format!("{tag} live"), &set, &r.live));
                if let Some(k) = &r.kept {
                    bad.extend(report(&format!("{tag} tool_output"), &set, k));
                }
            }
        }
    }
    assert!(bad.is_empty(), "leaks:\n{}", bad.join("\n"));
}

const S_A: &str = "sk-test-0123";
const S_B: &str = "456789abcdef";

/// Real `grep --color=always` over a file holding the key.
#[tokio::test]
async fn a_real_grep_colour_does_not_leak_the_key() {
    let set = secrets();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".env"), format!("KEY={S}\n")).unwrap();
    let mut bad = vec![];
    for filter in [false, true] {
        // A match inside the key: grep colours just that part.
        for pat in ["0123", "sk-test-0123456789abcdef", "456789"] {
            let mut ctx = ToolCtx::new(dir.path());
            ctx.savings.filter_shell_output = filter;
            let r = Bash
                .run(
                    &ctx,
                    json!({"command": format!("grep --color=always -E '{pat}|sk' .env")}),
                )
                .await;
            bad.extend(report(&format!("filter={filter} {pat}"), &set, &r.output));
        }
    }
    assert!(bad.is_empty(), "leaks:\n{}", bad.join("\n"));
}
