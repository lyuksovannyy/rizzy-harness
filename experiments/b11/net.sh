#!/usr/bin/env bash
# Test 5, network (test D14 of the nodes design). Host listeners: TCP and
# UDP on 127.0.0.1 and on the LAN address. Each option runs netprobe.py;
# "off" variants must reach nothing; "on" variants also do one DNS lookup
# and one HTTPS HEAD to example.com.
source "$(dirname "$0")/lib.sh"
setup_dirs
LAN=${LAN:-$(ip -4 -o addr show scope global | awk '{print $4}' | cut -d/ -f1 | head -1)}
TL=18731 TN=18732 UL=18733 UN=18734
cp "$HERE/netprobe.py" "$SA/.netprobe.py"
RECV=$B11/udp-received.log; : >"$RECV"
python3 - "$LAN" $TL $TN $UL $UN "$RECV" <<'EOF' &
import socket, sys, threading
lan, tl, tn, ul, un, log = sys.argv[1], *map(int, sys.argv[2:6]), sys.argv[6]
def tcp(h, p):
    s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1); s.bind((h, p)); s.listen()
    while True:
        c, a = s.accept(); c.sendall(b"hello-from-host-" + h.encode()); c.close()
def udp(h, p):
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM); s.bind((h, p))
    while True:
        d, a = s.recvfrom(100)
        with open(log, "a") as f: f.write(f"udp {h}:{p} got {d.decode()} from {a[0]}\n")
for f, h, p in ((tcp, "127.0.0.1", tl), (tcp, lan, tn), (udp, "127.0.0.1", ul), (udp, lan, un)):
    threading.Thread(target=f, args=(h, p), daemon=True).start()
threading.Event().wait()
EOF
LPID=$!
trap 'kill $LPID; pm_rm rh-b11-n1 rh-b11-n2; rm -f "$SA/.netprobe.py"' EXIT
sleep 0.5
P=(python3 "$SA/.netprobe.py" "$LAN" $TL $TN $UL $UN)

echo "== LAN address $LAN; listeners on 127.0.0.1 and $LAN"
echo "== sandbox-exec --no-network (Landlock ABI 4 TCP rules)"
SBX_NET=off sbx "${P[*]} sbx-off"
echo "== bwrap --unshare-net"
bw "${P[*]} bw-off"
echo "== podman --network=none"
PM_NET=none pm_start rh-b11-n1
pm_exec rh-b11-n1 "${P[*]} pm-none"
echo "== podman default rootless network (pasta), the design's network = \"on\""
PM_NET=pasta pm_start rh-b11-n2
pm_exec rh-b11-n2 "${P[*]} pm-pasta internet"
echo "   pasta's command line: $(pgrep -af '^/usr/bin/pasta' | grep -o -- '--config-net.*' | head -c 300)"
echo "== podman pasta with --network=pasta:--no-map-gw (host mapping off)"
mapfile -d '' a < <(PM_NET='pasta:--no-map-gw' pm_args yes)
podman run --rm "${a[@]}" "$IMG" bash -c "${P[*]} pm-pasta-nomapgw"
echo "== sandbox-exec with network on, for comparison"
sbx "${P[*]} sbx-on internet"
sleep 0.5
echo "== UDP datagrams the host received (tag -> destination, source address)"
sed 's/^/   /' "$RECV"
