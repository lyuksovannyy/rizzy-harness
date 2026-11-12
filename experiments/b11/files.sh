#!/usr/bin/env bash
# Test 4, files: what a command can read outside its session folder, who
# owns what it writes, what an outward symlink gives, and whether a mount
# source that is a symlink (or is swapped after the check) is followed.
# This machine has no ~/.ssh; $SECRET (0700, under the same home) stands in.
source "$(dirname "$0")/lib.sh"
setup_dirs
trap 'pm_rm rh-b11-f1 rh-b11-f2 rh-b11-f3 rh-b11-f4; rm -rf "$SA"/own-* "$SA"/link-*' EXIT
pm_start rh-b11-f1 yes               # keep-id (the design)
pm_start rh-b11-f2 no                # no keep-id, root in the container
mapfile -d '' a < <(pm_args no)
podman run -d --name rh-b11-f3 --user 1000:1000 "${a[@]}" "$IMG" sleep infinity >/dev/null
                                     # no keep-id, uid 1000 in the container

ln -sfn "$SECRET" "$SA/link-dir"
ln -sfn /etc/passwd "$SA/link-passwd"
HOSTPW=$(sha256sum /etc/passwd | cut -c1-12)
NODEPID=$$
probe=$(cat <<EOF
pw=\$(sha256sum /etc/passwd 2>/dev/null | cut -c1-12)
[ "\$pw" = $HOSTPW ] && pw=host || pw="not-host(\${pw:-unreadable})"
echo "passwd=\$pw"
echo "secret=\$(cat $SECRET/id_test 2>&1 | head -c 60)"
echo "sibling=\$(cat $SB/sibling.txt 2>&1 | head -c 60)"
echo "home-ls=\$(ls /home 2>&1 | head -c 60 | tr '\n' ' ')"
echo "proc-pids=\$(ls /proc | grep -c '^[0-9]')"
echo "host-pid1-cmdline=\$(tr '\0' ' ' < /proc/1/cmdline 2>&1 | head -c 50)"
echo "node-environ=\$(head -c 40 /proc/$NODEPID/environ 2>&1 | tr '\0' ' ')"
echo "link-dir=\$(cat $SA/link-dir/id_test 2>&1 | head -c 60)"
echo "link-passwd=\$(sha256sum $SA/link-passwd 2>&1 | cut -c1-12)"
touch $SA/own-\$1 && mkdir -p $SA/own-\$1.d && echo "wrote=ok"
EOF
)
run() { # run <option> <tag>
  case $1 in
    sbx) sbx "set -- $2; $probe" ;;
    bw) bw "set -- $2; $probe" ;;
    pm1) podman exec --workdir "$SA" rh-b11-f1 bash -c "set -- $2; $probe" ;;
    pm2) podman exec --workdir "$SA" rh-b11-f2 bash -c "set -- $2; $probe" ;;
    pm3) podman exec --workdir "$SA" rh-b11-f3 bash -c "set -- $2; $probe" ;;
  esac
}
for o in sbx bw pm1 pm2 pm3; do
  echo "== $o  (pm1 = keep-id, pm2 = root in container without keep-id, pm3 = uid 1000 without keep-id)"
  run $o $o 2>&1 | sed 's/^/   /'
  printf '   on the host: %s\n' "$(stat -c '%n owner=%u:%g mode=%a' "$SA/own-$o" "$SA/own-$o.d" 2>&1 | sed "s|$SA/||" | tr '\n' ' ')"
  rm -rf "$SA/own-$o" "$SA/own-$o.d" 2>/dev/null || echo "   the host user cannot remove them: $(rm -rf "$SA/own-$o" 2>&1 | head -1)"
done
# pm3's files belong to a subordinate uid: remove them through podman
podman unshare rm -rf "$SA/own-pm3" "$SA/own-pm3.d" 2>/dev/null

echo "== a mount source that is a symlink (S-2): src=link -> secret folder"
ln -sfn "$SECRET" "$B11/swap"
podman run --rm --mount "type=bind,src=$B11/swap,dst=/mnt" --pull=never --network=none "$IMG" \
  sh -c 'echo "podman: /mnt holds: $(ls /mnt)"' 2>&1
bwrap --ro-bind /usr /usr --symlink usr/bin /bin --symlink usr/lib /lib64 --bind "$B11/swap" /mnt \
  ls /mnt 2>&1 | sed 's/^/bwrap: \/mnt holds: /'

echo "== the swap race closed by a descriptor: open the folder, swap it, mount the descriptor"
mkdir -p "$B11/race"; rm -rf "$B11/race/"*
mkdir "$B11/race/dir"; echo checked > "$B11/race/dir/marker"
exec 9<"$B11/race/dir"
mv "$B11/race/dir" "$B11/race/dir.moved"; mkdir "$B11/race/dir"; echo swapped > "$B11/race/dir/marker"
podman run --rm --mount "type=bind,src=/proc/$$/fd/9,dst=/mnt" --pull=never --network=none "$IMG" \
  cat /mnt/marker 2>&1 | sed 's/^/podman src=\/proc\/<node>\/fd\/9 mounts the folder marked: /'
bwrap --ro-bind /usr /usr --symlink usr/bin /bin --symlink usr/lib /lib64 --bind-fd 9 /mnt \
  cat /mnt/marker 2>&1 | sed 's/^/bwrap --bind-fd 9 mounts the folder marked: /'
exec 9<&-
echo "== what podman inspect shows for the fd mount (does it record the resolved path?)"
exec 9<"$B11/race/dir.moved"
podman run -d --name rh-b11-f4 --mount "type=bind,src=/proc/$$/fd/9,dst=/mnt" --pull=never --network=none "$IMG" sleep 30 >/dev/null 2>&1
podman inspect -f '{{range .Mounts}}{{.Source}} -> {{.Destination}}{{end}}' rh-b11-f4 2>&1
exec 9<&-
