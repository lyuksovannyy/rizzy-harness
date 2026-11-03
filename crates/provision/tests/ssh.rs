//! The SSH client against an in-process server.

use harness_provision::testing as fake_sshd;

use harness_core::Secret;
use harness_provision::Error;
use harness_provision::ssh::{Auth, HarnessKey, Ssh, Target, probe};

#[tokio::test]
async fn probe_pin_password_key_and_exec() {
    let s = fake_sshd::start().await;
    let t = Target {
        host: "127.0.0.1".into(),
        port: s.port,
        user: "deploy".into(),
    };

    let fp = probe("127.0.0.1", s.port).await.unwrap();
    assert_eq!(fp, s.fingerprint);
    assert!(fp.starts_with("SHA256:"));

    // A wrong pin is refused before any credential is sent.
    let bad = "SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    let e = Ssh::connect(
        &t,
        bad,
        Auth::Password(Secret::new(fake_sshd::PASSWORD.into())),
    )
    .await
    .err()
    .unwrap();
    assert!(matches!(e, Error::HostKeyChanged { .. }), "{e}");

    let e = Ssh::connect(&t, &fp, Auth::Password(Secret::new("nope".into())))
        .await
        .err()
        .unwrap();
    assert!(matches!(e, Error::AuthFailed), "{e}");
    let c = Ssh::connect(
        &t,
        &fp,
        Auth::Password(Secret::new(fake_sshd::PASSWORD.into())),
    )
    .await
    .unwrap();
    let o = c.exec("echo hi; echo err >&2; exit 3", None).await.unwrap();
    assert_eq!(
        (o.code, o.stdout.as_str(), o.stderr.as_str()),
        (3, "hi\n", "err\n")
    );
    let o = c.exec("cat", Some(b"piped")).await.unwrap();
    assert_eq!(o.stdout, "piped");
    let o = c.root_script("echo from-script").await.unwrap();
    assert_eq!(o.stdout.trim(), "from-script");

    // Harness's own key: install it, then log in with it.
    let k = HarnessKey::generate();
    assert!(k.public_openssh().starts_with("ssh-ed25519 "));
    let o = c
        .exec(
            "mkdir -p ~/.ssh && chmod 700 ~/.ssh && cat >> ~/.ssh/authorized_keys",
            Some(format!("{}\n", k.public_openssh()).as_bytes()),
        )
        .await
        .unwrap();
    assert!(o.ok(), "{}", o.stderr);
    c.close().await;
    let again = HarnessKey::from_openssh(k.private_openssh().expose()).unwrap();
    let c = Ssh::connect(&t, &fp, again.auth()).await.unwrap();
    assert!(c.exec("true", None).await.unwrap().ok());
    let e = Ssh::connect(&t, &fp, HarnessKey::generate().auth())
        .await
        .err()
        .unwrap();
    assert!(matches!(e, Error::AuthFailed), "{e}");
}
