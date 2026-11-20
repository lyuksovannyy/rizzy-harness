# B.1

## Experiment

Tests D1 and D2: `setsid sleep 1000`, then cancel; `kill -9` the node during a long child, restart (`bash.rs`, `acceptance_restart.rs`)

## Outcome that changes a decision

Predicted: both survive our `killpg`.
Then C.11 picks a containment and T.8 builds it; otherwise C.11 closes with the evidence.

## Size

**S**

