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
- The only writable place is a tmpfs in that namespace, 256 MiB by
  default (`[sandbox] read_only_tmp_mib` in node.toml, 64 to 65536):
  `TMPDIR` points there, it is empty when the command starts and gone when
  it ends, and nothing of it is on the node's disk. The session's own temp
  folder (which later commands read) is not granted. It is mounted at
  `/tmp`, unless something granted lies beneath `/tmp` (a chat whose
  folder is under the host's `/tmp`, as people clone there and the tests
  do): then it is at `/.harness-tmp`, and `/tmp` is a read-only empty
  folder (plus the path down to the chat's folder). A tool that hard-codes
  `/tmp` instead of reading `TMPDIR` gets "Read-only file system" there.
- Landlock inside (`sandbox-exec`) gets the same lists: the folder only
  readable, the tmpfs writable. It is the second layer for **every** grant:
  the binds alone hold (`the_binds_hold_without_landlock`), and Landlock
  refuses on its own what a read-only bind does not, such as `connect` to a
  pathname Unix socket in the folder (`LANDLOCK_ACCESS_FS_RESOLVE_UNIX`,
  Landlock ABI 9 per the kernel's `include/uapi/linux/landlock.h`; this
  machine's kernel 7.2 reports ABI 10). A Landlock rule covers everything
  beneath its folder, which is why the tmpfs never lies above a grant
  (`read_only_tmp_dir`; before the review of PR 131 a folder under `/tmp`
  sat beneath the tmpfs's write rule, and a socket in it was reachable).
- `GIT_OPTIONAL_LOCKS=0`, so `git status` and `git diff` do not try to
  refresh `.git/index`; `CARGO_TARGET_DIR=$TMPDIR/cargo-target`, so cargo
  builds and tests there.
- **No network** by default (`[sandbox] read_only_network = false`):
  the command gets a network namespace of its own (bubblewrap's
  `--unshare-net`), whose loopback works (a test suite's own server) and
  from which nothing outside is reachable, the host's loopback included.
  Where bubblewrap cannot make one, Landlock's TCP rules refuse TCP
  `connect` and `bind` instead (UDP is then open: see residuals).
  `read_only_network = true` gives it the network `[sandbox] network` says;
  it never widens that. The reason: these commands run without a card
  (plan mode, a read-only agent in `ask`) and may run the repository's own
  code (its tests, its build scripts), so a hostile repository under review
  would otherwise send out everything the shell can read.

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
with the node's size, that every command builds from nothing, and that
there is no network, pointing to `web_fetch` where the agent has it).

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
  with it in the temp folder it builds and runs. Nothing else had to be writable:
  rustup and cargo read `~/.rustup` and `~/.cargo` without writing them.
  The shell sets it, so a model need not know. A repository without
  `Cargo.lock` fails (cargo writes one). Dependencies must already be in
  `~/.cargo/registry`: a download writes `~/.cargo`, which is read-only
  here, so it fails, network or not.
- `HOME` is left as it is: pointing it at `/tmp` would hide `~/.gitconfig`
  and `~/.cargo/config.toml` and gained nothing in the measurement.
- Size: every command builds from nothing (the tmpfs is gone with the
  command before). `cargo test` of a crate using serde, serde_json and
  tokio (all features) wrote 159 MiB of `target/` (measured here): that
  fits the default 256 MiB, a crate with a few more dependencies does not,
  and this repository's own workspace does not fit in 1 GiB. Raise
  `read_only_tmp_mib` for builds, knowing the cost: up to that much **host
  memory per running read-only command, background ones included**
  (`background_per_node` allows 64), held as tmpfs pages that are in no
  process's RSS, so the OOM killer frees none of it by killing anything; it
  is returned when the command ends (`the_temp_folder_is_host_memory_until_the_command_ends`).
- Node.js and Python toolchains were not measured; a test runner that keeps
  a cache in the folder (`node_modules/.vite`) may fail.
- What a hostile repository can then do: its build script and tests run,
  with every file read-only, the temp folder writable and no network by
  default. A command of the shell runs code anyway (it is `bash`); what
  running the tests adds is that the repository's own code runs without a
  card. With `read_only_network = true` it can send out what the shell
  reads (read-only is not secrecy). It can use memory (the temp folder,
  as above) and CPU.

The toolchain folders are visible only where node.toml grants them. Grant
narrow ones: `[sandbox] read = ["~/.cargo/registry", "~/.cargo/bin",
"~/.rustup/toolchains"]`, not `~/.cargo`, which holds `credentials.toml`
(a registry token every uncarded read-only command could then read).
`write` entries are bound read-only in this shell. Other harnesses:
Codex's `read-only` sandbox mode and its guardian reviewer run commands
with nothing writable and no network (research round 2, `codex.safety.md`);
Claude Code's sandbox confines both the file system and the network, and
runs a sandboxed command without a prompt (round 1, `claude-code.md`
"Sandbox modes"); Antigravity's sandbox has no network by default and lists
"common build caches" as writable (round 1, `antigravity.md`); how any of
them runs a build read-only is not in the research.

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
  out where node.toml turns its network on: read-only is about changes,
  not secrecy.
- Without a network namespace (bubblewrap cannot make one here), only TCP
  is refused: UDP and DNS stay open, as for `network = "off"` on the
  Landlock-only path (T.6).
- Pathname Unix sockets in a granted folder are refused only by Landlock
  ABI 9 and later; on an older kernel a command can connect to a socket a
  host process listens on in the chat's folder (a read-only bind does not
  stop `connect`). Abstract sockets are refused from ABI 6.
- The temp folder is host memory: its size per running read-only command,
  background ones included, outside any process's RSS, until the command
  ends.
- Toolchains that must write their home or the folder cannot build or
  test there (`npm install`, a cargo download of a missing dependency).
  The earlier note here, that cargo fails on a read-only `~/.rustup` even
  with `CARGO_TARGET_DIR` in `/tmp`, was not borne out when measured: what
  failed was `~/.rustup` missing from the view, not its being read-only.
- Writes outside the file system (a request to a local service where
  node.toml turns the read-only shell's network on, a background command
  started before plan mode) are not file writes and are not stopped by this
  shell.
