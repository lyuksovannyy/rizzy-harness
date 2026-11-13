//! Schema versions across the data directory (fix plan 4.3, review R43-3),
//! on the real binary: `serve` on a data directory in which one database
//! was written by a newer build refuses before it creates, migrates or
//! sweeps anything, naming that file and leaving it byte-identical. Every
//! database is checked first, so no file is migrated before a later one is
//! found to be newer.

use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use rusqlite::Connection;

const EXE: &str = env!("CARGO_BIN_EXE_rizzy-harness");
const DATABASES: [&str; 4] = ["master.db", "chat.db", "node.db", "memory.db"];

#[test]
fn a_newer_database_anywhere_in_the_data_directory_stops_serve_before_anything_is_touched() {
    for newer in DATABASES {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        std::fs::create_dir(&data).unwrap();
        std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = data.join(newer);
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch("CREATE TABLE t (x INTEGER); INSERT INTO t VALUES (1);")
                .unwrap();
            c.pragma_update(None, "user_version", 962001).unwrap();
        }
        let before = std::fs::read(&path).unwrap();
        let mut cmd = Command::new(EXE);
        cmd.args(["--data-dir", data.to_str().unwrap()])
            .args(["serve", "--bind", "127.0.0.1:0"])
            .current_dir(root.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        for (k, _) in std::env::vars_os() {
            if k.to_string_lossy().starts_with("HARNESS_") {
                cmd.env_remove(k);
            }
        }
        cmd.env("HARNESS_LOG", "warn");
        let mut child = cmd.spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        let status = loop {
            if let Some(s) = child.try_wait().unwrap() {
                break s;
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("{newer}: serve did not refuse within 60 s");
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let mut stderr = String::new();
        std::io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut stderr).unwrap();
        assert!(!status.success(), "{newer}: serve exited 0\n{stderr}");
        assert!(
            stderr.contains(newer) && stderr.contains("newer rizzy-harness"),
            "{newer}: {stderr}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), before, "{newer}");
        let mut left: Vec<String> = std::fs::read_dir(&data)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        // The lock file is taken first, by design (A.20); nothing else.
        assert_eq!(left, vec!["lock".to_string(), newer.to_string()], "{newer}");
    }
}
