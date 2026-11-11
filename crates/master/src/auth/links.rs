//! Links for what the browser fetches by itself (Gate 3 P-1).
//!
//! An `<img>`, `<video>`, `<audio>` or `<a download>` cannot send the
//! login key header, and the cookie alone must not be enough. So the page
//! asks for a link with an authenticated call (`POST /api/links`) and puts
//! that in the element. A link is:
//!
//! * for one URL: the path and query it was minted for, nothing else;
//! * for one login: the MAC covers the SHA-256 of the session cookie, and
//!   the request must carry that cookie, so a link copied out of the
//!   browser (its download list, say) is useless without the login, and a
//!   client with the cookie and no link gets nothing;
//! * short-lived: [`LINK_TTL_MS`], and ended with the login (the session
//!   row is looked up on every use), and with the process (the key is new
//!   at every start);
//! * accepted only by the handlers that serve bytes ([`linkable`]).
//!
//! It holds no secret of its own: it is a MAC over public parts with this
//! process's key, so the "no secret in a URL" rule holds (the cookie never
//! appears in it), and only paths are ever logged, never queries.
//!
//! HMAC-SHA256 is RFC 2104 over `sha2` (already a dependency); there is no
//! HMAC crate in the tree and this change adds none.

use axum::http::Uri;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// How long a minted link works.
pub const LINK_TTL_MS: i64 = 30 * 60 * 1000;
/// Most links one call mints.
pub const MAX_LINKS: usize = 200;
/// Longest URL a link is minted for.
pub const MAX_TARGET: usize = 2048;
/// The query parameter that carries a link.
pub const PARAM: &str = "link";

/// The URLs a link may be minted for: the handlers that serve bytes to an
/// element or a download, and nothing that changes state or lists a chat.
pub fn linkable(path: &str) -> bool {
    let seg: Vec<&str> = path.split('/').collect();
    let id = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    match seg.as_slice() {
        ["", "api", "media", m, "content" | "thumb"] => id(m),
        ["", "api", "artifacts", a, "latest", "download"] => id(a),
        [
            "",
            "api",
            "artifacts",
            a,
            "versions",
            v,
            "download" | "preview" | "thumb",
        ] => id(a) && id(v),
        _ => false,
    }
}

/// Clones share the key (the `Auth` they are in is cloned per request).
#[derive(Clone)]
pub struct Signer {
    key: [u8; 32],
}

impl Default for Signer {
    fn default() -> Self {
        Self::new()
    }
}

fn hmac(key: &[u8; 32], msg: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut k = [0u8; BLOCK];
    k[..32].copy_from_slice(key);
    let (mut ipad, mut opad) = ([0x36u8; BLOCK], [0x5cu8; BLOCK]);
    for i in 0..BLOCK {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }
    let inner = Sha256::new()
        .chain_update(ipad)
        .chain_update(msg)
        .finalize();
    Sha256::new()
        .chain_update(opad)
        .chain_update(inner)
        .finalize()
        .into()
}

/// The URL without its link parameter, when it has exactly one: the URL
/// the link was minted for, and the link.
fn split(uri: &Uri) -> Option<(String, &str)> {
    let query = uri.query()?;
    let mut link = None;
    let mut rest = Vec::new();
    for p in query.split('&') {
        match p.strip_prefix(PARAM).and_then(|v| v.strip_prefix('=')) {
            Some(v) if link.is_none() => link = Some(v),
            Some(_) => return None,
            None => rest.push(p),
        }
    }
    let target = if rest.is_empty() {
        uri.path().to_string()
    } else {
        format!("{}?{}", uri.path(), rest.join("&"))
    };
    Some((target, link?))
}

impl Signer {
    pub fn new() -> Self {
        let mut key = [0u8; 32];
        getrandom::fill(&mut key).expect("OS randomness");
        Self { key }
    }

    fn mac(&self, session_hash: &str, expires_ms: i64, target: &str) -> String {
        let msg = format!("harness-link\n{session_hash}\n{expires_ms}\n{target}");
        hex::encode(hmac(&self.key, msg.as_bytes()))
    }

    /// `target` (a path and an optional query) with a link for the login
    /// whose cookie hashes to `session_hash`, or `None` when no link is
    /// made for it.
    pub fn mint(&self, session_hash: &str, target: &str, now_ms: i64) -> Option<String> {
        let ok_chars = target.bytes().all(|b| b.is_ascii_graphic() && b != b'#');
        if target.len() > MAX_TARGET || !ok_chars {
            return None;
        }
        let (path, query) = match target.split_once('?') {
            Some((p, q)) => (p, Some(q)),
            None => (target, None),
        };
        if !linkable(path) {
            return None;
        }
        // The query must survive `split` unchanged.
        if let Some(q) = query
            && (q.is_empty() || q.split('&').any(|p| p.is_empty() || p.starts_with(PARAM)))
        {
            return None;
        }
        let exp = now_ms + LINK_TTL_MS;
        let sep = if query.is_some() { '&' } else { '?' };
        let mac = self.mac(session_hash, exp, target);
        Some(format!("{target}{sep}{PARAM}={exp}.{mac}"))
    }

    /// Does `uri` carry a live link for the login `session_hash`?
    pub fn verify(&self, session_hash: &str, uri: &Uri, now_ms: i64) -> bool {
        let Some((target, link)) = split(uri) else {
            return false;
        };
        let Some((exp, mac)) = link.split_once('.') else {
            return false;
        };
        let Ok(exp) = exp.parse::<i64>() else {
            return false;
        };
        if exp <= now_ms || exp > now_ms + LINK_TTL_MS || !linkable(uri.path()) {
            return false;
        }
        let want = self.mac(session_hash, exp, &target);
        want.len() == mac.len() && bool::from(want.as_bytes().ct_eq(mac.as_bytes()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 4231 case 1 with its key widened to our 32 bytes of 0x0b; the
    /// expected value is Python's `hmac.new(b"\x0b" * 32, b"Hi There",
    /// sha256)`.
    #[test]
    fn hmac_matches_a_reference() {
        let key = [0x0bu8; 32];
        assert_eq!(
            hex::encode(hmac(&key, b"Hi There")),
            "198a607eb44bfbc69903a0f1cf2bbdc5ba0aa3f3d9ae3c1c7a3b1696a0b68cf7"
        );
    }

    fn uri(s: &str) -> Uri {
        s.parse().unwrap()
    }

    #[test]
    fn a_link_is_for_one_url_one_login_and_a_while() {
        let s = Signer::new();
        let me = "a".repeat(64);
        let t = "/api/media/m1/content?download=1";
        let link = s.mint(&me, t, 1000).unwrap();
        assert!(link.starts_with("/api/media/m1/content?download=1&link="));
        assert!(s.verify(&me, &uri(&link), 1000));
        assert!(s.verify(&me, &uri(&link), 1000 + LINK_TTL_MS - 1));
        assert!(!s.verify(&me, &uri(&link), 1000 + LINK_TTL_MS), "expired");
        assert!(
            !s.verify(&"b".repeat(64), &uri(&link), 1000),
            "another login"
        );
        assert!(
            !Signer::new().verify(&me, &uri(&link), 1000),
            "another process"
        );
        // Not another URL, query or parameter order.
        let other = link.replace("/m1/", "/m2/");
        assert!(!s.verify(&me, &uri(&other), 1000));
        let inline = link.replace("download=1&", "");
        assert!(!s.verify(&me, &uri(&inline), 1000));
        let twice = format!("{link}&{}", link.split_once('&').unwrap().1);
        assert!(!s.verify(&me, &uri(&twice), 1000), "two links");
        let plain = s.mint(&me, "/api/media/m1/thumb", 1000).unwrap();
        assert!(plain.starts_with("/api/media/m1/thumb?link="));
        assert!(s.verify(&me, &uri(&plain), 1000));
        // A forged expiry far out is refused whatever its MAC.
        let (head, _) = plain.split_once("link=").unwrap();
        let far = format!("{head}link={}.{}", i64::MAX, "0".repeat(64));
        assert!(!s.verify(&me, &uri(&far), 1000));
    }

    #[test]
    fn only_byte_urls_get_links() {
        let s = Signer::new();
        let me = "a".repeat(64);
        for ok in [
            "/api/media/m1/content",
            "/api/media/m1/thumb",
            "/api/artifacts/a1/latest/download",
            "/api/artifacts/a1/versions/v1/download",
            "/api/artifacts/a1/versions/v1/preview",
            "/api/artifacts/a1/versions/v1/thumb",
        ] {
            assert!(s.mint(&me, ok, 0).is_some(), "{ok}");
        }
        for bad in [
            "/api/sessions/s1/events",
            "/api/sessions/s1/approvals/a1",
            "/api/media/m1",
            "/api/media/../m1/content",
            "/api/media/m1/content/x",
            "/api/favicon?url=x",
            "/api/media/m1/content?",
            "/api/media/m1/content?link=1",
            "/api/media/m1/content?a=1&&b=2",
            "/api/media/m1/content#x",
            "/api/media/m 1/content",
        ] {
            assert!(s.mint(&me, bad, 0).is_none(), "{bad}");
        }
        let long = format!("/api/media/m1/content?x={}", "a".repeat(MAX_TARGET));
        assert!(s.mint(&me, &long, 0).is_none());
    }
}
