//! Invariants of Undo under the size cap, on random small trees (the
//! second review of PR 154 asked for this so the cases stop going one by
//! one). Each scenario: a repository with tracked and untracked files under
//! and over the cap, folders, file and folder swaps; a random turn (edits,
//! creates, deletes, renames, grows, shrinks); maybe random changes by the
//! person after it; then an unforced Undo, a forced one where that refused,
//! and an undo of the undo. Asserted for every scenario:
//!
//! 1. nothing that was on disk before the Undo is lost: a file the Undo
//!    changed or removed is held, byte for byte, by the snapshot it took
//!    first;
//! 2. an unforced Undo that goes ahead changes nothing the person changed
//!    after the turn (at, below or above a path it writes);
//! 3. every file the plan calls kept, lost or unsure is as it was, and every
//!    one it calls removed is gone and held by that snapshot;
//! 4. with the cap at 0 nothing is ever left out, and with no change by the
//!    person an unforced Undo puts the folder back exactly as it was before
//!    the turn;
//! 5. undoing the Undo puts the folder back exactly as it was before it;
//! 6. a file git tracks is never left out.
//!
//! Seeded: `CAP_SEED=<n>` reruns one scenario; a failure prints its seed.

use super::*;
use std::collections::BTreeMap;

type Disk = BTreeMap<String, Vec<u8>>;

/// The small cap of these scenarios (bytes).
const SMALL_CAP: u64 = 32;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn chance(&mut self, pct: u64) -> bool {
        self.below(100) < pct
    }
}

const PATHS: &[&str] = &[
    "a", "b", "c", "é", "a/x", "a/y", "b/x", "c/x", "c/x/y", "é/z",
];

fn sh(dir: &Path, args: &[&str]) -> Vec<u8> {
    let o = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("XDG_CONFIG_HOME", "/nonexistent")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}: {o:?}");
    o.stdout
}

fn disk(dir: &Path) -> Disk {
    walkdir::WalkDir::new(dir)
        .min_depth(1)
        .into_iter()
        .filter_entry(|e| e.file_name() != ".git")
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| {
            (
                e.path()
                    .strip_prefix(dir)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                std::fs::read(e.path()).unwrap(),
            )
        })
        .collect()
}

/// Write `p` (removing a file where a folder of it goes, or a folder at it).
fn put(dir: &Path, p: &str, bytes: &[u8]) {
    let full = dir.join(p);
    let mut at = dir.to_path_buf();
    let parts: Vec<&str> = p.split('/').collect();
    for c in &parts[..parts.len() - 1] {
        at = at.join(c);
        if std::fs::symlink_metadata(&at).is_ok_and(|m| !m.is_dir()) {
            std::fs::remove_file(&at).unwrap();
        }
    }
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    if full.is_dir() {
        std::fs::remove_dir_all(&full).unwrap();
    }
    std::fs::write(&full, bytes).unwrap();
}

fn remove(dir: &Path, p: &str) {
    let full = dir.join(p);
    match std::fs::symlink_metadata(&full) {
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(&full).unwrap(),
        Ok(_) => std::fs::remove_file(&full).unwrap(),
        Err(_) => {}
    }
}

fn content(rng: &mut Rng, cap: u64, tag: u8) -> Vec<u8> {
    let cap = if cap == 0 { SMALL_CAP } else { cap };
    let n = if rng.chance(40) {
        cap + 1 + rng.below(20)
    } else {
        1 + rng.below(cap / 2)
    };
    (0..n)
        .map(|i| b'a' + ((i as u8).wrapping_add(tag)) % 26)
        .collect()
}

/// One random change to the folder.
fn change(dir: &Path, rng: &mut Rng, cap: u64, tag: u8) {
    let p = PATHS[rng.below(PATHS.len() as u64) as usize];
    match rng.below(4) {
        0 | 1 => put(dir, p, &content(rng, cap, tag)),
        2 => remove(dir, p),
        _ => {
            // Rename a file somewhere else.
            let q = PATHS[rng.below(PATHS.len() as u64) as usize];
            let full = dir.join(p);
            if full.is_file() && p != q && !q.starts_with(&format!("{p}/")) {
                let bytes = std::fs::read(&full).unwrap();
                remove(dir, p);
                put(dir, q, &bytes);
            }
        }
    }
}

async fn blob(repo: &Repo, sha: &str, p: &str) -> Option<Vec<u8>> {
    run_raw(
        repo,
        &["cat-file", "blob", &format!("{sha}:{p}")],
        None,
        None,
    )
    .await
    .ok()
}

async fn scenario(seed: u64, cap: u64) -> Result<(), String> {
    let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().canonicalize().unwrap().join("repo");
    std::fs::create_dir(&dir).unwrap();
    sh(&dir, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("keep"), "k\n").unwrap();
    for i in 0..3 + rng.below(3) {
        change(&dir, &mut rng, cap, i as u8);
    }
    // Some of it tracked (committed), the rest not.
    sh(&dir, &["add", "keep"]);
    for p in disk(&dir).keys() {
        if rng.chance(40) {
            sh(&dir, &["add", "--", p]);
        }
    }
    sh(&dir, &["commit", "-qm", "init"]);
    let bounds = Bounds {
        untracked_max: cap,
        ..Default::default()
    };
    let repo = checkpoint_repo(&dir, Some(&d.path().join("cp")), &bounds)
        .await
        .unwrap()
        .unwrap();
    let tracked: Vec<Vec<u8>> = sh(&dir, &["ls-files", "-z"])
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(<[u8]>::to_vec)
        .collect();
    let before_disk = disk(&dir);
    let b = snapshot_full(&repo, "refs/harness/s/t/before", "b")
        .await
        .unwrap();
    for _ in 0..1 + rng.below(4) {
        change(&dir, &mut rng, cap, 100);
    }
    let a = snapshot_full(&repo, "refs/harness/s/t/after", "a")
        .await
        .unwrap();
    let after_disk = disk(&dir);
    for s in [&b.skipped, &a.skipped] {
        if s.files.iter().any(|(p, _)| tracked.contains(p)) {
            return Err(format!("6: a tracked file was left out: {s:?}"));
        }
        if cap == 0 && !s.is_empty() {
            return Err(format!("4: cap 0 left out {s:?}"));
        }
    }
    let person = rng.chance(40);
    if person {
        for _ in 0..1 + rng.below(2) {
            change(&dir, &mut rng, cap, 200);
        }
    }
    let d0 = disk(&dir);
    if std::env::var("CAP_DEBUG").is_ok() {
        eprintln!("before turn: {before_disk:?}\nafter turn:  {after_disk:?}\nbefore undo: {d0:?}");
    }
    let person_changed: Vec<String> = d0
        .keys()
        .chain(after_disk.keys())
        .filter(|p| d0.get(*p) != after_disk.get(*p))
        .cloned()
        .collect();
    let mut forced = false;
    let done = match restore_report(&repo, &b.sha, &a.sha, false).await {
        Ok(d) => d,
        Err(e) => {
            if disk(&dir) != d0 {
                return Err(format!("a refused undo changed the folder: {e}"));
            }
            forced = true;
            restore_report(&repo, &b.sha, &a.sha, true)
                .await
                .map_err(|e| format!("forced undo failed: {e}"))?
        }
    };
    let d1 = disk(&dir);
    if std::env::var("CAP_DEBUG").is_ok() {
        eprintln!("forced {forced}; after undo: {d1:?}\nplan: {:?}", done.plan);
    }
    // 1: whatever changed is held by the pre-restore snapshot.
    for (p, was) in &d0 {
        if d1.get(p) != Some(was) && blob(&repo, &done.now, p).await.as_ref() != Some(was) {
            return Err(format!("1: {p} changed by the undo and not saved first"));
        }
    }
    // 2: an unforced undo leaves the person's changes.
    if !forced {
        for p in &person_changed {
            if d1.get(p) != d0.get(p) {
                return Err(format!("2: unforced undo changed the person's {p}"));
            }
        }
    }
    // 3: the plan's words.
    for l in &done.plan.left {
        let p = String::from_utf8_lossy(&l.path).into_owned();
        match l.kind {
            LeftKind::Removed => {
                if d1.contains_key(&p) || blob(&repo, &done.now, &p).await != d0.get(&p).cloned() {
                    return Err(format!("3: {p} said removed and kept, is not"));
                }
            }
            _ => {
                if d1.get(&p) != d0.get(&p) {
                    return Err(format!("3: {p} said {:?}, changed", l.kind));
                }
            }
        }
    }
    // 4: no cap, no person: exactly the folder before the turn.
    if cap == 0 {
        if !done.plan.left.is_empty() {
            return Err(format!("4: cap 0 left {:?}", done.plan.left));
        }
        if !person && (forced || d1 != before_disk) {
            return Err(format!(
                "4: cap 0 undo is not the folder before the turn (forced {forced})"
            ));
        }
    }
    // 5: undoing the undo.
    let here = snapshot(&repo, "refs/harness/s/u/after", "u")
        .await
        .unwrap();
    restore_report(&repo, &done.now, &here, true)
        .await
        .map_err(|e| format!("5: undo of the undo failed: {e}"))?;
    let d2 = disk(&dir);
    if d2 != d0 {
        let diff: Vec<&String> = d0
            .keys()
            .chain(d2.keys())
            .filter(|p| d0.get(*p) != d2.get(*p))
            .collect();
        return Err(format!("5: undo of the undo differs at {diff:?}"));
    }
    Ok(())
}

async fn run_many(cap: u64, n: u64) {
    if let Ok(s) = std::env::var("CAP_SEED") {
        let seed: u64 = s.parse().unwrap();
        if let Err(e) = scenario(seed, cap).await {
            panic!("seed {seed}, cap {cap}: {e}");
        }
        return;
    }
    let mut failed = Vec::new();
    for seed in 0..n {
        if let Err(e) = scenario(seed, cap).await {
            failed.push(format!("seed {seed}, cap {cap}: {e}"));
        }
    }
    assert!(
        failed.is_empty(),
        "{} of {n} failed:\n{}",
        failed.len(),
        failed.join("\n")
    );
}

#[tokio::test]
async fn undo_invariants_hold_on_random_trees_with_no_cap() {
    run_many(0, 150).await;
}

#[tokio::test]
async fn undo_invariants_hold_on_random_trees_with_a_small_cap() {
    run_many(SMALL_CAP, 150).await;
}
