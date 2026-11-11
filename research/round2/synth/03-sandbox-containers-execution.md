# Synthesis 03: sandboxing and where commands run

Round 2 source-code synthesis, written 2026-11-10. One feature group across thirteen repositories,
each pinned to the commit in `research/round2/00-repos.tsv`.

## 1. Scope

**The group.** Four topics:

- OS sandboxes for the commands an agent runs. That covers bubblewrap (`bwrap`, a namespace and
  mount tool), seccomp (a kernel syscall filter), Landlock (a kernel filesystem and network access
  control that a process applies to itself), macOS Seatbelt (`sandbox-exec` profiles) and the
  Windows mechanisms. For each: how the policy is built from configuration, what it covers, what
  happens when it is unavailable, and why that mechanism was chosen.
- The request a model makes for more access than the sandbox gives ("escalation").
- Container execution: the invocation each project builds, and who controls the image and mounts.
- Remote executors (SSH helpers, remote workspaces, exec servers), and how child processes are
  killed.

**Files read.** In `research/round2/`, these sections in full: the sandboxing and
"running commands somewhere else" sections of `codex.safety.md`, `deepseek-harness.safety.md`,
`openclaw.safety.md`, `hermes-agent.safety.md`, `oh-my-pi.safety.md`, `pi.safety.md`,
`goose.safety.md`, `opencode.safety.md`, `cline.safety.md`, `openhands.safety.md`, `zed.md`,
`warp.md` and `opencodex.md` (findings 29 to 37). The remote-worker sections of
`codex.sessions.md`, `deepseek-harness.sessions.md`, `openclaw.sessions.md`, `pi.sessions.md`,
`goose.sessions.md`, `opencode.sessions.md` and `openhands.agents-sessions.md`. The process and
kill findings in `codex.agents.md`, `cline.safety.md`, `goose.safety.md`, `oh-my-pi.safety.md`
and `pi.safety.md`. Section 8 (Review) of every one of these files, and section 4 (Round 1
claims) wherever it bears on this group. Also read: `research/harnesses-round-1.md` (the
sandbox lines), `docs/nodes-credentials-environments.md` §4.5 and §4.6, and the module header of
our own `crates/tools/src/sandbox.rs` on branch `integration/gate3` (pull request 4).

**No clone was opened.** No contradiction needed it: every conflict between input files had
already been settled by a reviewer (section 7 lists them).

**What was not available.**

- OpenHands: the per-conversation Docker runtime, the agent loop and the security analysers live
  in `OpenHands/software-agent-sdk`, which was not cloned.
- Warp: the agent loop and the self-hosted worker (`oz-agent-worker`) live elsewhere.
- Codex: its macOS Seatbelt profile and its Windows backends were not read beyond the dispatch
  code.
- DeepSeek harness: its Windows restricted-token code was not read.
- OpenCodex: its macOS and Windows helper (`native/remote-workspace-helper`) was not read.

**Trust labels.**

- `checked in review`: an independent reviewer opened the cited code and agreed.
- `corrected in review`: the reviewer changed the claim; the corrected version is given here.
- `unchecked`: the author read the code; no reviewer checked it.
- `inferred`: the author reasoned from the code without tracing the behaviour.
- `absence confirmed`: a reviewer searched for the thing and did not find it.

## 2. Implementations by project

### Codex

The only project with a full sandbox on all three platforms, and the only one with a seccomp
filter.

**Linux: two-stage helper (bubblewrap, then seccomp).** `checked in review`

1. The helper's outer stage builds a bubblewrap argument list and execs `bwrap`.
2. `bwrap` runs the same helper again with `--apply-seccomp-then-exec`. That inner stage:
   - checks the descriptor-backed mounts;
   - checks that no effective or permitted capabilities remain;
   - optionally starts the proxy bridge;
   - sets `no_new_privs` and installs seccomp;
   - forks and execs the command, forwarding signals.
3. The stated reason seccomp comes second: many `bwrap` installs are setuid, and `no_new_privs`
   would break them. Policies with full disk write and no managed network skip `bwrap` and only
   apply seccomp in-process.

Trace: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/linux_run_main.rs#L88-L94 (why seccomp is a second stage); https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/linux_run_main.rs#L165-L214 (inner stage); https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/linux_run_main.rs#L251-L321 (outer stage, "never falls back to legacy Landlock"); https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/seccomp.rs#L67-L75 (`no_new_privs` only when seccomp or legacy Landlock is needed).

If `/proc` cannot be mounted (some containers), a preflight `bwrap` run detects it and retries
without a fresh `/proc`, silently: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/linux_run_main.rs#L395-L428.

**The bubblewrap argument list.** `checked in review`

- Always: `--new-session --die-with-parent`, an explicit `--unshare-user` (so it also works as
  root in a container), `--unshare-pid` (unless the trusted executor asks to inherit it),
  `--unshare-ipc`, `--proc /proc` and `--cap-drop ALL`. `--unshare-net` is added unless network
  is fully enabled.
- Filesystem, full-read policies: start from `--ro-bind / /` plus `--dev /dev`.
- Filesystem, restricted-read policies: start from `--tmpfs /` and `--ro-bind` each readable root.
- Writable roots are re-bound with `--bind`, then protected subpaths are re-applied read-only.
- Deny-read paths and expanded deny globs are masked.

Trace: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/bwrap.rs#L228-L300 ; https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/bwrap.rs#L303-L384 ; https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/bwrap.rs#L386-L414 ; https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/bwrap.rs#L485-L566.

Two gaps:

- A writable root that does not exist is skipped silently.
- Deny globs are expanded once at launch, so a file created later is readable:
  https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/bwrap.rs#L470-L481.

**Seccomp, per mode.** `corrected in review`

There are three modes. Every denied call returns `EPERM`; the default action is allow, so this is
a denylist, not an allowlist.

| Mode | When | What it denies |
|---|---|---|
| `Restricted` | network off | `connect, accept, accept4, bind, listen, getpeername, getsockname, shutdown, sendto, sendmmsg, recvmmsg, getsockopt, setsockopt`; `socket` and `socketpair` for any family but `AF_UNIX`; `ptrace`, `process_vm_readv/writev`; all `io_uring` calls |
| `ProxyRouted` | managed proxy | allows only `AF_INET`/`AF_INET6` sockets inside the isolated namespace (`AF_UNIX` only with `dangerously_allow_all_unix_sockets`) |
| `VmSocketRestricted` | network on, filesystem restricted | only `AF_VSOCK` sockets and `io_uring` |

The stated reason for blocking `AF_VSOCK`: vsock reaches host services and, on WSL2, launches
Windows processes. `recvfrom` is left open on purpose, so tools that use socketpairs (cargo
clippy) work.

Reviewer correction: `sendmsg` and `recvmsg` are in no deny list. In `Restricted` mode a process
can still create an `AF_UNIX` datagram socket and send to a pathname datagram socket with an
addressed `sendmsg`. This was not tested. Stream sockets such as Docker and D-Bus need `connect`,
so they are blocked.

The filter supports x86_64 and aarch64 only; other architectures hit `unimplemented!`.

Trace: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/seccomp.rs#L53-L65 (mode choice, vsock reason); https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/seccomp.rs#L107-L128 (managed network fails closed); https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/seccomp.rs#L183-L301 (rule table).

**Why legacy Landlock is rejected.** `corrected in review`. What the code says and what was
inferred are separate here.

What the code says:

- The helper refuses legacy Landlock for any filesystem-restricted policy. It panics with
  "requires bubblewrap to isolate app-server sockets":
  https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/linux_run_main.rs#L386-L393.
  The test is named `legacy_landlock_cannot_bypass_daemon_socket_isolation`.
- The app-server daemon listens on a Unix socket whose real file lives in `/tmp/codex-daemon-<uid>`
  (mode 0700, owner checked). The control-socket acceptor authenticates nothing: it upgrades every
  connection to a WebSocket. The reviewer searched for `peer_cred` and `SO_PEERCRED` and found
  neither. Only the directory mode protects the socket:
  https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/uds/src/daemon_directory.rs#L1-L39 ; https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/unix_socket.rs#L148-L246 ; https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/unix_socket.rs#L353-L368.
- bubblewrap hides that directory with `--perms 000 --tmpfs <dir> --remount-ro <dir>` after every
  bind that would expose it. That includes bind-mount aliases found by parsing
  `/proc/self/mountinfo`. An unsupported alias or a nested mount aborts the launch:
  https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/bwrap.rs#L796-L830 ; https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/daemon_mounts.rs#L24-L61 ; https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/daemon_mounts.rs#L207-L219.
- The Landlock code is kept behind the deprecated feature `use_legacy_landlock` (ABI v5,
  best-effort, read everything, write only to the roots):
  https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/seccomp.rs#L1-L5 ; https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/seccomp.rs#L139-L174.

What was inferred, and is not in the code:

- "Landlock restricts filesystem access and does not block `connect()` to an existing Unix
  socket." That is the Round 2 author's general knowledge, not a code statement.
- "The daemon can spawn unsandboxed processes." The author drew this from the doc comment's
  phrase "privileged app-server RPC sockets"; it was not traced.

Related masks: the WSL interop directory `/run/WSL` is masked for the same reason:
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/bwrap.rs#L337-L349.
A matching Seatbelt rule exists at `seatbelt.rs` L1007-L1009 but was not read (`unchecked`).

**Which `bwrap` is run.** `checked in review`

- A `bwrap` found on `PATH` is used only if two conditions hold: its `--help` shows `--as-pid-1`
  and `--perms`, and it is not under the cwd or any path the policy lets the sandboxed process
  write.
- Otherwise the bundled `codex-resources/bwrap` is used. It is opened, its SHA-256 is compared to
  a digest compiled into the binary, and it is executed through `/proc/self/fd/N`.
- If neither exists, the helper panics and nothing runs.
- WSL1 is refused with `Wsl1UnsupportedForBubblewrap`.

Trace: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/launcher.rs#L41-L66 ; https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/launcher.rs#L200-L220 ; https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/sandboxing/src/bwrap.rs#L190-L207 ; https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/bundled_bwrap.rs#L66-L101 ; https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/bundled_bwrap.rs#L148-L156 ; https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/sandboxing/src/manager.rs#L865-L879.

**Policy selection, defaults and unavailability.** `unchecked` (the reviewer did not check the
policy-selection table)

- Backend per OS: `select_initial` picks Seatbelt on macOS and the `bwrap` helper on Linux. On
  Windows it picks the restricted-token or MXC sandbox only if enabled, otherwise `None`.
- A sandbox is required whenever network is disabled (unless an external sandbox is declared),
  whenever managed network rules exist, or whenever the filesystem is restricted.
- Defaults: `workspace-write` has `network_access = false`. `/tmp` and `$TMPDIR` are writable.
  `.git`, `.agents`, `.codex` and `.aws` directly under a writable root stay read-only.
- When the sandbox is unavailable: a missing Linux helper is an error. The exec-server refuses
  with "sandbox intent cannot be enforced on this executor".

Trace: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/sandboxing/src/manager.rs#L49-L63 ; https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/sandboxing/src/manager.rs#L331-L371 ; https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/sandboxing/src/policy_transforms.rs#L646-L666 ; https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/protocol/src/protocol.rs#L1294-L1316 ; https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/protocol/src/permissions.rs#L49-L54 ; https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/sandboxing/src/manager.rs#L539-L574 ; https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/process_sandbox.rs#L226-L230.

When the Windows sandbox is disabled, the rule fallback prompts instead of allowing
(exec_policy.rs L884-L902; `unchecked`).

**Network that is really isolated.** `unchecked`

- With managed networking, `bwrap` unshares the network namespace.
- Before launch, the outer helper forks one host-side bridge per proxy endpoint. It accepts only
  exact env keys, loopback endpoints and allow-listed ports.
- Inside the namespace, the inner stage brings up `lo`, binds a loopback listener and passes that
  listener to the host bridge over a socketpair (`SCM_RIGHTS`). It then rewrites the proxy env
  variable to the new port.
- The bridge writes an attribution frame first and caps concurrent connections at 128.
- A missing proxy env in managed mode is a hard error.
- With network on and the proxy off, egress is unrestricted.

Trace: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/proxy_routing.rs#L59-L121 ; https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/proxy_routing.rs#L131-L206 ; https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/proxy_routing.rs#L359-L414 ; https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/linux_run_main.rs#L430-L441.

**macOS.** Only one piece was read. When a proxy config exists but no loopback endpoint can be
inferred, Seatbelt returns an empty (deny) network section:
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/sandboxing/src/seatbelt.rs#L306-L361
(`unchecked`). The base profile was not read.

**Windows.** Not read beyond dispatch (`WindowsRestrictedToken`, `WindowsMxc`).

**Escalation: a sandbox denial leads to a second attempt that needs its own approval.**
`checked in review`

1. `ToolOrchestrator::run` gets approval, runs a first attempt in the sandbox, and only on
   `SandboxErr::Denied` considers a retry without the sandbox.
2. `OnRequest` and `Never` never retry without the sandbox, and nothing retries when denied reads
   exist.
3. Only a `User` or `Hook` approval carries over to the retry. A Guardian (the model reviewer)
   approval never does.

Trace: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/tools/orchestrator.rs#L164-L233 ; https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/tools/orchestrator.rs#L333-L360 ; https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/tools/orchestrator.rs#L446-L480 ; https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/tools/orchestrator.rs#L491-L501 ; https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/tools/sandboxing.rs#L349-L356.

"Denied" is a heuristic:

- Exit codes 0, 2, 126 and 127 are never a denial.
- On Linux, 128 plus SIGSYS is a denial.
- Otherwise the code matches keywords such as `permission denied`, `sandbox` and `seccomp`:
  https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/sandboxing/src/denial.rs#L13-L72.
- A `bwrap` start panic could match `sandbox`. Whether that ever produces a retry offer is not
  settled (`inferred`).

**An `allow` rule removes the sandbox.** `checked in review`

- When every parsed segment of a command matches an explicit `allow` rule, the first attempt
  bypasses the sandbox. The only exception is a policy with denied reads.
- "Accept and remember" writes such a rule, so it also grants unsandboxed execution for that
  prefix from then on.

Trace: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/exec_policy.rs#L492-L512 ; https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/tools/sandboxing.rs#L253-L298 ; https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/exec_policy.rs#L516-L564.

**Remote exec-server: the executor has no policy of its own.** `checked in review` (the absence
of an executor-side ceiling was confirmed in review)

- `codex exec-server` is a JSON-RPC tool host: `process/start|read|write|signal|terminate`,
  `fs/*`, `http/request`, skill discovery. The agent loop stays with the harness.
- `process/start` carries `sandbox: Option<FileSystemSandboxContext>`, a "portable sandbox
  intent". The executor turns it into its own platform's wrapper.
- A request without `sandbox` runs unsandboxed. The policy (which paths are writable) comes from
  the caller, and nothing lets the executor's owner cap it. The reviewer found executor-local
  requirements only for MCP.
- Credentials in the environment are masked before spawn (failure mode `Warn`).

Trace: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server-protocol/src/protocol.rs#L25-L65 ; https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server-protocol/src/protocol.rs#L374-L414 ; https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/sandbox_selection.rs#L21-L46 ; https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/process_sandbox.rs#L43-L45 ; https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/process_sandbox.rs#L142-L172 ; https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/process_sandbox.rs#L215-L230.

How the channel is authenticated:

- Over the relay, it uses Noise hybrid IK with a registry-pinned executor key and registry
  authorisation of the harness key:
  https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/noise_channel.rs#L1-L11.
- With direct `--listen ws://IP:PORT`, authentication is optional. Unlike the app-server, this
  listener does not refuse an unauthenticated non-loopback bind (`corrected in review`):
  https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server-cli/src/command.rs#L56-L160.

No container, VM or SSH backend exists. The `SandboxType` variants are `None`, `MacosSeatbelt`,
`LinuxSeccomp`, `WindowsRestrictedToken` and `WindowsMxc` (`unchecked`).

**Processes.** `checked in review`

- Unified exec keeps at most 64 processes. It prunes the oldest-used exited process first and
  protects the 8 most recently used. A pruned process is terminated:
  https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/unified_exec/process_manager.rs#L1774-L1800 ; https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/unified_exec/process_manager.rs#L1302-L1305.
- Sandboxed children are tied to the helper by `--die-with-parent`, and the PID namespace hides
  the parent.
- `pre_main_hardening` (non-dumpable, core size 0) is not applied to the main binary
  (`absence confirmed`).

### OpenCode

**No OS sandbox.** `checked in review`

- A search for `sandbox-exec`, `bwrap`, `landlock`, `seatbelt`, `seccomp`, `firejail` and
  `unshare` found nothing relevant.
- Commands run as the host user with the full environment:
  https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell.ts#L370-L382.
- Path containment for the file tools is lexical (`path.relative`), so a symlink inside the
  project that points outside counts as inside:
  https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/util/src/fs-util.ts#L269-L272.
- Plan mode does not deny `shell`, so "read-only" is a prompt, not a mechanism:
  https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/plan.ts#L33-L53.

**Workspace drivers: a seam with nothing behind it.** `checked in review`

- An `Environment` (file access plus a process spawner) can come from a registered
  `WorkspaceDriver` with `create`, `connect`, `suspendForIdle` and `destroy`.
- The default registry is empty; only SDK embedders supply drivers.
- Permissions are still evaluated in the server; the driver only executes.
- When a workspace is bound, shells start with no inherited host environment.
- No container, VM or SSH backend ships.

Trace: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/environment/environment.ts#L18-L35 ; https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/workspace/driver.ts#L26-L62 ; https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell.ts#L331-L346.

**SSH remote (desktop "Add SSH server").** `checked in review`

- The desktop runs `ssh` and installs or finds `opencode` on the remote. It then `cat`s the
  service file to learn the loopback URL and password, and tunnels with `ssh -L`.
- The binary is verified only by its `--version` string.
- SSH options are filtered to an allow-list.
- The remote side runs the same server code, so it has a full policy of its own. It is a whole
  server, not an executor.

Trace: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/gui-plugins/src/ssh/remote-cli.ts#L120-L122 ; https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/gui-plugins/src/ssh/command.ts#L48-L134.

**Processes.** Persistent terminals live in a Rust PTY daemon (`opencode-pty`) that survives
server restarts:
https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/process-daemon/Cargo.toml#L1-L16
(`checked in review`). How agent shell children are killed was not in the input.

### OpenClaw

**Sandbox: off by default, Docker by default.** `checked in review`

- Defaults: `mode` `off`, `workspaceAccess` `none`, `backend` `docker`.
- With `ro` or `rw` access, the agent workspace is mounted at `/agent`.

Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/config.ts#L212-L227 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/workspace-mounts.ts#L179-L202.

The architecture section names the CLI as "the Docker or Podman CLI", driven by
`buildSandboxCreateArgs`:
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/container-allocation.ts#L54-L75.

**The `docker create` invocation.** `checked in review`

- Always added; configuration cannot remove these:
  - `--init`;
  - identity labels (`openclaw.sandbox`, the session key, `configHash`, `createArgsEpoch`);
  - `--security-opt no-new-privileges`;
  - `--pull=never` (Docker);
  - `--env-file`.
- Added by default, configurable:
  - `--read-only`;
  - tmpfs `/tmp` 1 GB, `/var/tmp` and `/run` 256 MB each;
  - `--network none`;
  - `--cap-drop ALL`;
  - `--pids-limit 1024`;
  - `ulimit core=0`.
- Optional: `user`, memory, CPUs, GPUs, seccomp and AppArmor profiles, DNS, extra hosts.
- The container runs `sleep infinity`, and commands are exec'd into it.
- The created id must be a 64-hex immutable id.
- A change to the create flags bumps `SANDBOX_DOCKER_CREATE_ARGS_EPOCH`, so old containers are
  recreated rather than kept with stale flags.
- Default image: `openclaw-sandbox:trixie-slim`.

Two `inferred` points: no default seccomp profile is supplied, so the engine's default applies;
`user` is unset, so the image's user applies.

Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/config.ts#L91-L117 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/container-allocation.ts#L71-L167 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/container-allocation.ts#L214-L273 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/constants.ts#L10 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/constants.ts#L62-L64.

**Who controls the image and the mounts, and what is validated.** `checked in review`

The image, binds and network come from `agents.defaults.sandbox.docker` and per-agent overrides
in `openclaw.json`. With `scope: shared`, the per-agent overrides are dropped.

At create time, `validateSandboxSecurity` rejects:

- bind sources that are not absolute;
- `/`, `/etc`, `/proc`, `/sys`, `/dev`, `/root`, `/boot` and `/run`;
- Docker socket paths;
- `~/.aws`, `.cargo`, `.config`, `.docker`, `.gnupg`, `.netrc`, `.npm` and `.ssh`, whether the
  source sits inside one of them or covers it;
- sources outside the workspace roots;
- targets over `/workspace` or `/agent`;
- `--network host` and `container:*`;
- seccomp or AppArmor `unconfined`.

Each check runs lexically and again on the real path of the nearest existing ancestor. Three
`dangerously*` booleans relax the roots, the reserved targets and namespace joins. The file
states its threat model as "local-trusted config, but protect against foot-guns and config
injection".

Who can change the config: the agent's `gateway` tool cannot write it. An agent running host exec
with `security=full` could still edit `openclaw.json` directly (`inferred`).

Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/validate-sandbox-security.ts#L23-L53 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/validate-sandbox-security.ts#L93-L132 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/validate-sandbox-security.ts#L274-L356 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/config.ts#L245-L258 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/tools/gateway-tool.ts#L77.

**A safety check that is skipped inside the sandbox.** `corrected in review`

- The default security value for a sandbox target is `deny`, but that does not block sandboxed
  exec. The pre-run check throws only when the host is not the sandbox, or when `deny` was set
  explicitly.
- So a default-configured sandboxed agent runs its commands with no approval.
- A session marked `sandbox: required` throws if the sandbox is unavailable, and refuses the
  `elevated`, `gateway` and `node` targets.

Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/bash-tools.exec-run.ts#L309-L317 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/bash-tools.exec-target.ts#L45-L66.

**Environment in the sandbox.** `checked in review`

- Sandbox runs get only config-declared variables, value-checked:
  https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/sanitize-env-vars.ts#L113-L118.
- Host exec inherits the Gateway's environment minus injection variables, which includes
  provider keys:
  https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/host-env-security.ts#L38-L65.

**Browser sandbox.** `unchecked`

- The browser container has its own bridge network with egress, and publishes CDP and noVNC on
  random `127.0.0.1` ports.
- CDP is protected by a 24-byte random auth token, and noVNC by a generated password.
- Chrome runs with `--no-sandbox`. The browser is disabled by default.

Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/browser.ts#L362-L411.

**SSH sandbox backend.** `unchecked`

- `strictHostKeyChecking` and `updateHostKeys` default to true.
- The remote root must be an absolute POSIX path (default `/tmp/openclaw-sandboxes`).
- Children get `sanitizeEnvVars`.

Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/config.ts#L162-L198 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/ssh.ts#L149.

**Executor-side policy (paired nodes).** `checked in review`

Both sides check:

- The Gateway requires that a command is in its allow-list, is not private, and is declared by
  the node.
- For `system.run`, the node recomputes `security` and `ask` from its own config and approvals
  file.
- The node refuses an approval that lacks a `systemRunPlan` exactly matching argv, command text,
  cwd, agentId and sessionKey.
- The node refuses a delayed approval if its policy snapshot has changed since the request.

But a Gateway-forwarded `approved === true` satisfies both "ask" and "allowlist miss" on the
node. Only a local `security=deny` holds against it.

Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/node-command-policy.ts#L460-L480 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/invoke-system-run.ts#L121-L141 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/invoke-system-run.ts#L233-L264 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/invoke-system-run.ts#L349-L357 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/exec-policy.ts#L60-L117.

**Cloud workers.** `checked in review`

- A closed dispatcher with a fixed method list. Every frame re-checks the credential hash, the
  environment state and the owner epoch. Expiry is the exception: it is waived for an exact
  durable turn.
- The worker dials the Gateway, but the Gateway also SSHes into the worker box for bootstrap and
  workspace transfer, with pinned options (`-F none`, `StrictHostKeyChecking=yes`,
  `IdentityAgent=none`, no forwarding).

Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/ws-connection/worker-connection-dispatch.ts#L90-L149 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/ssh.ts#L259-L291.

**No OS sandbox for host exec:** nothing in the input describes one, and no reviewer ran a
search for one, so this is not an `absence confirmed`. How child processes are killed was not in
the input.

### Hermes Agent

**No OS sandbox on the local backend.** `absence confirmed`

- The local backend spawns `bash [-l] -c <cmd>` with `start_new_session=True`.
- The reviewer found no `bwrap`, `landlock`, `seatbelt` or `seccomp` in `tools`, `agent`,
  `hermes_cli`, `gateway` or `cron`.
- The process group goes to an owner-death supervisor, so children die with Hermes.

Trace: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/local.py#L1149-L1168.

**Docker backend.** `checked in review`

- Creation: `docker run -d [--init] --name hermes-<8hex> --label … -w <cwd> <args> <image> sleep infinity`.
- Each command: `docker exec [-i] -e NAME… <id> bash -c …`.
- Fixed flags:
  - `--cap-drop ALL`, then adds back `DAC_OVERRIDE`, `CHOWN` and `FOWNER`;
  - `--security-opt no-new-privileges`;
  - tmpfs `/tmp` (512 MB, nosuid, exec), `/var/tmp` (256 MB, noexec), `/run` (64 MB, noexec);
  - `SETUID` and `SETGID`, unless `--user` is given;
  - when cgroups allow it: `--pids-limit 2048`, `--cpus 1`, `--memory 5120m`.
- `docker_extra_args` are appended last, so the operator can override anything.

Trace: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/docker.py#L321-L343 ; https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/docker.py#L400-L409 ; https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/docker.py#L814-L848 ; https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/docker.py#L741-L745 ; https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/docker.py#L1022-L1037 ; https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/docker.py#L1100-L1127.

Mounts, user and network:

- Persistent mode (the default) bind-mounts `~/.hermes/sandboxes/docker/<task>/{home,workspace}`
  to `/root` and `/workspace`. The process runs as root inside, with `DAC_OVERRIDE`.
- The host cwd is mounted only with `docker_mount_cwd_to_workspace`.
- Skills, credential files and cache directories are mounted read-only.
- Network is on by default. `docker_network: false` adds `--network=none`.
- `docker_snap_compat` drops `--init` and `no-new-privileges`.
- Containers are reused across processes by label.

Trace: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/docker.py#L903-L923 ; https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/docker.py#L552-L575 ; https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/docker.py#L747-L766.

Environment values travel in the docker client's own environment, with name-only `-e KEY`, which
keeps them out of `/proc/*/cmdline`:
https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/docker.py#L539-L542.
The operator opt-in `docker_forward_env` bypasses the credential blocklist:
https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/remote_common.py#L23-L62.

**Safety checks skipped inside containers.** `checked in review`

- `_should_skip_container_guards` returns early for `docker` without host access, and always for
  `modal`, `daytona`, `vercel_sandbox` and flagged plugin backends.
- On that path only the user's deny globs run. The hardline floor, the sudo guard and the
  dangerous-pattern prompts are all skipped. The docstring's reason: the container is the
  boundary.
- "Host access" means a mounted host cwd or a `docker_volumes` entry that is a host path. A
  `-v /:/host` in `docker_extra_args` is not inspected (confirmed in review).

Trace: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval.py#L1161-L1175 ; https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval.py#L1282-L1287 ; https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/terminal_tool.py#L145-L162.

**Egress proxy (iron-proxy), wired into Docker only.** `checked in review`

- The proxy holds the real keys; the container gets tokens under the provider names. The proxy
  binds the `docker0` address or `127.0.0.1`, never `0.0.0.0`.
- Collision guards reject `docker_extra_args` and env entries that would override the proxy
  settings (`-e`, `--env-file`, `--network`).
- Egress goes only through proxy environment variables. The container stays on the default
  bridge network, so a process that ignores `HTTPS_PROXY` reaches the internet directly, though
  it holds only useless tokens.

Trace: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/docker_egress.py#L29-L76 ; https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/docker_egress.py#L180-L211 ; https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/proxy_sources/iron_proxy.py#L400-L430.

**SSH backend.** `checked in review`; the write-back into skills is `inferred`

- Every call is a new
  `ssh -o ControlMaster=auto -o ControlPersist=300 -o BatchMode=yes -o StrictHostKeyChecking=accept-new … bash -c '<quoted>'`.
- Skills and credential files are pushed to the remote before each command.
- On cleanup it tars the remote `~/.hermes` back and writes changed or new files into the host,
  last write wins. A compromised remote could add files under the host's skills tree.
- The ControlMaster socket directory `/tmp/hermes-ssh` is created with default permissions.
- The remote side has no policy of its own.

Trace: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/ssh.py#L122-L134 ; https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/ssh.py#L298-L309 ; https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/file_sync.py#L484-L496 ; https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/ssh.py#L71-L81.

**Cloud sandboxes (Modal, Daytona).** `unchecked`

- Modal: `Sandbox.create("sleep", "infinity", …, timeout=3600)` plus `sandbox.exec("bash", "-c", cmd)`.
- Daytona: `process.exec`.
- Neither forwards passthrough env or wires in the egress proxy.
- Daytona cancel stops the whole sandbox, which is restarted before the next command.

Trace: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/modal.py#L157-L165 ; https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/daytona.py#L162-L184.

**Processes.** Stdio MCP children of failed attempts are reaped with SIGTERM, wait, then SIGKILL.
On Windows the process is attached to a kill-on-close job object (`hermes-agent.sessions.md`
L353, `unchecked`).

### Oh My Pi

**No OS sandbox.** `absence confirmed`

- The reviewer's `git grep` for landlock, seccomp, bwrap, seatbelt, firejail and nsjail found
  only unrelated comments.
- `pi-iso` provides copy-on-write worktrees for merging, not confinement.
- `file-write-fallback.ts` is a seam for a host that embeds omp inside its own sandbox:
  https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/file-write-fallback.ts#L1-L15.
- Child shells inherit the whole environment, provider keys included:
  https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/utils/src/procmgr.ts#L43-L55
  (`checked in review`).

**SSH is file access only.** `checked in review`

- `ssh://` paths spawn the system `ssh` with `ControlMaster=auto`, `ControlPersist=3600`,
  `BatchMode=yes` and `StrictHostKeyChecking=accept-new`.
- There is no remote command tool.
- Project `ssh.json` files can define hosts with `${VAR}` expanded from the environment. That
  could leak a secret into a hostname (`inferred`).

Trace: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/ssh/connection-manager.ts#L193-L209 ; https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/discovery/ssh.ts#L98-L104.

**Processes.** `unchecked` (the reviewer did not open the kill code)

- `pi-shell` signals SIGTERM to the process group, to every signalable descendant and to the
  root. It waits a grace period, re-walks the tree to catch new or re-parented grandchildren,
  then sends SIGKILL.
- Children start in their own session (`setsid`).
- A child that double-forks and changes session escapes the walk (`inferred`; no cgroup or pidfd
  use was found).

Trace: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-shell/src/proctree.rs#L843-L884.

Timeouts (`checked in review`): the default deadline is 300 s, clamped to 1 to 3600 s.
`timeout: 0` from the model disables it unless `tools.maxTimeout` caps it:
https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/sdk.ts#L4814-L4824.

### Pi

**No sandbox in the CLI.** `absence confirmed`

- The shell tool is `spawn(shell, ["-c", command], {cwd, detached: true, env})`.
- No namespace, seccomp, Landlock, Seatbelt or network control exists.
- The file tools resolve any path.
- Pi's stated reason, in its documentation and SECURITY policy: prompts and heuristics are
  theatre, so containment is the user's job.

Trace: https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/tools/bash.ts#L124-L131 ; https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/tools/path-utils.ts#L48-L50.

**Containers: documentation only.** `absence confirmed`

- Plain Docker, Docker Sandboxes and OpenShell appear only as instructions. The whole Pi process
  runs in the container, and keys are passed as `-e`.
- The image, mounts and flags are entirely the user's.

Trace: https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/docs/containerization.md#L9-L16.

**Gondolin example extension (micro-VM).** `checked in review`

- Built-in tools move into a QEMU micro-VM. The host cwd is mounted at `/workspace`.
- `sanitizeEnv` drops only non-string values, so host keys enter the VM.
- No network policy, secret mapping or read-only mount is configured.

Trace: https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/examples/extensions/gondolin/index.ts#L325-L358 ; https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/examples/extensions/gondolin/index.ts#L380-L390.

**`ssh.ts` example.** `unchecked`. It uses bare `ssh <remote> <command>` with no `BatchMode`, no
host-key policy and no forwarding restrictions:
https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/examples/extensions/ssh.ts#L29-L45.

**pi-env (remote executor, used only by the prototype `apps/p2`).** `checked in review`

SSH flags:

- every call: `-T -a -x`, `BatchMode=yes`, `ClearAllForwardings=yes`, `ForwardAgent=no`,
  `ForwardX11=no`, `ControlMaster=no`, `ControlPath=none`, `RemoteCommand=none`,
  `PermitLocalCommand=no`, `SendEnv=-*`, `StrictHostKeyChecking=yes`;
- an app-owned `UserKnownHostsFile`, `GlobalKnownHostsFile=none` and a fixed `HostKeyAlias`;
- `--` before the host.

Host keys:

- Trust on first use is an explicit step: `scanHostKey` probes with `accept-new` into a throwaway
  file and returns fingerprints for the owner to compare.
- A changed host key is never accepted automatically.

Trace: https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/env/src/ssh.ts#L59-L123 ; https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/env/src/ssh.ts#L164-L171.

The daemon accepts any path and any command and has no policy of its own:

- Paths go straight to the OS.
- `exec` takes a shell string or argv, a cwd, an env map and `inheritEnv` (default true), and
  spawns with `setsid`.
- The reviewer's grep found no seccomp, namespace, chroot or rlimit code in the daemon.
- The `--token` is only a marker for skipping shell banners. The daemon never checks it; the
  boundary is the SSH login.

Trace: https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/env/daemon/src/main.rs#L137-L282 ; https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/env/daemon/src/exec.rs#L78-L109 ; https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/env/src/connection.ts#L344-L345.

Liveness: both sides ping every 5 s. The daemon kills every process group it started when stdin
ends or after 30 s of silence:
https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/env/daemon/src/main.rs#L532-L547 ; https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/env/daemon/src/main.rs#L556-L558.

Deployment (`unchecked`): the daemon binary is named by content hash and verified before each
start, but checking the hash and executing are separate steps:
https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/env/src/ssh.ts#L403-L464.

**Processes in the CLI.** `checked in review`

- On abort or timeout, the whole process group gets SIGKILL. On macOS it is first stopped with
  SIGSTOP, to avoid a race with `a; b`.
- Detached children are tracked and killed when Pi gets SIGHUP or SIGTERM. If Pi itself is
  SIGKILLed, they survive.
- There is no default timeout.

Trace: https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/tools/bash.ts#L145-L158 ; https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/utils/shell.ts#L196-L215.

### DeepSeek harness

**The backend chain, per platform, failing closed.** `checked in review`

- `PLATFORM_CHAINS` is `linux: [bwrap, landlock]`, `darwin: [seatbelt]`, `win32: [windows-acl]`.
- A longer chain is probed in order: the real read-only profile actually runs `true`, and the
  first backend that passes wins. The verdict is cached.
- When nothing passes, it throws `SANDBOX_UNAVAILABLE` and the command never runs.
- A runner that starts and then refuses is recognised by its stderr signature and exit code
  (Landlock exit 125). That becomes `SANDBOX_UNAVAILABLE`, not a command failure.
- Denials are classified in each backend's own dialect: EROFS for bwrap, EACCES for Landlock,
  EPERM for Seatbelt.
- An operator `runnerCommand` skips selection and asserts `full`.
- Stated reason bwrap comes first on Linux: its mount profile is "closest to the mode
  vocabulary".

Trace: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/sandbox/sandbox-local/src/index.ts#L151-L189 ; https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/sandbox/sandbox-local/src/index.ts#L68-L113 ; https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/sandbox/sandbox-local/src/index.ts#L499-L554 ; https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/sandbox/sandbox-local/src/index.ts#L220-L242 ; https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/sandbox/sandbox-local/src/index.ts#L202-L215 ; https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/shell/bash-sandbox/src/index.ts#L115-L132.

**What each profile covers: writes only.** `checked in review`

| Backend | Profile |
|---|---|
| bwrap | `--ro-bind / / --dev /dev --unshare-pid --proc /proc --die-with-parent`, plus `--tmpfs /tmp` and `--bind root root` per writable root |
| Landlock | read and execute beneath `/`; read-write on `/dev/null`, `/tmp` and the roots |
| Seatbelt | `(allow default) (deny file-write*)`, with writes allowed to `/dev/null` and the roots (the real `/tmp` and `os.tmpdir()` included) |

None of the three restricts reads, network, IPC or Unix sockets:

- bwrap has no `--unshare-net`.
- Landlock declares no network rights.
- Seatbelt allows everything by default.

Other gaps:

- There are no protected subpaths, so `.git/hooks` is writable.
- `/tmp` is a private tmpfs under bwrap but the shared host `/tmp` under Landlock and Seatbelt.
- Unix-socket connects are not blocked (`inferred`; no rule addresses them).

Trace: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/sandbox/sandbox-local/src/profiles.ts#L11-L60 ; https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/sandbox/sandbox/src/roots.ts#L70-L100.

**The Landlock launcher (`landlock-run`, static musl C).** `checked in review`

1. It creates a ruleset, adds `--ro` paths with read and execute and `--rw` paths with every
   handled right, sets `PR_SET_NO_NEW_PRIVS`, calls `landlock_restrict_self` and `execvp`s the
   command.
2. Any failure (an unopenable grant path, no Landlock, a failed syscall) exits 125 with a
   `landlock-run: ` line, and the command does not run.
3. A file grant keeps only file-compatible bits.
4. No `handled_access_net` and no scope flags are set.

Trace: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/native/system/packages/entry/src/main.c#L1-L36 ; https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/native/system/packages/entry/src/main.c#L193-L218 ; https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/native/system/packages/entry/src/main.c#L230-L262.

**Enforcement level, computed but shown to nobody.** `checked in review`

- bwrap and Seatbelt are `full` by construction. Windows ACL is always `partial`.
- Landlock is `partial` when the kernel ABI is below `MAX_ABI = 5`.
- So on ABI 6 or later it reports `full` while not using the newer rights (abstract-socket and
  signal scoping). "Full" means "every filesystem right this build knows".
- The value is stored on each confined `bash` result as `sandbox.enforcement`. The model-facing
  render prints only a denial marker, and no client, API or startup code reads `enforcement`.

Trace: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/native/system/packages/entry/src/main.c#L70-L94 ; https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/native/system/packages/entry/src/main.c#L264-L297 ; https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/sandbox/sandbox-local/src/index.ts#L526-L554 ; https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/shell/tool-bash/src/render.ts#L40-L50.

**Windows.** `unchecked`

- A restricted token with low integrity.
- The write capability is a SID derived from the canonical workspace path. It is granted once
  per workspace and left standing, so the ACEs persist across restarts by design.
- Each session gets a random private temp directory with its own SID, revoked on dispose.
- Always `partial`, and `additionalWritableRoots` is rejected.

Trace: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/sandbox/sandbox-local/src/index.ts#L369-L457 ; https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/sandbox/sandbox-local/src/index.ts#L459-L491.

**The in-process fence for the file tools.** `checked in review`

- `SandboxedFileSystem` fences write, edit, patch, delete and move. It re-resolves the path just
  before the change and checks containment lexically, then by device and inode.
- Reads are not fenced.
- It shares `writableRoots()` with the Seatbelt profile. So under bwrap the write tool can write
  the host `/tmp` while bash sees a private tmpfs.

Trace: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/fs/fs-sandbox/src/index.ts#L145-L178.

**Escalation: one-shot, strictly wider, bound to the call that asked.** `checked in review`

- `bash` and the fs mutation tools advertise `sandbox_permissions`
  (`workspace-write | danger-full-access`) plus a required `justification`.
- The requested mode must be strictly wider than the effective mode.
- On `allowed-once`, the wider policy is stamped onto that one call and nothing is stored.
- Nothing checks that the call was denied first.
- `bash` rejects `sandbox_permissions` when no confining executor exists.

Trace: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/sandbox/sandbox/src/escalation.ts#L23-L42 ; https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/sandbox/sandbox/src/escalation.ts#L172-L206 ; https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/shell/tool-bash/src/index.ts#L478-L497 ; https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/shell/tool-bash/src/index.ts#L228-L248.

Sub-agents inherit the parent's sandbox override and have approval pinned to `never`, so they
cannot escalate:
https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/subagent/src/child-agent.ts#L238-L256
(`checked in review`).

**SSH remote: the loop stays local, a helper runs files, processes and the sandbox.**
`checked in review`

SSH and the protocol:

- `ssh -T -M -S <ctl> -o ControlPersist=no -o BatchMode=yes -o StrictHostKeyChecking=yes -o ForwardAgent=no -o ClearAllForwardings=yes -o ServerAliveInterval=10 -o ServerAliveCountMax=3 <alias> <helper>`.
- The alias must match `^[a-zA-Z0-9][a-zA-Z0-9_.@-]*$`.
- Streams use TLS 1.2 PSK with a per-stream 256-bit key.

The remote's own policy:

- The SSH sandbox provider forwards `confine` to the helper, which runs the remote's own local
  provider chain. Any error maps to `SANDBOX_UNAVAILABLE`. So the executing side does apply its
  own platform sandbox.
- The helper's hash check is self-attestation: the helper hashes its own file. The remote's
  reported `enforcement` is trusted as sent.

Liveness:

- The helper holds a lease, with a heartbeat every third of it (default 30 s). On expiry it
  cleans up the remote processes it manages.
- Loss of the connection is final.

Trace: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/ssh/ssh/src/index.ts#L279-L328 ; https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/ssh/sandbox-ssh/src/index.ts#L9-L31 ; https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/ssh/ssh/src/helper.ts#L155-L161 ; https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/ssh/ssh/src/helper.ts#L103-L119 ; https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/ssh/ssh/src/helper.ts#L60-L90.

**No container or VM backend.** The author searched the sandbox and shell groups for docker and
podman wrappers and found none (`unchecked`).

**Secrets readable inside the sandbox.** `inferred`. The keys that the environment scrub hides
sit in `$DSH_HOME/.credentials.yaml` (mode 0600). Any confined command can read that file,
because no profile restricts reads.

### goose

**No OS sandbox; the macOS Seatbelt sandbox was removed.** `absence confirmed`

- The only hit for the sandbox terms is a blog post with an "Outdated" banner saying the
  experimental Seatbelt sandbox "has been removed".
- The Developer shell runs `<shell> -c <line>`.
- The only "sandbox" code is Flatpak detection, which wraps commands in
  `flatpak-spawn --host --watch-bus` to escape the Flatpak sandbox.

Trace: https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/documentation/blog/2026-02-23-goose-v1-25-0/index.md#L19-L34 ; https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/developer/shell.rs#L575-L590 ; https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/developer/shell.rs#L26-L53.

**`--container` covers MCP extensions, not the shell.** `checked in review`

- Stdio extensions start as `docker exec -i [-e K=V ...] <id> <cmd>`.
- Developer, which holds `shell`, `write` and `edit`, is a platform extension. It is built
  in-process first and runs on the host.
- goose does not create the container, choose the image or set mounts. The user supplies the id.
- Extension env values, often API keys, appear on the `docker exec` command line, where the
  host's process list shows them.

Trace: https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/extension_manager/mod.rs#L932-L948 ; https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/extension_manager/stdio.rs#L40-L53.

**Processes.** `unchecked`

- Unix: `process_group(0)`, and termination is `kill(-pgid, SIGKILL)`.
- Windows: the shell is created suspended, assigned to a Job Object with `KILL_ON_JOB_CLOSE`,
  then resumed.
- `Drop` kills the tree. After a normal exit the tree is disarmed, so deliberately backgrounded
  work survives.
- A `setsid` child escapes the kill.
- The shell gets no `PR_SET_PDEATHSIG`, so if goose crashes its commands keep running. MCP
  servers do get it, from a dedicated spawner thread.

Trace: https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/developer/shell_process.rs#L11-L49 ; https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/developer/shell_process.rs#L51-L92 ; https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/developer/shell_process.rs#L127-L151 ; https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/subprocess.rs#L11-L28.

The model sets `timeout_secs`, and `0` disables the deadline (`checked in review`):
https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/developer/shell.rs#L424-L426.

**Remote executors.** None. The only topology is a client dialling `goose serve`, or an iroh
peer (`checked in review`).

### Cline

**No sandbox; `CLINE_SANDBOX` only moves the data directory.** `checked in review` (the absence
is by grep)

- `CLINE_SANDBOX=1` or `--data-dir` points the data paths at `$TMPDIR/cline-sandbox` and forces
  the local backend. It changes nothing about what tools may do.
- The author searched for `sandbox-exec`, seatbelt, bwrap, landlock, seccomp, firejail,
  AppContainer, nsjail, gVisor and docker launching, and found none in the product path.
- `--yolo` warns "use only in sandboxed environments".

Trace: https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/cli/src/utils/helpers.ts#L583-L605 ; https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/cli/src/commands/program.ts#L29-L32.

The "sandbox" plugin mode is crash isolation only. Both the worker and the subprocess get the
full environment (`unchecked`):
https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/runtime/tools/subprocess-sandbox.ts#L308-L327.

**SSH remote: the loop and tools run on the remote host.** `checked in review`

1. The local `ssh` runs with `BatchMode=yes`, `ConnectTimeout=30` and
   `StrictHostKeyChecking=yes`. One-shot commands add `ClearAllForwardings=yes`.
2. It uploads a helper CLI to `~/.cline/remote` and starts a loopback hub there.
3. It tunnels with `ssh -N -L` and does not set `ExitOnForwardFailure`.
4. Each session start sends the provider key.
5. The remote runs its own Cline with its own settings, so it is a full agent, not an executor.

Trace: https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/remote/remote-environments.ts#L704-L732 ; https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/remote/remote-environments.ts#L734-L753 ; https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/examples/desktop-app/sidecar/chat-session.ts#L921-L980.

**Processes.** The kill path is `unchecked`; the spawn path is `checked in review`.

- The shell is spawned `detached`.
- Abort and timeout kill the process group with SIGKILL (`taskkill /T /F` on Windows). A
  grandchild that calls `setsid()` survives.
- Hook timeouts kill the direct child only.
- Only a human can detach a running command ("Proceed While Running"). Once detached, it
  outlives the run.

Trace: https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/tools/executors/bash.ts#L773-L817 ; https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/tools/executors/bash.ts#L733-L742 ; https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hooks/subprocess-runner.ts#L288-L292 ; https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/tools/executors/bash.ts#L850-L897.

### OpenHands

Only the Agent Canvas client and its launchers are in this repository. The `docker run`
invocation, its hardening flags, the agent loop and the analysers are in
`OpenHands/software-agent-sdk`, which was not cloned (`absence confirmed` for the structural
claim). What follows is what the client and launchers do.

**The default stack has no sandbox.** `unchecked`

- The agent-server runs through `uvx` as a child of the user, with the full launcher environment.
- So the agent's shell can read `~/.openhands/agent-canvas/api-key.txt` and `secret-key.txt`
  despite their 0600 mode.

Trace: https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L727-L736 ; https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-safe.mjs#L84-L106.

**Per-conversation Docker: only env knobs are passed on.** `checked in review`

- `OH_CONVERSATION_IMAGE` and the `..._CONTAINER_MEMORY/CPUS/PIDS_LIMIT` variables are copied
  into the agent-server environment.
- The client sends `DockerExecutionWorkspace` with `/workspace`.
- The client sends `container: {memory, cpus, browser}` from settings with no bound; the only
  check is a UI minimum of 0.01 CPUs.

Trace: https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-safe.mjs#L1009-L1028 ; https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L804-L835.

**Docker mode binds every interface by default.** `checked in review`. The reason given: so
containers can reach the host through `host.docker.internal`. The whole API is then on the LAN,
guarded only by the session key:
https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L528-L543.

**The session key goes into the agent's environment.** `checked in review`

- `OPENHANDS_AUTOMATION_API_KEY = sessionApiKey` is set for terminal commands and seeded into the
  secret store.
- Every saved secret is attached to every conversation, so the key also reaches the
  per-conversation containers.
- With it, the agent can approve its own pending actions and run server-level bash.

Trace: https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L975-L985 ; https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/bash-service/bash-service.api.ts#L24-L44.

**A failed probe falls back to a host workspace.** `checked in review`. When the `server_info`
probe fails (3 s), the client sends `LocalWorkspace` with a host path:
https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L236-L254.

**All-in-one image and Helm.** `checked in review`

- All-in-one: agent, state and keys share one container and one uid; the encryption key sits
  beside what it encrypts:
  https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/docker/Dockerfile#L188-L206.
- Helm: container `securityContext: {}`, with `drop: [ALL]` only as a comment. The
  service-account token is automounted, and RBAC can bind `admin` or `cluster-admin`:
  https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/helm/agent-canvas/values.yaml#L41-L62.

Hooks are skipped for container workspaces (a review correction in `openhands.safety.md`
section 8).

**Remote executor: DigitalOcean "MARS".** `checked in review`

- A microVM runs an Agent Server, reached through a bearer-authenticated port-forward tunnel that
  the Node side dials. The cloud token never reaches the renderer.
- It is opt-in: `MARS_WEB` must be exactly `1` (corrected in review).
- No pairing and no fencing exist.

Trace: https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/mars-web-bridge.mjs#L182-L185 ; https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/tunnel-client.mjs#L148-L154.

### Zed

**The tool surface: two terminal tools, one name.** `checked in review`

- `sandboxed_terminal` replaces `terminal`, under the same name, when the `sandboxing` flag is on
  and three more conditions hold: the project is local, the OS is supported, and
  `allow_unsandboxed` is not set. The flag is `enabled_for_all`.
- Remote projects are never sandboxed.
- On a platform without an integration, the wrap is a no-op.

Trace: https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/sandboxing.rs#L1-L26 ; https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/feature_flags/src/flags.rs#L107-L121 ; https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/thread.rs#L4412-L4450.

**The policy built from configuration.** `unchecked`

- The intent type is `SandboxPolicy { fs, network }`:
  - `fs` is unrestricted, or restricted to writable paths plus protected paths plus protected
    file names;
  - `network` is unrestricted, blocked, or a host list.
- Three layers merge to the least restrictive: worktree roots (writable) plus `.git` (protected),
  thread grants, and persistent settings.
- Writable paths are captured as a canonical path plus a pinned descriptor or inode, and
  re-verified at use. An overlap between writable and protected paths is an error.
- Protected file names (such as `.envrc`) are enforced on macOS only. Linux and WSL reject any
  policy that contains them.

Trace: https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/sandbox/src/sandbox.rs#L81-L173 ; https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/sandbox/src/sandbox.rs#L220-L265 ; https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/sandboxing.rs#L36-L90 ; https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/sandbox/src/sandbox.rs#L288-L333 ; https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/sandbox/src/sandbox.rs#L120-L129.

**macOS: Seatbelt, deny by default.** `unchecked`

- The profile starts `(deny default)`.
- It allows:
  - all file reads, process exec and fork, signals, and sysctl reads;
  - an allow-list of Mach services rather than a blanket `mach-lookup`, so a command cannot ask
    LaunchServices to spawn an unsandboxed process;
  - pty devices;
  - writes under the writable subpaths only.
- Protected paths get `(deny file-write* …)` after the allows, because the last match wins.
- Network: none, only `localhost:<port>` of the proxy, or all.
- No Unix sockets are allowed.
- It launches as `/usr/bin/sandbox-exec -f <tmp profile>` with the canonical paths captured at
  approval time.
- If the policy cannot be built, the tool fails closed.

Trace: https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/sandbox/src/macos_seatbelt.rs#L215-L262 ; https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/sandbox/src/macos_seatbelt.rs#L296-L412 ; https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/sandbox/src/macos_seatbelt.rs#L185-L200 ; https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/sandbox/src/sandbox.rs#L946-L1000 ; https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/tools/terminal_tool.rs#L814-L846.

**Linux: bubblewrap.** `checked in review`

- Arguments:
  - `--ro-bind / /`, `--dev /dev`, `--proc /proc`, `--tmpfs /tmp`;
  - a `--bind` per writable directory at its exact path;
  - a read-only re-bind over protected paths;
  - `--unshare-user/ipc/uts/pid/cgroup-try`, `--die-with-parent`;
  - `--unshare-net` for blocked or proxied network. The proxy is reached over a bind-mounted
    Unix socket bridged to a loopback port.
- Full write uses `--bind / /`.
- The `bwrap` binary: found on `PATH`, and a setuid-root `bwrap` is rejected. A probe runs
  `bwrap … true` first.
- After mounting, an in-sandbox launcher compares the mounted inodes with descriptors the host
  passed, and fails closed on a mismatch.
- A path that does not exist yet is skipped. So the `.git` protection that is added "even when it
  doesn't exist yet" works on macOS but not on Linux (`inferred`, not tested).

Trace: https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/sandbox/src/linux_bubblewrap.rs#L246-L352 ; https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/sandbox/src/linux_bubblewrap.rs#L120-L178 ; https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/sandbox/src/linux_bubblewrap.rs#L180-L200 ; https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/sandbox/src/sandbox.rs#L841-L944 ; https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/sandboxing.rs#L53-L66.

Why Linux moved from Landlock to bubblewrap: no reason is written in the code. A comment calls
the Landlock probe "former" and mentions a planned syscall-interception backend:
https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/sandbox/src/sandbox.rs#L700-L712
(`unchecked`).

**When the mechanism is unavailable.** `unchecked`

- Linux, and Windows WSL setup errors: a prompt with Retry (re-probe), Run unsandboxed (once,
  for this thread, or always) and Deny.
- macOS: fails closed.
- A "for this thread" answer is stored as a grant, so later commands in the thread run
  unsandboxed without asking.
- Whatever the choice, a note is attached to the tool result.

Trace: https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/tools/terminal_tool.rs#L722-L813 ; https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/tools/terminal_tool.rs#L859-L946.

**Windows.** `unchecked`

- Bubblewrap inside WSL, with a Zed binary provisioned into the distribution as the in-sandbox
  helper.
- Restricted host networking is rejected; network is only on or off.
- DrvFs paths get a recurring warning.

Trace: https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/sandbox/src/windows_wsl.rs#L1-L40 ; https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/sandbox/src/sandbox.rs#L1115-L1121.

**Escalation: the model asks, with a reason.** `checked in review`

- The input carries `allow_hosts`, `allow_all_hosts`, `fs_write_paths`, `allow_fs_write_all`,
  `unsandboxed` and a required `reason`.
- The ordinary permission check runs first; the sandbox decision is an extra layer.
- Grants last once, for the thread, or always.
- A request weaker than a standing grant is rejected with an explanation.
- A command that ran without a sandbox gets an explicit note in its result.
- Git metadata cannot be granted; the model has to ask for `unsandboxed`.

Trace: https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/tools/terminal_tool.rs#L94-L210 ; https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/tools/terminal_tool.rs#L417-L472 ; https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/tools/terminal_tool.rs#L678-L705 ; https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/tools/terminal_tool.rs#L474-L525 ; https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/tools/terminal_tool.rs#L947-L983.

**Network egress for the terminal.** `unchecked`

- For a host list, an in-process HTTP(S) proxy on a Unix socket is reached through a reserved
  loopback port. `HTTPS_PROXY`, `HTTP_PROXY` and `ALL_PROXY` point at it, and `NO_PROXY` is
  blanked.
- The proxy matches hostnames, denies IP literals, resolves names itself, connects to the vetted
  addresses and caps concurrent connections at 256.
- SSH and raw TCP cannot be allow-listed.

Trace: https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/sandbox/src/sandbox.rs#L1179-L1198 ; https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/http_proxy/src/proxy.rs#L404-L412.

`fetch` is not in the OS sandbox. It is gated in-process, with host grants shared with the
terminal plus a forbidden-IP check (`checked in review`):
https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/tools/fetch_tool.rs#L45-L60.

**ACP terminals are unsandboxed.** `unchecked`. `terminal/create` from an external agent runs in
a normal project terminal, with no sandbox and no permission check. ACP agent children are
killed on drop:
https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent_servers/src/acp/transport.rs#L16-L25.

**No container backend and no remote executor** appear in the code that was read. The remote
server was not read.

### Warp

**No OS sandbox in the client.** `absence confirmed`. There is no seatbelt, landlock, bwrap,
seccomp or AppContainer in any `.rs` file. Commands run in the user's own interactive PTY
(`unchecked`):
https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/blocklist/action_model/execute/shell_command.rs#L324-L413.

**`--sandboxed` is a self-declared flag, not a mechanism.** `corrected in review`

- With the hidden flag `--sandboxed`, the client builds a profile with an empty denylist and
  allows reads and writes.
- An organisation-configured "sandboxed agent" denylist is still evaluated.
- No OS sandbox is created.
- `isolation_platform::detect` only detects Docker, Kubernetes and similar, to obtain a workload
  token; it confines nothing.

Trace: https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/warp_cli/src/agent.rs#L621-L623 ; https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/cloud_object_models/src/ai_execution_profile.rs#L463-L471 ; https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/blocklist/permissions.rs#L233-L243 ; https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/isolation_platform/src/lib.rs#L29-L48.

**An experimental local Docker sandbox (dogfood flag only).** `unchecked`

- The shell is
  `sbx run --name warp-sandbox-<id> shell <empty host dir> <init dir>:ro -- -c "cd /home/agent && exec bash --rcfile ..."`.
- The workspace is a fresh empty directory. The user's tree and home are deliberately not
  mounted.
- Per-sandbox host directories are mode 0700.
- Container cleanup is a TODO, so stopped containers accumulate.

Trace: https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/warp_terminal/src/local_tty/unix.rs#L65-L99 ; https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/warp_terminal/src/local_tty/unix.rs#L930-L953 ; https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/warp_terminal/src/local_tty/docker_sandbox.rs#L131-L138.

**Pass-through flags that defeat other harnesses' isolation.** `checked in review`

- Local child agents:
  - `claude … --dangerously-skip-permissions`;
  - `codex --dangerously-bypass-approvals-and-sandbox`.
- Cloud runs: the same, plus `--dangerously-bypass-hook-trust`.
- The driver also pre-writes trust into `.claude.json` and Codex's config.
- Warp's own policy is not applied to these CLIs.

Trace: https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/pane_group/pane/local_harness_launch.rs#L120-L137 ; https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/agent_sdk/driver/harness/codex.rs#L206-L227.

**Remote worker.** The worker (`oz-agent-worker`) is a separate Go repository and was not read
(`inferred` from a design note).

### OpenCodex

Only Remote Workspace is in this group. It is experimental and needs both
`runtimeRole === "hub"` and `OCX_REMOTE_WORKSPACE_ENABLED=1`.

**How the hub confines the agent.** `checked in review`

- The agent (the Codex app-server) runs on the hub in a `mkdtemp` directory (mode 0700).
- Its only tools are four remote tools behind a local MCP bridge, auto-approved, with
  `approvalPolicy: "never"`.
- The user's MCP servers and hooks are disabled.
- "Use no other tool" is an instruction to the model, not a hard guarantee.

Trace: https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-codex-runtime.ts#L330-L349 ; https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-tools.ts#L212-L225.

**What the executor refuses: its own policy, independent of the hub.** `corrected in review`

Path checks:

- Paths must be relative, non-empty, at most 4,096 characters, and free of NUL.
- `..` is contained by a resolve-then-inside check, not rejected by name (corrected in review).
- Every component is `lstat`ed; a symlink anywhere on the path is an error.
- An opened file is checked against its `lstat` by `fstat` (identity, birth time, regular file).
- A file with more than one hard link is refused.
- The root's device, inode and birth time are re-checked before every operation.

Operation rules:

- Writes are a temp file plus atomic rename, and need the previous content's SHA-256.
- Unknown argument keys are an error.
- `exec` is refused outright unless an OS sandbox runner exists ("disabled until an OS sandbox is
  configured").

Trace: https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-executor.ts#L88-L120 ; https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-executor.ts#L122-L166 ; https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-executor.ts#L338-L360.

The RPC is `prepare`, then a separate `grant`, then `response`, with `cancel` available. Each
request is bound to the session and root fixed at the handshake (`unchecked`):
https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-rpc.ts#L162-L207.

**The bubblewrap command.** `checked in review`

- `bwrap --die-with-parent --new-session --unshare-pid --unshare-ipc --unshare-uts --unshare-net --proc /proc --dev /dev --tmpfs /tmp`.
- Read-only binds:
  - `--ro-bind` for `/usr /bin /sbin /lib /lib64`;
  - `--ro-bind-try` for a short `/etc` list (`ca-certificates`, `ssl`, `hosts`, `nsswitch.conf`,
    `passwd`, `group`, `resolv.conf` and others);
  - read-only toolchain roots chosen at pairing;
  - the Bun executable as a single read-only file.
- One writable `--bind <root> /workspace`.
- `--chdir`, then `--clearenv` with only `HOME=/workspace`, `PATH`, `LANG` and `LC_ALL`.
- `--`, then the model's argv. It is an argv vector of at most 64 items, never a shell string.
- Network is unshared unless `networkAccess` is true, and no caller sets it.

Not in the argv: `--unshare-user`, `--cap-drop`, `--unshare-cgroup` and a seccomp filter.

Trace: https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-command-runner.ts#L23-L47 ; https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-command-runner.ts#L375-L436 ; https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-command-runner.ts#L469-L534.

The `bwrap` binary must meet four conditions: an absolute path, a private regular file
(nlink 1), outside every writable root, and not group- or world-writable along its whole
parent chain.

Before any command, the workspace is scanned (up to 250,000 entries). The run is refused if any
non-directory has more than one hard link, because a bind mount cannot tell two names for one
inode apart.

The child has no stdin. Its output goes through a bounded collector that kills it on overflow;
timeout and abort also kill it:
https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-command-runner.ts#L438-L467.

Unix sockets (`inferred` from the mount list): `/run` and `/var/run` are not bound, so host
pathname sockets such as the Docker socket are not visible. The input file does not state this.

**The probe gates `exec`, not writing.** `checked in review`

- A trial `bwrap … /bin/true` must succeed within 2 s, and the result is cached.
- `workspace.exec` is offered only if a runner exists. Read and write are always offered.
- The hub picks `accessMode` per session, default `read-only`.
- On macOS and Windows the native helper's probe fails by design, so those executors offer file
  tools only.

Trace: https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-command-runner.ts#L706-L747 ; https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-device.ts#L372-L384 ; https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-sessions.ts#L406-L418.

**Enrolment.** `corrected in review`. The stored capabilities are what the executor declared at
pairing, not an operator decision. A later presence message may only narrow them:
https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-hub.ts#L427-L463.

## 3. Comparison

### 3.1 Mechanism per OS and behaviour when unavailable

| Project | Linux | macOS | Windows | When unavailable | Enforcement level surfaced? |
|---|---|---|---|---|---|
| Codex | bwrap + seccomp (two stages); legacy Landlock refused for restricted policies | Seatbelt (profile not read) | restricted token or MXC, only if enabled; otherwise the rule fallback prompts | Linux: the helper panics, nothing runs. Exec-server: refusal string. WSL1: refused | not found in the input |
| OpenCode | none | none | none | n/a | n/a |
| OpenClaw | Docker or Podman container (opt-in) | same | same | `sandbox: required`: throws. Otherwise host exec | not found |
| Hermes Agent | none locally; Docker, SSH, Modal or Daytona backends | same | same | n/a | n/a |
| Oh My Pi | none | none | none | n/a | n/a |
| Pi | none (Gondolin VM as an example extension) | none | none | n/a | n/a |
| DeepSeek harness | bwrap, then Landlock, probed in order | Seatbelt `(allow default)` | restricted token plus ACL SIDs | `SANDBOX_UNAVAILABLE`, nothing runs | computed (`full`/`partial`), stored on the result, shown to nobody |
| goose | none (Seatbelt removed) | none | Job Object (for killing only) | n/a | n/a |
| Cline | none | none | none | n/a | n/a |
| OpenHands | per-conversation Docker, upstream and not read | same | same | a failed probe falls back to a host workspace | not visible |
| Zed | bwrap (setuid refused) | Seatbelt `(deny default)` | bwrap inside WSL | Linux and WSL: a prompt (retry, run unsandboxed, deny). macOS: fails closed | the model is told when it ran unsandboxed |
| Warp | none (`sbx` Docker behind a dogfood flag) | none | none | n/a | `--sandboxed` is self-declared |
| OpenCodex | bwrap on the executor | native helper, probe fails | native helper, probe fails | `exec` not offered; file tools only | the capability list (`workspace.exec` present or not) |

### 3.2 What the sandbox confines

| Project / mechanism | File reads | File writes | TCP/UDP | Unix sockets | Processes and signals |
|---|---|---|---|---|---|
| Codex bwrap+seccomp, network off | restricted-read policies start from `--tmpfs /`; deny paths masked; deny globs snapshotted at launch | writable roots only; `.git`, `.agents`, `.codex`, `.aws` read-only | netns; seccomp denies `connect`, `bind`, `sendto` and non-`AF_UNIX` `socket` | `connect` denied, so pathname stream sockets are blocked; `sendmsg` to a pathname datagram socket not denied (review); daemon directory masked | PID, user and IPC namespaces; `ptrace` and `process_vm_*` denied; `io_uring` denied |
| Codex, network on | as above | as above | open (unless proxy mode) | reachable except masked directories | `AF_VSOCK` and `io_uring` denied |
| DeepSeek bwrap | not confined | roots, plus a private `/tmp` | not confined | not confined (inferred) | `--unshare-pid` |
| DeepSeek Landlock | not confined | roots, plus the shared `/tmp` | not confined | not confined | none |
| DeepSeek Seatbelt | not confined | roots | not confined | not confined | none |
| Zed bwrap | `--ro-bind / /` (all readable) | exact writable binds; protected paths re-bound read-only (missing paths skipped) | netns when blocked or proxied; HTTP(S) proxy for host lists | not stated in the input for Linux | user, pid, ipc, uts and cgroup namespaces |
| Zed Seatbelt | all reads allowed | writable subpaths; protected denies last | none, the proxy port, or all | none allowed | Mach-service allow-list |
| OpenCodex bwrap | `/usr`, `/bin`, `/lib*`, a short `/etc` list, toolchain roots | one `/workspace` bind | `--unshare-net` always | host sockets not mounted (inferred) | pid, ipc and uts namespaces; no user namespace, no `cap-drop`, no seccomp |
| OpenClaw container | container filesystem plus validated binds | `--read-only` root, sized tmpfs, binds | `--network none` by default | Docker socket binds rejected | `--cap-drop ALL`, `no-new-privileges`, `--pids-limit 1024` |
| Hermes Docker | container filesystem plus persistent home and workspace binds | the same binds (as root with `DAC_OVERRIDE`) | on by default | not addressed | `cap-drop ALL` plus 3 to 5 added back; `no-new-privileges`; pids 2048 if cgroups allow |
| rizzy PR 4 Landlock (our code) | `SYSTEM_READ` list plus node.toml `read`; `/proc` readable | session folder, per-session `TMPDIR`, node.toml `write` | network off: TCP connect and bind denied (ABI 4); UDP and DNS open | pathname sockets open below ABI 9; abstract sockets scoped (ABI 6, best-effort) | signals scoped (ABI 6, best-effort); `no_new_privs` required |

### 3.3 Container invocations: fixed flags and who controls them

| Flag or property | OpenClaw | Hermes Docker | goose `--container` | Warp `sbx` | OpenHands | rizzy design §4.6 |
|---|---|---|---|---|---|---|
| Who builds the invocation | Gateway, from `openclaw.json` | Hermes, from config | the user (an existing container) | the client, with fixed arguments | upstream SDK (not read) | the node, from `node.toml` |
| Image | config; default `openclaw-sandbox:trixie-slim` | config | the user's | Docker Sandboxes | `OH_CONVERSATION_IMAGE` env | allow-list pinned by digest |
| Pull | `--pull=never` | not stated | n/a | not stated | not visible | `--pull=never` |
| User | image default (inferred) | root, unless `--user` | the user's | not stated | not visible | `--userns=keep-id --user <uid>:<gid>` |
| Capabilities | `--cap-drop ALL` (default) | `ALL` dropped, then `DAC_OVERRIDE`, `CHOWN`, `FOWNER` (+`SETUID`, `SETGID`) | n/a | not stated | not visible | `--cap-drop=all` |
| `no-new-privileges` | always | yes, unless `docker_snap_compat` | n/a | not stated | not visible | yes |
| Root filesystem | `--read-only` (default) | writable | n/a | not stated | not visible | `--read-only` |
| tmpfs | `/tmp` 1 GB, `/var/tmp` and `/run` 256 MB | `/tmp` 512 MB, `/var/tmp` 256 MB, `/run` 64 MB | n/a | not stated | not visible | `/tmp` 1 GB, `/run` (no size) |
| Network | `none` (default); host and `container:*` rejected | on (default) | n/a | not stated | not visible; the launcher binds `0.0.0.0` so containers reach the host | pasta or none; host only with `allow_host_network` |
| Limits | `--pids-limit 1024`, `ulimit core=0`; memory and CPU optional | pids 2048, 1 CPU, 5120 MB if cgroups allow | n/a | not stated | the client sends unbounded memory and CPU | memory, swap, CPUs, pids from `node.toml` |
| Mounts | validated binds (blocked list, roots, realpath through the ancestor) | persistent home and workspace; read-only skills and creds; cwd optional | n/a | an empty host directory plus a read-only init directory | `/workspace` | one environment volume, `relabel=private` |
| Environment | `--env-file`, config-declared only | name-only `-e KEY` (values not in argv) | `-e K=V` on argv (leaks into `ps`) | not stated | every saved secret plus the session key | `--env` allow-listed names |
| Pass-through args | three `dangerously*` booleans | `docker_extra_args` appended last, override anything | n/a | none | n/a | none (by design) |
| Recreate on flag change | `createArgsEpoch` label | labels (task, profile, egress, env fingerprints) | n/a | n/a | n/a | not specified |
| Checks skipped inside | approval (sandboxed exec runs without one by default) | hardline floor, sudo guard, dangerous patterns | n/a | n/a | hooks skipped for container workspaces | none planned |

### 3.4 Remote executors: what the executing side decides

| Project | Executor | Transport and authentication | Executor-side policy | Refusals |
|---|---|---|---|---|
| Codex | `codex exec-server` | Noise IK through a relay, registry-pinned key; direct `--listen` with optional auth | none: the caller's sandbox intent is materialised; no intent means no sandbox | "sandbox intent cannot be enforced on this executor" |
| DeepSeek harness | SSH helper | OpenSSH with fixed flags; TLS-PSK streams | runs its own platform sandbox chain for `confine` | `SANDBOX_UNAVAILABLE`; reported enforcement is self-reported |
| Pi (pi-env) | Rust daemon over SSH stdio | the SSH login only | none: any path, any command | unknown ops (`EINVAL`) only |
| OpenCodex | Remote Workspace executor | WSS bearer plus signed ECDH, AES-GCM | path, identity and hard-link checks; `exec` only with an OS sandbox; roots chosen on the executor | many (see section 2) |
| OpenClaw | paired node | Gateway WebSocket | own `security`/`ask`/allow-list; plan binding; policy-snapshot check | `SYSTEM_RUN_DENIED`; but a Gateway approval overrides the node allow-list |
| Hermes Agent | SSH, Modal, Daytona | `accept-new` SSH; vendor SDKs | none (guards skipped for cloud backends) | none |
| Cline | remote Cline hub over SSH | strict host keys, tunnel, token | a full remote agent with its own settings | its own |
| OpenCode | remote OpenCode server over SSH; empty driver seam | SSH tunnel plus service password | a full remote server | its own |
| OpenHands | DigitalOcean microVM agent-server | bearer port-forward tunnel | upstream, not read | not visible |
| goose, Oh My Pi, Zed, Warp | none read (OMP SSH is file I/O only; Warp's worker is not in the repository) | n/a | n/a | n/a |

### 3.5 How child processes are killed, and what survives

| Project | Kill mechanism | What survives |
|---|---|---|
| Codex | `--die-with-parent`, `--new-session`, PID namespace; the inner stage forwards signals; LRU of 64 processes terminates pruned ones | not traced |
| OpenClaw / Hermes containers | `--init`; container lifetime (`sleep infinity`); Daytona cancel stops the sandbox | Hermes reuses containers across processes by label |
| Hermes local | `start_new_session` plus an owner-death supervisor | not stated |
| Oh My Pi | SIGTERM to the group and descendants, grace, re-walk, SIGKILL (unchecked) | a double-fork that changes session (inferred) |
| Pi CLI | SIGKILL to the process group; SIGSTOP first on macOS; tracked detached children killed on SIGHUP/SIGTERM | everything, if Pi itself is SIGKILLed |
| Pi pi-env | the daemon kills every process group it started on stdin EOF or 30 s of silence | n/a |
| DeepSeek SSH | the helper lease expires and remote managed processes are cleaned up | n/a |
| goose | `kill(-pgid, SIGKILL)`; Windows Job Object created suspended; `Drop` kills | `setsid` children; background work after a normal exit; everything if goose crashes (no PDEATHSIG for the shell) |
| Cline | SIGKILL to the detached group; `taskkill /T /F` | `setsid` grandchildren; human-detached commands; hook timeouts kill the direct child only |
| OpenCodex | bwrap `--die-with-parent --new-session`; kill on output overflow, timeout or abort | not stated |
| Zed | bwrap `--die-with-parent`; ACP children killed on drop | not stated |
| OpenCode | PTY daemon keeps terminals across server restarts | terminals, by design |

## 4. Patterns and trade-offs

**A. Namespace tool plus syscall filter (Codex).** bubblewrap builds the filesystem view and the
namespaces; seccomp after it closes sockets, `ptrace` and `io_uring`.

- Cost: a helper binary to locate and verify, a two-stage exec, and an architecture-specific
  filter (x86_64 and aarch64 only).
- What it still fails to cover: it is a denylist with default allow. `sendmsg` is left open
  (review). Deny globs are a snapshot taken at launch. Missing writable roots are skipped
  silently.
- It is the only design in the set that blocks UDP and Unix-socket connects when network is off.

**B. Namespace tool alone (Zed, OpenCodex, DeepSeek's bwrap rung).** The coverage depends
entirely on the flags.

- Zed's `--ro-bind / /` leaves every file readable.
- DeepSeek has no `--unshare-net`, so the network is open.
- OpenCodex mounts a minimal set and unshares the network, but lacks `--unshare-user`,
  `--cap-drop` and seccomp.
- Missing paths cannot be bound, so "protect `.git` before it exists" silently fails on Linux
  (Zed, inferred).

**C. Landlock self-restriction (DeepSeek's second rung; our PR 4).** It needs no helper binary
and no user namespace.

- DeepSeek grants read everywhere and sets no network or scope rights, so it confines writes
  only. It still reports `full` on ABI 6 and later kernels.
- Codex refuses Landlock because it cannot hide a same-uid control socket. That is the code's
  statement; the claim that Landlock cannot block `connect()` is the author's inference.
- Our module header (PR 4) notes that ABI 9 adds pathname Unix-socket connect control, so on new
  kernels that gap closes. That fact comes from our code, not from the input files.

**D. Seatbelt.**

- Deny by default with a Mach allow-list (Zed) versus allow by default with only writes denied
  (DeepSeek). The first confines; the second does not.
- `sandbox-exec` is deprecated by Apple, but it is the only per-command option in the set.

**E. Containers (OpenClaw, Hermes, Warp `sbx`, OpenHands upstream).** The host boundary is the
container.

- Strength depends on three things: who controls the flags and mounts, whether pass-through
  arguments exist, and whether the harness relaxes its own checks inside.
- OpenClaw is the most careful: validated binds, `--pull=never`, a recreate epoch, `--network
  none` by default.
- Hermes appends operator extras last and skips its safety floor.
- goose's `--container` does not cover the shell at all.
- OpenHands lets the client set limits and puts the control-plane key in the container.

**F. No sandbox, by policy (OpenCode, Oh My Pi, Pi, goose, Cline, Warp, Hermes local).** Pi and
Cline say outright that the sandbox is the user's job. Warp's `--sandboxed` and Cline's
`CLINE_SANDBOX` use the word for things that confine nothing.

**G. Escalation shapes.**

| Project | Shape | Costs and gaps |
|---|---|---|
| Codex | retry after a denial, with fresh approval | keyword denial detection on a path that can lead to unsandboxed execution; an `allow` rule removes the sandbox |
| DeepSeek harness | the model declares a wider mode plus a justification, one-shot | no proof of a prior denial |
| Zed | the model declares hosts, paths or `unsandboxed` plus a reason, with three grant lifetimes | a thread-lifetime unsandboxed grant spreads silently |

**H. Executor trust direction.**

| Executor | Trust direction | Weak point |
|---|---|---|
| Codex exec-server | trusts the caller's policy | no ceiling of its own |
| pi-env | trusts its stdin | no policy at all |
| OpenClaw node | decides | lets a Gateway approval override its allow-list |
| OpenCodex executor | validates everything itself and refuses `exec` without a sandbox | capabilities are self-declared at enrolment, with no operator decision |
| DeepSeek SSH helper | sandboxes with its own chain | self-reports its hash and its enforcement |

**I. Killing.**

- Process-group kill is universal and leaks `setsid` children: Cline and goose say so; Oh My Pi
  and Pi are inferred.
- Oh My Pi's re-walk before SIGKILL narrows the leak.
- A PID namespace (`--die-with-parent`, the container's init) or a cgroup closes it. Codex and
  Zed (via bwrap) and the container backends get this; the plain-spawn harnesses do not.

## 5. Documentation versus code

| Project | Round 1 (documentation) said | The code shows | Verdict | Permalink |
|---|---|---|---|---|
| Codex | Linux sandbox is bubblewrap plus seccomp | two-stage helper, seccomp after bwrap | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/linux_run_main.rs#L265-L321 |
| Codex | first `bwrap` on PATH, else the bundled helper | PATH entries the sandbox can write are skipped; the bundled binary is digest-pinned and run by fd | corrected | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/sandboxing/src/bwrap.rs#L190-L207 |
| Codex | Landlock is rejected for restricted policies (app-server sockets) | the rejection is confirmed; the stated reason is "isolate app-server sockets"; "Landlock cannot block connect" is an inference | confirmed (reason partly inferred) | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/linux_run_main.rs#L386-L393 |
| Codex (as cited in `harnesses-round-1.md`) | our residuals (pathname sockets, UDP, DNS) "are why Codex rejects Landlock" | the code names only socket isolation from the app-server daemon; UDP and DNS are not mentioned | corrected | same as above |
| Codex | `workspace-write` network off by default (unconfirmed) | `network_access` serde default false | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/protocol/src/protocol.rs#L1294-L1316 |
| Codex | `.git`, `.agents`, `.codex`, `.aws` protected | the four names | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/protocol/src/permissions.rs#L49-L54 |
| Codex | deny-read globs are a per-launch snapshot | yes | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/bwrap.rs#L470-L481 |
| Codex | refuses a command whose sandbox cannot be enforced | refusal string on the exec-server | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/process_sandbox.rs#L226-L230 |
| Codex | proxy opt-in; with network on and proxy off, egress unrestricted | yes | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/linux_run_main.rs#L430-L441 |
| DeepSeek | the sandbox is filesystem only | and reads are not confined either | confirmed | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/sandbox/sandbox-local/src/profiles.ts#L11-L60 |
| DeepSeek | no backend means `SANDBOX_UNAVAILABLE`, no passthrough | yes | confirmed | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/sandbox/sandbox-local/src/index.ts#L499-L524 |
| DeepSeek | backends report `full` or `partial` | computed and stored on the tool result, shown to nobody | corrected | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/shell/bash-sandbox/src/index.ts#L115-L124 |
| DeepSeek | SSH helper checked against a configured hash; loss is final | yes; the hash is self-reported by the remote | confirmed with correction | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/ssh/ssh/src/index.ts#L312-L318 |
| DeepSeek | sub-agent permission inheritance undocumented | inherits the sandbox override; approval pinned to `never` | settled | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/subagent/src/child-agent.ts#L249-L256 |
| Zed | the sandbox covers terminal and fetch | fetch is gated in-process, not OS-sandboxed | corrected | https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/tools/fetch_tool.rs#L45-L60 |
| Zed | Linux bubblewrap "may fall back to unsandboxed with a warning" | a three-way prompt; macOS fails closed | corrected | https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/tools/terminal_tool.rs#L722-L846 |
| Zed | sandbox maturity unclear | the flag is enabled for all | corrected | https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/feature_flags/src/flags.rs#L107-L121 |
| Zed | defaults: reads allowed, writes to the project and temp, git never writable, network blocked | confirmed; on Linux the `.git` protection skips a path that does not exist yet | confirmed with caveat | https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/sandbox/src/linux_bubblewrap.rs#L246-L352 |
| Zed | Windows only inside WSL; native commands unconfined | yes | confirmed | https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/sandbox/src/windows_wsl.rs#L1-L40 |
| OpenClaw | sandbox off by default | `mode ?? "off"` | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/config.ts#L212-L227 |
| OpenClaw | the gate is on the Gateway | the node also enforces its own policy, but a Gateway approval satisfies its allow-list | corrected | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/exec-policy.ts#L60-L117 |
| OpenClaw | cloud workers are outbound-only (inferred) | the worker dials, but the Gateway SSHes into the box | corrected | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/ssh.ts#L259-L291 |
| Hermes | the hardline floor is refused even under YOLO, everywhere | skipped on container backends | corrected | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval.py#L1161-L1175 |
| Hermes | Docker: cap-drop ALL plus 3, no-new-privileges, pids limit 256 | pids 2048, gated on cgroups; `SETUID`/`SETGID` unless `--user` | corrected | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/docker.py#L336-L343 |
| Hermes | one long-lived container, `sleep infinity`, `docker exec` | yes | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/docker.py#L1022-L1037 |
| Hermes | iron-proxy for remote and Docker sandboxes | Docker only | corrected | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/docker_egress.py#L29-L47 |
| Hermes | the local terminal backend is not a sandbox | no OS sandbox code | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/local.py#L1149-L1168 |
| Oh My Pi | whether bash can run remotely: open | no remote exec; SSH is file I/O only | corrected | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/ssh/connection-manager.ts#L193-L209 |
| Oh My Pi | no OS sandbox | absence confirmed | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/file-write-fallback.ts#L1-L15 |
| Pi | runs with the account's permissions | plain `spawn` | confirmed | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/tools/bash.ts#L124-L131 |
| Pi | Docker, Docker Sandboxes and OpenShell isolation patterns | documentation only | confirmed | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/docs/containerization.md#L9-L16 |
| Pi | pi-env: restricted SSH options, strict host keys; "host-key scan wording not found" | flags confirmed; `scanHostKey` exists | confirmed | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/env/src/ssh.ts#L59-L123 |
| Pi | how pi-env connects to the CLI: not stated | it does not; only `apps/p2` uses it | confirmed | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/apps/p2/package.json#L29-L30 |
| goose | an opt-in macOS Desktop Seatbelt sandbox (unverified) | removed; only a blog post remains | corrected | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/documentation/blog/2026-02-23-goose-v1-25-0/index.md#L19-L34 |
| goose | `--container` runs extensions in Docker | stdio and built-in extensions only; the Developer shell stays on the host | corrected | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/extension_manager/mod.rs#L932-L948 |
| Cline | no OS sandbox; `CLINE_SANDBOX` meaning unclear | none; `CLINE_SANDBOX` moves the data directory | confirmed / settled | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/cli/src/utils/helpers.ts#L583-L605 |
| Cline | SSH tunnel timeout 45 s | 40 s | corrected | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/remote/remote-environments.ts#L177-L182 |
| OpenCode | nothing documented | nothing in code | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell.ts#L370-L382 |
| OpenHands | host process, no isolation | yes | confirmed | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L727-L736 |
| OpenHands | Docker hardening flags and the outer proxy | not in this repository | not in code | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-safe.mjs#L1009-L1028 |
| OpenHands | Docker runtime mode binds all interfaces | yes | confirmed | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L528-L543 |
| OpenHands | Docker whole-stack: container, mounted directories only | true toward the host; inside, the agent reads every credential | corrected | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/docker/Dockerfile#L188-L206 |
| Warp | no local sandbox | confirmed; `--sandboxed` is a trusted flag; a dogfood Docker sandbox exists | confirmed, plus new | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/warp_cli/src/agent.rs#L621-L623 |
| OpenCodex | bubblewrap-sandboxed commands | yes, with the flag list in section 2 | confirmed | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-command-runner.ts#L23-L47 |
| OpenCodex | read-only by default; writing needs a command-sandbox probe | the probe gates `exec`; writing needs no probe | corrected | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-device.ts#L372-L384 |
| OpenCodex | a disconnected executor never falls back to the hub filesystem | an instruction plus the absence of a local tool path, not a hard guarantee | confirmed (weaker) | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-tools.ts#L212-L225 |

## 6. For rizzy-harness

These points are ranked. Each says what to do, whom to copy from, and how it relates to `main`,
pull request 4 (Landlock, on `integration/gate3`) and the design note
(`docs/nodes-credentials-environments.md` §4.6).

### 6.1 The recommended rootless Podman invocation

The node builds this argv and nothing else contributes to it. The design note's §4.6 is close;
the changes are marked.

```
podman run --detach
  --name rh-<env>-<session>
  --label rizzy.env=<env> --label rizzy.session=<session>
  --label rizzy.argv=<sha256 of the canonical argv below>            # ADD
  --userns=keep-id --user <node uid>:<gid>
  --cap-drop=all --security-opt=no-new-privileges
  --read-only
  --tmpfs /tmp:size=1g,mode=1777,nosuid,nodev
  --tmpfs /run:size=64m,nosuid,nodev,noexec                          # ADD sizes and flags
  --network=none | --network=pasta                                   # see the network row
  --memory=<m> --memory-swap=<m> --cpus=<n> --pids-limit=<p>
  --ulimit core=0                                                    # ADD
  --mount type=bind,src=<root>/<env>,dst=<root>/<env>,relabel=private
  --workdir <root>/<env>/worktrees/<session>
  --env HOME=<root>/<env>/home --env NAME …                          # names only; values in podman's env
  --init --pull=never --log-driver=none
  <image@sha256:…> sleep infinity
```

Each `bash` call runs as `podman exec --user <uid>:<gid> --workdir … --env NAME … <ctr> <wrapper> bash -c <cmd>`.

Why each flag is fixed, and the project it comes from:

| Flag | Reason | Evidence |
|---|---|---|
| Node-built argv; no `extra_args` key in `node.toml`, now or later | Hermes appends `docker_extra_args` last, so a `-v /:/host` overrides everything, and its host-access check never reads extras. OpenClaw's three `dangerously*` booleans are the same hole, behind a name. | Hermes `docker.py#L741-L745`, `approval.py#L1161-L1175`; OpenClaw `validate-sandbox-security.ts#L23-L53` |
| `--userns=keep-id --user` | Hermes runs as root with `DAC_OVERRIDE` over host binds; do not. With keep-id, files written in the volume are owned by the node user, which the node's file tools and checkpoint git need. | Hermes `docker.py#L321-L343`; design note §4.6 |
| `--cap-drop=all`, nothing added back | Hermes re-adds `DAC_OVERRIDE`, `CHOWN`, `FOWNER`, `SETUID`, `SETGID`. OpenClaw keeps `ALL` dropped by default. | Hermes `docker.py#L321-L343`; OpenClaw `container-allocation.ts#L71-L167` |
| `--security-opt=no-new-privileges` | The one flag OpenClaw's configuration cannot remove. Hermes drops it in `docker_snap_compat`. | OpenClaw `container-allocation.ts#L71-L167` |
| No `--security-opt seccomp=unconfined` or `apparmor=unconfined`, ever | OpenClaw rejects both. Keep Podman's default seccomp profile. | OpenClaw `validate-sandbox-security.ts#L274-L356` |
| `--read-only` plus sized tmpfs | OpenClaw sizes every tmpfs. The design note's `/run` has no size, so a full `/run` is RAM pressure on the node. | OpenClaw `config.ts#L91-L117`; Hermes tmpfs `noexec` on `/run` |
| `--network=none` by default | OpenClaw's default is `none`; Hermes's default of on is the mistake. When `network = "on"`, Hermes shows that an env-var proxy is not egress control: raw sockets bypass it. So "on" means really on. Any allow-list needs a namespace with only a proxy route, which Codex's bridge and Zed's proxy show. | OpenClaw `config.ts#L91-L117`; Hermes `docker_egress.py#L29-L76`; Codex `proxy_routing.rs#L131-L206` |
| Never `--network=host` or `container:*` without `allow_host_network` | OpenClaw rejects both outright. | OpenClaw `validate-sandbox-security.ts#L93-L132` |
| No published ports; nothing on the host listens for the container | OpenHands binds `0.0.0.0` so containers can reach the host, putting the whole API on the LAN. | OpenHands `dev-with-automation.mjs#L528-L543` |
| Limits from `node.toml` only; a session may lower them, never raise them | OpenHands forwards client-supplied memory and CPU with no bound. | OpenHands `agent-server-adapter.ts#L804-L835` |
| `--ulimit core=0` | OpenClaw. A core dump of a process holding a secret lands on the volume. | OpenClaw `config.ts#L91-L117` |
| One `--mount`, source validated | OpenClaw rejects socket paths, credential directories and system directories, and checks the real path of the nearest existing ancestor. Our single source must pass `check_protected` and must not be, or contain, the data directory, `$XDG_RUNTIME_DIR` (the rootless Podman socket lives there) or `~/.ssh`. | OpenClaw `validate-sandbox-security.ts#L23-L53` |
| `--env NAME` with the value in the `podman` child's environment | Hermes keeps values out of `/proc/*/cmdline` this way; goose puts `-e K=V` on argv, and the host process list shows it. | Hermes `docker.py#L539-L542`; goose `stdio.rs#L40-L53` |
| `--pull=never`, image by digest from the allow-list | OpenClaw `--pull=never`. The image is never chosen by the master, the project or the repository. | OpenClaw `container-allocation.ts#L71-L167` |
| `--label rizzy.argv=<hash>`; recreate on mismatch | OpenClaw bumps `createArgsEpoch` so an old container never runs with stale flags after an upgrade. | OpenClaw `constants.ts#L62-L64` |
| Check that the created id is 64 hex digits | OpenClaw. It rejects a confused CLI output before the id is used in later commands. | OpenClaw `container-allocation.ts#L214-L273` |
| `--init` | OpenClaw and Hermes. It reaps zombies and gives the container a real PID 1, so stopping the container kills every descendant (see 6.4). | as cited |
| No node or master credential in the container | OpenHands puts the session key, which can approve the agent's own actions, into every container. The link key and any grant must never be in the volume or the environment. | OpenHands `dev-with-automation.mjs#L975-L985` |

Network caveat (`not settled`): no input file says what rootless `pasta` exposes to the container.
In particular, it is not known whether the host loopback (the master on `127.0.0.1:7777`, the
node's own sockets) is reachable. Codex's reviewer flagged the same shape: with network on, an
unauthenticated loopback listener is reachable from the sandbox (`inferred`). Write the test
before shipping `network = "on"`.

### 6.2 What to add to the Landlock path (PR 4): a seccomp layer, rule by rule

Our Landlock path already confines reads, which DeepSeek's does not. Its residuals are UDP, DNS
and pathname sockets below ABI 9. Codex's `Restricted` table fills exactly that gap.

Install the filter inside `sandbox-exec`: after `restrict_self` and `no_new_privs`, immediately
before `exec`. The order follows Codex's inner stage, which also checks that no capabilities
remain first.

| # | Rule | Mode | Closes | Caveat |
|---|---|---|---|---|
| 1 | `socket`: allow only `AF_UNIX`; `EPERM` otherwise | network off | UDP and DNS, raw sockets, `AF_VSOCK`, and anything Landlock ABI 4 does not cover | Codex accepts this in `Restricted`. Test that the tools we expect (git, cargo, compilers) still run. |
| 2 | `socketpair`: `AF_UNIX` only | network off | nothing new; matches Codex | none |
| 3 | `connect`, `bind`, `listen`, `accept`, `accept4`: deny | network off | pathname Unix stream sockets (Docker, D-Bus, ssh-agent) on kernels below ABI 9 | Breaks any legitimate local socket use under network off. Codex accepts that. |
| 4 | `sendto`, `sendmmsg`, `recvmmsg`: deny | network off | addressed datagrams | `recvfrom` stays open: Codex keeps it for socketpair users (cargo clippy). |
| 5 | `sendmsg`: decide explicitly | network off | an addressed `sendmsg` to a pathname datagram socket (for example a syslog or notify socket) | Codex leaves `sendmsg`/`recvmsg` open (the reviewer's correction). Seccomp cannot read `msg_name` behind the pointer, so denying `sendmsg` also breaks socketpair IPC that uses it. Whether Landlock ABI 9 covers datagram sends is not in the input. Either deny it and test the toolchain, or keep it open and list it as a named residual in `PLAN.md` R2. Do not leave it unmentioned. |
| 6 | `getsockopt`, `setsockopt`, `getpeername`, `getsockname`, `shutdown`: deny | network off | nothing on its own; it is part of Codex's set | Codex denies these and its tools work. Copy the set, and drop a rule only if a test shows breakage. |
| 7 | `io_uring_setup`, `io_uring_enter`, `io_uring_register`: deny | always | io_uring ops do not pass through the seccomp checks on `connect` and the rest; Codex blocks io_uring in every mode | none known |
| 8 | `socket(AF_VSOCK)`: deny | always, including network on | host services over vsock; on WSL2, Windows process launch | Codex's stated reason |
| 9 | `ptrace`, `process_vm_readv`, `process_vm_writev`: deny | always | reads of another sandboxed process's memory | Landlock already denies ptrace-class access to processes outside the domain; this is defence in depth inside the domain |
| 10 | Default action allow; denied calls return `EPERM`, not `SIGSYS` | | | A denylist, as in Codex. Use `EPERM`, never a kill: Codex treats 128 plus SIGSYS as a "sandbox denial" that can lead to an unsandboxed retry. |
| 11 | Unknown architecture: `Refused` in `required` mode | | | Codex supports only x86_64 and aarch64 and panics elsewhere. Our equivalent must be a refusal, not a silent skip. The filter must also check the syscall architecture field (the 32-bit compat syscall path); that requirement is ours and is not in the input. |

How to report it: extend `Enforced` to include "seccomp filter installed". The status line
should say which of rules 1 to 9 are active. DeepSeek computes `full`/`partial` and shows it to
no one; ours must appear in the node status and in the tool result (as Zed tells the model when
it ran unsandboxed). With rules 1 to 4, the PR 4 residuals "UDP, DNS" close under network off,
and "pathname sockets below ABI 9" closes for stream sockets. The datagram case stays open unless
rule 5 denies `sendmsg`.

Dependency: a seccomp crate must go through the `CLAUDE.md` dependency check (exact pin, docs.rs,
licence, two weeks old, `cargo deny`). No input file evaluates one. Our own code must stay free
of `unsafe`.

### 6.3 Also on the Landlock path

1. **Protected subpaths inside writable roots.**
   - Codex keeps `.git`, `.agents`, `.codex` and `.aws` read-only.
   - DeepSeek has none, so `.git/hooks` is writable and a later unconfined `git` runs the hook.
   - Zed's Linux protection skips a `.git` that does not exist yet.
   - Check whether PR 4 protects `.git/hooks` and `.git/config` in the session folder. If not,
     add Landlock rules that leave them read-only. Test the "`.git` did not exist at start"
     case.
2. **The node's own sockets.** Codex had to mask its daemon directory because a same-uid process
   could drive an unauthenticated control socket.
   - With `network = "on"`, below ABI 9, a Landlock-confined command can connect to any pathname
     socket the node user owns.
   - Inventory every socket the node and the master open: master WebSocket, node link, MCP
     bridges, the Podman socket. Each must be authenticated or outside `SYSTEM_READ`.
   - Test: a sandboxed command cannot connect to any of them.
3. **One root derivation.** DeepSeek's file fence and its bwrap backend disagree about `/tmp`.
   Our file tools and the sandbox should derive their roots from one function. The design note's
   "same absolute mount path" requirement for containers is the same rule.
4. **Kill on Landlock.** A process-group kill leaks `setsid` children (Cline, goose; inferred for
   Oh My Pi and Pi). Oh My Pi's re-walk narrows the leak; a cgroup closes it. On the host path,
   consider a per-session cgroup or systemd scope. At least test that a `setsid sleep 1000`
   started by the agent is gone after cancel.

### 6.4 Killing inside the container

The design note assumes that killing the `podman exec` client does not kill the process inside.
No input settles that.

Keep the design's three steps, all three:

1. A wrapper prints its process-group id.
2. `podman exec <ctr> kill -TERM -<pgid>`, a grace period, then `-KILL`.
3. If that fails, `podman stop`. That step is the guarantee: with `--init`, stopping the
   container kills its whole cgroup, the same reason Codex and Zed use `--die-with-parent` and a
   PID namespace.

Add Pi-env's liveness rule: when the node loses a session's actor, it stops that session's
containers. Do not leave them for a later sweep: Warp's `sbx` cleanup is a TODO and stopped
containers pile up.

### 6.5 Do not relax checks inside a container

- Hermes skips the hardline floor, the sudo guard and the dangerous-pattern prompts on container
  backends.
- OpenClaw's default lets sandboxed exec run without approval.
- Warp's `--sandboxed` empties the denylist.
- OpenHands skips hooks for container workspaces.

A container with network can still push to a remote or exfiltrate. Keep the approval path, the
floor and checkpoints identical for host and container sessions. Add a test that a command
denied on the host is also denied in a container session.

### 6.6 Executor policy and escalation

Our node is the executor and the decider. Three models to set against it:

- Do not copy Codex's exec-server, which applies whatever sandbox intent arrives and runs
  unsandboxed when none does.
- Do not copy OpenClaw's rule that a Gateway-forwarded approval satisfies the node's allow-list.
- Copy OpenCodex's executor rules:
  - validate every argument on the executor;
  - refuse `exec` when no sandbox exists;
  - check file identity after open;
  - refuse hard links;
  - re-check the root's identity on each call.

For escalation, copy Zed and DeepSeek, not Codex:

- The model asks for a specific widening with a required reason. The grant is one-shot and bound
  to that call. A sub-agent cannot escalate (DeepSeek pins it to `never`).
- The result tells the model the truth about its isolation.
- Never let an `allow` rule remove the sandbox (Codex). Never let a "for this thread" answer
  silently unsandbox later commands (Zed).
- Never decide "denied by sandbox" from stderr keywords (Codex). Use our exit code 125 and the
  runner's own signature (DeepSeek).

### 6.7 Unavailability

- Keep PR 4's fail-closed `required` mode.
- Copy DeepSeek's functional probe: really restrict and run `true`, never a version check. Run it
  once at node start, and announce the result.
- For containers, the design note's self-test (run, exec, kill, `keep-id` ownership) is the
  matching probe. Add `podman inspect` assertions to it: no privilege, `CapEff` empty, one mount,
  `ReadonlyRootfs`, the expected network mode, the argv label.
- Zed's "Run unsandboxed" fallback prompt is a design we should not offer. A node either has a
  confinement kind or refuses the session.

### 6.8 Pass-through flags for foreign agents

If rizzy ever runs Codex or Claude Code as a sub-agent, Warp shows the failure:
`--dangerously-bypass-approvals-and-sandbox` plus pre-written trust. The foreign CLI must run
inside our confinement (Landlock or the container), never with its own sandbox turned off on the
bare node.

## 7. Not settled

- **Rootless `pasta` reachability.** Whether the host loopback and the node's sockets are
  reachable from a `network = "on"` container. No input covers Podman networking.
- **`podman exec` signal delivery.** Whether killing the client reaches the process inside. No
  input covers it (open in the design note too).
- **Podman's default seccomp profile.** Whether it denies `io_uring` and `AF_VSOCK`. Not in any
  input. OpenClaw only infers that the engine default applies.
- **`sendmsg` to pathname datagram sockets** under Codex's `Restricted` filter. The reviewer
  flagged it and nobody tested it. Whether Landlock ABI 9 covers datagram sends is also not in
  the input.
- **Why Codex rejects Landlock.** The code states only "isolate app-server sockets". The
  `connect()` explanation is inferred. Whether the daemon can spawn unsandboxed processes was
  not traced.
- **Why Zed moved from Landlock to bubblewrap.** No reason is written in the code.
- **Codex:** the Seatbelt base profile, the Windows restricted-token and MXC backends, and
  whether a `bwrap` start failure can be misread as a denial and lead to an unsandboxed retry.
  The policy-selection table and the proxy bridge internals are unchecked.
- **OpenHands:** the real `docker run` invocation, its hardening flags, server-side clamping of
  the client limits, and which names `sanitized_env()` strips. All need
  `OpenHands/software-agent-sdk`, not cloned.
- **Warp:** the self-hosted worker (`warpdotdev/oz-agent-worker`, not cloned). The `sbx` Docker
  sandbox finding is unchecked.
- **OpenClaw:** the browser sandbox and the SSH sandbox backend are unchecked. How `docker exec`
  children are killed is not in the input. The claim that a host-exec agent can edit
  `openclaw.json` is inferred.
- **Oh My Pi:** the kill semantics in `pi-shell` were not checked in review. A decision on the
  re-walk rests on the author's reading alone.
- **goose:** the process-cleanup finding (process group, Job Object, no PDEATHSIG for the shell)
  is unchecked.
- **DeepSeek:** the Windows ACL backend is unchecked, and its token code was not read. The claim
  that confined commands can read `.credentials.yaml` is inferred.
- **OpenCodex:** the macOS and Windows helper was not read. That host sockets are invisible in
  its bubblewrap argv is our inference from the mount list.
