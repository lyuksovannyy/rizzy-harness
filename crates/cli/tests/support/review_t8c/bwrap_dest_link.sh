#!/bin/sh
# T.8c review: what bubblewrap itself does when the DESTINATION of a bind
# goes through a folder an earlier bind made attacker-writable (a grant
# inside another grant). No harness code: the real bwrap, the options
# `Contain::args` emits, stand-in folders made here.
#
#   bwrap_dest_link.sh <scratch-dir>      (a new folder is made inside it)
#
# Nothing is mounted outside bubblewrap's own namespace; nothing is left
# running (every bwrap here runs one short `sh -c` and exits).
set -u
scratch=${1:?scratch dir}
mkdir -p "$scratch" || exit 1
R=$(mktemp -d "$scratch/destlink.XXXXXX") || exit 1
[ -n "$R" ] && [ -d "$R" ] || exit 1
W=$R/home/ws
mkdir -p "$W/x/e" "$W/drop" "$W/drop2" "$R/home/secret" "$R/hostdir"
echo INNER > "$W/x/e/inner"
echo SECRET > "$R/home/secret/key"
echo "bubblewrap: $(bwrap --version)"
BW="bwrap --unshare-pid --proc /proc --dev /dev --dir /tmp --ro-bind /usr /usr --symlink usr/bin /bin --symlink usr/lib /lib --symlink usr/lib /lib64 --ro-bind /etc /etc"

echo "== A0 control (--bind-fd, nothing swapped)"
( exec 8<"$W" 9<"$W/x/e"
  $BW --bind-fd 8 "$W" --bind-fd 9 "$W/x/e" --remount-ro / -- /bin/sh -c "cat $W/x/e/inner" ) 2>&1
echo "exit=$?"

echo "== A1 --bind-fd, LAST component of the inner grant swapped for a link (-> ../drop) after the descriptor was opened"
( exec 8<"$W" 9<"$W/x/e"
  mv "$W/x/e" "$W/real_e"; ln -s ../drop "$W/x/e"
  $BW --bind-fd 8 "$W" --bind-fd 9 "$W/x/e" --remount-ro / -- /bin/sh -c "echo RAN; ls $W/drop" ) 2>&1
echo "exit=$?"
mv "$W/x/e" "$R/trash.a1"; mv "$W/real_e" "$W/x/e"

echo "== A2 --bind-fd, an INTERMEDIATE component (x) swapped for a link (-> drop2) after the descriptor was opened"
( exec 8<"$W" 9<"$W/x/e"
  mv "$W/x" "$W/real_x"; ln -s drop2 "$W/x"
  $BW --bind-fd 8 "$W" --bind-fd 9 "$W/x/e" --remount-ro / -- /bin/sh -c "echo RAN; echo in-view: \$(ls $W/drop2/e); grep -c ' $W/drop2/e ' /proc/self/mountinfo" ) 2>&1
echo "exit=$?"
echo "host after A2: drop2 holds: $(ls "$W/drop2")   (made on the host by bubblewrap)"
mv "$W/x" "$R/trash.a2"; mv "$W/real_x" "$W/x"

echo "== A3 --bind-fd, intermediate component swapped for an ABSOLUTE link into bubblewrap's /oldroot (the host), to a folder outside every grant"
( exec 8<"$W" 9<"$W/x/e"
  mv "$W/x" "$W/real_x"; ln -s "/oldroot$R/hostdir" "$W/x"
  $BW --bind-fd 8 "$W" --bind-fd 9 "$W/x/e" --remount-ro / -- /bin/sh -c "echo RAN" ) 2>&1
echo "exit=$?"
echo "host after A3: $R/hostdir (outside every grant) holds: $(ls -la "$R/hostdir" | tail -n +2 | awk '{print $1, $NF}' | tr '\n' ';')"
mv "$W/x" "$R/trash.a3"; mv "$W/real_x" "$W/x"

echo "== A4 --bind-fd, intermediate component swapped for a link to the view's /usr (dest = /usr/e): attacker content inside a system folder?"
( exec 8<"$W" 9<"$W/x/e"
  mv "$W/x" "$W/real_x"; ln -s /newroot/usr "$W/x"
  $BW --bind-fd 8 "$W" --bind-fd 9 "$W/x/e" --remount-ro / -- /bin/sh -c "echo RAN; ls -la /usr/e" ) 2>&1
echo "exit=$?"
mv "$W/x" "$R/trash.a4"; mv "$W/real_x" "$W/x"

echo "== B1 --bind by path (older bubblewrap's way), inner grant swapped for a RELATIVE link to an ancestor (home): source and destination both follow it"
# W = $R/home/ws, inner = W/x/e; ../../.. from W/x is $R/home on the host
# and <newroot>$R/home in the view (a folder bubblewrap made as W's parent).
( mv "$W/x/e" "$W/real_e"; ln -s ../.. "$W/x/e"
  $BW --bind "$W" "$W" --bind "$W/x/e" "$W/x/e" --remount-ro / -- /bin/sh -c "echo RAN; echo home-in-view: \$(ls $R/home); echo secret: \$(cat $R/home/secret/key 2>&1); touch $R/home/secret/WRITTEN-FROM-INSIDE 2>&1; grep -c ' $R/home ' /proc/self/mountinfo" ) 2>&1
echo "exit=$?"
echo "host after B1: secret holds: $(ls "$R/home/secret" | tr '\n' ' ')"
mv "$W/x/e" "$R/trash.b1"; mv "$W/real_e" "$W/x/e"
echo "scratch: $R"
