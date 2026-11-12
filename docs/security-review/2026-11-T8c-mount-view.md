# T.8c review, 2026-11-12: the file view from the node's grants

Reviewed: `feat/T.8c-mount-view` at `15a780b`, only what it adds to
`feat/T.8b-network-off` (`a4445a2`, `8b6a14c`, `8e6fe33`, `1de1a05`). Read in
full, then attacked on the branch's own build. No product or test source
changed; only this report and the scripts under
`crates/cli/tests/support/review_t8c/` were added. Machine: Arch Linux,
kernel 7.2.15, Landlock ABI (effective) as the suite reports, bubblewrap
0.13.1 (not setuid), ext4 (`nosuid,nodev`), `fs.protected_hardlinks=1`,
`fs.protected_symlinks=1`, LSMs `capability,landlock,lockdown,yama,bpf`. The
top-level `/bin /sbin /lib /lib64` are symlinks into `/usr`. Format and
severity scale as in `2026-11-T8b-network-off.md`.

## 1. Verdict

**The view holds.** What is not granted is absent, not "permission denied",
with Landlock and without it; a pathname socket outside the grants is gone; a
grant is bound from a descriptor whose identity the guard checked (fd mode) or
is caught inside by `sandbox-exec` (path mode); the node's image is bound last
from its own running image and cannot be shadowed; the probes are the
wrapper's own checks and no program is found by name. The containment, kill,
network-off and step-up paths keep working (the `contain`, `network_off` and
`sandbox` suites pass, 18/17/13). I could not break out of the view, read a
sibling session's folder, a `~/.ssh` stand-in or the data directory, reach a
socket outside the grants, or make a later command run attacker content as
`sandbox-exec`.

The findings are all Note/Low: a test that fails for the wrong reason under a
long `TMPDIR`, an inherited `O_PATH` descriptor of the node's image in path
mode that grants no new power, a self-inflicted denial of service that is
already the documented dev-tree warning, and a host-side mkdir side effect
during bubblewrap's setup that is harmless. Nothing asked for before merge.

| Id | Severity | What | Proof |
|---|---|---|---|
| T8C-N1 | Note | `a_socket_outside_the_grants_cannot_be_reached` panics (`SUN_LEN`) under a long `TMPDIR`: on CI with a long temp path the claim-3 test fails for an unrelated reason instead of exercising the claim. | re-run with a long vs. short `TMPDIR` |
| T8C-N2 | Note | In path mode the command inherits an `O_PATH` descriptor of the node's image (the test acknowledges it, asserts only "not a DIR"). `/proc/self/fd/<n>` could reopen the underlying binary inode, past the read-only bind — but only where the binary is already session-writable, so no new capability. | reading; T8E-5 scenario |
| T8C-N3 | Note | The deleted-binary refusal is a self-DoS for a dev-tree node (binary in a writable grant): one command bricks every later contained command until restart. Fail-closed and within the existing warning. | reading; `a_deleted_binary_refuses_the_command_with_the_reason` |
| T8C-N4 | Note | bubblewrap `mkdir`s destination parents on the host when a bind destination goes through an attacker-swapped intermediate symlink (writable space only; the command is still refused by identity). | `bwrap_dest_link.sh`, case A2 |

Verdict per claim: 1 holds, 2 holds, 3 holds, 4 holds, 5 holds, 6 holds.

## 2. What I attacked and what held

### Claim 1 — only the grants exist, nothing new readable/writable. Holds.

The suite's `outside_the_grants_is_absent`, `_without_landlock` and
`_in_every_form_and_bind` pass: a sibling session's folder, the `~/.ssh`
stand-in, the data directory and both outward symlinks' targets are `absent`
(not denied), with Landlock, with `sandbox-exec` taken out so only the view
confines, and in `(Full,Path) (SharedProc,Fd) (SharedProc,Path)`.

I read `view()` (`crates/tools/src/sandbox.rs:2078`) for a way to mount or
turn writable something that was not granted:

- **`@rizzy`/newline/`..`/token-shaped paths.** Grant paths are absolute and
  canonical (`Sandbox::new` refuses relative; `real()`/`FolderPin` canonicalize),
  so no component is `..` and no destination word begins with `@rizzy` (they
  begin with `/`). `open_view` rejects any stray `@rizzy` word with "a bad view
  argument". argv is passed to `exec`, not a shell, so a newline cannot split.
- **A grant inside another / read inside write / write inside read.** The
  writable-propagation (`m.write |= ancestor-is-rw`) matches Landlock's
  union-of-rules-above-a-path, so the view is not more permissive than Landlock
  was. `the_view_is_the_grants` covers the read-inside-write case. A path
  granted twice is bound once, by its identity.
- **A destination under `/proc`, `/dev`, or the image's place.** `view()` skips
  `starts_with("/dev")` and `== /proc`; the image is bound **last** at
  `/.rizzy-harness/rizzy-harness` and then `--remount-ro /`, so no grant can
  cover it (no session folder canonicalizes to `/.rizzy-harness`). Order is by
  component count, folder before child.
- **System links out of the set / attacker-writable system folder.** A system
  entry that is a symlink into another granted folder becomes `--symlink`
  (`/bin -> usr/bin`); otherwise `Source::Host`, read-only, link followed as
  Landlock follows it. These are root-owned; nothing a session writes replaces
  them.
- **Hard links to outside the view.** A command can only name sources that
  exist in the view, so it cannot `ln` an out-of-view file (e.g. `~/.ssh/id`)
  into its folder — the source path is absent. `fs.protected_hardlinks=1`
  further bounds it. A hardlink would have to be planted by an *unconfined*
  process, outside the threat model.
- **A submount under a granted folder.** A recursive bind would expose only
  what is already inside the granted subtree (Landlock grants the subtree too);
  the owner's other mounts are not under a grant and stay absent.

### Claim 2 — what is bound is what the node checked. Holds.

- **Fd mode (0.10.0+).** The guard opens the destination `O_PATH|O_NOFOLLOW`,
  `fstat`s it and compares dev/ino to what the node recorded, then hands
  bubblewrap the **descriptor** (`--bind-fd <n>`), so the source is the checked
  object regardless of any later path games; the destination path only decides
  *where* in the (own) namespace it appears. A swap after the open lands on a
  different inode → identity mismatch → refused.
- **Path mode (0.9.0, CI).** bubblewrap mounts by path before `sandbox-exec`
  runs; `sandbox-exec`'s `open_grant` then opens the mounted path
  `O_NOFOLLOW`, `fstat`s and compares to the recorded identity, refusing
  (`EXIT_UNCONFINED`, command not run) on a mismatch. `with_binds_by_path_a_swapped_folder_refuses_the_command`
  proves it. The wrong folder is, at worst, briefly bound inside the **refused**
  command's own mount namespace (invisible to others); no attacker code runs
  there.
- **The setup window (T8C-N4).** `bwrap_dest_link.sh` (real bubblewrap, no
  harness code) shows that when a bind destination goes through an
  attacker-swapped *intermediate* symlink, bubblewrap `mkdir`s the parents on
  the host (case A2 created `drop2/e`), and when the *last* component is a
  symlink it refuses ("Can't mount on symlink destination", cases A1/B1).
  Absolute destination links resolve inside the new root and fail
  "Can't mkdir parents" (A3/A4; `bwrap_setup_root.sh`: only `x -> /` and
  `x -> /tmp`, i.e. dirs that exist in the root, succeed). The host mkdir lands
  in already-writable space and the command is still refused by the identity
  check above, so it is a harmless side effect, not an escape. In the real node
  the destination is a canonical grant path and its parents are caught by
  `FolderPin::check`/`Extra::grant` before the view is built.

**The path-mode stand-in.** The suite exercises path mode by forcing
`Binds::Path` on bubblewrap 0.13.1 (`Contain::with_binds`). That forces the
argument **form** (`--bind`/`--ro-bind` by path, `--symlink`, `--ro-bind-try`,
`--dev`, `--proc`, `--dir`, `--remount-ro`, `--unshare-*` — all present in
0.9.0; only `--bind-fd`/`--ro-bind-fd` are 0.10.0), but runs 0.13.1's mount
code. A real 0.9.0 could differ in *whether* it refuses a symlink destination
or how it orders/remounts, but the security of path mode rests on
`sandbox-exec`'s inside identity check, not on bubblewrap's behaviour, so those
differences cannot widen the grants — at most they would make a mount fail and
the node refuse. Not tested: a real 0.9.0, setuid bubblewrap, a nested
container.

### Claim 3 — a pathname socket outside the grants is unreachable. Holds.

`a_socket_outside_the_grants_cannot_be_reached` passes under a short `TMPDIR`:
the connect gets `FileNotFoundError` (the socket is absent from the view), with
this kernel's Landlock and with the ABI 4 stand-in, and the service starts no
process. The residuals are stated honestly in `contained_line` and the module
docs: a socket **inside** a granted folder (the session's own, a `[sandbox]
write` socket dir — warned by `socket_write_entries`) and, below ABI 6 with the
network **on**, abstract sockets (closed by the network-off namespace). See
T8C-N1 for the test's fragility.

### Claim 4 — image and probes from the running image, no program by name. Holds.

`restrict`'s probe branch runs `req.probe.run()` after confinement, in place of
a command; `Probe` checks files/`pid`/`/proc`/`net ns`/absence directly in
Rust — no `sh`/`cat`/`readlink`. `a_planted_program_on_path_does_not_decide_the_probe`
and `the_probe_checks_what_it_is_asked` pass. `sandbox-exec` runs from
`VIEW_EXE`, bound from the guard's `/proc/self/exe` (the node's running image),
so a replaced on-disk binary is not what runs:
`a_replaced_binary_does_not_run_as_sandbox_exec` passes. The deleted-binary
check (`/proc/self/fd/<n>` readlink ends `" (deleted)"`) is sound and
fail-closed; its only edge is the self-DoS of T8C-N3, which is the documented
dev-tree case.

### Claim 5 — guard, kill, network-off, step-up, both bind forms keep working; no folder fd reaches the command. Holds.

`contain` (18), `network_off` (17) and `sandbox` (13) pass.
`no_descriptor_of_a_folder_reaches_the_command` passes in both modes.
T8C-N2: in path mode the command does inherit the image's `O_PATH` descriptor
(the test says so and asserts only that it is not a directory). `openat(fd,
"..")` on an `O_PATH` of a regular file is `ENOTDIR`; `/proc/self/fd/<n>`
reopen for write would hit the underlying binary inode past the read-only bind,
but only succeeds where the binary is already session-writable — the same
condition under which the command can write the binary through the view mount
anyway, so no new capability. A clean install (binary outside every grant)
closes it.

### Claim 6 — no rule relaxed; the documents say what is left. Holds.

`contain = "required"` still refuses below ABI 9 (unchanged); `host_shell_shared`
untouched. `check_binary_reach` still warns/refuses for a session-writable
binary and now says plainly that a **contained** command is unaffected while the
node's helpers and an uncontained shell still start `sandbox-exec` by path. The
module docs, `README`, `PLAN.md` R2/T9 and the new `doctor` "file view" line
state that the Landlock-only fallback and `contain = "off"` have no view, and
that sockets inside granted folders remain. I found no sentence that claims
more than is true; "Nothing else of this machine is there" is paired in the
same line with the explicit `/proc` disclosure (`this machine's /proc` in the
`SharedProc` form).

## 3. The tests, per claim

Each claim has a test that targets the claim, not the happy path, and that
failed before the change (per the plan row: 8 of the first 11 `mount_view`
tests failed on `main`). `outside_the_grants_is_absent*` assert **absence**
(not "denied"), the swap tests assert the checked folder **or** a refusal (not
the link's target), the socket test asserts the service started no process,
`a_replaced_binary_*` asserts the replacement did not run as `sandbox-exec`.
The weak spot is T8C-N1 (the socket test's own `UnixListener::bind` breaks
above the 108-byte `sun_path` limit); the smallest fix is to bind the
stand-in service at a short fixed path (or skip-with-reason when the fixture
path is too long) so the claim is exercised rather than the harness tripping.

## 4. Findings (all Note)

### T8C-N1. The socket claim's test fails for the wrong reason under a long TMPDIR

**Claim touched.** Claim 3's test.
**Reproduction.** `CARGO_TARGET_DIR=<t> TMPDIR=<long> cargo test -p harness
--test mount_view a_socket_outside_the_grants_cannot_be_reached` panics:
`path must be shorter than SUN_LEN` at `mount_view.rs:486`
(`UnixListener::bind`). With the default short `TMPDIR=/tmp` the same test
passes. So the product defence is sound, but on a runner with a long temp
path the test fails before it tests anything.
**Smallest fix.** Bind the stand-in service at a short path (e.g. a fixed
`/tmp/..`-free short dir the test owns), or `return` with a printed reason when
`f.root.join("home/bus")` exceeds the limit. **Test that should exist:** the
same test, resilient to a long `TMPDIR`.

### T8C-N2. Path mode leaks the image O_PATH fd (no new power)

**Claim touched.** Claim 5. Documented by the test; stated here so the
residual is explicit. **Fix I would accept:** none required; optionally open
the image `O_PATH` with `O_CLOEXEC` dropped only for bubblewrap's inheritance
and closed before the command, or state in the module doc that the leak is
bounded by the binary's on-disk permissions. **Test:** a path-mode command
tries `exec 3</proc/self/fd/<image>` for write and is denied when the binary
is read-only.

### T8C-N3. Deleted-binary self-DoS in a dev tree

**Claim touched.** Claim 4 (fail-closed). A command in a node whose binary is
inside a writable grant can delete/replace it and brick every later contained
command ("restart the node") for all sessions until restart. This is the
`check_binary_reach` scenario that already warns (`auto`) or refuses
(`required`); fail-closed is correct. **Fix:** none; keep the warning.

### T8C-N4. bubblewrap mkdirs host parents through a swapped destination link

**Claim touched.** Claim 2's window. Harmless (writable space, command refused
by identity); noted so the host-side side effect is on record. **Fix:** none.

## 5. What I could not test, and why

- A real bubblewrap 0.9.0 (path mode is exercised on 0.13.1 via `with_binds`);
  setuid bubblewrap; a nested/rootless container (`SharedProc` without a fresh
  `/proc`).
- The ABI 4 socket path only through the stand-in (`landlock_abi4.py`); no
  kernel pinned at ABI 4 here.
- The `O_PATH` reopen-for-write on a genuinely read-only installed binary (the
  test binary is user-writable); reasoned, not run.

## 6. Housekeeping

Scripts added under `crates/cli/tests/support/review_t8c/`
(`bwrap_dest_link.sh`, `bwrap_setup_root.sh`); each guards its scratch dir,
runs only short `sh -c` payloads inside its own bubblewrap namespaces, and
leaves nothing. After the run: no `bwrap`/`sleep` of mine alive, no mount of
mine in `/proc/self/mountinfo`, no stray stand-in `sleep`. Checked by `ps -u`,
by grepping `/proc/self/mountinfo` for the scratch paths, and by scanning
`/proc/*/cmdline` for the stand-in sleeps — all clear.
