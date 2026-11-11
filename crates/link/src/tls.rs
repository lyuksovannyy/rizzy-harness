//! The master's self-signed certificate and a client verifier that trusts
//! exactly one certificate fingerprint (no CAs).

use std::path::Path;
use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use sha2::{Digest, Sha256};

use crate::{LinkError, Result};

/// `sha256:<hex>` of a DER certificate.
pub fn fingerprint(der: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(der)))
}

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::aws_lc_rs::default_provider())
}

pub struct MasterCert {
    pub cert_der: Vec<u8>,
    key_der: Vec<u8>,
}

impl MasterCert {
    /// Load `keys/master-cert.der` + `keys/master-key.der`, creating them
    /// (key 0600) on first use: only when neither exists. Any other read
    /// error, or half a pair, is an error with the reason: a new pair would
    /// silently break every node's pinned fingerprint (Gate 3 review N-4).
    pub fn load_or_create(dir: &Path) -> Result<Self> {
        let (c, k) = (
            dir.join("keys/master-cert.der"),
            dir.join("keys/master-key.der"),
        );
        let read = |p: &Path| match std::fs::read(p) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(LinkError::Key(format!(
                "cannot read {}: {e}; the node link's certificate is not replaced (that would \
                 break every node's pin): fix the file or restore it",
                p.display()
            ))),
        };
        match (read(&c)?, read(&k)?) {
            (Some(cert_der), Some(key_der)) => return Ok(Self { cert_der, key_der }),
            (None, None) => {}
            (cert, _) => {
                let missing = if cert.is_none() { &c } else { &k };
                return Err(LinkError::Key(format!(
                    "{} is missing but its pair is there; restore it, or remove both to make a \
                     new certificate (every node must then join again)",
                    missing.display()
                )));
            }
        }
        let ck = rcgen::generate_simple_self_signed(vec![format!("{}-master", "harness")])
            .map_err(|e| LinkError::Tls(e.to_string()))?;
        let me = Self {
            cert_der: ck.cert.der().to_vec(),
            key_der: ck.signing_key.serialize_der(),
        };
        crate::write_private(&k, &me.key_der)?;
        crate::write_private(&c, &me.cert_der)?;
        Ok(me)
    }

    /// A self-signed certificate for `sans` (names or IP addresses) stored
    /// as `keys/<stem>-cert.der` / `-key.der`; regenerated when the names change.
    pub fn load_or_create_for(dir: &Path, stem: &str, sans: &[String]) -> Result<Self> {
        let keys = dir.join("keys");
        let (c, k, n) = (
            keys.join(format!("{stem}-cert.der")),
            keys.join(format!("{stem}-key.der")),
            keys.join(format!("{stem}-names.txt")),
        );
        let want = sans.join("\n");
        if let (Ok(cert_der), Ok(key_der), Ok(names)) = (
            std::fs::read(&c),
            std::fs::read(&k),
            std::fs::read_to_string(&n),
        ) && names == want
        {
            return Ok(Self { cert_der, key_der });
        }
        let ck = rcgen::generate_simple_self_signed(sans.to_vec())
            .map_err(|e| LinkError::Tls(e.to_string()))?;
        let me = Self {
            cert_der: ck.cert.der().to_vec(),
            key_der: ck.signing_key.serialize_der(),
        };
        crate::write_private(&k, &me.key_der)?;
        crate::write_private(&c, &me.cert_der)?;
        std::fs::write(&n, want)?;
        // PEM copy for importing into a browser or OS trust store.
        std::fs::write(keys.join(format!("{stem}-cert.pem")), ck.cert.pem())?;
        Ok(me)
    }

    pub fn fingerprint(&self) -> String {
        fingerprint(&self.cert_der)
    }

    pub fn server_config(&self) -> Result<Arc<rustls::ServerConfig>> {
        let cfg = rustls::ServerConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()
            .map_err(|e| LinkError::Tls(e.to_string()))?
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(self.cert_der.clone())],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key_der.clone())),
            )
            .map_err(|e| LinkError::Tls(e.to_string()))?;
        Ok(Arc::new(cfg))
    }
}

/// Accepts only the certificate with the pinned fingerprint; handshake
/// signatures are still verified normally.
#[derive(Debug)]
struct Pinned {
    fingerprint: String,
    provider: Arc<rustls::crypto::CryptoProvider>,
}

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        if fingerprint(end_entity) == self.fingerprint {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General(
                "master certificate does not match the pinned fingerprint".into(),
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

pub fn pinned_client_config(fingerprint: &str) -> Result<Arc<rustls::ClientConfig>> {
    let p = provider();
    let cfg = rustls::ClientConfig::builder_with_provider(p.clone())
        .with_safe_default_protocol_versions()
        .map_err(|e| LinkError::Tls(e.to_string()))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(Pinned {
            fingerprint: fingerprint.to_string(),
            provider: p,
        }))
        .with_no_client_auth();
    Ok(Arc::new(cfg))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Gate 3 review N-4: any read error made a new key pair, which
    /// silently broke every node's pin. Only a missing pair is created.
    #[test]
    fn an_unreadable_master_key_stops_instead_of_making_a_new_one() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let first = MasterCert::load_or_create(dir.path())
            .unwrap()
            .fingerprint();
        assert_eq!(
            MasterCert::load_or_create(dir.path())
                .unwrap()
                .fingerprint(),
            first,
            "loaded again"
        );
        let key = dir.path().join("keys/master-key.der");
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read(&key).is_ok() {
            eprintln!("SKIPPED: running as a user who reads any file");
            return;
        }
        let e = MasterCert::load_or_create(dir.path())
            .err()
            .expect("an unreadable key must stop the server");
        assert!(e.to_string().contains("master-key.der"), "{e}");
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(
            MasterCert::load_or_create(dir.path())
                .unwrap()
                .fingerprint(),
            first,
            "the pair was kept"
        );
        // Half a pair is not a missing pair either.
        std::fs::remove_file(dir.path().join("keys/master-cert.der")).unwrap();
        assert!(MasterCert::load_or_create(dir.path()).is_err());
    }
}
