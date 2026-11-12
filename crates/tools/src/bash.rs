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

/// The `bash` timeout ceiling when node.toml does not set one (a call may
/// ask for less, never more), and the range node.toml may set it in.
pub const DEFAULT_TIMEOUT_MAX_S: u64 = 3600;
pub const MIN_TIMEOUT_MAX_S: u64 = 1;
pub const MAX_TIMEOUT_MAX_S: u64 = 3600;

/// Grace period between SIGTERM and SIGKILL.
const TERM_GRACE: Duration = Duration::from_secs(2);

/// SIGKILLs a process group when dropped while still armed. A contained
/// command needs none: dropping its [`ShellChild::Contained`] closes its
/// guard's channel, which is the guard's kill order (T.8e).
pub(crate) struct KillGroupOnDrop(pub(crate) Option<i32>);

impl Drop for KillGroupOnDrop {
    fn drop(&mut self) {
        if let Some(pgid) = self.0.take() {
            signal_group(pgid, Signal::SIGKILL);
        }
    }
}

/// A started shell command. A contained one is only ever held as
/// [`crate::contain::ContainedChild`]: its guard, and the channel whose
/// closing (on drop, or when the node dies) is the kill order (T.8e).
pub enum ShellChild {
    Plain(tokio::process::Child),
    Contained(crate::contain::ContainedChild),
}

impl std::ops::Deref for ShellChild {
    type Target = tokio::process::Child;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Plain(c) => c,
            Self::Contained(c) => c,
        }
    }
}

impl std::ops::DerefMut for ShellChild {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::Plain(c) => c,
            Self::Contained(c) => c,
        }
    }
}
/// Chunks (of at most 8 KiB) in flight from the pipe readers to the loop.
/// When it is full the readers stop reading and the child blocks on its
/// pipe: back-pressure, never unbounded memory.
const PIPE_CHANNEL: usize = 8;

pub(crate) fn signal_group(pgid: i32, sig: Signal) {
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

/// What [`crate::contain::sweep_session`] found, for the tool result.
fn sweep_note(found: &[crate::contain::Swept]) -> String {
    if found.is_empty() {
        return "the node found nothing of it outside a live guard".into();
    }
    let ended = found.iter().filter(|s| s.ended).count();
    let alive = found.len() - ended;
    let mut t = format!("the node ended {ended} namespace(s) it left");
    if alive > 0 {
        t.push_str(&format!(
            ", and {alive} is still alive after the kill (the kernel cannot end it yet)"
        ));
    }
    t
}

#[derive(Debug, PartialEq)]
enum End {
    Exited(Option<std::process::ExitStatus>),
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
        self.schema_for(Duration::from_secs(DEFAULT_TIMEOUT_MAX_S))
    }

    fn schema_for(&self, bash_timeout_max: Duration) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {"type": "string", "description": "The command to run"},
                "timeout_secs": {
                    "type": "integer",
                    "description": format!(
                        "Optional timeout in seconds (max {})",
                        bash_timeout_max.as_secs()
                    )
                }
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
        // The node's ceiling applies to what the call asks for and to the
        // default alike.
        let ceiling = ctx.bash_timeout_max.max(Duration::from_secs(1));
        let timeout = input
            .get("timeout_secs")
            .and_then(|v| v.as_u64())
            .map(|s| Duration::from_secs(s.max(1)))
            .unwrap_or(ctx.timeout)
            .min(ceiling);

        // The allow-listed environment (node.toml `env_pass`), nothing else,
        // in the Landlock sandbox where the node has one (node.toml
        // `[sandbox]`): read-write the session folder and its temp folder;
        // inside the node's process containment where it has one (T.8a).
        let (mut cmd, contained): (Command, bool) = match ctx.sandbox.shell(
            "bash".as_ref(),
            &["-c".as_ref(), command.as_ref()],
            &ctx.workspace,
            ctx.pin.as_ref(),
            &ctx.env,
            &ctx.protected,
            crate::sandbox::Grants {
                workspace_write: true,
                owned: ctx.owned.clone(),
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
        // The spawn is a `fork` and `exec` on this thread; a contained
        // command's guard starts in `/`, so it does not wait on the session's
        // folder (the folder was resolved above, in this task).
        let deadline = tokio::time::sleep(timeout);
        tokio::pin!(deadline);
        // What runs first: the node's own image (the guard, or the wrapper)
        // where the shell is confined, else bash.
        let program = cmd.as_std().get_program().to_owned();
        let mut child = match crate::sandbox::Confinement::spawn(cmd, contained).await {
            Ok(c) => c,
            Err(e) => {
                return ToolOutcome::err(
                    crate::sandbox::image_spawn_error(&program, &e)
                        .unwrap_or_else(|| format!("failed to spawn bash: {e}")),
                );
            }
        };
        let pgid = child.id().map(|p| p as i32);
        // If this call is dropped before it reaped the group itself (the
        // agent loop abandons a tool that outlives a cancel), the whole
        // group dies, not only the shell `kill_on_drop` covers. Contained,
        // the dropped channel is the guard's kill order.
        let mut group = KillGroupOnDrop(pgid.filter(|_| !contained));

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
                status = child.wait() => break End::Exited(status.ok()),
                _ = ctx.cancel.cancelled() => break End::Cancelled,
                _ = &mut deadline => break End::TimedOut,
            }
        };

        // Contained: the guard's report of the stop (T.8e). A command that
        // exited needs none: its guard exits only after it has reaped
        // everything the command started, unless it was killed itself.
        let mut stopped = None;
        let mut guard_note = None;
        if let ShellChild::Contained(c) = &mut child {
            use crate::guard::{By, Report};
            use std::os::unix::process::ExitStatusExt;
            let killed = match &end {
                End::Exited(Some(st)) => st.signal(),
                _ => None,
            };
            match (&end, killed) {
                (End::Cancelled | End::TimedOut, _) => {
                    // The graceful stop first (SIGTERM to the command's
                    // session, the guard's 2 s grace), then the kill, repeated
                    // until the guard has reaped the namespace or reports it
                    // still alive.
                    stopped = Some(c.stop(true).await);
                }
                (_, Some(sig)) => {
                    guard_note = Some(format!(
                        "[its guard was killed (signal {sig}) before it confirmed the end"
                    ));
                }
                _ => match c.exit_report().await {
                    Some(Report::Ended(_, By::Signal)) => {
                        guard_note = Some(
                            "[stopped: its guard was told to stop (a signal to the guard) and \
                             killed everything the command started; this is not the command's \
                             own exit]"
                                .into(),
                        );
                    }
                    Some(Report::Ended(_, By::Bwrap)) => {
                        guard_note = Some(
                            "[bubblewrap was killed by a signal before its command ended; \
                             the guard killed what was left]"
                                .into(),
                        );
                    }
                    _ => {}
                },
            }
            // The guard died, or did not answer: what it left is ended here
            // and now, not at the node's next start (review T8E-2). Only
            // orphans of this session; a namespace under a live guard (this
            // one's, if it is frozen; another command's) is never touched.
            let within = if killed.is_some() {
                Some(Duration::from_secs(2))
            } else if matches!(stopped, Some(crate::contain::Stopped::Unconfirmed(_))) {
                Some(crate::contain::SWEEP_IN_STOP)
            } else {
                None
            };
            if let (Some(within), Some(mark)) = (within, ctx.sandbox.mark()) {
                let found = tokio::task::spawn_blocking(move || {
                    crate::contain::sweep_session(&mark, within)
                })
                .await
                .unwrap_or_default();
                let what = sweep_note(&found);
                match &mut stopped {
                    Some(crate::contain::Stopped::Unconfirmed(why)) => {
                        why.push_str("; ");
                        why.push_str(&what);
                    }
                    _ => {
                        if let Some(n) = &mut guard_note {
                            n.push_str("; ");
                            n.push_str(&what);
                        }
                    }
                }
            }
            if let Some(n) = &mut guard_note
                && !n.ends_with(']')
            {
                n.push(']');
            }
        } else if let Some(pgid) = pgid {
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
        // A process still alive holds them: do not wait for it, so the
        // report reaches the loop inside its grace for a cancelled tool.
        let drain = match stopped {
            Some(crate::contain::Stopped::Ended(_)) | None => Duration::from_secs(2),
            Some(_) => crate::contain::STUCK_DRAIN,
        };
        let _ = tokio::time::timeout(drain, async {
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
            End::Exited(status) => {
                if output.is_empty() {
                    output = "(no output)".into();
                }
                if let Some(note) = guard_note {
                    // Not the command's own exit: no exit code to report.
                    return ToolOutcome {
                        ok: false,
                        output: format!("{output}\n{note}"),
                        truncated,
                        ..Default::default()
                    };
                }
                let code = status.and_then(|s| s.code());
                let code_str = code.map_or("signal".to_string(), |c| c.to_string());
                ToolOutcome {
                    ok: code == Some(0),
                    output: format!("{output}\n[exit code: {code_str}]"),
                    exit_code: code,
                    truncated,
                    ..Default::default()
                }
            }
            End::Cancelled | End::TimedOut => {
                let how = if end == End::Cancelled {
                    "cancelled".to_string()
                } else {
                    format!("timed out after {}s", timeout.as_secs())
                };
                let note = match stopped {
                    Some(crate::contain::Stopped::StillAlive(d)) => format!(
                        "[{} and killed, but NOT ended: a process of this command is still alive \
                         {:.1}s after the stop was ordered; the kernel cannot end it yet (it \
                         waits on I/O that does not return, as on a stalled network or FUSE \
                         file system). Its guard keeps killing it; it ends when that I/O \
                         returns]",
                        if end == End::Cancelled {
                            "stop ordered"
                        } else {
                            &how
                        },
                        d.as_secs_f64()
                    ),
                    Some(crate::contain::Stopped::Unconfirmed(why)) => format!(
                        "[{} and killed; the end is not confirmed: {why}]",
                        if end == End::Cancelled {
                            "stop ordered"
                        } else {
                            &how
                        }
                    ),
                    // Contained: a namespace, killed, and its end confirmed.
                    Some(crate::contain::Stopped::Ended(d)) => format!(
                        "[{how}: the command and everything it started were killed; its guard \
                         confirmed the end ({} ms)]",
                        d.as_millis()
                    ),
                    // Not contained: a process group, unconfirmed.
                    None => format!("[{how}: process group killed]"),
                };
                ToolOutcome {
                    ok: false,
                    output: format!("{output}\n{note}"),
                    truncated,
                    ..Default::default()
                }
            }
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
                // Zombie (`Z`) and dead (`X`, shown for a moment while it is
                // reaped) are both gone; reading `X` as alive made
                // `cancel_kills_process_tree` fail about 1 run in 100.
                let state = stat.rsplit(')').next().unwrap_or("").trim_start();
                !(state.starts_with('Z') || state.starts_with('X') || state.starts_with('x'))
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
        // The group got SIGKILL before the call returned; the orphaned
        // grandchild is then reaped by whoever adopts it (init or a
        // subreaper). Wait for that with a deadline, not a fixed pause.
        let deadline = Instant::now() + Duration::from_secs(10);
        while pid_alive(pid) && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(
            !pid_alive(pid),
            "grandchild {pid} survived cancel: {:?} {:?}",
            std::fs::read_to_string(format!("/proc/{pid}/stat")),
            std::fs::read_to_string(format!("/proc/{pid}/cmdline"))
        );
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

    // ---- Experiment B.1 (ADOPTION_PLAN): processes that leave the group ----
    //
    // The tool signals one process group. A process that calls `setsid` is in
    // another session and group, so these tests record what happens to it.
    // Every process a test expects to survive is recorded by pid and killed
    // by exactly that pid (after checking its command line) by `Reaper`.

    /// Kills one recorded pid on drop, only if `/proc/<pid>/cmdline` is still
    /// exactly `sleep <secs>` (so a recycled pid is never touched).
    struct Reaper {
        pid: i32,
        secs: u32,
    }

    fn is_our_sleep(pid: i32, secs: u32) -> bool {
        pid_alive(pid)
            && std::fs::read(format!("/proc/{pid}/cmdline"))
                .map(|c| c == format!("sleep\0{secs}\0").as_bytes())
                .unwrap_or(false)
    }

    impl Drop for Reaper {
        fn drop(&mut self) {
            if is_our_sleep(self.pid, self.secs) {
                let _ = nix::sys::signal::kill(Pid::from_raw(self.pid), Signal::SIGKILL);
            }
        }
    }

    /// Wait for the command to write its pid, and guard that pid.
    async fn recorded(pidfile: &std::path::Path, secs: u32) -> Reaper {
        for _ in 0..250 {
            if let Ok(s) = std::fs::read_to_string(pidfile)
                && let Ok(pid) = s.trim().parse::<i32>()
                && is_our_sleep(pid, secs)
            {
                return Reaper { pid, secs };
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("the command never recorded a `sleep {secs}` pid");
    }

    /// `sh -c 'echo $$ > FILE; exec sleep N'`: the pid in FILE is the sleep.
    fn recording_sleep(pidfile: &std::path::Path, secs: u32) -> String {
        format!("sh -c 'echo $$ > {}; exec sleep {secs}'", pidfile.display())
    }

    enum Stop {
        Cancel,
        Timeout,
        Drop,
    }

    /// Run `command`, wait until `secs`'s sleep is recorded, stop the call
    /// the given way, wait two seconds, and return the guard and whether the
    /// recorded process is still alive.
    async fn stop_and_look(
        command: String,
        pidfile: &std::path::Path,
        secs: u32,
        how: Stop,
    ) -> (Reaper, bool) {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolCtx::new(dir.path());
        let cancel = ctx.cancel.clone();
        let input = match how {
            Stop::Timeout => json!({"command": command, "timeout_secs": 1}),
            _ => json!({"command": command}),
        };
        let h = tokio::spawn(async move { Bash.run(&ctx, input).await });
        let reaper = recorded(pidfile, secs).await;
        match how {
            Stop::Cancel => {
                cancel.cancel();
                let r = h.await.unwrap();
                assert!(r.output.contains("cancelled"), "{}", r.output);
            }
            Stop::Timeout => {
                let r = h.await.unwrap();
                assert!(r.output.contains("timed out"), "{}", r.output);
            }
            Stop::Drop => {
                h.abort();
                let _ = h.await;
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
        let alive = is_our_sleep(reaper.pid, secs);
        (reaper, alive)
    }

    /// Control: a background child that stays in the group is gone.
    #[tokio::test]
    async fn b1_control_same_group_background_child_dies_on_cancel() {
        let pf = tempfile::tempdir().unwrap();
        let pidfile = pf.path().join("pid");
        let cmd = format!("{} & wait", recording_sleep(&pidfile, 987_601));
        let (_r, alive) = stop_and_look(cmd, &pidfile, 987_601, Stop::Cancel).await;
        assert!(!alive, "control failed: a same-group child survived");
    }

    /// Control: a process the test starts itself is alive after the same
    /// wait, so "alive" detection works.
    #[tokio::test]
    async fn b1_control_unrelated_process_is_detected_alive() {
        let mut c = std::process::Command::new("sleep")
            .arg("987602")
            .spawn()
            .unwrap();
        let r = Reaper {
            pid: c.id() as i32,
            secs: 987_602,
        };
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert!(is_our_sleep(r.pid, 987_602));
        drop(r);
        let _ = c.wait();
    }

    /// RESIDUAL WITHOUT CONTAINMENT (PLAN.md T9): a child that called
    /// `setsid` is not in the process group the tool signals, so it
    /// survives a cancel. These tests run the shell unconfined, as a node
    /// without bubblewrap does; contained, it dies (`cli/tests/contain.rs`).
    #[tokio::test]
    async fn a_setsid_child_survives_cancel() {
        let pf = tempfile::tempdir().unwrap();
        let pidfile = pf.path().join("pid");
        let cmd = format!("setsid {} & wait", recording_sleep(&pidfile, 987_603));
        let (_r, alive) = stop_and_look(cmd, &pidfile, 987_603, Stop::Cancel).await;
        assert!(alive, "the setsid child died: update PLAN.md T9");
    }

    /// RESIDUAL WITHOUT CONTAINMENT: same as above, ended by the timeout.
    #[tokio::test]
    async fn a_setsid_child_survives_timeout() {
        let pf = tempfile::tempdir().unwrap();
        let pidfile = pf.path().join("pid");
        let cmd = format!("setsid {} & wait", recording_sleep(&pidfile, 987_604));
        let (_r, alive) = stop_and_look(cmd, &pidfile, 987_604, Stop::Timeout).await;
        assert!(alive, "the setsid child died: update PLAN.md T9");
    }

    /// RESIDUAL WITHOUT CONTAINMENT: same as above, ended by dropping the call
    /// future (`KillGroupOnDrop` path).
    #[tokio::test]
    async fn a_setsid_child_survives_a_dropped_call() {
        let pf = tempfile::tempdir().unwrap();
        let pidfile = pf.path().join("pid");
        let cmd = format!("setsid {} & wait", recording_sleep(&pidfile, 987_605));
        let (_r, alive) = stop_and_look(cmd, &pidfile, 987_605, Stop::Drop).await;
        assert!(alive, "the setsid child died: update PLAN.md T9");
    }

    /// RESIDUAL WITHOUT CONTAINMENT: `setsid -f` (forks, parent exits at once).
    #[tokio::test]
    async fn a_setsid_f_child_survives_cancel() {
        let pf = tempfile::tempdir().unwrap();
        let pidfile = pf.path().join("pid");
        let cmd = format!(
            "setsid -f {}; sleep 1000",
            recording_sleep(&pidfile, 987_606)
        );
        let (_r, alive) = stop_and_look(cmd, &pidfile, 987_606, Stop::Cancel).await;
        assert!(alive, "the setsid -f child died: update PLAN.md T9");
    }

    /// RESIDUAL WITHOUT CONTAINMENT: a classic double fork (subshell in the
    /// background, then `setsid`), the shape of a daemon.
    #[tokio::test]
    async fn a_double_forked_setsid_daemon_survives_cancel() {
        let pf = tempfile::tempdir().unwrap();
        let pidfile = pf.path().join("pid");
        let cmd = format!(
            "( setsid nohup {} >/dev/null 2>&1 & ) ; sleep 1000",
            recording_sleep(&pidfile, 987_607)
        );
        let (_r, alive) = stop_and_look(cmd, &pidfile, 987_607, Stop::Cancel).await;
        assert!(alive, "the daemon died: update PLAN.md T9");
    }

    /// Control: a double fork with `nohup` but without `setsid` is still in
    /// the group, so it is killed. (`nohup` ignores SIGHUP only; it does not
    /// leave the group.)
    #[tokio::test]
    async fn b1_control_double_fork_with_nohup_only_dies_on_cancel() {
        let pf = tempfile::tempdir().unwrap();
        let pidfile = pf.path().join("pid");
        let cmd = format!(
            "( nohup {} >/dev/null 2>&1 & ) ; sleep 1000",
            recording_sleep(&pidfile, 987_608)
        );
        let (_r, alive) = stop_and_look(cmd, &pidfile, 987_608, Stop::Cancel).await;
        assert!(
            !alive,
            "control failed: a nohup child in the group survived"
        );
    }

    /// The node is SIGKILLed while an ordinary in-group command runs.
    ///
    /// Not the real node: the test re-runs this test binary as a helper
    /// process that drives the real `Bash` tool (the same code the node
    /// runs), then SIGKILLs that helper, the way `kill -9` on the node
    /// would. It does not prove anything about the node's supervisor, the
    /// session actor or a restart: only that nothing in `bash.rs` can clean
    /// up after a SIGKILL of its process.
    ///
    /// RESIDUAL WITHOUT CONTAINMENT (PLAN.md T9): the ordinary child
    /// survives. Contained, it dies with the node
    /// (`cli/tests/contain.rs`, `detached_children_end_when_the_node_is_killed`).
    #[tokio::test]
    async fn a_command_survives_sigkill_of_its_node() {
        const SECS: u32 = 987_609;
        let pf = tempfile::tempdir().unwrap();
        let pidfile = pf.path().join("pid");
        let mut helper = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "bash::tests::b1_helper_runs_a_long_command",
                "--nocapture",
            ])
            .env("B1_HELPER_PIDFILE", &pidfile)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let reaper = recorded(&pidfile, SECS).await;
        helper.kill().unwrap(); // SIGKILL
        let _ = helper.wait();
        tokio::time::sleep(Duration::from_secs(2)).await;
        let alive = is_our_sleep(reaper.pid, SECS);
        assert!(alive, "the command died with its node: update PLAN.md T9");
    }

    /// Not a test on its own: the body of the helper process above. Without
    /// the env var it returns at once.
    #[tokio::test]
    async fn b1_helper_runs_a_long_command() {
        let Some(pidfile) = std::env::var_os("B1_HELPER_PIDFILE") else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolCtx::new(dir.path());
        let cmd = recording_sleep(std::path::Path::new(&pidfile), 987_609);
        let _ = Bash.run(&ctx, json!({"command": cmd})).await;
    }
}
