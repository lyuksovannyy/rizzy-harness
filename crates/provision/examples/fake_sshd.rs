//! The test SSH server as a process, for browser tests:
//! `cargo run -p harness-provision --features test-util --example fake_sshd`
//! prints `port=<n> fingerprint=<fp> home=<dir>` and serves until killed.

#[tokio::main]
async fn main() {
    let s = harness_provision::testing::start().await;
    println!(
        "port={} fingerprint={} home={}",
        s.port,
        s.fingerprint,
        s.home.path().display()
    );
    std::future::pending::<()>().await;
}
