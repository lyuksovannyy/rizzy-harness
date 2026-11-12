#!/bin/sh
# T.8f review: run review_t8f_proofs.rs (the proofs that need the node's own
# code). It is not part of the test suite: this copies it to crates/cli/tests/
# for the run and removes the copy again, whatever happens.
#
#   run_rust_proofs.sh [test-name-filter]
#
# Needs bubblewrap and python3. One cargo command. Temp folders go to
# $TMPDIR (set it to a folder on the build's file system: the proofs need
# the binary's copy and the data directory on one file system).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
tests=$(cd "$here/../.." && pwd)
copy="$tests/review_t8f_proofs.rs"
trap 'rm -f "$copy"' EXIT INT TERM
cp "$here/review_t8f_proofs.rs" "$copy"
cd "$tests/../../.."
HARNESS_REQUIRE_BWRAP=1 cargo test -p harness --test review_t8f_proofs -- \
    --nocapture --test-threads=1 "$@"
