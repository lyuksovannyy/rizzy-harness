# C.6

## Decision

Containers, forge, deletion (note amendments)

## What the note settles

§4.5: A.5's keys; a checkpoint budget if B.3 asks; caps a session may lower, never raise; never `extra_args`. §4.6: `--network=none` default until test D14 shows what rootless `pasta` exposes; `--ulimit core=0`; sized `nosuid,nodev,noexec` tmpfs on `/run`; argv-hash label, recreate on mismatch; 64-hex id check; mount source validated against the data directory, `$XDG_RUNTIME_DIR` and `~/.ssh`; `podman inspect` assertions; stop on actor loss; identical checks inside.
Tests D14, D15 are N.4's first step.
B.11 corrections (note §4.5, §4.6, written 2026-11-12): `network = "on"` means `--network=pasta:--map-guest-addr,none` (the default pasta reaches host services on `0.0.0.0` and the LAN address); the mount source is checked again after start by device and inode from inside the container, since Podman follows a symlinked source; a cancel kills the command's group inside and stops the container when anything untracked is left, since a `setsid` child survives the group kill. §4.7: a forge token is readable in container sessions. §4.8: deletion stops routines and bumps epochs.
OpenHands read first if O7. `03` §6.1, §6.4, §6.5.

## Tests it commits to

Off-list image, raised cap, unknown flag refused by the node; inspect shows label, `core=0`, no network; cancel kills inside within 2 s; a routine on a deleted environment stops.

