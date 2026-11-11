# FIX PLAN — rizzy-harness

Fixes everything in [`AUDIT.md`](AUDIT.md) (commit `62c17fd`). Section 8 maps
every audit finding to a task here, so nothing is dropped.

## 1. Rules for the whole plan

1. **Feature freeze.** No new feature merges until the gate of Phase 3 is
   passed. Every hour spent on features now widens what has to be fixed.
2. **Delete before you fix.** Phase 1 removes code. Hardening something that
   is about to be deleted is wasted work, and removed code has no bugs.
3. **A fix lands with the test that proves it.** The test is written first
   and must fail on the old code. A security fix without such a test is not
   done.
4. **`main` stays green.** No merge with a red or still-running check.
5. **One task, one pull request.** Tasks are sized so that is possible.
6. **A new dependency follows the project's own rule** (`PLAN.md` §0): exact
   pin, API checked on docs.rs, at least two weeks old, passes `cargo deny`.
   Crates named below are candidates; check each before adding it.
7. **Update the threat model in the same pull request** as the code it
   describes. Several findings exist only because the two drifted apart.

Sizes: **S** under half a day, **M** one to two days, **L** three to five
days. They are estimates for one person working with an agent, not promises.

## 2. Decisions needed from the owner

The plan proceeds on the recommended answer unless you say otherwise. Each
one changes scope, so decide before the phase that depends on it starts.

| # | Decision | Recommended | If you choose the other |
|---|---|---|---|
| Q1 | Auth and storage back ends | **Decided:** remove JWT, refresh tokens and the KV layer; go back to a server-side session table. Remove the Redis and MariaDB containers from the templates until code uses them. Keep a MariaDB port with Redis as a planned milestone (section 3, Phase 6) with a written trigger. | — |
| Q2 | SSH provisioning (create users, WireGuard, nftables, sshd, watchdog) | **Decided: cut.** Nodes are set up by hand with the join command; the node app sets up and checks its own sandbox. | — |
| Q3 | Remove the HTTP endpoints that install graphify and edit `node.toml`? | **Yes.** Keep the CLI command; the UI shows it. | Keep: hash-pin the package and switch the editor to `toml_edit` with an atomic write, and delete the T5 claim. |
| Q4 | The home-screen "wall" lists scraped from Google's autocomplete | **Decided: keep for now.** Cosmetic and low risk. Skim the lists once for anything you would not choose to show; replace with a hand-written list when convenient (required before any commercial use). | — |
| Q5 | May a project member put text into shared project memory without a manager? | **No.** A manager activates; add a per-project switch to allow members. | Leave as is and record it in the threat model as an accepted cross-user injection path. |
| Q6 | Sandbox network default for the agent's shell | **On**, with `network = "off"` per node, because builds and installs need it. An allow-list proxy comes later. | Off by default: safer, and most real coding work breaks until the proxy exists. |
| Q7 | Who may use the embedded local node | **Server admins only.** Other accounts need a remote node. | Everyone with the grant: then task 2.7 (sandbox) must ship before a second account is created. |

## 3. Phases

### Phase 0 — Make the process trustworthy (half a day)

| ID | Fixes | Change | Done when |
|---|---|---|---|
| 0.1 | §5.1 | `web/scripts/size.mjs`: re-measure, set `BASE_KB` to the measured total with the reason. Make the total limit what its comment says it is: print a warning, do not fail. The first-load ceiling, lazy-chunk checks and library markers stay hard. | `pnpm size` exits 0 at HEAD; CI is green on `main`. **S** |
| 0.2 | §5.1 | GitHub branch protection on `main`: required checks `rust`, `deny`, `web`, `image`; no direct pushes; no merge while checks run. *Owner action in repository settings.* | A pull request with a failing check cannot be merged. **S** |
| 0.3 | §5.2 | `ci.yml`: `on: push: branches: [main], tags: ['v*']` plus `pull_request`. | A pull-request commit runs CI once. **S** |
| 0.4 | §5.1 | README: remove the `docker pull …:edge` line from the top until Phase 4's gate is passed; say plainly that there is no upgrade path yet. | README no longer invites installs. **S** |

**Gate 0:** `main` green, protection on.

### Phase 1 — Cuts (one to two days)

| ID | Fixes | Change | Done when |
|---|---|---|---|
| 1.1 | D1, L9 (needs Q1) | Restore server-side sessions: one opaque 256-bit token in an `HttpOnly; SameSite=Strict` cookie, its SHA-256 in a `sessions` table (`user_id`, `created_ms`, `last_seen_ms`, `expires_ms`), 30-day sliding expiry. `authenticate` is one indexed lookup joined to `users`. Delete `jwt.rs`, `kv.rs`, `/api/auth/refresh`, the legacy-cookie clearing, the per-user session version in `kv`, `web/src/session.ts` and its `fetch` patch, the `redis` dependency, `HARNESS_JWT_SECRET` / `HARNESS_KV_URL` / `HARNESS_DATABASE_URL`, the Redis and MariaDB services in `docker/` (they return with Phase 6). `docs/storage-backends.md` keeps the MariaDB section, rewritten as the Phase 6 plan. | Logout, disable, delete and password reset each remove the user's rows and the next request is 401 (tests). `grep -ri "jwt\|redis"` finds nothing outside the changelog and the Phase 6 plan; no template starts a service the server does not use. `docker/standard` is gone or is `minimal`. **M** |
| 1.2 | S13 (needs Q3) | Delete `POST /api/admin/graphify/install`, `PUT /api/admin/graphify` and `set_toml_bool`. The settings page shows `rizzy-harness graphify install` and the `node.toml` line to add. | No HTTP handler writes `node.toml` or spawns an installer. T5 is true again. **S** |
| 1.3 | §5.2 (Q4) | Read through `web/public/wall/*.txt` once and remove anything unsuitable. Record the source of the lists in `THIRD_PARTY_NOTICES.md`. Replacing them is optional, not scheduled. | Lists reviewed; provenance noted. **S** |
| 1.4 | D4, L2 (Q2) | **Cut SSH provisioning.** Delete `crates/provision`, `master/src/provision.rs`, `node_setups.rs`, their routes, UI (`ServerSetup`, `ManagedServers`), the watchdog, `e2e/setup.cjs` and the `russh` dependency. Keep "Create join command". `node join` prints what it will do, and `node run` checks at start that its sandbox works (2.7) and that its data directory is outside every workspace root (2.1), refusing to start otherwise. README: a short manual node setup (user, folder, join, systemd unit to copy). | `cargo tree` has no `russh`; a node set up from the README passes its own start checks. **M** |
| 1.5 | D7, §5.2 | Remove dead code: `searchUrl` in `lib/engines.ts`, `highlight()` in `md/highlight.ts`, three of the four FNV-1a copies, `.idea/` (and add it to `.gitignore`). | `pnpm typecheck` and tests pass; `git ls-files .idea` is empty. **S** |

**Gate 1:** auth is one table and one cookie; about 4,500 lines gone; CI green.

### Phase 2 — Contain the agent (one to two weeks)

This is the phase that matters. Order inside it is by cost: the cheap
structural fixes first, the sandbox last.

| ID | Fixes | Change | Done when |
|---|---|---|---|
| 2.1 | S1 | **Keep the data directory out of every session.** `NodePolicy::check_cwd` refuses a cwd that is inside the node's data directory, equal to it, *or an ancestor of it* (so `$HOME` is refused while the data directory lives under it). `ToolCtx::resolve` refuses any path under the data directory even with `allow_outside_workspace`. Add the API token to the redaction set. (Needs Q7:) the master refuses agentic sessions on the embedded node for accounts that are not server admins, at create and at every prompt. | Tests: cwd `$HOME`, cwd `<data>`, cwd `<data>/x` are refused; `read ../../.local/share/rizzy-harness/api-token` from a project folder is refused; a non-admin with the agentic grant gets 403 on the local node and works on a remote one. **M** |
| 2.2 | S1, S11 | **Allow-list the shell's environment.** `env_clear()`, then pass `PATH HOME USER LOGNAME SHELL LANG LC_* TERM TZ TMPDIR` plus `env_pass = [...]` from `node.toml`. Same for `git` and graphify child processes. `node run` uses the same code path as `serve`. | Test: a variable set in the server's environment and not listed is absent in `env` output, on the embedded node and through `node run`. README documents `env_pass` with the usual toolchain variables (`CARGO_HOME`, `NVM_DIR`, `SSH_AUTH_SOCK`, proxies). **S** |
| 2.3 | S2 | `OutputStore`: random ids (64 bits, 16 hex characters, short enough for a model to copy); entries keyed by session; `get` requires the caller's session. | Tests: 1,000 puts in one millisecond give 1,000 distinct ids; another session's id is "not found". **S** |
| 2.4 | S3 | **Cap streamed output.** A per-call byte budget (default 1 MiB, in `node.toml`) for `ToolOutputDelta`; past it, one marker event `[output truncated: … not shown]` and the rest is read and discarded so the child does not block. Replace the three unbounded channels on this path with bounded ones. | Test: `yes` for 5 seconds produces at most budget + one marker in the log, and resident memory stays flat. T9 in the threat model is true. **M** |
| 2.5 | S8 | **Symlink-safe paths.** `resolve` walks components with `symlink_metadata`; a symlink is followed only if its target resolves inside the workspace; a dangling symlink is refused. `write` and `edit` open with `O_NOFOLLOW` on the final component. Writes under `.git/` are refused. | Tests: dangling link as last component, dangling link as a middle component, link to an outside existing file, link to an inside file (allowed), `.git/config` (refused). **M** |
| 2.6 | S9 | **Git without the repository's own config.** Checkpoints always use a harness-owned git directory (`--git-dir <data>/shadow/<id> --work-tree <root>`), as already done for folders that are not repositories; point it at the user's objects with `GIT_ALTERNATE_OBJECT_DIRECTORIES` so nothing is stored twice. Every git call: `GIT_CONFIG_NOSYSTEM=1`, `GIT_CONFIG_GLOBAL=/dev/null`, `-c core.fsmonitor=false -c core.hooksPath=/dev/null`, a timeout; diffs add `--no-ext-diff --no-textconv`. Sub-agent worktrees still run in the user's repository: before the first one, list local config and show a notice if it defines `filter.*`, `core.sshCommand`, `diff.*.textconv` or hooks. | Test: a repository whose `.git/config` sets `core.fsmonitor` to a script that touches a file; after a prompt the file does not exist. Test: a `post-checkout` hook does not run on worktree add. Residual (filters during worktree checkout) is written into the threat model. Borrowing has a residual too (B.2, A.16): the user's own history rewrite plus pruning can make an older checkpoint unrestorable, which is refused with the reason and nothing changed (T33), and a restore copies the blobs it writes into the shadow first. **M** |
| 2.7 | S1 | **Sandbox the shell with Landlock.** New subcommand `rizzy-harness sandbox-exec --rw <dir>… --ro <dir>… -- <cmd>`: applies a Landlock ruleset to itself, then `exec`s the command. That needs no `unsafe` (no `pre_exec`) and no extra program on the node. Bash, graphify and the PDF helper (3.4) are started through it. Defaults: read-write the session cwd and a per-session temp directory; read-only `/usr /bin /lib* /etc /opt` and the lists `sandbox.read` / `sandbox.write` in `node.toml` (toolchains and caches under `$HOME`); nothing else, so `~/.ssh`, `~/.aws`, the data directory and other users' folders do not exist for the agent. `sandbox = "required"` (default: refuse agentic sessions on a kernel without Landlock, with a message naming the setting), `"best-effort"` or `"off"`. Network per Q6: on by default, `network = "off"` denies TCP connect and bind where the kernel supports it. | Tests on a Landlock kernel: `cat ~/.ssh/id_ed25519` and `cat <data>/api-token` fail with permission denied from bash; a build in the cwd works; with `network = "off"` a TCP connect fails. CI runs these on `ubuntu-latest`. `PLAN.md` R2 is closed; the network residual (UDP and DNS are not covered by Landlock) is written down. **L** |
| 2.8 | S10 | **No tool can wedge a node.** `read`: refuse anything that is not a regular file; read through a buffered reader with a byte cap instead of loading the file. `grep`: skip non-regular files and files over a size cap. Every tool receives a live cancellation token and returns on it. The node's command loop hands each command to its session's task; `Cancel` and `ApprovalRespond` never wait behind `Restore` or `Delete`. | Tests: `read` on a FIFO returns an error at once; `read` on a 2 GB sparse file stays under the cap; cancel during a slow restore is answered within a second. **M** |
| 2.9 | S11 | **Redaction that does not split secrets.** Hold back the last (longest secret − 1) bytes of each streamed chunk until the next one arrives, so a secret across a flush boundary is still replaced. If scrubbing breaks JSON, scrub the string fields, not the serialized text. State in the threat model that redaction guards against accidents only. | Test: a secret split across two chunks at every offset is redacted. **S** |

**Gate 2:** on a default install, an agentic session cannot read or write
anything outside its working folder and its declared tool directories, and
no command can fill the disk or block cancel. Each line of that sentence has
a test in CI. Only after this gate may a second account get the agentic grant.

### Phase 3 — Harden the server (about one week)

| ID | Fixes | Change | Done when |
|---|---|---|---|
| 3.1 | S4 | **Login.** Validate the username format before anything else (invalid format: the same 401, no map entry, no hash). Reserve-then-release throttle, as `try_reserve_lookup` already does. Bound the failure map (fixed capacity, oldest evicted). A semaphore around Argon2 (4 at once, short bounded wait, then 429). Same limits on `/api/setup`. | Tests: 10,000 logins with random 1 MiB usernames leave the map empty and memory flat; 50 parallel wrong passwords for one account record 5 failures and reject the rest; at most 4 hashes run at once. **M** |
| 3.2 | S5 | **Sockets end with the login.** Each UI socket keeps its session id. Logout, disable, delete and password reset broadcast a per-user message that closes that user's sockets (or the one session's); each socket also re-checks its session every 60 seconds and before every subscribe. Message size limit 64 KiB; at most 8 sockets per user. | Tests: logout closes the socket within a second; a disabled account's socket closes; a 1 MiB client frame is rejected. T2 and T12 are true for WebSockets. **M** |
| 3.3 | S6 | **Server limits.** Serve through a hyper-util builder with a timer (or `axum-server`, if it passes rule 6): header-read timeout 10 s, a connection cap, graceful shutdown. `tower-http` `TimeoutLayer` (30 s) on every route except uploads, blob downloads and the WebSocket. If `axum-server` is used it also replaces the hand-written TLS accept loop. | Test: a client that sends half a request line is disconnected after the timeout; a handler that sleeps is cut at 30 s. **M** |
| 3.4 | S7 | **PDFs out of process.** `rizzy-harness pdf-text`: reads a PDF on stdin, writes text on stdout, sets its own address-space limit at start, runs under `sandbox-exec` with no filesystem access (2.7). The server starts it with a 20-second timeout and kills it; 2 at once. A file is a PDF only by its `%PDF` header, never by name. Per-user cap on unsent attachments and a sweep for old ones. `/api/transcribe`: one at a time per user and a daily cap in config. | Tests: a PDF that makes the parser loop is killed and the server stays responsive; a `.pdf` text file is treated as text. `deny.toml` exception for `ttf-parser` names this task. **M** |
| 3.5 | S12 (needs Q5) | Activating a project fact requires "may manage"; a member's proposals stay proposals. Optional per-project setting to allow members. | Test: a member's `PATCH {status: "active"}` on their own proposal is 403. **S** |
| 3.6 | L3–L8 | Provider "test" and "model list" calls use `url_allowed` and the public-address resolver unless the host is in `HARNESS_PLAIN_HTTP_HOSTS`. 500 responses carry a fixed message and a request id; the detail goes to the log. `source_path` is returned to the artifact's owner only. Cookie-authenticated state-changing requests must carry an allowed `Origin` with the server's scheme. `Strict-Transport-Security` with `--tls`; `Cache-Control: no-store` on `/api/*`. Secret files are refused when group- or world-readable. `docs/media.md`: describe the blob scheme as crypto-shredding, add the format specification and test vectors. | One test per item. **M** |
| 3.7 | L1, L15 | Node link: bounded channels with back-pressure in both directions; connect and handshake timeouts on the node; snapshot batches limited by bytes. Either implement `allow_remote_sessions` in `node.toml` or remove the sentence from T5. Correct "mutual challenge-response" in the docs. | Test: a node that floods frames is throttled, master memory stays flat. **M** |
| 3.8 | — | *Removed: provisioning was cut in 1.4.* | — |
| 3.9 | L10–L13 | "Allow for this turn" on bash is labelled as exactly that and offered only after the first approval. `web_fetch` provenance counts user messages and successful results of search, fetch and read tools only, never error text. `is_public` rejects 6to4, Teredo and IPv4-compatible IPv6. The mem0 client percent-encodes ids and uses the same-origin redirect policy as the other clients. | Tests: a URL that appears only in an error message is refused; `2002:7f00:1::` is not public. **S** |
| 3.10 | L14 | Browser: drafts are cleared on logout and chat delete and never stored for incognito chats; Markdown `#` links are limited to footnote ids; a user's temporary password is generated by the server and shown once, not typed into `prompt()`. | Tests for each. **S** |

**Gate 3:** every High and Medium finding and every Low in the audit is
closed with a test, or recorded in the threat model as accepted with a
reason. Ask for one outside review of `crates/link`, the approval path and
the sandbox before this gate is called passed. **The feature freeze ends here.**

### Phase 4 — Foundations (one to two weeks)

| ID | Fixes | Change | Done when |
|---|---|---|---|
| 4.1 | D6 | **Auth by construction.** First, a test that walks the route table and asserts every route outside a short public list answers 401 without credentials: that is cheap and should land on day one of this phase. Then `Principal`, `ActiveUser` and `Admin` as axum extractors; public, user and admin routers. | The test exists and passes; no handler calls `require*` by hand. **M** |
| 4.2 | D6, L4 | Split `http.rs` by area into `http/*`; one `ApiError` enum that logs server errors and maps them; one keyed rate-limit module (`governor`) replacing the five throttles; file IO in handlers moved to `spawn_blocking`. | `http.rs` holds the router and shared types only. **M** |
| 4.3 | D2 | **Migrations.** `rusqlite_migration` (or equivalent): migration 1 is today's full schema; `user_version` is set. A test builds a fresh database and a migrated one and compares `sqlite_master`. The leftover `ui_sessions` table and the per-user session version are already gone (1.1). Rule from here on: a value that is queried or joined is a column; the `kv` table is for opaque documents only. Same runner for `node.db`, `chat.db`, `memory.db`. | A database created by commit N opens on commit N+1 for every later schema change, enforced by the test. **M** |
| 4.4 | §6.3 | **Backup and restore.** `rizzy-harness backup <dir>`: takes the storage `commit` lock, `VACUUM INTO` for each database, copies the blobs the snapshot references, `config.toml`, `node.toml`, `secrets.toml`, `keys/`. `rizzy-harness restore <dir>` into an empty data directory. Works while the server runs. | CI test: create a chat and a stored file, back up, restore into a new directory, start the server there, read both back. README has the two commands. **M** |
| 4.5 | D3 | **Store.** One writer connection and a small pool of read-only connections; `Store::read` and `Store::write`. Session queries take owner, workspace, limit and cursor, with an index on `(owner_id, updated_ms)`. `/api/sessions` is paginated; the sidebar loads more on scroll. `s.sessions()` is no longer called from a request path. Remove the N+1 loops in routines and members. | Benchmark test: 50,000 sessions across 5 users; listing one user's first page takes under 20 ms and does not block a concurrent append. **L** |
| 4.6 | §6.8–6.11 | **Operations.** `/healthz` (no auth, no detail) and `rizzy-harness health`; Docker `HEALTHCHECK`. Handle SIGTERM like SIGINT in `serve` and `node run`; drop `STOPSIGNAL SIGINT`. Request log with ids (`TraceLayer`). Audit rows for login success and failure, account create/delete/disable, role and grant changes, password resets, provider-key changes and tool approvals; `rizzy-harness audit` and an admin page to read them. | `docker stop` exits cleanly in under 2 seconds; a failed login appears in `rizzy-harness audit`. **M** |
| 4.7 | §6.6–6.7 | **TLS and exposure.** `--tls-cert` and `--tls-key`; `--secure-cookies` independent of `--tls`; a README section for running behind a reverse proxy (allowed hosts, cookies, which address is trusted). `SECURITY.md` with a contact and a supported-versions line. | A documented Caddy or nginx example works end to end. **S** |
| 4.8 | D5 | **Event-log growth: measure, then decide.** After 2.4 the unbounded part is gone. Measure bytes per turn for deltas against final events on real sessions. Write an ADR choosing between (a) leaving it, (b) pruning deltas of finished turns behind a compaction marker, (c) not replicating deltas to `master.db`. Options (b) and (c) touch the contiguous-sequence invariant and the restart acceptance test, so nothing is changed without the ADR. | ADR merged with numbers. Implementation is a separate task if the ADR says so. **S** |

**Gate 4:** a data directory survives an upgrade and a restore; a second
active user does not slow the first. Tag `v0.1.0` here, put the `docker pull`
line back.

### Phase 5 — Supply chain, frontend, clean-up (ongoing, in parallel with features)

| ID | Fixes | Change | Done when |
|---|---|---|---|
| 5.1 | §5.2 | Pin every GitHub Action to a commit SHA and every base image to a digest. Renovate with a 14-day minimum release age, grouped updates. | Renovate opens its first pull request. **S** |
| 5.2 | §5.2 | `deny.toml`: `unmaintained = "all"` with named exceptions. `pnpm audit --prod` in CI. `rust-toolchain.toml` pinning one version for CI, Docker and local; either test `rust-version` in CI or delete the claim. | CI fails on a new unmaintained transitive crate. **S** |
| 5.3 | §5.2 | Sign the image (cosign, keyless) and attest the release tarball (`actions/attest-build-provenance`); publish an SBOM. | `cosign verify` and `gh attestation verify` pass on a release. **S** |
| 5.4 | D7 | ESLint with `react-hooks` and `jsx-a11y` (or Biome) in CI. An error boundary around the shell and around each lazy route; a failed chunk load offers a reload. | Lint is a required check; throwing inside a lazy page shows a message, not a blank screen. **M** |
| 5.5 | D7 | Accessibility: `Select` gets `aria-activedescendant`, blur close, Home/End; tabs get panels and arrow keys; login fields get labels; the composer stops capturing Tab (use a visible control and a non-reserved shortcut); the remaining `confirm()` and `prompt()` calls become the existing dialog. | A keyboard-only pass through login, new chat, settings and a project; axe finds no critical issue on those pages. **M** |
| 5.6 | D7 | Generate `web/src/types.ts` from the Rust types (`ts-rs` or `specta`); CI fails when the generated file differs from the committed one. | Changing a field in `core/src/event.rs` without regenerating breaks CI. **M** |
| 5.7 | D7 | One `useResource` hook (dedupe, abort on unmount, invalidate on the WebSocket `*_changed` topics, error state) and move `Shell.tsx`'s loaders onto it, one list at a time. Not TanStack Query: it costs about 12 KB gzip and the first load has 0.4 KB of room. Revisit if the hook passes 300 lines. | `Shell.tsx` has no `refresh*` callbacks; no bare `.catch(() => {})` remains. **L** |
| 5.8 | §2.2 | e2e on `@playwright/test`, declared in `package.json`, as a CI job against the built binary and the demo provider. Start with five flows: claim and onboarding, a chat, an approval, a media upload, a routine. Port the rest as they are touched. | The job is a required check. **M** |
| 5.9 | D8 | Split `PLAN.md`: `ARCHITECTURE.md`, `SECURITY.md` (threat model, one row per threat, each naming the test that backs it), `CHANGELOG.md`. Reconcile T2, T5, T8, T9, R2, R5 with the code as it is after Phase 3. | No claim in the threat model lacks a test or an "accepted" note. **M** |
| 5.10 | §7.1 | Library swaps left over: `imagesize` for the image-header readers; `dom_query` for the favicon and source scanners; `percent-encoding`; one HTML-to-text path instead of three libraries. | Two `html5ever` versions become one; fuzz and table tests still pass. **M** |
| 5.11 | D9 | Static musl builds for x86_64 and aarch64 as release assets. A node installs the asset for its own architecture, verified against a hash the master pins, instead of receiving the master's binary. | A node on an arm64 machine joins an amd64 master. **L** |
| 5.12 | §2.2 | Temml font: get the licence clarified upstream or remove `Temml.woff2`. | `THIRD_PARTY_NOTICES.md` has no disputed entry. **S** |
| 5.13 | §2.1 | `genai`: write down the fallback (which crates, what changes behind `Provider`) and store Anthropic thinking blocks so R8 can be closed against a real key. | R7 and R8 have an owner and a test plan. **M** |

### Phase 6 — MariaDB and Redis for teams (planned, not scheduled)

The goal is real: one code base that serves a single user on SQLite and a
team on MariaDB. It is not scheduled because nothing needs it yet, and it
roughly doubles the cost of every later storage change.

**Trigger:** a concrete deployment that needs more than one server process
(high availability, rolling upgrades) or must use a managed database. Team
size alone is not the trigger: after 4.5, SQLite carries a medium team.

**Why Redis waits for the same trigger:** with one process, a session is one
indexed lookup in SQLite. Redis earns its place only when several processes
share sessions, rate limits and the live-event fan-out.

| ID | Change | Done when |
|---|---|---|
| 6.1 | **Storage behind a trait.** `harness-store` exposes its operations as a trait; the SQLite implementation is the current code. No behaviour change. | Master and node compile against the trait only. **L** |
| 6.2 | **Choose the query layer.** The closest Rust equivalent of SQLAlchemy is **SeaORM** (an async ORM over SQLx, with **SeaQuery** as its dialect-neutral query builder): one set of entities and queries for SQLite, MariaDB/MySQL and PostgreSQL, with raw SQL for the exceptions. **Diesel** is the other mature ORM, stricter at compile time and less convenient for code that must be generic over back ends. **SQLx** alone is not an ORM: its queries are per-dialect SQL strings. Recommended: SeaQuery for building queries, SeaORM's migrator for both dialects, entities only where they pay for themselves. Check each against rule 6 and build one vertical slice (users and sessions) on both databases before committing. | An ADR with the slice's code and timings. **M** |
| 6.3 | **Port.** Move the store's queries to the chosen layer. This makes the store async, so `Store::call` becomes an `async fn`. Known exceptions that need per-dialect code: full-text search (FTS5 and `bm25` against InnoDB `FULLTEXT`), `secure_delete` crypto-shredding, `INSERT OR IGNORE` / `ON CONFLICT`, and the single-writer assumption in `append` and the workspace membership checks (`SELECT … FOR UPDATE` on MariaDB). The node's own log stays SQLite. | The store and master test suites pass on SQLite and on MariaDB in CI (service container). **L, several weeks** |
| 6.4 | **Redis** for sessions, rate limits and cross-process live events (pub/sub), with `rediss://`. In-process implementations stay the default. | Two server processes behind one address: login on one is valid on the other, a chat streams through either. **L** |
| 6.5 | `rizzy-harness db copy --to mysql://…`; the `standard` and `full` Docker templates return with services the server uses. | A SQLite data directory moves to MariaDB and back in a test. **M** |

## 4. Order and dependencies

```
0 ─► 1.1 ─► 3.1, 3.2          (do not harden the JWT flow you are deleting)
0 ─► 1.2                       (removes the TOML editor instead of fixing it)
2.1 ─► 2.7                     (the cwd rule is what lets Landlock exclude the data dir)
2.7 ─► 3.4                     (the PDF helper reuses sandbox-exec)
2.4 ─► 4.8                     (measure log growth only after the cap exists)
1.1 ─► 4.3                     (the migration baseline should not contain dead tables)
4.1 ─► 4.2                     (the route test protects the split)
4.3 ─► 4.4, 4.5 ─► tag v0.1.0 ─► (trigger) ─► 6.1 ─► 6.2 ─► 6.3 ─► 6.4, 6.5
```

Phases 2 and 3 can overlap once 1.1 is merged: they touch different crates
(`tools`, `node`, `agent-native` against `master`).

## 5. What this plan deliberately does not do

- **No network allow-list proxy** for the shell yet. Landlock covers files
  and TCP; DNS and UDP stay open when network is on. Recorded as a residual
  risk after 2.7.
- **No signed approvals** from a user device (`PLAN.md` R1). A compromised
  master can still approve within node policy.
- **No MariaDB port or multi-process server before `v0.1.0`.** It is Phase 6, with a trigger.
- **No rewrite of the event log.** 4.8 measures first.
- **No new features.** Skills, MCP, external harness backends and the rest
  of Phase 9 wait for Gate 3.

## 6. How this plan can fail

Stress-tested before writing; these are the weak points.

| Risk | Why it is real | What to do |
|---|---|---|
| The sandbox breaks real work | Toolchains live under `$HOME` and write caches there; an allow-list misses some on day one. | Ship 2.7 with `best-effort` logging for a week on your own machine (log what would be denied), tune the default lists, then switch the default to `required`. |
| The env allow-list breaks tools | Same reason: `CARGO_HOME`, `NVM_DIR`, proxies. | `env_pass` exists from the first version, and the error message for a missing tool points at it. |
| Checkpoints through a separate git directory are slow on large repositories | The first `git add -A` hashes every file. | Alternates avoid storing objects twice; measure on a large repository in 2.6 and keep a per-node switch to turn checkpoints off. |
| 1.1 is called a step backwards | It removes a feature that was just merged. | It removes no capability anyone can use today; Phase 6 brings Redis back when a second process exists. |
| Cutting provisioning makes node setup too tedious, so people skip hardening | Automatic mode did the firewall and SSH steps for them. | The node refuses to start without a working sandbox and a safe data directory (1.4), so the part that protects the harness is not optional. Host hardening is the operator's job and the README says so. |
| Phase 2 stalls and the freeze is abandoned | The sandbox is the largest task and the least fun. | 2.1–2.6 are small and independently valuable; merge them first. If 2.7 slips, Q7 = "admins only" keeps the worst case closed. |
| Fixes drift from the threat model again | That is how half of these findings arose. | Rule 7, and task 5.9 ties each threat to a test. |

## 7. Minimum set, if time is short

Phase 0, then 1.1, 2.1, 2.3, 2.4, 2.5, 3.1, 3.2 and 4.1's route test. That
is roughly one week and removes: the admin escalation through the data
directory, the cross-session output leak, disk filling, the symlink write,
the login DoS and immortal sockets. It does not give you a sandbox; keep the
embedded node admin-only until 2.7 ships.

## 8. Traceability

Every finding in `AUDIT.md` and the task that closes it.

| Audit | Task | | Audit | Task |
|---|---|---|---|---|
| S1 | 2.1, 2.2, 2.7 | | L1 | 3.7 |
| S2 | 2.3 | | L2 | 1.4 (cut) |
| S3 | 2.4 | | L3 | 3.6 |
| S4 | 3.1 | | L4 | 3.6, 4.2 |
| S5 | 3.2 | | L5 | 3.6 |
| S6 | 3.3, 3.2 | | L6 | 3.6 |
| S7 | 3.4 | | L7 | 3.6 |
| S8 | 2.5 | | L8 | 3.6 |
| S9 | 2.6 | | L9 | 1.1 |
| S10 | 2.8 | | L10 | 3.9 |
| S11 | 2.1, 2.2, 2.9 | | L11 | 3.9 |
| S12 | 3.5 | | L12 | 3.9 |
| S13 | 1.2 | | L13 | 3.9 |
| D1 | 1.1 | | L14 | 3.10 |
| D2 | 4.3 | | L15 | 3.7 |
| D3 | 4.5 | | §5.1 CI red, no protection | 0.1, 0.2, 0.4 |
| D4 | 1.4 (cut), freeze (rule 1) | | §5.2 Actions, images, signing | 5.1, 5.3 |
| D5 | 2.4, 4.8 | | §5.2 bots, deny, audit, toolchain | 5.1, 5.2 |
| D6 | 4.1, 4.2 | | §5.2 double CI runs, `.idea` | 0.3, 1.5 |
| D7 | 1.5, 5.4–5.8 | | §5.2 scraped wall data | 1.3 (kept, reviewed) |
| D8 | 5.9 | | §5.2 bus factor, outside review | Gate 3 |
| D9 | 5.11 | | §6 backup | 4.4 |
| §2.1 `pdf-extract` | 3.4 | | §6 limits, TLS, health, SIGTERM, audit, logs | 3.3, 4.6, 4.7 |
| §2.1 three HTML libraries | 5.10 | | §6 `SECURITY.md` | 4.7 |
| §2.1 `genai` | 5.13 | | §6 e2e, lint, types, pagination, arm64 | 5.8, 5.4, 5.6, 4.5, 5.11 |
| §2.2 temml font | 5.12 | | §7.1 replace list | 1.1, 1.2, 1.4, 3.1, 3.3, 4.3, 5.6–5.10 |
