//! The watchdog: read-only checks of a set-up server. It only reports;
//! fixing is up to the user (or an automatic-mode action they start).

use serde::{Deserialize, Serialize};

use crate::scripts::WATCH;
use crate::setup::Connector;
use crate::ssh::{Target, auth_methods};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Problem {
    /// `ssh`, `firewall`, `reach`, `host_key`.
    pub kind: String,
    pub detail: String,
}

/// Check SSH (from outside: which login methods are offered) and/or the
/// firewall (through harness's login and its read-only sudo rule).
pub async fn check(
    target: &Target,
    fingerprint: &str,
    conn: &dyn Connector,
    ssh: bool,
    firewall: bool,
) -> Vec<Problem> {
    let mut out = vec![];
    if ssh {
        match auth_methods(target, fingerprint).await {
            Ok(m) => {
                if m.iter()
                    .any(|x| x == "password" || x == "keyboard-interactive")
                {
                    out.push(Problem {
                        kind: "ssh".into(),
                        detail: format!("password logins are on again (offered: {})", m.join(", ")),
                    });
                }
            }
            Err(crate::Error::HostKeyChanged { expected, got }) => out.push(Problem {
                kind: "host_key".into(),
                detail: format!(
                    "the server's host key changed ({expected} -> {got}); harness will not log in"
                ),
            }),
            Err(e) => out.push(Problem {
                kind: "reach".into(),
                detail: format!("SSH check failed: {e}"),
            }),
        }
    }
    if firewall && !out.iter().any(|p| p.kind == "host_key") {
        match conn.connect().await {
            Err(e) => out.push(Problem { kind: "reach".into(), detail: format!("could not log in to check the firewall: {e}") }),
            Ok(r) => match r.run(WATCH, false).await {
                Ok(o) => match o.stdout.trim() {
                    "fw=on" => {}
                    "fw=off" => out.push(Problem {
                        kind: "firewall".into(),
                        detail: "the harness firewall table is not loaded".into(),
                    }),
                    _ => out.push(Problem {
                        kind: "firewall".into(),
                        detail: "cannot check the firewall: harness has no read-only sudo rule for it here".into(),
                    }),
                },
                Err(e) => out.push(Problem { kind: "reach".into(), detail: e.to_string() }),
            },
        }
    }
    out
}
