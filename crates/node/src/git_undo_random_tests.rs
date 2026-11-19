//! Invariants of Undo on random small trees (the idea of the closed PR
//! 154's random test, without any size cap). Each scenario: a repository
//! with tracked and untracked files, links, executable files, empty
//! folders, ignored files and odd names (`*`, pathspec magic, a newline,
//! bytes that are not UTF-8); a random turn; maybe random changes by the
//! person after it, at, above and below what the turn changed (and `git
//! add` / `git rm --cached` by either); then an unforced Undo, a forced
//! one where that refused, and an undo of the undo. For every scenario:
//!
//! 1. an unforced Undo that goes ahead loses nothing: whatever it changed
//!    or removed is held, content, mode and link target, by the snapshot
//!    taken first;
//! 2. an unforced Undo that goes ahead writes over nothing changed after
//!    the turn;
//! 3. a forced Undo keeps everything it changed or removed in that
//!    snapshot, except the files it names as in no checkpoint;
//! 4. what is named is as said: every entry the Undo changed lies at,
//!    below or above a path its plan names, each path it writes is the
//!    state before the turn, and with no change by the person the folder
//!    is that state exactly (an unforced Undo refuses then only for files
//!    in no checkpoint);
//! 5. undoing the Undo puts back the folder as it was before it (but the
//!    files named as in no checkpoint).
//!
//! Entries are files (bytes and the executable bit) and links (target);
//! folders are not compared (git records no empty folder).
//!
//! `UNDO_SEED=<n>` reruns one scenario (`UNDO_DEBUG=1` prints its trees);
//! `UNDO_SEEDS=<n>` runs seeds `0..n` (default 200).

use super::tests::sh;
use super::*;
use std::collections::BTreeMap;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;

#[derive(Clone, Debug, PartialEq, Eq)]
enum E {
    /// Content and whether it is executable.
    F(Vec<u8>, bool),
    /// A link and its target.
    L(Vec<u8>),
}

type Disk = BTreeMap<Vec<u8>, E>;

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
    fn of<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[self.below(v.len() as u64) as usize]
    }
}

const PATHS: &[&[u8]] = &[
    b"a",
    b"a/b",
    b"a/b/c",
    b"a/x",
    b"a/z.log",
    b"x",
    b"x/y",
    b"q*",
    b"q1",
    b":(attr:zz)k",
    b":!q1",
    b"n\xffm",
    b"n\xffm/k",
    b"nl\nx",
    b"sp ace",
    b"\xc3\xa9/f",
    b"e/f/g",
    b"t.log",
];

const TARGETS: &[&[u8]] = &[b"keep", b"a", b"nowhere", b"../outside"];

fn at(dir: &Path, p: &[u8]) -> PathBuf {
    dir.join(std::ffi::OsStr::from_bytes(p))
}

fn disk(dir: &Path) -> Disk {
    walkdir::WalkDir::new(dir)
        .min_depth(1)
        .into_iter()
        .filter_entry(|e| e.file_name() != ".git")
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let rel = e.path().strip_prefix(dir).ok()?.as_os_str().as_bytes();
            let m = std::fs::symlink_metadata(e.path()).ok()?;
            let entry = if m.file_type().is_symlink() {
                E::L(
                    std::fs::read_link(e.path())
                        .ok()?
                        .as_os_str()
                        .as_bytes()
                        .to_vec(),
                )
            } else if m.is_file() {
                E::F(
                    std::fs::read(e.path()).ok()?,
                    m.permissions().mode() & 0o100 != 0,
                )
            } else {
                return None;
            };
            Some((rel.to_vec(), entry))
        })
        .collect()
}

/// Make room for an entry at `p`: a non-folder where a folder of it goes
/// is removed, and so is whatever is at `p`.
fn clear_for(dir: &Path, p: &[u8]) {
    for d in parents(p) {
        let full = at(dir, d);
        if std::fs::symlink_metadata(&full).is_ok_and(|m| !m.is_dir()) {
            std::fs::remove_file(&full).unwrap();
        }
    }
    let full = at(dir, p);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    remove(dir, p);
}

fn remove(dir: &Path, p: &[u8]) {
    let full = at(dir, p);
    match std::fs::symlink_metadata(&full) {
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(&full).unwrap(),
        Ok(_) => std::fs::remove_file(&full).unwrap(),
        Err(_) => {}
    }
}

/// Whether `p` can be reached without passing through a link or a file.
fn reachable(dir: &Path, p: &[u8]) -> bool {
    parents(p).all(|d| std::fs::symlink_metadata(at(dir, d)).map_or(true, |m| m.is_dir()))
}

fn content(rng: &mut Rng, tag: u8, same_len_as: Option<usize>) -> Vec<u8> {
    let n = same_len_as.unwrap_or_else(|| 1 + rng.below(12) as usize);
    (0..n)
        .map(|i| {
            b'a' + ((i as u8)
                .wrapping_add(tag)
                .wrapping_add(rng.below(26) as u8))
                % 26
        })
        .collect()
}

/// A path for a change: one of the fixed names, or (with `near`, the paths
/// the turn changed) one of those, a folder above one, or a path below one.
fn pick(rng: &mut Rng, near: &[Vec<u8>]) -> Vec<u8> {
    if !near.is_empty() && rng.chance(60) {
        let base = rng.of(near).clone();
        match rng.below(3) {
            0 => base,
            1 => {
                let ps: Vec<Vec<u8>> = parents(&base).map(<[u8]>::to_vec).collect();
                if ps.is_empty() {
                    base
                } else {
                    rng.of(&ps).clone()
                }
            }
            _ => {
                let mut b = base;
                b.extend_from_slice(if rng.chance(50) { b"/u" } else { b"/v/w" });
                b
            }
        }
    } else {
        rng.of(PATHS).to_vec()
    }
}

/// One random change to the folder (or to the user's index).
fn change(dir: &Path, rng: &mut Rng, tag: u8, near: &[Vec<u8>]) {
    let p = pick(rng, near);
    let full = at(dir, &p);
    let old = std::fs::symlink_metadata(&full).ok();
    match rng.below(9) {
        0..=2 => {
            // Write a file; over an existing one, sometimes at its length.
            let len = old
                .as_ref()
                .filter(|m| m.is_file() && rng.chance(50))
                .map(|m| m.len() as usize)
                .filter(|n| *n > 0);
            let bytes = content(rng, tag, len);
            if old.as_ref().is_some_and(|m| m.is_file()) && reachable(dir, &p) {
                std::fs::write(&full, bytes).unwrap();
            } else {
                clear_for(dir, &p);
                std::fs::write(&full, bytes).unwrap();
            }
        }
        3 => remove(dir, &p),
        4 => {
            // Rename a file elsewhere.
            let q = pick(rng, near);
            if old.as_ref().is_some_and(|m| m.is_file())
                && reachable(dir, &p)
                && p != q
                && !q.starts_with(&[&p[..], b"/"].concat())
            {
                let e = disk(dir).remove(&p);
                remove(dir, &p);
                if let Some(E::F(bytes, x)) = e {
                    clear_for(dir, &q);
                    std::fs::write(at(dir, &q), bytes).unwrap();
                    set_x(&at(dir, &q), x);
                }
            }
        }
        5 => {
            clear_for(dir, &p);
            std::os::unix::fs::symlink(std::ffi::OsStr::from_bytes(rng.of(TARGETS)), &full)
                .unwrap();
        }
        6 => {
            // Flip the executable bit of a file.
            if old.as_ref().is_some_and(|m| m.is_file()) && reachable(dir, &p) {
                let x = old.unwrap().permissions().mode() & 0o100 != 0;
                set_x(&full, !x);
            }
        }
        7 => {
            // An empty folder.
            clear_for(dir, &p);
            std::fs::create_dir(&full).unwrap();
        }
        _ => {
            // The user's index: stage the path or take it out.
            let args: &[&str] = if rng.chance(50) {
                &["add", "-f", "--"]
            } else {
                &["rm", "-q", "-r", "--cached", "--ignore-unmatch", "--"]
            };
            let _ = git_quiet(dir, args, &p);
        }
    }
}

fn set_x(p: &Path, x: bool) {
    let mut perm = std::fs::metadata(p).unwrap().permissions();
    perm.set_mode(if x { 0o755 } else { 0o644 });
    std::fs::set_permissions(p, perm).unwrap();
}

/// A git call in the user's repository that may fail (a path git cannot
/// take, say).
fn git_quiet(dir: &Path, args: &[&str], path: &[u8]) -> bool {
    std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .arg(std::ffi::OsStr::from_bytes(path))
        .env("GIT_LITERAL_PATHSPECS", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Every entry of snapshot `sha`, as the folder would hold it.
async fn tree(repo: &Repo, sha: &str) -> Disk {
    let out = run_raw(
        repo,
        &["ls-tree", "-r", "-z", "--full-tree", sha],
        None,
        None,
    )
    .await
    .unwrap();
    let mut d = Disk::new();
    for e in out.split(|b| *b == 0).filter(|e| !e.is_empty()) {
        let tab = e.iter().position(|b| *b == b'\t').unwrap();
        let meta = String::from_utf8_lossy(&e[..tab]).into_owned();
        let f: Vec<&str> = meta.split(' ').collect();
        let bytes = run_raw(repo, &["cat-file", "blob", f[2]], None, None)
            .await
            .unwrap();
        let entry = match f[0] {
            "120000" => E::L(bytes),
            "100755" => E::F(bytes, true),
            _ => E::F(bytes, false),
        };
        d.insert(e[tab + 1..].to_vec(), entry);
    }
    d
}

fn is_ignored(p: &[u8]) -> bool {
    p.ends_with(b".log")
}

/// Paths whose entry differs between two folders.
fn differ<'a>(x: &'a Disk, y: &'a Disk) -> Vec<&'a Vec<u8>> {
    let mut v: Vec<&Vec<u8>> = x
        .keys()
        .chain(y.keys())
        .filter(|p| x.get(*p) != y.get(*p))
        .collect();
    v.sort();
    v.dedup();
    v
}

fn show(d: &Disk) -> String {
    d.iter()
        .map(|(p, e)| {
            let e = match e {
                E::F(b, x) => format!(
                    "F{}{}",
                    String::from_utf8_lossy(b),
                    if *x { "x" } else { "" }
                ),
                E::L(t) => format!("L->{}", String::from_utf8_lossy(t)),
            };
            format!("{:?}={e}", String::from_utf8_lossy(p))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

async fn scenario(seed: u64) -> Result<&'static str, String> {
    let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().canonicalize().unwrap().join("repo");
    std::fs::create_dir(&dir).unwrap();
    sh(&dir, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("keep"), "k\n").unwrap();
    std::fs::write(dir.join(".gitignore"), "*.log\n").unwrap();
    for i in 0..3 + rng.below(4) {
        change(&dir, &mut rng, i as u8, &[]);
    }
    // Some of it tracked (committed), the rest not.
    sh(&dir, &["add", "keep", ".gitignore"]);
    for p in disk(&dir).keys() {
        if rng.chance(40) {
            let _ = git_quiet(&dir, &["add", "--"], p);
        }
    }
    sh(&dir, &["commit", "-qm", "init"]);
    let repo = repo_for(&dir, Some(&d.path().join("cp"))).await.unwrap();
    let snap = |r: &'static str| {
        let repo = &repo;
        async move {
            snapshot(repo, r, "s")
                .await
                .map_err(|e| format!("snapshot {r} failed: {e}"))
        }
    };
    let before_disk = disk(&dir);
    let b = snap("refs/harness/s/t/before").await?;
    for _ in 0..1 + rng.below(4) {
        change(&dir, &mut rng, 100, &[]);
    }
    let a = snap("refs/harness/s/t/after").await?;
    let after_disk = disk(&dir);
    let near: Vec<Vec<u8>> = differ(&before_disk, &after_disk)
        .into_iter()
        .cloned()
        .collect();
    let person = rng.chance(50);
    if person {
        for _ in 0..1 + rng.below(3) {
            change(&dir, &mut rng, 200, &near);
        }
    }
    let d0 = disk(&dir);
    let debug = std::env::var("UNDO_DEBUG").is_ok();
    let trees = format!(
        "\n before turn {}\n after turn  {}\n before undo {}",
        show(&before_disk),
        show(&after_disk),
        show(&d0)
    );
    if debug {
        eprintln!("{trees}");
    }
    let person_changed: Vec<Vec<u8>> = differ(&after_disk, &d0).into_iter().cloned().collect();
    let mut forced = false;
    let mut refusal = None;
    let done = match restore_report(&repo, &b, &a, false).await {
        Ok(d) => d,
        Err(e) => {
            if disk(&dir) != d0 {
                return Err(format!("a refused undo changed the folder: {e}{trees}"));
            }
            forced = true;
            refusal = Some(e);
            restore_report(&repo, &b, &a, true)
                .await
                .map_err(|e| format!("forced undo failed: {e}{trees}"))?
        }
    };
    let now = run(&repo, &["rev-parse", "refs/harness/test/now/before"], None)
        .await
        .unwrap();
    let held = tree(&repo, &now).await;
    let d1 = disk(&dir);
    let fail = |n: &str, what: String| {
        Err(format!(
            "{n}: {what} (forced {forced}, refusal {refusal:?}){trees}\n after undo  {}\n plan {:?}",
            show(&d1),
            done.plan
        ))
    };
    if debug {
        eprintln!("forced {forced} {refusal:?}\n after undo  {}", show(&d1));
    }
    let unsaved: Vec<String> = done.plan.unsaved.shown.clone();
    let named_unsaved = |p: &[u8]| unsaved.contains(&shown(p));
    // 1 and 3: whatever the undo changed is held by the snapshot before it
    // (a forced one: or named as in no checkpoint).
    for p in differ(&d0, &d1) {
        let Some(was) = d0.get(p) else { continue };
        if held.get(p) != Some(was) && !(forced && named_unsaved(p)) {
            return fail(
                if forced { "3" } else { "1" },
                format!(
                    "{:?} changed and not saved first",
                    String::from_utf8_lossy(p)
                ),
            );
        }
    }
    // 2: an unforced undo leaves the person's changes.
    if !forced {
        for p in &person_changed {
            if d1.get(p) != d0.get(p) {
                return fail(
                    "2",
                    format!(
                        "unforced undo changed the person's {:?}",
                        String::from_utf8_lossy(p)
                    ),
                );
            }
        }
    }
    // 4: what is named is as said.
    let touched: Vec<Vec<u8>> = done
        .plan
        .writes
        .iter()
        .map(|(_, _, p)| p.clone())
        .chain(done.plan.deletes.iter().cloned())
        .collect();
    let set: std::collections::HashSet<&[u8]> = touched.iter().map(Vec::as_slice).collect();
    let above: std::collections::HashSet<&[u8]> = touched.iter().flat_map(|p| parents(p)).collect();
    for p in differ(&d0, &d1) {
        if !within(p, &set) && !above.contains(p.as_slice()) {
            return fail(
                "4",
                format!("{:?} changed, not named", String::from_utf8_lossy(p)),
            );
        }
    }
    let was = tree(&repo, &b).await;
    for p in &touched {
        if d1.get(p) != was.get(p) {
            return fail(
                "4",
                format!("{:?} is not as before the turn", String::from_utf8_lossy(p)),
            );
        }
    }
    if !person {
        if let Some(e) = &refusal
            && !e.contains("in no checkpoint")
            && !e.contains("git repository")
        {
            return fail("4", format!("refused with no change by the person: {e}"));
        }
        let plain = |d: &Disk| -> Disk {
            d.iter()
                .filter(|(p, _)| !is_ignored(p))
                .map(|(p, e)| (p.clone(), e.clone()))
                .collect()
        };
        if plain(&d1) != plain(&before_disk) {
            return fail("4", "not the folder before the turn".into());
        }
    }
    // 5: undoing the undo.
    let here = snap("refs/harness/s/u/after").await?;
    restore_report(&repo, &now, &here, true)
        .await
        .map_err(|e| format!("5: undo of the undo failed: {e}{trees}"))?;
    let d2 = disk(&dir);
    let back: Vec<String> = differ(&d0, &d2)
        .into_iter()
        .filter(|p| !named_unsaved(p))
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .collect();
    if !back.is_empty() {
        return fail(
            "5",
            format!("undo of the undo differs at {back:?}: {}", show(&d2)),
        );
    }
    Ok(match (person, forced) {
        (false, false) => "no person, unforced",
        (false, true) => "no person, forced",
        (true, false) => "person, unforced",
        (true, true) => "person, refused then forced",
    })
}

#[tokio::test]
async fn undo_invariants_hold_on_random_trees() {
    if let Ok(s) = std::env::var("UNDO_SEED") {
        let seed: u64 = s.parse().unwrap();
        if let Err(e) = scenario(seed).await {
            panic!("seed {seed}: {e}");
        }
        return;
    }
    let n: u64 = std::env::var("UNDO_SEEDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(200);
    let mut failed = Vec::new();
    let mut kinds: BTreeMap<&str, u64> = BTreeMap::new();
    for seed in 0..n {
        match scenario(seed).await {
            Ok(k) => *kinds.entry(k).or_default() += 1,
            Err(e) => failed.push(format!("seed {seed}: {e}")),
        }
    }
    eprintln!("{n} scenarios: {kinds:?}");
    assert!(
        failed.is_empty(),
        "{} of {n} failed:\n{}",
        failed.len(),
        failed.join("\n\n")
    );
}
