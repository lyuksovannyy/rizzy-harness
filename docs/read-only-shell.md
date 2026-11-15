# The read-only shell

ADOPTION_PLAN T.7 and X.7; PLAN.md T66 and T70. Code: `Grants::read_only` and
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
- The only writable place is `/tmp`, a tmpfs in that namespace, 1 GiB
  by default (`[sandbox] read_only_tmp_mib` in node.toml, 64 to 65536;
  `READ_ONLY_TMP_BYTES` is the default): `TMPDIR` points there, it is
  empty when the command starts and gone when it ends, and nothing of it is
  on the node's disk. The session's own temp folder (which later commands
  read) is not granted.
- Landlock inside (`sandbox-exec`) gets the same lists: the folder only
  readable, `/tmp` writable. It is the second layer; the binds alone hold
  (`the_binds_hold_without_landlock`).
- `GIT_OPTIONAL_LOCKS=0`, so `git status` and `git diff` do not try to
  refresh `.git/index`; `CARGO_TARGET_DIR=/tmp/cargo-target`, so cargo
  builds and tests there.
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
into the folder (`npm install`) fails unless it can be told to write into
`$TMPDIR`. The tool's description says this (`harness_tools::read_only_note`,
with the node's size).

## Who gets it

Two users, one code path (`read_only_turn` in the agent loop, which sets
`Grants::read_only` per call):

- **Plan mode** (X.7, PLAN.md T67): every agent of the chat.
- **Read-only agents** (T.7, PLAN.md T70): an agent every other tool of
  which is on the shared read-only list (`harness_core::agent::READ_ONLY_TOOLS`,
  `AgentSpec::changes_things`), counting only what the node offers. Its
  `bash`, `bash_output` and `bash_stop` exist only in this shell, in every
  approval mode and both execution modes. The node decides from the tool
  list: there is no field that asks for a writable shell, and a spec that
  adds one is ignored (`cli/tests/read_only_agents.rs`). Built-in Scout and
  Reviewer have it (the Agents page's **Read-only commands** switch; off
  for a new custom agent). Such an agent still counts as a reader
  everywhere: it works in its parent's folder, qualifies for the small
  model, and may be started in plan mode. Its commands ask no card in
  `ask` and `accept_edits` (they change nothing, like `read`); `ask_all`
  asks. Where the node has no read-only shell it gets no `bash`, and the
  context block says why.

## Toolchains

What a test run needs to write, and what this shell gives it (measured on
this machine with bubblewrap 0.13 and a crate without dependencies, its
`Cargo.lock` present, `~/.cargo` and `~/.rustup` bound read-only):

- `cargo test` with no `CARGO_TARGET_DIR` fails (`target/` in the folder);
  with it in `/tmp` it builds and runs. Nothing else had to be writable:
  rustup and cargo read `~/.rustup` and `~/.cargo` without writing them.
  The shell sets it, so a model need not know. A repository without
  `Cargo.lock` fails (cargo writes one). Dependencies must already be in
  `~/.cargo/registry`: a download writes `~/.cargo`, which is read-only
  here, so it fails, network or not.
- `HOME` is left as it is: pointing it at `/tmp` would hide `~/.gitconfig`
  and `~/.cargo/config.toml` and gained nothing in the measurement.
- Size: `cargo test` of a crate using serde, serde_json and tokio (all
  features) wrote 159 MiB of `target/` (measured here), most of the old 256 MiB for three
  dependencies, which is why the default is now 1 GiB and the size is
  node.toml's (a larger project needs more).
- Node.js and Python toolchains were not measured; a test runner that keeps
  a cache in the folder (`node_modules/.vite`) may fail.
- What a hostile repository can then do: its build script and tests run,
  with every file read-only, `/tmp` writable and the network as
  `[sandbox] network` says. That is what any command of the shell can do
  already (it runs `bash`); nothing new is writable. It can read what the
  shell sees and send it out where the network is on (read-only is not
  secrecy), and use memory and CPU.

The toolchain folders are visible only where node.toml grants them
(`[sandbox] read = ["~/.cargo", "~/.rustup"]`, or `write`, which this shell
binds read-only). Other harnesses: Codex's `read-only` sandbox mode and its
guardian reviewer run commands with nothing writable (research round 2,
`codex.safety.md`), Antigravity's writable sandbox lists "common build
caches"; how either runs a build read-only is not in the research.

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
- `/tmp` is memory: its size per running command, charged where the node's
  own processes are (no cgroup per command), so a node with many background
  commands in plan mode can hold that much each.
- Toolchains that must write their home or the folder cannot build or
  test there (`npm install`, a cargo download of a missing dependency).
  The earlier note here, that cargo fails on a read-only `~/.rustup` even
  with `CARGO_TARGET_DIR` in `/tmp`, was not borne out when measured: what
  failed was `~/.rustup` missing from the view, not its being read-only.
- Writes outside the file system (a request to a local service listening on
  TCP where the network is on, a background command started before plan
  mode) are not file writes and are not stopped by this shell.
