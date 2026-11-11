# Node update and drain: design note

Status: **revised twice on 2026-11-12 after the owner's answers (§2); for sign-off on the one
question in §12.** Nothing built. Task C.14 of `ADOPTION_PLAN.md`. Two pieces fix built behaviour and may go
first (task A.19): the master tells a node why it was refused, and a stop signal stops a node
cleanly. The rest is a feature, after Gate 3 of `FIX_PLAN.md`.

**Summary.** The master states which build it requires of a node. A node whose own `node.toml`
allows it **asks its master for that build** over the authenticated node link, verifies it,
stages it beside its binary, drains, and restarts into it. The master never sends a binary, a URL
or a command unasked. When a node restarts, for an update or because its supervisor stops it, it
drains (§7) and its supervisor starts it again. **What a node trusts is its owner's choice
(§6.4):** with `update_trust = "release"` it checks the master's bytes against the hash in the
project's GitHub release, so the master alone cannot run its code there; with `"master"`, the
only choice for untagged and self-built servers, it runs whatever its master serves.

Claims about code were read on `main` at `e988625`.

## Terms

- **Build**: one `rizzy-harness` executable, identified by its SHA-256, its version, its build id
  and its target (§4.1).
- **Requirement**: the master's "this server requires build B", *recommended*, or *mandatory*
  with a *deadline* after which a node on another build is refused at admission.
- **Drain**: the node's own shutdown sequence (§7). Started only by the node.
- **Step boundary**: after one model call and its tool results are recorded, before the next
  (the loop at `crates/agent-native/src/lib.rs:1000`), or the end of the turn.

## 1. What exists, and what this must not bring back

| Area | On `main` | File |
|---|---|---|
| One binary | `serve`, `node run`, `sandbox-exec`, `pdf-text` and the rest are subcommands of one executable. | `crates/cli/src/main.rs` |
| Versions in the handshake | `node.hello` carries `protocol_version` (the constant `1`) and `version`; `master.welcome` carries `protocol_version` only. | `crates/protocol/src/lib.rs:10`, `:79-88`, `:130`; `crates/node/src/lib.rs:402` |
| Build identity | None beyond the crate version, `0.1.0` in every build. No build script in `crates/cli`; the image build excludes `.git`. CI checks that a release tag equals the crate version. | `Cargo.toml:6`; `crates/master/build.rs` (web assets only); `.dockerignore`; `.github/workflows/ci.yml:82-86` |
| Mismatch at the master | `add_link` refuses `protocol_version != 1`, but **after** `Accepted` was sent, so the node never learns why: it reconnects with backoff up to 30 s, forever. | `crates/master/src/lib.rs:894-897`; `crates/master/src/nodes.rs:234-268`; `crates/link/src/client.rs:52-63` |
| A refusal that reaches the node | `ServerMsg::Rejected { reason }` before `Accepted`: the client stops and `node run` exits with the reason. | `crates/master/src/nodes.rs:161-163`; `crates/link/src/client.rs:56` |
| Stop signal | `serve` and `node run` stop on SIGINT only; SIGTERM kills. The README unit sets `KillSignal=SIGINT`; the image `STOPSIGNAL SIGINT`. No stop closes running turns; the next start does. | `crates/cli/src/main.rs:432`, `:670`; `README.md:1054-1058`; `Dockerfile:38` |
| The sandbox helper | Each shell command runs through `sandbox-exec` of the binary **at the node's own path**, resolved once at start: "a binary replaced by an upgrade is picked up at the same path". | `crates/tools/src/sandbox.rs:670-705` |
| After a crash | `recover_interrupted` denies pending approvals (`interrupted`), writes an error, ends the turn `interrupted`. Tool processes survive a SIGKILL of the node (a known gap, C.11). | `crates/node/src/session.rs:222-232`, `:236-260`; `crates/tools/src/bash.rs:905-910` |
| Cancel | Bash gets SIGTERM to its group, SIGKILL 2 s later; a tool running 5 s after cancel is abandoned. Sub-agents run under the parent's token. | `crates/tools/src/bash.rs:24`, `:248`; `crates/agent-native/src/lib.rs:44`; `crates/node/src/session.rs:1670`, `:1702-1712` |
| Releases | One `x86_64-linux-gnu` tarball per tag, with the tarball's SHA-256 beside it (not the binary's); no signature. The binary in it is copied out of the published image, so the image and the release hold the same file. | `.github/workflows/ci.yml:114-130`; `AUDIT.md:534` |
| HTTP client | `reqwest` 0.13 with its `rustls` feature, which verifies certificates through `rustls-platform-verifier`: the operating system's trust store (the image copies the CA bundle in). | `Cargo.toml:74`; `Cargo.lock` (`reqwest` dependencies); `Dockerfile` (CA bundle) |

**What was removed, and why this is not it.** Commit `1fa8f27` (`FIX_PLAN.md` 1.2, 1.4, Q2, Q3;
`AUDIT.md` S13, D4, L2) deleted SSH node setup, in which **the master logged in to servers as
root, uploaded its own binary and rewrote firewall and sshd**, and the HTTP endpoints that
installed graphify and rewrote `node.toml`. A test keeps those routes gone
(`crates/master/tests/settings_http.rs:352`). The reasoning stands: no path may let whoever holds
the master push software or policy onto a node. This design keeps it by direction. **The node
pulls; the master never pushes.** Only a node whose owner wrote `update_mode = "auto"` and an
`update_trust` into its own `node.toml` asks for a build; no setting on the master turns it on.
What is given up is stated in §6.4.
`Dockerfile:4-5` and `AUDIT.md:502` still describe the removed upload and are stale.

## 2. Decided (owner, 2026-11-12)

| # | Question | Answer |
|---|---|---|
| D1 | Where a new build comes from | From the master, over a node-authenticated endpoint; the master has its own target's build by construction and fetches other targets from the releases once. |
| D2 | A stop signal | SIGTERM is a request, not a kill: it starts the drain. When the grace period ends, cancelling and killing are allowed. |
| D3 | Modes | All of them, under a key with an understandable name, not a bare `mode`. |
| D4 | Before signed releases exist | Distribute a compatible build from the master via the dedicated, node-authenticated endpoint. |
| D5 | An unanswered turn's running command at five minutes | Stopped (the command, not the turn). |
| D6 | May the master cut off a draining node | No. A deadline applies at admission only. |
| D7 | Guarding against a compromised master | In the owner's words: "instead of downloading releases from github, instead parse node binary hash and compare with downloaded one from master before replacing". **Reading:** the bytes still come from the master; the expected hash comes from a second, independent source, the project's GitHub release for that version, fetched by the node itself; the node installs only on a match (`update_trust = "release"`, §6.4). Signing keys are deferred (§6.5). |

## 3. Settings in `node.toml`

Top-level keys, beside `max_approval_mode` (`README.md:1027`), and one table:

```toml
update_mode = "notify"     # "off" | "notify" | "restart" | "auto"
update_trust = ""          # required with "auto": "release" | "master"
update_release_repo = "lyuksovannyy/rizzy-harness"   # the compiled-in default; a fork sets its own

[drain]
ask_minutes = 5            # the agent has this long to answer the drain note
force_after_minutes = 30   # from the start of the drain to the forced cancel
hard_stop_seconds = 60     # bound on the clean cancel before the process exits anyway
min_minutes_between = 60   # no new update drain within this long after one
```

| `update_mode` | Behaviour | Asks the master for a build |
|---|---|---|
| `off` | Logs a requirement once per connect; serves until refused at admission. | Never |
| `notify` (**default**) | As `off`, and the master shows it to server admins and to every attached workspace's admins. | Never |
| `restart` | When the binary at the node's path is already the required build (the owner's packaging put it there), drains and restarts into it. Otherwise as `notify`. | Never |
| `auto` | When the master requires or offers a newer compatible build: fetches, verifies, stages, preflights (§6), drains, restarts into it. Without a build for its target, as `notify`. | Yes |

`restart` is `auto` with nothing to fetch and shares its code; it stays a separate value because
it grants something different (the owner's packaging, not the master or a release).

**Checked at start.** `auto` without `update_trust`, or with a value other than `"release"` or
`"master"`: `node run` refuses to start, naming the setting. `update_release_repo` must be
`owner/name` of letters, digits, `-`, `_` and `.`. `auto` when the node's user cannot replace its binary
(§6.2) degrades to `notify` with the reason. `update_trust = "master"` is logged at every start
and shown by `doctor` as "this node runs code its master serves". Durations: `ask_minutes` 1 to 30,
`force_after_minutes` from `ask_minutes` to 1440, `hard_stop_seconds` 10 to 300. **The master
can change none of these.** The hard bound is 60 s because the after-turn checkpoint took up to
15.5 s on B.3's slow disk (`docs/checkpoint-cost.md`).

**Defaults.** By hand with the README unit: `notify` (its binary is root-owned). In a container:
not a supported kind (`Dockerfile:1-3`). `local`: no setting; it is the master's own process and
build (`crates/cli/src/main.rs:395`), updated when the server is (§8.2).

## 4. Builds

### 4.1 Identity

| Field | Source | Before the first tag |
|---|---|---|
| `sha256` | Computed from the file | Identifies a build exactly. |
| `version` | `CARGO_PKG_VERSION` | `0.1.0` everywhere: cannot order builds. |
| `build_id` | New: commit hash and commit time baked in at compile time (a build script when `git` is present; build arguments in the image, since `.dockerignore` excludes `.git`) | The commit time orders builds on `main`. A build without it is `unknown`: never installed, never served. |
| `target` | `std::env::consts` and the libc, as `x86_64-linux-gnu` | |

Cost **S**. So the master can read the identity of another target's build without running it,
the identity is also stored as a marked byte string it finds by scanning.

"Compatible" means: the master's own build. No version ranges and no downgrade: a node installs a
build only if it is newer than its own (by version after the first tag, by commit time before),
never on the master's word that an older one is required.

### 4.2 The master's own target

The master already holds a compatible node build: itself. At start it opens `/proc/self/exe`
(the running image, even if the file on disk was replaced since), hashes it, reads its identity,
and keeps the file open. The endpoint reads from that open file (`FileExt::read_at`, safe Rust),
so it always serves the build it runs, whatever an upgrade did to the path, with no copy (a
running executable cannot be rewritten in place; a rename leaves the open file intact).

### 4.3 Other targets: the cache

| Part | Decision |
|---|---|
| Where | `<data>/node-builds/<target>/rizzy-harness` and `meta.json` (identity, origin, time added), 0600, owned by the server's user. |
| How a build is added | (a) **By the server's admin** on the server's machine: `rizzy-harness node-build add <file>`, which refuses a file whose identity is not the master's own version and build id. No HTTP route adds one (`FIX_PLAN.md` Q3). (b) **Fetched by the master** from the release for its own version, at the first request from that target, if `config.toml` has `[nodes] fetch_builds = "github"` (default `"off"`). The URL comes from a fixed repository name, the version and the target, never from a node; it passes the outbound rules (`crates/master/src/outbound.rs`), with the release and download hosts allowed by name (the download redirects off origin). |
| Verified on arrival | The tarball's SHA-256 against the release's `.sha256`; the extracted file's identity against the master's own version and build id. That is integrity, not authenticity (both come from the same place). **For nodes with `update_trust = "release"` the cache needs no trust at all:** each node checks the bytes against the release hash itself, so a corrupted or tampered cache is refused at the node. |
| Bounds | At most 4 targets, 200 MiB per file, one build per target (the master's own); a build for an older master version is deleted at start. One fetch at a time, at most 3 attempts per target per day. |
| No build | The endpoint answers `none` with the reason; the node behaves as `notify`. |

**What works when.** Before the first tag: **same target only, plus admin-placed files**; no
releases exist to fetch. After the first tag: fetching for `x86_64-linux-gnu` only, the one asset
CI publishes. ARM and static builds need `FIX_PLAN.md` 5.11. A glibc build served to a node with
an older glibc does not start; the preflight (§6.1) catches that before any drain.

## 5. The endpoint: requests on the node link

**On the existing link, not a separate HTTPS route.** The link is already authenticated by the
node's Ed25519 key over TLS pinned to the master's certificate (`crates/master/src/nodes.rs:139-268`,
`crates/link/src/identity.rs`), a revoked node is refused and cut (`nodes.rs:73-101`, `:247-258`),
and frames are bounded (`crates/protocol/src/lib.rs:15`). A second route would need a second
authentication path, the riskier choice. Never the cookie or the API token.

| Message | Direction | Content |
|---|---|---|
| `build.query { req_id }` | node to master | "what is your build for my target?" The target is the one the node's hello declared; there is no target field to ask for another. |
| `build.info { req_id, build? }` | master to node | `{ sha256, version, build_id, target, size }`, or none with a reason. There is no field for a URL, a host or a hash source; an extra field in the JSON is ignored by the parser and reaches no code. |
| `build.fetch { req_id, sha256, offset, len }` | node to master | A range of that build; `len` at most `MAX_CHUNK` (256 KiB, `crates/protocol/src/lib.rs:20`). |
| `build.chunk { req_id, offset, data, eof, error? }` | master to node | Base64, as `artifact.chunk` (`crates/protocol/src/lib.rs:110-123`). |

Rules: only for nodes whose hello lists `update` and `update_mode = "auto"` (a node that lies gets
bytes it will not install). One transfer per node, 2 nodes at once, 3 full transfers per node per
day, from a task of its own behind session traffic; each request re-checks revocation. The answer
depends only on the target and the master's builds, so it reveals nothing about other nodes.
Base64 adds a third; it happens once per update.

## 6. Verification, install, restart, rollback, and what trust this is

### 6.1 On the node, in order, while still serving

1. `build.info`: its own target; newer than its own build (§4.1); at most 200 MiB; not listed in
   `<data>/update-failed` (§6.3).
2. With `update_trust = "release"`: the release hash (§6.5), compared with `build.info.sha256`
   **before any byte is downloaded**, so a mismatch costs no transfer.
3. Fetch into `<bin dir>/.rizzy-harness.next`, mode 0600; the SHA-256 of the bytes checked
   against `build.info` and, with `"release"`, against the release hash again.
4. File identity read and compared with `build.info`.
5. Mode 0755, flushed. **Preflight:** `.rizzy-harness.next version --json`, 10 s, empty
   environment, must report the same identity. No new trust (it is about to run anyway), and a
   build that cannot start (wrong libc) costs nobody's turn.

Any failure: the staged file is deleted, the reason logged and sent to the master for its
notice, **no drain**.

### 6.2 Install and restart

The staged file stays beside the binary through the drain. **It must not be renamed over the
binary while the node still serves:** every shell command runs `sandbox-exec` from the binary at
that path (`crates/tools/src/sandbox.rs:670-705`), so a mid-drain rename would run the new
build's helper under the old node. After the last turn has ended, the node links the current
binary to `rizzy-harness.prev`, renames `.next` over the binary path (atomic in one directory),
syncs the directory, writes `<data>/update-pending.json`, and exits.

This needs the node's user to own the binary and its directory. The README unit's binary is
root-owned in `/usr/local/bin` under `ProtectSystem=strict` (`README.md:1054-1061`): there `auto`
degrades to `notify`. An `auto` install keeps the binary under the data directory
(`/var/lib/rizzy-harness/bin/`), which the agent's sandbox does not grant (fix plan 2.7), and the
README gains that variant.

**Restart by exit status, not re-exec.** The node exits 75 and its supervisor starts the binary
at the path. `CommandExt::exec` is safe Rust and already used (`crates/tools/src/sandbox.rs:904-920`),
but a re-exec keeps the old process's PID, environment, signal mask, limits and any descriptor a
dependency opened without close-on-exec, and when the new build dies nothing is left to roll back
to. A hand-started node prints "updated; start me again" and exits.

### 6.3 Rollback

At start, a build that finds `update-pending.json` naming itself must complete the handshake with
its master within 2 minutes. If it does, it deletes the file and reports success. If not (no
`Accepted`, or a refusal), it renames `rizzy-harness.prev` back, records the failed `sha256` in
`<data>/update-failed`, and exits 76; the supervisor starts the previous build, which never
fetches that build again and reports the rollback. A build that crashes before its own code runs
is caught by the preflight, not here.

### 6.4 What trust this is

Today a compromised master can drive agent turns only inside each node's policy and sandbox
(`PLAN.md` T5, T6). `auto` changes that by the node owner's choice, to a degree set by
`update_trust`:

| `update_trust` | Who must be compromised to run code on the node | Works with |
|---|---|---|
| `"master"` | **The master alone.** Whoever controls it runs code of their choice as the node's user, outside the sandbox, on every node in `auto` with this setting. | Any master build, tagged or not, self-built included. |
| `"release"` | **The master and the project's GitHub releases** (the account, a workflow, or the release assets), or TLS to GitHub. | A master running a tagged release binary or the published image of that tag. |
| `"signature"` (later, §6.5) | The master and an offline signing key. | Signed releases. |

Opt-in per node, never the default, logged at every start and shown by `doctor`. Integrity holds
whatever the trust: hash, target, identity, size, never older.

**`"release"` is the documented setting for `auto` from the first tag on.** It trusts what anyone
who downloads the release directly trusts, the install path of every other harness's installer.
**`"master"` is for untagged and self-built servers,** by the node owner's explicit choice: a
fair trade when one person runs the master and its nodes, not for a node attached to someone
else's master or a shared node serving people who do not run it.

### 6.5 The release hash

| Part | Decision |
|---|---|
| The URL | Built by the node only: `https://github.com/<update_release_repo>/releases/download/v<version>/rizzy-harness-v<version>-<target>.bin.sha256`. The repository comes from `node.toml` (compiled-in default `lyuksovannyy/rizzy-harness`); the target is the node's own; the version is the one in `build.info`, accepted only as plain `MAJOR.MINOR.PATCH` digits, so nothing the master sends can add a path, a host or a query. **Never a URL, a host or a hash source from the master.** |
| Transport | The node's existing `reqwest` client, HTTPS through the operating system's trust store (§1). GitHub answers a release download with a redirect to its asset host: redirects are allowed only to `github.com` and hosts under `githubusercontent.com`, at most 3. Timeout 15 s; answer capped at 1 KiB; no token. |
| Parse | Exactly one line, 64 lowercase hex characters, two spaces, `rizzy-harness`; anything else is refused. |
| Failure | GitHub unreachable, rate-limited or answering 404: retried with backoff (1, 5, 15, 60 minutes, then once a day), and meanwhile the node behaves as `notify` with the reason. **It never falls back to trusting the master: no automatic downgrade of trust, ever.** No drain until verified. |
| A private fork | Not supported in `"release"` mode (it would need a GitHub token on every node); such a node uses `"master"`. |
| The workflow change | The release step publishes one more asset per target, the binary's own hash: `sha256sum rizzy-harness > "$name.bin.sha256"` beside the tarball's (`.github/workflows/ci.yml:124-127`), added to `gh release create`. **S.** |

**Consequences, plainly.**
- **A master built from source cannot serve `"release"` nodes.** Its binary is not the release
  artifact, so the hashes never match: that is the property working. Those nodes behave as
  `notify` and say why. A master running the release binary, or the image of that tag, matches.
- **Nothing works before the first tag, nor between tags** (the `:edge` image is not a release):
  only `"master"` covers an untagged build.
- **A node with no route to GitHub** cannot verify and stays at `notify`, with the reason.
- An older release named by the master is refused as before (never older).
- Defends against a compromised master and a corrupted cache; not against a compromised GitHub
  account, workflow or release, or broken TLS to GitHub.

**GitHub, checked 2026-11-12 in its documentation.** *Immutable releases* are a repository or
organisation setting: a published release's assets cannot be modified or deleted, its tag cannot
move, and GitHub records a release attestation ("Immutable releases"). **Turn it on before the
first tag**, so a `.bin.sha256` cannot be swapped later; its interaction with `gh release create`
uploading in the same call is not confirmed. The REST asset object has a `digest` field ("Release
assets"), format not confirmed; it is the tarball's, not the binary's, so it does not replace the
asset above.

**Signature, later.** A release signing key whose public half each node's owner pins would make
the master and GitHub both mere carriers: it survives a compromised GitHub account if the key is
kept offline. It needs tagged releases, a signing step, key custody and rotation, none of which
exist, and `"release"` covers the threat this note is about. Deferred, not built in N.9; its test
would be `signature_trust_refuses_an_unsigned_or_wrongly_signed_build`.

### 6.6 An escaped agent

**An agent that escaped the sandbox** can replace a binary the node's user owns, and so persist.
`auto` adds no write access that it lacks today: it can already rewrite `node.toml`, which the
node's user owns (`README.md:1023`) and which widens policy at the next start. A node whose
binary and `node.toml` are root-owned is safer, and cannot use `auto`.

## 7. The drain

### 7.1 What starts one

| Trigger | Exit |
|---|---|
| SIGTERM or SIGINT, from the owner or the supervisor | 0 |
| `restart` or `auto` with a verified, preflighted build (§6.1), and no drain in the last `min_minutes_between` | 75 |
| A second SIGTERM or SIGINT during a drain | skips to the forced point at once |

A signal is a request; when the grace ends (the forced point, or a shorter supervisor timeout),
cancelling and killing are allowed. Default timeouts cut the drain short (systemd 90 s, Docker
10 s): the shipped unit sets `TimeoutStopSec=35min` with SIGTERM, compose `stop_grace_period: 35m`.

### 7.2 The note and the answer

At the start the node appends `node_draining` (§8.1) to every session with a running turn. From
the next step boundary the model sees a fixed note from the harness, not a user message and not
tool output:

> This node must restart (update / stopped by its owner). If you are nearly done, finish.
> Otherwise stop at a safe point: call `drain_answer` with `pause` and say what remains. Work
> still running at 14:32 will be cancelled.

`drain_answer { choice: "finish" | "pause", remaining?: string }` is added to that turn's tools
from then on and exists nowhere else (one prompt-cache miss per turn per drain).

| The first model call after the note | Outcome |
|---|---|
| calls `drain_answer` with `pause` | **Pause.** Nothing else in that message runs; the turn ends `completed`, `drain.outcome = "paused"`; `remaining` is in the recorded call. |
| calls it with `finish`, calls other tools, or ends the turn | **Finish.** The turn runs to its end, bounded by the forced point. |
| has not completed within `ask_minutes` | **Not answered** (§7.3). |

### 7.3 Timeline

| From the start | What happens |
|---|---|
| 0 | New turns, sessions, restores and stages are acked `error: "draining"` with `retry_after_ms`, shown as a notice, not an error. Approval answers and cancels are still taken. |
| 0 to 5 min | Agents get the note at their next step boundary and answer. |
| 5 min | An unanswered turn inside a tool call has **that call stopped** (its own cancel, output kept), so the agent answers with the note in front of it; the turn is not cancelled. A turn waiting on its provider or on an approval is left (§7.4). |
| 5 to 30 min | Nothing is cancelled. When the last turn has ended and no restore runs, the node goes to exit. |
| 30 min | **Forced:** every remaining turn is cancelled through the normal path (process trees killed within the existing grace, approvals denied, `drain.outcome = "cut"`, the after checkpoint if it can be taken). |
| + 60 s | A cancel that has not closed its turn is abandoned; the next start closes it, naming the update (§8.2). Tool processes may survive (C.11). |
| exit | Up to `hard_stop_seconds` for acknowledgements; containers stopped; install (§6.2); exit. |

### 7.4 Cases

- **Sub-agents.** A child runs under its parent's token while the parent is blocked in the
  delegate call, so the parent cannot answer while its child works. Children get the note; a
  child's `pause` returns its summary to the parent as its result; the parent's answer then
  decides for the turn. Cancelling the parent cancels the children.
- **An approval waiting on a person** is not the agent answering. It stays open with the forced
  time on its card; an allowed command runs until the forced point; at 30 minutes it is denied,
  `resolved_by: "interrupted"`, as `close_open_turn` does (`crates/node/src/session.rs:254`).
- **Routines.** The runner treats `draining` like an offline node and retries at
  `retry_after_ms`, not as a refusal that blocks the routine (`runner.rs:736`). A paused or cut run
  records `interrupted` with the reason, does not count toward the five failures, and is not re-run.
- **A restore in progress** is never cut by the node: the drain waits for it, past the forced
  point. If the supervisor kills the process anyway, `before_restore` lists the paths and a second
  restore puts them back.
- **Unacknowledged events** replay at the next hello, from the node's log on `main` or from the
  spool (`docs/nodes-credentials-environments.md` §4.12). **Containers (N.4)** are stopped when
  their turn ends, and orphans at start. **Several workspaces:** one drain; every attached
  workspace's admins are told. **Offline when a requirement is issued:** it arrives in the next
  `welcome`.

### 7.5 Abuse

- **Repeated requirements** drain a node only with a verified newer build; `min_minutes_between`
  and the failed-build list fence the rest.
- **A prompt-injected agent** cannot start, cancel or extend a drain (no tool or message exists
  outside one; the node fixes the forced time) nor reach the build endpoint (the link belongs to
  the node process, not the shell). Text imitating the note can make it stop early, as injected
  text already can.
- **A hostile master** can send a node in `notify` or `restart` nothing it will run.

## 8. What else changes

### 8.1 Protocol and events

- `Hello::Auth` gains `version`, `build_id`, `protocol_version`; `node.hello` gains `target`,
  `update_mode` and the capability `update`; `master.welcome` gains `requirement?`. Old peers
  ignore new fields.
- New `node.update_required` (while connected, only to nodes with `update`), `node.draining`
  (node to master, only if the welcome announced it), and the four `build.*` messages (§5); `ack`
  gains `retry_after_ms`. Nothing carries a URL or a command.
- Events, a schema change allowed before the first tag: new kind `node_draining { drain_id,
  reason, build?, ask_until_ms, force_at_ms }`, because the note is neither the person's message
  nor an error and must replay the same way; and `TurnEnded` gains `drain: Option<{ drain_id,
  outcome: "finished" | "paused" | "cut" }>` beside `reason` (`crates/core/src/event.rs:512`).

### 8.2 The marker, the chat, `local`

`<data>/drain.json` is written at the start and removed at a clean exit; `recover_interrupted`
reads it to write "The node restarted for an update while this turn was running" and set `cut`.
After the restart each drained turn shows one line (finished, paused with its summary, or cut off);
paused and cut turns show **Continue**, a fixed prompt sent as the person's own message. Nothing
resumes by itself (`PLAN.md` §2), unlike OpenCode's automatic resume
(`research/round2/synth/05-links-workers-protocols.md` §2, OpenCode, item 8).

**`local` and `serve`.** An update of the server is an update of `local` and of the chat runtime
(`crates/cli/src/main.rs:395`, `:403`): the operator restarts the server; the master never
restarts itself. On the stop signal `serve` drains `local`'s turns **while still serving HTTP**,
so people can see the drain and answer approvals, then gives HTTP its 30 s grace
(`crates/master/src/serve.rs:62`). The image runs no `local` (`Dockerfile:34`), so there only chat
turns drain.

### 8.3 Version gate and skew

OpenClaw refuses a worker whose build differs; OpenHands one older than a minimum (`05` §6.9).
Ours, at authentication and **before `Accepted`**: a protocol outside the master's range, or a
build other than a mandatory requirement past its deadline, gets `Rejected { reason }`. Today's
check after `Accepted` goes (task A.19). A node must understand the update messages before it
can be told to update, so the first build that has them is installed by hand everywhere. Admitting
an older protocol until a deadline is compatibility code, which `CLAUDE.md` forbids before the
first tag: until then a protocol change is a refusal with a reason, and `auto` replaces only
builds of the same protocol. **Upgrade order: master first, then nodes.**

### 8.4 Supervisors

| Install kind | Settings | Result |
|---|---|---|
| systemd (README) | `TimeoutStopSec=35min`, SIGTERM, `Restart=always` | Drains on `systemctl restart`; exits 75 for an update; root-owned binary: `auto` degrades to `notify`; the `auto` variant keeps it under `/var/lib/rizzy-harness/bin/` and rolls back with 76. |
| The server image | `stop_grace_period: 35m` | The runtime's stop drains `serve`. |
| Hand-started | none | Exits and says so. |

### 8.5 Invariants

**The node decides** whether it ever asks for a build, whom it trusts for code, and every
duration; what is given up is exactly §6.4, with the owner's yes of 2026-11-12, and `PLAN.md` T5
must say so when built. The master still runs no tools; drain events come from the session actor;
`drain_answer` decides only its own turn; no compatibility code before the first tag (§8.3); the
master holds one open file, not a copy.

### 8.6 Threat-model row (draft, for `PLAN.md` when built)

| # | Threat | Mitigation |
|---|---|---|
| T35 | A compromised master serves a malicious build to nodes that update from it, or uses requirements to keep nodes from working; a prompt-injected agent starts, cancels or stretches a restart | **The node pulls; the master never pushes.** A node asks for a build only with `update_mode = "auto"` and an explicit `update_trust` in its own `node.toml`; nothing on the master enables it. **With `update_trust = "master"` a compromised master runs code of its choice as the node's user, outside the sandbox, on every such node: accepted, by each node owner's choice, logged at every start and shown by `doctor`.** With `"release"` the node fetches the binary's hash itself from a release URL it builds from its own `node.toml` and a plain version number, compares it before downloading and after, and refuses any mismatch, so the master alone cannot run code there; GitHub unreachable leaves the node at `notify`, never at `"master"` trust. Always: hash, target, identity and size checked, never older, preflighted, renamed in after the drain, rolled back on a failed handshake, never retried. The endpoint is on the authenticated link, own target only, rate-limited, refused to revoked nodes. The drain's durations are the node's; the agent chooses only finish or pause for its own turn. Tests: `docs/node-update.md` §9. **Residual:** `"master"` trust as above; with `"release"`, a compromised GitHub repository or release together with the master; an escaped agent can replace a node-owned binary, as it can already rewrite `node.toml`. |

## 9. Tests the build commits to

1. `a_node_not_in_auto_never_requests_a_build`: `off`, `notify` and `restart` send no `build.*`.
2. `the_master_sends_nothing_unasked`: no `build.info` or `build.chunk` without a request, in every mode.
3. `the_build_endpoint_refuses_without_node_authentication`: no route outside the authenticated link reaches it.
4. `a_revoked_node_gets_no_build`: revoked during a transfer, the next chunk is refused.
5. `a_node_cannot_fetch_another_targets_build`: the answer follows the hello's target only.
6. `a_build_with_a_wrong_hash_target_or_identity_is_refused_and_nothing_is_installed`.
7. `a_build_older_than_the_running_one_is_refused`.
8. `a_build_whose_hash_differs_from_the_release_is_refused`: nothing downloaded, nothing installed, no drain.
9. `auto_without_update_trust_refuses_to_start`, and with a root-owned binary degrades to `notify`.
10. `a_failed_download_or_verification_causes_no_drain`.
11. `a_build_that_fails_preflight_causes_no_drain`.
12. `the_binary_is_replaced_only_after_the_drain`: a command run during the drain uses the old helper.
13. `a_new_build_that_fails_its_first_handshake_is_rolled_back_and_not_retried`.
14. `the_master_serves_the_build_it_runs_after_its_file_was_replaced`.
15. `a_sigterm_drains_and_a_second_one_forces`.
16. `a_protocol_mismatch_is_refused_before_accepted_with_the_reason`.
17. `new_turns_are_refused_during_a_drain_with_the_reason`.
18. `a_paused_turn_ends_with_its_summary_and_continues_after_restart`.
19. `an_unanswered_turns_command_is_stopped_at_ask_minutes`: a `sleep 3600` is stopped and the agent gets the note.
20. `the_forced_point_cancels_cleanly_and_names_the_update`.
21. `a_restore_in_progress_is_not_cut_by_the_drain`.
22. `unacknowledged_events_survive_the_restart`.
23. `repeated_requirements_cannot_keep_a_node_from_working`.
24. `an_agent_cannot_start_or_extend_a_drain`.
25. `a_draining_refusal_does_not_block_a_routine`.
26. `an_approval_left_open_is_denied_at_the_forced_point`.
27. `the_node_never_takes_a_url_or_hash_source_from_the_master`: a `build.info` with extra `url`, `host` or `hash_url` fields fetches only the node-built URL.
28. `a_master_supplied_version_with_path_characters_is_refused`: `1.2.3/../x`, `1.2.3?a` and `v1.2` are refused before any request.
29. `github_unreachable_leaves_the_node_at_notify_not_master_trust`.
30. `a_self_built_masters_build_is_refused_under_release_trust`.

## 10. Build order and size

| Step | What | Size | When |
|---|---|---|---|
| 1 | **A.19:** version check before `Accepted` with the reason; SIGTERM handled like SIGINT; a stop refuses new work, cancels running turns through the normal path with a reason, closes them, exits; README unit (tests 15, 16, 20) | **M** | A fix: now |
| 2 | Build identity: build script, CI arguments, the marked string | **S** | After Gate 3 |
| 3 | The full drain: phases, the note, `drain_answer`, events, marker, Continue, routines (tests 17 to 26) | **L** | After Gate 3, on step 1 |
| 4 | Requirement: storage, admin setting, `welcome`, notices; `off`, `notify`, `restart` (tests 1, 23) | **M** | After 3 |
| 5 | The endpoint on the link and the master's own build (tests 2 to 5, 14) | **M** | After 2 |
| 6 | Node side of `auto`: fetch, verify, preflight, install after drain, rollback, `"master"` trust (tests 6, 7, 9 to 13) | **L** | After 3 and 5 |
| 7 | `"release"` trust: the hash fetch, URL building, version check, no fallback (tests 8, 27 to 30); the workflow's `.bin.sha256` asset; immutable releases turned on | **M** (fetch) + **S** (workflow) | After 6; usable from the first tag |
| 8 | The cache: `node-build add`, the GitHub fetch | **M** | After 6; the fetch after the first tag |

`signature` trust is not built (§6.5). About five weeks in all. Nothing from the nodes design is needed first: N.1 adds the spool
flush and per-workspace notices, N.4 container stop and reconcile.

## 11. Not settled

- The size of a release binary, and so the transfer time over the link (not measured).
- Whether `/proc/self/exe` behaves as described inside the server's container image (test 14).
- Tool processes surviving a hard stop (C.11; `crates/tools/src/bash.rs:905-910` shows they do today).

## 12. Questions for the owner

| # | Decision | Recommended | If you choose the other |
|---|---|---|---|
| Q1 | D7 as read in §2: the bytes come from the master, the hash from the GitHub release, fetched by the node. `"release"` becomes the documented setting for `auto` after the first tag; `"master"` remains for untagged and self-built servers by the node owner's explicit choice; signature trust is deferred. Confirm? | **Yes.** | If `"master"` should not exist at all: `auto` does nothing before the first tag or on a self-built master, and those nodes stay at `notify` or `restart`. If signatures are wanted now: tagged releases, a signing step and key custody first, about a week more. |

Withdrawn: the earlier Q2 (where a signing key lives), since signatures are deferred (§6.5).
