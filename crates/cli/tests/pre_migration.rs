//! The copy before each migration and its rollback (FIX_PLAN 4.4, second
//! half; docs/backup-restore.md), on the real binary: a start that will
//! migrate copies all four databases first; a start that cannot write the
//! copy stops before migrating anything (unless the operator says
//! `--migrate-without-copy`); a migration failing on the second database
//! names the rollback, and `restore --pre-migration` puts every database
//! back at its old version, keeping the failed ones aside.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

#[path = "support/serve.rs"]
#[allow(dead_code)] // the helpers other tests use
mod real;

use real::EXE;

const DEADLINE: Duration = Duration::from_secs(120);
/// A start that must be refused is refused within this.
const REFUSED_WITHIN: Duration = Duration::from_secs(20);

fn cmd(data: &Path, args: &[&str], env: &[(&str, &str)]) -> Command {
    let mut c = Command::new(EXE);
    c.args(["--data-dir", data.to_str().unwrap()])
        .args(args)
        .current_dir(data.parent().unwrap())
        .env("HARNESS_LOG", "warn")
        .env_remove("HARNESS_TEST_FAIL_MIGRATION")
        .env_remove("HARNESS_TEST_FREE_BYTES")
        .env_remove("HARNESS_MIGRATE_WITHOUT_COPY")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in env {
        c.env(k, v);
    }
    c
}

fn wait_within(mut child: Child, within: Duration, what: &str) -> Output {
    use std::io::Read;
    let mut out = child.stdout.take();
    let mut err = child.stderr.take();
    let o = std::thread::spawn(move || {
        let mut b = Vec::new();
        if let Some(o) = out.as_mut() {
            let _ = o.read_to_end(&mut b);
        }
        b
    });
    let e = std::thread::spawn(move || {
        let mut b = Vec::new();
        if let Some(e) = err.as_mut() {
            let _ = e.read_to_end(&mut b);
        }
        b
    });
    let by = Instant::now() + within;
    let status = loop {
        if let Some(st) = child.try_wait().unwrap() {
            break Some(st);
        }
        if Instant::now() >= by {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let (stdout, stderr) = (o.join().unwrap(), e.join().unwrap());
    let Some(status) = status else {
        panic!(
            "{what}: still running after {within:?}, killed; it printed:\n--- stdout\n{}--- stderr\n{}",
            String::from_utf8_lossy(&stdout),
            String::from_utf8_lossy(&stderr)
        );
    };
    Output {
        status,
        stdout,
        stderr,
    }
}

fn run(data: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    let child = cmd(data, args, env).spawn().unwrap();
    wait_within(child, DEADLINE, &format!("`{}`", args.join(" ")))
}

/// `serve` that must be refused at its start.
fn serve_refused(data: &Path, env: &[(&str, &str)]) -> Output {
    let child = cmd(data, &["serve", "--bind", "127.0.0.1:0"], env)
        .spawn()
        .unwrap();
    wait_within(
        child,
        REFUSED_WITHIN,
        &format!(
            "serve on {} started although it must be refused",
            data.display()
        ),
    )
}

fn text(o: &Output) -> String {
    format!(
        "status {:?}\n--- stdout\n{}--- stderr\n{}",
        o.status.code(),
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn ok(o: Output) -> String {
    assert!(o.status.success(), "{}", text(&o));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn refused(o: Output, why: &str) -> String {
    let t = text(&o);
    assert!(!o.status.success(), "{t}");
    assert!(t.contains(why), "wanted {why:?} in\n{t}");
    t
}

const DBS: [&str; 3] = ["master.db", "chat.db", "node.db"];

fn version(p: &Path) -> i64 {
    let c = rusqlite::Connection::open_with_flags(p, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .unwrap();
    // A server may be migrating the file this moment.
    c.busy_timeout(Duration::from_secs(10)).unwrap();
    c.query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap()
}

fn current() -> i64 {
    harness_store::DB_SCHEMA.steps.len() as i64
}

/// A data directory whose three store databases are at schema version 1
/// (as an older build left them: the recorded migrations up to 1).
fn old_data() -> (tempfile::TempDir, PathBuf) {
    let (dir, data, _ws) = real::setup();
    for name in DBS {
        let mut c = rusqlite::Connection::open(data.join(name)).unwrap();
        harness_store::migrate::migrate(&mut c, name, &harness_store::DB_SCHEMA.steps[..1])
            .unwrap();
    }
    for name in DBS {
        assert_eq!(version(&data.join(name)), 1);
    }
    (dir, data)
}

fn all_private(root: &Path) {
    let m = std::fs::symlink_metadata(root).unwrap();
    assert_eq!(m.permissions().mode() & 0o777, 0o700, "{}", root.display());
    for e in std::fs::read_dir(root).unwrap() {
        let p = e.unwrap().path();
        let m = std::fs::symlink_metadata(&p).unwrap();
        assert!(m.is_file(), "{}", p.display());
        assert_eq!(m.permissions().mode() & 0o777, 0o600, "{}", p.display());
    }
}

fn hash(p: &Path) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(std::fs::read(p).unwrap())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A start that migrates first copies every database, at its old version,
/// into `pre-migration/` with a manifest (0700/0600); the live ones are
/// then at this build's version. A start with nothing to migrate leaves
/// the set as it is.
#[test]
fn a_migrating_start_copies_every_database_first() {
    let (dir, data) = old_data();
    drop(real::serve(&data));
    let set = data.join("pre-migration");
    for name in DBS {
        assert_eq!(version(&set.join(name)), 1, "{name} in the set");
        assert_eq!(version(&data.join(name)), current(), "{name} live");
    }
    assert!(set.join("MANIFEST.json").is_file());
    all_private(&set);
    let m = std::fs::read_to_string(set.join("MANIFEST.json")).unwrap();
    assert!(m.contains("\"pre-migration\""), "{m}");
    let before = hash(&set.join("MANIFEST.json"));
    drop(real::serve(&data));
    assert_eq!(
        before,
        hash(&set.join("MANIFEST.json")),
        "an up-to-date start replaced the set"
    );
    let o = run(&data, &["doctor"], &[]);
    assert!(
        text(&o).contains("pre-migration: a copy of the databases"),
        "{}",
        text(&o)
    );
    drop(dir);
}

/// A migration that fails on the second database (a test hook): the start
/// stops naming the set and the rollback command; the rollback puts all
/// databases back at version 1 and keeps the failed ones aside (0600);
/// this build then starts and migrates them again.
#[test]
fn a_failed_migration_is_rolled_back_from_the_set() {
    let (dir, data) = old_data();
    let t = refused(
        serve_refused(&data, &[("HARNESS_TEST_FAIL_MIGRATION", "node.db")]),
        "restore --pre-migration",
    );
    assert!(t.contains("pre-migration"), "{t}");
    assert_eq!(
        version(&data.join("master.db")),
        current(),
        "the first one migrated"
    );
    assert_eq!(version(&data.join("node.db")), 1);
    let out = ok(run(&data, &["restore", "--pre-migration", "--yes"], &[]));
    assert!(out.contains("start the older build"), "{out}");
    for name in DBS {
        assert_eq!(version(&data.join(name)), 1, "{name} after the rollback");
    }
    let aside: Vec<_> = std::fs::read_dir(&data)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".failed-upgrade-"))
        .collect();
    assert!(
        aside
            .iter()
            .any(|n| n.starts_with("master.db.failed-upgrade-")),
        "{aside:?}"
    );
    for n in &aside {
        let m = std::fs::metadata(data.join(n)).unwrap();
        assert_eq!(m.permissions().mode() & 0o777, 0o600, "{n}");
    }
    let s = real::serve(&data);
    drop(s);
    assert_eq!(version(&data.join("node.db")), current());
    drop(dir);
}

/// No room for the copy (the free space is faked through a test hook):
/// the start stops with both numbers and migrates nothing; with
/// `--migrate-without-copy` it migrates without a set and says so.
#[test]
fn a_start_without_room_for_the_copy_stops() {
    let (dir, data) = old_data();
    let t = refused(
        serve_refused(&data, &[("HARNESS_TEST_FREE_BYTES", "1000")]),
        "free",
    );
    assert!(t.contains("--migrate-without-copy"), "{t}");
    for name in DBS {
        assert_eq!(version(&data.join(name)), 1, "{name} was migrated");
    }
    assert!(!data.join("pre-migration").exists());
    let child = cmd(
        &data,
        &["--migrate-without-copy", "serve", "--bind", "127.0.0.1:0"],
        &[("HARNESS_TEST_FREE_BYTES", "1000")],
    )
    .spawn()
    .unwrap();
    // It starts: give it a moment to migrate, then stop it.
    let by = Instant::now() + DEADLINE;
    while version(&data.join("chat.db")) != current() {
        assert!(Instant::now() < by, "never migrated");
        std::thread::sleep(Duration::from_millis(50));
    }
    let o = wait_after_kill(child);
    assert!(text(&o).contains("WITHOUT a copy"), "{}", text(&o));
    assert!(!data.join("pre-migration").exists());
    drop(dir);
}

fn wait_after_kill(mut child: Child) -> Output {
    let _ = child.kill();
    wait_within(child, DEADLINE, "a killed serve")
}

/// The rollback refuses: no set, an incomplete one, one whose file does
/// not match its manifest, and a directory a server runs on.
#[test]
fn a_rollback_refuses_what_it_cannot_trust() {
    let (dir, data) = old_data();
    refused(
        run(&data, &["restore", "--pre-migration", "--yes"], &[]),
        "no copy",
    );
    drop(real::serve(&data));
    let set = data.join("pre-migration");
    let keep = std::fs::read(set.join("chat.db")).unwrap();
    std::fs::remove_file(set.join("chat.db")).unwrap();
    refused(
        run(&data, &["restore", "--pre-migration", "--yes"], &[]),
        "chat.db",
    );
    let mut flipped = keep.clone();
    let i = flipped.len() / 2;
    flipped[i] ^= 1;
    std::fs::write(set.join("chat.db"), &flipped).unwrap();
    refused(
        run(&data, &["restore", "--pre-migration", "--yes"], &[]),
        "does not match",
    );
    std::fs::write(set.join("chat.db"), &keep).unwrap();
    let s = real::serve(&data);
    refused(
        run(&data, &["restore", "--pre-migration", "--yes"], &[]),
        "is in use by serve",
    );
    drop(s);
    for name in DBS {
        assert_eq!(
            version(&data.join(name)),
            current(),
            "a refused rollback changed {name}"
        );
    }
    drop(dir);
}

/// A backup leaves the set out and says so.
#[test]
fn a_backup_leaves_the_set_out() {
    let (dir, data) = old_data();
    drop(real::serve(&data));
    let dest = data.parent().unwrap().join("backup");
    let out = ok(run(&data, &["backup", dest.to_str().unwrap()], &[]));
    assert!(out.contains("pre-migration"), "{out}");
    assert!(!dest.join("pre-migration").exists());
    assert!(!out.contains("skipped pre-migration"), "{out}");
    drop(dir);
}

fn versions(data: &Path) -> Vec<(String, i64)> {
    DBS.iter()
        .map(|n| (n.to_string(), version(&data.join(n))))
        .collect()
}

fn set_before(data: &Path) -> String {
    let m: serde_json::Value =
        serde_json::from_slice(&std::fs::read(data.join("pre-migration/MANIFEST.json")).unwrap())
            .unwrap();
    m["before"].to_string()
}

const FAIL_NODE: (&str, &str) = ("HARNESS_TEST_FAIL_MIGRATION", "node.db");

/// B1: two (and three) starts whose migration fails on node.db: the set
/// stays the one from before the first, and the rollback brings every
/// database back to 1.
#[test]
fn failing_starts_keep_the_first_set() {
    for starts in [2, 3] {
        let (dir, data) = old_data();
        for _ in 0..starts {
            refused(
                serve_refused(&data, &[FAIL_NODE]),
                "restore --pre-migration",
            );
        }
        assert_eq!(
            set_before(&data),
            r#"{"chat.db":1,"master.db":1,"node.db":1}"#,
            "after {starts} failing starts"
        );
        ok(run(&data, &["restore", "--pre-migration", "--yes"], &[]));
        for (n, v) in versions(&data) {
            assert_eq!(v, 1, "{n} after {starts} failing starts and a rollback");
        }
        drop(dir);
    }
}

/// B1: a failing start, then one that succeeds: the set still holds the
/// state from before the upgrade.
#[test]
fn a_resumed_migration_keeps_the_set() {
    let (dir, data) = old_data();
    refused(
        serve_refused(&data, &[FAIL_NODE]),
        "restore --pre-migration",
    );
    drop(real::serve(&data));
    assert_eq!(
        set_before(&data),
        r#"{"chat.db":1,"master.db":1,"node.db":1}"#
    );
    for (n, v) in versions(&data) {
        assert_eq!(v, current(), "{n}");
    }
    drop(dir);
}

/// B1: databases at mixed versions with no set to match: no honest
/// rollback point can be made, so the start stops (the escape overrides).
#[test]
fn mixed_versions_without_a_set_stop_the_start() {
    let (dir, data) = old_data();
    // master.db one step further than the others, by hand.
    let mut c = rusqlite::Connection::open(data.join("master.db")).unwrap();
    harness_store::migrate::migrate(&mut c, "master.db", &harness_store::DB_SCHEMA.steps[..2])
        .unwrap();
    drop(c);
    let t = refused(serve_refused(&data, &[]), "mixed");
    assert!(t.contains("--migrate-without-copy"), "{t}");
    assert!(!data.join("pre-migration").exists());
    assert_eq!(version(&data.join("chat.db")), 1);
    drop(dir);
}

/// H1: databases at version 0 that hold the first schema (an old
/// installation from before schema versions) are behind: the set is made,
/// with `before` 0.
#[test]
fn version_zero_with_tables_gets_a_set() {
    let zeroed = |mix: bool| {
        let (dir, data, _ws) = real::setup();
        for name in DBS {
            let mut c = rusqlite::Connection::open(data.join(name)).unwrap();
            harness_store::migrate::migrate(&mut c, name, &harness_store::DB_SCHEMA.steps[..1])
                .unwrap();
            c.pragma_update(None, "user_version", 0).unwrap();
        }
        let mut c = rusqlite::Connection::open(data.join("memory.db")).unwrap();
        harness_store::migrate::migrate(&mut c, "memory.db", &harness_memory::DB_SCHEMA.steps[..1])
            .unwrap();
        c.pragma_update(None, "user_version", 0).unwrap();
        drop(c);
        if mix {
            // chat.db at 1 (0 with tables is adopted as 1).
            let c = rusqlite::Connection::open(data.join("chat.db")).unwrap();
            c.pragma_update(None, "user_version", 1).unwrap();
        }
        drop(real::serve(&data));
        assert!(data.join("pre-migration").exists(), "mix {mix}: no set");
        let want = if mix {
            r#"{"chat.db":1,"master.db":0,"memory.db":0,"node.db":0}"#
        } else {
            r#"{"chat.db":0,"master.db":0,"memory.db":0,"node.db":0}"#
        };
        assert_eq!(set_before(&data), want);
        drop(dir);
    };
    zeroed(false);
    zeroed(true);
}

/// M1: a start that fails for another reason after master.db was migrated
/// (a debug-only hook) still prints the rollback advice.
#[test]
fn any_failure_after_a_migration_names_the_rollback() {
    let (dir, data) = old_data();
    refused(
        serve_refused(&data, &[("HARNESS_TEST_FAIL_AFTER_OPEN", "master.db")]),
        "restore --pre-migration",
    );
    assert_eq!(version(&data.join("master.db")), current());
    drop(dir);
}

/// M2: a set that cannot be written (a leftover partial folder this user
/// cannot empty) stops the start before anything is migrated.
#[test]
fn an_unwritable_copy_stops_the_start() {
    let (dir, data) = old_data();
    let p = data.join("pre-migration.partial");
    std::fs::create_dir(&p).unwrap();
    std::fs::write(p.join("x"), b"x").unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o500)).unwrap();
    let t = refused(serve_refused(&data, &[]), "could not be written");
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(t.contains("--migrate-without-copy"), "{t}");
    for (n, v) in versions(&data) {
        assert_eq!(v, 1, "{n} was migrated");
    }
    drop(dir);
}

/// M3: without `--yes` the rollback says what it would do and changes
/// nothing.
#[test]
fn a_rollback_without_yes_only_tells() {
    let (dir, data) = old_data();
    refused(
        serve_refused(&data, &[FAIL_NODE]),
        "restore --pre-migration",
    );
    let before = versions(&data);
    let t = refused(run(&data, &["restore", "--pre-migration"], &[]), "--yes");
    for s in ["master.db", "now", "failed-upgrade", "stored files"] {
        assert!(t.contains(s), "{s} missing in\n{t}");
    }
    assert_eq!(before, versions(&data));
    drop(dir);
}

fn blob_files(data: &Path) -> Vec<PathBuf> {
    let mut v = Vec::new();
    let Ok(rd) = std::fs::read_dir(data.join("blobs")) else {
        return v;
    };
    for shard in rd.flatten() {
        if shard.file_name().len() != 2 {
            continue;
        }
        for f in std::fs::read_dir(shard.path()).unwrap().flatten() {
            v.push(f.path());
        }
    }
    v.sort();
    v
}

/// B2: the rollback killed at each step (a debug-only pause hook): every
/// command then refuses the directory, and running the rollback again
/// ends with every database at its version from before and the stored
/// files where they were.
#[test]
fn a_rollback_killed_at_any_step_can_be_run_again() {
    for step in ["marked", "copied", "half-placed", "placed", "blobs-moved"] {
        let (dir, data) = old_data();
        // A stored file the set's master.db knows nothing of, like one
        // uploaded after the upgrade.
        let shard = data.join("blobs/aa");
        std::fs::create_dir_all(&shard).unwrap();
        std::fs::write(shard.join("0123456789abcdef0123456789abcdaa.rzb"), b"x").unwrap();
        refused(
            serve_refused(&data, &[FAIL_NODE]),
            "restore --pre-migration",
        );
        let pause = data.parent().unwrap().join(format!("pause-{step}"));
        let mut child = cmd(
            &data,
            &["restore", "--pre-migration", "--yes"],
            &[
                ("HARNESS_ROLLBACK_PAUSE", step),
                ("HARNESS_ROLLBACK_PAUSE_FILE", pause.to_str().unwrap()),
            ],
        )
        .spawn()
        .unwrap();
        let mark = PathBuf::from(format!("{}.snapshotted", pause.display()));
        let by = Instant::now() + DEADLINE;
        while !mark.exists() {
            assert!(
                Instant::now() < by,
                "{step}: the rollback never reached its pause"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        child.kill().unwrap();
        child.wait().unwrap();
        let t = refused(serve_refused(&data, &[]), "restore --pre-migration");
        assert!(!t.contains("claim code"), "{step}: {t}");
        ok(run(&data, &["restore", "--pre-migration", "--yes"], &[]));
        for (n, v) in versions(&data) {
            assert_eq!(v, 1, "{step}: {n}");
        }
        assert!(!data.join("RESTORING").exists(), "{step}");
        assert_eq!(
            set_before(&data),
            r#"{"chat.db":1,"master.db":1,"node.db":1}"#,
            "{step}"
        );
        // The orphan was moved aside, not deleted.
        let aside: Vec<_> = std::fs::read_dir(&data)
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("blobs.failed-upgrade-")
            })
            .collect();
        assert!(
            !aside.is_empty(),
            "{step}: the stored file was not kept aside"
        );
        drop(dir);
    }
}

/// B2c: a start on a master.db with no stored-file rows while files lie in
/// blobs/ does not delete them (an empty table with files on disk is a
/// broken or mismatched directory).
#[test]
fn an_empty_blob_table_never_wipes_the_files() {
    let (dir, data, _ws) = real::setup();
    drop(real::serve(&data));
    let shard = data.join("blobs/aa");
    std::fs::create_dir_all(&shard).unwrap();
    let f = shard.join("0123456789abcdef0123456789abcdaa.rzb");
    std::fs::write(&f, b"x").unwrap();
    drop(real::serve(&data));
    assert!(f.exists(), "the start deleted every stored file");
    drop(dir);
}

/// B2d: databases missing while pre-migration/, failed-upgrade files or
/// stored files are there is no first run: refused, naming the rollback.
#[test]
fn missing_databases_beside_a_set_are_no_first_run() {
    let (dir, data) = old_data();
    refused(
        serve_refused(&data, &[FAIL_NODE]),
        "restore --pre-migration",
    );
    std::fs::remove_file(data.join("master.db")).unwrap();
    let t = refused(serve_refused(&data, &[]), "restore --pre-migration");
    assert!(!t.contains("claim"), "{t}");
    assert!(!data.join("master.db").exists());
    drop(dir);
    // Only stored files, no database at all.
    let (dir, data, _ws) = real::setup();
    let shard = data.join("blobs/aa");
    std::fs::create_dir_all(&shard).unwrap();
    std::fs::write(shard.join("0123456789abcdef0123456789abcdaa.rzb"), b"x").unwrap();
    let t = refused(serve_refused(&data, &[]), "stored files");
    assert!(!data.join("master.db").exists(), "{t}");
    let _ = blob_files(&data);
    drop(dir);
}
