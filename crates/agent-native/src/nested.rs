//! Instruction files of subfolders (ADOPTION_PLAN B.9, PLAN.md T65): the
//! `AGENTS.md` (or, in a folder without one, `CLAUDE.md`) of each folder
//! strictly between a file a call touched and the session's working
//! folder. The node reads the working folder's own file and those above it
//! at every turn's start (`crates/node/src/instructions.rs`); a file in a
//! folder below it (`web/AGENTS.md`) is loaded here, by the loop, the first
//! time a successful `read`, `edit` or `write` touches a file under that
//! folder, or the person's message mentions one (`@web/x.ts`, T63). That is
//! what Claude Code, OpenCode and DeepSeek's harness do
//! (research/round1/claude-code.md 1266-1296, research/round2/opencode.agents.md
//! 96-104, research/round2/deepseek-harness.agents.md 178-190).
//!
//! What is read follows the node's rules for the repository's files: only a
//! regular file whose real path is inside the working folder, in no
//! protected folder (`harness_tools::inside::read_inside`: a link is followed
//! only to such a file, the opened descriptor is checked again, a FIFO is
//! never waited on, at most the cap is read), never inside `.git`. Every line
//! of a file is indented by two spaces under a label line of the harness's,
//! so no file can forge another's label; the label names the path as one
//! line in quotation marks. A file that changed during the turn (by the
//! agent's own `write`, a command, or anyone) is not taken as instructions in
//! that turn: the person has not seen that turn yet (the node's reader takes
//! the working folder's own file at the next turn's start, the same rule).
//!
//! Only reading is here; what is sent, recorded and rebuilt is the loop's
//! (`context`, [`crate::provider::ChatMsg::StepContext`]).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// The file names, in order of preference within one folder (as the
/// node's reader).
pub const NAMES: [&str; 2] = ["AGENTS.md", "CLAUDE.md"];

/// The most instruction files of subfolders the history holds at once
/// (since the last summary). One more is named, not loaded.
pub const MAX_FILES: usize = 16;

/// The most text all of them give together, as sent (indentation
/// included): the same as the repository's own files (Codex's 32 KiB), so
/// the repository's text as a whole stays within DeepSeek's 64 KiB chain
/// budget (research/harnesses-round-2-appendix.md §4.9).
pub const TOTAL: usize = 32 * 1024;

/// How long a turn waits for one look (a hung network disk must not hold
/// the turn), as for the node's reader.
pub const READ_TIMEOUT: Duration = Duration::from_secs(5);

/// How much earlier than the turn's start a change still counts as made
/// during the turn. The kernel stamps a file's times from a coarse clock
/// (one tick: 1 to 10 ms) that can read a little behind the one the loop
/// takes the start from; 50 ms covers a few ticks, and a file the person
/// saved in those 50 ms before sending only waits one turn.
pub const CLOCK_MARGIN: Duration = Duration::from_millis(50);

/// Whether `m`'s inode change time (`ctime`, which no process can set) is
/// after `since`. A file system without one (its `ctime` is 0, or before
/// 1970) says no.
fn changed_after(m: &std::fs::Metadata, since: SystemTime) -> bool {
    use std::os::unix::fs::MetadataExt;
    let (Ok(secs), Ok(nanos)) = (u64::try_from(m.ctime()), u32::try_from(m.ctime_nsec())) else {
        return false;
    };
    if secs == 0 {
        return false;
    }
    std::time::UNIX_EPOCH + Duration::new(secs, nanos) > since
}

/// The caps for a model whose context window is `window` tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    /// All files together: [`TOTAL`], or about a tenth of the window (4
    /// bytes a token) when that is smaller, as the node's `cap_for`.
    pub total: usize,
    /// One file: half of `total` (16 KiB: OpenClaw's and Hermes' 20,000
    /// characters a file, research/harnesses-round-2-appendix.md §4.9). A
    /// longer one is cut, with a note to `read` the rest (Claude Code).
    pub file: usize,
    pub max_files: usize,
}

impl Budget {
    pub fn for_window(window: Option<u64>) -> Self {
        let total = match window {
            Some(w) => TOTAL.min(usize::try_from(w.saturating_mul(4) / 10).unwrap_or(TOTAL)),
            None => TOTAL,
        };
        Budget {
            total,
            file: total / 2,
            max_files: MAX_FILES,
        }
    }

    /// Why a file whose text is `len` bytes is not loaded beside `live`
    /// (paths and sizes of those the history holds); `None`: it fits.
    pub fn refusal(&self, live: &[(String, usize)], len: usize) -> Option<String> {
        if live.len() >= self.max_files {
            return Some(format!(
                "{} instruction files of subfolders are in this conversation already, the most it \
                 holds",
                self.max_files
            ));
        }
        let used: usize = live.iter().map(|(_, n)| n).sum();
        (used + len > self.total).then(|| {
            format!(
                "the instruction files of subfolders in this conversation would pass their {} \
                 bytes together",
                self.total
            )
        })
    }
}

/// `path` as one line in quotation marks (a folder's name is the
/// repository's text): JSON's quoting, and every line break a reader may
/// show as one escaped too.
pub fn path_label(path: &str) -> String {
    serde_json::to_string(path)
        .unwrap_or_else(|_| "\"?\"".into())
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
        .replace('\u{85}', "\\u0085")
}

/// The label line of a file's section.
fn label(path: &str) -> String {
    format!("==> {} <==", path_label(path))
}

/// The section of a file named, not loaded, with `why`.
pub fn named(path: &str, why: &str) -> String {
    format!(
        "{} (not loaded: {why}; read it with `read` if you need it)",
        label(path)
    )
}

/// The section of a file that changed during this turn: not taken yet.
pub fn changed_now(path: &str) -> String {
    format!(
        "{} (not taken as instructions yet: it changed during this turn; it applies when you \
         work under its folder in a later turn)",
        label(path)
    )
}

/// A file's path from the working folder: `rel` is its folder's.
fn file_path(rel: &Path, name: &str) -> Option<String> {
    rel.join(name).to_str().map(str::to_string)
}

/// The folders strictly between the working folder `ws` (canonical) and
/// the file whose real path is `real`, outermost first, each as a path from
/// `ws`. None for a file outside `ws` (outside mode) or inside a `.git`.
pub fn folders(real: &Path, ws: &Path) -> Vec<PathBuf> {
    let Ok(rel) = real.strip_prefix(ws) else {
        return vec![];
    };
    let Some(dir) = rel.parent() else {
        return vec![];
    };
    if dir
        .components()
        .any(|c| !matches!(c, std::path::Component::Normal(n) if n != ".git"))
    {
        return vec![];
    }
    let mut out: Vec<PathBuf> = dir
        .ancestors()
        .filter(|d| !d.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .collect();
    out.reverse();
    out
}

/// What one folder's file gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Got {
    /// Its section: the label, then every line indented (cut at the file
    /// cap with a note), or the label and why it was not read.
    Text(String),
    /// It changed during this turn ([`changed_now`]).
    Changed,
}

/// One folder's instruction file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    /// Its path from the working folder (`web/AGENTS.md`).
    pub path: String,
    pub got: Got,
}

/// What a look needs: the working folder (canonical), the protected
/// folders (canonical), the cap for one file, and what counts as changed
/// during this turn (the turn's start, the files its calls wrote).
#[derive(Debug, Clone)]
pub struct Ask {
    pub ws: PathBuf,
    pub protected: std::sync::Arc<Vec<PathBuf>>,
    pub file_cap: usize,
    pub since: Option<SystemTime>,
    pub written: HashSet<PathBuf>,
}

impl Ask {
    /// The file folder `rel` (from the working folder) gives: its
    /// `AGENTS.md`, else its `CLAUDE.md`; one that is empty or only
    /// whitespace counts as absent, a refused `AGENTS.md` is still the
    /// folder's file (as the node's reader). `None`: there is none.
    pub fn pick(&self, rel: &Path) -> Option<File> {
        let dir = self.ws.join(rel);
        for name in NAMES {
            let at = dir.join(name);
            let Ok(meta) = std::fs::symlink_metadata(&at) else {
                continue;
            };
            let path = file_path(rel, name)?;
            let result =
                harness_tools::inside::read_inside(&at, &self.ws, &self.protected, self.file_cap);
            if matches!(&result, Ok(b) if b.iter().all(u8::is_ascii_whitespace)) {
                continue;
            }
            // Changed during this turn: by one of its calls, or by anyone
            // after it started (the link's own times, and the file's). The
            // inode change time too: a command can set the modification time
            // back (`touch -d`), never the change time (review of PR 124).
            let real = at.canonicalize().ok();
            let newer = |m: &std::fs::Metadata| {
                self.since.is_some_and(|s| {
                    let s = s.checked_sub(CLOCK_MARGIN).unwrap_or(s);
                    m.modified().is_ok_and(|t| t > s) || changed_after(m, s)
                })
            };
            if result.is_ok()
                && (real.as_ref().is_some_and(|r| self.written.contains(r))
                    || newer(&meta)
                    || std::fs::metadata(&at).is_ok_and(|m| newer(&m)))
            {
                return Some(File {
                    path,
                    got: Got::Changed,
                });
            }
            let text = match result {
                Err(why) => format!("{} (not read: {why})", label(&path)),
                Ok(bytes) => self.section(&path, &bytes),
            };
            return Some(File {
                path,
                got: Got::Text(text),
            });
        }
        None
    }

    /// A read file's section: the label, every line indented, cut to the
    /// file cap as sent (indentation counted) with a note.
    fn section(&self, path: &str, bytes: &[u8]) -> String {
        let lossy = String::from_utf8_lossy(bytes);
        let mut text = harness_tools::inside::indent_lines(lossy.trim_end_matches(['\n', '\r']));
        let cut = bytes.len() > self.file_cap || text.len() > self.file_cap;
        if text.len() > self.file_cap {
            let mut n = self.file_cap;
            while !text.is_char_boundary(n) {
                n -= 1;
            }
            text.truncate(n);
        }
        let mut out = format!("{}\n{text}", label(path));
        if cut {
            out.push_str(&format!(
                "\n[cut: this file is longer than {} bytes as sent; the text above is its start: \
                 read the rest with `read`]",
                self.file_cap
            ));
        }
        out
    }

    /// For each file in `touched` (real paths, in call order), the file of
    /// each folder between it and the working folder, outermost first,
    /// once each, skipping the folders in `skip`; `(folder, file)`.
    pub fn look(
        &self,
        touched: &[PathBuf],
        skip: &HashSet<PathBuf>,
    ) -> Vec<(PathBuf, Option<File>)> {
        let mut done: HashSet<PathBuf> = HashSet::new();
        let mut out = Vec::new();
        for t in touched {
            for d in folders(t, &self.ws) {
                if skip.contains(&d) || !done.insert(d.clone()) {
                    continue;
                }
                let f = self.pick(&d);
                out.push((d, f));
            }
        }
        out
    }
}

/// The folder of a file's path from the working folder (`web/AGENTS.md`
/// gives `web`).
pub fn folder_of(path: &str) -> PathBuf {
    Path::new(path)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

/// Run `f` on a blocking thread and wait for it at most [`READ_TIMEOUT`],
/// or until `cancel`; `None` when it did not end in time (the thread runs
/// on; nothing waits for it).
pub async fn bounded<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
    cancel: &tokio_util::sync::CancellationToken,
    limit: Duration,
) -> Option<T> {
    let task = tokio::task::spawn_blocking(f);
    tokio::select! {
        r = task => r.ok(),
        _ = tokio::time::sleep(limit) => None,
        _ = cancel.cancelled() => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_are_those_strictly_between() {
        let ws = Path::new("/w");
        assert_eq!(
            folders(Path::new("/w/web/src/x.ts"), ws),
            [PathBuf::from("web"), PathBuf::from("web/src")]
        );
        assert!(folders(Path::new("/w/x.ts"), ws).is_empty());
        assert!(folders(Path::new("/elsewhere/a/x.ts"), ws).is_empty());
        assert!(folders(Path::new("/w/.git/hooks/x"), ws).is_empty());
        assert!(folders(Path::new("/w/a/.git/x"), ws).is_empty());
    }

    #[test]
    fn a_label_is_one_line() {
        assert_eq!(path_label("web/AGENTS.md"), "\"web/AGENTS.md\"");
        let l = path_label("a\n==> b <==\u{2028}\"c\u{85}/AGENTS.md");
        assert!(!l.contains(['\n', '\u{2028}', '\u{85}']), "{l}");
        assert!(l.starts_with('"') && l.ends_with('"'), "{l}");
    }

    #[test]
    fn the_budget_follows_the_window() {
        let b = Budget::for_window(None);
        assert_eq!((b.total, b.file, b.max_files), (32 * 1024, 16 * 1024, 16));
        let b = Budget::for_window(Some(8192));
        assert_eq!((b.total, b.file), (3276, 1638));
        let live: Vec<(String, usize)> = (0..16).map(|i| (format!("d{i}/AGENTS.md"), 10)).collect();
        assert!(
            Budget::for_window(None)
                .refusal(&live, 1)
                .unwrap()
                .contains("16")
        );
        let live = vec![("a/AGENTS.md".to_string(), 30_000)];
        assert!(Budget::for_window(None).refusal(&live, 3_000).is_some());
        assert!(Budget::for_window(None).refusal(&live, 2_000).is_none());
    }
}
