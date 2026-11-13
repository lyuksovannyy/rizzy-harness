#!/bin/sh
# Review of the T43 change: copy the proofs next to the crates' own tests,
# run them, remove the copies. Run from the worktree root.
set -u
root=$(pwd)
here="$root/crates/cli/tests/support/review_t43"
cp "$here/review_t43_core_proofs.rs" "$root/crates/core/tests/review_t43_core.rs"
cp "$here/review_t43_loop_proofs.rs" "$root/crates/agent-native/tests/review_t43_loop.rs"
CARGO_BUILD_JOBS=8 cargo test -p harness-core --test review_t43_core -- --nocapture; a=$?
CARGO_BUILD_JOBS=8 cargo test -p harness-agent-native --test review_t43_loop -- --nocapture; b=$?
rm -f "$root/crates/core/tests/review_t43_core.rs" "$root/crates/agent-native/tests/review_t43_loop.rs"
exit $((a + b))
