# T.7 **sec**

## Adds

Read-only shell for read-only agents (C.10)

## Safeguards and abuse tests, written first

A Landlock profile with only a per-call temp directory writable, chosen by the node for agents without a write tool; gives Scout and Reviewer `bash` for `grep` and `git log`. `touch x` fails; `git log` works; a spec asking a writable shell for Scout is refused by the node.
**Shell done (feat/plan-mode, PLAN.md T66, docs/read-only-shell.md):** bubblewrap's read-only binds with a sized tmpfs `/tmp`, Landlock inside as the second layer, not a Landlock profile alone (it leaves `chmod`, `utime`, xattrs open); none without a containment.
Plan mode is its first user.
**Done (feat/read-only-agents-shell, PLAN.md T70):** one allow-list of tools that change nothing in `harness-core` (`READ_ONLY_TOOLS`, `AgentSpec::changes_things`) read by the master's small-model setting, the node's sub-agent folders, plan mode, the parallel batch and the repeated-call guard; an agent that changes nothing else gets `bash` only in the read-only shell, chosen by the node from the tools it offers (no spec field can ask for a writable one);
Scout and Reviewer have it (the per-agent switch "Read-only commands", off for new custom agents); `CARGO_TARGET_DIR` in the temp folder and a node.toml size (`[sandbox] read_only_tmp_mib`, 256 MiB default) so a small `cargo test` runs there; review of PR 131: Landlock is the second layer for a folder under `/tmp` too, no network in the read-only shell by default (`[sandbox] read_only_network`, plan mode included), and `publish`/MCP-only sub-agents keep sharing the parent's folder (`needs_own_folder`).
**Left:** Node.js and Python toolchains not measured; no `bash` at all for read-only agents on a node without bubblewrap; seccomp (T.6) for the Landlock-only path.

## Invariants

Node decides

## Podman / Node

No / No

## Depends

C.10

## Size

**M**

