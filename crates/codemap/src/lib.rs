//! Code structure without file bodies, from tree-sitter tag queries:
//!
//! - [`outline`]: one file's definitions with line numbers and their
//!   signature line (the `outline` tool).
//! - [`repo_map`]: the repository's most important definitions within a
//!   token budget. Files are ranked with PageRank over "file A refers to a
//!   name defined in file B" edges, the approach of aider's repo map
//!   (reimplemented; see THIRD_PARTY_NOTICES.md).
//!
//! Languages: Rust, Python, JavaScript, TypeScript/TSX, Go. Other files are
//! skipped. Everything is read-only and bounded (file count, file size).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use tree_sitter_tags::{TagsConfiguration, TagsContext};

/// Files larger than this are skipped (generated or vendored code).
const MAX_FILE_BYTES: u64 = 512 * 1024;
/// Files scanned for a repository map.
const MAX_FILES: usize = 3000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Lang {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Tsx,
    Go,
}

fn lang_of(path: &Path) -> Option<Lang> {
    Some(match path.extension()?.to_str()? {
        "rs" => Lang::Rust,
        "py" | "pyi" => Lang::Python,
        "js" | "mjs" | "cjs" | "jsx" => Lang::JavaScript,
        "ts" | "mts" | "cts" => Lang::TypeScript,
        "tsx" => Lang::Tsx,
        "go" => Lang::Go,
        _ => return None,
    })
}

fn config(lang: Lang) -> Option<&'static TagsConfiguration> {
    fn build(
        cell: &'static OnceLock<Option<TagsConfiguration>>,
        lang: tree_sitter::Language,
        tags: &str,
        locals: &str,
    ) -> Option<&'static TagsConfiguration> {
        cell.get_or_init(|| TagsConfiguration::new(lang, tags, locals).ok())
            .as_ref()
    }
    static RUST: OnceLock<Option<TagsConfiguration>> = OnceLock::new();
    static PY: OnceLock<Option<TagsConfiguration>> = OnceLock::new();
    static JS: OnceLock<Option<TagsConfiguration>> = OnceLock::new();
    static TS: OnceLock<Option<TagsConfiguration>> = OnceLock::new();
    static TSX: OnceLock<Option<TagsConfiguration>> = OnceLock::new();
    static GO: OnceLock<Option<TagsConfiguration>> = OnceLock::new();
    // TypeScript's tag query covers TS-only constructs; JavaScript's covers
    // functions, classes and methods. TS files get both.
    let ts_tags = || {
        format!(
            "{}\n{}",
            tree_sitter_javascript::TAGS_QUERY,
            tree_sitter_typescript::TAGS_QUERY
        )
    };
    match lang {
        Lang::Rust => build(
            &RUST,
            tree_sitter_rust::LANGUAGE.into(),
            tree_sitter_rust::TAGS_QUERY,
            "",
        ),
        Lang::Python => build(
            &PY,
            tree_sitter_python::LANGUAGE.into(),
            tree_sitter_python::TAGS_QUERY,
            "",
        ),
        Lang::JavaScript => build(
            &JS,
            tree_sitter_javascript::LANGUAGE.into(),
            tree_sitter_javascript::TAGS_QUERY,
            tree_sitter_javascript::LOCALS_QUERY,
        ),
        Lang::TypeScript => build(
            &TS,
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            &ts_tags(),
            tree_sitter_typescript::LOCALS_QUERY,
        ),
        Lang::Tsx => build(
            &TSX,
            tree_sitter_typescript::LANGUAGE_TSX.into(),
            &ts_tags(),
            tree_sitter_typescript::LOCALS_QUERY,
        ),
        Lang::Go => build(
            &GO,
            tree_sitter_go::LANGUAGE.into(),
            tree_sitter_go::TAGS_QUERY,
            "",
        ),
    }
}

/// One tag found in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    /// `function`, `method`, `class`, `module`, `interface`, `call`...
    pub kind: String,
    /// 1-based line of the name.
    pub line: usize,
    /// The source line the definition starts on, trimmed.
    pub signature: String,
    pub definition: bool,
}

/// Every tag in `source`, or `None` for unsupported languages.
pub fn symbols(path: &Path, source: &str) -> Option<Vec<Symbol>> {
    let cfg = config(lang_of(path)?)?;
    let mut ctx = TagsContext::new();
    let (tags, _) = ctx.generate_tags(cfg, source.as_bytes(), None).ok()?;
    let lines: Vec<&str> = source.lines().collect();
    let mut out = Vec::new();
    for t in tags.flatten() {
        let name = source.get(t.name_range.clone()).unwrap_or("").to_string();
        if name.is_empty() {
            continue;
        }
        let line = t.span.start.row + 1;
        let sig: String = lines
            .get(t.span.start.row)
            .map(|l| l.trim())
            .unwrap_or("")
            .chars()
            .take(160)
            .collect();
        out.push(Symbol {
            name,
            kind: cfg.syntax_type_name(t.syntax_type_id).to_string(),
            line,
            signature: sig,
            definition: t.is_definition,
        });
    }
    Some(out)
}

/// A file's definitions, one per line: `line: signature`.
pub fn outline(path: &Path, source: &str) -> Option<String> {
    let syms = symbols(path, source)?;
    let mut out = String::new();
    let mut seen = HashSet::new();
    for s in syms.into_iter().filter(|s| s.definition) {
        if seen.insert(s.line) {
            out.push_str(&format!("{}: {}\n", s.line, s.signature));
        }
    }
    if out.is_empty() {
        out.push_str("(no definitions found)\n");
    }
    Some(out)
}

/// Whether `outline` and `repo_map` understand this file type.
pub fn supported(path: &Path) -> bool {
    lang_of(path).is_some()
}

/// The ranked map of the repository at `root`, at most about
/// `budget_tokens` long (4 characters per token). `None` when no supported
/// source file was found.
pub fn repo_map(root: &Path, budget_tokens: usize) -> Option<String> {
    let mut files: Vec<(PathBuf, Vec<Symbol>)> = Vec::new();
    let walker = ignore::WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .build();
    for entry in walker.flatten() {
        if files.len() >= MAX_FILES {
            break;
        }
        let p = entry.path();
        if !entry.file_type().is_some_and(|t| t.is_file()) || !supported(p) {
            continue;
        }
        if entry.metadata().map(|m| m.len()).unwrap_or(u64::MAX) > MAX_FILE_BYTES {
            continue;
        }
        let Ok(src) = std::fs::read_to_string(p) else {
            continue;
        };
        if let Some(syms) = symbols(p, &src) {
            let rel = p.strip_prefix(root).unwrap_or(p).to_path_buf();
            files.push((rel, syms));
        }
    }
    if files.is_empty() {
        return None;
    }
    let ranks = rank_files(&files);
    let mut order: Vec<usize> = (0..files.len()).collect();
    order.sort_by(|a, b| {
        ranks[*b]
            .partial_cmp(&ranks[*a])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| files[*a].0.cmp(&files[*b].0))
    });
    let budget = budget_tokens.max(64) * 4;
    let mut out = String::from(
        "Repository map (most referenced files first; definitions with line numbers):\n",
    );
    for i in order {
        let (path, syms) = &files[i];
        let mut block = format!("{}:\n", path.display());
        let mut seen = HashSet::new();
        let mut n = 0;
        for s in syms.iter().filter(|s| s.definition) {
            if seen.insert(s.line) {
                block.push_str(&format!("  {}: {}\n", s.line, s.signature));
                n += 1;
            }
        }
        if n == 0 {
            continue;
        }
        if out.len() + block.len() > budget {
            // Fit what we can of this file, then stop.
            let room = budget.saturating_sub(out.len());
            if room > 200 {
                let cut: String = block
                    .lines()
                    .take_while({
                        let mut used = 0;
                        move |l| {
                            used += l.len() + 1;
                            used <= room
                        }
                    })
                    .map(|l| format!("{l}\n"))
                    .collect();
                out.push_str(&cut);
            }
            break;
        }
        out.push_str(&block);
    }
    Some(out.trim_end().to_string())
}

/// PageRank over files; an edge A→B for each name A refers to that B defines.
fn rank_files(files: &[(PathBuf, Vec<Symbol>)]) -> Vec<f64> {
    let n = files.len();
    let mut defined_in: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, (_, syms)) in files.iter().enumerate() {
        for s in syms.iter().filter(|s| s.definition) {
            defined_in.entry(s.name.as_str()).or_default().push(i);
        }
    }
    // Weighted edges (BTreeMap for a deterministic order).
    let mut out_edges: Vec<BTreeMap<usize, f64>> = vec![BTreeMap::new(); n];
    for (i, (_, syms)) in files.iter().enumerate() {
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for s in syms.iter().filter(|s| !s.definition) {
            *counts.entry(s.name.as_str()).or_default() += 1;
        }
        for (name, c) in counts {
            if let Some(targets) = defined_in.get(name) {
                // Common names defined in many files say little.
                let w = (c as f64).sqrt() / targets.len() as f64;
                for &t in targets {
                    if t != i {
                        *out_edges[i].entry(t).or_default() += w;
                    }
                }
            }
        }
    }
    let d = 0.85;
    let mut rank = vec![1.0 / n as f64; n];
    for _ in 0..30 {
        let mut next = vec![(1.0 - d) / n as f64; n];
        let mut dangling = 0.0;
        for i in 0..n {
            let total: f64 = out_edges[i].values().sum();
            if total == 0.0 {
                dangling += rank[i];
                continue;
            }
            for (&j, &w) in &out_edges[i] {
                next[j] += d * rank[i] * w / total;
            }
        }
        for v in next.iter_mut() {
            *v += d * dangling / n as f64;
        }
        rank = next;
    }
    rank
}

#[cfg(test)]
mod tests {
    use super::*;

    const RS: &str = "/// Docs\npub struct Store {\n    x: u32,\n}\n\nimpl Store {\n    pub fn open(path: &str) -> Self {\n        todo!()\n    }\n}\n\npub fn helper() -> u8 {\n    1\n}\n";

    #[test]
    fn rust_outline_lists_definitions_without_bodies() {
        let o = outline(Path::new("a.rs"), RS).unwrap();
        assert!(o.contains("2: pub struct Store {"), "{o}");
        assert!(o.contains("7: pub fn open(path: &str) -> Self {"), "{o}");
        assert!(o.contains("12: pub fn helper() -> u8 {"), "{o}");
        assert!(!o.contains("todo!"), "{o}");
    }

    #[test]
    fn other_languages() {
        let py =
            "class A:\n    def run(self, x):\n        return x\n\ndef main():\n    A().run(1)\n";
        let o = outline(Path::new("m.py"), py).unwrap();
        assert!(
            o.contains("1: class A:")
                && o.contains("2: def run(self, x):")
                && o.contains("5: def main():"),
            "{o}"
        );
        let ts = "export interface Opts { a: number }\nexport function go(o: Opts): void {}\nclass K { m() {} }\n";
        let o = outline(Path::new("x.ts"), ts).unwrap();
        assert!(o.contains("export function go"), "{o}");
        assert!(o.contains("class K") || o.contains("m() {}"), "{o}");
        let go = "package main\n\nfunc Add(a, b int) int { return a + b }\n";
        let o = outline(Path::new("x.go"), go).unwrap();
        assert!(o.contains("3: func Add(a, b int) int"), "{o}");
        assert!(outline(Path::new("x.txt"), "hi").is_none());
    }

    #[test]
    fn repo_map_ranks_referenced_files_first_and_fits_the_budget() {
        let dir = tempfile::tempdir().unwrap();
        let w = |p: &str, s: &str| {
            let f = dir.path().join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, s).unwrap();
        };
        w("src/core.rs", "pub fn central() {}\npub struct Thing;\n");
        w("src/a.rs", "fn a() { central(); central(); }\n");
        w("src/b.rs", "fn b() { central(); let _t = Thing; }\n");
        w("src/leaf.rs", "fn lonely() {}\n");
        w("notes.txt", "not code");
        let map = repo_map(dir.path(), 1024).unwrap();
        let core = map.find("src/core.rs").unwrap();
        let leaf = map.find("src/leaf.rs").unwrap();
        assert!(core < leaf, "{map}");
        assert!(map.contains("1: pub fn central() {}"), "{map}");
        assert!(!map.contains("notes.txt"));
        // Tiny budget: still valid, shorter.
        let small = repo_map(dir.path(), 64).unwrap();
        assert!(small.len() <= 64 * 4 + 100, "{}", small.len());
        assert!(small.contains("src/core.rs"), "{small}");
    }
}
