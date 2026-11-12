#!/usr/bin/env bash
# Tests 7 and 8: what the machine must provide, what breaks under the
# README's systemd hardening and without a user runtime directory, what
# happens nested inside a rootless container (as the published image
# runs), and whether sandbox-exec's Landlock still works inside Podman and
# inside bwrap.
# A second Podman store ($B11/altroot) holds only the Debian base image
# (copied from the main store with podman save/load, nothing downloaded),
# so the odd configurations below never touch the main store.
source "$(dirname "$0")/lib.sh"
setup_dirs
ALT=$B11/altroot ALTRUN=$B11/altrun
BASEIMG=docker.io/library/debian:trixie-slim
trap 'pm_rm rh-b11-m1; podman --root "$ALT" --runroot "$ALTRUN" rm -af -t 0 >/dev/null 2>&1' EXIT
if ! podman --root "$ALT" --runroot "$ALTRUN" image exists "$BASEIMG" 2>/dev/null; then
  podman save -o "$B11/debian.tar" "$BASEIMG" >/dev/null
  podman --root "$ALT" --runroot "$ALTRUN" load -q -i "$B11/debian.tar" >/dev/null
fi
apm() { podman --root "$ALT" --runroot "$ALTRUN" "$@"; }

echo "== what this machine provides"
echo "   kernel $(uname -r); LSMs: $(cat /sys/kernel/security/lsm)"
echo "   user.max_user_namespaces=$(cat /proc/sys/user/max_user_namespaces)"
echo "   newuidmap: $(getcap /usr/bin/newuidmap 2>/dev/null) ; subuid: $(grep "^$USER:" /etc/subuid)"
echo "   bwrap: $(stat -c '%A %U' /usr/bin/bwrap) (not setuid)"
echo "   XDG_RUNTIME_DIR=$XDG_RUNTIME_DIR; linger: $(loginctl show-user "$USER" -p Linger --value 2>/dev/null)"
echo "   cgroup controllers delegated to the user manager: $(cat /sys/fs/cgroup/user.slice/user-$(id -u).slice/user@$(id -u).service/cgroup.controllers 2>/dev/null)"

echo "== Podman without XDG_RUNTIME_DIR and without a user bus (a systemd system unit, nologin user, no linger)"
env -u XDG_RUNTIME_DIR -u DBUS_SESSION_BUS_ADDRESS podman --root "$ALT" run --rm --pull=never \
  --network=none "$BASEIMG" true 2>&1 | sed 's/^/   /' ; echo "   rc=${PIPESTATUS[0]}"
env -u XDG_RUNTIME_DIR -u DBUS_SESSION_BUS_ADDRESS podman --root "$ALT" run --rm --pull=never \
  --network=none --memory=1g --pids-limit=100 "$BASEIMG" true 2>&1 | sed 's/^/   with --memory --pids-limit: /'; echo "   rc=${PIPESTATUS[0]}"
env -u XDG_RUNTIME_DIR -u DBUS_SESSION_BUS_ADDRESS podman --root "$ALT" run --rm --pull=never \
  "$BASEIMG" true 2>&1 | sed 's/^/   with the default network (pasta): /'; echo "   rc=${PIPESTATUS[0]}"
echo "   where the run state went: $(ls -d /tmp/podman-run-$(id -u) /tmp/storage-run-$(id -u) /tmp/containers-user-$(id -u) 2>/dev/null | tr '\n' ' ')"
echo "== bwrap without XDG_RUNTIME_DIR"
env -u XDG_RUNTIME_DIR -u DBUS_SESSION_BUS_ADDRESS bash -c "source $HERE/lib.sh; bw true" && echo "   works"

echo "== newuidmap under no_new_privs: file capabilities are not granted, so a new rootless user namespace cannot be mapped"
unshare -U sleep 10 & p=$!; sleep 0.3
setpriv --no-new-privs newuidmap $p 0 "$(id -u)" 1 1 100000 65536 2>&1 | sed 's/^/   with no_new_privs: /'; echo "   rc=${PIPESTATUS[0]}"
unshare -U sleep 10 & q=$!; sleep 0.3
newuidmap $q 0 "$(id -u)" 1 1 100000 65536 2>&1; echo "   without: rc=$?"
kill $p $q
echo "== podman under no_new_privs via setpriv: succeeds only because it joins the user's existing"
echo "   pause process (made earlier without no_new_privs); a fresh node user has none"
setpriv --no-new-privs podman --root "$ALT" --runroot "$ALTRUN" run --rm --pull=never --network=none \
  "$BASEIMG" sh -c 'cat /proc/self/uid_map' 2>&1 | sed 's/^/   podman: /'; echo "   rc=${PIPESTATUS[0]}"
setpriv --no-new-privs bash -c "source $HERE/lib.sh; bw 'echo bwrap works'" 2>&1 | sed 's/^/   /'
setpriv --no-new-privs bash -c "source $HERE/lib.sh; sbx 'echo sandbox-exec works'" 2>&1 | sed 's/^/   /'

echo "== the README unit's hardening as a transient user service (systemd-run --user; not the system manager)"
# Podman is not run this way any more: the user manager applies
# ProtectSystem through a user namespace of its own, so podman inside it
# cannot see its pause process, deletes the user's pause.pid and then
# fails in newuidmap ("Could not set caps"); the result mixes two causes
# and disturbs the user's Podman state. Recorded in results/machine.txt of
# the first run.
props=(-p NoNewPrivileges=yes -p ProtectSystem=strict -p "ReadWritePaths=$HOME $XDG_RUNTIME_DIR")
systemd-run --user --quiet --wait --pipe --collect "${props[@]}" \
  bash -c "source $HERE/lib.sh; bw 'echo bwrap ran'" 2>&1 | sed 's/^/   /'
systemd-run --user --quiet --wait --pipe --collect "${props[@]}" \
  bash -c "source $HERE/lib.sh; sbx 'echo sandbox-exec ran'" 2>&1 | sed 's/^/   /'

echo "== nested: inside a rootless Podman container like the published image (no flags beyond the default)"
# The harness binary needs glibc 2.43 (Arch); Debian 13 has 2.41. It is
# started with the host's loader and libraries mounted at /hostlib; the
# command it runs after Landlock is the container's own bash.
HL=(--mount type=bind,src=/usr/lib,dst=/hostlib,ro=true --mount "type=bind,src=$SBX_BIN,dst=/rh,ro=true")
SBXIN='/hostlib/ld-linux-x86-64.so.2 --library-path /hostlib /rh sandbox-exec --status --rw system /tmp --ro system /usr --ro system /etc --ro system /lib --ro system /bin --ro system /proc -- '
podman run --rm --pull=never --network=none --user 1000:1000 --userns=keep-id "${HL[@]}" \
  --mount "type=bind,src=$B11/debian.tar,dst=/debian.tar,ro=true" --env HOME=/tmp "$IMG" bash -c "
  echo \"bwrap --unshare-all: \$(bwrap --unshare-all --ro-bind /usr /usr --symlink usr/bin /bin --symlink usr/lib64 /lib64 --symlink usr/lib /lib --proc /proc --dev /dev true 2>&1 && echo works)\"
  echo \"bwrap without a new /proc: \$(bwrap --unshare-user --unshare-pid --unshare-net --ro-bind /usr /usr --symlink usr/bin /bin --symlink usr/lib64 /lib64 --symlink usr/lib /lib --dev /dev true 2>&1 && echo works)\"
  echo \"podman load (vfs): \$(podman --storage-driver=vfs load -q -i /debian.tar 2>&1 | tail -2 | tr '\n' ' ' | head -c 300)\"
  S='--storage-driver=vfs --storage-opt vfs.ignore_chown_errors=true'
  podman \$S load -q -i /debian.tar >/dev/null 2>&1
  echo \"podman (vfs, ignore_chown_errors): \$(podman \$S run --rm --pull=never --network=none docker.io/library/debian:trixie-slim echo works 2>&1 | tail -2 | tr '\n' ' ')\"
  echo \"sandbox-exec (Landlock): \$($SBXIN bash -c 'ls /tmp >/dev/null && echo granted-ok; cat /root/x 2>&1 | head -c 50; ls /home 2>&1 | head -c 60' 2>&1 | tr '\n' ' ')\"
" 2>&1 | sed 's/^/   /'
echo "== nested, with the flags that are known to let bwrap run in a container (unmask /proc, no seccomp)"
podman run --rm --pull=never --network=none --user 1000:1000 --userns=keep-id \
  --security-opt unmask=ALL --security-opt seccomp=unconfined "$IMG" bash -c "
  echo \"bwrap --unshare-all: \$(bwrap --unshare-all --ro-bind /usr /usr --symlink usr/bin /bin --symlink usr/lib64 /lib64 --symlink usr/lib /lib --proc /proc --dev /dev true 2>&1 && echo works)\"
" 2>&1 | sed 's/^/   /'
podman run --rm --pull=never --network=none --user 1000:1000 --userns=keep-id \
  --security-opt unmask=ALL "$IMG" bash -c "
  echo \"unmask only: bwrap --unshare-all: \$(bwrap --unshare-all --ro-bind /usr /usr --symlink usr/bin /bin --symlink usr/lib64 /lib64 --symlink usr/lib /lib --proc /proc --dev /dev true 2>&1 && echo works)\"
" 2>&1 | sed 's/^/   /'

echo "== test 8: sandbox-exec's Landlock inside the design's session container"
mapfile -d '' a < <(pm_args yes)
podman run --rm "${a[@]}" "${HL[@]}" "$IMG" bash -c "
  $SBXIN bash -c 'echo inside-landlock; ls /tmp >/dev/null && echo tmp-granted-ok; ls $SA 2>&1 | head -c 80; echo; touch /tmp/ok && echo write-granted-ok'
" 2>&1 | sed 's/^/   /'
echo "== test 8: sandbox-exec's Landlock inside bwrap"
mapfile -d '' a < <(bw_args)
"${a[@]}" --ro-bind "$SBX_BIN" /rh /rh sandbox-exec --status --rw system /tmp --ro system /usr \
  --ro system /etc --ro system /proc --ro system /dev/null -- /usr/bin/bash -c \
  "ls /tmp >/dev/null && echo tmp-granted-ok; ls $SA 2>&1 | head -c 80" 2>&1 | sed 's/^/   /'
