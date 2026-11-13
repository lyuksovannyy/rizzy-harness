#!/bin/sh
# T.8d review: run the proofs that need the workspace's own crates. They are
# not part of the test suite: the file is copied into crates/cli/tests/ for
# the run and the copy is removed again, whatever happens.
#
#   run_rust_proofs.sh [test-name-filter]
#
# One cargo command, the worktree's own target/ (CARGO_TARGET_DIR is used
# as the caller set it). Temporary folders go under the target directory,
# never /tmp.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../../../.." && pwd)
src=review_t8d_proofs.rs
copy="$root/crates/cli/tests/$src"
trap 'rm -f "$copy"' EXIT INT TERM
cp "$here/$src" "$copy"
cd "$root"
TMPDIR="${CARGO_TARGET_DIR:-$root/target}/review_t8d/tmp"
mkdir -p "$TMPDIR"
export TMPDIR
HARNESS_REQUIRE_BWRAP=1 cargo test -p harness --test "${src%.rs}" -- --nocapture --test-threads=1 "$@"
