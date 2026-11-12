#!/bin/sh
# T.8e review: run review_t8e_proofs.rs (the proofs that need the node's own
# code). It is not part of the test suite: this copies it to crates/cli/tests/
# for the run and removes the copy again, whatever happens.
#
#   run_rust_proofs.sh [test-name-filter]
#
# Needs bubblewrap, fusermount3 and python3. One cargo command.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
tests=$(cd "$here/../.." && pwd)
copy="$tests/review_t8e_proofs.rs"
trap 'rm -f "$copy"' EXIT INT TERM
cp "$here/review_t8e_proofs.rs" "$copy"
cd "$tests/../../.."
HARNESS_REQUIRE_BWRAP=1 cargo test -p harness --test review_t8e_proofs -- \
    --nocapture --test-threads=1 "$@"
