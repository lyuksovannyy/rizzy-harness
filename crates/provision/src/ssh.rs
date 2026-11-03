//! A small SSH client for setting up nodes: host-key pinning, password or
//! key login, commands with exit codes, and harness's own login key.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_core::Secret;
use russh::ChannelMsg;
use russh::client::{self, Handle};
use russh::keys::{HashAlg, PrivateKey, PrivateKeyWithHashAlg, PublicKeyOrCertificate};

use crate::{Error, Result};

const TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone)]
pub struct Target {
    pub host: String,
    pub port: u16,
    pub user: String,
}

/// How to log in. Credentials live only in memory and are dropped after use.
pub enum Auth {
    Password(Secret<String>),
    /// OpenSSH private key text, optional passphrase.
    Key(Secret<String>, Option<Secret<String>>),
}

/// Records the server key; accepts only the pinned fingerprint (if any).
struct Pin {
    expected: Option<String>,
    seen: Arc<Mutex<Option<String>>>,
}

fn fingerprint_of(k: &PublicKeyOrCertificate) -> String {
    match k {
        PublicKeyOrCertificate::PublicKey { key, .. } => {
            key.fingerprint(HashAlg::Sha256).to_string()
        }
        PublicKeyOrCertificate::Certificate(c) => {
            c.public_key().fingerprint(HashAlg::Sha256).to_string()
        }
    }
}

impl client::Handler for Pin {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        key: &PublicKeyOrCertificate,
    ) -> std::result::Result<bool, Self::Error> {
        let fp = fingerprint_of(key);
        *self.seen.lock().unwrap_or_else(|p| p.into_inner()) = Some(fp.clone());
        Ok(self.expected.as_deref() == Some(fp.as_str()))
    }
}

fn config() -> Arc<client::Config> {
    Arc::new(client::Config {
        inactivity_timeout: Some(Duration::from_secs(120)),
        ..Default::default()
    })
}

/// The server's host key fingerprint (`SHA256:...`), without logging in.
/// Show it to the user to compare with the provider's console before any
/// credential is sent.
pub async fn probe(host: &str, port: u16) -> Result<String> {
    let seen = Arc::new(Mutex::new(None));
    let h = Pin {
        expected: None,
        seen: seen.clone(),
    };
    let _ = tokio::time::timeout(TIMEOUT, client::connect(config(), (host, port), h))
        .await
        .map_err(|_| Error::Ssh(format!("{host}:{port} did not answer")))?;
    let fp = seen.lock().unwrap_or_else(|p| p.into_inner()).clone();
    fp.ok_or_else(|| Error::Ssh(format!("could not read the host key of {host}:{port}")))
}

/// The login methods the server offers to `user` (e.g. `["publickey"]`),
/// asked without logging in. Used by the watchdog: `password` or
/// `keyboard-interactive` means password logins are on again.
pub async fn auth_methods(target: &Target, fingerprint: &str) -> Result<Vec<String>> {
    let seen = Arc::new(Mutex::new(None));
    let h = Pin {
        expected: Some(fingerprint.to_string()),
        seen: seen.clone(),
    };
    let mut handle = tokio::time::timeout(
        TIMEOUT,
        client::connect(config(), (target.host.as_str(), target.port), h),
    )
    .await
    .map_err(|_| Error::Ssh(format!("{}:{} did not answer", target.host, target.port)))?
    .map_err(|e| {
        let got = seen.lock().unwrap_or_else(|p| p.into_inner()).clone();
        match got {
            Some(g) if g != fingerprint => Error::HostKeyChanged {
                expected: fingerprint.into(),
                got: g,
            },
            _ => Error::Ssh(e.to_string()),
        }
    })?;
    let res = handle
        .authenticate_none(&target.user)
        .await
        .map_err(|e| Error::Ssh(e.to_string()))?;
    let methods = match res {
        russh::client::AuthResult::Success => vec!["none".to_string()],
        russh::client::AuthResult::Failure {
            remaining_methods, ..
        } => remaining_methods
            .iter()
            .map(|m| <&'static str>::from(m).to_string())
            .collect(),
    };
    let _ = handle
        .disconnect(russh::Disconnect::ByApplication, "", "en")
        .await;
    Ok(methods)
}

pub struct Output {
    pub code: u32,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn ok(&self) -> bool {
        self.code == 0
    }
}

pub struct Ssh {
    handle: Handle<Pin>,
    pub target: Target,
}

impl Ssh {
    /// Connect, refusing any host key but `fingerprint`, and log in.
    pub async fn connect(target: &Target, fingerprint: &str, auth: Auth) -> Result<Self> {
        let seen = Arc::new(Mutex::new(None));
        let h = Pin {
            expected: Some(fingerprint.to_string()),
            seen: seen.clone(),
        };
        let res = tokio::time::timeout(
            TIMEOUT,
            client::connect(config(), (target.host.as_str(), target.port), h),
        )
        .await
        .map_err(|_| Error::Ssh(format!("{}:{} did not answer", target.host, target.port)))?;
        let mut handle = match res {
            Ok(h) => h,
            Err(e) => {
                let got = seen.lock().unwrap_or_else(|p| p.into_inner()).clone();
                return Err(match got {
                    Some(g) if g != fingerprint => Error::HostKeyChanged {
                        expected: fingerprint.into(),
                        got: g,
                    },
                    _ => Error::Ssh(e.to_string()),
                });
            }
        };
        let ok = match auth {
            Auth::Password(p) => handle
                .authenticate_password(&target.user, p.expose().as_str())
                .await
                .map_err(|e| Error::Ssh(e.to_string()))?
                .success(),
            Auth::Key(k, pass) => {
                let key = russh::keys::decode_secret_key(
                    k.expose(),
                    pass.as_ref().map(|p| p.expose().as_str()),
                )
                .map_err(|e| Error::Ssh(format!("cannot read the private key: {e}")))?;
                let hash = handle
                    .best_supported_rsa_hash()
                    .await
                    .ok()
                    .flatten()
                    .flatten();
                handle
                    .authenticate_publickey(
                        &target.user,
                        PrivateKeyWithHashAlg::new(Arc::new(key), hash),
                    )
                    .await
                    .map_err(|e| Error::Ssh(e.to_string()))?
                    .success()
            }
        };
        if !ok {
            return Err(Error::AuthFailed);
        }
        Ok(Self {
            handle,
            target: target.clone(),
        })
    }

    /// Run `cmd` (through the login shell), optionally feeding `stdin`.
    pub async fn exec(&self, cmd: &str, stdin: Option<&[u8]>) -> Result<Output> {
        let mut ch = self
            .handle
            .channel_open_session()
            .await
            .map_err(|e| Error::Ssh(e.to_string()))?;
        ch.exec(true, cmd)
            .await
            .map_err(|e| Error::Ssh(e.to_string()))?;
        if let Some(data) = stdin {
            ch.data(data).await.map_err(|e| Error::Ssh(e.to_string()))?;
        }
        ch.eof().await.map_err(|e| Error::Ssh(e.to_string()))?;
        let (mut out, mut err, mut code) = (Vec::new(), Vec::new(), None);
        while let Some(msg) = ch.wait().await {
            match msg {
                ChannelMsg::Data { data } => out.extend_from_slice(&data),
                ChannelMsg::ExtendedData { data, .. } => err.extend_from_slice(&data),
                ChannelMsg::ExitStatus { exit_status } => code = Some(exit_status),
                _ => {}
            }
        }
        Ok(Output {
            code: code.unwrap_or(255),
            stdout: String::from_utf8_lossy(&out).into_owned(),
            stderr: String::from_utf8_lossy(&err).into_owned(),
        })
    }

    /// Run a bash script with root rights: as is for root, via `sudo -n`
    /// otherwise (passwordless sudo is required for automatic setup).
    pub async fn root_script(&self, script: &str) -> Result<Output> {
        let cmd = if self.target.user == "root" {
            "bash -s"
        } else {
            "sudo -n bash -s"
        };
        self.exec(cmd, Some(script.as_bytes())).await
    }

    pub async fn close(self) {
        let _ = self
            .handle
            .disconnect(russh::Disconnect::ByApplication, "", "en")
            .await;
    }
}

/// Harness's own login key for a node (ed25519), so the user's password or
/// private key is never stored.
pub struct HarnessKey {
    key: PrivateKey,
}

impl HarnessKey {
    pub fn generate() -> Self {
        let mut seed = [0u8; 32];
        getrandom::fill(&mut seed).expect("OS randomness");
        let pair = russh::keys::ssh_key::private::Ed25519Keypair::from_seed(&seed);
        let mut key = PrivateKey::from(pair);
        key.set_comment("harness");
        Self { key }
    }

    pub fn from_openssh(text: &str) -> Result<Self> {
        Ok(Self {
            key: russh::keys::decode_secret_key(text, None)
                .map_err(|e| Error::Ssh(e.to_string()))?,
        })
    }

    /// Private key in OpenSSH format (store 0600).
    pub fn private_openssh(&self) -> Secret<String> {
        Secret::new(
            self.key
                .to_openssh(russh::keys::ssh_key::LineEnding::LF)
                .expect("ed25519 encodes")
                .to_string(),
        )
    }

    /// `ssh-ed25519 AAAA... harness` for authorized_keys.
    pub fn public_openssh(&self) -> String {
        self.key.public_key().to_openssh().expect("ed25519 encodes")
    }

    pub fn auth(&self) -> Auth {
        Auth::Key(self.private_openssh(), None)
    }
}
