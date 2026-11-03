//! Where the web UI may listen, and HTTPS for it.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_rustls::server::TlsStream;

/// The WireGuard interface that owns `ip`, if any (Linux: the kernel marks
/// WireGuard devices with `DEVTYPE=wireguard`).
pub fn wireguard_interface(ip: IpAddr) -> Option<String> {
    let addrs = nix::ifaddrs::getifaddrs().ok()?;
    for a in addrs {
        let Some(sa) = a.address else { continue };
        let found = sa
            .as_sockaddr_in()
            .map(|s| IpAddr::V4(s.ip()))
            .or_else(|| sa.as_sockaddr_in6().map(|s| IpAddr::V6(s.ip())));
        if found == Some(ip) {
            let uevent =
                std::fs::read_to_string(format!("/sys/class/net/{}/uevent", a.interface_name))
                    .unwrap_or_default();
            if is_wireguard(&uevent) {
                return Some(a.interface_name);
            }
        }
    }
    None
}

fn is_wireguard(uevent: &str) -> bool {
    uevent.lines().any(|l| l.trim() == "DEVTYPE=wireguard")
}

/// How the UI is exposed, decided from the bind address and flags.
#[derive(Debug, PartialEq, Eq)]
pub enum Exposure {
    Loopback,
    /// Bound to a WireGuard interface address: reachable only by peers.
    WireGuard(String),
    /// Any other address, allowed by `--insecure-bind`.
    Insecure,
    /// All interfaces of a container's own network: who can reach it is
    /// decided by how the port is published (`-p 127.0.0.1:7777:7777`).
    Container,
}

/// Set by the image (`HARNESS_CONTAINER=1`); never guessed.
pub fn in_container() -> bool {
    std::env::var_os("HARNESS_CONTAINER").is_some_and(|v| v == "1")
}

/// Interfaces that exist only in a host's network namespace. Seeing one from
/// inside a container means it runs with host networking, where `0.0.0.0`
/// really is every interface of the host.
const HOST_ONLY_IFACES: [&str; 3] = ["docker0", "podman0", "cni-podman0"];

fn host_network_sign(ifaces: &[String]) -> Option<&str> {
    ifaces
        .iter()
        .map(String::as_str)
        .find(|i| HOST_ONLY_IFACES.contains(i))
}

fn interface_names() -> Vec<String> {
    nix::ifaddrs::getifaddrs()
        .map(|a| a.map(|i| i.interface_name).collect())
        .unwrap_or_default()
}

pub fn exposure(
    bind: SocketAddr,
    insecure_bind: bool,
    container: bool,
) -> anyhow::Result<Exposure> {
    let ip = bind.ip();
    if ip.is_loopback() {
        return Ok(Exposure::Loopback);
    }
    if ip.is_unspecified() && container && !insecure_bind {
        return container_exposure(bind, &interface_names());
    }
    if ip.is_unspecified() && !insecure_bind {
        anyhow::bail!(
            "refusing to listen on all interfaces ({bind}). Bind to your WireGuard address \
             (e.g. 10.8.0.1:7777), or pass --insecure-bind if you really mean it"
        );
    }
    if let Some(iface) = wireguard_interface(ip) {
        return Ok(Exposure::WireGuard(iface));
    }
    if insecure_bind {
        return Ok(Exposure::Insecure);
    }
    anyhow::bail!(
        "{ip} is not a WireGuard interface address. Bind to 127.0.0.1, to this machine's \
         WireGuard address, or pass --insecure-bind on a network you trust"
    )
}

fn container_exposure(bind: SocketAddr, ifaces: &[String]) -> anyhow::Result<Exposure> {
    if let Some(i) = host_network_sign(ifaces) {
        anyhow::bail!(
            "refusing {bind}: this container shares the host's network ({i} is visible), so it \
             would listen on every interface of the host. Bind 127.0.0.1:7777 or the WireGuard \
             address instead"
        );
    }
    Ok(Exposure::Container)
}

/// A TLS listener for `axum::serve`. Handshakes run in their own tasks with
/// a timeout, so a slow or silent client cannot hold up the others.
pub struct TlsListener {
    rx: mpsc::Receiver<(TlsStream<TcpStream>, SocketAddr)>,
    local: SocketAddr,
}

impl TlsListener {
    pub fn new(tcp: TcpListener, cfg: Arc<rustls::ServerConfig>) -> std::io::Result<Self> {
        let local = tcp.local_addr()?;
        let (tx, rx) = mpsc::channel(64);
        let acceptor = tokio_rustls::TlsAcceptor::from(cfg);
        tokio::spawn(async move {
            loop {
                let (sock, peer) = match tcp.accept().await {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!(error = %e, "accept failed");
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        continue;
                    }
                };
                let (acceptor, tx) = (acceptor.clone(), tx.clone());
                tokio::spawn(async move {
                    match tokio::time::timeout(Duration::from_secs(10), acceptor.accept(sock)).await
                    {
                        Ok(Ok(tls)) => {
                            let _ = tx.send((tls, peer)).await;
                        }
                        Ok(Err(e)) => tracing::debug!(%peer, error = %e, "TLS handshake failed"),
                        Err(_) => tracing::debug!(%peer, "TLS handshake timed out"),
                    }
                });
            }
        });
        Ok(Self { rx, local })
    }
}

impl axum::serve::Listener for TlsListener {
    type Io = TlsStream<TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        match self.rx.recv().await {
            Some(v) => v,
            // The accept task never ends while the process runs.
            None => std::future::pending().await,
        }
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        Ok(self.local)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exposure_t(bind: SocketAddr, insecure: bool) -> anyhow::Result<Exposure> {
        exposure(bind, insecure, false)
    }

    #[test]
    fn container_bind() {
        let a: SocketAddr = "0.0.0.0:7777".parse().unwrap();
        let v = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            container_exposure(a, &v(&["lo", "eth0"])).unwrap(),
            Exposure::Container
        );
        let e = container_exposure(a, &v(&["lo", "eth0", "docker0"])).unwrap_err();
        assert!(e.to_string().contains("host's network"), "{e}");
        // Loopback stays loopback; a specific address follows the normal rules.
        assert_eq!(
            exposure("127.0.0.1:7777".parse().unwrap(), false, true).unwrap(),
            Exposure::Loopback
        );
        assert!(exposure("192.0.2.1:7777".parse().unwrap(), false, true).is_err());
    }

    #[test]
    fn exposure_rules() {
        let a = |s: &str| s.parse::<SocketAddr>().unwrap();
        assert_eq!(
            exposure_t(a("127.0.0.1:7777"), false).unwrap(),
            Exposure::Loopback
        );
        assert!(exposure_t(a("0.0.0.0:7777"), false).is_err());
        assert_eq!(
            exposure_t(a("0.0.0.0:7777"), true).unwrap(),
            Exposure::Insecure
        );
        // A documentation address is on no interface here, so not WireGuard.
        assert!(exposure_t(a("192.0.2.1:7777"), false).is_err());
        assert_eq!(
            exposure_t(a("192.0.2.1:7777"), true).unwrap(),
            Exposure::Insecure
        );
        // A real, non-WireGuard interface address on this machine is refused.
        let real = nix::ifaddrs::getifaddrs()
            .unwrap()
            .filter_map(|i| i.address.and_then(|a| a.as_sockaddr_in().map(|s| s.ip())))
            .find(|ip| !ip.is_loopback());
        if let Some(ip) = real {
            assert!(wireguard_interface(IpAddr::V4(ip)).is_none());
            assert!(exposure(SocketAddr::new(IpAddr::V4(ip), 7777), false, false).is_err());
        }
    }

    #[test]
    fn wireguard_uevent() {
        assert!(is_wireguard(
            "DEVTYPE=wireguard\nINTERFACE=wg0\nIFINDEX=5\n"
        ));
        assert!(!is_wireguard("INTERFACE=eth0\nIFINDEX=4\n"));
        assert!(!is_wireguard("DEVTYPE=wireguardx\n"));
    }
}
