//! PDF text, out of process (FIX_PLAN 3.4, AUDIT S7); office documents ride
//! the same supervisor and the same confinement (`crate::doc`, X.55, T96).
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
/// The same binary's subcommand for Word, Excel, PowerPoint and
/// OpenDocument files (`crate::doc`): same confinement, same limits.
pub const DOC_SUBCOMMAND: &str = "doc-text";
/// And the one that edits Word and Excel files (`crate::doc_edit`, X.57,
/// T98): a request and a file in, the new file out.
pub const EDIT_SUBCOMMAND: &str = "doc-edit";
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
/// `doc-text`: a zip that is not a readable office document (by its parts,
/// never by a name), or an encrypted one: nothing to say about it.
pub const EXIT_NOT_DOCUMENT: i32 = 5;
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
    /// The arguments of an office document's helper (`doc-text`).
    doc_args: Vec<OsString>,
    /// The arguments of the editing helper (`doc-edit`).
    edit_args: Vec<OsString>,
    slots: Arc<Semaphore>,
    /// One helper at a time per account (see [`Self::account_slot`]).
    accounts: std::sync::Mutex<std::collections::HashMap<String, Arc<Semaphore>>>,
    timeout: Duration,
    queue_wait: Duration,
}

impl PdfReader {
    /// The real helper: `exe pdf-text`.
    pub fn new(exe: PathBuf) -> Self {
        let mut r = Self::with_command(exe, vec![SUBCOMMAND.into()]);
        r.doc_args = vec![DOC_SUBCOMMAND.into()];
        r.edit_args = vec![EDIT_SUBCOMMAND.into()];
        r
    }

    /// Any command speaking the helper's protocol (tests use stand-ins).
    pub fn with_command(program: PathBuf, args: Vec<OsString>) -> Self {
        Self {
            program,
            doc_args: args.clone(),
            edit_args: args.clone(),
            args,
            slots: Arc::new(Semaphore::new(CONCURRENT)),
            accounts: Default::default(),
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

    /// The one helper an account may hold at a time: a second file of the
    /// same account waits here (at most the queue bound, then
    /// [`PdfError::Busy`]) without holding a server-wide slot, so one account
    /// cannot keep both slots busy and starve the others. Take it before
    /// [`Self::extract`] or [`Self::extract_doc`] and keep it until they end.
    pub async fn account_slot(
        &self,
        account: &str,
    ) -> Result<tokio::sync::OwnedSemaphorePermit, PdfError> {
        let sem = {
            let mut m = self.accounts.lock().unwrap_or_else(|e| e.into_inner());
            // Forget the accounts nobody holds or waits on.
            m.retain(|_, s| Arc::strong_count(s) > 1);
            m.entry(account.to_string())
                .or_insert_with(|| Arc::new(Semaphore::new(1)))
                .clone()
        };
        tokio::time::timeout(self.queue_wait, sem.acquire_owned())
            .await
            .map_err(|_| PdfError::Busy)?
            .map_err(|_| PdfError::Busy)
    }

    /// Free helper slots now (tests).
    pub fn free_slots(&self) -> usize {
        self.slots.available_permits()
    }

    /// The text of `pdf`, at most [`HELPER_CHARS`] characters.
    pub async fn extract(&self, pdf: Vec<u8>) -> Result<String, PdfError> {
        let (code, out) = self.run(&self.args, pdf).await?;
        match code {
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

    /// The helper, bounded: one of [`CONCURRENT`] slots (waiting at most
    /// the queue bound), its own process group, no environment, stdout
    /// capped, a wall-time limit. Returns its exit code and its stdout.
    pub(crate) async fn run(
        &self,
        args: &[OsString],
        input: Vec<u8>,
    ) -> Result<(Option<i32>, Vec<u8>), PdfError> {
        self.run_capped(args, input, MAX_OUTPUT).await
    }

    /// [`Self::run`] with its own bound on what the helper may write.
    pub(crate) async fn run_capped(
        &self,
        args: &[OsString],
        input: Vec<u8>,
        max_output: usize,
    ) -> Result<(Option<i32>, Vec<u8>), PdfError> {
        // The permit is released when this future ends or is dropped.
        let _slot = tokio::time::timeout(self.queue_wait, self.slots.clone().acquire_owned())
            .await
            .map_err(|_| PdfError::Busy)?
            .map_err(|_| PdfError::Busy)?;
        let mut cmd = tokio::process::Command::new(&self.program);
        cmd.args(args)
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
                let _ = stdin.write_all(&input).await;
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
                    if out.len() + n > max_output {
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
        Ok((status.code(), out))
    }

    /// A request and a Word or Excel file through `doc-edit` (X.57): the
    /// helper's reply, framed (`harness_docs::edit::serve`). The helper has
    /// its own bounds on what it reads and writes; this one holds the time,
    /// the output and the slots, as for every file the server reads.
    pub async fn edit(&self, input: Vec<u8>) -> Result<Vec<u8>, DocError> {
        let cap = harness_docs::limits::MAX_OUTPUT_BYTES
            + harness_docs::edit::serve::MAX_HEADER_BYTES
            + 4096;
        self.edit_capped(input, cap).await
    }

    /// [`Self::edit`] for a request whose answer is small (a preview): more
    /// output than `cap` is a failed helper, not something read.
    pub async fn edit_capped(&self, input: Vec<u8>, cap: usize) -> Result<Vec<u8>, DocError> {
        let (code, out) = self.run_capped(&self.edit_args, input, cap).await?;
        match code {
            Some(0) => Ok(out),
            Some(EXIT_UNREADABLE) => Err(DocError::Unreadable(message(&out))),
            Some(EXIT_UNCONFINED) => Err(DocError::Unconfined),
            _ => Err(DocError::Crashed),
        }
    }

    /// An office document's text through `doc-text`: `(mime, text)`, or why
    /// not. `Ok(None)`: the helper found it is not a document (not by its
    /// parts, or encrypted): it is whatever it was before.
    pub async fn extract_doc(&self, bytes: Vec<u8>) -> Result<Option<(String, String)>, DocError> {
        let (code, out) = self.run(&self.doc_args, bytes).await?;
        match code {
            Some(0) => {
                // The first line is the type the helper found; the rest is
                // the text. Anything else is a misbehaving helper.
                let text = String::from_utf8(out).map_err(|_| DocError::Crashed)?;
                let (mime, body) = text.split_once('\n').ok_or(DocError::Crashed)?;
                if crate::doc::Format::from_mime(mime).is_none() {
                    return Err(DocError::Crashed);
                }
                if body.trim().is_empty() {
                    return Err(DocError::NoText);
                }
                Ok(Some((mime.to_string(), body.to_string())))
            }
            Some(EXIT_NOT_DOCUMENT) => Ok(None),
            Some(EXIT_UNREADABLE) => Err(DocError::Unreadable(message(&out))),
            Some(EXIT_UNCONFINED) => Err(DocError::Unconfined),
            _ => Err(DocError::Crashed),
        }
    }
}

/// Why a document's text did not come back. `Display` is the sentence for
/// the user.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DocError {
    #[error("this server cannot read documents (the reader is not set up)")]
    NotSetUp,
    #[error(
        "this server cannot read documents safely: the reader needs Linux 6.7 or newer with \
         Landlock enabled on the server"
    )]
    Unconfined,
    #[error("the server is busy reading other files; try again in a minute")]
    Busy,
    #[error("this document took too long to read (over {} s)", TIMEOUT.as_secs())]
    Timeout,
    #[error("this document could not be read: {0}")]
    Unreadable(String),
    #[error("this document could not be read (it is malformed, or too large to read)")]
    Crashed,
    #[error("this document has no text")]
    NoText,
}

impl From<PdfError> for DocError {
    fn from(e: PdfError) -> Self {
        match e {
            PdfError::NotSetUp => Self::NotSetUp,
            PdfError::Unconfined => Self::Unconfined,
            PdfError::Busy => Self::Busy,
            PdfError::Timeout => Self::Timeout,
            PdfError::Unreadable(m) => Self::Unreadable(m),
            PdfError::Crashed => Self::Crashed,
            PdfError::NoText => Self::NoText,
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

/// What a helper's reader decides about its input.
#[derive(Debug)]
pub(crate) enum Verdict {
    /// Write this to stdout, exit 0.
    Text(Vec<u8>),
    /// Exit [`EXIT_UNREADABLE`] with this one line.
    Unreadable(String),
    /// Exit [`EXIT_NOT_DOCUMENT`].
    NotDocument,
    /// Exit [`EXIT_BAD_INPUT`].
    BadInput,
}

/// The helpers' common start: confine, limit memory, read at most
/// [`crate::attachments::MAX_UPLOAD`] bytes of stdin, hand them to `read`,
/// write what it says. Returns the exit code. Must run before any other
/// thread exists (Landlock confines the calling thread only).
pub(crate) fn confined_helper(read: impl FnOnce(&[u8]) -> Verdict) -> i32 {
    confined_helper_cap(crate::attachments::MAX_UPLOAD, read)
}

/// [`confined_helper`] reading at most `max` bytes of stdin.
pub(crate) fn confined_helper_cap(max: usize, read: impl FnOnce(&[u8]) -> Verdict) -> i32 {
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
    // No core dump of an uploaded file: the host's core pattern often pipes
    // to a handler outside every sandbox (systemd-coredump) that stores the
    // process image. Soft and hard limit 0 so nothing can raise it, and not
    // dumpable at all (as `sandbox-exec` does for a contained command).
    if nix::sys::resource::setrlimit(nix::sys::resource::Resource::RLIMIT_CORE, 0, 0).is_err()
        || nix::sys::prctl::set_dumpable(false).is_err()
    {
        return EXIT_UNCONFINED;
    }
    #[cfg(feature = "helper-probe")]
    if let Some(port) = std::env::var_os("HARNESS_HELPER_PROBE") {
        return probe(&port);
    }
    let mut input = Vec::new();
    let max = max as u64;
    if std::io::stdin()
        .lock()
        .take(max + 1)
        .read_to_end(&mut input)
        .is_err()
        || input.len() as u64 > max
    {
        return EXIT_BAD_INPUT;
    }
    let mut out = std::io::stdout().lock();
    match read(&input) {
        Verdict::Text(t) => {
            if out.write_all(&t).is_err() || out.flush().is_err() {
                return EXIT_BAD_INPUT;
            }
            0
        }
        Verdict::Unreadable(msg) => {
            let msg: String = msg.chars().take(MAX_MESSAGE).collect();
            let _ = out.write_all(msg.as_bytes());
            let _ = out.flush();
            EXIT_UNREADABLE
        }
        Verdict::NotDocument => EXIT_NOT_DOCUMENT,
        Verdict::BadInput => EXIT_BAD_INPUT,
    }
}

/// What a confined helper can still do, for the test that proves the
/// confinement (`crates/cli/tests/helper_confinement.rs`): after everything
/// above it tries to open a file that exists and to connect to the port it
/// was given, and says what it found and which limits it holds. Compiled
/// only with the `helper-probe` feature, which no shipped build has.
#[cfg(feature = "helper-probe")]
fn probe(port: &std::ffi::OsStr) -> i32 {
    use std::io::Write;
    let opened = std::fs::File::open("/etc/passwd").is_ok();
    let port: u16 = port.to_str().and_then(|p| p.parse().ok()).unwrap_or(1);
    let connected = std::net::TcpStream::connect(("127.0.0.1", port)).is_ok();
    let (soft, hard) =
        nix::sys::resource::getrlimit(nix::sys::resource::Resource::RLIMIT_CORE).unwrap_or((9, 9));
    let dumpable = nix::sys::prctl::get_dumpable().map_or(9, u8::from);
    let report = format!(
        "open {}\nconnect {}\ncore {soft} {hard}\ndumpable {dumpable}\n",
        if opened { "opened" } else { "refused" },
        if connected { "connected" } else { "refused" },
    );
    let _ = std::io::stdout().write_all(report.as_bytes());
    0
}

/// `rizzy-harness pdf-text`: read the PDF from stdin, write its text to
/// stdout. Returns the exit code.
pub fn helper_main() -> i32 {
    confined_helper(|pdf| {
        if !is_pdf(pdf) {
            return Verdict::BadInput;
        }
        match pdf_extract::extract_text_from_mem(pdf) {
            Ok(text) => {
                let end = text
                    .char_indices()
                    .nth(HELPER_CHARS)
                    .map_or(text.len(), |(i, _)| i);
                Verdict::Text(text.as_bytes()[..end].to_vec())
            }
            Err(e) => Verdict::Unreadable(e.to_string()),
        }
    })
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
        // Spins while this test lives: a SIGKILL of the test reparents it
        // and the loop ends (no drop guard runs then). Its child, the one
        // the group kill must end, lives while the test does too (it was a
        // `sleep 300` that outlived a killed test by five minutes).
        let r = sh(&format!(
            "( while kill -0 {pid} 2>/dev/null; do sleep 1; done ) & echo $! > {}; \
             while read -r _ _ _ p _ </proc/$$/stat && [ \"$p\" = {pid} ]; do :; done",
            pidfile.display(),
            pid = std::process::id()
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

    /// One helper per account at a time; another account is not held up, and
    /// a forgotten account leaves nothing behind.
    #[tokio::test]
    async fn an_account_holds_one_helper_slot_and_others_are_not_starved() {
        let r = sh("true").with_limits(Duration::from_secs(5), Duration::from_millis(200));
        let a = r.account_slot("alex").await.unwrap();
        // His second file waits its bound, then is told the server is busy.
        let t = std::time::Instant::now();
        assert_eq!(r.account_slot("alex").await.err(), Some(PdfError::Busy));
        assert!(t.elapsed() >= Duration::from_millis(150));
        // Bob is not waiting behind him, and no server-wide slot was used.
        let b = r.account_slot("bob").await.unwrap();
        assert_eq!(r.free_slots(), CONCURRENT);
        drop(a);
        let _again = r.account_slot("alex").await.unwrap();
        drop(b);
        // Accounts nobody holds are forgotten on the next call.
        let _c = r.account_slot("carol").await.unwrap();
        assert!(r.accounts.lock().unwrap().len() <= 2);
    }

    const DOCX: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

    /// The document mode speaks one more thing: the type on the first line.
    #[tokio::test]
    async fn a_document_helper_is_read_and_its_type_checked() {
        let r = sh(&format!("cat >/dev/null; printf '{DOCX}\\nhello\\n'"));
        assert_eq!(
            r.extract_doc(b"PK".to_vec()).await,
            Ok(Some((DOCX.to_string(), "hello\n".to_string())))
        );
        // Not a document: the helper says so by its exit code.
        let r = sh("cat >/dev/null; exit 5");
        assert_eq!(r.extract_doc(b"PK".to_vec()).await, Ok(None));
        let r = sh("cat >/dev/null; printf 'a part is missing'; exit 3");
        assert_eq!(
            r.extract_doc(b"PK".to_vec()).await,
            Err(DocError::Unreadable("a part is missing".into()))
        );
        let r = sh("exit 125");
        assert_eq!(
            r.extract_doc(b"PK".to_vec()).await,
            Err(DocError::Unconfined)
        );
        // A type the server does not know, or none, is a misbehaving helper.
        for out in ["text/evil\\nhello", "hello"] {
            let r = sh(&format!("cat >/dev/null; printf '{out}'"));
            assert_eq!(r.extract_doc(b"PK".to_vec()).await, Err(DocError::Crashed));
        }
        // A type and no text.
        let r = sh(&format!("cat >/dev/null; printf '{DOCX}\\n  \\n'"));
        assert_eq!(r.extract_doc(b"PK".to_vec()).await, Err(DocError::NoText));
        assert_eq!(r.free_slots(), CONCURRENT);
    }

    /// The same bounds as a PDF's: time, output and slots.
    #[tokio::test]
    async fn a_document_helper_has_the_same_bounds() {
        let r = sh("yes 'ab'").with_limits(Duration::from_secs(10), Duration::from_secs(1));
        assert_eq!(
            r.extract_doc(vec![b'P'; 1 << 20]).await,
            Err(DocError::Crashed)
        );
        let r = sh("sleep 300").with_limits(Duration::from_millis(300), Duration::from_millis(300));
        assert_eq!(r.extract_doc(b"PK".to_vec()).await, Err(DocError::Timeout));
        assert_eq!(r.free_slots(), CONCURRENT);
        let r = std::sync::Arc::new(
            sh("sleep 300").with_limits(Duration::from_secs(60), Duration::from_millis(300)),
        );
        let (a, b) = (r.clone(), r.clone());
        let ha = tokio::spawn(async move { a.extract_doc(b"PK".to_vec()).await });
        let hb = tokio::spawn(async move { b.extract_doc(b"PK".to_vec()).await });
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(r.extract_doc(b"PK".to_vec()).await, Err(DocError::Busy));
        ha.abort();
        hb.abort();
        let _ = ha.await;
        let _ = hb.await;
        assert_eq!(r.free_slots(), CONCURRENT);
    }

    /// The editing helper rides the same supervisor: time, the output cap
    /// (larger than a text's, a file can come back), the slots, the exit codes.
    #[tokio::test]
    async fn an_edit_helper_has_the_same_bounds() {
        let r = sh("cat >/dev/null; printf 'done'");
        assert_eq!(r.edit(b"x".to_vec()).await, Ok(b"done".to_vec()));
        // More than a text may be, less than a file may be: allowed here.
        let r = sh("cat >/dev/null; head -c 3000000 /dev/zero");
        assert_eq!(r.edit(b"x".to_vec()).await.map(|o| o.len()), Ok(3_000_000));
        let r = sh("yes 'ab'").with_limits(Duration::from_secs(10), Duration::from_secs(1));
        assert_eq!(r.edit(vec![b'P'; 1 << 20]).await, Err(DocError::Crashed));
        let r = sh("sleep 300").with_limits(Duration::from_millis(300), Duration::from_millis(300));
        assert_eq!(r.edit(b"x".to_vec()).await, Err(DocError::Timeout));
        let r = sh("cat >/dev/null; printf 'bad part'; exit 3");
        assert_eq!(
            r.edit(b"x".to_vec()).await,
            Err(DocError::Unreadable("bad part".into()))
        );
        let r = sh("exit 125");
        assert_eq!(r.edit(b"x".to_vec()).await, Err(DocError::Unconfined));
        assert_eq!(r.free_slots(), CONCURRENT);
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
