#!/usr/bin/env bash
# Copies the review's proof tests into crates/master/tests, runs them, and
# removes the copy again. Usage: run.sh [test-name-filter]
set -u
root="$(cd "$(dirname "$0")/../../../../.." && pwd)"
here="$root/crates/cli/tests/support/review_n10a2"
dst="$root/crates/master/tests/review_n10a2_levels.rs"
cp "$here/review_n10a2_levels.rs" "$dst"
trap 'rm -f "$dst"' EXIT
cd "$root"
CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-12}" cargo test -p harness-master \
  --test review_n10a2_levels -- --test-threads=4 "$@"
