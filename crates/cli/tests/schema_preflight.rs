//! Schema versions across the data directory (fix plan 4.3, review R43-3),
//! on the real binary: `serve` on a data directory in which one database
//! was written by a newer build refuses before it creates, migrates or
//! sweeps anything, naming that file and leaving it byte-identical. Every
//! database is checked first, so no file is migrated before a later one is
//! found to be newer.

use std::io::{BufRead, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use rusqlite::Connection;

const EXE: &str = env!("CARGO_BIN_EXE_rizzy-harness");
const DATABASES: [&str; 4] = ["master.db", "chat.db", "node.db", "memory.db"];

fn data_dir(root: &Path) -> std::path::PathBuf {
    let data = root.join("data");
    std::fs::create_dir(&data).unwrap();
    std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o700)).unwrap();
    data
}

fn serve(root: &Path, data: &Path) -> Command {
    let mut cmd = Command::new(EXE);
    cmd.args(["--data-dir", data.to_str().unwrap()])
        .args(["serve", "--bind", "127.0.0.1:0"])
        .current_dir(root)
        .stdin(Stdio::null());
    for (k, _) in std::env::vars_os() {
        if k.to_string_lossy().starts_with("HARNESS_") {
            cmd.env_remove(k);
        }
    }
    cmd.env("HARNESS_LOG", "warn");
    cmd
}

fn listing(data: &Path) -> Vec<String> {
    let mut left: Vec<String> = std::fs::read_dir(data)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    left
}

/// Two kinds of newer file: one in rollback-journal mode made by a raw
/// connection, and one in WAL mode as the harness itself writes it
/// (`Store::open`, then a newer version stamped on it).
#[test]
fn a_newer_database_anywhere_in_the_data_directory_stops_serve_before_anything_is_touched() {
    for wal in [false, true] {
        for newer in DATABASES {
            let case = format!("{newer} ({})", if wal { "WAL" } else { "rollback journal" });
            let root = tempfile::tempdir().unwrap();
            let data = data_dir(root.path());
            let path = data.join(newer);
            if wal {
                drop(harness_store::Store::open(&path, harness_store::Durability::Full).unwrap());
            }
            {
                let c = Connection::open(&path).unwrap();
                if !wal {
                    c.execute_batch("CREATE TABLE t (x INTEGER); INSERT INTO t VALUES (1);")
                        .unwrap();
                }
                c.pragma_update(None, "user_version", 962001).unwrap();
            }
            assert_eq!(listing(&data), vec![newer.to_string()], "{case}");
            let before = std::fs::read(&path).unwrap();
            let mut child = serve(root.path(), &data)
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(60);
            let status = loop {
                if let Some(s) = child.try_wait().unwrap() {
                    break s;
                }
                if Instant::now() > deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("{case}: serve did not refuse within 60 s");
                }
                std::thread::sleep(Duration::from_millis(50));
            };
            let mut stderr = String::new();
            std::io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut stderr).unwrap();
            assert!(!status.success(), "{case}: serve exited 0\n{stderr}");
            assert!(
                stderr.contains(newer) && stderr.contains("newer rizzy-harness"),
                "{case}: {stderr}"
            );
            assert_eq!(std::fs::read(&path).unwrap(), before, "{case}");
            // The lock file is taken first, by design (A.20). Beside a WAL
            // file the read-only connection leaves an empty `-wal` and a
            // `-shm` (re-check D2); the database file itself is unchanged.
            let mut expected = vec!["lock".to_string(), newer.to_string()];
            if wal {
                expected.push(format!("{newer}-shm"));
                expected.push(format!("{newer}-wal"));
                let wal_file = data.join(format!("{newer}-wal"));
                assert_eq!(std::fs::metadata(&wal_file).unwrap().len(), 0, "{case}");
            }
            expected.sort();
            assert_eq!(listing(&data), expected, "{case}");
        }
    }
}

/// Re-check D1 on the real binary: `master.db`'s first creation was cut
/// off with a hot rollback journal beside it (pages written, no commit).
/// The preflight refused that on every start; `serve` must recover it and
/// start, as it did before the preflight.
#[test]
fn serve_starts_on_a_data_directory_whose_first_creation_was_interrupted() {
    let root = tempfile::tempdir().unwrap();
    let data = data_dir(root.path());
    let live = root.path().join("live");
    std::fs::create_dir(&live).unwrap();
    let c = Connection::open(live.join("master.db")).unwrap();
    c.pragma_update(None, "cache_size", 10).unwrap();
    c.execute_batch("BEGIN IMMEDIATE;").unwrap();
    for step in harness_store::MIGRATIONS {
        for sql in *step {
            c.execute_batch(sql).unwrap();
        }
    }
    for name in ["master.db", "master.db-journal"] {
        std::fs::copy(live.join(name), data.join(name)).unwrap();
    }
    c.execute_batch("ROLLBACK;").unwrap();
    drop(c);
    assert!(std::fs::metadata(data.join("master.db")).unwrap().len() > 0);

    let mut child = serve(root.path(), &data)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stderr = child.stderr.take().unwrap();
    let errors = std::thread::spawn(move || {
        let mut s = String::new();
        std::io::Read::read_to_string(&mut BufReader::new(stderr), &mut s).unwrap();
        s
    });
    let listening = BufReader::new(child.stdout.take().unwrap())
        .lines()
        .map_while(Result::ok)
        .any(|l| l.contains("listening on http://"));
    let _ = child.kill();
    let _ = child.wait();
    let errors = errors.join().unwrap();
    assert!(listening, "serve did not start: {errors}");
    let version: i64 = Connection::open(data.join("master.db"))
        .unwrap()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, harness_store::MIGRATIONS.len() as i64);
    assert!(!data.join("master.db-journal").exists());
}
