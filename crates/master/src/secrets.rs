//! `<data>/secrets.toml`: API keys entered in the UI (PLAN.md §0 onboarding).
//!
//! * Mode 0600, written atomically (temp file with 0600, then rename).
//! * Values are `Secret<String>`: redacted in `Debug`, never returned to the
//!   browser, never logged, never written to the event log.
//! * Config refers to keys by name (`api_key_secret = "provider-<id>"`);
//!   headless installs can keep using `api_key_env` instead.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use harness_core::Secret;

#[derive(Debug, thiserror::Error)]
pub enum SecretsError {
    #[error("{0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("{0} is not valid TOML: {1}")]
    Parse(PathBuf, toml::de::Error),
    #[error("{0} holds secrets and is open to group or others (mode {1:03o}); run: chmod 600 {0}")]
    Permissions(PathBuf, u32),
    #[error(
        "{0} is owned by another user (uid {1}, this process runs as uid {2}); the harness only uses files and folders of its own user"
    )]
    Owner(PathBuf, u32, u32),
    #[error("secret names must be 1-64 characters: a-z, 0-9, _ -")]
    BadName,
}

/// Refuse `md` (of `path`) when another user owns it (Gate 3 review N-3).
#[cfg(unix)]
fn check_owner(path: &Path, md: &std::fs::Metadata) -> Result<(), SecretsError> {
    use std::os::unix::fs::MetadataExt;
    let me = nix::unistd::geteuid().as_raw();
    if md.uid() != me {
        return Err(SecretsError::Owner(path.into(), md.uid(), me));
    }
    Ok(())
}

/// The data directory: refused when another user owns it (Gate 3 review
/// N-3), then made 0700 (a chmod that fails is an error, not ignored).
pub fn check_data_dir(dir: &Path) -> Result<(), SecretsError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let md = std::fs::metadata(dir).map_err(|e| SecretsError::Io(dir.into(), e))?;
        check_owner(dir, &md)?;
        if md.permissions().mode() & 0o777 != 0o700 {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| SecretsError::Io(dir.into(), e))?;
        }
    }
    #[cfg(not(unix))]
    let _ = dir;
    Ok(())
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct File {
    #[serde(default)]
    keys: BTreeMap<String, String>,
}

fn valid_name(n: &str) -> bool {
    (1..=64).contains(&n.len())
        && n.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// Refuse a file that holds secrets when another user owns it (Gate 3
/// review N-3) or its group or others have any access (`mode & 0o077`),
/// checked before it is read. A symlink is judged
/// by its target (`metadata` follows it, as `chmod` does). A missing file
/// passes: there is nothing to read. Unix only; elsewhere there are no
/// mode bits to check and this always passes.
pub fn check_private(path: &Path) -> Result<(), SecretsError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(path) {
            Ok(md) => {
                check_owner(path, &md)?;
                let mode = md.permissions().mode() & 0o777;
                if mode & 0o077 != 0 {
                    return Err(SecretsError::Permissions(path.into(), mode));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(SecretsError::Io(path.into(), e)),
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// The private keys under `<data>/keys` (TLS keys `*-key.der`, the node
/// key `node.key`), each through [`check_private`]. Certificates and the
/// name list there are public.
pub fn check_keys(data_dir: &Path) -> Result<(), SecretsError> {
    let dir = data_dir.join("keys");
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(SecretsError::Io(dir, e)),
    };
    for entry in entries {
        let entry = entry.map_err(|e| SecretsError::Io(dir.clone(), e))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.ends_with("-key.der") || name.ends_with(".key") {
            check_private(&entry.path())?;
        }
    }
    Ok(())
}

fn read(path: &Path) -> Result<File, SecretsError> {
    check_private(path)?;
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(File::default()),
        Err(e) => return Err(SecretsError::Io(path.into(), e)),
    };
    toml::from_str(&text).map_err(|e| SecretsError::Parse(path.into(), e))
}

/// Look up one secret by name.
pub fn get(path: &Path, name: &str) -> Result<Option<Secret<String>>, SecretsError> {
    Ok(read(path)?.keys.get(name).cloned().map(Secret::new))
}

/// Names of stored secrets (never their values).
pub fn names(path: &Path) -> Result<Vec<String>, SecretsError> {
    Ok(read(path)?.keys.into_keys().collect())
}

/// Insert or replace a secret; `None` removes it.
pub fn set(path: &Path, name: &str, value: Option<&Secret<String>>) -> Result<(), SecretsError> {
    if !valid_name(name) {
        return Err(SecretsError::BadName);
    }
    let mut f = read(path)?;
    match value {
        Some(v) => {
            f.keys.insert(name.to_string(), v.expose().clone());
        }
        None => {
            f.keys.remove(name);
        }
    }
    let text = toml::to_string(&f).expect("secrets serialize");
    let tmp = path.with_extension("toml.tmp");
    let io = |e| SecretsError::Io(tmp.clone(), e);
    // A temp file left by a crash keeps its old mode when reopened, so it
    // goes first and the new one is created 0600 (never chmod-ed later).
    let _ = std::fs::remove_file(&tmp);
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(&tmp).map_err(io)?;
    file.write_all(text.as_bytes()).map_err(io)?;
    file.sync_all().map_err(io)?;
    std::fs::rename(&tmp, path).map_err(|e| SecretsError::Io(path.into(), e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_remove_and_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("secrets.toml");
        assert!(get(&p, "provider-openai").unwrap().is_none());
        set(&p, "provider-openai", Some(&Secret::new("sk-abc".into()))).unwrap();
        set(
            &p,
            "provider-anthropic",
            Some(&Secret::new("sk-ant".into())),
        )
        .unwrap();
        assert_eq!(
            get(&p, "provider-openai").unwrap().unwrap().expose(),
            "sk-abc"
        );
        assert_eq!(
            names(&p).unwrap(),
            vec!["provider-anthropic", "provider-openai"]
        );
        set(&p, "provider-openai", None).unwrap();
        assert!(get(&p, "provider-openai").unwrap().is_none());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&p).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert!(matches!(
                get(&p, "provider-anthropic"),
                Err(SecretsError::Permissions(..))
            ));
        }
        assert!(matches!(
            set(&p, "Bad Name", None),
            Err(SecretsError::BadName)
        ));
    }

    /// AUDIT L7: private keys under `keys/` are checked, the public
    /// certificates beside them are not.
    #[cfg(unix)]
    #[test]
    fn keys_open_to_others_are_refused() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        assert!(check_keys(dir.path()).is_ok(), "no keys yet");
        let keys = dir.path().join("keys");
        std::fs::create_dir(&keys).unwrap();
        for (name, m) in [
            ("ui-cert.pem", 0o644),
            ("ui-cert.der", 0o644),
            ("ui-names.txt", 0o644),
            ("ui-key.der", 0o600),
            ("node.key", 0o600),
        ] {
            std::fs::write(keys.join(name), "x").unwrap();
            std::fs::set_permissions(keys.join(name), std::fs::Permissions::from_mode(m)).unwrap();
        }
        assert!(check_keys(dir.path()).is_ok());
        for name in ["ui-key.der", "master-key.der", "node.key"] {
            let p = keys.join(name);
            std::fs::write(&p, "x").unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o640)).unwrap();
            let e = check_keys(dir.path()).unwrap_err().to_string();
            assert!(e.contains(name) && e.contains("chmod 600"), "{e}");
            assert!(e.contains("mode 640"), "{e}");
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        assert!(check_private(&dir.path().join("missing")).is_ok());
    }

    /// Gate 3 review N-3: a secret file of another user was read as long
    /// as its mode was private. Its owner is checked first.
    #[cfg(unix)]
    #[test]
    fn a_secret_file_of_another_user_is_refused() {
        use std::os::unix::fs::MetadataExt;
        let theirs = Path::new("/etc/passwd");
        if std::fs::metadata(theirs).unwrap().uid() == nix::unistd::geteuid().as_raw() {
            eprintln!("SKIPPED: {} belongs to this user", theirs.display());
            return;
        }
        let e = check_private(theirs).unwrap_err().to_string();
        assert!(e.contains("owned by another user"), "{e}");
        assert!(e.contains("/etc/passwd"), "{e}");
    }

    #[test]
    fn debug_never_shows_values() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("secrets.toml");
        set(&p, "k", Some(&Secret::new("sk-visible?".into()))).unwrap();
        let s = get(&p, "k").unwrap();
        assert!(!format!("{s:?}").contains("sk-visible"));
    }
}
