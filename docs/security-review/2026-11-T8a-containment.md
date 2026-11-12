# T.8a review, 2026-11-12: the shell's containment in bubblewrap

Reviewed: `feat/T.8a-shell-containment` at `2d98e91` (five commits on
`origin/main` `f5223ab`). Read in full, then attacked on the branch's own
build. No product change. Machine: Arch Linux, kernel 7.2.15, Landlock ABI 10
(the wrapper uses up to 9), bubblewrap 0.13.1 (not setuid), `ptrace_scope` 1,
a KDE login session with a systemd user manager and session bus.

## 1. Verdict

**Fit after the named fixes.** The mechanism is sound: on this machine every
process a contained command started was gone after exit, cancel, timeout, a
dropped call and `kill -9` of the node, and I found no way for a command to do
more to files, devices or other processes than it could before. What is not
fit is the sentence the change is sold with. "Nothing a command starts
outlives it" is printed by `doctor`, the start-up log, `README.md` and
`PLAN.md` T9 without condition, and it is true only where Landlock is new
enough to close Unix sockets. On the kernels people run today it is false in
one D-Bus call (T8A-1).

| Id | Severity | What | Proof |
|---|---|---|---|
| T8A-1 | **High** (the claim; the hole is the known R2) | Below Landlock ABI 9 a contained command asks the session bus to start a process. It runs outside the namespace and outside Landlock, and survives the kill. `doctor` says "contained … nothing a command starts outlives it". | two tests, one script |
| T8A-2 | **Low** | A kill of bubblewrap about 0.5 ms after its spawn leaves the namespace running while the node lives; a `kill -9` of the node in the first 0.6 ms does the same. `PLAN.md` names a narrower window. | one test, one script |
| N-1 to N-6 | Note | `PATH` lookup, the spawner thread, a node that re-executes itself, test gaps, `doctor`'s wording, what a command newly sees | one test; reading |

Fixes asked for before merge: T8A-1's text and `doctor` line, and T8A-2's
wording in `PLAN.md`. T8A-2's code fix is five lines and should ride along.

## 2. Findings

### T8A-1. High. One call to a service outside the namespace starts a process the kill never reaches

**What the agent does.** From a contained command, on a kernel below Landlock
ABI 9 (Linux 7.1; Ubuntu 24.04's 6.8 has ABI 4, Debian 13's 6.12 has ABI 6),
it connects to `/run/user/<uid>/bus` and sends `StartTransientUnit`:

```
gdbus call --session --dest org.freedesktop.systemd1 --object-path /org/freedesktop/systemd1 \
  --method org.freedesktop.systemd1.Manager.StartTransientUnit x.service fail \
  "[('ExecStart', <[('/bin/sh', ['/bin/sh','-c','…'], false)]>)]" '[]'
```

**What happens.** The user manager starts the program in the host's PID
namespace, without Landlock and without `no_new_privs`. The node's kill ends
everything inside the namespace and leaves that process. The same works
through any service of the node user that takes requests on a Unix socket: a
container runtime's socket, a tmux server, an ssh-agent; below ABI 6 also
through abstract sockets (X11's `@/tmp/.X11-unix/X0` accepted a connect).

**Why it is this change's finding.** The socket hole is `PLAN.md` R2, open and
documented for the file policy. T.8a adds `--dev-bind / /`, so every socket
stays in view, and Landlock's pathname-socket right (ABI 9) and socket scope
(ABI 6) are asked for best-effort only (`crates/tools/src/sandbox.rs:1282-1294`).
The containment therefore promises something that only those two rights
deliver, and says so nowhere:

- `crates/tools/src/sandbox.rs:952-956` and `:965-971`: the `doctor` and log
  line, "nothing a command starts outlives it", whatever the ABI;
- `README.md:705`: "**Nothing a command starts outlives it.**";
- `PLAN.md:545` (T9): the same in bold; its residual list omits the sockets;
- `contain = "required"` is satisfied by it.

**Proof.**

- `review_t8a_1_with_abi_4_rights_one_connect_leaves_a_process_the_kill_does_not_reach`:
  the node's exact bubblewrap command line (`Confinement::shell`), with
  `sandbox-exec` replaced by a stand-in that applies what an ABI 4 kernel
  enforces (`support/review_t8a/landlock_abi4.py`). The command is in its PID
  namespace, a file outside the session folder is still refused, and one
  connect to a pathname socket and one to an abstract socket of a listener in
  the test make it start two processes. After SIGKILL to bubblewrap the
  `setsid` child and the foreground child are gone; the two started through
  the sockets run on. Passes; passing is the gap.
- `review_t8a_1_what_a_service_outside_starts_outlives_the_command_below_abi_9`:
  the same through the real wrapper and the real `bash` tool. It records
  which side of the line a kernel is on. Here (ABI 9 effective): both connects
  refused, nothing started.
- `support/review_t8a/session_bus.sh`, the real thing, run once on this
  machine against the user's systemd:

  ```
  === ABI 4 stand-in
  before the kill: inside the namespace: 1 setsid child, 1 foreground; the unit: active
  after the kill:  inside the namespace: 0 setsid child, 0 foreground; the unit: active
    the unit's process 203457: sleep 975236
    pid namespace pid:[4026531836], the host's is pid:[4026531836]; NoNewPrivs: 0
    it copied a file from outside the session folder: SECRET
  === the real wrapper, this kernel
  Error connecting: Could not connect: Permission denied
  after the kill:  inside the namespace: 0 setsid child, 0 foreground; the unit: inactive
  ```

A real kernel below ABI 9 was not available; the stand-in is the limit of this
proof (section 5).

One thing that looks like protection and is not: `systemd-run --user` and
`busctl` fail inside the PID namespace even where the socket is reachable
("No data available"; I did not trace why, most likely the peer's pid has no
number there). `gdbus` and `dbus-send` connect and work, and a client written
for the purpose would too.

**Smallest fix I would accept.** Tell the truth where the node already knows
it. `containment()` runs after the Landlock probe, whose status line carries
the effective ABI:

1. Below ABI 9 the containment line is a warning and says what is open, for
   example "contained, except through services reachable over Unix sockets
   (the session bus, a container runtime): Landlock ABI 4 < 9, see README".
   Below ABI 6 it names abstract sockets too.
2. `README.md` and `PLAN.md` T9 carry the condition next to the bold
   sentence, and T9's residual list gets the item.
3. `review_t8a_1_what_a_service_outside_…` goes into `cli/tests/contain.rs`
   so the line the kernel is on is recorded in CI (the runner's kernel is
   expected to be below ABI 9).

The real closure is T.8c (a mount view built from the grants has no
`/run/user`) plus a network namespace for abstract sockets; say in T.8c's row
that it closes this. A stop-gap inside T.8a, if the owner wants one:
`--tmpfs /run --tmpfs /tmp` hides the usual sockets, costs nothing that
Landlock allows today, and is not complete (sockets under `$HOME`).

### T8A-2. Low. The namespace survives a kill that lands before its init has set its death signal

**The mechanism.** The node never signals the namespace's init to kill it. It
kills bubblewrap and relies on `PR_SET_PDEATHSIG` in the init. bubblewrap's
init sets that itself and late (`bubblewrap.c`: `setsid`, the fork of the
command, then `do_init` calls `handle_die_with_parent`): after the mounts, and
after `setsid` has taken it out of the process group the node kills. If
bubblewrap dies before that call, the init has no parent and no death signal.
The command then runs to its own end in a namespace nobody holds; no timeout
applies any more.

**(a) While the node lives.** SIGKILL to bubblewrap's group in the roughly
50 microseconds after the init's `setsid`, about 0.5 ms after the spawn. That
is what `KillGroupOnDrop` and tokio's `kill_on_drop` send when a call is
dropped (`crates/tools/src/bash.rs:26-35`, `:176`, `:186`). The node reports
the call as ended and never looks again.

- `review_t8a_2_a_kill_just_after_the_inits_setsid_leaves_the_namespace_running`
  kills the group the instant `/proc` shows the init leading its session:
  bubblewrap is reaped, the init and the command run on under the user
  manager. First or second try here. With a delay of 0, 20, 50 and 100
  microseconds after the `setsid`: 19, 14, 1 and 0 of 20 survived.
- `kill_window.py group` (fixed delays after the spawn, no trigger): 3/40 at
  500, 1/40 at 600 and 700, 0 at 400 and 1000 microseconds.

Today's callers hardly reach it: a cancel and a timeout wait 2 s before
SIGKILL, and the loop abandons a tool only 5 s after a cancel
(`TOOL_CANCEL_GRACE`), so only a call dropped within a millisecond of its own
spawn qualifies. T.8d (background commands the node holds and ends) will add
kill paths, which is why it should be closed now.

**(b) When the node dies.** `PLAN.md` T9 says: "a node killed in the first
moment after a spawn, before bwrap has set its death signal". The window is
longer than that: until bwrap's *init* has set its own. `kill_window.py node`:
20/20 namespaces survived a `kill -9` of the spawning process at 0, 200, 400
and 500 microseconds after the spawn, 1/20 at 600, 2/20 at 700, 0 at 1000.
An agent cannot time a node's death, and cannot signal the node from inside.

**Fix.** For (a): on every SIGKILL path, `KillGroupOnDrop` included, read the
init with `sandbox_init` first, send it SIGKILL (from the parent namespace
that kills the namespace outright), then kill bubblewrap's group; or hold a
pidfd of the init. Then invert the test. For (b): correct the sentence in T9
to the measured window; I would accept that. Closing it needs a liveness
check inside the namespace (for example a pipe the node holds, looked at by
`sandbox-exec` before it `exec`s), which means passing a descriptor without
`unsafe`: a design question better taken with T.8d.

### Notes

- **N-1. `bwrap` is the first match on the node's `PATH`, checked only for
  being a file** (`crates/tools/src/contain.rs:117-131`). It is the one program
  on the command path that runs outside Landlock, at every start, in `doctor`
  and for every call. A `bwrap` planted in a session-writable folder that
  precedes `/usr/bin` on the node's `PATH` (a project's `bin`, an activated
  virtualenv) runs with the node's rights at the next start:
  `review_t8a_n1_the_first_bwrap_on_path_runs_outside_the_sandbox` has it copy
  the API token, and `doctor` prints "contained (<planted path>…)". A file
  there that is not executable is taken too, and under `auto` the node then
  runs uncontained instead of finding `/usr/bin/bwrap`. This is the trust in
  `PATH` the node's own git already has (`crates/node/src/git.rs:633`), so a
  Note. Cheap hardening: accept only a `bwrap` whose file and folder are
  owned by root and not writable by the node user, or refuse one inside a
  workspace root or `[sandbox] write` entry, as the extras are checked.
- **N-2. One spawner thread, no recovery.** If it ever ends, every running
  command is killed and every later one fails with "the containment spawner is
  gone" until a restart (fail closed, by reading). Every spawn of every
  session waits on it, and that wait is not raced against the cancel
  (`bash.rs:177`): a spawn that blocks (a session folder on a stalled network
  file system, where `chdir` hangs) stops the shell for all sessions. Not
  reproduced.
- **N-3. A node that re-executes itself kills every contained command with
  SIGKILL, no grace** (`execve` ends the spawner thread; measured with a
  Python stand-in: 0 processes left). Safe, but `docs/node-update.md`'s
  restart should know it. A stopped node (SIGSTOP) kills nothing and enforces
  no timeout until continued, as before.
- **N-4. Tests.** Each claim in T9 has a test that waits for the processes
  before it looks for them, so none passes on a process that never ran. Gaps:
  (1) every test builds its sandbox by hand (`support/bwrap.rs`); nothing
  runs `sandbox::start`'s successful branch or reads `doctor`'s line, so a
  start path that probed and then forgot `with_containment` would pass all of
  them while saying "contained". One CLI test that runs `doctor` and one
  contained command through a started node would close it. (2) Only four
  file-policy tests of `sandbox.rs` run both ways; network off, the nested
  wrapper, signals, sockets and `/proc/<pid>/environ` run without bubblewrap
  only (by hand inside: all held). (3) The cancel without a `children` file
  has no test. (4) Without `HARNESS_REQUIRE_BWRAP=1` a skipped test prints
  `ok`; with it, a missing `bwrap` fails (checked: 3 of 3 failed with `PATH`
  emptied of it).
- **N-5. `doctor` prints `ok    shell containment: NOT CONTAINED, …`.** The
  text is honest, the mark is not. A third mark (`warn`) would do.
- **N-6. What a contained command newly sees:** `/proc/1/cmdline` is
  bubblewrap's whole command line, so every grant: each `[sandbox]
  read`/`write` path, the session's temp path, the folders' device and inode
  numbers. Nothing secret. The README's list of what a command notices could
  name it. The IPC namespace is still the host's (`--unshare-ipc` would be
  free); not a finding.

## 3. What I tried that held

All inside a contained command on this machine, through the node's command
line and the real wrapper.

- **Capabilities.** `CapEff`, `CapPrm` and `CapBnd` are 0 and `NoNewPrivs` is
  1 in the command; bubblewrap drops everything before the wrapper runs.
- **Mounts and the file view.** `mount` (tmpfs, bind, proc, remount),
  `umount /proc`, `mknod`, `chroot`, `chown` to another id: refused. In a
  nested user namespace the command has every capability there, and mounts
  are still refused by Landlock; `chroot` then works, into a folder it could
  already reach. A file outside the grants stays unreadable through
  `/proc/1/root`, a nested namespace and a relative path alike.
- **Other processes.** `/proc/1/{root,cwd,environ,maps,ns}` and its fd
  targets: denied. `kill` of pid 1: denied. `nsenter -t 1`: denied. The
  wrapper cannot be traced before Landlock: no other process of the session
  is in its namespace.
- **Devices.** `/dev` cannot be listed; `/dev/sda`, `/dev/tty`, `/dev/pts`,
  `/dev/kmsg`, `/dev/shm`: denied.
- **Identity checks.** Unchanged code; `--dev-bind / /` keeps device and inode
  numbers; a replaced folder is still refused inside bubblewrap
  (`sub_agent_sandbox.rs`, which now runs contained, 7 passed).
- **Outside helpers on this kernel (ABI 9 effective).** The session bus, the
  system bus, systemd's private socket, Wayland, PipeWire, X11 by path and by
  abstract name, gpg-agent: connect refused. `systemd-run --user`, `busctl`,
  `dbus-send`, `gdbus`: fail. `podman run -d`: fails (no runtime directory, and
  `newuidmap` needs privileges). `at`, `crontab`, `tmux`: not installed.
- **The shared-`/proc` form.** Run on the host: the kill guarantee holds
  (author's test). A host process's command line and fd numbers are visible;
  `environ`, `mem`, `cwd`, `root`: denied; `kill` by its number: no such
  process; `pidfd_send_signal` on its `/proc` directory: invalid argument;
  `ptrace`: no such process. The log line and README describe it as it is.
- **The cancel path.** The number signalled comes from the kernel's
  `children` file of the node's own unreaped child; before the init's
  `setsid` the SIGTERM finds no group and is lost, and the SIGKILL 2 s later
  still ends everything. The reuse the author names needs a pid cycle inside a
  few microseconds.
- **The setting.** Only `node.toml`; an unknown key (a `bwrap` path among
  them) is a parse error; `required` with `mode = "off"` is an error, with
  `best-effort` it contains, without bubblewrap it refuses and `doctor` fails;
  `auto` and `off` say what they are. No argument reaches bubblewrap from a
  call: the folder is canonical and absolute, the labels are fixed.
- **Callers.** Nothing in the loop, the event schema or the UI branches on
  the old "signal" form. With network off, TCP connect and bind are denied
  inside too.
- **The earlier review's fixes are intact.** The diff leaves `open_grant`,
  `Grant`, `FolderPin` and the node's git untouched. With
  `HARNESS_REQUIRE_BWRAP=1`: `contain` 12 passed, `sandbox` 13,
  `sub_agent_sandbox` 7, `snapshot_sandbox` 4 (1 ignored).

## 4. CI

`.github/workflows/ci.yml` installs bubblewrap, sets
`kernel.apparmor_restrict_unprivileged_userns=0` where that file exists, and
runs the node's exact bubblewrap line before the tests, so a runner that
cannot do it fails there and not in a skip. That sysctl is Ubuntu's documented
switch for 24.04, and Ubuntu 24.04's bubblewrap has every flag used. I expect
it to work; it has not run. `HARNESS_REQUIRE_BWRAP=1` does make a missing
bubblewrap a failure (N-4).

## 5. Not tested

- A real kernel below Landlock ABI 9 or 6. T8A-1 rests on a stand-in that
  applies ABI 4's rights with the node's bubblewrap line. Running
  `session_bus.sh <rizzy-harness>` on Ubuntu 24.04 settles it.
- The CI job on `ubuntu-latest`.
- A setuid bubblewrap, a kernel without `CONFIG_PROC_CHILDREN`, the
  shared-`/proc` form inside a real nested container.
- N-2 (a dead or blocked spawner), a `bwrap` removed after start (by reading:
  the spawn fails and the call fails closed).
- Reading through an `open_tree` clone of `/` (the call succeeds in a nested
  user namespace; Landlock's rules are per inode, so I expect no gain).
- SysV shared memory of host processes (the IPC namespace is shared; this
  host has no segment to attack).
- `cargo test --workspace`, `cargo deny check` and the web checks. Run:
  `cargo fmt --all --check`, `cargo clippy -p harness-tools -p harness
  --all-targets -- -D warnings` (clean, proofs applied), and the four test
  binaries above.

## 6. Running the proofs

```bash
git apply docs/security-review/proofs/2026-11-T8a-containment.patch
cargo test -p harness --test review_t8a -- --nocapture          # T8A-1, T8A-2, N-1
REVIEW_T8A_TRIES=20 REVIEW_T8A_DELAY_US=20 \
  cargo test -p harness --test review_t8a review_t8a_2 -- --nocapture   # the window's width
python3 crates/cli/tests/support/review_t8a/kill_window.py node 0 200 400 500 600 700 1000
python3 crates/cli/tests/support/review_t8a/kill_window.py group 400 500 600 700 1000
crates/cli/tests/support/review_t8a/session_bus.sh                       # ABI 4 stand-in
crates/cli/tests/support/review_t8a/session_bus.sh target/debug/rizzy-harness   # this kernel
```

The tests find their processes by a unique `sleep <n>` command line and kill
them by pid. `session_bus.sh` starts one transient unit in the user's systemd
and stops it; its scratch folder is under `$TMPDIR`.
