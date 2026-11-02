//! `<data>/secrets.toml`: API keys entered in the UI (PLAN.md §0 onboarding).
//!
//! * Mode 0600, written atomically (temp file with 0600, then rename).
//! * Values are `Secret<String>`: redacted in `Debug`, never returned to the
//!   browser, never logged, never written to the event log.
//! * Config refers to keys by name (`api_key_secret = "openai"`); headless
//!   installs can keep using `api_key_env` instead.

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
    #[error("{0} must not be readable by group/others (chmod 600)")]
    Permissions(PathBuf),
    #[error("secret names must be 1-64 characters: a-z, 0-9, _ -")]
    BadName,
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

fn read(path: &Path) -> Result<File, SecretsError> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(File::default()),
        Err(e) => return Err(SecretsError::Io(path.into(), e)),
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(path)
            .map_err(|e| SecretsError::Io(path.into(), e))?
            .permissions()
            .mode();
        if mode & 0o077 != 0 {
            return Err(SecretsError::Permissions(path.into()));
        }
    }
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
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
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
        assert!(get(&p, "openai").unwrap().is_none());
        set(&p, "openai", Some(&Secret::new("sk-abc".into()))).unwrap();
        set(&p, "anthropic", Some(&Secret::new("sk-ant".into()))).unwrap();
        assert_eq!(get(&p, "openai").unwrap().unwrap().expose(), "sk-abc");
        assert_eq!(names(&p).unwrap(), vec!["anthropic", "openai"]);
        set(&p, "openai", None).unwrap();
        assert!(get(&p, "openai").unwrap().is_none());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&p).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert!(matches!(
                get(&p, "anthropic"),
                Err(SecretsError::Permissions(_))
            ));
        }
        assert!(matches!(
            set(&p, "Bad Name", None),
            Err(SecretsError::BadName)
        ));
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
