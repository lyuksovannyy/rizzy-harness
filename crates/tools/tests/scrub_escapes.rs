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

// ---- fix round: stripping must not hide text silently ----

async fn run_plain(cmd: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let ctx = ToolCtx::new(dir.path());
    Bash.run(&ctx, json!({ "command": cmd })).await.output
}

/// What the command printed, between the result's own lines.
fn body(out: &str) -> &str {
    out.strip_suffix("\n[exit code: 0]").unwrap_or(out)
}

/// An invalid byte, then a three-byte character cut across two writes: one
/// replacement character and the character whole (the old reader decoded
/// the unfinished tail lossily too).
#[tokio::test]
async fn a_character_split_after_an_invalid_byte_is_read_whole() {
    let out = run_plain("printf 'a\\377b\\342\\202'; sleep 0.3; printf '\\254f\\n'").await;
    assert_eq!(body(&out), "a\u{fffd}b\u{20ac}f\n");
}

/// A made-up non-ASCII secret after an invalid byte, cut inside a
/// character, is scrubbed.
#[tokio::test]
async fn a_non_ascii_secret_cut_inside_a_character_after_an_invalid_byte_is_scrubbed() {
    let secret = "k\u{e9}y-\u{20ac}\u{20ac}-test-0123456789";
    let set = SecretSet::new(vec![Secret::new(secret.to_string())]);
    // The secret's bytes, cut inside the first euro sign.
    let bytes = secret.as_bytes();
    let cut = secret.find('\u{20ac}').unwrap() + 1;
    let octal = |b: &[u8]| b.iter().map(|x| format!("\\{x:03o}")).collect::<String>();
    let cmd = format!(
        "printf 'x\\377 {}'; sleep 0.3; printf '{}\\n'",
        octal(&bytes[..cut]),
        octal(&bytes[cut..])
    );
    let out = run_plain(&cmd).await;
    let shown = set.scrub(&out);
    assert!(
        !shown.contains("test-0123456789") && !shown.contains('\u{20ac}'),
        "{shown:?}"
    );
}

/// Output with no escape is what the command printed, byte for byte, except
/// that a C1 code point is dropped.
#[tokio::test]
async fn output_with_no_escape_is_unchanged() {
    let out = run_plain(
        "printf 'a\\377b\\r\\nc\\rd\\te\\342\\202'; sleep 0.3; printf '\\254f\\r10%%\\r20%%\\r\\nz\\n'",
    )
    .await;
    assert_eq!(
        body(&out),
        "a\u{fffd}b\r\nc\rd\te\u{20ac}f\r10%\r20%\r\nz\n"
    );
    // A multi-byte character cut at every byte of a run of them.
    let out = run_plain(
        "printf '\\303'; sleep 0.2; printf '\\251\\342'; sleep 0.2; printf '\\202\\254\\360\\237'; sleep 0.2; printf '\\230\\200\\n'",
    )
    .await;
    assert_eq!(body(&out), "\u{e9}\u{20ac}\u{1f600}\n");
    // The one difference: a C1 code point is dropped, alone.
    let out = run_plain("printf 'caf\\302\\205e\\n'").await;
    assert_eq!(body(&out), "cafe\n");
}

/// A string sequence that is never closed hides nothing: only its
/// introducer goes. Chained, as the review's probe.
#[tokio::test]
async fn an_unterminated_string_shows_its_payload() {
    let h = "q".repeat(4096);
    let mut input = String::from("shown ");
    for _ in 0..20 {
        input.push_str("\u{1b}]");
        input.push_str(&h);
    }
    input.push_str("END\n");
    let got = harness_tools::filter::strip_ansi(&input);
    assert_eq!(got.matches('q').count(), 20 * 4096, "hidden: {}", got.len());
    assert!(got.starts_with("shown ") && got.ends_with("END\n"));
    // Through the pipe reader too.
    let cmd = (
        "printf 'shown '; for i in $(seq 5); do printf '\\033]'; head -c 4096 /dev/zero | tr '\\0' q; done; printf 'END\\n'"
    ).to_string();
    let out = run_plain(&cmd).await;
    assert_eq!(body(&out).matches('q').count(), 5 * 4096);
}

/// Terminated strings may remove at most 256 characters a line without a
/// trace; past that each removal leaves a marker naming its size, also the
/// one that crosses the bound; a new line starts the count again.
#[tokio::test]
async fn terminated_strings_past_256_a_line_leave_a_marker() {
    let strip = harness_tools::filter::strip_ansi;
    let osc = |n: usize| format!("\u{1b}]8;;{}\u{7}", "u".repeat(n.saturating_sub(3)));
    // Under the bound: no trace.
    assert_eq!(strip(&format!("a{}b\n", osc(200))), "ab\n");
    // Chained: 4 x 100 characters. The third crosses 256, the fourth is past it.
    let line = format!("a{}{}{}{}b\n", osc(100), osc(100), osc(100), osc(100));
    let got = strip(&line);
    assert_eq!(
        got,
        "a[100 characters of an escape sequence not shown][100 characters of an escape sequence not shown]b\n"
    );
    // One long one: a marker at once.
    assert_eq!(
        strip(&format!("a{}b", osc(300))),
        "a[300 characters of an escape sequence not shown]b"
    );
    // A newline resets the count.
    let again = format!("{}\n{}\n", osc(200), osc(200));
    assert_eq!(strip(&again), "\n\n");
    // 4 KB payloads, each ended by BEL, chained.
    let big = format!("\u{1b}]{}\u{7}", "q".repeat(4096));
    let got = strip(&format!("shown {big}{big}END\n"));
    assert_eq!(
        got,
        "shown [4096 characters of an escape sequence not shown][4096 characters of an escape sequence not shown]END\n"
    );
}

/// `\r` ends an open string like `\n`, and stays.
#[test]
fn a_carriage_return_ends_an_open_string_and_stays() {
    let got = harness_tools::filter::strip_ansi("10% \u{1b}P\r50%\r100%\n");
    assert_eq!(got, "10% \r50%\r100%\n");
}

/// C1 code points in text are dropped one at a time and open nothing.
#[test]
fn a_c1_code_point_opens_no_sequence() {
    let strip = harness_tools::filter::strip_ansi;
    assert_eq!(
        strip("caf\u{85}e \u{9d}rest of line\nnext"),
        "cafe rest of line\nnext"
    );
    assert_eq!(strip("a\u{90}b\u{98}c\u{9e}d\u{9f}e\u{9c}f"), "abcdef");
    // U+009B with parameters and a final byte is a CSI (removed); without, dropped alone.
    assert_eq!(strip("a\u{9b}0mb"), "ab");
    assert_eq!(strip("a\u{9b}1;31mb"), "ab");
    assert_eq!(strip("a\u{9b} rest\nb"), "a rest\nb");
    assert_eq!(strip("a\u{9b}\nb"), "a\nb");
}

/// Both streams carry escapes, interleaved.
#[tokio::test]
async fn escapes_in_both_streams_are_removed_and_a_key_across_them_is_scrubbed() {
    let set = secrets();
    let out = run_plain(
        "printf 'out sk-test-0123\\033[0m456789abcdef\\n'; printf 'err sk-test-0123\\033[1m456789abcdef\\n' >&2; printf '\\033[31mred\\033[0m\\n' >&2; printf 'end\\n'",
    )
    .await;
    assert!(!out.contains('\u{1b}'), "{out:?}");
    let shown = set.scrub(&out);
    assert!(!shown.contains(TAIL) && !shown.contains(S), "{shown:?}");
    assert!(out.contains("red") && out.contains("end"));
}

/// Real colouring tools, where they exist here.
#[tokio::test]
async fn real_rg_and_git_grep_colours_do_not_leak_the_key() {
    let set = secrets();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".env"), format!("KEY={S}\n")).unwrap();
    let have = |bin: &str| {
        std::process::Command::new(bin)
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    };
    let mut ran = 0;
    for (bin, cmd) in [
        (
            "rg",
            "rg --hidden --color=always -e 0123 -e 456789 -e sk- .",
        ),
        (
            "git",
            "git init -q . && git add .env && git -c core.pager=cat grep --color=always -e 0123 -e 456789 -e sk-",
        ),
    ] {
        if !have(bin) {
            eprintln!("SKIPPED: {bin} is not installed here");
            continue;
        }
        ran += 1;
        let ctx = ToolCtx::new(dir.path());
        let r = Bash.run(&ctx, json!({ "command": cmd })).await;
        assert!(r.output.contains("KEY"), "{bin}: {}", r.output);
        assert!(!r.output.contains('\u{1b}'), "{bin}: {:?}", r.output);
        let shown = set.scrub(&r.output);
        assert!(
            !shown.contains(TAIL) && !shown.contains(S),
            "{bin}: {shown:?}"
        );
    }
    eprintln!("real tools run: {ran}");
}
