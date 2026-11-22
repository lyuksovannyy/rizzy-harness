//! The node's side of attached files (PLAN.md T94): names, the folder the
//! file goes into, and the staging of what the master pushes.

use super::*;

use std::os::unix::fs::{MetadataExt, symlink};

fn sha(b: &[u8]) -> String {
    hex::encode(Sha256::digest(b))
}

fn sid() -> SessionId {
    "0193f2a0-0000-7000-8000-000000000001".parse().unwrap()
}

/// A chat folder and a staged file in a folder of its own.
struct Rig {
    _tmp: tempfile::TempDir,
    folder: PathBuf,
    staging: PathBuf,
}

impl Rig {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let folder = tmp.path().join("chat").canonicalize_or_make();
        let staging = tmp.path().join("stage");
        std::fs::create_dir_all(&staging).unwrap();
        Rig {
            _tmp: tmp,
            folder,
            staging,
        }
    }

    fn stage(&self, bytes: &[u8]) -> PathBuf {
        let p = self.staging.join(format!("{}.part", sha(bytes)));
        std::fs::write(&p, bytes).unwrap();
        p
    }

    fn save(&self, name: &str, bytes: &[u8]) -> Result<Saved, String> {
        save_file(
            &self.folder,
            &self.stage(bytes),
            name,
            "application/zip",
            bytes.len() as u64,
            &sha(bytes),
        )
    }

    fn listing(&self) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(self.folder.join(DIR))
            .map(|d| {
                d.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    }
}

trait MakeDir {
    fn canonicalize_or_make(self) -> PathBuf;
}

impl MakeDir for PathBuf {
    fn canonicalize_or_make(self) -> PathBuf {
        std::fs::create_dir_all(&self).unwrap();
        self.canonicalize().unwrap()
    }
}

// ---------------------------------------------------------------- names

/// Every way a name can try to leave `attachments/` or to be read as
/// something else by a command: one safe component comes out.
#[test]
fn a_name_becomes_one_safe_component() {
    let cases: &[(&str, &str)] = &[
        ("../../x", "x"),
        ("/etc/passwd", "passwd"),
        ("a/b", "b"),
        ("a\\b.zip", "b.zip"),
        ("C:\\Users\\me\\report.docx", "report.docx"),
        ("...", "file"),
        ("..", "file"),
        (".", "file"),
        ("", "file"),
        ("-rf", "rf"),
        ("--no-preserve-root.zip", "no-preserve-root.zip"),
        (".bashrc", "bashrc"),
        ("name.", "name"),
        ("a\0b.zip", "a b.zip"),
        ("a\nb\r\x1b[31m.zip", "a b [31m.zip"),
        ("a\u{202e}fdp.exe", "afdp.exe"),
        ("a\u{2215}b\u{2044}c.zip", "a_b_c.zip"),
        ("a\u{ff0f}..\u{ff0f}b", "a_.._b"),
        ("what?*.zip", "what__.zip"),
        ("   spaced name.zip  ", "spaced name.zip"),
    ];
    for (given, want) in cases {
        let got = safe_name(given);
        assert_eq!(&got, want, "{given:?}");
        assert!(!got.contains('/') && !got.contains('\\') && !got.contains('\0'));
        assert!(!got.starts_with(['.', '-']), "{got:?}");
    }
}

#[test]
fn a_long_name_is_cut_on_a_character_boundary_and_keeps_its_extension() {
    let long = format!("{}.zip", "a".repeat(300));
    let n = safe_name(&long);
    assert_eq!(n.len(), MAX_NAME_BYTES);
    assert!(n.ends_with("aaa.zip"), "{n}");
    // Multi-byte characters: never cut inside one.
    let wide = format!("{}.tar.gz", "é".repeat(200));
    let n = safe_name(&wide);
    assert!(n.len() <= MAX_NAME_BYTES, "{}", n.len());
    assert!(n.ends_with(".gz"));
    let none = "日".repeat(100);
    let n = safe_name(&none);
    assert!(n.len() <= MAX_NAME_BYTES && n.chars().all(|c| c == '日'));
    // A long "extension" is not one: the whole name is cut.
    let odd = format!("a.{}", "b".repeat(300));
    assert!(safe_name(&odd).len() <= MAX_NAME_BYTES);
}

#[test]
fn numbered_names_keep_the_extension() {
    assert_eq!(numbered("x.zip", 1), "x.zip");
    assert_eq!(numbered("x.zip", 2), "x (2).zip");
    assert_eq!(numbered("x.tar.gz", 3), "x.tar (3).gz");
    assert_eq!(numbered("noext", 2), "noext (2)");
}

#[test]
fn a_type_is_kept_only_when_it_is_a_plain_media_type() {
    assert_eq!(clean_mime("Application/ZIP"), "application/zip");
    assert_eq!(clean_mime("a\"> b"), "application/octet-stream");
    assert_eq!(clean_mime(""), "application/octet-stream");
}

// -------------------------------------------------------------- saving

#[test]
fn a_file_is_saved_under_attachments_with_its_bytes_and_hash() {
    let rig = Rig::new();
    let bytes = b"PK\x03\x04 pretend zip";
    let s = rig.save("../../Reports/q3.zip", bytes).unwrap();
    assert_eq!(s.file.path, "attachments/q3.zip");
    assert_eq!(s.file.name, "q3.zip");
    assert_eq!(s.file.bytes, bytes.len() as u64);
    assert_eq!(s.file.sha256, sha(bytes));
    assert_eq!(s.file.mime, "application/zip");
    assert_eq!(
        std::fs::read(rig.folder.join("attachments/q3.zip")).unwrap(),
        bytes
    );
    // No temporary file is left behind, and the mode is an ordinary one.
    assert_eq!(rig.listing(), vec!["q3.zip".to_string()]);
    let mode = std::fs::metadata(rig.folder.join("attachments/q3.zip"))
        .unwrap()
        .mode()
        & 0o777;
    assert_eq!(mode & 0o111, 0, "{mode:o}");
}

/// The same name twice: the second file gets `(2)`; the first is not
/// touched.
#[test]
fn the_same_name_twice_does_not_overwrite() {
    let rig = Rig::new();
    let a = rig.save("x.zip", b"first").unwrap();
    let b = rig.save("x.zip", b"second").unwrap();
    let c = rig.save("x.zip", b"third").unwrap();
    assert_eq!(a.file.path, "attachments/x.zip");
    assert_eq!(b.file.path, "attachments/x (2).zip");
    assert_eq!(c.file.path, "attachments/x (3).zip");
    assert_eq!(
        std::fs::read(rig.folder.join("attachments/x.zip")).unwrap(),
        b"first"
    );
    assert_eq!(
        std::fs::read(rig.folder.join("attachments/x (2).zip")).unwrap(),
        b"second"
    );
}

/// The agent (or anyone) planted `attachments` as a link out of the
/// folder: nothing is written there, the link stays, and the refusal says
/// what it is.
#[test]
fn attachments_planted_as_a_link_is_not_followed() {
    let rig = Rig::new();
    let outside = rig._tmp.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    symlink(&outside, rig.folder.join(DIR)).unwrap();
    let e = rig.save("x.zip", b"bytes").unwrap_err();
    assert!(
        e.contains("is a link") && e.contains("nothing was saved"),
        "{e}"
    );
    assert_eq!(
        std::fs::read_dir(&outside).unwrap().count(),
        0,
        "nothing outside"
    );
    assert!(
        std::fs::symlink_metadata(rig.folder.join(DIR))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn attachments_planted_as_a_file_is_not_replaced() {
    let rig = Rig::new();
    std::fs::write(rig.folder.join(DIR), b"mine").unwrap();
    let e = rig.save("x.zip", b"bytes").unwrap_err();
    assert!(
        e.contains("is a file") && e.contains("nothing was saved"),
        "{e}"
    );
    assert_eq!(std::fs::read(rig.folder.join(DIR)).unwrap(), b"mine");
}

/// `attachments/x.zip` planted as a link to a file outside: the link and
/// its target are untouched; the new file takes the next name.
#[test]
fn a_planted_link_in_the_name_is_never_written_through_or_replaced() {
    let rig = Rig::new();
    std::fs::create_dir(rig.folder.join(DIR)).unwrap();
    let target = rig._tmp.path().join("secret.txt");
    std::fs::write(&target, b"secret").unwrap();
    symlink(&target, rig.folder.join("attachments/x.zip")).unwrap();
    // A dangling link too.
    symlink(
        rig._tmp.path().join("nowhere"),
        rig.folder.join("attachments/y.zip"),
    )
    .unwrap();
    let s = rig.save("x.zip", b"new bytes").unwrap();
    let t = rig.save("y.zip", b"other bytes").unwrap();
    assert_eq!(s.file.path, "attachments/x (2).zip");
    assert_eq!(t.file.path, "attachments/y (2).zip");
    assert_eq!(std::fs::read(&target).unwrap(), b"secret");
    assert!(!rig._tmp.path().join("nowhere").exists());
    assert!(
        std::fs::symlink_metadata(rig.folder.join("attachments/x.zip"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

/// A folder named like the file also keeps its name.
#[test]
fn a_folder_in_the_way_keeps_its_name() {
    let rig = Rig::new();
    std::fs::create_dir_all(rig.folder.join("attachments/x.zip")).unwrap();
    let s = rig.save("x.zip", b"bytes").unwrap();
    assert_eq!(s.file.path, "attachments/x (2).zip");
}

#[test]
fn a_wrong_hash_or_size_saves_nothing() {
    let rig = Rig::new();
    let bytes = b"real bytes";
    let staged = rig.stage(bytes);
    let e = save_file(
        &rig.folder,
        &staged,
        "x.zip",
        "application/zip",
        bytes.len() as u64,
        &sha(b"other"),
    )
    .unwrap_err();
    assert!(
        e.contains("sha256") && e.contains("nothing was saved"),
        "{e}"
    );
    let e = save_file(
        &rig.folder,
        &staged,
        "x.zip",
        "application/zip",
        bytes.len() as u64 + 1,
        &sha(bytes),
    )
    .unwrap_err();
    assert!(e.contains("nothing was saved"), "{e}");
    // The folder holds no final name and no temporary one.
    assert_eq!(rig.listing(), Vec::<String>::new());
}

/// A full disk: the copy fails, the refusal says so and nothing stays.
#[test]
fn a_full_disk_saves_nothing_and_says_so() {
    let rig = Rig::new();
    let bytes = b"x".repeat(1000);
    let e = save_with(
        &rig.folder,
        &rig.stage(&bytes),
        "big.zip",
        "application/zip",
        1000,
        &sha(&bytes),
        |_, _| Err(std::io::Error::from_raw_os_error(28)),
    )
    .unwrap_err();
    assert!(e.contains("No space left") && e.contains("disk"), "{e}");
    assert_eq!(rig.listing(), Vec::<String>::new());
}

/// The copy itself: a source longer than it said stops at the bound, and
/// a writer that fails is an error.
#[test]
fn the_copy_is_bounded_and_reports_a_failing_writer() {
    struct Full;
    impl Write for Full {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::from_raw_os_error(28))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let data = vec![7u8; 200_000];
    let mut out = Vec::new();
    let (n, h) = copy_hashing(&mut &data[..], &mut out, 200_000).unwrap();
    assert_eq!((n, h), (200_000, sha(&data)));
    assert!(copy_hashing(&mut &data[..], &mut Vec::new(), 100).is_err());
    assert!(copy_hashing(&mut &data[..], &mut Full, 200_000).is_err());
}

/// A rollback removes the very file that was saved, nothing else: a file
/// the agent put in its place (another inode) stays.
#[test]
fn a_rollback_removes_only_the_saved_file() {
    let rig = Rig::new();
    let s = rig.save("x.zip", b"saved").unwrap();
    rollback(&rig.folder, &s);
    assert_eq!(rig.listing(), Vec::<String>::new());
    let s = rig.save("y.zip", b"saved").unwrap();
    std::fs::remove_file(rig.folder.join("attachments/y.zip")).unwrap();
    std::fs::write(rig.folder.join("attachments/y.zip"), b"the agent's own").unwrap();
    rollback(&rig.folder, &s);
    assert_eq!(
        std::fs::read(rig.folder.join("attachments/y.zip")).unwrap(),
        b"the agent's own"
    );
}

// -------------------------------------------------------------- staging

fn staging(tmp: &Path) -> Staging {
    Staging::new(Some(tmp.join("uploads-stage")))
}

const MAX: u64 = 50 * 1024 * 1024;

#[test]
fn chunks_build_a_staged_file_that_is_ready_only_when_whole_and_right() {
    let tmp = tempfile::tempdir().unwrap();
    let st = staging(tmp.path());
    let bytes: Vec<u8> = (0..700_000u32).map(|i| (i % 251) as u8).collect();
    let id = "a".repeat(32);
    let r = UploadRef {
        upload_id: id.clone(),
        name: "d.bin".into(),
        mime: "application/octet-stream".into(),
        size: bytes.len() as u64,
        sha256: sha(&bytes),
    };
    let chunks: Vec<&[u8]> = bytes.chunks(MAX_CHUNK as usize).collect();
    for (i, c) in chunks.iter().enumerate() {
        // Not ready before the last chunk.
        assert!(st.ready(sid(), &r).is_err(), "ready early at {i}");
        st.chunk(
            sid(),
            &id,
            (i * MAX_CHUNK as usize) as u64,
            r.size,
            &r.sha256,
            c,
            MAX,
        )
        .unwrap();
    }
    let path = st.ready(sid(), &r).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    // Another chat cannot take it by its id.
    let other: SessionId = "0193f2a0-0000-7000-8000-000000000002".parse().unwrap();
    assert!(st.ready(other, &r).is_err());
    // Nor with another size or hash named.
    let mut lie = r.clone();
    lie.sha256 = sha(b"other");
    assert!(st.ready(sid(), &lie).is_err());
    st.discard(sid(), &id);
    assert!(!path.exists());
    assert_eq!(st.held(), 0);
}

#[test]
fn a_wrong_sha256_at_the_last_chunk_deletes_the_staged_file() {
    let tmp = tempfile::tempdir().unwrap();
    let st = staging(tmp.path());
    let id = "b".repeat(32);
    let e = st
        .chunk(sid(), &id, 0, 5, &sha(b"other"), b"hello", MAX)
        .unwrap_err();
    assert!(e.contains("sha256"), "{e}");
    assert_eq!(st.held(), 0);
    assert!(
        !tmp.path()
            .join("uploads-stage")
            .join(sid().to_string())
            .join(format!("{id}.part"))
            .exists()
    );
}

#[test]
fn bad_chunks_are_refused_and_delete_what_was_staged() {
    let tmp = tempfile::tempdir().unwrap();
    let st = staging(tmp.path());
    let id = "c".repeat(32);
    let data = b"0123456789";
    let h = sha(data);
    // Over the node's limit.
    let e = st.chunk(sid(), &id, 0, 100, &h, b"x", 50).unwrap_err();
    assert!(e.contains("up to"), "{e}");
    // Not a plain id, not a hash.
    assert!(st.chunk(sid(), "../x", 0, 10, &h, b"x", MAX).is_err());
    assert!(st.chunk(sid(), &id, 0, 10, "zz", b"x", MAX).is_err());
    // Empty, and over 256 KiB.
    assert!(st.chunk(sid(), &id, 0, 10, &h, b"", MAX).is_err());
    let big = vec![0u8; MAX_CHUNK as usize + 1];
    assert!(st.chunk(sid(), &id, 0, 1 << 20, &h, &big, MAX).is_err());
    // A chunk past the declared size.
    st.chunk(sid(), &id, 0, 10, &h, b"01234", MAX).unwrap();
    assert!(st.chunk(sid(), &id, 5, 10, &h, b"56789012", MAX).is_err());
    assert_eq!(st.held(), 0);
    // A gap: offset 5 without the first five.
    assert!(st.chunk(sid(), &id, 5, 10, &h, b"56789", MAX).is_err());
    // A restart at 0 begins again.
    st.chunk(sid(), &id, 0, 10, &h, b"01234", MAX).unwrap();
    st.chunk(sid(), &id, 0, 10, &h, b"01234", MAX).unwrap();
    st.chunk(sid(), &id, 5, 10, &h, b"56789", MAX).unwrap();
    let r = UploadRef {
        upload_id: id.clone(),
        name: "n".into(),
        mime: "x/y".into(),
        size: 10,
        sha256: h,
    };
    assert!(st.ready(sid(), &r).is_ok());
}

/// Staging is bounded by count, and a chat's deletion takes its files.
#[test]
fn staging_is_bounded_and_a_deleted_chat_takes_its_files() {
    let tmp = tempfile::tempdir().unwrap();
    let st = staging(tmp.path());
    let h = sha(b"abcd");
    for i in 0..MAX_STAGED {
        let id = format!("{i:032x}");
        st.chunk(sid(), &id, 0, 8, &h, b"abcd", MAX).unwrap();
    }
    let e = st
        .chunk(sid(), &"f".repeat(32), 0, 8, &h, b"abcd", MAX)
        .unwrap_err();
    assert!(e.contains("already holds"), "{e}");
    st.drop_session(sid());
    assert_eq!(st.held(), 0);
    assert!(
        !tmp.path()
            .join("uploads-stage")
            .join(sid().to_string())
            .exists()
    );
}

/// A new run starts with an empty staging folder; a node with no folder
/// takes nothing.
#[test]
fn a_restart_wipes_the_staging_folder_and_none_means_off() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("uploads-stage");
    std::fs::create_dir_all(dir.join("old")).unwrap();
    std::fs::write(dir.join("old/x.part"), b"half").unwrap();
    let st = Staging::new(Some(dir.clone()));
    assert!(!dir.join("old").exists());
    assert!(st.enabled());
    let off = Staging::new(None);
    assert!(!off.enabled());
    assert!(
        off.chunk(sid(), &"a".repeat(32), 0, 4, &sha(b"abcd"), b"abcd", MAX)
            .is_err()
    );
}
