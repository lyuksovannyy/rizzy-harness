#!/bin/sh
# T.8b review: run review_t8b_proofs.rs (the proofs that need the node's own
# code). It is not part of the test suite: this copies it to crates/cli/tests/
# for the run and removes the copy again, whatever happens.
#
#   CARGO_TARGET_DIR=<target> run_rust_proofs.sh [test-name-filter]
#
# Needs bubblewrap and python3. One cargo command. Scratch files go under
# $CARGO_TARGET_DIR/review_t8b (default: <worktree>/target/review_t8b), and
# TMPDIR is pointed there too, so nothing is written to /tmp.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
tests=$(cd "$here/../.." && pwd)
root=$(cd "$tests/../../.." && pwd)
copy="$tests/review_t8b_proofs.rs"
trap 'rm -f "$copy"' EXIT INT TERM
cp "$here/review_t8b_proofs.rs" "$copy"
cd "$root"
CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$root/target}
export CARGO_TARGET_DIR
mkdir -p "$CARGO_TARGET_DIR/review_t8b/tmp"
TMPDIR="$CARGO_TARGET_DIR/review_t8b/tmp" HARNESS_REQUIRE_BWRAP=1 \
    cargo test -p harness --test review_t8b_proofs -- \
    --nocapture --test-threads=1 "$@"
