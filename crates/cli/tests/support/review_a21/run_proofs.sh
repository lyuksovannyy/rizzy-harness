#!/bin/sh
# Independent review of A.21: run the proofs in review_a21_proofs.rs. Not
# part of the suite: copies the file into crates/node/tests for the run and
# removes the copy again, whatever happens. One cargo command.
#
#   run_proofs.sh [test-name-filter]
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../../../.." && pwd)
copy="$root/crates/node/tests/review_a21.rs"
trap 'rm -f "$copy"' EXIT INT TERM
cp "$here/review_a21_proofs.rs" "$copy"
cd "$root"
CARGO_BUILD_JOBS=12 cargo test -p harness-node --test review_a21 -- \
    --nocapture --test-threads=1 "$@"
