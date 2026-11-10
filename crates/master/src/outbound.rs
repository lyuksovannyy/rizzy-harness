//! Where the server itself connects for an admin's provider test or model
//! listing (AUDIT L3, PLAN.md T23).
//!
//! Those calls fetch a base URL an admin typed and show part of the answer
//! (the start of an error body, a model list), so without a check they
//! read the server's own network: cloud metadata, a router, a database's
//! HTTP port. A URL must pass [`url_allowed`] (https, or plain http only to
//! this machine or a listed host) and then reaches public addresses only,
//! through `web_fetch`'s resolver: the address checked is the address
//! connected to, for every connection, so a name that later resolves
//! inward (DNS rebinding) is refused too. Redirects stay on the same
//! origin (`harness_agent_native::models::same_origin_redirects`), so they
//! cannot leave the checked host.
//!
//! Local model servers (Ollama on this machine, a LAN or container host)
//! are allowed by name: `HARNESS_PLAIN_HTTP_HOSTS=localhost,ollama`. That
//! includes `localhost`: a fresh Ollama setup needs it listed, and the
//! refusal says so with the exact value to add.

use crate::config::url_allowed;
use harness_agent_native::models::Dns;

/// The hosts the operator vouches for (`HARNESS_PLAIN_HTTP_HOSTS`).
#[derive(Debug, Clone, Default)]
pub struct Outbound {
    listed: Vec<String>,
}

impl Outbound {
    /// The list in `HARNESS_PLAIN_HTTP_HOSTS` (comma-separated host names
    /// or addresses, no ports).
    pub fn from_env() -> Self {
        Self::new(
            std::env::var("HARNESS_PLAIN_HTTP_HOSTS")
                .unwrap_or_default()
                .split(',')
                .map(str::to_string),
        )
    }

    pub fn new(hosts: impl IntoIterator<Item = String>) -> Self {
        Self {
            listed: hosts
                .into_iter()
                .map(|h| h.trim().to_ascii_lowercase())
                .filter(|h| !h.is_empty())
                .collect(),
        }
    }

    fn is_listed(&self, host: &str) -> bool {
        let host = host.trim_start_matches('[').trim_end_matches(']');
        self.listed
            .iter()
            .any(|h| h.trim_start_matches('[').trim_end_matches(']') == host)
    }

    /// May the server call `url` for an admin's test or model listing?
    /// `Ok` carries the resolver to connect through (`None` for a listed
    /// host, which may be private); `Err` says why not, and for a private
    /// host what to add to the environment.
    pub async fn provider_call(&self, url: &str) -> Result<Dns, String> {
        let refused = "base URL must be https:// (plain http only for localhost or a host in \
                       HARNESS_PLAIN_HTTP_HOSTS)";
        if !url_allowed(url) {
            return Err(refused.into());
        }
        let u = reqwest::Url::parse(url).map_err(|_| refused.to_string())?;
        let host = u.host_str().unwrap_or_default().to_ascii_lowercase();
        if self.is_listed(&host) {
            return Ok(None);
        }
        let private = || {
            let bare = host.trim_start_matches('[').trim_end_matches(']');
            format!(
                "{host} is a private or local address. If a model server runs there, add it \
                 to HARNESS_PLAIN_HTTP_HOSTS (HARNESS_PLAIN_HTTP_HOSTS={bare}) and restart \
                 the server"
            )
        };
        harness_web::check_public_url(&u).map_err(|_| private())?;
        let bare = host.trim_start_matches('[').trim_end_matches(']');
        if bare.parse::<std::net::IpAddr>().is_err() {
            // A name: answered here for a message that names the fix; the
            // resolver below still checks every connection.
            let port = u.port_or_known_default().unwrap_or(443);
            let addrs = tokio::net::lookup_host((bare, port))
                .await
                .map_err(|_| format!("{host} could not be resolved"))?;
            if !addrs.into_iter().any(|a| harness_web::is_public(a.ip())) {
                return Err(private());
            }
        }
        Ok(Some(harness_web::public_only_resolver()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn private_hosts_need_listing() {
        let none = Outbound::new([]);
        for url in [
            "http://127.0.0.1:11434/v1",
            "http://localhost:11434/v1",
            "http://[::1]:8000/v1",
            "https://10.0.0.5/v1",
            "https://169.254.169.254/latest",
            "https://192.168.1.2:8443/v1",
            "https://[fd00::1]/v1",
            "https://localhost/v1",
        ] {
            let e = none.provider_call(url).await.err().unwrap();
            assert!(e.contains("HARNESS_PLAIN_HTTP_HOSTS="), "{url}: {e}");
        }
        let e = none
            .provider_call("http://localhost:11434/v1")
            .await
            .err()
            .unwrap();
        assert!(e.contains("HARNESS_PLAIN_HTTP_HOSTS=localhost"), "{e}");
        // Plain http to another host is refused before anything else.
        let e = none
            .provider_call("http://example.com/v1")
            .await
            .err()
            .unwrap();
        assert!(e.contains("https://"), "{e}");
        assert!(none.provider_call("ftp://x/").await.is_err());
        assert!(
            none.provider_call("https://user:pw@example.com/")
                .await
                .is_err()
        );

        let listed = Outbound::new(["localhost".into(), " 127.0.0.1 ".into(), "[::1]".into()]);
        for url in [
            "http://localhost:11434/v1",
            "http://127.0.0.1:8000/v1",
            "http://[::1]:8000/v1",
        ] {
            assert!(listed.provider_call(url).await.unwrap().is_none(), "{url}");
        }
        // Listing one host does not open the others.
        assert!(listed.provider_call("https://10.0.0.5/v1").await.is_err());
    }
}
