//! An in-process SSH server for tests: one password, keys from
//! `<home>/.ssh/authorized_keys`, and commands run with `bash -c` as the
//! current user with HOME pointing at a temporary directory. Nothing outside
//! that directory is touched.

#![allow(dead_code)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use russh::keys::{HashAlg, PrivateKey};
use russh::server::{self, Auth, Msg, Session, VerifiedPublicKey};
use russh::{Channel, ChannelId};

pub const PASSWORD: &str = "pw-harness-test";

/// Per channel: the command and its stdin so far.
type Pending = Arc<Mutex<HashMap<ChannelId, (String, Vec<u8>)>>>;

#[derive(Clone)]
struct H {
    home: PathBuf,
    allow_password: Arc<Mutex<bool>>,
    pending: Pending,
}

impl server::Handler for H {
    type Error = russh::Error;

    async fn auth_password(&mut self, _user: &str, password: &str) -> Result<Auth, Self::Error> {
        let ok = *self.allow_password.lock().unwrap() && password == PASSWORD;
        Ok(if ok { Auth::Accept } else { Auth::reject() })
    }

    async fn auth_publickey(
        &mut self,
        _user: &str,
        key: VerifiedPublicKey<'_>,
    ) -> Result<Auth, Self::Error> {
        let mine = key.public_key.to_openssh().unwrap_or_default();
        let mine = mine
            .split_whitespace()
            .take(2)
            .collect::<Vec<_>>()
            .join(" ");
        let listed =
            std::fs::read_to_string(self.home.join(".ssh/authorized_keys")).unwrap_or_default();
        let ok = listed
            .lines()
            .any(|l| l.split_whitespace().take(2).collect::<Vec<_>>().join(" ") == mine);
        Ok(if ok { Auth::Accept } else { Auth::reject() })
    }

    async fn channel_open_session(
        &mut self,
        _channel: Channel<Msg>,
        reply: server::ChannelOpenHandle,
        _s: &mut Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        Ok(())
    }

    async fn exec_request(
        &mut self,
        ch: ChannelId,
        data: &[u8],
        s: &mut Session,
    ) -> Result<(), Self::Error> {
        let cmd = String::from_utf8_lossy(data).into_owned();
        // `sudo -n` is the real-server way to get root; here it is a no-op.
        let cmd = cmd
            .strip_prefix("sudo -n ")
            .map(str::to_string)
            .unwrap_or(cmd);
        self.pending.lock().unwrap().insert(ch, (cmd, Vec::new()));
        s.channel_success(ch)?;
        Ok(())
    }

    async fn data(
        &mut self,
        ch: ChannelId,
        data: &[u8],
        _s: &mut Session,
    ) -> Result<(), Self::Error> {
        if let Some(p) = self.pending.lock().unwrap().get_mut(&ch) {
            p.1.extend_from_slice(data);
        }
        Ok(())
    }

    async fn channel_eof(&mut self, ch: ChannelId, s: &mut Session) -> Result<(), Self::Error> {
        let Some((cmd, input)) = self.pending.lock().unwrap().remove(&ch) else {
            return Ok(());
        };
        let home = self.home.clone();
        let h = s.handle();
        tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            let mut child = tokio::process::Command::new("bash")
                .arg("-c")
                .arg(&cmd)
                .env("HOME", &home)
                .current_dir(&home)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            let mut stdin = child.stdin.take().unwrap();
            stdin.write_all(&input).await.ok();
            drop(stdin);
            let out = child.wait_with_output().await.unwrap();
            if !out.stdout.is_empty() {
                let _ = h.data(ch, out.stdout).await;
            }
            if !out.stderr.is_empty() {
                let _ = h.extended_data(ch, 1, out.stderr).await;
            }
            let _ = h
                .exit_status_request(ch, out.status.code().unwrap_or(255) as u32)
                .await;
            let _ = h.eof(ch).await;
            let _ = h.close(ch).await;
        });
        Ok(())
    }
}

pub struct FakeSshd {
    pub port: u16,
    pub fingerprint: String,
    pub home: tempfile::TempDir,
    allow_password: Arc<Mutex<bool>>,
}

impl FakeSshd {
    /// Turn password login off, as the hardening step would.
    pub fn set_password_login(&self, on: bool) {
        *self.allow_password.lock().unwrap() = on;
    }
}

pub async fn start() -> FakeSshd {
    let home = tempfile::tempdir().unwrap();
    let mut seed = [7u8; 32];
    seed[0] = 1;
    let host = PrivateKey::from(russh::keys::ssh_key::private::Ed25519Keypair::from_seed(
        &seed,
    ));
    let fingerprint = host.public_key().fingerprint(HashAlg::Sha256).to_string();
    let cfg = Arc::new(server::Config {
        keys: vec![host],
        auth_rejection_time: std::time::Duration::from_millis(10),
        auth_rejection_time_initial: Some(std::time::Duration::from_millis(0)),
        ..Default::default()
    });
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    let allow_password = Arc::new(Mutex::new(true));
    let handler = H {
        home: home.path().to_path_buf(),
        allow_password: allow_password.clone(),
        pending: Arc::default(),
    };
    tokio::spawn(async move {
        loop {
            let (sock, _) = l.accept().await.unwrap();
            let (cfg, h) = (cfg.clone(), handler.clone());
            tokio::spawn(async move {
                if let Ok(s) = server::run_stream(cfg, sock, h).await {
                    let _ = s.await;
                }
            });
        }
    });
    FakeSshd {
        port,
        fingerprint,
        home,
        allow_password,
    }
}
