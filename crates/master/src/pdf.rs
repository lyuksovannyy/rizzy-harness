//! PDF text, out of process (FIX_PLAN 3.4, AUDIT S7).
//!
//! The parser (`pdf-extract` → `lopdf` → `ttf-parser`) never runs inside the
//! server: the process that holds every provider key and controls every node.
//! The server starts its own binary as `rizzy-harness pdf-text`
//! ([`helper_main`]), writes the PDF to its stdin and reads the text from its
//! stdout ([`PdfReader`]).
//!
//! The helper, before it reads a byte: confines itself with Landlock to no
//! filesystem access at all and no TCP (it fails closed where the kernel
//! cannot), then caps its own address space at [`MEMORY_HEADROOM`] above what
//! it uses at start. It is single-threaded (dispatched before any runtime
//! starts) and gets an empty environment.
//!
//! The server: at most [`CONCURRENT`] helpers at once (a request waits at most
//! [`QUEUE_WAIT`] for a slot, then gets 503), [`TIMEOUT`] per file, stdout
//! capped at [`MAX_OUTPUT`] bytes and stderr discarded; the helper runs in its
//! own process group, which is killed on timeout, on excess output and when
//! the request is dropped (client gone). Nothing about the file (name, bytes,
//! text, the parser's message) is logged.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Semaphore;

/// The helper's subcommand.
pub const SUBCOMMAND: &str = "pdf-text";
/// Helpers running at once, server-wide.
pub const CONCURRENT: usize = 2;
/// How long a request waits for a free helper slot.
pub const QUEUE_WAIT: Duration = Duration::from_secs(10);
/// Wall time for one file, start to exit.
pub const TIMEOUT: Duration = Duration::from_secs(20);
/// Address space the helper may add to what it has mapped at start. Input is
/// at most 10 MiB; real documents of that size parse in well under 100 MiB.
/// Two helpers at once stay under 1 GiB even on a small VPS.
pub const MEMORY_HEADROOM: u64 = 512 * 1024 * 1024;
/// Characters the helper writes (one more than is kept, so the server can
/// tell the text was cut).
pub const HELPER_CHARS: usize = crate::attachments::MAX_TEXT_CHARS + 1;
/// Bytes the server reads from the helper's stdout: [`HELPER_CHARS`] in
/// UTF-8 at worst. More means a misbehaving helper: it is killed.
pub const MAX_OUTPUT: usize = HELPER_CHARS * 4;
/// Error text the helper may write on exit [`EXIT_UNREADABLE`].
const MAX_MESSAGE: usize = 300;

/// Helper exit codes.
pub const EXIT_UNREADABLE: i32 = 3;
pub const EXIT_BAD_INPUT: i32 = 4;
/// Same as `sandbox-exec`: it could not confine itself, nothing was parsed.
pub const EXIT_UNCONFINED: i32 = harness_tools::sandbox::EXIT_UNCONFINED;

/// A file is a PDF only when it starts with `%PDF-` at offset 0. Readers that
/// look further (Acrobat allows junk in the first 1024 bytes) are not
/// followed: such a file is judged by its other bytes (text if it is UTF-8).
pub fn is_pdf(bytes: &[u8]) -> bool {
    bytes.starts_with(b"%PDF-")
}

/// Why no text came back. `Display` is the message for the user.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PdfError {
    #[error("this server cannot read PDFs (the PDF reader is not set up)")]
    NotSetUp,
    #[error(
        "this server cannot read PDFs safely: the PDF reader needs Linux 6.7 or newer with \
         Landlock enabled on the server"
    )]
    Unconfined,
    #[error("the server is busy reading other PDFs; try again in a minute")]
    Busy,
    #[error("this PDF took too long to read (over {} s)", TIMEOUT.as_secs())]
    Timeout,
    #[error("this PDF could not be read: {0}")]
    Unreadable(String),
    #[error("this PDF could not be read (it is malformed, or too large to read)")]
    Crashed,
    #[error("this PDF has no text layer (scanned images are not supported yet)")]
    NoText,
}

/// The server side: starts the helper, bounded.
pub struct PdfReader {
    program: PathBuf,
    args: Vec<OsString>,
    slots: Arc<Semaphore>,
    timeout: Duration,
    queue_wait: Duration,
}

impl PdfReader {
    /// The real helper: `exe pdf-text`.
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

    /// Shorter bounds (tests).
    pub fn with_limits(mut self, timeout: Duration, queue_wait: Duration) -> Self {
        self.timeout = timeout;
        self.queue_wait = queue_wait;
        self
    }

    /// Free helper slots now (tests).
    pub fn free_slots(&self) -> usize {
        self.slots.available_permits()
    }

    /// The text of `pdf`, at most [`HELPER_CHARS`] characters.
    pub async fn extract(&self, pdf: Vec<u8>) -> Result<String, PdfError> {
        // The permit is released when this future ends or is dropped.
        let _slot = tokio::time::timeout(self.queue_wait, self.slots.clone().acquire_owned())
            .await
            .map_err(|_| PdfError::Busy)?
            .map_err(|_| PdfError::Busy)?;
        let mut cmd = tokio::process::Command::new(&self.program);
        cmd.args(&self.args)
            .env_clear()
            .current_dir("/")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            // The parser's panic text may quote the file: never read, never logged.
            .stderr(std::process::Stdio::null())
            .process_group(0)
            .kill_on_drop(true);
        let mut child = cmd.spawn().map_err(|_| PdfError::NotSetUp)?;
        let mut group = Group(child.id().and_then(|p| i32::try_from(p).ok()));
        let mut stdin = child.stdin.take().ok_or(PdfError::Crashed)?;
        let mut stdout = child.stdout.take().ok_or(PdfError::Crashed)?;
        let run = async {
            let write = async move {
                // A helper that stops reading makes this fail or block; the
                // timeout (or an error from `read`) ends the block, a broken
                // pipe is fine.
                let _ = stdin.write_all(&pdf).await;
                drop(stdin);
                Ok::<(), PdfError>(())
            };
            let read = async {
                let mut out = Vec::new();
                let mut buf = [0u8; 16 * 1024];
                loop {
                    let n = stdout.read(&mut buf).await.map_err(|_| PdfError::Crashed)?;
                    if n == 0 {
                        return Ok(out);
                    }
                    if out.len() + n > MAX_OUTPUT {
                        return Err(PdfError::Crashed);
                    }
                    out.extend_from_slice(&buf[..n]);
                }
            };
            // Excess output stops at once, even while the write blocks.
            let ((), out) = tokio::try_join!(write, read)?;
            let status = child.wait().await.map_err(|_| PdfError::Crashed)?;
            Ok::<_, PdfError>((status, out))
        };
        let (status, out) = tokio::time::timeout(self.timeout, run)
            .await
            .map_err(|_| PdfError::Timeout)??;
        // Reaped: the group id may be reused from here on.
        group.0 = None;
        match status.code() {
            Some(0) => {
                let text = String::from_utf8(out).map_err(|_| PdfError::Crashed)?;
                if text.trim().is_empty() {
                    Err(PdfError::NoText)
                } else {
                    Ok(text)
                }
            }
            Some(EXIT_UNREADABLE) => Err(PdfError::Unreadable(message(&out))),
            Some(EXIT_UNCONFINED) => Err(PdfError::Unconfined),
            _ => Err(PdfError::Crashed),
        }
    }
}

/// Kills the helper's process group unless it was reaped (timeout, excess
/// output, a dropped request). `kill_on_drop` then lets tokio reap the child.
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

/// The helper's error text, one short line of printable characters.
fn message(out: &[u8]) -> String {
    let s: String = String::from_utf8_lossy(out)
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(MAX_MESSAGE)
        .collect();
    let s = s.trim();
    if s.is_empty() {
        "unknown error".into()
    } else {
        s.to_string()
    }
}

/// `rizzy-harness pdf-text`: confine, limit memory, read the PDF from stdin,
/// write its text to stdout. Returns the exit code. Must run before any other
/// thread exists (Landlock confines the calling thread only).
pub fn helper_main() -> i32 {
    use std::io::{Read, Write};
    // What is mapped now (the binary, its libraries), read before /proc is
    // closed by the sandbox.
    let baseline = mapped_bytes();
    let confined = harness_tools::sandbox::restrict_self(&harness_tools::sandbox::ExecRequest {
        no_network: true,
        ..Default::default()
    });
    if confined.is_err() {
        return EXIT_UNCONFINED;
    }
    if limit_memory(baseline.saturating_add(MEMORY_HEADROOM)).is_err() {
        return EXIT_UNCONFINED;
    }
    let mut pdf = Vec::new();
    let max = crate::attachments::MAX_UPLOAD as u64;
    if std::io::stdin()
        .lock()
        .take(max + 1)
        .read_to_end(&mut pdf)
        .is_err()
        || pdf.len() as u64 > max
        || !is_pdf(&pdf)
    {
        return EXIT_BAD_INPUT;
    }
    let mut out = std::io::stdout().lock();
    match pdf_extract::extract_text_from_mem(&pdf) {
        Ok(text) => {
            let end = text
                .char_indices()
                .nth(HELPER_CHARS)
                .map_or(text.len(), |(i, _)| i);
            if out.write_all(&text.as_bytes()[..end]).is_err() || out.flush().is_err() {
                return EXIT_BAD_INPUT;
            }
            0
        }
        Err(e) => {
            let msg: String = e.to_string().chars().take(MAX_MESSAGE).collect();
            let _ = out.write_all(msg.as_bytes());
            let _ = out.flush();
            EXIT_UNREADABLE
        }
    }
}

/// Virtual memory mapped by this process now (`VmSize`), 0 if unknown.
fn mapped_bytes() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("VmSize:"))
                .and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<u64>().ok())
        })
        .map_or(0, |kb| kb * 1024)
}

/// Cap this process's address space (`RLIMIT_AS`, soft and hard): an
/// allocation past it fails, and Rust aborts this process, not the server.
fn limit_memory(bytes: u64) -> Result<(), nix::Error> {
    use nix::sys::resource::{Resource, setrlimit};
    setrlimit(Resource::RLIMIT_AS, bytes, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(script: &str) -> PdfReader {
        PdfReader::with_command(
            "/bin/sh".into(),
            vec!["-c".into(), script.into(), "sh".into()],
        )
    }

    const PDF: &[u8] = b"%PDF-1.4\n";

    #[test]
    fn the_header_is_at_offset_zero_only() {
        assert!(is_pdf(b"%PDF-1.7\n..."));
        assert!(!is_pdf(b"%PDF"));
        assert!(!is_pdf(b" %PDF-1.7"));
        assert!(!is_pdf(b"\xEF\xBB\xBF%PDF-1.7"));
        assert!(!is_pdf(b"notes about %PDF-1.7"));
    }

    #[tokio::test]
    async fn a_well_behaved_helper_is_read() {
        let r = sh("cat >/dev/null; printf 'hello'");
        assert_eq!(r.extract(PDF.to_vec()).await.unwrap(), "hello");
        let r = sh("cat >/dev/null; printf 'bad xref'; exit 3");
        assert_eq!(
            r.extract(PDF.to_vec()).await,
            Err(PdfError::Unreadable("bad xref".into()))
        );
        let r = sh("exit 125");
        assert_eq!(r.extract(PDF.to_vec()).await, Err(PdfError::Unconfined));
        let r = sh("kill -ABRT $$");
        assert_eq!(r.extract(PDF.to_vec()).await, Err(PdfError::Crashed));
        assert_eq!(r.free_slots(), CONCURRENT);
    }

    /// The supervision of a parser that never finishes: a stand-in that
    /// ignores stdin and spins (and has a child of its own) is killed with
    /// its whole group at the timeout, and its slot comes back.
    #[tokio::test]
    async fn a_looping_helper_is_killed_with_its_group() {
        let dir = tempfile::tempdir().unwrap();
        let pidfile = dir.path().join("pid");
        let r = sh(&format!(
            "sleep 300 & echo $! > {}; while :; do :; done",
            pidfile.display()
        ))
        .with_limits(Duration::from_millis(700), Duration::from_secs(1));
        let t = std::time::Instant::now();
        // More than a pipe buffer, so the write blocks too.
        assert_eq!(r.extract(vec![b'%'; 1 << 20]).await, Err(PdfError::Timeout));
        assert!(t.elapsed() < Duration::from_secs(3));
        assert_eq!(r.free_slots(), CONCURRENT);
        let pid: i32 = std::fs::read_to_string(&pidfile)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!(gone(pid).await, "the helper's child survived");
    }

    /// Unbounded output is cut off at [`MAX_OUTPUT`] and the helper killed.
    #[tokio::test]
    async fn unbounded_output_is_cut_off() {
        let r = sh("yes 'ab'").with_limits(Duration::from_secs(10), Duration::from_secs(1));
        let t = std::time::Instant::now();
        assert_eq!(r.extract(PDF.to_vec()).await, Err(PdfError::Crashed));
        // Also while the helper never reads its (larger than a pipe) input.
        assert_eq!(r.extract(vec![b'%'; 1 << 20]).await, Err(PdfError::Crashed));
        assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
        assert_eq!(r.free_slots(), CONCURRENT);
    }

    /// Two at once; a third waits for its bound, then is told the server is
    /// busy. A request dropped mid-way (client gone) kills its helper and
    /// frees its slot.
    #[tokio::test]
    async fn two_at_once_then_busy_and_cancel_frees_the_slot() {
        let dir = tempfile::tempdir().unwrap();
        let script = format!(
            "echo $$ >> {}; sleep 300",
            dir.path().join("pids").display()
        );
        let r =
            Arc::new(sh(&script).with_limits(Duration::from_secs(60), Duration::from_millis(300)));
        let (a, b) = (r.clone(), r.clone());
        let ha = tokio::spawn(async move { a.extract(PDF.to_vec()).await });
        let hb = tokio::spawn(async move { b.extract(PDF.to_vec()).await });
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(r.free_slots(), 0);
        assert_eq!(r.extract(PDF.to_vec()).await, Err(PdfError::Busy));
        ha.abort();
        hb.abort();
        let _ = ha.await;
        let _ = hb.await;
        assert_eq!(r.free_slots(), CONCURRENT);
        let pids = std::fs::read_to_string(dir.path().join("pids")).unwrap();
        for pid in pids.lines() {
            assert!(
                gone(pid.trim().parse().unwrap()).await,
                "helper survived cancel"
            );
        }
    }

    #[tokio::test]
    async fn the_helper_gets_no_environment() {
        // The server's environment holds provider keys; none may reach it.
        let r = sh(
            "cat >/dev/null; env | grep -v '^PWD=' | grep -v '^SHLVL=' | grep -v '^_='; printf .",
        );
        assert_eq!(r.extract(PDF.to_vec()).await.unwrap(), ".");
    }

    /// True once `pid` no longer runs (gone, or a zombie nobody reaped yet).
    async fn gone(pid: i32) -> bool {
        for _ in 0..50 {
            match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
                Err(_) => return true,
                Ok(s)
                    if s.rsplit(')')
                        .next()
                        .is_some_and(|r| r.trim_start().starts_with('Z')) =>
                {
                    return true;
                }
                Ok(_) => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        }
        false
    }
}
