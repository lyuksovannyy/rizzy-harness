# X.43 **sec** (T9, T36, T39)

## Adds

Background commands in a `host` chat (the owner, 2026-11-20: "Yes, allow"; review N-c / R16 refused them because only a contained command's end can be confirmed; that no longer decides, since a steered agent in a host chat can already leave a process from a foreground call with `setsid`, and refusing pushed agents to that invisible route)

## Safeguards and abuse tests

**Done on branch `feat/host-background`:** a host chat's `bash` with `background: true` runs under the guard in a new `--host` mode (no bubblewrap, no PID namespace; its proof is the subreaper's: every descendant stays in the guard's subtree, the kill loop kills the command's process group and every child until `waitpid` says ECHILD, `ended` only then), held in the same table with the same stop paths (timeout, `bash_stop`, Stop and Stop all, deletion, revoked owner, node stop and death); the start, stop and detach texts say what is true per mode; background sub-agents stay refused in a host chat; the foreground host call is unchanged.
**Amended 2026-11-20 (the owner: "dont, like if user selected host project workspace, why do we need to limit it? its his choice"; branch `feat/host-no-extra-limits`):** background sub-agents are allowed in a host chat, and background commands on a node with `[sandbox] mode = "off"` (the guard starts from the node's held image, no wrapper); a host chat gets no limit a sandbox chat does not have.
Leftovers named in PLAN T9: another service the command asked to start, a guard killed with SIGKILL (the start sweep finds nothing: no `--mark`).

