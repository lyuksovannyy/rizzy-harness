#!/bin/bash
# T.8a review, T8A-1 on a real session bus: one StartTransientUnit call from
# inside a contained command (what `systemd-run --user` sends; systemd's own
# client refuses to talk from a PID namespace, `gdbus` and libdbus do not),
# then the node's kill (SIGKILL to bubblewrap).
#
#   session_bus.sh                      the wrapper as a kernel with Landlock
#                                       ABI 4 enforces it (landlock_abi4.py)
#   session_bus.sh <rizzy-harness>      the real wrapper on this kernel
#
# Needs bwrap, gdbus, python3 and a systemd user session. It starts one
# transient unit `rizzy-review-t8a-<pid>` and stops it at the end; scratch
# files go to a folder under $TMPDIR that it removes.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
D=$(mktemp -d); trap 'rm -rf "$D"' EXIT
WS=$D/ws; TMP=$D/tmp; mkdir -p "$WS" "$TMP"; echo SECRET > "$D/secret"
U=rizzy-review-t8a-$$; N=$((975200 + $$ % 90)); UID_=$(id -u)
ident() { stat -c '%d:%i' "$1"; }
if [ $# -ge 1 ]; then wrapper=("$1" sandbox-exec); else wrapper=(python3 -I "$HERE/landlock_abi4.py"); fi
# The node's command line (crates/tools/src/contain.rs, sandbox.rs `wrapper`).
cmd=(bwrap --die-with-parent --new-session --unshare-pid --dev-bind / / --proc /proc --chdir "$WS" --
     "${wrapper[@]}"
     --rw "$(ident "$WS"):the session's folder" "$WS" --rw "$(ident "$TMP"):the session's temp folder" "$TMP")
for d in /dev/null /dev/zero /dev/full /dev/random /dev/urandom; do cmd+=(--rw system $d); done
for d in /usr /bin /sbin /lib /lib32 /lib64 /libx32 /etc /opt /proc; do [ -e $d ] && cmd+=(--ro system $d); done
script="export DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$UID_/bus
cat $D/secret 2>&1
gdbus call --session --dest org.freedesktop.systemd1 --object-path /org/freedesktop/systemd1 \
  --method org.freedesktop.systemd1.Manager.StartTransientUnit $U.service fail \
  \"[('ExecStart', <[('/bin/sh', ['/bin/sh', '-c', 'cat $D/secret > $D/leak; exec sleep $N'], false)]>)]\" '[]' 2>&1
echo StartTransientUnit rc=\$?
setsid sleep $((N + 1)) & sleep $((N + 2))"
(cd "$WS" && exec env -i PATH="$PATH" HOME="$HOME" TMPDIR="$TMP" setsid "${cmd[@]}" -- bash -c "$script") 2>&1 | grep -v bashrc &
sleep 2
BW=$(pgrep -n -f "^bwrap .*sleep $((N + 2))")
inside() { echo "$(pgrep -f "^sleep $((N + 1))\$" | wc -l) setsid child, $(pgrep -f "^sleep $((N + 2))\$" | wc -l) foreground"; }
echo "before the kill: inside the namespace: $(inside); the unit: $(systemctl --user is-active $U)"
kill -KILL -- "-$BW"
sleep 2
echo "after the kill:  inside the namespace: $(inside); the unit: $(systemctl --user is-active $U)"
P=$(systemctl --user show -p MainPID --value $U)
if [ "${P:-0}" != 0 ]; then
  echo "  the unit's process $P: $(tr '\0' ' ' < /proc/$P/cmdline)"
  echo "  pid namespace $(readlink /proc/$P/ns/pid), the host's is $(readlink /proc/self/ns/pid); $(grep NoNewPrivs /proc/$P/status | tr '\t' ' ')"
  echo "  it copied a file from outside the session folder: $(cat "$D/leak" 2>&1)"
fi
systemctl --user stop $U 2>/dev/null; echo "cleaned up: the unit is $(systemctl --user is-active $U)"
