#!/bin/sh
# T.8c review: where does an ABSOLUTE symbolic link lead while bubblewrap
# builds the view? One bind whose destination goes through a link the
# (already bound, writable) outer folder holds. Stand-in folders only.
#   bwrap_setup_root.sh <scratch-dir>
set -u
scratch=${1:?scratch dir}
mkdir -p "$scratch" || exit 1
R=$(mktemp -d "$scratch/setuproot.XXXXXX") || exit 1
[ -n "$R" ] && [ -d "$R" ] || exit 1
W=$R/home/ws
mkdir -p "$W/x/e" "$R/hostdir" "$R/home/secret/e"
echo INNER > "$W/x/e/inner"
echo SECRET > "$R/home/secret/e/key"
echo "bubblewrap: $(bwrap --version)"
BW="bwrap --unshare-pid --proc /proc --dev /dev --dir /tmp --ro-bind /usr /usr --symlink usr/bin /bin --symlink usr/lib /lib --symlink usr/lib /lib64 --ro-bind /etc /etc"
n=0
for target in "$R/hostdir" / /oldroot /newroot /newroot/usr "/oldroot$R/hostdir" /tmp /tmp/newroot /tmp/oldroot "/tmp/oldroot$R/hostdir" /proc/self/cwd /proc/self/root; do
  n=$((n+1))
  ( exec 8<"$W" 9<"$W/x/e"
    mv "$W/x" "$W/real_x"; ln -s "$target" "$W/x"
    out=$($BW --bind-fd 8 "$W" --bind-fd 9 "$W/x/e" --remount-ro / -- /bin/sh -c "echo RAN" 2>&1)
    echo "x -> $target : exit=$? : $out" | sed "s|$R|<R>|g" )
  mv "$W/x" "$R/trash.$n"; mv "$W/real_x" "$W/x"
done
echo "host: <R>/hostdir holds: $(ls "$R/hostdir" | tr '\n' ' ')"
echo "scratch: $R"
