//! Node identity: an Ed25519 key pair in `keys/node.key` (PKCS#8, 0600).

use std::path::Path;

use aws_lc_rs::signature::{ED25519, Ed25519KeyPair, KeyPair, UnparsedPublicKey};

use crate::{LinkError, Result};

pub struct NodeKey {
    pair: Ed25519KeyPair,
}

impl NodeKey {
    pub fn generate() -> Result<(Self, Vec<u8>)> {
        let doc = Ed25519KeyPair::generate_pkcs8(&aws_lc_rs::rand::SystemRandom::new())
            .map_err(|e| LinkError::Key(e.to_string()))?;
        let bytes = doc.as_ref().to_vec();
        Ok((Self::from_pkcs8(&bytes)?, bytes))
    }

    pub fn from_pkcs8(bytes: &[u8]) -> Result<Self> {
        Ed25519KeyPair::from_pkcs8(bytes)
            .map(|pair| Self { pair })
            .map_err(|e| LinkError::Key(e.to_string()))
    }

    /// Load the key at `path`, creating it (0600) on first use.
    pub fn load_or_create(path: &Path) -> Result<Self> {
        match std::fs::read(path) {
            Ok(b) => Self::from_pkcs8(&b),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let (k, bytes) = Self::generate()?;
                crate::write_private(path, &bytes)?;
                Ok(k)
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Hex public key (32 bytes).
    pub fn public_hex(&self) -> String {
        hex::encode(self.pair.public_key().as_ref())
    }

    pub fn sign_hex(&self, msg: &[u8]) -> String {
        hex::encode(self.pair.sign(msg).as_ref())
    }
}

pub fn verify(public_hex: &str, msg: &[u8], sig_hex: &str) -> bool {
    let (Ok(pk), Ok(sig)) = (hex::decode(public_hex), hex::decode(sig_hex)) else {
        return false;
    };
    pk.len() == 32
        && UnparsedPublicKey::new(&ED25519, pk)
            .verify(msg, &sig)
            .is_ok()
}

/// What a node signs to authenticate: domain tag, nonce, and the master's
/// certificate fingerprint (channel binding).
pub fn auth_message(nonce_hex: &str, master_fingerprint: &str) -> Vec<u8> {
    format!("harness-node-auth-v1\n{nonce_hex}\n{master_fingerprint}").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_verify_and_persist() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("keys/node.key");
        let k = NodeKey::load_or_create(&p).unwrap();
        let again = NodeKey::load_or_create(&p).unwrap();
        assert_eq!(k.public_hex(), again.public_hex());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&p).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let m = auth_message("aa", "fp");
        let sig = k.sign_hex(&m);
        assert!(verify(&k.public_hex(), &m, &sig));
        assert!(!verify(
            &k.public_hex(),
            &auth_message("aa", "other-master"),
            &sig
        ));
        let (other, _) = NodeKey::generate().unwrap();
        assert!(!verify(&other.public_hex(), &m, &sig));
        assert!(!verify("zz", &m, &sig));
    }
}
