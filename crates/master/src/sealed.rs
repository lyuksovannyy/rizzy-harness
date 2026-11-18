//! Sealed storage for secrets kept in `master.db` (ADOPTION_PLAN C.5 and
//! X.27, PLAN.md T83, docs/nodes-credentials-environments.md §4.1).
//!
//! A secret is sealed with AES-256-GCM (aws-lc-rs) under one key-encryption
//! key, `<data>/keys/secrets.key`: 32 random bytes, mode 0600 in a 0700
//! folder, made on the first seal and never rewritten. A sealed value is
//!
//! ```text
//! version (1 byte, 0x01) | nonce (12 random bytes) | ciphertext | tag (16 bytes)
//! ```
//!
//! and its associated data binds it to its row: a domain tag, the version,
//! then each of the table's name, the owner's id and the row's own key
//! fields as a 4-byte big-endian length and the bytes ([`Binding`]). A value
//! copied to another row, another owner or another table, a flipped bit, a
//! cut blob or an unknown version does not open, with an error that names
//! no secret.
//!
//! What this does not do (stated in README "Connections"): no key rotation
//! yet; losing `secrets.key` loses every sealed secret (a backup must hold
//! it with `master.db`, and a copied `master.db` alone opens nothing). The
//! key is never made again while sealed values exist: that would orphan
//! them silently, so a missing or damaged file is an error that says what
//! to do. `Secret<…>` holds the plaintext; it is not zeroed on drop
//! (neither is any other `Secret` in this code base).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use aws_lc_rs::aead::{AES_256_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use harness_core::Secret;

/// The only format so far.
pub const VERSION: u8 = 1;
const KEY_LEN: usize = 32;
const TAG_LEN: usize = 16;
/// The shortest sealed value (an empty plaintext).
pub const MIN_LEN: usize = 1 + NONCE_LEN + TAG_LEN;
const DOMAIN: &[u8] = b"rizzy-harness sealed\0";

/// Where a sealed value belongs: its table, its owner and the row's own key
/// fields, in the order the table names them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding<'a> {
    pub table: &'a str,
    pub owner: &'a str,
    pub fields: &'a [&'a str],
}

impl Binding<'_> {
    /// The associated data: unambiguous, so no two bindings give the same
    /// bytes (each part is length-prefixed).
    fn aad(&self, version: u8) -> Vec<u8> {
        let mut out = Vec::with_capacity(64);
        out.extend_from_slice(DOMAIN);
        out.push(version);
        for part in [self.table, self.owner]
            .into_iter()
            .chain(self.fields.iter().copied())
        {
            out.extend_from_slice(&(part.len() as u32).to_be_bytes());
            out.extend_from_slice(part.as_bytes());
        }
        out
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SealError {
    #[error(
        "{path} is missing, but master.db holds {sealed} sealed secret(s) made with it. Restore the \
         file from the backup that has this master.db; the server does not make a new one, which \
         would leave them unreadable for good. To give them up instead, people disconnect the \
         connections (no key is needed for that) and the next one makes a new key"
    )]
    KeyMissing { path: PathBuf, sealed: u64 },
    #[error(
        "{path} cannot be used: {why}. Sealed secrets are neither made nor opened until it is \
         fixed; restore it from a backup (a regular file of this server's user, mode 0600, {KEY_LEN} bytes)"
    )]
    KeyBad { path: PathBuf, why: String },
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("a sealed value has format version {0}, which this build does not know")]
    UnknownVersion(u8),
    #[error(
        "a sealed value did not open: it is damaged, was moved from another row, or was sealed \
         under another secrets.key"
    )]
    Damaged,
}

/// The key-encryption key at `<data>/keys/secrets.key`, read on first use.
#[derive(Clone)]
pub struct Sealer {
    path: PathBuf,
    key: Arc<Mutex<Option<Arc<LessSafeKey>>>>,
}

impl std::fmt::Debug for Sealer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sealer").field("path", &self.path).finish()
    }
}

impl Sealer {
    /// The sealer of the data directory `data_dir`.
    pub fn new(data_dir: &Path) -> Self {
        Self::at(data_dir.join("keys").join("secrets.key"))
    }

    pub fn at(path: PathBuf) -> Self {
        Self {
            path,
            key: Arc::default(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Check the file as it is now (startup): fine when it is absent and
    /// nothing is sealed, or present and usable.
    pub fn check(&self, sealed: u64) -> Result<(), SealError> {
        match load(&self.path)? {
            Some(_) => Ok(()),
            None if sealed > 0 => Err(SealError::KeyMissing {
                path: self.path.clone(),
                sealed,
            }),
            None => Ok(()),
        }
    }

    fn cached(&self) -> Option<Arc<LessSafeKey>> {
        self.key.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    fn remember(&self, k: LessSafeKey) -> Arc<LessSafeKey> {
        let k = Arc::new(k);
        *self.key.lock().unwrap_or_else(|p| p.into_inner()) = Some(k.clone());
        k
    }

    /// The key for opening: the file must exist.
    fn key_to_open(&self) -> Result<Arc<LessSafeKey>, SealError> {
        if let Some(k) = self.cached() {
            return Ok(k);
        }
        match load(&self.path)? {
            Some(k) => Ok(self.remember(k)),
            None => Err(SealError::KeyMissing {
                path: self.path.clone(),
                sealed: 1,
            }),
        }
    }

    /// The key for sealing: made when absent, but only while nothing is
    /// sealed (`sealed` is how many values the database holds).
    fn key_to_seal(&self, sealed: u64) -> Result<Arc<LessSafeKey>, SealError> {
        if let Some(k) = self.cached() {
            return Ok(k);
        }
        if let Some(k) = load(&self.path)? {
            return Ok(self.remember(k));
        }
        if sealed > 0 {
            return Err(SealError::KeyMissing {
                path: self.path.clone(),
                sealed,
            });
        }
        create(&self.path)?;
        match load(&self.path)? {
            Some(k) => Ok(self.remember(k)),
            None => Err(SealError::KeyBad {
                path: self.path.clone(),
                why: "it was made but cannot be read back".into(),
            }),
        }
    }

    /// Seal `plain` for the row `at`. `sealed` is how many sealed values
    /// the database already holds (a missing key is made only at 0).
    pub fn seal(
        &self,
        at: &Binding<'_>,
        plain: &Secret<String>,
        sealed: u64,
    ) -> Result<Vec<u8>, SealError> {
        let key = self.key_to_seal(sealed)?;
        let mut nonce = [0u8; NONCE_LEN];
        getrandom::fill(&mut nonce).map_err(|e| SealError::Io {
            path: self.path.clone(),
            source: std::io::Error::other(e.to_string()),
        })?;
        let mut buf = plain.expose().as_bytes().to_vec();
        key.seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(at.aad(VERSION)),
            &mut buf,
        )
        .map_err(|_| SealError::Damaged)?;
        let mut out = Vec::with_capacity(1 + NONCE_LEN + buf.len());
        out.push(VERSION);
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&buf);
        Ok(out)
    }

    /// Open a value sealed for the row `at`.
    pub fn open(&self, at: &Binding<'_>, sealed: &[u8]) -> Result<Secret<String>, SealError> {
        let Some(&version) = sealed.first() else {
            return Err(SealError::Damaged);
        };
        if version != VERSION {
            return Err(SealError::UnknownVersion(version));
        }
        if sealed.len() < MIN_LEN {
            return Err(SealError::Damaged);
        }
        let key = self.key_to_open()?;
        let nonce: [u8; NONCE_LEN] = sealed[1..1 + NONCE_LEN]
            .try_into()
            .map_err(|_| SealError::Damaged)?;
        let mut buf = sealed[1 + NONCE_LEN..].to_vec();
        let plain = key
            .open_in_place(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(at.aad(version)),
                &mut buf,
            )
            .map_err(|_| SealError::Damaged)?;
        let text = String::from_utf8(plain.to_vec()).map_err(|_| SealError::Damaged)?;
        Ok(Secret::new(text))
    }
}

fn bad(path: &Path, why: impl Into<String>) -> SealError {
    SealError::KeyBad {
        path: path.into(),
        why: why.into(),
    }
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> SealError + '_ {
    move |source| SealError::Io {
        path: path.into(),
        source,
    }
}

/// The key in `path`, `None` when there is no file. Anything else that is
/// not a regular private file of this user, of exactly 32 bytes, is an
/// error: never replaced, never read past.
#[cfg(unix)]
fn load(path: &Path) -> Result<Option<LessSafeKey>, SealError> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let md = match std::fs::symlink_metadata(path) {
        Ok(md) => md,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io(path)(e)),
    };
    if md.file_type().is_symlink() {
        return Err(bad(path, "it is a symbolic link"));
    }
    if md.is_dir() {
        return Err(bad(path, "it is a directory"));
    }
    // Opened without following a link, then judged by what was opened.
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::fcntl::OFlag::O_NOFOLLOW.bits())
        .open(path)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::IsADirectory {
                bad(path, "it is a directory")
            } else {
                io(path)(e)
            }
        })?;
    let md = f.metadata().map_err(io(path))?;
    if !md.file_type().is_file() {
        return Err(bad(path, "it is not a regular file"));
    }
    let me = nix::unistd::geteuid().as_raw();
    if md.uid() != me {
        return Err(bad(
            path,
            format!(
                "it is owned by uid {}, not this server's user (uid {me})",
                md.uid()
            ),
        ));
    }
    let mode = md.mode() & 0o777;
    if mode & 0o077 != 0 {
        return Err(bad(
            path,
            format!("it is open to group or others (mode {mode:03o}; it must be 600)"),
        ));
    }
    if md.len() != KEY_LEN as u64 {
        return Err(bad(
            path,
            format!("it is {} bytes long, not {KEY_LEN}", md.len()),
        ));
    }
    let mut raw = [0u8; KEY_LEN];
    f.read_exact(&mut raw).map_err(io(path))?;
    let mut rest = [0u8; 1];
    if f.read(&mut rest).map_err(io(path))? != 0 {
        return Err(bad(path, format!("it is longer than {KEY_LEN} bytes")));
    }
    let k = UnboundKey::new(&AES_256_GCM, &raw).map_err(|_| bad(path, "it is not a key"))?;
    raw.fill(0);
    Ok(Some(LessSafeKey::new(k)))
}

#[cfg(not(unix))]
fn load(path: &Path) -> Result<Option<LessSafeKey>, SealError> {
    Err(bad(path, "sealed storage needs a Unix system"))
}

/// The folder `keys/`: made 0700, refused when it is a link or another
/// user's, narrowed to 0700 when it is wider (the TLS keys made it with the
/// process's umask before this existed).
#[cfg(unix)]
fn key_dir(dir: &Path) -> Result<(), SealError> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    match std::fs::symlink_metadata(dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let mut b = std::fs::DirBuilder::new();
            b.recursive(true).mode(0o700);
            b.create(dir).map_err(io(dir))?;
        }
        Err(e) => return Err(io(dir)(e)),
        Ok(md) => {
            if !md.file_type().is_dir() {
                return Err(bad(dir, "it is not a folder (or it is a symbolic link)"));
            }
            let me = nix::unistd::geteuid().as_raw();
            if md.uid() != me {
                return Err(bad(
                    dir,
                    format!("it is owned by uid {}, not uid {me}", md.uid()),
                ));
            }
            if md.mode() & 0o077 != 0 {
                std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
                    .map_err(io(dir))?;
            }
        }
    }
    Ok(())
}

/// Make the key file: 32 random bytes into a new private file, then linked
/// into place, which fails when a file is already there (another start won
/// the race): that one stays, this one goes.
#[cfg(unix)]
fn create(path: &Path) -> Result<(), SealError> {
    use std::os::unix::fs::OpenOptionsExt;
    let dir = path.parent().ok_or_else(|| bad(path, "it has no folder"))?;
    key_dir(dir)?;
    let mut raw = [0u8; KEY_LEN];
    getrandom::fill(&mut raw).map_err(|e| io(path)(std::io::Error::other(e.to_string())))?;
    let tmp = dir.join(format!(".secrets.key.{}.new", crate::auth::random_hex(8)));
    let written = (|| {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(nix::fcntl::OFlag::O_NOFOLLOW.bits())
            .open(&tmp)?;
        f.write_all(&raw)?;
        f.sync_all()
    })();
    raw.fill(0);
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(io(&tmp)(e));
    }
    let linked = std::fs::hard_link(&tmp, path);
    let _ = std::fs::remove_file(&tmp);
    match linked {
        Ok(()) => {
            if let Ok(d) = std::fs::File::open(dir) {
                let _ = d.sync_all();
            }
            tracing::info!(path = %path.display(), "made the key for sealed secrets");
            Ok(())
        }
        // Someone else's file: it is loaded and judged like any other.
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(io(path)(e)),
    }
}

#[cfg(not(unix))]
fn create(path: &Path) -> Result<(), SealError> {
    Err(bad(path, "sealed storage needs a Unix system"))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    const TOKEN: &str = "canary-ghp_0123456789abcdefTOKEN";

    fn at<'a>(user: &'a str, host: &'a str, account: &'a str) -> [&'a str; 3] {
        [user, host, account]
    }

    fn conn<'a>(f: &'a [&'a str; 3]) -> Binding<'a> {
        Binding {
            table: "user_connections",
            owner: f[0],
            fields: &f[1..],
        }
    }

    fn secret() -> Secret<String> {
        Secret::new(TOKEN.into())
    }

    fn mode(p: &Path) -> u32 {
        std::fs::metadata(p).unwrap().permissions().mode() & 0o777
    }

    fn no_secret(e: &SealError) {
        let text = format!("{e} {e:?}");
        assert!(!text.contains(TOKEN), "{text}");
    }

    #[test]
    fn seals_and_opens_and_makes_a_private_key_once() {
        let d = tempfile::tempdir().unwrap();
        let s = Sealer::new(d.path());
        let f = at("u1", "github.com", "42");
        let blob = s.seal(&conn(&f), &secret(), 0).unwrap();
        assert_eq!(blob[0], VERSION);
        assert_eq!(blob.len(), MIN_LEN + TOKEN.len());
        assert!(
            !blob
                .windows(8)
                .any(|w| TOKEN.as_bytes().windows(8).any(|t| t == w))
        );
        assert_eq!(s.open(&conn(&f), &blob).unwrap().expose(), TOKEN);
        // A fresh nonce each time.
        assert_ne!(blob, s.seal(&conn(&f), &secret(), 1).unwrap());
        let key = d.path().join("keys/secrets.key");
        assert_eq!(mode(&key), 0o600);
        assert_eq!(mode(&d.path().join("keys")), 0o700);
        let first = std::fs::read(&key).unwrap();
        // Another sealer (a restart) uses the same file and never rewrites it.
        let again = Sealer::new(d.path());
        assert_eq!(again.open(&conn(&f), &blob).unwrap().expose(), TOKEN);
        again.seal(&conn(&f), &secret(), 0).unwrap();
        assert_eq!(std::fs::read(&key).unwrap(), first);
        // No temporary file is left beside it.
        let names: Vec<_> = std::fs::read_dir(d.path().join("keys"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("secrets.key")]);
    }

    #[test]
    fn an_existing_key_file_is_never_overwritten() {
        let d = tempfile::tempdir().unwrap();
        let key = d.path().join("keys/secrets.key");
        std::fs::create_dir_all(key.parent().unwrap()).unwrap();
        std::fs::write(&key, [7u8; KEY_LEN]).unwrap();
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
        // The race: create() finds a file already there and keeps it.
        create(&key).unwrap();
        assert_eq!(std::fs::read(&key).unwrap(), vec![7u8; KEY_LEN]);
        let s = Sealer::new(d.path());
        let f = at("u", "h", "a");
        let blob = s.seal(&conn(&f), &secret(), 0).unwrap();
        assert_eq!(std::fs::read(&key).unwrap(), vec![7u8; KEY_LEN]);
        assert_eq!(s.open(&conn(&f), &blob).unwrap().expose(), TOKEN);
    }

    #[test]
    fn a_value_moved_to_another_row_owner_or_table_does_not_open() {
        let d = tempfile::tempdir().unwrap();
        let s = Sealer::new(d.path());
        let f = at("alice", "github.com", "42");
        let blob = s.seal(&conn(&f), &secret(), 0).unwrap();
        for other in [
            at("bob", "github.com", "42"),
            at("alice", "gitlab.com", "42"),
            at("alice", "github.com", "43"),
            // Length prefixes: moving bytes between fields is another row.
            at("alic", "egithub.com", "42"),
            at("alice", "github.com4", "2"),
        ] {
            let e = s.open(&conn(&other), &blob).unwrap_err();
            assert!(matches!(e, SealError::Damaged), "{other:?}: {e}");
            no_secret(&e);
        }
        let ws = Binding {
            table: "workspace_secrets",
            owner: "alice",
            fields: &["github.com", "42"],
        };
        assert!(matches!(s.open(&ws, &blob), Err(SealError::Damaged)));
        assert_eq!(s.open(&conn(&f), &blob).unwrap().expose(), TOKEN);
    }

    #[test]
    fn a_changed_cut_or_unknown_blob_does_not_open() {
        let d = tempfile::tempdir().unwrap();
        let s = Sealer::new(d.path());
        let f = at("u", "github.com", "1");
        let blob = s.seal(&conn(&f), &secret(), 0).unwrap();
        for i in 1..blob.len() {
            for bit in [0x01u8, 0x80] {
                let mut b = blob.clone();
                b[i] ^= bit;
                let e = s.open(&conn(&f), &b).unwrap_err();
                assert!(matches!(e, SealError::Damaged), "byte {i}: {e}");
                no_secret(&e);
            }
        }
        for n in 0..blob.len() {
            let e = s.open(&conn(&f), &blob[..n]).unwrap_err();
            assert!(
                matches!(e, SealError::Damaged | SealError::UnknownVersion(_)),
                "cut at {n}: {e}"
            );
        }
        let mut longer = blob.clone();
        longer.push(0);
        assert!(matches!(
            s.open(&conn(&f), &longer),
            Err(SealError::Damaged)
        ));
        for v in [0u8, 2, 0xff] {
            let mut b = blob.clone();
            b[0] = v;
            let e = s.open(&conn(&f), &b).unwrap_err();
            assert!(matches!(e, SealError::UnknownVersion(x) if x == v), "{e}");
            assert!(e.to_string().contains(&format!("version {v}")), "{e}");
        }
    }

    /// A copied master.db without secrets.key: nothing opens, under no key
    /// and under a new one.
    #[test]
    fn a_database_without_its_key_opens_nothing() {
        let d = tempfile::tempdir().unwrap();
        let f = at("u", "github.com", "1");
        let blob = Sealer::new(d.path()).seal(&conn(&f), &secret(), 0).unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let s = Sealer::new(elsewhere.path());
        let e = s.open(&conn(&f), &blob).unwrap_err();
        assert!(matches!(e, SealError::KeyMissing { .. }), "{e}");
        no_secret(&e);
        // Opening never makes a key.
        assert!(!elsewhere.path().join("keys/secrets.key").exists());
        // A new key (sealing with nothing stored there) opens nothing old.
        s.seal(&conn(&f), &secret(), 0).unwrap();
        assert!(matches!(s.open(&conn(&f), &blob), Err(SealError::Damaged)));
    }

    #[test]
    fn a_missing_key_is_not_made_again_while_secrets_exist() {
        let d = tempfile::tempdir().unwrap();
        let s = Sealer::new(d.path());
        let e = s.seal(&conn(&at("u", "h", "a")), &secret(), 3).unwrap_err();
        let text = e.to_string();
        assert!(
            matches!(e, SealError::KeyMissing { sealed: 3, .. }),
            "{text}"
        );
        assert!(
            text.contains("secrets.key") && text.contains("Restore"),
            "{text}"
        );
        assert!(!d.path().join("keys/secrets.key").exists());
        assert!(s.check(3).is_err());
        assert!(s.check(0).is_ok());
    }

    #[test]
    fn a_key_file_that_is_not_private_and_exact_is_refused_and_kept() {
        let d = tempfile::tempdir().unwrap();
        let s = Sealer::new(d.path());
        let f = at("u", "h", "a");
        let blob = s.seal(&conn(&f), &secret(), 0).unwrap();
        let key = d.path().join("keys/secrets.key");
        let good = std::fs::read(&key).unwrap();
        let expect_bad = |label: &str, needle: &str| {
            // A fresh sealer: as after a restart (no cached key).
            let s = Sealer::new(d.path());
            for e in [
                s.open(&conn(&f), &blob).unwrap_err(),
                s.seal(&conn(&f), &secret(), 0).unwrap_err(),
                s.check(1).unwrap_err(),
            ] {
                let t = e.to_string();
                assert!(matches!(e, SealError::KeyBad { .. }), "{label}: {t}");
                assert!(t.contains(needle), "{label}: {t}");
                no_secret(&e);
            }
        };
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o644)).unwrap();
        expect_bad("world-readable", "mode 644");
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o640)).unwrap();
        expect_bad("group-readable", "mode 640");
        assert_eq!(std::fs::read(&key).unwrap(), good, "not rewritten");
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();

        std::fs::write(&key, &good[..31]).unwrap();
        expect_bad("short", "31 bytes");
        assert_eq!(std::fs::read(&key).unwrap().len(), 31, "not rewritten");
        std::fs::write(&key, [good.as_slice(), b"\n"].concat()).unwrap();
        expect_bad("long", "33 bytes");

        std::fs::remove_file(&key).unwrap();
        let target = d.path().join("elsewhere.key");
        std::fs::write(&target, &good).unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::os::unix::fs::symlink(&target, &key).unwrap();
        expect_bad("link", "symbolic link");
        std::fs::remove_file(&key).unwrap();

        std::fs::create_dir(&key).unwrap();
        expect_bad("directory", "directory");
        assert!(key.is_dir(), "not replaced");
        std::fs::remove_dir(&key).unwrap();

        std::fs::write(&key, &good).unwrap();
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(
            Sealer::new(d.path())
                .open(&conn(&f), &blob)
                .unwrap()
                .expose(),
            TOKEN
        );
    }

    #[test]
    fn a_wide_key_folder_is_narrowed_and_a_linked_one_refused() {
        let d = tempfile::tempdir().unwrap();
        let keys = d.path().join("keys");
        std::fs::create_dir(&keys).unwrap();
        std::fs::set_permissions(&keys, std::fs::Permissions::from_mode(0o755)).unwrap();
        Sealer::new(d.path())
            .seal(&conn(&at("u", "h", "a")), &secret(), 0)
            .unwrap();
        assert_eq!(mode(&keys), 0o700);

        let e = tempfile::tempdir().unwrap();
        let real = e.path().join("real");
        std::fs::create_dir(&real).unwrap();
        std::os::unix::fs::symlink(&real, e.path().join("keys")).unwrap();
        let err = Sealer::new(e.path())
            .seal(&conn(&at("u", "h", "a")), &secret(), 0)
            .unwrap_err();
        assert!(matches!(err, SealError::KeyBad { .. }), "{err}");
        assert!(!real.join("secrets.key").exists());
    }

    #[test]
    fn debug_shows_no_key() {
        let d = tempfile::tempdir().unwrap();
        let s = Sealer::new(d.path());
        s.seal(&conn(&at("u", "h", "a")), &secret(), 0).unwrap();
        let text = format!("{s:?}");
        assert!(
            text.contains("secrets.key") && !text.contains("key: "),
            "{text}"
        );
    }
}
