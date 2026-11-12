#!/usr/bin/env python3
"""netprobe.py <lan-ip> <tcp-lo> <tcp-lan> <udp-lo> <udp-lan> <tag> [internet]
Prints one word per probe. UDP probes only send; the host listener reports
what arrived. 'internet' adds one DNS lookup and one HTTPS HEAD to
example.com."""
import socket
import sys

lan, tl, tn, ul, un, tag = sys.argv[1:7]
internet = len(sys.argv) > 7


def tcp(host, port, t=2.0):
    try:
        s = socket.create_connection((host, int(port)), timeout=t)
        s.settimeout(t)
        data = s.recv(16)
        s.close()
        return "CONNECTED:" + data.decode(errors="replace").strip()
    except Exception as e:
        return type(e).__name__ + (":" + str(e.errno) if getattr(e, "errno", None) else "")


def udp(host, port):
    try:
        s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        s.sendto(f"{tag}->{host}".encode(), (host, int(port)))
        return "sent"
    except Exception as e:
        return type(e).__name__ + ":" + str(getattr(e, "errno", ""))


def dns(name):
    try:
        return "resolved:" + socket.gethostbyname(name)
    except Exception as e:
        return type(e).__name__


out = {
    "tcp-127.0.0.1": tcp("127.0.0.1", tl),
    "tcp-lan": tcp(lan, tn),
    "tcp-host.containers.internal": tcp("host.containers.internal", tn) if dns("host.containers.internal").startswith("resolved") else "no-such-name",
    "tcp-169.254.1.2(pasta-host-map)-lo-port": tcp("169.254.1.2", tl),
    "tcp-metadata-169.254.169.254:80": tcp("169.254.169.254", 80),
    "udp-127.0.0.1": udp("127.0.0.1", ul),
    "udp-lan": udp(lan, un),
}
out["dns-example.com"] = dns("example.com")
if internet:
    try:
        import http.client
        c = http.client.HTTPSConnection("example.com", timeout=5)
        c.request("HEAD", "/")
        out["https-head-example.com"] = "HTTP %d" % c.getresponse().status
    except Exception as e:
        out["https-head-example.com"] = type(e).__name__
for k, v in out.items():
    print(f"   {k:<42} {v}")
