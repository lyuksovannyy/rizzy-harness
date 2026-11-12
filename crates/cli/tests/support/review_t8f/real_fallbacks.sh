#!/bin/sh
# T.8f review: the two fallbacks the suite cannot reach, made real without
# root (review_t8f_proofs.rs, `f_a_real_fallback`):
#
#   EPERM  a copy of the binary owned by ANOTHER user (a subordinate uid of
#          this user, through newuidmap), mode 755: what an installed
#          root-owned binary is to fs.protected_hardlinks.
#   EXDEV  the data directory on a tmpfs mounted in a private mount
#          namespace (seen by this run only), the binary on the disk.
#
# Needs: bubblewrap, util-linux unshare, newuidmap and an /etc/subuid entry
# for this user, CARGO_TARGET_DIR, and TMPDIR on the build's file system.
# Nothing is mounted or changed outside the run's own namespaces.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
tests=$(cd "$here/../.." && pwd)
copy="$tests/review_t8f_proofs.rs"
: "${TMPDIR:?set TMPDIR to a folder on the build's file system}"
: "${CARGO_TARGET_DIR:?set CARGO_TARGET_DIR}"
foreign="$TMPDIR/review-t8f-foreign-$$"
otherfs="$TMPDIR/review-t8f-otherfs-$$"
cleanup() {
    rm -f "$copy"
    rm -rf "$foreign"
    rmdir "$otherfs" 2>/dev/null || true
}
trap cleanup EXIT INT TERM
cp "$here/review_t8f_proofs.rs" "$copy"
cd "$tests/../../.."
run="cargo test -p harness --test review_t8f_proofs -- --nocapture --test-threads=1 f_a_real_fallback"

echo "fs.protected_hardlinks = $(cat /proc/sys/fs/protected_hardlinks)"
HARNESS_REQUIRE_BWRAP=1 cargo test -p harness --test review_t8f_proofs --no-run

echo "==== EPERM: a binary this user does not own ===="
mkdir -p "$foreign"
subuid=$(awk -F: -v u="$(id -un)" '$1 == u { print $2; exit }' /etc/subuid)
unshare -U --map-root-user --map-users="1:$subuid:1" sh -c \
    "cp '$CARGO_TARGET_DIR/debug/rizzy-harness' '$foreign/rizzy-harness' && chown 1 '$foreign/rizzy-harness'"
ls -ln "$foreign/rizzy-harness"
HARNESS_REQUIRE_BWRAP=1 REVIEW_T8F_FOREIGN_EXE="$foreign/rizzy-harness" $run

echo "==== EXDEV: the data directory on another file system ===="
mkdir -p "$otherfs"
unshare -Urm sh -c "mount -t tmpfs tmpfs '$otherfs' && exec unshare -U \
    --map-user=$(id -u) --map-group=$(id -g) env HARNESS_REQUIRE_BWRAP=1 \
    REVIEW_T8F_DATA_PARENT='$otherfs' $run"
