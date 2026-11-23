//! The check an edited package passes before it is handed back (PLAN.md T98):
//! every part well-formed, every internal relationship target a part of the
//! package, every part with a content type. It judges only what the edit
//! changed: a problem the input already had is not the edit's.

use std::collections::HashSet;

use quick_xml::events::Event;

use super::pkg::Source;
use super::scan::{NS_CT, NS_REL, Scan, attr};
use crate::Result;
use crate::limits::MAX_EDIT_EVENTS;

const SMALL: usize = 4 << 20;

/// What is wrong with a package, as sentences naming parts, never content.
fn problems(src: &Source<'_>) -> Result<HashSet<String>> {
    let mut out = HashSet::new();
    let names = src.names();
    let set: HashSet<String> = names.iter().map(|n| n.to_lowercase()).collect();
    // Content types.
    let mut defaults = HashSet::new();
    let mut overrides = HashSet::new();
    if src.has("[Content_Types].xml") {
        let ct = src.read("[Content_Types].xml", SMALL)?;
        let mut s = Scan::new(&ct, NS_CT, MAX_EDIT_EVENTS)?;
        while let Some(t) = s.next()? {
            if let Event::Start(e) | Event::Empty(e) = &t.ev {
                match s.local(e.name()) {
                    Some("Default") => {
                        if let Some(x) = attr(e, "Extension")? {
                            defaults.insert(x.to_lowercase());
                        }
                    }
                    Some("Override") => {
                        if let Some(x) = attr(e, "PartName")? {
                            overrides.insert(x.trim_start_matches('/').to_lowercase());
                        }
                    }
                    _ => {}
                }
            }
        }
    } else {
        out.insert("[Content_Types].xml is missing".to_string());
    }
    for o in &overrides {
        if !set.contains(o) {
            out.insert(format!(
                "a content type names a part that is not there: {o}"
            ));
        }
    }
    for n in names {
        if n.ends_with('/') || n == "[Content_Types].xml" {
            continue;
        }
        let ext = n
            .rsplit_once('.')
            .map(|(_, e)| e.to_lowercase())
            .unwrap_or_default();
        if !overrides.contains(&n.to_lowercase()) && !defaults.contains(&ext) {
            out.insert(format!("{n} has no content type"));
        }
    }
    // Relationships.
    for n in names {
        if !(n.ends_with(".rels")) {
            continue;
        }
        let part = src.read(n, SMALL)?;
        // `dir/_rels/x.xml.rels` belongs to `dir/x.xml`: its targets are
        // relative to `dir/`.
        let dir = match n.rsplit_once("/_rels/") {
            Some((d, _)) => format!("{d}/"),
            None => String::new(),
        };
        let mut s = Scan::new(&part, NS_REL, MAX_EDIT_EVENTS)?;
        while let Some(t) = s.next()? {
            if let Event::Start(e) | Event::Empty(e) = &t.ev
                && s.local(e.name()) == Some("Relationship")
            {
                if attr(e, "TargetMode")?.as_deref() == Some("External") {
                    continue;
                }
                let Some(target) = attr(e, "Target")? else {
                    out.insert(format!("{n}: a relationship has no target"));
                    continue;
                };
                match resolve(&dir, &target) {
                    None => {
                        out.insert(format!("{n}: a relationship points outside the package"));
                    }
                    Some(path) => {
                        if !set.contains(&path.to_lowercase()) {
                            out.insert(format!(
                                "{n}: a relationship points at a part that is not there: {path}"
                            ));
                        }
                    }
                }
            }
        }
    }
    Ok(out)
}

/// The package path a relationship target names (never opened as a file).
pub fn resolve(dir: &str, target: &str) -> Option<String> {
    let full = if let Some(abs) = target.strip_prefix('/') {
        abs.to_string()
    } else {
        format!("{dir}{target}")
    };
    let mut parts: Vec<&str> = vec![];
    for seg in full.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

/// The edited package `out` is no worse than the input `before`.
pub fn package(before: &[u8], out: &[u8]) -> Result<()> {
    let a = problems(&Source::open(before)?).unwrap_or_default();
    let b = Source::open(out).map_err(|e| {
        format!("the edited file did not pass its own check ({e}); nothing was written")
    })?;
    let b = problems(&b)?;
    if let Some(p) = b.difference(&a).next() {
        return Err(format!(
            "the edited file did not pass its own check ({p}); nothing was written"
        ));
    }
    Ok(())
}
