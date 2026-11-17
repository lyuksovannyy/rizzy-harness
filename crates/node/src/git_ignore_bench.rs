//! What a folder only the user's global git ignore file named costs the
//! node's git once checkpoints no longer follow that file (owner's
//! decision of 2026-11-17, ADOPTION_PLAN X.31): a first and a second
//! checkpoint, the store's size, and one sub-agent's folder (base, fill,
//! result). Ignored benchmark; the numbers are in `docs/undo-and-stash.md`.
//!
//! ```text
//! GI_DIR=/some/real/disk \
//!   cargo test -p harness-node --lib globally_ignored_tree_cost -- --ignored --nocapture
//! ```
//!
//! `GI_FILES` (default 20,000) files of `GI_BYTES` (default 15,000) bytes
//! of text under `deps/`, which the repository's own `.gitignore` does not
//! name. Everything is made under a temporary folder in `GI_DIR` and removed
//! at the end.

use super::*;
use std::time::Instant;

fn env_or(k: &str, d: usize) -> usize {
    std::env::var(k)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(d)
}

/// `bytes` of source-like text for file `i`, different in every file
/// (xorshift), so it compresses about as well as a dependency folder.
fn text(i: usize, bytes: usize) -> Vec<u8> {
    let mut x = (i as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
    let mut v = Vec::with_capacity(bytes + 64);
    let mut k = 0;
    while v.len() < bytes {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        v.extend_from_slice(format!("export const v{i}_{k} = \"{x:016x}\";\n").as_bytes());
        k += 1;
    }
    v.truncate(bytes);
    v
}

fn du(p: &Path) -> u64 {
    use std::os::unix::fs::MetadataExt;
    let mut n = 0;
    let mut stack = vec![p.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let Ok(m) = e.metadata() else { continue };
            if m.is_dir() {
                stack.push(e.path());
            } else {
                n += m.blocks() * 512;
            }
        }
    }
    n
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn globally_ignored_tree_cost() {
    let parent = std::env::var("GI_DIR").expect("GI_DIR: a folder on a real disk");
    let (files, bytes) = (env_or("GI_FILES", 20_000), env_or("GI_BYTES", 15_000));
    let d = tempfile::tempdir_in(parent).unwrap();
    let ws = d.path().join("ws");
    std::fs::create_dir_all(ws.join("src")).unwrap();
    for i in 0..100 {
        std::fs::write(ws.join(format!("src/f{i}.rs")), text(i, 2000)).unwrap();
    }
    std::fs::write(ws.join(".gitignore"), "*.log\n").unwrap();
    tests::sh(&ws, &["init", "-q", "-b", "main"]);
    tests::sh(&ws, &["add", "."]);
    tests::sh(&ws, &["commit", "-qm", "init", "--no-gpg-sign"]);
    let t = Instant::now();
    for i in 0..files {
        let dir = ws.join(format!("deps/p{}/lib", i / 100));
        if i % 100 == 0 {
            std::fs::create_dir_all(&dir).unwrap();
        }
        std::fs::write(dir.join(format!("m{i}.js")), text(i + 1000, bytes)).unwrap();
    }
    eprintln!(
        "tree: {files} files, {} MiB apparent, {} MiB on disk, written in {:?}",
        (files * bytes) >> 20,
        du(&ws.join("deps")) >> 20,
        t.elapsed()
    );
    // origin/main before X.31, with a global ignore file naming `deps/`,
    // took 8 ms for each checkpoint and stored nothing of `deps/` (see the
    // doc for the full table).
    let bounds = Bounds::default();
    let base = d.path().join("cp");

    let repo = checkpoint_repo(&ws, Some(&base), &bounds)
        .await
        .unwrap()
        .unwrap();
    for run in 1..=2 {
        let t = Instant::now();
        let r = snapshot(&repo, &format!("refs/harness/s/t{run}/before"), "b").await;
        let took = t.elapsed();
        let n = match &r {
            Ok(sha) => paths_in(&repo, sha).await,
            Err(_) => 0,
        };
        eprintln!("checkpoint {run}: {took:?} ({n} paths, {:?})", r.err());
    }
    eprintln!("checkpoint store: {} MiB", du(&base) >> 20);

    let sub = sub_repo(&ws, Some(&base), &bounds).await.unwrap().unwrap();
    let t = Instant::now();
    let b = sub.base("c1").await.unwrap();
    eprintln!("sub-agent base: {:?}", t.elapsed());
    let t = Instant::now();
    let folder = sub
        .make_folder("c1", &b, &d.path().join("sessions"))
        .await
        .unwrap();
    eprintln!(
        "sub-agent folder filled: {:?}, {} MiB",
        t.elapsed(),
        du(&folder.path) >> 20
    );
    let t = Instant::now();
    sub.result(&folder, "c1").await.unwrap();
    eprintln!("sub-agent result: {:?}", t.elapsed());
    remove_folder(&folder).await;
    eprintln!(
        "checkpoint store after the sub-agent: {} MiB",
        du(&base) >> 20
    );
}

async fn paths_in(repo: &Repo, sha: &str) -> usize {
    run(repo, &["ls-tree", "-r", "--name-only", sha], None)
        .await
        .map(|s| s.lines().count())
        .unwrap_or(0)
}
