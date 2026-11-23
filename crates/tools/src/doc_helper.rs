//! Running the confined `rizzy-harness doc-edit` helper from a node (X.57,
//! PLAN.md T98) for a chat with a folder: the node's own binary, started with
//! an empty environment in its own process group, the file on its stdin, its
//! stdout capped, a wall-time limit, at most two at once on the node, the
//! group killed when the call ends any other way. The file is parsed in that
//! process only, after its Landlock confinement (`crates/master/src/pdf.rs`
//! `confined_helper`); a node that cannot confine it (no Landlock) does not
//! edit, it says so.
//!
//! The server's own copy of this supervision is `PdfReader::run_capped`; the
//! two are separate crates' code over the same exit codes.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use harness_core::docs::EditHelper;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Semaphore;

/// The helper's subcommand.
pub const SUBCOMMAND: &str = "doc-edit";
/// Helpers running at once on a node.
pub const CONCURRENT: usize = 2;
/// How long a call waits for a free place.
pub const QUEUE_WAIT: Duration = Duration::from_secs(10);
/// Wall time of one run, start to exit.
pub const TIMEOUT: Duration = Duration::from_secs(20);
/// Bytes of stdout read at most: the largest file and its reply header.
pub const MAX_OUTPUT: usize =
    harness_docs::limits::MAX_OUTPUT_BYTES + harness_docs::edit::serve::MAX_HEADER_BYTES + 4096;
const EXIT_UNREADABLE: i32 = 3;
/// The sentence of a call that found no free place; callers that must tell
/// "busy" from a damaged file compare with it, not with a guess.
pub const BUSY: &str = "this node is busy editing other files; try again in a minute";
/// The sentence of a node that cannot confine the helper.
pub const UNCONFINED: &str = "this node cannot edit documents safely: the editor needs Linux 6.7 or newer with Landlock enabled";

pub struct ExeHelper {
    program: PathBuf,
    args: Vec<OsString>,
    slots: Arc<Semaphore>,
    timeout: Duration,
    queue_wait: Duration,
}

impl ExeHelper {
    /// `exe doc-edit`.
    pub fn new(exe: PathBuf) -> Self {
        Self::with_command(exe, vec![SUBCOMMAND.into()])
    }

    /// Any command speaking the helper's protocol (tests use stand-ins).
    pub fn with_command(program: PathBuf, args: Vec<OsString>) -> Self {
        Self {
            program,
            args,
            slots: Arc::new(Semaphore::new(CONCURRENT)),
            timeout: TIMEOUT,
            queue_wait: QUEUE_WAIT,
        }
    }

    pub fn with_limits(mut self, timeout: Duration, queue_wait: Duration) -> Self {
        self.timeout = timeout;
        self.queue_wait = queue_wait;
        self
    }

    pub fn free_slots(&self) -> usize {
        self.slots.available_permits()
    }
}

/// Kills the helper's process group unless it was reaped.
struct Group(Option<i32>);

impl Drop for Group {
    fn drop(&mut self) {
        if let Some(pgid) = self.0 {
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(pgid),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
    }
}

fn message(out: &[u8]) -> String {
    let s: String = String::from_utf8_lossy(out)
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(300)
        .collect();
    let s = s.trim();
    if s.is_empty() {
        "unknown error".into()
    } else {
        s.to_string()
    }
}

#[async_trait::async_trait]
impl EditHelper for ExeHelper {
    async fn run(&self, input: Vec<u8>) -> Result<Vec<u8>, String> {
        let _slot = tokio::time::timeout(self.queue_wait, self.slots.clone().acquire_owned())
            .await
            .map_err(|_| BUSY.to_string())?
            .map_err(|_| BUSY.to_string())?;
        let mut cmd = tokio::process::Command::new(&self.program);
        cmd.args(&self.args)
            .env_clear()
            .current_dir("/")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            // A parser's panic text may quote the file: never read.
            .stderr(std::process::Stdio::null())
            .process_group(0)
            .kill_on_drop(true);
        let mut child = cmd
            .spawn()
            .map_err(|_| "this node cannot start the document editor".to_string())?;
        let mut group = Group(child.id().and_then(|p| i32::try_from(p).ok()));
        let mut stdin = child.stdin.take().ok_or("the document editor failed")?;
        let mut stdout = child.stdout.take().ok_or("the document editor failed")?;
        let run = async {
            let write = async move {
                let _ = stdin.write_all(&input).await;
                drop(stdin);
                Ok::<(), String>(())
            };
            let read = async {
                let mut out = Vec::new();
                let mut buf = [0u8; 16 * 1024];
                loop {
                    let n = stdout
                        .read(&mut buf)
                        .await
                        .map_err(|_| "the document editor failed".to_string())?;
                    if n == 0 {
                        return Ok(out);
                    }
                    if out.len() + n > MAX_OUTPUT {
                        return Err("the document editor wrote more than it may".to_string());
                    }
                    out.extend_from_slice(&buf[..n]);
                }
            };
            let ((), out) = tokio::try_join!(write, read)?;
            let status = child
                .wait()
                .await
                .map_err(|_| "the document editor failed".to_string())?;
            Ok::<_, String>((status, out))
        };
        let (status, out) = tokio::time::timeout(self.timeout, run)
            .await
            .map_err(|_| {
                format!(
                    "editing this file took too long (over {} s) and was stopped",
                    self.timeout.as_secs()
                )
            })??;
        group.0 = None;
        match status.code() {
            Some(0) => Ok(out),
            Some(EXIT_UNREADABLE) => Err(message(&out)),
            Some(crate::sandbox::EXIT_UNCONFINED) => Err(UNCONFINED.into()),
            _ => Err("this file could not be edited (it is malformed, or too large)".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(script: &str) -> ExeHelper {
        ExeHelper::with_command(
            "/bin/sh".into(),
            vec!["-c".into(), script.into(), "sh".into()],
        )
    }

    #[tokio::test]
    async fn the_helpers_answers_and_failures_are_sentences() {
        assert_eq!(
            sh("cat >/dev/null; printf ok").run(vec![1]).await,
            Ok(b"ok".to_vec())
        );
        assert_eq!(
            sh("cat >/dev/null; printf 'a part is bad'; exit 3")
                .run(vec![1])
                .await,
            Err("a part is bad".into())
        );
        assert!(
            sh("exit 125")
                .run(vec![1])
                .await
                .unwrap_err()
                .contains("Landlock")
        );
        assert!(
            sh("kill -ABRT $$")
                .run(vec![1])
                .await
                .unwrap_err()
                .contains("malformed")
        );
        let h = sh("yes ab").with_limits(Duration::from_secs(10), Duration::from_secs(1));
        assert!(
            h.run(vec![b'x'; 1 << 20])
                .await
                .unwrap_err()
                .contains("more than it may")
        );
        let h = sh("sleep 300").with_limits(Duration::from_millis(300), Duration::from_millis(300));
        assert!(h.run(vec![1]).await.unwrap_err().contains("too long"));
        assert_eq!(h.free_slots(), CONCURRENT);
    }

    /// A third call waits its bound, then is told the node is busy; a call
    /// dropped mid-way frees its place.
    #[tokio::test]
    async fn two_at_once_then_busy_and_a_dropped_call_frees_its_place() {
        let h = Arc::new(
            sh("sleep 300").with_limits(Duration::from_secs(60), Duration::from_millis(300)),
        );
        let (a, b) = (h.clone(), h.clone());
        let ta = tokio::spawn(async move { a.run(vec![1]).await });
        let tb = tokio::spawn(async move { b.run(vec![1]).await });
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(h.run(vec![1]).await.unwrap_err().contains("busy"));
        ta.abort();
        tb.abort();
        let _ = ta.await;
        let _ = tb.await;
        assert_eq!(h.free_slots(), CONCURRENT);
    }
}
