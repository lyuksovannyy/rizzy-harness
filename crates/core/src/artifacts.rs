//! Publishing files as artifacts: the seam between the agent loop, which
//! offers the `publish` tool, and the node, which confines, stages and
//! records the file (docs/artifacts.md). The master only ever names a stage
//! by its id; the node resolves ids through its own index.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Longest artifact name.
pub const MAX_NAME: usize = 80;
/// Longest note on a version (one line).
pub const MAX_NOTES: usize = 500;
/// The node-reported metadata of one version, serialized.
pub const MAX_META_BYTES: usize = 16 * 1024;
/// Entries of a folder pack listed in its metadata.
pub const MAX_LISTED: usize = 200;
/// Publishes one turn may make (counted on the node).
pub const MAX_PER_TURN: usize = 10;

/// What the `publish` tool asks the node to stage.
#[derive(Debug, Clone, Default)]
pub struct PublishRequest {
    /// The tool call, so the staged event lands on its card.
    pub call_id: String,
    /// Relative to the chat's folder (or absolute inside it).
    pub path: String,
    /// Already checked with `check_name`.
    pub name: Option<String>,
    /// Already checked with `check_notes`.
    pub notes: Option<String>,
    pub kind_hint: Option<String>,
    /// The tool call's cancellation: copying stops and the stage goes.
    pub cancel: tokio_util::sync::CancellationToken,
}

/// What the node staged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Published {
    pub name: String,
    pub size: u64,
    pub sha256: String,
    /// Folders are packed into one `.tar.gz`.
    pub packed: bool,
}

/// Where a turn publishes to: the node session's stager. `None` from
/// `TurnHost::artifacts` means publishing is off for this session.
#[async_trait::async_trait]
pub trait ArtifactSink: Send + Sync {
    async fn publish(&self, req: PublishRequest) -> Result<Published, String>;
}

/// Who staged a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StagedBy {
    /// The `publish` tool.
    Agent,
    /// "Save as artifact" in the UI.
    User,
}

/// A package manifest's name and version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Package {
    pub name: String,
    pub version: String,
}

/// The repository state of the folder a file came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitInfo {
    pub commit: String,
    pub branch: String,
    pub dirty: bool,
}

/// One file of a folder pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub path: String,
    pub size: u64,
}

/// What a node reports about a published file. Display only: nothing on
/// the master trusts it for anything else. Unknown fields are dropped.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toolchain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<Package>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<GitInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entries: Option<Vec<Entry>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entries_total: Option<u64>,
    /// Pack entries left out, by reason (`symlink`, `secret`, `git`, ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skipped: Option<BTreeMap<String, u64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
}

impl ArtifactMeta {
    /// Every string cleaned to one short line and the lists capped, so a
    /// node's report can only ever be a label.
    pub fn cleaned(mut self) -> Self {
        let line = |s: &mut String, max: usize| *s = crate::text::clean_line(s, max);
        let opt = |s: &mut Option<String>, max: usize| {
            if let Some(v) = s {
                *v = crate::text::clean_line(v, max);
            }
        };
        opt(&mut self.toolchain, 80);
        opt(&mut self.target, 80);
        opt(&mut self.source_path, 1024);
        if let Some(p) = &mut self.package {
            line(&mut p.name, 120);
            line(&mut p.version, 60);
        }
        if let Some(g) = &mut self.git {
            g.commit = g
                .commit
                .chars()
                .filter(char::is_ascii_hexdigit)
                .take(64)
                .collect();
            line(&mut g.branch, 120);
        }
        if let Some(e) = &mut self.entries {
            e.truncate(MAX_LISTED);
            for x in e.iter_mut() {
                line(&mut x.path, 255);
            }
        }
        if let Some(s) = &mut self.skipped {
            *s = std::mem::take(s)
                .into_iter()
                .take(16)
                .map(|(k, v)| (crate::text::clean_line(&k, 32), v))
                .collect();
        }
        self
    }

    /// Drop listed entries from the end until the serialized form fits
    /// `MAX_META_BYTES` (snapshot batches carry it in every replay).
    pub fn fit(mut self) -> Self {
        while serde_json::to_vec(&self).map_or(0, |v| v.len()) > MAX_META_BYTES {
            match &mut self.entries {
                Some(e) if !e.is_empty() => {
                    let keep = e.len() / 2;
                    e.truncate(keep);
                }
                _ => {
                    self.source_path = None;
                    self.package = None;
                    break;
                }
            }
        }
        self
    }
}

/// A stage id: a uuid v7 as `new_id()` writes it (32 lowercase hex). It is
/// a key into the node's index, never part of a path the master chose.
pub fn valid_stage_id(s: &str) -> bool {
    s.len() == 32
        && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        && uuid::Uuid::parse_str(s).is_ok_and(|u| u.get_version_num() == 7)
}

/// An artifact name: at most `MAX_NAME` characters, no `/`, `\` or
/// control characters, not empty.
pub fn check_name(name: &str) -> Result<String, String> {
    if name
        .chars()
        .any(|c| c == '/' || c == '\\' || c.is_control())
    {
        return Err("the name may not contain `/`, `\\` or control characters".into());
    }
    let clean = crate::text::clean_line(name, name.len());
    if clean.is_empty() {
        return Err("the name is empty".into());
    }
    if clean.chars().count() > MAX_NAME {
        return Err(format!("the name is longer than {MAX_NAME} characters"));
    }
    Ok(clean)
}

/// A version note: one line of at most `MAX_NOTES` characters.
pub fn check_notes(notes: &str) -> Result<String, String> {
    if notes.contains(['\n', '\r']) {
        return Err("notes are one line".into());
    }
    let clean = crate::text::clean_line(notes, notes.len());
    if clean.chars().count() > MAX_NOTES {
        return Err(format!("notes are at most {MAX_NOTES} characters"));
    }
    Ok(clean)
}

/// A size as people read it: `12.3 MB`.
pub fn human_size(n: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut v = n as f64 / 1024.0;
    let mut unit = UNITS[0];
    for u in &UNITS[1..] {
        if v < 1024.0 {
            break;
        }
        v /= 1024.0;
        unit = u;
    }
    format!("{v:.1} {unit}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_ids_are_uuid_v7_only() {
        assert!(valid_stage_id(&crate::new_id()));
        assert!(!valid_stage_id("../../etc/passwd"));
        assert!(!valid_stage_id(&crate::new_id().to_uppercase()));
        // A v4 uuid in the same shape.
        assert!(!valid_stage_id("0123456789ab4def8123456789abcdef"));
        assert!(!valid_stage_id(""));
    }

    #[test]
    fn names_and_notes() {
        assert_eq!(check_name(" myapp ").unwrap(), "myapp");
        assert!(check_name("a/b").is_err());
        assert!(check_name("a\\b").is_err());
        assert!(check_name("a\u{7}b").is_err());
        assert!(check_name("   ").is_err());
        assert!(check_name(&"x".repeat(81)).is_err());
        assert!(check_name(&"x".repeat(80)).is_ok());
        assert_eq!(check_notes("release build").unwrap(), "release build");
        assert!(check_notes("two\nlines").is_err());
        assert!(check_notes(&"n".repeat(501)).is_err());
    }

    #[test]
    fn meta_is_cleaned_and_fits() {
        let m = ArtifactMeta {
            toolchain: Some("rust\u{202e} 1.90".into()),
            git: Some(GitInfo {
                commit: "abc123<script>".into(),
                branch: "main\nx".into(),
                dirty: true,
            }),
            entries: Some(
                (0..500)
                    .map(|i| Entry {
                        path: format!("{}/{i}", "p".repeat(250)),
                        size: 1,
                    })
                    .collect(),
            ),
            ..Default::default()
        }
        .cleaned()
        .fit();
        assert_eq!(m.toolchain.as_deref(), Some("rust 1.90"));
        assert_eq!(m.git.as_ref().unwrap().commit, "abc123c");
        assert_eq!(m.git.as_ref().unwrap().branch, "main x");
        assert!(m.entries.as_ref().unwrap().len() <= MAX_LISTED);
        assert!(serde_json::to_vec(&m).unwrap().len() <= MAX_META_BYTES);
    }

    #[test]
    fn sizes() {
        assert_eq!(human_size(900), "900 B");
        assert_eq!(human_size(12_900_000), "12.3 MB");
        assert_eq!(human_size(2048), "2.0 KB");
    }
}
