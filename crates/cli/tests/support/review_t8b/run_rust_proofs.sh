#!/bin/sh
# T.8b review: run the proofs that need the node's own code. They are not
# part of the test suite: this copies the file to crates/cli/tests/ for the
# run and removes the copy again, whatever happens.
#
#   CARGO_TARGET_DIR=<target> run_rust_proofs.sh [test-name-filter]
#       review_t8b_proofs.rs, the first review (its two finding_* proofs
#       fail since the fixes: that is their purpose)
#   CARGO_TARGET_DIR=<target> run_rust_proofs.sh recheck [test-name-filter]
#       recheck_t8b_proofs.rs, the re-check
#
# Needs bubblewrap, python3 and util-linux's unshare. One cargo command.
# Scratch files go under $CARGO_TARGET_DIR/review_t8b (default:
# <worktree>/target/review_t8b), and TMPDIR is pointed there too, so nothing
# is written to /tmp.
set -eu
name=review_t8b_proofs
if [ "${1:-}" = recheck ]; then
    name=recheck_t8b_proofs
    shift
fi
here=$(cd "$(dirname "$0")" && pwd)
tests=$(cd "$here/../.." && pwd)
root=$(cd "$tests/../../.." && pwd)
copy="$tests/$name.rs"
trap 'rm -f "$copy"' EXIT INT TERM
cp "$here/$name.rs" "$copy"
cd "$root"
CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$root/target}
export CARGO_TARGET_DIR
mkdir -p "$CARGO_TARGET_DIR/review_t8b/tmp"
TMPDIR="$CARGO_TARGET_DIR/review_t8b/tmp" HARNESS_REQUIRE_BWRAP=1 \
    cargo test -p harness --test "$name" -- \
    --nocapture --test-threads=1 "$@"
