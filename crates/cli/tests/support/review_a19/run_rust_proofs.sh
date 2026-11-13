#!/bin/sh
# A.19 review: run the proofs that need the workspace's own crates. They are
# not part of the test suite: each is copied into its crate's tests/ for the
# run and the copy is removed again, whatever happens.
#
#   run_rust_proofs.sh handshake|reason|loop [test-name-filter]
#
# One cargo command. Uses CARGO_TARGET_DIR as the caller set it.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../../../.." && pwd)
case "${1:-}" in
handshake) src=review_a19_handshake.rs; crate=master; pkg=harness-master ;;
reason) src=review_a19_node_reason.rs; crate=cli; pkg=harness ;;
loop) src=review_a19_put_back_loop.rs; crate=master; pkg=harness-master ;;
*) echo "usage: $0 handshake|reason|loop [filter]" >&2; exit 2 ;;
esac
shift
copy="$root/crates/$crate/tests/$src"
trap 'rm -f "$copy"' EXIT INT TERM
cp "$here/$src" "$copy"
cd "$root"
# The proofs' temporary folders go under the target directory, not /tmp.
TMPDIR="${CARGO_TARGET_DIR:-$root/target}/review_a19/tmp"
mkdir -p "$TMPDIR"
export TMPDIR
cargo test -p "$pkg" --test "${src%.rs}" -- --nocapture --test-threads=1 "$@"
