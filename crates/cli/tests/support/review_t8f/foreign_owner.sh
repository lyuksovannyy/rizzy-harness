#!/bin/sh
# T.8f review, re-check: a node run from a binary its user does NOT own
# (review_t8f_proofs.rs, `f_a_binary_this_user_does_not_own`), without root:
# a copy of the binary owned by a subordinate uid of this user (through
# newuidmap), mode 755. That is what an installed root-owned binary is to
# the node: not its file, and an owner that is not mapped inside
# bubblewrap's user namespace. (The first design could not link such a
# binary and fell back; there is no fallback now.)
#
# Needs: bubblewrap, util-linux unshare, newuidmap and an /etc/subuid entry
# for this user, CARGO_TARGET_DIR, and TMPDIR outside /tmp.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
tests=$(cd "$here/../.." && pwd)
copy="$tests/review_t8f_proofs.rs"
: "${TMPDIR:?set TMPDIR}"
: "${CARGO_TARGET_DIR:?set CARGO_TARGET_DIR}"
foreign="$TMPDIR/review-t8f-foreign-$$"
cleanup() {
    rm -f "$copy"
    rm -rf "$foreign"
}
trap cleanup EXIT INT TERM
cp "$here/review_t8f_proofs.rs" "$copy"
cd "$tests/../../.."
HARNESS_REQUIRE_BWRAP=1 cargo test -p harness --test review_t8f_proofs --no-run
mkdir -p "$foreign"
subuid=$(awk -F: -v u="$(id -un)" '$1 == u { print $2; exit }' /etc/subuid)
unshare -U --map-root-user --map-users="1:$subuid:1" sh -c \
    "cp '$CARGO_TARGET_DIR/debug/rizzy-harness' '$foreign/rizzy-harness' && chown 1 '$foreign/rizzy-harness'"
ls -ln "$foreign/rizzy-harness"
HARNESS_REQUIRE_BWRAP=1 REVIEW_T8F_FOREIGN_EXE="$foreign/rizzy-harness" \
    cargo test -p harness --test review_t8f_proofs -- --nocapture --test-threads=1 f_a_binary
