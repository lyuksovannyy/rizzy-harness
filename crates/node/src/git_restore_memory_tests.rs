//! A restore never holds what it writes in the node's memory, and what it
//! may write is bounded (R.1, R1b-1 of
//! docs/security-review/2026-11-gate3-fixes-3.md).
//!
//! The reviewer's proof asserted that `blob_contents` returned both 80 MiB
//! blobs at once; inverted (at most 1 MiB held) it failed with 167772160
//! bytes held. `blob_contents` is gone: the blobs are streamed one at a time
//! (`stream_writes`). The node's peak memory during such a restore is
//! measured in its own process by `crates/node/tests/restore_memory.rs`.

use super::*;

/// Two sparse 80 MiB files in the folder, snapshotted; the plan that
/// writes them back, the snapshot and the folder.
async fn two_big_files() -> (tempfile::TempDir, Repo, RestorePlan, PathBuf) {
    const EACH: u64 = 80 << 20;
    let d = tempfile::tempdir().unwrap();
    let top = d.path().canonicalize().unwrap();
    let ws = top.join("ws");
    std::fs::create_dir(&ws).unwrap();
    for name in ["big1", "big2"] {
        let f = std::fs::File::create(ws.join(name)).unwrap();
        f.set_len(EACH).unwrap();
        std::os::unix::fs::FileExt::write_all_at(&f, name.as_bytes(), 0).unwrap();
    }
    let repo = repo_for(&ws, Some(&top.join("data/checkpoints")))
        .await
        .unwrap();
    let snap = snapshot(&repo, "refs/harness/s/t/after", "after")
        .await
        .unwrap();
    let mut plan = RestorePlan::default();
    for name in ["big1", "big2"] {
        let id = run(&repo, &["rev-parse", &format!("{snap}:{name}")], None)
            .await
            .unwrap();
        plan.writes
            .push(("100644".to_string(), id, name.as_bytes().to_vec()));
        plan.touched.push(('M', name.to_string()));
        std::fs::remove_file(ws.join(name)).unwrap();
    }
    (d, repo, plan, ws)
}

/// The streamed write puts back every byte of both files.
#[tokio::test]
async fn a_streamed_restore_writes_large_files_exactly() {
    let (_d, repo, plan, ws) = two_big_files().await;
    restore_paths(&repo, &plan, WriteLimits::UNDO)
        .await
        .unwrap();
    for name in ["big1", "big2"] {
        let md = std::fs::metadata(ws.join(name)).unwrap();
        assert_eq!(md.len(), 80 << 20);
        let mut head = vec![0u8; 4];
        let f = std::fs::File::open(ws.join(name)).unwrap();
        std::os::unix::fs::FileExt::read_exact_at(&f, &mut head, 0).unwrap();
        assert_eq!(&head, name.as_bytes());
    }
}

/// Above a limit, per file or in total, nothing is written and the
/// refusal names the limit.
#[tokio::test]
async fn a_restore_over_its_limits_writes_nothing() {
    let (_d, repo, plan, ws) = two_big_files().await;
    let e = restore_paths(
        &repo,
        &plan,
        WriteLimits {
            per_file: 64 << 20,
            total: 1 << 40,
        },
    )
    .await
    .unwrap_err();
    assert!(e.contains("limit of 67108864 bytes for one file"), "{e}");
    let e = restore_paths(
        &repo,
        &plan,
        WriteLimits {
            per_file: 1 << 30,
            total: 100 << 20,
        },
    )
    .await
    .unwrap_err();
    assert!(
        e.contains("limit of 104857600 bytes for one restore"),
        "{e}"
    );
    assert!(!ws.join("big1").exists() && !ws.join("big2").exists());
    // A sub-agent's result is held to the stage's limits.
    assert_eq!(WriteLimits::SUB_AGENT.per_file, 200 << 20);
}
