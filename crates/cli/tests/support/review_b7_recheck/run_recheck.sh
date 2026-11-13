#!/bin/sh
# Delta re-check of B.7: run the proofs in real_node_recheck.rs. Not part of
# the suite: copies the file into crates/node/tests for the run and removes
# the copy again, whatever happens. One cargo command.
#
#   run_recheck.sh [test-name-filter]
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../../../.." && pwd)
copy="$root/crates/node/tests/review_b7_recheck.rs"
trap 'rm -f "$copy"' EXIT INT TERM
cp "$here/real_node_recheck.rs" "$copy"
cd "$root"
CARGO_BUILD_JOBS=5 cargo test -p harness-node --test review_b7_recheck -- \
    --nocapture --test-threads=1 "$@"
