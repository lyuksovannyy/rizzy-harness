# B.7

## Experiment

Test D17: stream at the link cap while a cancel and an approval answer arrive

## Outcome that changes a decision

Either waiting past grace means a control lane in the pump, as a fix before Gate 3. **Run (branch `exp/B.7-control-frames-under-load`, `crates/link/tests/control_under_load.rs` and `crates/node/tests/control_under_load.rs`): both directions wait past the grace, and on the real node a Stop does not end a streaming command while the out queue is full; see the results below.
The row's outcome holds: a fix is needed.
Gate 3 was called before this ran; the owner put the fix in Phase A as A.21 (security work, 2026-11-13).
Nothing is built here.**

## Size

**S**

