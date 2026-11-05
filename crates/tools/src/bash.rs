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
    tx: mpsc::UnboundedSender<(OutputStream, String)>,
) {
    let mut chunk = [0u8; 8192];
    let mut pending = Vec::new();
    loop {
        match r.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                pending.extend_from_slice(&chunk[..n]);
                let s = drain_utf8(&mut pending);
                if !s.is_empty() && tx.send((stream, s)).is_err() {
                    break;
                }
            }
        }
    }
    if !pending.is_empty() {
        let _ = tx.send((stream, String::from_utf8_lossy(&pending).into_owned()));
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

        let mut cmd = Command::new("bash");
        cmd.arg("-c")
            .arg(&command)
            .current_dir(&ctx.workspace)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .kill_on_drop(true);
        for k in &ctx.env_remove {
            cmd.env_remove(k);
        }
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => return ToolOutcome::err(format!("failed to spawn bash: {e}")),
        };
        let pgid = child.id().map(|p| p as i32);

        let (tx, mut rx) = mpsc::unbounded_channel();
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
        let forward = |stream: OutputStream, s: String, combined: &mut String| {
            if combined.len() < keep_chars {
                combined.push_str(&s);
            }
            if let Some(ui) = &ctx.output {
                let _ = ui.send((stream, s));
            }
        };

        let deadline = tokio::time::sleep(timeout);
        tokio::pin!(deadline);
        let end = loop {
            tokio::select! {
                Some((stream, s)) = rx.recv() => forward(stream, s, &mut combined),
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
        // Drain whatever is left; the group is dead so the pipes will close.
        let _ = tokio::time::timeout(Duration::from_secs(2), async {
            let _ = out_task.await;
            let _ = err_task.await;
        })
        .await;
        while let Ok((stream, s)) = rx.try_recv() {
            forward(stream, s, &mut combined);
        }

        let (mut output, truncated) = match &ctx.outputs {
            Some(store) if ctx.savings.filter_shell_output => {
                let f = crate::filter::filter(&combined);
                let (text, cut) = cap(f.text);
                if f.removed_lines > 0 || cut {
                    let total = combined.lines().count();
                    let id = store.put(combined);
                    (
                        format!(
                            "{text}\n[filtered: {} of {total} lines not shown; full output: \
                             tool_output id={id}]",
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
                }
            }
            End::Cancelled => ToolOutcome {
                ok: false,
                output: format!("{output}\n[cancelled: process group killed]"),
                exit_code: None,
                truncated,
            },
            End::TimedOut => ToolOutcome {
                ok: false,
                output: format!(
                    "{output}\n[timed out after {}s: process group killed]",
                    timeout.as_secs()
                ),
                exit_code: None,
                truncated,
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

    #[tokio::test]
    async fn streams_output() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut ctx = ToolCtx::new(dir.path());
        ctx.output = Some(tx);
        let r = Bash
            .run(&ctx, json!({"command": "printf a; printf b"}))
            .await;
        assert!(r.ok);
        let mut got = String::new();
        while let Ok((_, s)) = rx.try_recv() {
            got.push_str(&s);
        }
        assert_eq!(got, "ab");
    }

    #[tokio::test]
    async fn env_remove_hides_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = ToolCtx::new(dir.path());
        // cargo sets CARGO_MANIFEST_DIR for test processes.
        let cmd = json!({"command": "echo \"[${CARGO_MANIFEST_DIR:-unset}]\""});
        let visible = Bash.run(&ctx, cmd.clone()).await;
        assert!(!visible.output.contains("[unset]"), "{}", visible.output);
        ctx.env_remove = vec!["CARGO_MANIFEST_DIR".into()];
        let r = Bash.run(&ctx, cmd).await;
        assert!(r.output.contains("[unset]"), "{}", r.output);
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
