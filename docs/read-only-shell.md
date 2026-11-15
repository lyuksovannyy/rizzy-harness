# The read-only shell

ADOPTION_PLAN T.7 and X.7; PLAN.md T66. Code: `Grants::read_only` and
`Confinement::read_only_shell` in `crates/tools/src/sandbox.rs`, the sized
`/tmp` in `crates/tools/src/contain.rs`; tests in
`crates/cli/tests/read_only_shell.rs`.

## What it is

`bash` (foreground and background) run so that it can look at everything a
normal command sees and change none of it:

- The chat's folder, node.toml's `[sandbox] read` **and `write`** entries,
  the system folders and a sub-agent's folder are bound **read-only** in the
  command's own mount namespace (bubblewrap's `--ro-bind`). The root is
  read-only, as for every contained command.
- The only writable place is `/tmp`, a tmpfs of 256 MiB
  (`READ_ONLY_TMP_BYTES`) in that namespace: `TMPDIR` points there, it is
  empty when the command starts and gone when it ends, and nothing of it is
  on the node's disk. The session's own temp folder (which later commands
  read) is not granted.
- Landlock inside (`sandbox-exec`) gets the same lists: the folder only
  readable, `/tmp` writable. It is the second layer; the binds alone hold
  (`the_binds_hold_without_landlock`).
- `GIT_OPTIONAL_LOCKS=0`, so `git status` and `git diff` do not try to
  refresh `.git/index`.
- The network is what the node's `[sandbox] network` says, as for any
  command.

Nothing reads the command. There is no parser and no list of safe commands:
other harnesses' plan-mode shells are blocklists that `python -c` gets past
(Cline's `command-guard.ts`, research/harnesses-round-2-appendix.md §4.5) or
a sentence in the prompt (OpenCode, Codex, research round 2 §3.3).

## What a command meets there

`touch`, `>`, `>>`, `sed -i`, here-docs into files, `mkdir`, `rm`, `mv`,
`chmod`, `touch -d`, `ln` (hard or soft), a write through `/proc/self/cwd`,
a symlink or hard link from `/tmp` into the folder, `mount`, a remount in a
new user namespace, `git add`, `commit`, `checkout`, `stash`, `update-ref`,
a write into a `[sandbox] write` entry, `python3 -c "open(...,'w')"`: each
fails, most with "Read-only file system". `ls`, `cat`, `grep`, `find`,
`git log`, `diff`, `status`, `show` work. A build or test runner that writes
into the folder (`cargo build`, `npm install`) fails unless it can be told
to write into `$TMPDIR`. The tool's description says this
(`harness_tools::READ_ONLY_NOTE`).

## Where it exists

Only where the node contains its shell with bubblewrap, and that bubblewrap
can size a tmpfs (`--size`, read from `bwrap --help`). It is the same
containment in both execution modes: a `host` chat's read-only command runs
contained too, which never gives it more than its mode.

Where there is no containment (the node offers no `sandbox` mode, `[sandbox]
mode = "off"`, a `host`-only node without bubblewrap) there is **no**
read-only shell, and `bash` is not offered where one is needed. Landlock
alone is not offered as read-only: it does not restrict `chmod`, `chown`,
`utime` or extended attributes (the kernel's Landlock documentation lists
them as not covered), so "read-only" would be a best effort. That differs
from ADOPTION_PLAN T.7's first wording ("a Landlock profile"); the plan row
says so.

Where the containment shares this machine's `/proc` (the `SharedProc`
form, inside a rootless container), `/proc/<pid>/root` of a process
outside leads past the binds; only Landlock stops that, so in that form
the read-only shell exists only with `[sandbox] mode = "required"` (tested
in `the_shared_proc_form_holds_through_landlock`).

Every sandboxed command, read-only or not, runs with core dumps off
(`RLIMIT_CORE` 0, soft and hard, set by `sandbox-exec` before the command):
the host's core pattern may pipe a dump to a handler outside the sandbox.

## Residuals

- A command can still read what any command of the chat reads, and send it
  out where the network is on: read-only is about changes, not secrecy.
- `/tmp` is memory: 256 MiB per running command, charged where the node's
  own processes are (no cgroup per command), so a node with many background
  commands in plan mode can hold that much each.
- Toolchains that need a writable home or build folder cannot build or test
  (`cargo` fails on a read-only `~/.rustup` even with
  `CARGO_TARGET_DIR=$TMPDIR`).
- Writes outside the file system (a request to a local service listening on
  TCP where the network is on, a background command started before plan
  mode) are not file writes and are not stopped by this shell.
