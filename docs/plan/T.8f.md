# T.8f **sec**

## Adds

**Done (branch `feat/T.8f-image-link`): `sandbox-exec` in a contained command is started through a descriptor of the node's image, not a path.
The guard opens its own `/proc/self/exe` `O_PATH`, not close-on-exec, and gives bubblewrap `/proc/self/fd/<n>` as the program; bubblewrap leaves descriptors it was not told about open in the process that runs the program (tested on 0.13.1; 0.9.0's source closes extra descriptors only in its monitor and pid-1 init, CI is the real test), so `execve` runs the node's inode whatever any path holds.
The image is no longer in the view (`/.rizzy-harness/…` and its bind removed). `sandbox-exec` closes every descriptor above 2 first (`close_inherited`: `/proc/self/fd` listed, each closed; review T8F-2).
A rebuild, an upgrade, a deleted binary or a session's file at the binary's path changes nothing for contained commands, on any file systems; no "restart the node" refusal is left; nothing is written to the data directory.
The first design, a hard link of the image in the data directory, was built and dropped after its review (`docs/security-review/2026-11-T8f-image-link.md`): it could not be made across file systems (this project's own setup) or for a root-owned binary, and with binds by path its own path was bound unchecked (T8F-1 to T8F-4).
Tests `cli/tests/own_image.rs` (5, all failing on the link design: 8 form/bind/network combinations with the data directory on another file system than the binary; no descriptor of the image in the command and its mode unchanged after `chmod` through every descriptor; 200 races by path with 0 session files run, 2 and 20 refusals before; a failed second start, `doctor` of either build and a restart leave the node's commands running and make no `node-image*`); `mount_view`'s deleted-binary test now expects the command to run.
The view's cost per command unchanged (0.2 to 0.3 ms over T.8a's `--dev-bind / /`, median of 30, debug build, a loaded machine: 7.0 ms against 6.8 ms).
Residuals, in README and PLAN: the node runs its old build silently until restarted (T8F-9: `doctor` cannot tell, there is no record of the running node before A.20); a session that owns a name of the file the node runs can `chmod 000` it and stop every command and the restart (T8F-5, now in `doctor`'s warning).
Re-check (`docs/security-review/2026-11-T8f-image-link.md`, "Re-check after the redesign"): R-1 the image is held `O_PATH`, opened once from `/proc/self/exe` at start (`hold_own_image`, which also removes `open_image`'s second `current_exe()` comparison), so a binary of mode 311 is run only through it; a node without a descriptor says so on its containment line and `required` refuses.
R-4 the helpers (`Sandbox::helper`: checkpoint git, graphify) and an uncontained shell start `sandbox-exec` from the same descriptor: once started the node executes its binary's path nowhere (what is left by path: the fallback without a descriptor, and the master's PDF reader).
R-2 at mode 000 the tool result and a helper's error say that the node's own binary could not be executed, its path and `chmod`.
R-5 `open_view`'s comment says what it does (another program after the options is passed through: tests of the view without Landlock need it).
R-3 `close_inherited` without `/proc` stops at the soft limit, said in its comment (nix 0.31.3 has no safe `close_range`).
Tests `own_image.rs` 9 (4 new, failing before), `sandbox.rs` `a_node_without_a_descriptor_of_its_image_says_so`. `argv[0]` of `sandbox-exec` is `/proc/self/fd/<n>` (bubblewrap's `--argv0` is not used: not known to be in every version a node may have).****
Keep the node's own image reachable after its file is replaced (T.8c's review, note T8C-N3).
Since T.8c a contained command is refused with "restart the node" once the harness binary on disk was deleted or replaced under a running node.
That is right after an upgrade and a lockout for a node run from its own build tree, where one `cargo build` by a session ends every later command.
Start `sandbox-exec` through the guard's descriptor of its own image instead of binding the image by a path, and close every inherited descriptor in `sandbox-exec`.

## Safeguards and abuse tests, written first

Tests first: with the binary replaced by a rename after the node started, contained commands still run, from the old image, in both forms and both binds, with the data directory on another file system than the binary (today: refused); no descriptor of the image reaches the command; by path a session's file never runs as `sandbox-exec`; a second start leaves the first node's commands running.

## Invariants

Node decides

## Podman / Node

No / No

## Depends

T.8c

## Size

**S**

