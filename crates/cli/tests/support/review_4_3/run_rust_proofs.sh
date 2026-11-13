#!/bin/sh
# Fix plan 4.3 review: run review_4_3_proofs.rs against the real runner. It
# is not part of the test suite: this copies it to crates/store/tests/ for
# the run and removes the copy again, whatever happens.
#
#   run_rust_proofs.sh [test-name-filter]
#
# One cargo command. Temp folders go to target/tmp/review_4_3 (never /tmp).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../../../.." && pwd)
copy="$root/crates/store/tests/review_4_3_proofs.rs"
trap 'rm -f "$copy"' EXIT INT TERM
cp "$here/review_4_3_proofs.rs" "$copy"
mkdir -p "$root/target/tmp/review_4_3"
cd "$root"
TMPDIR="$root/target/tmp/review_4_3" CARGO_BUILD_JOBS=5 \
    cargo test -p harness-store --test review_4_3_proofs -- \
    --nocapture --test-threads=1 "$@"
