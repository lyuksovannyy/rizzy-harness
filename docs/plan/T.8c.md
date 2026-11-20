# T.8c **sec**

## Adds

**Done (branch `feat/T.8c-mount-view`): a contained command's mount namespace is bubblewrap's fresh root (read-only once built) with only the system read set (the top-level links as links), node.toml's `read` and `write` entries, the session's grants, a private `/dev`, `/proc` as the form has it, an empty `/tmp` and the node's running image at `/.rizzy-harness/rizzy-harness`, from which `sandbox-exec` starts (closes T8E-5 for contained commands; the node's helpers and an uncontained shell still start it by path, so `doctor`'s warning stays).
Grants with an identity are bound with `--bind-fd` from descriptors the guard opens without following a link and checks (bubblewrap 0.10.0 and newer, which added the option: v0.9.0's `bubblewrap.c` has none); with an older bubblewrap (Ubuntu 24.04 and CI: 0.9.0) by path, and `sandbox-exec` refuses a mismatch inside (B.11's fallback); chosen from `bwrap --version`, named in `doctor`'s new `file view` line.
Landlock inside unchanged.
The probes are `sandbox-exec`'s own checks (R-3 closed) and the containment probe checks that a file outside the grants is absent; `doctor` warns about a `write` entry holding a `PATH` folder.
Pathname sockets outside the grants are gone on every ABI (T8A-1 narrowed to sockets inside granted folders below ABI 9, and abstract sockets below ABI 6 with the network on); `contain = "required"` and `host_shell_shared` not relaxed (the owner's decision).
Tests `cli/tests/mount_view.rs` (14; 8 of the first 11 failed before), the sandbox, contain and network-off suites adjusted where they expected "permission denied".
About 0.2 ms a command.
Not tested: bubblewrap 0.9.0 itself (its path is exercised on 0.13.1), setuid bubblewrap, nested containers.**
The file view without Landlock (O12).
The mount namespace is built from the node's grants: `--bind-fd`/`--ro-bind-fd` on the descriptors the node already checked by identity (`Grant`, `crates/tools/src/sandbox.rs`), so the shell is confined on kernels without Landlock too, and the S-2 swap between check and mount is closed outright.
Landlock stays inside where the kernel has it.
Also (T.8b re-check R-3): the containment probe becomes a subcommand of our own binary (no `sh`, `cat` or `readlink` found by name on `PATH`), and `doctor` warns about a `[sandbox] write` entry that holds a folder on the node's `PATH`.

## Safeguards and abuse tests, written first

Tests first, from `files.sh`: a sibling session's folder, a `~/.ssh` stand-in and an outward symlink's target are absent; a folder swapped for a link after the node's check is not what gets mounted (the checked one is); the same tests pass with Landlock unavailable; files the command creates are owned by the node user with ordinary modes; git and a compiler work in the folder.

## Invariants

Node decides

## Podman / Node

No / No

## Depends

T.8a

## Size

**M**

