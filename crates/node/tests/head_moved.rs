//! A turn that moved the person's HEAD says so (X.42, PLAN.md T33, the
//! `checkpoint` row): the `after` checkpoint carries `head_moved` only when
//! the branch or the commit HEAD names differs from the turn's start. HEAD
//! is read from the repository's files by the node's own code, never by a
//! git process, so a hostile `.git/config` runs nothing. Driven by the
//! scripted agent (`run: <shell>`).

#[path = "support/rig.rs"]
mod support;

use harness_core::event::*;
use serde_json::{Value, json};
use support::{AGENT_GIT, Rig};

async fn repo_rig() -> (Rig, String) {
    let r = Rig::new().await;
    r.git(&["init", "-q"]);
    r.write("a.txt", "a\n");
    r.git(&["add", "."]);
    r.git(&["commit", "-qm", "init"]);
    let c1 = r.git(&["rev-parse", "HEAD"]);
    (r, c1)
}

/// The `after` checkpoint of `turn`, as JSON (so a missing field is a
/// failed assertion, not a compile error).
fn after(rig: &Rig, turn: &str) -> Value {
    rig.seen
        .iter()
        .filter_map(|e| match e.typed() {
            Ok(EventKind::Checkpoint(c))
                if c.turn_id == turn && c.phase == CheckpointPhase::After =>
            {
                Some(serde_json::to_value(&c).unwrap())
            }
            _ => None,
        })
        .next_back()
        .expect("an after checkpoint")
}

fn moved(rig: &Rig, turn: &str) -> Option<Value> {
    after(rig, turn).get("head_moved").cloned()
}

#[tokio::test]
async fn a_branch_switch_is_recorded() {
    let (mut rig, c1) = repo_rig().await;
    let t = rig
        .turn(&format!(
            "{AGENT_GIT}; git checkout -q -b x; echo n > n.txt"
        ))
        .await;
    let turn = t.id.unwrap();
    assert_eq!(
        moved(&rig, &turn),
        Some(json!({
            "from": {"branch": "main", "commit": c1},
            "to": {"branch": "x", "commit": c1},
        }))
    );
}

#[tokio::test]
async fn a_commit_on_the_same_branch_is_recorded() {
    let (mut rig, c1) = repo_rig().await;
    let t = rig
        .turn(&format!(
            "{AGENT_GIT}; echo b > b.txt; git add b.txt; git commit -qm two"
        ))
        .await;
    let turn = t.id.unwrap();
    let c2 = rig.git(&["rev-parse", "HEAD"]);
    assert_ne!(c1, c2);
    assert_eq!(
        moved(&rig, &turn),
        Some(json!({
            "from": {"branch": "main", "commit": c1},
            "to": {"branch": "main", "commit": c2},
        }))
    );
}

/// A branch whose ref sits in `packed-refs` is read the same way.
#[tokio::test]
async fn a_packed_branch_is_read() {
    let (mut rig, c1) = repo_rig().await;
    rig.git(&["pack-refs", "--all"]);
    let t = rig
        .turn(&format!(
            "{AGENT_GIT}; git checkout -q -b y; echo n > n.txt"
        ))
        .await;
    let turn = t.id.unwrap();
    let m = moved(&rig, &turn).unwrap();
    assert_eq!(m["from"], json!({"branch": "main", "commit": c1}));
    assert_eq!(m["to"], json!({"branch": "y", "commit": c1}));
}

#[tokio::test]
async fn a_turn_that_only_changes_files_records_nothing() {
    let (mut rig, _) = repo_rig().await;
    let t = rig.turn("echo more >> a.txt; echo n > n.txt").await;
    let turn = t.id.unwrap();
    assert_eq!(moved(&rig, &turn), None);
    // A branch switch that ends where it began is no move either.
    let t = rig
        .turn(&format!(
            "{AGENT_GIT}; git checkout -q -b z; git checkout -q main"
        ))
        .await;
    assert_eq!(moved(&rig, &t.id.unwrap()), None);
}

#[tokio::test]
async fn a_detached_head_has_no_branch() {
    let (mut rig, c1) = repo_rig().await;
    let t = rig
        .turn(&format!("{AGENT_GIT}; git checkout -q --detach"))
        .await;
    assert_eq!(
        moved(&rig, &t.id.unwrap()),
        Some(json!({
            "from": {"branch": "main", "commit": c1},
            "to": {"commit": c1},
        }))
    );
    // And back from detached.
    let t = rig
        .turn(&format!("{AGENT_GIT}; git checkout -q main"))
        .await;
    assert_eq!(
        moved(&rig, &t.id.unwrap()),
        Some(json!({
            "from": {"commit": c1},
            "to": {"branch": "main", "commit": c1},
        }))
    );
}

/// The names of branches are text from the repository: the ref files are
/// written directly (git refuses some of these names).
#[tokio::test]
async fn a_hostile_branch_name_is_cut_and_cleaned() {
    let (mut rig, c1) = repo_rig().await;
    // An escape sequence, a line separator and a right-to-left override.
    let t = rig
        .turn(
            "N=$(printf 'a\\033[31m\\342\\200\\250x\\342\\200\\256y\\007z'); \
             cp .git/refs/heads/main \".git/refs/heads/$N\"; \
             printf 'ref: refs/heads/%s\\n' \"$N\" > .git/HEAD",
        )
        .await;
    let m = moved(&rig, &t.id.unwrap()).expect("moved");
    let to = m["to"]["branch"].as_str().unwrap();
    assert_eq!(m["to"]["commit"], json!(c1));
    assert!(
        !to.chars().any(|c| c.is_control()
            || matches!(c, '\u{2028}' | '\u{2029}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')),
        "{to:?}"
    );
    assert!(to.starts_with('a') && to.ends_with('z'), "{to:?}");

    // A 5 KB name (in `packed-refs`: a file name holds 255 bytes).
    let t = rig
        .turn(
            "N=$(head -c 5000 /dev/zero | tr '\\0' 'q'); \
             printf '%s refs/heads/%s\\n' \"$(cat .git/refs/heads/main)\" \"$N\" > .git/packed-refs; \
             printf 'ref: refs/heads/%s\\n' \"$N\" > .git/HEAD",
        )
        .await;
    let m = moved(&rig, &t.id.unwrap()).expect("moved");
    let to = m["to"]["branch"].as_str().unwrap();
    assert!(!to.is_empty() && to.len() <= 200, "{} bytes", to.len());
    assert!(to.bytes().all(|b| b == b'q'));
    // The `from` side of that turn was the hostile first name, cleaned too.
    assert!(
        !m["from"]["branch"]
            .as_str()
            .unwrap()
            .chars()
            .any(|c| c.is_control() || c == '\u{2028}' || c == '\u{202E}')
    );
}

/// A multi-byte name is cut on a character boundary.
#[tokio::test]
async fn a_long_multibyte_name_is_cut_on_a_character_boundary() {
    let (mut rig, _) = repo_rig().await;
    let t = rig
        .turn(
            "N=$(i=0; while [ $i -lt 300 ]; do printf '\\303\\274'; i=$((i+1)); done); \
             cp .git/refs/heads/main \".git/refs/heads/u\"; \
             printf '%s refs/heads/%s\\n' \"$(cat .git/refs/heads/main)\" \"$N\" > .git/packed-refs; \
             printf 'ref: refs/heads/%s\\n' \"$N\" > .git/HEAD",
        )
        .await;
    let m = moved(&rig, &t.id.unwrap()).expect("moved");
    let to = m["to"]["branch"].as_str().unwrap();
    assert!(
        to.len() <= 200 && to.chars().all(|c| c == '\u{fc}'),
        "{to:?}"
    );
    assert_eq!(to.len() % 2, 0);
}

/// A folder that is no repository, and one whose `.git` is a file, record
/// nothing, whatever happens to the repository the file points at.
#[tokio::test]
async fn a_plain_folder_and_a_gitfile_record_nothing() {
    let mut rig = Rig::new().await;
    let t = rig.turn("echo x > f.txt").await;
    assert_eq!(moved(&rig, &t.id.unwrap()), None);

    let (mut rig, _) = repo_rig().await;
    // The workspace's `.git` becomes a gitfile naming a real repository.
    let other = rig.dir.path().join("other.git");
    std::fs::rename(rig.ws.join(".git"), &other).unwrap();
    std::fs::write(
        rig.ws.join(".git"),
        format!("gitdir: {}\n", other.display()),
    )
    .unwrap();
    let t = rig
        .turn(&format!(
            "printf 'ref: refs/heads/other\\n' > '{}/HEAD'; echo x > g.txt",
            other.display()
        ))
        .await;
    assert_eq!(moved(&rig, &t.id.unwrap()), None);
}

/// A repository with a hostile `.git/config` (fsmonitor, hooks): the node
/// runs none of it while it reads HEAD (it reads files, it starts no git
/// for it).
#[cfg(unix)]
#[tokio::test]
async fn a_hostile_config_runs_nothing_while_head_is_read() {
    use std::os::unix::fs::PermissionsExt;
    let (mut rig, c1) = repo_rig().await;
    let marks = rig.dir.path().join("marks");
    std::fs::create_dir(&marks).unwrap();
    let script = |name: &str| {
        let p = rig.dir.path().join(format!("{name}.sh"));
        std::fs::write(
            &p,
            format!(
                "#!/bin/sh\ntouch '{}'\nexit 0\n",
                marks.join(name).display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p.display().to_string()
    };
    rig.git(&["config", "core.fsmonitor", &script("fsmonitor")]);
    for n in [
        "post-index-change",
        "reference-transaction",
        "post-checkout",
    ] {
        let p = rig.ws.join(".git/hooks").join(n);
        std::fs::copy(script(n), &p).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    // Moves HEAD without git, so no hook of the agent's own runs.
    let t = rig
        .turn(
            "cp .git/refs/heads/main .git/refs/heads/x; \
             printf 'ref: refs/heads/x\\n' > .git/HEAD",
        )
        .await;
    let m = moved(&rig, &t.id.unwrap());
    assert_eq!(
        m,
        Some(json!({
            "from": {"branch": "main", "commit": c1},
            "to": {"branch": "x", "commit": c1},
        }))
    );
    let ran: Vec<_> = std::fs::read_dir(&marks)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert!(ran.is_empty(), "repository config ran: {ran:?}");
}
