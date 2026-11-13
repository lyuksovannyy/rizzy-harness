#!/bin/sh
# Review of PR 76: copy the proofs next to the crates' own tests, run them,
# remove the copies. Run from the worktree root.
set -u
root=$(pwd)
here="$root/crates/cli/tests/support/review_76"
cp "$here/review_76_core_proofs.rs" "$root/crates/core/tests/review_76_core.rs"
cp "$here/review_76_loop_proofs.rs" "$root/crates/agent-native/tests/review_76_loop.rs"
CARGO_BUILD_JOBS=8 cargo test -p harness-core --test review_76_core -- --nocapture; a=$?
CARGO_BUILD_JOBS=8 cargo test -p harness-agent-native --test review_76_loop -- --nocapture; b=$?
rm -f "$root/crates/core/tests/review_76_core.rs" "$root/crates/agent-native/tests/review_76_loop.rs"
rmdir "$root/crates/core/tests" 2>/dev/null
exit $((a + b))
