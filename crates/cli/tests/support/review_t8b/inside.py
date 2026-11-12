#!/usr/bin/env python3
"""T.8b review: what a command has with the network off.

Run INSIDE a contained command by review_t8b_proofs.rs (never by hand on the
host). argv[1] is a JSON file the test wrote into the session's folder: the
addresses of listeners the test itself holds on this machine.

Safety: unless every interface this process sees is `lo`, it prints
NOT-ISOLATED and sends nothing (so a failed isolation cannot put a broadcast
or multicast datagram on the LAN). With "control": true (the Landlock-only
run, in the host's namespace) it sends only to this machine's own unicast
addresses and to the Unix sockets.

It opens only socket families that are built into this kernel or already in
use (INET, INET6, UNIX, NETLINK, PACKET): a family whose module is not loaded
would be loaded by the attempt, on the owner's machine.

One line per probe: `<name> <result>`.
"""
import errno, json, os, socket, struct, subprocess, sys

cfg = json.load(open(sys.argv[1]))
control = cfg.get("control", False)


def err(e):
    return errno.errorcode.get(e.errno, str(e.errno))


def say(name, result):
    print(f"{name} {result}", flush=True)


def interfaces():
    names = []
    with open("/proc/net/dev") as f:
        for line in f.readlines()[2:]:
            names.append(line.split(":")[0].strip())
    return names


ifs = interfaces()
say("interfaces", ",".join(ifs))
say("netns", os.readlink("/proc/self/ns/net"))
say("userns", os.readlink("/proc/self/ns/user"))
say("uid", f"{os.getuid()} euid {os.geteuid()}")
for line in open("/proc/self/status"):
    if line.startswith(("CapInh", "CapPrm", "CapEff", "CapBnd", "CapAmb", "NoNewPrivs")):
        k, v = line.split(":")
        say(k, v.strip())

# Descriptors this process was handed (before it opens any socket). The
# listing's own directory descriptor is left out.
d = os.open("/proc/self/fd", os.O_RDONLY | os.O_DIRECTORY)
for n in sorted(int(x) for x in os.listdir(d)):
    if n == d:
        continue
    try:
        say(f"fd-{n}", os.readlink(f"/proc/self/fd/{n}"))
    except OSError as e:
        say(f"fd-{n}", err(e))
os.close(d)

if not control and ifs != ["lo"]:
    say("NOT-ISOLATED", "nothing sent")
    sys.exit(0)


def udp(name, family, addr, tag, opts=()):
    try:
        s = socket.socket(family, socket.SOCK_DGRAM)
        for level, opt, val in opts:
            s.setsockopt(level, opt, val)
        s.sendto(tag.encode(), addr)
        say(name, "sent")
    except OSError as e:
        say(name, err(e))


def tcp(name, family, addr):
    try:
        s = socket.socket(family, socket.SOCK_STREAM)
        s.settimeout(2)
        s.connect(addr)
        say(name, "CONNECTED")
    except OSError as e:
        say(name, err(e))


tag = cfg["tag"]
# Unicast to this machine's own addresses.
udp("udp4-loopback", socket.AF_INET, tuple(cfg["udp4_loopback"]), tag + "-udp4-loopback")
if cfg.get("udp6_loopback"):
    udp("udp6-loopback", socket.AF_INET6, tuple(cfg["udp6_loopback"]), tag + "-udp6-loopback")
if cfg.get("udp4_lan"):
    udp("udp4-lan", socket.AF_INET, tuple(cfg["udp4_lan"]), tag + "-udp4-lan")
tcp("tcp4-loopback", socket.AF_INET, tuple(cfg["tcp4_loopback"]))
if cfg.get("tcp4_lan"):
    tcp("tcp4-lan", socket.AF_INET, tuple(cfg["tcp4_lan"]))

if not control:
    # This machine's IPv6 link-local address: every scope the command has.
    if cfg.get("udp6_linklocal"):
        host, port = cfg["udp6_linklocal"]
        for idx, name in socket.if_nameindex():
            udp(f"udp6-linklocal-via-{name}", socket.AF_INET6, (host, port, 0, idx),
                tag + "-udp6-linklocal")
    # Broadcast and multicast (only `lo` is here: checked above).
    port = cfg["any4_port"]
    udp("udp4-broadcast", socket.AF_INET, ("255.255.255.255", port), tag + "-broadcast",
        [(socket.SOL_SOCKET, socket.SO_BROADCAST, 1)])
    udp("udp4-multicast", socket.AF_INET, (cfg["mcast4"], port), tag + "-multicast")
    udp("udp4-multicast-if-lo", socket.AF_INET, (cfg["mcast4"], port), tag + "-multicast-lo",
        [(socket.IPPROTO_IP, socket.IP_MULTICAST_IF, socket.inet_aton("127.0.0.1")),
         (socket.IPPROTO_IP, socket.IP_MULTICAST_LOOP, 1)])
    udp("udp6-multicast-all-nodes", socket.AF_INET6, ("ff02::1", cfg["any6_port"], 0, 1),
        tag + "-multicast6")
    # SO_BINDTODEVICE to an interface of the host.
    for dev in cfg["host_ifs"]:
        try:
            s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
            s.setsockopt(socket.SOL_SOCKET, socket.SO_BINDTODEVICE, dev.encode() + b"\0")
            s.sendto((tag + "-bindtodevice").encode(), tuple(cfg["udp4_loopback"]))
            say(f"bindtodevice-{dev}", "sent")
        except OSError as e:
            say(f"bindtodevice-{dev}", err(e))
    # Ping sockets and raw sockets.
    for name, fam, typ, proto in [
        ("icmp-ping-socket", socket.AF_INET, socket.SOCK_DGRAM, socket.IPPROTO_ICMP),
        ("icmp6-ping-socket", socket.AF_INET6, socket.SOCK_DGRAM, socket.IPPROTO_ICMPV6),
        ("raw4-socket", socket.AF_INET, socket.SOCK_RAW, socket.IPPROTO_UDP),
        ("raw6-socket", socket.AF_INET6, socket.SOCK_RAW, socket.IPPROTO_UDP),
        ("packet-socket", socket.AF_PACKET, socket.SOCK_RAW, 0),
    ]:
        try:
            s = socket.socket(fam, typ, proto)
            if name == "icmp-ping-socket" and cfg.get("udp4_lan"):
                # An echo request to this machine's own LAN address.
                s.settimeout(1)
                s.sendto(b"\x08\x00\xf7\xff\x00\x00\x00\x00", (cfg["udp4_lan"][0], 0))
                say(name, "created, echo sent")
            else:
                say(name, "created")
        except OSError as e:
            say(name, err(e))
    # rtnetlink: the links this namespace has, as the kernel lists them.
    try:
        s = socket.socket(socket.AF_NETLINK, socket.SOCK_RAW, 0)  # NETLINK_ROUTE
        s.settimeout(2)
        # RTM_GETLINK (18), NLM_F_REQUEST | NLM_F_DUMP (0x301), ifinfomsg.
        s.send(struct.pack("=IHHII", 32, 18, 0x301, 1, 0) + struct.pack("=BxHiII", 0, 0, 0, 0, 0))
        names, done = [], False
        while not done:
            data = s.recv(65536)
            off = 0
            while off < len(data):
                ln, typ, _, _, _ = struct.unpack_from("=IHHII", data, off)
                if typ in (2, 3):  # NLMSG_ERROR, NLMSG_DONE
                    done = True
                    break
                if typ == 16:  # RTM_NEWLINK
                    a = off + 16 + 16
                    while a < off + ln:
                        alen, atype = struct.unpack_from("=HH", data, a)
                        if atype == 3:  # IFLA_IFNAME
                            names.append(data[a + 4:a + alen].rstrip(b"\0").decode())
                        a += (alen + 3) & ~3
                off += (ln + 3) & ~3
        say("netlink-links", ",".join(names))
    except OSError as e:
        say("netlink-links", err(e))
    # A user and network namespace of the command's own, inside: what it holds.
    try:
        out = subprocess.run(
            ["unshare", "-Urn", "sh", "-c",
             "echo ifs=$(sed -n 's/^ *\\([^:]*\\):.*/\\1/p' /proc/net/dev | tr '\\n' ','); "
             "grep CapEff /proc/self/status | tr -s '\\t ' ' '; "
             "(printf '%s' \"$1\" >/dev/udp/$2/$3) 2>/dev/null && echo udp-sent || echo udp-unsent",
             "sh", tag + "-nested", cfg["udp4_loopback"][0], str(cfg["udp4_loopback"][1])],
            capture_output=True, text=True, timeout=10)
        say("nested-namespace", " ".join((out.stdout + out.stderr).split()))
    except (OSError, subprocess.SubprocessError) as e:
        say("nested-namespace", repr(e))


# Unix sockets of the host.
def unix(name, typ, addr, tag):
    try:
        s = socket.socket(socket.AF_UNIX, typ)
        s.connect(addr)
        s.send(tag.encode())
        say(name, "CONNECTED")
    except OSError as e:
        say(name, err(e))


unix("abstract-stream", socket.SOCK_STREAM, "\0" + cfg["abstract_stream"], tag + "-abstract-stream")
unix("abstract-dgram", socket.SOCK_DGRAM, "\0" + cfg["abstract_dgram"], tag + "-abstract-dgram")
unix("pathname-outside-the-grants", socket.SOCK_STREAM, cfg["path_outside"], tag + "-path-outside")
unix("pathname-in-the-session-folder", socket.SOCK_STREAM, cfg["path_inside"], tag + "-path-inside")
