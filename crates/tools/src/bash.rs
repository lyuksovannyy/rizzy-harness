use std::process::Stdio;
use std::time::Duration;

use harness_core::event::OutputStream;
use harness_core::policy::ToolRisk;
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::sync::mpsc;

use crate::{Tool, ToolCtx, ToolOutcome, cap, str_arg};

pub struct Bash;

/// Grace period between SIGTERM and SIGKILL.
const TERM_GRACE: Duration = Duration::from_secs(2);

/// SIGKILLs a process group when dropped while still armed.
struct KillGroupOnDrop(Option<i32>);

impl Drop for KillGroupOnDrop {
    fn drop(&mut self) {
        if let Some(pgid) = self.0.take() {
            signal_group(pgid, Signal::SIGKILL);
        }
    }
}
/// Chunks (of at most 8 KiB) in flight from the pipe readers to the loop.
/// When it is full the readers stop reading and the child blocks on its
/// pipe: back-pressure, never unbounded memory.
const PIPE_CHANNEL: usize = 8;

fn signal_group(pgid: i32, sig: Signal) {
    // ESRCH (group already gone) is fine.
    let _ = killpg(Pid::from_raw(pgid), sig);
}

/// Decode as much valid UTF-8 as possible, keeping an incomplete trailing
/// sequence for the next read.
fn drain_utf8(buf: &mut Vec<u8>) -> String {
    match std::str::from_utf8(buf) {
        Ok(s) => {
            let s = s.to_string();
            buf.clear();
            s
        }
        Err(e) if e.error_len().is_none() => {
            let n = e.valid_up_to();
            let s = String::from_utf8_lossy(&buf[..n]).into_owned();
            buf.drain(..n);
            s
        }
        Err(_) => {
            let s = String::from_utf8_lossy(buf).into_owned();
            buf.clear();
            s
        }
    }
}

async fn pump<R: AsyncRead + Unpin>(
    mut r: R,
    stream: OutputStream,
    tx: mpsc::Sender<(OutputStream, String)>,
) {
    let mut chunk = [0u8; 8192];
    let mut pending = Vec::new();
    loop {
        match r.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                pending.extend_from_slice(&chunk[..n]);
                let s = drain_utf8(&mut pending);
                if !s.is_empty() && tx.send((stream, s)).await.is_err() {
                    break;
                }
            }
        }
    }
    if !pending.is_empty() {
        let _ = tx
            .send((stream, String::from_utf8_lossy(&pending).into_owned()))
            .await;
    }
}

#[derive(Debug, PartialEq)]
enum End {
    Exited(Option<i32>),
    Cancelled,
    TimedOut,
}

#[async_trait::async_trait]
impl Tool for Bash {
    fn name(&self) -> &'static str {
        "bash"
    }

    fn description(&self) -> &'static str {
        "Run a shell command with bash in the session workspace. Returns combined stdout/stderr and the exit code. Avoid interactive commands."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {"type": "string", "description": "The command to run"},
                "timeout_secs": {"type": "integer", "description": "Optional timeout in seconds (max 3600)"}
            },
            "required": ["command"]
        })
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::Mutating
    }

    async fn run(&self, ctx: &ToolCtx, input: Value) -> ToolOutcome {
        let command = match str_arg(&input, "command") {
            Ok(c) => c.to_string(),
            Err(e) => return ToolOutcome::err(e),
        };
        let timeout = input
            .get("timeout_secs")
            .and_then(|v| v.as_u64())
            .map(|s| Duration::from_secs(s.clamp(1, 3600)))
            .unwrap_or(ctx.timeout);

        // The allow-listed environment (node.toml `env_pass`), nothing else,
        // in the Landlock sandbox where the node has one (node.toml
        // `[sandbox]`): read-write the session folder and its temp folder.
        let mut cmd: Command = match ctx.sandbox.command(
            "bash".as_ref(),
            &["-c".as_ref(), command.as_ref()],
            &ctx.workspace,
            &ctx.env,
            &ctx.protected,
            crate::sandbox::Grants {
                workspace_write: true,
                ..Default::default()
            },
        ) {
            Ok(c) => c,
            Err(e) => return ToolOutcome::err(e),
        };
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .kill_on_drop(true);
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => return ToolOutcome::err(format!("failed to spawn bash: {e}")),
        };
        let pgid = child.id().map(|p| p as i32);
        // If this call is dropped before it reaped the group itself (the
        // agent loop abandons a tool that outlives a cancel), the whole
        // group dies, not only the shell `kill_on_drop` covers.
        let mut group = KillGroupOnDrop(pgid);

        let (tx, mut rx) = mpsc::channel(PIPE_CHANNEL);
        let out_task = tokio::spawn(pump(
            child.stdout.take().expect("piped"),
            OutputStream::Stdout,
            tx.clone(),
        ));
        let err_task = tokio::spawn(pump(
            child.stderr.take().expect("piped"),
            OutputStream::Stderr,
            tx,
        ));

        let mut combined = String::new();
        // With filtering on, keep (much) more: the full text is stored for
        // `tool_output`, the model only sees the filtered version.
        let keep_chars = if ctx.savings.filter_shell_output && ctx.outputs.is_some() {
            crate::outputs::MAX_STORED
        } else {
            4 * crate::MAX_OUTPUT_CHARS
        };
        // Keep the model's copy (capped) and pass the chunk on to the UI
        // stream, which has its own budget: past it `send` returns at once,
        // so the pipes are still read (and discarded) at full speed.
        let keep = |s: &str, combined: &mut String| {
            if combined.len() < keep_chars {
                combined.push_str(s);
            }
        };

        let deadline = tokio::time::sleep(timeout);
        tokio::pin!(deadline);
        let end = loop {
            tokio::select! {
                Some((stream, s)) = rx.recv() => {
                    keep(&s, &mut combined);
                    if let Some(ui) = &ctx.output {
                        // The consumer may be behind (the channel is
                        // bounded): waiting for it must not delay a cancel
                        // or the timeout.
                        tokio::select! {
                            _ = ui.send(stream, s) => {}
                            _ = ctx.cancel.cancelled() => break End::Cancelled,
                            _ = &mut deadline => break End::TimedOut,
                        }
                    }
                }
                status = child.wait() => {
                    break End::Exited(status.ok().and_then(|s| s.code()));
                }
                _ = ctx.cancel.cancelled() => break End::Cancelled,
                _ = &mut deadline => break End::TimedOut,
            }
        };

        if let Some(pgid) = pgid {
            match end {
                End::Exited(_) => {
                    // The leader exited; reap anything it left in the group so
                    // background children cannot hold the pipes open forever.
                    signal_group(pgid, Signal::SIGKILL);
                }
                End::Cancelled | End::TimedOut => {
                    signal_group(pgid, Signal::SIGTERM);
                    if tokio::time::timeout(TERM_GRACE, child.wait())
                        .await
                        .is_err()
                    {
                        signal_group(pgid, Signal::SIGKILL);
                    }
                    signal_group(pgid, Signal::SIGKILL);
                    let _ = child.wait().await;
                }
            }
        }
        // The group was killed and its leader reaped above: disarm, so a
        // later drop cannot signal a group id the kernel has reused.
        group.0 = None;
        // Drain whatever is left; the group is dead so the pipes will close
        // (unless something outside it holds them: give up after a while).
        let _ = tokio::time::timeout(Duration::from_secs(2), async {
            while let Some((stream, s)) = rx.recv().await {
                keep(&s, &mut combined);
                if let Some(ui) = &ctx.output {
                    ui.send(stream, s).await;
                }
            }
        })
        .await;
        out_task.abort();
        err_task.abort();

        let (mut output, truncated) = match &ctx.outputs {
            Some(store) if ctx.savings.filter_shell_output => {
                let f = crate::filter::filter(&combined);
                let (text, cut) = cap(f.text);
                if f.removed_lines > 0 || cut {
                    let total = combined.lines().count();
                    let note = match store.put(combined) {
                        Some(id) => format!("full output: tool_output id={id}"),
                        None => "the full output could not be kept".to_string(),
                    };
                    (
                        format!(
                            "{text}\n[filtered: {} of {total} lines not shown; {note}]",
                            f.removed_lines
                        ),
                        cut,
                    )
                } else {
                    (text, false)
                }
            }
            _ => cap(combined),
        };
        match end {
            End::Exited(code) => {
                if output.is_empty() {
                    output = "(no output)".into();
                }
                let code_str = code.map_or("signal".to_string(), |c| c.to_string());
                ToolOutcome {
                    ok: code == Some(0),
                    output: format!("{output}\n[exit code: {code_str}]"),
                    exit_code: code,
                    truncated,
                    ..Default::default()
                }
            }
            End::Cancelled => ToolOutcome {
                ok: false,
                output: format!("{output}\n[cancelled: process group killed]"),
                truncated,
                ..Default::default()
            },
            End::TimedOut => ToolOutcome {
                ok: false,
                output: format!(
                    "{output}\n[timed out after {}s: process group killed]",
                    timeout.as_secs()
                ),
                truncated,
                ..Default::default()
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    /// Alive and not a zombie.
    fn pid_alive(pid: i32) -> bool {
        match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(stat) => {
                let state = stat.rsplit(')').next().unwrap_or("").trim_start();
                !state.starts_with('Z')
            }
            Err(_) => false,
        }
    }

    #[tokio::test]
    async fn runs_and_reports_exit_code() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolCtx::new(dir.path());
        let r = Bash
            .run(&ctx, json!({"command": "echo hi; echo err >&2; exit 3"}))
            .await;
        assert!(!r.ok);
        assert_eq!(r.exit_code, Some(3));
        assert!(r.output.contains("hi") && r.output.contains("err"));
    }

    use crate::ToolEnv;
    use crate::stream::{OutputSink, STREAM_CHANNEL, TRUNCATED_MARKER};

    /// A ctx streaming into a sink with `budget`, and a reader that
    /// collects every chunk and the longest queue it saw.
    fn streaming(
        dir: &std::path::Path,
        budget: usize,
    ) -> (ToolCtx, tokio::task::JoinHandle<(Vec<String>, usize)>) {
        let (sink, mut rx) = OutputSink::channel(budget);
        let mut ctx = ToolCtx::new(dir);
        ctx.output = Some(sink);
        let reader = tokio::spawn(async move {
            let (mut got, mut longest) = (Vec::new(), 0);
            while let Some((_, s)) = rx.recv().await {
                longest = longest.max(rx.len() + 1);
                got.push(s);
            }
            (got, longest)
        });
        (ctx, reader)
    }

    async fn run_streaming(ctx: ToolCtx, input: Value) -> ToolOutcome {
        // Dropping the ctx drops the sink, which ends the reader.
        Bash.run(&ctx, input).await
    }

    #[tokio::test]
    async fn streams_output() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, reader) = streaming(dir.path(), crate::stream::DEFAULT_STREAM_BYTES);
        let r = run_streaming(ctx, json!({"command": "printf a; printf b"})).await;
        assert!(r.ok);
        assert_eq!(reader.await.unwrap().0.concat(), "ab");
    }

    /// Under the budget the stream is exactly what the command printed.
    #[tokio::test]
    async fn output_under_the_budget_is_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, reader) = streaming(dir.path(), crate::stream::DEFAULT_STREAM_BYTES);
        let r = run_streaming(ctx, json!({"command": "seq 1 20000"})).await;
        assert!(r.ok);
        let want: String = (1..=20000).map(|n| format!("{n}\n")).collect();
        let got = reader.await.unwrap().0.concat();
        assert_eq!(got.len(), want.len());
        assert!(got == want, "stream differs from the command's output");
        assert!(!got.contains(TRUNCATED_MARKER));
    }

    /// A flood far past the budget streams at most budget + one marker, the
    /// queue stays bounded, and the exit code still comes back.
    #[tokio::test]
    async fn flood_is_capped_with_one_marker() {
        let dir = tempfile::tempdir().unwrap();
        let budget = 64 * 1024;
        let (ctx, reader) = streaming(dir.path(), budget);
        let r = run_streaming(
            ctx,
            json!({"command": "head -c 5000000 /dev/zero | tr '\\0' a; exit 7"}),
        )
        .await;
        assert_eq!(
            r.exit_code,
            Some(7),
            "{}",
            &r.output[r.output.len() - 200..]
        );
        let (chunks, longest) = reader.await.unwrap();
        let all = chunks.concat();
        assert!(
            all.len() <= budget + TRUNCATED_MARKER.len(),
            "streamed {} bytes",
            all.len()
        );
        assert_eq!(all.matches(TRUNCATED_MARKER).count(), 1);
        assert!(all.ends_with(TRUNCATED_MARKER), "nothing after the marker");
        assert_eq!(all.len(), budget + TRUNCATED_MARKER.len());
        assert!(longest <= STREAM_CHANNEL, "queue reached {longest}");
    }

    /// A consumer that never reads fills the bounded channel; cancel must
    /// still kill the group promptly (no deadlock on the full channel).
    #[tokio::test]
    async fn cancel_during_flood_with_a_stuck_consumer() {
        let dir = tempfile::tempdir().unwrap();
        let (sink, rx) = OutputSink::channel(crate::stream::DEFAULT_STREAM_BYTES);
        let mut ctx = ToolCtx::new(dir.path());
        ctx.output = Some(sink);
        let cancel = ctx.cancel.clone();
        let h = tokio::spawn(async move { Bash.run(&ctx, json!({"command": "yes"})).await });
        // Wait for the state the test needs (the bounded channel is full, so
        // the producer is blocked on the stuck consumer), not for a fixed
        // time: a loaded machine can take far longer than 300 ms to get there.
        let full_by = Instant::now() + Duration::from_secs(60);
        while rx.len() < STREAM_CHANNEL {
            assert!(
                Instant::now() < full_by,
                "channel never filled: {}",
                rx.len()
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let started = Instant::now();
        cancel.cancel();
        let r = tokio::time::timeout(Duration::from_secs(10), h)
            .await
            .expect("cancel hung")
            .unwrap();
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(
            r.output.contains("cancelled"),
            "{}",
            &r.output[r.output.len() - 100..]
        );
        assert_eq!(rx.len(), STREAM_CHANNEL, "the consumer never read");
    }

    #[tokio::test]
    async fn timeout_during_flood_with_a_stuck_consumer() {
        let dir = tempfile::tempdir().unwrap();
        let (sink, _rx) = OutputSink::channel(crate::stream::DEFAULT_STREAM_BYTES);
        let mut ctx = ToolCtx::new(dir.path());
        ctx.output = Some(sink);
        let started = Instant::now();
        let r = Bash
            .run(&ctx, json!({"command": "yes", "timeout_secs": 1}))
            .await;
        assert!(started.elapsed() < Duration::from_secs(6));
        assert!(r.output.contains("timed out"));
    }

    #[tokio::test]
    async fn timeout_during_flood_past_the_budget() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, reader) = streaming(dir.path(), 8192);
        let started = Instant::now();
        let r = run_streaming(ctx, json!({"command": "yes", "timeout_secs": 1})).await;
        assert!(started.elapsed() < Duration::from_secs(6));
        assert!(r.output.contains("timed out"));
        let all = reader.await.unwrap().0.concat();
        assert_eq!(all.len(), 8192 + TRUNCATED_MARKER.len());
    }

    async fn shell_var(env: ToolEnv, name: &str) -> String {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = ToolCtx::new(dir.path());
        ctx.env = env;
        let cmd = format!("printf '[%s]' \"${{{name}:-unset}}\"");
        Bash.run(&ctx, json!({"command": cmd})).await.output
    }

    // Cargo sets CARGO_PKG_NAME and CARGO_PKG_VERSION for test processes;
    // the tests read them and set nothing, so they cannot race.
    #[tokio::test]
    async fn unlisted_variables_are_absent() {
        let out = shell_var(ToolEnv::default(), "CARGO_PKG_NAME").await;
        assert!(out.contains("[unset]"), "{out}");
    }

    #[tokio::test]
    async fn listed_and_glob_matched_variables_are_present() {
        let env = ToolEnv::new(&["CARGO_PKG_NAME".into()], []).unwrap();
        let out = shell_var(env, "CARGO_PKG_NAME").await;
        assert!(out.contains("[harness-tools]"), "{out}");
        let env = ToolEnv::new(&["CARGO_PKG_*".into()], []).unwrap();
        let out = shell_var(env.clone(), "CARGO_PKG_VERSION").await;
        assert!(!out.contains("[unset]"), "{out}");
        let out = shell_var(env, "CARGO_MANIFEST_DIR").await;
        assert!(out.contains("[unset]"), "the glob does not match it: {out}");
    }

    #[tokio::test]
    async fn deny_listed_variables_stay_absent_even_with_star() {
        let env = ToolEnv::new(&["*".into()], ["CARGO_PKG_NAME".to_string()]).unwrap();
        let out = shell_var(env.clone(), "CARGO_PKG_NAME").await;
        assert!(out.contains("[unset]"), "{out}");
        let out = shell_var(env, "CARGO_PKG_VERSION").await;
        assert!(!out.contains("[unset]"), "`*` passes the rest: {out}");
    }

    #[tokio::test]
    async fn core_variables_let_ordinary_commands_run() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.txt"), "x").unwrap();
        let ctx = ToolCtx::new(dir.path());
        let r = Bash
            .run(
                &ctx,
                json!({"command": "test -n \"$PATH\" && test -n \"$HOME\" && ls && env | cut -d= -f1 | sort"}),
            )
            .await;
        assert!(r.ok, "{}", r.output);
        assert!(r.output.contains("f.txt"), "{}", r.output);
        for line in r
            .output
            .lines()
            .filter(|l| !l.contains(' ') && !l.is_empty())
        {
            if line == "f.txt" || line.starts_with('[') {
                continue;
            }
            // bash itself adds PWD, SHLVL and `_`.
            assert!(
                crate::env::CORE_VARS.contains(&line)
                    || line.starts_with("LC_")
                    || ["PWD", "OLDPWD", "SHLVL", "_"].contains(&line),
                "unexpected variable {line}"
            );
        }
    }

    /// Cancel must kill the whole tree, including a grandchild that ignores
    /// SIGTERM and runs in the background.
    #[tokio::test]
    async fn cancel_kills_process_tree() {
        let dir = tempfile::tempdir().unwrap();
        let pidfile = dir.path().join("pid");
        let ctx = ToolCtx::new(dir.path());
        let cancel = ctx.cancel.clone();
        let cmd = format!(
            "(trap '' TERM; sleep 1000) & echo $! > {}; wait",
            pidfile.display()
        );
        let started = Instant::now();
        let h = tokio::spawn(async move { Bash.run(&ctx, json!({"command": cmd})).await });
        // Wait for the grandchild to be recorded.
        let mut pid = None;
        for _ in 0..100 {
            if let Ok(s) = std::fs::read_to_string(&pidfile)
                && let Ok(p) = s.trim().parse::<i32>()
            {
                pid = Some(p);
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let pid = pid.expect("grandchild pid");
        assert!(pid_alive(pid));
        cancel.cancel();
        let r = h.await.unwrap();
        assert!(!r.ok);
        assert!(r.output.contains("cancelled"));
        assert!(started.elapsed() < Duration::from_secs(10));
        // Give the kernel a moment to reap.
        for _ in 0..50 {
            if !pid_alive(pid) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(!pid_alive(pid), "grandchild {pid} survived cancel");
    }

    /// The agent loop abandons a tool that outlives a cancel by dropping its
    /// future. That must kill the whole group too, not only the shell: here
    /// the shell and its child both ignore SIGTERM.
    #[tokio::test]
    async fn dropping_the_call_kills_the_process_tree() {
        let dir = tempfile::tempdir().unwrap();
        let pidfile = dir.path().join("pid");
        let ctx = ToolCtx::new(dir.path());
        let cmd = format!(
            "trap '' TERM; (trap '' TERM; sleep 1000) & echo $! > {}; wait",
            pidfile.display()
        );
        let h = tokio::spawn(async move { Bash.run(&ctx, json!({"command": cmd})).await });
        let mut pid = None;
        for _ in 0..100 {
            if let Ok(s) = std::fs::read_to_string(&pidfile)
                && let Ok(p) = s.trim().parse::<i32>()
            {
                pid = Some(p);
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let pid = pid.expect("grandchild pid");
        assert!(pid_alive(pid));
        h.abort();
        let _ = h.await;
        for _ in 0..50 {
            if !pid_alive(pid) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(
            !pid_alive(pid),
            "grandchild {pid} survived the dropped call"
        );
    }

    #[tokio::test]
    async fn timeout_kills() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolCtx::new(dir.path());
        let r = Bash
            .run(&ctx, json!({"command": "sleep 100", "timeout_secs": 1}))
            .await;
        assert!(!r.ok);
        assert!(r.output.contains("timed out"));
    }

    #[tokio::test]
    async fn background_children_do_not_hang() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolCtx::new(dir.path());
        let started = Instant::now();
        let r = Bash
            .run(&ctx, json!({"command": "sleep 1000 & echo done"}))
            .await;
        assert!(r.ok, "{}", r.output);
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
