#!/usr/bin/env bash
# Test 6, privilege: capabilities, no_new_privs, seccomp, mount, module
# load, ptrace inside the confinement, devices, nested user namespaces; and
# whether ordinary work (a shell, git, a compiler) still runs with
# --cap-drop=all, no-new-privileges and a read-only root.
source "$(dirname "$0")/lib.sh"
setup_dirs
trap 'pm_rm rh-b11-p1' EXIT
pm_start rh-b11-p1
cat >"$SA/.ptrace.py" <<'EOF'
import ctypes, os, subprocess, sys, time
libc = ctypes.CDLL(None, use_errno=True)
def attach(pid):
    r = libc.ptrace(16, pid, 0, 0)  # PTRACE_ATTACH
    e = ctypes.get_errno()
    if r == 0:
        libc.ptrace(17, pid, 0, 0)  # PTRACE_DETACH
        return "allowed"
    return "denied:" + os.strerror(e)
child = subprocess.Popen(["sleep", "30"])
time.sleep(0.2)
print("ptrace-own-child=" + attach(child.pid))
sib = int(sys.argv[1]) if len(sys.argv) > 1 else 0
if sib:
    print("ptrace-sibling=" + attach(sib))
child.kill()
EOF
printf '#include <stdio.h>\nint main(void){puts("compiled-and-ran");return 0;}\n' >"$SA/.hello.c"
probe=$(cat <<'EOF'
grep -E '^(CapEff|CapBnd|NoNewPrivs|Seccomp):' /proc/self/status | tr '\n' ' ' | tr -s '\t' ' '; echo
echo "uid=$(id -u) user=$(id -un 2>/dev/null)"
mkdir -p /tmp/m 2>/dev/null; echo "mount-tmpfs=$(mount -t tmpfs none /tmp/m 2>&1 | head -c 70 || true)$( mountpoint -q /tmp/m && echo MOUNTED)"
echo "insmod=$(insmod /dev/null 2>&1 | head -c 60)"
echo "unshare-user=$(unshare -U true 2>&1 && echo allowed | head -c 60)"
echo "unshare-user-mount=$(unshare -Urm true 2>&1 && echo allowed | head -c 60)"
sleep 30 & SIB=$!
( python3 "$PWD/.ptrace.py" $SIB 2>&1 ) | tr '\n' ' '; echo
kill $SIB
echo "dev-entries=$(ls /dev | wc -l) block-devices=$(ls /dev | grep -cE '^(sd|nvme|vd)')"
echo "write-/usr=$(touch /usr/b11 2>&1 | head -c 60)"
echo "write-/etc=$(touch /etc/b11 2>&1 | head -c 60)"
echo "write-/tmp=$(touch /tmp/b11 2>&1 && echo ok)"
echo "gcc=$(gcc -o "$PWD/.hello" "$PWD/.hello.c" 2>&1 | head -c 80 && "$PWD/.hello")"
cd "$PWD/work" && echo x >> f1.txt && echo "git-commit=$(git -c user.name=b11 -c user.email=b11@invalid commit -qam t 2>&1 | head -c 80 && echo ok)"
EOF
)
echo "== sandbox-exec"; sbx "$probe" 2>&1 | sed 's/^/   /'
echo "== bwrap (per command)"; bw "$probe" 2>&1 | sed 's/^/   /'
echo "== podman exec (--cap-drop=all, no-new-privileges, --read-only, keep-id)"
pm_exec rh-b11-p1 "$probe" 2>&1 | sed 's/^/   /'
echo "== podman run as root, no keep-id, same caps and flags"
mapfile -d '' a < <(pm_args no)
podman run --rm "${a[@]}" "$IMG" bash -c "$probe" 2>&1 | sed 's/^/   /'
rm -f "$SA/.ptrace.py" "$SA/.hello.c" "$SA/.hello"
