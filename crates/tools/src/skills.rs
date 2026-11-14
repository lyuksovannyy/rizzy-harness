//! Skills (ADOPTION_PLAN X.2, PLAN.md T46): a folder with a `SKILL.md`
//! (YAML front matter with a `name` and a `description`, then
//! instructions, with files beside it if it needs them), in the format
//! Claude Code, Codex, OpenCode, Pi and the others read, so existing skill
//! folders work as they are.
//!
//! The node finds them at the start of every turn ([`discover`]): in
//! `.agents/skills` and `.claude/skills` of each folder from the session
//! folder's repository top down to the session folder ([`repo_roots`]),
//! and in the folders node.toml `[skills] roots` names (by default
//! `~/.agents/skills` and `~/.claude/skills`). It reads the head of each
//! `SKILL.md` and nothing else; nothing in a skill ever runs. The model
//! sees a capped catalogue of names and descriptions ([`render`]) after
//! the repository's instruction files, and loads a body by name with the
//! [`SkillTool`]; running a script a skill names goes through `bash` and
//! its approval like any other command.
//!
//! What is read is what the instruction files allow (T44): a regular file
//! whose real path is inside its folder and in no protected folder
//! ([`crate::inside::read_inside`]). A repository's skill folder must lie
//! inside the repository (inside the session folder for the session
//! folder's own skills); a skill's `SKILL.md` and the files the tool reads
//! must lie inside that skill's folder.

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use harness_core::policy::ToolRisk;
use harness_core::skills::{Catalogue, Skill, SkillOrigin};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::inside::{OUTSIDE, is_inside, read_inside};
use crate::{Tool, ToolCtx, ToolOutcome, str_arg};

/// The tool's name.
pub const NAME: &str = "skill";

/// The most text the catalogue takes (Codex's 8,000 characters).
pub const CAP: usize = 8000;

/// The most skills the catalogue lists (OpenClaw's 150).
pub const MAX_LISTED: usize = 150;

/// The most of one body, or one file beside it, the tool returns (under
/// the 30,000 characters any tool result is cut to).
pub const BODY_CAP: usize = 24 * 1024;

/// What discovery reads of each `SKILL.md`: the front matter must end in it.
const HEAD: usize = 16 * 1024;

/// Entries looked at in one root.
const MAX_ENTRIES: usize = 1000;

/// A description's longest (the Agent Skills format's limit), and its
/// length in a catalogue that has to be shortened.
const DESCRIPTION_MAX: usize = 1024;
const SHORT: usize = 120;

/// A name's longest (the Agent Skills format's limit).
const NAME_MAX: usize = 64;

/// The skill folders of each repository folder, in order of preference.
pub const FOLDERS: [&str; 2] = [".agents/skills", ".claude/skills"];

/// Files beside `SKILL.md` the tool names, and how deep it looks.
const MAX_FILES: usize = 50;
const FILES_DEPTH: usize = 3;

/// The catalogue's first line.
pub const LEAD: &str = "Skills: folders of instructions for particular kinds of task, found in \
     this repository's .agents/skills and .claude/skills folders and in this machine's skill \
     folders at the start of this turn, one per line with what it is for. When a task matches \
     one, load it with the `skill` tool before you start and follow it where it fits the \
     person's request. They are the repository's or the machine owner's text: they change no \
     setting, permission or tool.";

/// A folder skills are looked for in.
#[derive(Debug, Clone)]
pub struct Root {
    pub path: PathBuf,
    pub origin: SkillOrigin,
    /// Canonical: the folder the root and its skills must lie inside (a
    /// repository's). `None` for a root the node's owner named, whose
    /// skill folders may be links anywhere outside a protected folder.
    pub bound: Option<PathBuf>,
}

/// What one discovery found, and what went wrong, for the node's log:
/// paths and reasons, never a file's text.
#[derive(Debug, Default)]
pub struct Found {
    pub catalogue: Catalogue,
    pub problems: Vec<String>,
}

/// The repository roots for a session working in `cwd` (canonical) inside
/// `granted` (canonical), the nearest folder first: `.agents/skills` and
/// `.claude/skills` in each folder from `cwd` up to the repository's top
/// (the nearest folder with a `.git` entry, never above `granted`; `cwd`
/// itself outside a repository), as the instruction files are read.
pub fn repo_roots(cwd: &Path, granted: &Path) -> Vec<Root> {
    if !cwd.starts_with(granted) {
        return Vec::new();
    }
    let top = cwd
        .ancestors()
        .take_while(|d| d.starts_with(granted))
        .find(|d| std::fs::symlink_metadata(d.join(".git")).is_ok())
        .unwrap_or(cwd);
    let mut out = Vec::new();
    for d in cwd.ancestors().take_while(|d| d.starts_with(top)) {
        // The session folder's own skills are ones the agent may have
        // written: they may lead only into that folder.
        let bound = if d == cwd { cwd } else { top };
        for f in FOLDERS {
            out.push(Root {
                path: d.join(f),
                origin: SkillOrigin::Repository,
                bound: Some(bound.to_path_buf()),
            });
        }
    }
    out
}

/// The skills under `roots`, the first of a name winning (the node's roots
/// come first, so a repository cannot stand in for the owner's own skill),
/// with no file in a `protected` folder (canonical) read.
pub fn discover(roots: &[Root], protected: &[PathBuf]) -> Found {
    let mut found = Found::default();
    let mut seen: HashSet<String> = HashSet::new();
    let mut from: Vec<(String, PathBuf)> = Vec::new();
    for root in roots {
        let real = match root.path.canonicalize() {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                found.problems.push(format!(
                    "{}: it could not be read: {e}",
                    root.path.display()
                ));
                continue;
            }
        };
        let bound = root.bound.as_deref().unwrap_or(Path::new("/"));
        if !is_inside(&real, bound, protected) {
            found
                .problems
                .push(format!("{}: {OUTSIDE}", root.path.display()));
            continue;
        }
        let entries = match std::fs::read_dir(&real) {
            Ok(rd) => {
                if root.origin == SkillOrigin::Node && !found.catalogue.read.contains(&real) {
                    found.catalogue.read.push(real.clone());
                }
                rd
            }
            // A file where a folder was looked for is nobody's skill.
            Err(e) if e.raw_os_error() == Some(nix::libc::ENOTDIR) => continue,
            Err(e) => {
                found.problems.push(format!(
                    "{}: it could not be read: {e}",
                    root.path.display()
                ));
                continue;
            }
        };
        let mut names: Vec<std::ffi::OsString> = Vec::new();
        for e in entries.flatten() {
            if names.len() == MAX_ENTRIES {
                found.problems.push(format!(
                    "{}: only its first {MAX_ENTRIES} entries were looked at",
                    root.path.display()
                ));
                break;
            }
            names.push(e.file_name());
        }
        names.sort();
        for n in names {
            if n.as_encoded_bytes().first() == Some(&b'.') {
                continue;
            }
            let entry = real.join(&n);
            let label = entry.display().to_string();
            match one(&entry, &n.to_string_lossy(), bound, protected) {
                Ok(None) => {}
                Err(why) => {
                    found.problems.push(format!("{label}: {why}"));
                    let shown: String =
                        clean(&n.to_string_lossy()).chars().take(NAME_MAX).collect();
                    found.catalogue.refused.push(shown);
                }
                Ok(Some((name, description, manual, dir))) => {
                    if !seen.insert(name.clone()) {
                        let first = from
                            .iter()
                            .find(|(n, _)| *n == name)
                            .map(|(_, d)| d.display().to_string())
                            .unwrap_or_default();
                        found.problems.push(format!(
                            "{label}: not used: a skill named `{name}` is already listed, from \
                             {first}"
                        ));
                        continue;
                    }
                    if root.origin == SkillOrigin::Node
                        && !dir.starts_with(&real)
                        && !found.catalogue.read.contains(&dir)
                    {
                        found.catalogue.read.push(dir.clone());
                    }
                    from.push((name.clone(), dir.clone()));
                    found.catalogue.skills.push(Skill {
                        name,
                        description,
                        dir,
                        origin: root.origin,
                        manual,
                    });
                }
            }
        }
    }
    found.catalogue.skills.sort_by(|a, b| a.name.cmp(&b.name));
    found
}

/// The skill in the folder `entry` (named `folder`): `None` when it is no
/// skill folder (not a folder, or no `SKILL.md`).
#[allow(clippy::type_complexity)]
fn one(
    entry: &Path,
    folder: &str,
    bound: &Path,
    protected: &[PathBuf],
) -> Result<Option<(String, String, bool, PathBuf)>, String> {
    let md = std::fs::metadata(entry).map_err(|e| format!("it could not be read: {e}"))?;
    if !md.is_dir() {
        return Ok(None);
    }
    let dir = entry
        .canonicalize()
        .map_err(|e| format!("it could not be read: {e}"))?;
    if !is_inside(&dir, bound, protected) {
        return Err(OUTSIDE.into());
    }
    let file = dir.join("SKILL.md");
    if std::fs::symlink_metadata(&file).is_err() {
        return Ok(None);
    }
    let bytes =
        read_inside(&file, &dir, protected, HEAD).map_err(|why| format!("SKILL.md: {why}"))?;
    let text = normalized(&bytes);
    let (front, body) = split_front(&text, bytes.len() > HEAD)?;
    let fm: Front = match front {
        // What other harnesses accept is not always YAML: an unquoted
        // `description: Use when: ...` is not. Read the keys line by line
        // then.
        Some(y) if !y.trim().is_empty() => {
            serde_saphyr::from_str(y).unwrap_or_else(|_| front_by_lines(y))
        }
        _ => Front::default(),
    };
    // A given name that is not one (`My Skill`): the folder's name.
    let name = match fm.name.as_deref().map(str::trim) {
        Some(n) if valid_name(n) => n.to_string(),
        _ => folder.to_string(),
    };
    if !valid_name(&name) {
        return Err(format!(
            "SKILL.md: its name is not 1 to {NAME_MAX} letters, digits, '.', '_' or '-' \
             starting with a letter or digit"
        ));
    }
    let description = match fm.description.as_deref().map(clean) {
        Some(d) if !d.is_empty() => d,
        _ => clean(&first_paragraph(body)),
    };
    if description.is_empty() {
        return Err("SKILL.md: it has no description".into());
    }
    Ok(Some((name, description, fm.manual.unwrap_or(false), dir)))
}

/// The front matter keys read; every other key (`allowed-tools`, `hooks`,
/// `model`, `context`, `metadata`, ...) is ignored: none of them widens
/// anything here.
#[derive(Debug, Default, Deserialize)]
struct Front {
    name: Option<String>,
    description: Option<String>,
    #[serde(rename = "disable-model-invocation")]
    manual: Option<bool>,
}

/// The keys of front matter that is not valid YAML, read line by line: a
/// line `key: value` at the start of a line (split at the first `": "`),
/// with the indented lines after it joined to its value; a value in
/// matching quotes loses them; `>` or `|` starts a value on the next lines.
fn front_by_lines(y: &str) -> Front {
    fn set(f: &mut Front, key: &str, value: &str) {
        let v = value.trim();
        let v = ['"', '\'']
            .iter()
            .find_map(|q| v.strip_prefix(*q).and_then(|r| r.strip_suffix(*q)))
            .unwrap_or(v)
            .to_string();
        match key {
            "name" => f.name = Some(v),
            "description" => f.description = Some(v),
            "disable-model-invocation" => f.manual = Some(v == "true"),
            _ => {}
        }
    }
    let mut f = Front::default();
    let mut cur: Option<(String, String)> = None;
    for l in y.lines() {
        if l.starts_with(char::is_whitespace) || l.trim().is_empty() {
            if let Some((_, v)) = cur.as_mut()
                && !l.trim().is_empty()
            {
                if !v.is_empty() {
                    v.push(' ');
                }
                v.push_str(l.trim());
            }
            continue;
        }
        if let Some((k, v)) = cur.take() {
            set(&mut f, &k, &v);
        }
        let (k, v) = match l.split_once(": ") {
            Some((k, v)) => (k, v.trim()),
            None => match l.strip_suffix(':') {
                Some(k) => (k, ""),
                None => continue,
            },
        };
        let v = if v.starts_with(['>', '|']) && v.len() <= 2 {
            ""
        } else {
            v
        };
        cur = Some((k.trim().to_string(), v.to_string()));
    }
    if let Some((k, v)) = cur {
        set(&mut f, &k, &v);
    }
    f
}

/// UTF-8 (U+FFFD for other bytes), without a BOM, with `\n` line ends.
fn normalized(bytes: &[u8]) -> String {
    let t = String::from_utf8_lossy(bytes);
    t.strip_prefix('\u{FEFF}')
        .unwrap_or(&t)
        .replace("\r\n", "\n")
}

/// The front matter (between a first line `---` and the next `---` line)
/// and the text after it; no front matter when the text does not start
/// with `---`. `cut`: the text is the head of a longer file.
fn split_front(text: &str, cut: bool) -> Result<(Option<&str>, &str), String> {
    let Some(rest) = text.strip_prefix("---\n") else {
        return Ok((None, text));
    };
    if let Some(body) = rest.strip_prefix("---\n") {
        return Ok((Some(""), body));
    }
    match rest.find("\n---\n") {
        Some(i) => Ok((Some(&rest[..i]), &rest[i + 5..])),
        None => match rest.strip_suffix("\n---") {
            Some(y) if !cut => Ok((Some(y), "")),
            _ => Err(format!(
                "SKILL.md: its front matter does not end{}",
                if cut {
                    format!(" within its first {} KiB", HEAD / 1024)
                } else {
                    String::new()
                }
            )),
        },
    }
}

fn valid_name(n: &str) -> bool {
    let b = n.as_bytes();
    !b.is_empty()
        && b.len() <= NAME_MAX
        && b[0].is_ascii_alphanumeric()
        && b.iter()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
}

/// Characters that change how text around them reads without being seen:
/// direction overrides and isolates, zero-width characters.
fn invisible(c: char) -> bool {
    matches!(c,
        '\u{00AD}' | '\u{061C}' | '\u{180E}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}'
        | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}')
}

/// One line: control characters and line breaks become spaces, invisible
/// characters go, runs of spaces become one; at most
/// [`DESCRIPTION_MAX`] characters.
fn clean(s: &str) -> String {
    let mut out = String::with_capacity(s.len().min(DESCRIPTION_MAX * 4));
    let mut n = 0usize;
    for c in s.chars() {
        if invisible(c) {
            continue;
        }
        let c = if c.is_control() || c.is_whitespace() {
            ' '
        } else {
            c
        };
        if c == ' ' && (out.is_empty() || out.ends_with(' ')) {
            continue;
        }
        if n == DESCRIPTION_MAX {
            break;
        }
        out.push(c);
        n += 1;
    }
    out.trim_end().to_string()
}

/// The first paragraph of text in `body`: headings and empty lines before
/// it skipped, up to the next empty line or heading.
fn first_paragraph(body: &str) -> String {
    let mut lines = Vec::new();
    for l in body.lines() {
        let t = l.trim();
        let stop = t.is_empty() || t.starts_with('#');
        if stop && !lines.is_empty() {
            break;
        }
        if !stop {
            lines.push(t);
        }
    }
    lines.join(" ")
}

/// [`CAP`], lowered for a model whose context window is `window` tokens to
/// about 2% of it (4 bytes a token), as Codex sizes its list.
pub fn cap_for(window: Option<u64>) -> usize {
    match window {
        Some(w) => CAP.min(usize::try_from(w.saturating_mul(8) / 100).unwrap_or(CAP)),
        None => CAP,
    }
}

/// About how many shortened lines the catalogue always has room for,
/// whatever the cap.
const FLOOR_LINES: usize = 10;

/// The catalogue for the model: [`LEAD`], then `- name: description` per
/// listed skill, sorted by name, at most [`MAX_LISTED`] and `cap` bytes,
/// everything included; `cap` is never taken below the lead, the notes and
/// about [`FLOOR_LINES`] shortened lines, so a small window still lists
/// some. When the whole list does not fit, descriptions are shortened
/// first, then skills are left out, with a note saying how many and how to
/// reach them. A manual skill is not listed; skill folders that could not
/// be read are named; the one the person invoked is named last. `None`
/// when there is nothing to say.
pub fn render(c: &Catalogue, cap: usize) -> Option<String> {
    let listed: Vec<&Skill> = c.skills.iter().filter(|s| !s.manual).collect();
    let mut after: Vec<String> = Vec::new();
    if !c.refused.is_empty() {
        let mut names: Vec<&str> = c
            .refused
            .iter()
            .take(FLOOR_LINES)
            .map(String::as_str)
            .collect();
        let more = c.refused.len() - names.len();
        let and_more = format!("and {more} more");
        if more > 0 {
            names.push(&and_more);
        }
        after.push(format!(
            "[{} skill folder(s) could not be read: {}; the node's log says why]",
            c.refused.len(),
            names.join(", ")
        ));
    }
    if let Some(n) = c.invoked.as_deref() {
        after.push(format!(
            "[The person's message starts with /{n}: they ask for the skill `{n}`. Load it with \
             the `skill` tool first.]"
        ));
    }
    if listed.is_empty() && after.is_empty() {
        return None;
    }
    let total = listed.len();
    let take = total.min(MAX_LISTED);
    let tail: usize = after.iter().map(|a| a.len() + 1).sum();
    let note_for = |left: usize, cap: usize| {
        format!(
            "[not listed: {left} more skills (the list holds at most {MAX_LISTED} skills and \
             {cap} characters); the `skill` tool with a name it does not know lists them all]"
        )
    };
    let floor = LEAD.len()
        + 1
        + note_for(total, usize::MAX).len()
        + tail
        + FLOOR_LINES * (NAME_MAX + SHORT + 8);
    let cap = cap.max(floor);
    let note = |left: usize| note_for(left, cap);
    let line = |s: &Skill, short: bool| {
        let d = if short && s.description.chars().count() > SHORT {
            let mut d: String = s.description.chars().take(SHORT).collect();
            d.push('…');
            d
        } else {
            s.description.clone()
        };
        format!("\n- {}: {d}", s.name)
    };
    let finish = |mut out: String| {
        for a in &after {
            out.push('\n');
            out.push_str(a);
        }
        out
    };
    for short in [false, true] {
        let mut out = String::from(LEAD);
        for s in &listed[..take] {
            out.push_str(&line(s, short));
        }
        if take < total {
            out.push('\n');
            out.push_str(&note(total - take));
        }
        if out.len() + tail <= cap {
            return Some(finish(out));
        }
    }
    // Shortened, as many as fit with room for the note and the lines after.
    let room = cap.saturating_sub(note(total).len() + 1 + tail);
    let mut out = String::from(LEAD);
    let mut n = 0;
    for s in &listed[..take] {
        let l = line(s, true);
        if out.len() + l.len() > room {
            break;
        }
        out.push_str(&l);
        n += 1;
    }
    if n < total {
        out.push('\n');
        out.push_str(&note(total - n));
    }
    Some(finish(out))
}

/// The skill the person's message names with `/name` at its start (the
/// first word, without a trailing `:`, `,` or `.`), if there is one of
/// that name.
pub fn invoked(text: &str, c: &Catalogue) -> Option<String> {
    let rest = text.trim_start().strip_prefix('/')?;
    let name = rest
        .split(char::is_whitespace)
        .next()?
        .trim_end_matches([':', ',', '.']);
    c.skills
        .iter()
        .find(|s| s.name == name)
        .map(|s| s.name.clone())
}

/// The node owner's skill folders the agent's shell may read in this call
/// (`Catalogue::read`; only a session whose agent has the `skill` tool
/// has a catalogue): each one still at the real path discovery found (a
/// folder replaced by a link since is left out), never one that holds or
/// lies in a protected folder. Read-only, bound by their identity as
/// `[sandbox] read` entries are (`Grants::read_identified`).
pub fn shell_reads(ctx: &ToolCtx) -> Vec<(PathBuf, &'static str)> {
    let Some(c) = &ctx.skills else {
        return Vec::new();
    };
    c.read
        .iter()
        .filter(|p| p.canonicalize().ok().as_ref() == Some(*p))
        .filter(|p| {
            !ctx.protected
                .iter()
                .any(|x| x.starts_with(p) || p.starts_with(x))
        })
        .map(|p| (p.clone(), "a node.toml [skills] root"))
        .collect()
}

/// `skill`: one skill's `SKILL.md`, or one file beside it, by name.
pub struct SkillTool;

#[async_trait::async_trait]
impl Tool for SkillTool {
    fn name(&self) -> &'static str {
        NAME
    }

    fn description(&self) -> &'static str {
        "Load a skill listed under \"Skills\" in this turn's context: its instructions \
         (SKILL.md), with its folder's path and the files beside it. With `file`, returns one \
         of those files instead. Load a skill when the task matches its description, before \
         you start the work."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "The skill's name, as listed"
                },
                "file": {
                    "type": "string",
                    "description": "A file in the skill's folder, by its path there \
                                    (e.g. `reference.md`); leave out for SKILL.md"
                }
            },
            "required": ["name"]
        })
    }

    /// It reads text the node already lists; never changes anything.
    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }

    async fn run(&self, ctx: &ToolCtx, input: Value) -> ToolOutcome {
        const NONE: &str = "No skills are available in this session.";
        let Some(c) = ctx.skills.clone() else {
            return ToolOutcome::err(NONE);
        };
        let name = match str_arg(&input, "name") {
            Ok(n) => n.to_string(),
            Err(e) => return ToolOutcome::err(e),
        };
        let Some(skill) = c.loadable(&name).cloned() else {
            let known: Vec<&str> = c
                .skills
                .iter()
                .filter(|s| c.loadable(&s.name).is_some())
                .map(|s| s.name.as_str())
                .collect();
            if known.is_empty() {
                return ToolOutcome::err(NONE);
            }
            return ToolOutcome::err(format!(
                "There is no skill `{name}` here. The skills: {}.",
                known.join(", ")
            ));
        };
        let file = input
            .get("file")
            .and_then(Value::as_str)
            .map(str::to_string);
        let protected = ctx.protected.clone();
        let done = tokio::task::spawn_blocking(move || match file {
            Some(f) => beside(&skill, &f, &protected),
            None => body(&skill, &protected),
        })
        .await;
        match done {
            Ok(Ok(text)) => ToolOutcome::ok(text),
            Ok(Err(why)) => ToolOutcome::err(why),
            Err(e) => ToolOutcome::err(format!("the skill could not be read: {e}")),
        }
    }
}

/// `text` cut to [`BODY_CAP`] bytes (at a character boundary), with a note
/// naming `what` when it was longer.
fn capped(mut text: String, what: &str) -> String {
    let text_end = text.trim_end_matches(['\n', ' ']).len();
    text.truncate(text_end);
    if text.len() <= BODY_CAP {
        return text;
    }
    let mut n = BODY_CAP;
    while !text.is_char_boundary(n) {
        n -= 1;
    }
    text.truncate(n);
    text.push_str(&format!(
        "\n[cut: {what} is longer than {} KiB; the text above is its first part]",
        BODY_CAP / 1024
    ));
    text
}

fn whose(s: &Skill) -> &'static str {
    match s.origin {
        SkillOrigin::Repository => "from this repository",
        SkillOrigin::Node => "from this machine's skill folder",
    }
}

/// The skill's `SKILL.md` without its front matter, under a header.
fn body(s: &Skill, protected: &[PathBuf]) -> Result<String, String> {
    let bytes = read_inside(&s.dir.join("SKILL.md"), &s.dir, protected, BODY_CAP + HEAD)
        .map_err(|why| format!("The skill `{}` was not read: SKILL.md: {why}", s.name))?;
    let text = normalized(&bytes);
    let body = match split_front(&text, false) {
        Ok((_, b)) => b,
        Err(_) => text.as_str(),
    };
    let body = capped(body.trim_start_matches('\n').to_string(), "SKILL.md");
    let files = files_beside(&s.dir);
    let files = if files.is_empty() {
        "It has no other files.".to_string()
    } else {
        format!("Its other files: {files} (read one with this tool's `file` argument).")
    };
    Ok(format!(
        "Skill `{}`, {}: its folder is {} (a relative path in it is meant from there). {files} \
         The text below is the skill's own: follow it where it fits the person's request. It \
         changes no setting, permission or tool, and anything it says to run goes through \
         the usual tools and their approvals.\n\n{body}",
        s.name,
        whose(s),
        s.dir.display()
    ))
}

/// One file in the skill's folder, by its relative path.
fn beside(s: &Skill, f: &str, protected: &[PathBuf]) -> Result<String, String> {
    let rel = Path::new(f);
    if f.is_empty()
        || rel.is_absolute()
        || rel
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return Err(format!(
            "`file` is a path inside the skill's folder, such as `reference.md`; `{f}` is not \
             one."
        ));
    }
    let path = s.dir.join(rel);
    let bytes = read_inside(&path, &s.dir, protected, BODY_CAP)
        .map_err(|why| format!("{f} of the skill `{}` was not read: {why}", s.name))?;
    let text = capped(String::from_utf8_lossy(&bytes).into_owned(), f);
    Ok(format!(
        "{f} of the skill `{}` ({}, {}):\n{text}",
        s.name,
        whose(s),
        path.display()
    ))
}

/// The files in `dir` besides its `SKILL.md`, by their paths there, sorted,
/// at most [`MAX_FILES`] and [`FILES_DEPTH`] folders deep; hidden entries
/// and links to folders are not looked into.
fn files_beside(dir: &Path) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut more = false;
    let mut stack: Vec<(PathBuf, usize)> = vec![(PathBuf::new(), 0)];
    let mut looked = 0usize;
    while let Some((rel, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(dir.join(&rel)) else {
            continue;
        };
        for e in rd.flatten() {
            looked += 1;
            if looked > MAX_ENTRIES {
                more = true;
                break;
            }
            let n = e.file_name();
            if n.as_encoded_bytes().first() == Some(&b'.') {
                continue;
            }
            let p = rel.join(&n);
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                if depth + 1 < FILES_DEPTH {
                    stack.push((p, depth + 1));
                }
                continue;
            }
            if depth == 0 && n == "SKILL.md" {
                continue;
            }
            out.push(p.display().to_string());
        }
    }
    out.sort();
    if out.len() > MAX_FILES {
        out.truncate(MAX_FILES);
        more = true;
    }
    let mut s = out.join(", ");
    if more {
        s.push_str(", and more");
    }
    s
}
