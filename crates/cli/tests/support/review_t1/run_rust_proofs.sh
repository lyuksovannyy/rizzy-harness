#!/bin/sh
# T.1 review: run review_t1_proofs.rs against the real loop. It is not part
# of the test suite: this copies it to crates/agent-native/tests/ for the
# run and removes the copy again, whatever happens.
#
#   run_rust_proofs.sh [test-name-filter]
#
# One cargo command. Temporary folders go under the target directory.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../../../.." && pwd)
copy="$root/crates/agent-native/tests/review_t1_proofs.rs"
trap 'rm -f "$copy"' EXIT INT TERM
cp "$here/review_t1_proofs.rs" "$copy"
cd "$root"
TMPDIR="${CARGO_TARGET_DIR:-$root/target}/tmp/review_t1"
mkdir -p "$TMPDIR"
export TMPDIR
CARGO_BUILD_JOBS=5 cargo test -p harness-agent-native --test review_t1_proofs -- \
    --nocapture --test-threads=1 "$@"
