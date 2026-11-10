# AUDIT — rizzy-harness

Audited: commit `62c17fd` (main, 2026-11-09). Date of audit: 2026-11-09.

## 0. How to read this

**What was done.** Every Rust crate and the web frontend were read. The auth
core (auth, JWT, KV, secrets, blob crypto, bind rules, the store) I read line
by line myself. The HTTP handlers, the node/tools/link/provisioning crates
and the frontend were read by three parallel review passes; every finding
from those that appears below was then re-checked against the code at the
cited lines before it was included, and several were downgraded or dropped.
These were run:

| Check | Result |
|---|---|
| `cargo audit` (571 locked crates) | 0 vulnerabilities, 1 unmaintained (`ttf-parser`, via `pdf-extract` → `lopdf`) |
| `pnpm audit --prod` | 1 low (`katex` via `remark-math`; its HTML half is not bundled) |
| `pnpm typecheck`, `pnpm test`, `pnpm build` | pass (82 files, 776 tests) |
| `pnpm size` | **fails** (total JS 365.1 KB against a 364 KB budget) |
| GitHub Actions on `main` | **red** at HEAD, and red on the commit before it |

**What was not done.** `cargo test` was not run locally (CI's `rust` job is
green at HEAD: fmt, clippy `-D warnings`, tests). Nothing was run against a
real provider, a real remote node, or a real VPS. No dynamic testing or
fuzzing. Findings are from reading code; each one names the file and line so
it can be checked.

**Severity.** *High* = fix before anyone but you uses it. *Medium* = fix
before exposing it beyond loopback or to a second user. *Low* = hygiene.
Every finding assumes the documented default (bind `127.0.0.1`) unless it
says otherwise; several get worse the moment the UI is bound to a WireGuard
address, published from Docker, or shared with other accounts.

---

## 1. Verdict

The code is better than its process and its scope.

- **Code quality is high in the small.** Constant-time compares, a sound
  SSRF guard, magic-byte sniffing, sandboxed file serving, owner checks on
  every handler, about 630 Rust tests and 776 web tests. Most of the classic
  web bugs were looked for and are not there.
- **The project is far too wide for its age.** About 130k lines of Rust,
  TypeScript and CSS (tests included), 76 commits, seven days, one human. It is a chat UI, an agent runtime, a multi-tenant
  workspace system, a job scheduler, a file store with its own encryption
  format, a kanban board, a vector-memory service **and a tool that logs
  into servers as root and rewrites their firewall and sshd**. Nobody has
  reviewed most of this; the threat model in `PLAN.md` is the author's own
  claims about the author's own code.
- **The newest work is the weakest.** The last merge swapped working
  server-side sessions for a hand-written JWT system, added Redis, and ships
  a MariaDB container that nothing connects to. It was merged with CI red.
- **The real risk is not a clever bug.** It is that this is an RCE service
  by design, with no OS sandbox on the agent's shell, grown faster than it
  can be verified.

If you fix only five things: get CI green and protect `main` (§5.1); get
the server's own keys out of the agent's reach and sandbox its shell (§3,
S1); add schema migrations (§4, D2); delete the JWT/Redis/MariaDB layer
(§4, D1); and stop adding features until the list in §6 is done.

---

## 2. Technologies: keep, replace, watch

### 2.1 Backend

| Technology | Used for | Verdict |
|---|---|---|
| Rust 2024, tokio, axum 0.8 | everything | **Keep.** Right choice for a single static binary. `unsafe_code = "forbid"` workspace-wide is good. |
| rusqlite (bundled SQLite, WAL, FTS5) | all state | **Keep SQLite, replace how it is used.** One `Arc<Mutex<Connection>>` for the whole server and no migrations (D2, D3). Add `rusqlite_migration` (or `refinery`) and a reader pool (`r2d2_sqlite` / `deadpool-sqlite`), or move to `sqlx` if the MariaDB port is real. |
| Hand-written HS256 JWT (`master/src/jwt.rs`) + refresh rotation + KV | browser login | **Remove.** Every request still does a KV lookup and a user lookup (`auth.rs:724-742`), so the JWT buys nothing over an opaque session id. See D1. If JWTs must stay, use `jsonwebtoken`. |
| `redis` 1.8 | refresh tokens, "several servers" | **Remove until it has a use.** The server cannot run as several processes (each has its own SQLite; `docker/README.md` says so). `rediss://` is refused, so it is plaintext-only as well. |
| MariaDB container (`docker/standard`, `docker/full`) | nothing | **Delete.** Dead infrastructure behind a compose profile, for a port that is a six-step plan. |
| `genai` =0.7.2 | LLM providers | **Watch.** Single maintainer, pinned exactly, already known to be missing replay of Anthropic thinking blocks (PLAN R8). The `Provider` trait seam is the right mitigation. Decide a fallback now (`async-openai` plus a thin Anthropic client, or the vendors' own SDKs). |
| `pdf-extract` 0.12 (→ `lopdf` → unmaintained `ttf-parser`) | PDF attachments | **Replace or isolate.** A panicky parser on untrusted input inside the process that holds every key (S7). Run it in a child process with a timeout and memory limit, or shell out to `pdftotext` the same way. |
| `dom_smoothie` + `htmd` + `html2text` | `web_fetch` | **Consolidate.** Three HTML libraries and two `html5ever` versions (0.38 and 0.40) for one tool. Keep the readability pass and one converter. |
| `russh` 0.65 (pulls `ssh-key` 0.7.0-rc.11, `md5`, `des`) | node setup over SSH | **Question the feature, not the crate** (D4). If it stays, `russh` is the reasonable pure-Rust choice; a pre-release `ssh-key` in the tree is a watch item. |
| `aws-lc-rs`, `rustls`, `rcgen` | TLS, AEAD, HMAC | **Keep.** One crypto library, no OpenSSL, no `rsa` crate. |
| `argon2` 0.6 (default params: Argon2id, 19 MiB, t=2) | passwords | **Keep.** OWASP minimum. Add a concurrency cap (S4). |
| `croner` + `jiff` | routine schedules | **Keep.** Correctly not hand-written. |
| `serde-saphyr` =2.0.0 | agent frontmatter | **Watch.** A month old when pinned. Fine for now; the input is admin-written. |
| `tree-sitter` + 5 grammars | repo map | **Keep.** |
| `rust-embed`, `clap`, `tracing`, `dotenvy`, `infer`, `tar`, `flate2` | plumbing | **Keep.** |
| `tower-http`, `tower` | not used | **Add.** Timeouts, body limits, tracing, compression are all hand-done or missing (S6). |
| graphify (`graphifyy==0.9.102` from PyPI, installed at runtime) | optional repo graph | **Question.** Installed by `uv tool install` / `pipx` when an admin clicks a button (S13). |

440 crates end up in the binary. That is normal for this stack; the three
HTML libraries and `russh` are the biggest avoidable chunks.

### 2.2 Frontend

| Technology | Verdict |
|---|---|
| React 19, Vite 8, TypeScript 5.9 strict (`noUncheckedIndexedAccess` on) | **Keep.** |
| `react-markdown` + `remark-gfm` + `remark-math`, no `rehype-raw` | **Keep.** Used once, correctly, with a URL scheme allow-list. |
| `refractor` (Prism) in a worker | **Keep**, but the worker build stubs out `parse-entities` and swaps `hastscript` for a local file through Vite plugins (`vite.config.ts:10-24`) plus a pnpm override. That is patching a dependency's internals to save kilobytes; it will break on an upgrade. |
| `temml` | **Keep**, resolve the font licence (PLAN R10): `Temml.woff2` carries a "not for commercial use" note. Either drop the font or get it clarified before anyone ships this commercially. |
| Hand-written CSS, one 4,485-line global `styles.css` | **Keep the tokens, split the file.** It has 237 custom properties and a contrast test, which is good. But every lazy page's CSS is in the first load and the size budget only counts `.js`. |
| No ESLint / Biome / Prettier | **Add.** `react-hooks/exhaustive-deps` and `jsx-a11y` would have caught real issues listed in D7. |
| vitest + happy-dom, hand-rolled `createRoot`/`act` in 42 files | **Keep vitest; add Testing Library** when a test is next touched. |
| e2e: 24 ad-hoc `.cjs` scripts, `require('playwright')` not in `package.json`, not in CI | **Replace with `@playwright/test`** and a CI job. 3,400 lines of tests that never run automatically are decoration. |
| Server state by hand in `Shell.tsx` | **Replace with TanStack Query** or one shared resource hook (D7). |
| `types.ts` (858 lines, "Mirrors crates/core … Keep in step") | **Generate** with `ts-rs` or `specta`. `api.ts:79` is `return data as T`. |
| pnpm 12, exact pins, frozen lockfile | **Keep.** Add Renovate or Dependabot: exact pins without a bot means nothing ever updates. |

---

## 3. Security findings

No finding here is a remote, unauthenticated takeover. The serious ones are
what a logged-in user with the agentic grant, or a prompt-injected agent,
can do; and what an unauthenticated client can do to availability once the
port is reachable.

### High

#### S1. The agent's shell has no sandbox, and the default workspace contains the server's keys

- Bash is `bash -c` as the server's OS user (`tools/src/bash.rs:115-123`).
  No Landlock, bubblewrap, namespaces or cgroups. `PLAN.md` lists this as
  open risk R2.
- The embedded node's default `workspace_roots` is the user's home directory
  (`node/src/policy.rs:64`). The data directory defaults to
  `~/.local/share/rizzy-harness` (`cli/src/main.rs:129-135`), which is inside
  it. `check_cwd` (`policy.rs:93-113`) and `ToolCtx::resolve`
  (`tools/src/lib.rs:90-132`) exclude nothing; only the publish tool has a
  data-dir rule.
- `read` and `grep` are classed read-only (`tools/src/fs.rs:35`,
  `search.rs:54`), so in the default `ask` mode they need no approval.

Result: any account with the agentic grant on the embedded node can have its
agent read `api-token` and `keys/jwt.key` without a single approval click,
and everything else there (`secrets.toml`, and `master.db` with every user's
chats, password hashes and blob keys) by approving its own shell command.
`api-token` is documented as "the machine owner's root key: admin rights and
access to every session" (`auth.rs:21-23`) and it is not in the redaction
set (`cli/src/main.rs:210-213, 297`); `jwt.key` signs logins for any user.
That turns "may run agentic sessions" into "is server admin and can read
every user's chats", which breaks T10 and T11 of the project's own threat
model. On a single-user install the same files are what a prompt-injected
agent goes for. The only existing mitigation is `--no-local-node`.

Fix, in this order:
1. Refuse any session cwd and any tool path inside the data directory, on
   every node.
2. Default `workspace_roots` to empty (the user picks a folder), not `$HOME`.
3. Give the shell an allow-listed environment (`env_clear()` plus `PATH`,
   `HOME`, `LANG`, `TERM`) instead of the current deny-list
   (`bash.rs:124-126`), which passes through whatever else the server was
   started with (`AWS_*`, `GITHUB_TOKEN`, `SSH_AUTH_SOCK`).
4. Add the sandbox: Landlock for the filesystem, no network by default.

#### S2. `tool_output` ids collide and are guessable, across sessions and users

`OutputStore::put` uses the first 8 hex characters of a UUIDv7 as the id
(`tools/src/outputs.rs:25`). Those are the top 32 bits of the millisecond
timestamp, so the id changes once every 65.5 seconds. `get` returns the
first entry with that id (`outputs.rs:34-41`), and there is one store per
node backend, shared by every session (`agent-native/src/lib.rs:137, 1100`).

- Two filtered commands inside the same 65 seconds get the same id; the
  second one's full output can never be retrieved, and the model is handed
  the first one's output instead. This is a correctness bug in normal use:
  the feature is on by default.
- Any session on the node can ask for another session's recent shell output
  with an id derived from the clock, through a tool that needs no approval.

Fix: a random 128-bit id, and key the store by session.

#### S3. Shell output is streamed without any total limit

Only the copy kept for the model is capped (`bash.rs:150-156`). Every chunk
is still sent on (`bash.rs:157-159`) through three unbounded channels
(`bash.rs:133`, `agent-native/src/lib.rs:1089-1090`) and written as a
`ToolOutputDelta` event with an fsync, then replicated to the master
(`agent-native/src/lib.rs:1120-1128`, `node/src/session.rs:193`).

`yes`, or `cat /dev/urandom | base64`, for the 600-second default timeout
(the model may ask for 3600) grows `node.db`, `master.db` and memory until
something runs out. T9 says "Output is capped"; that is true only of the
final tool result. Cap the streamed total per call and drop the rest with a
marker.

### Medium

#### S4. Login can be used to exhaust memory and CPU before authentication

`http.rs:407-438`, `auth.rs:133-146`.

- Every failed login inserts the submitted username as a key into an
  in-memory map. The username is not validated or length-limited at login,
  so each request can add a key of up to about 2 MiB (axum's default JSON
  limit). Entries are only pruned when that same username is tried again.
- Every request runs Argon2id (19 MiB, 2 passes) in the blocking pool,
  including for unknown usernames. The throttle is per username, so rotating
  usernames is unthrottled. There is no per-IP or global cap.
- The throttle check and the failure record are separated by the awaited
  Argon2 call, so N parallel guesses at one account all pass the check.
  `try_reserve_lookup` (`auth.rs:558-567`) already does this correctly.

Fix: validate the username before anything else; bound the map; a semaphore
around password hashing; per-IP rate limiting on `/api/login` and
`/api/setup` (`tower_governor`).

#### S5. An open WebSocket survives logout, disable and password reset

`ws.rs:42-54` authenticates at upgrade. The loop (`ws.rs:99-196`) never
checks again, and no `Live` message closes a socket (`lib.rs:61-78`). After
logout, account disable, a password reset or expiry of the 15-minute access
token, an already-open socket keeps receiving every event of the chats it
subscribed to and can subscribe to more. T2 ("logout revokes it at once")
and T12 ("signs the user out everywhere") do not hold for WebSockets.

Fix: re-check the principal on a timer and on every subscribe, and broadcast
a per-user "revoked" message that closes that user's sockets.

#### S6. The HTTP server has no timeouts and no connection limits

`axum::serve(listener, router)` with one middleware layer
(`cli/src/main.rs:410-416`, `http.rs:199-204`). No header-read timeout, no
request timeout, no connection cap, no concurrency limit. Only the TLS
handshake has a timer (`cli/src/net.rs:140`). A handful of slow clients can
hold connections open indefinitely. Harmless on loopback; not once bound to
a tunnel address or published from Docker. The UI WebSocket also keeps the
library's 64 MiB message limit and sockets per user are uncapped
(`ws.rs:54`). Use `tower-http`'s `TimeoutLayer` and hyper's
`header_read_timeout`, cap connections, and set a small WebSocket message
size (the client only sends subscribe messages).

#### S7. Untrusted PDFs are parsed inside the server process

`attachments.rs:20-25`, `http.rs:1252-1275`. Any logged-in account, a
chat-only guest included, can upload a file that is treated as a PDF by
name alone and parsed by `pdf-extract` in a blocking task with no timeout,
no memory limit and no concurrency cap. The comment says the parser "can
panic"; a panic is caught, but a hang holds a blocking thread forever (it
cannot be cancelled), a decompression bomb takes the process's memory, and a
stack overflow aborts the whole server. This is the process that holds every
provider key and controls every node. The dependency chain ends in a crate
RustSec lists as unmaintained (`ttf-parser`).

Also here: `/api/transcribe` accepts 25 MiB bodies with no per-user limit
and forwards them to a paid API (`http.rs:1886-1912`).

#### S8. `write` escapes the workspace through a dangling symlink

`ToolCtx::resolve` walks up until `exists()` is true (`tools/src/lib.rs:106`).
`exists()` is false for a symlink whose target does not exist, so
`workspace/link` is accepted as an ordinary new file inside the workspace,
and `tokio::fs::write` then follows the link (`tools/src/fs.rs:138`). A
repository can ship `notes.md -> ~/.config/systemd/user/x.service` (or
`~/.ssh/authorized_keys` on a machine that has none) and the approval card
shows a harmless in-workspace path. The existing test only covers a link to
an existing directory (`lib.rs:251-257`). Open with `O_NOFOLLOW` and resolve
with `symlink_metadata`.

#### S9. git runs in the user's repository with the repository's own config, before any approval

`node/src/git.rs:41-74` runs `git -C <repo>` with no `-c` overrides. The
checkpoint taken at the start of every turn (`session.rs:401`) runs
`git add -A` (`git.rs:187`), which honours a repo-local `core.fsmonitor`
and clean filters; `worktree add` runs `post-checkout` hooks; `git diff`
lacks `--no-ext-diff --no-textconv`. A working copy that arrives with a
hostile `.git/config` (an archive, a shared folder) executes code on the
first prompt. And because `.git/` is writable through the `write` tool, a
node that sets `denied_tools = ["bash"]` can still be made to run commands.
Pass `-c core.fsmonitor=false -c core.hooksPath=/dev/null`, set
`GIT_CONFIG_NOSYSTEM`/`GIT_CONFIG_GLOBAL`, and refuse tool writes under
`.git/`.

#### S10. One stuck tool or one slow command blocks cancel for everyone on a node

- Tools get a cancellation token that never fires
  (`agent-native/src/lib.rs:1118`); only bash watches its own. `read` loads
  the whole file with no size check (`tools/src/fs.rs:62`), so a FIFO hangs
  the turn forever and a multi-gigabyte file takes the memory. `grep` reads
  whole files too.
- The node handles master commands one at a time
  (`node/src/lib.rs:202-217`). A slow restore (git, no timeout) or delete
  makes Cancel and approval answers wait, for every session on that node.

#### S11. Secret redaction is easy to defeat, and misses things

`core/src/secret.rs:65-74` replaces exact substrings of known secrets,
8 characters or longer. Base64, `rev`, or printing a key in two halves gets
past it. Streamed output is scrubbed per chunk
(`agent-native/src/lib.rs:1128`), so a secret that straddles a flush
boundary is stored and shown in full. The API token and JWT key are not in
the set (S1). A remote node's `node run` does not apply the `HARNESS_*`
environment filter that `serve` applies (`cli/src/main.rs:525-542` against
`215-218`). Treat redaction as a courtesy against accidents and say so in
the threat model; the real control is not putting secrets where the shell
can reach them (S1).

#### S12. Any project member can write into the shared project memory

`http/projects.rs:979-1026`. Adding a fact needs "may manage", but a
member's own *proposed* fact can be edited to any text and set to `active`
by that member. Project memory is injected into every member's chats in
that project, including agentic ones with a shell. That is a cross-user
prompt-injection path by design. Require a manager to activate.

#### S13. An admin click installs and runs a PyPI package; an HTTP call edits `node.toml`

`POST /api/admin/graphify/install` runs `uv tool install --force
graphifyy==0.9.102` or `pipx install` as the server user
(`tools/src/graphify.rs:92-121`): version-pinned, not hash-pinned, fetched
at run time. `PUT /api/admin/graphify` rewrites `node.toml` with a
hand-written line editor and a non-atomic write (`http.rs:2354-2443`).
T5 says the node policy file "is not settable over the protocol"; for the
embedded node it is settable over HTTP. Either drop the claim or drop the
endpoint.

### Low

| # | Finding | Where |
|---|---|---|
| L1 | Node link uses unbounded channels in both directions; a paired node can queue frames faster than the master stores them. No connect or handshake timeout on the node side. | `link/src/server.rs:23-24`, `client.rs:71-76, 105` |
| L2 | Provisioning scripts quote by deleting `'`; `{join_cmd_args}` is unquoted in a script run as root; root installs the binary from the fixed path `/tmp/rizzy-harness.upload`, which another local user on the target could swap. | `provision/src/scripts.rs:31, 62, 101, 322, 331` |
| L3 | Admin-only SSRF: the provider "test" call accepts any `http(s)` base URL, and `url_allowed` accepts https to private ranges. | `http/providers.rs:702-708`, `config.rs:509-515` |
| L4 | Store errors (`sqlite: …`) and file paths are returned to clients in 500 bodies and not logged. | `http.rs:230-240`, `storage.rs:630` |
| L5 | A shared artifact's metadata includes the owner's path on their node. | `artifacts.rs:390`, `http/artifacts.rs:179-210` |
| L6 | `Origin` is checked only when the header is present, and its scheme is ignored. No HSTS with `--tls`, no `Cache-Control: no-store` on API responses. | `auth.rs:640-645, 760-788` |
| L7 | `api-token` and `jwt.key` are created 0600 but their mode is not checked when read back; `secrets.toml` does check. | `auth.rs:209-230` against `secrets.rs` |
| L8 | Blob "encryption at rest" keeps each key in `master.db` beside the files. It gives crypto-shredding on delete, which is its stated purpose; it gives no confidentiality against anyone who can read the data directory. It is also a hand-designed file format (see §7). Do not describe it as encryption at rest. | `blobs.rs`, `docs/media.md` |
| L9 | Redis is plaintext only (`rediss://` is refused). | `kv.rs:41-45` |
| L10 | `AllowSession` is keyed by tool name: one click allows every later `bash` command in that turn, whatever it is. | `agent-native/src/lib.rs:1070-1084` |
| L11 | `web_fetch`'s "URL must have been seen" rule counts tool error text, and some errors echo model-chosen strings, so a model can launder a URL it built. Chat runtime only. Not demonstrated end to end. | `agent-native/src/lib.rs:386-394, 1062-1068` |
| L12 | `is_public` misses 6to4 (`2002::/16`), Teredo and IPv4-compatible IPv6. | `web/src/lib.rs:56-68` |
| L13 | mem0 client puts ids into URL paths unencoded and follows cross-host redirects with its API key. | `memory/src/mem0.rs:28-31, 134-218` |
| L14 | Browser: unsent drafts are kept in `localStorage` per chat, incognito chats included, and never cleared on logout or delete. Markdown `#…` links are passed through, so a model can link to app routes. An admin types a user's new password into `prompt()`. | `Composer.tsx:252-260`, `md/pipeline.ts:226`, `Users.tsx:98` |
| L15 | T5 says the node decides "whether remote-created sessions are allowed at all". No such setting exists. The docs say the node link is "mutual challenge-response"; only the node signs, the master is authenticated by the pinned certificate. | `node/src/policy.rs:11-27`, `link/src/wire.rs:121-142` |

Checked and found sound: authorization on every handler, webhook
authentication, the routine template engine (single pass, no re-scan), FTS
query building (tokens quoted and bound), `Content-Disposition` building,
the image-header readers (bounds-checked), SQL in the store and memory
crates (parameterised), SSH host-key pinning, the node link's pinned-cert
verifier and nonce handling, artifact staging (`O_NOFOLLOW`, fd re-check).

---

## 4. Decisions that are not the best

### D1. JWT + refresh rotation + Redis: complexity with no payoff

The last merge replaced a server-side session table (the `ui_sessions` table
and its functions are still in `store/src/lib.rs`, now dead, still tested)
with HS256 access and refresh tokens, a token-family revocation scheme, a
30-second replay grace window, and a pluggable KV store with a Redis backend.

- The stated benefit is "checked by signature, no session table"
  (`auth.rs:5-6`, `docs/storage-backends.md`). That is false in the code:
  `authenticate` verifies the signature, then reads the KV store for a
  revoked family, then reads the user row and session version from the
  database (`auth.rs:722-742`). Two lookups per request, same as before.
- The stated reason for Redis is several server processes behind one
  address. The same docs say that is "not a supported setup yet", because
  every process has its own SQLite.
- What it cost: a hand-written JWT implementation, a refresh endpoint, a
  `fetch` monkey-patch in the browser (`web/src/session.ts:47`), a race
  window by design, and a state machine where losing the KV store logs
  everyone out and writes `auth.refresh_reused` audit rows for honest users.

An opaque 256-bit session id in an `HttpOnly; SameSite=Strict` cookie, hashed
in a table, is simpler, revocable in one `DELETE`, and is what the code had.
Go back to it (`tower-sessions` if you want a library). Bring JWTs back on
the day there is a second server and a shared database, not before.

### D2. No schema migrations, and the trigger for adding them has already passed

Every table is `CREATE TABLE IF NOT EXISTS` run at start
(`store/src/lib.rs:470-478`). There is no `user_version`, no `ALTER TABLE`,
no migration list.

This is a recorded decision, not an oversight: `PLAN.md` §0 says "No
compatibility code before the first release … Migrations start with the
first tagged release", and no tag exists yet. As a rule for a private
prototype that is reasonable. Two things make it wrong today:

- The README's first line tells people to `docker pull …:edge`, and the
  image has been published from `main`. Anyone who followed it has a data
  directory with no upgrade path. The trigger should be "first published
  image", and that has happened.
- The last merge broke the rule in the other direction: it kept the old
  session table and its functions, and added code whose only job is to
  clear the pre-JWT cookie (`auth.rs:49-52, 668-676`). That is compatibility
  code for a release that never happened.

Separately, state has been pushed into the generic `kv` table as strings: the per-user session version
lives at `auth.session_version.<uid>` (`store/src/lib.rs:445-447`,
`1519-1527`), and preferences, node setups, the model cache, agent edits and
workspace settings are JSON blobs there too (9 modules call `kv_get`/`kv_set`).
That is a schemaless database inside the SQL database, with no constraints
and no foreign keys.

Add the migration runner before the first tag, and stop advertising the
image until it is there.

### D3. One mutex around one SQLite connection, and full-table scans behind it

`Store` is `Arc<Mutex<Connection>>` (`store/src/lib.rs:219-221`); every call
takes the lock inside `spawn_blocking`. WAL mode allows concurrent readers and
the design throws that away: a slow search blocks every event append, every
login and every auth check on the server.

On top of that, hot paths load every session of every user and filter in
Rust: `list_sessions` (`http.rs:1064`), three places in `http/projects.rs`
(139, 498, 884) and `workspaces.rs:464` all call `s.sessions()`, which is an
unbounded `SELECT` with no `WHERE` and no `LIMIT`. The chat list has no
pagination.

It works for one user and a few hundred chats. It will be the first thing
that falls over. Use a pool with one writer and several readers, and put
owner and workspace in the `WHERE` clause.

### D4. Scope

For a v0.1 with no users, the repository contains: workspaces with four
roles; routines with cron, webhooks and templating; an encrypted blob store;
media and artifact libraries; projects with kanban boards and board
templates; three memory backends; a tree-sitter repo map; an in-browser text
classifier; a provisioning system that creates users, installs WireGuard,
writes nftables rules, edits sshd and runs a watchdog on remote servers; and
3,186 lines of animated home-screen backgrounds, which is more code than the
whole markdown pipeline.

Each of these is a maintenance and attack surface you now own. The
provisioning feature in particular is a separate product (Ansible, or a
20-line cloud-init file, does it with a track record) and it is the part
where a bug locks someone out of their server or opens it. The README itself
says "Test it on a throwaway VPS first."

Cut or freeze: SSH provisioning, MariaDB/Redis, kanban templates, the wall
backgrounds and the Google-autocomplete data behind them, the classifier.
Finish: sandboxing, migrations, backup, limits.

### D5. The event log keeps every streaming delta forever, twice

`AssistantDelta` and `ToolOutputDelta` are ordinary events
(`core/src/event.rs:63-66`), appended with an fsync on the node
(`node/src/session.rs:193`) and replicated to `master.db`. On one machine
every token chunk is therefore stored in two databases, permanently. There is
no compaction and no retention. Once a message is complete its deltas carry
no information the final message does not; fold them away, or never
replicate them.

### D6. Handler structure

- **Auth is opt-in.** All of the roughly 150 handlers start with a hand-written
  `require(&m, &headers)` or a helper that calls it. Today none is missing
  (checked mechanically). The design still fails open: forget the line and
  the route is public. Make `Principal` and `Admin` axum extractors, or split
  the router into public, user and admin groups with middleware.
- **`http.rs` is 2,464 lines**: router, error type, sessions, users, memory
  config, SSH provisioning, graphify, the classifier and a TOML editor.
- **`ApiError(StatusCode, String)`** carries `e.to_string()` of store errors
  to the client (`http.rs:230-240`) and logs nothing server-side.
- **Five separate hand-written throttles** (login failures, username lookups,
  webhook buckets, artifact throttle, upload and favicon slots), each with
  its own pruning rule or none.
- **Blocking file IO on async threads** in the settings paths
  (`providers.rs:234-255`, `http/providers.rs:276-277`, `http.rs:2406-2442`).

### D7. Frontend structure

- `Shell.tsx` (748 lines, 24 `useState`, 15 `useEffect`) owns routing, all
  data loading, notifications, the classifier and layout. `Project.tsx` has
  35 `useState`, `Composer.tsx` 21. There are 31 `.catch(() => {})`.
- **No error boundary anywhere**, with 21 `lazy()` chunks. A chunk that fails
  to load after the binary is upgraded, or one exception in model-driven UI,
  blanks the app.
- **Accessibility of hand-rolled widgets.** `Select.tsx` has a `listbox`
  with no `aria-activedescendant` and no blur handling; `role="tab"` without
  tab panels in three places; login fields have placeholders and no labels;
  the home composer captures Tab and Shift+Tab to switch modes
  (`Composer.tsx:417-421`), which traps keyboard users. `Combobox.tsx` and
  the menus are done properly, so this is inconsistency, not ignorance.
- 20 native `confirm()`/`prompt()` calls, including one that asks an admin
  to type a user's new password into `prompt()` in clear text
  (`Users.tsx:98`).
- "i18n" is three string catalogs plus about 90 hard-coded English
  attributes. It is not i18n; either do it or delete the scaffolding.
- FNV-1a is implemented four times (`md/fnv.ts`, `lib/classifier.ts`,
  `bg/rng.ts`, `AgentAvatar.tsx`).

### D8. Documentation as changelog

`PLAN.md` is 1,831 lines, most of it dated status entries. It has drifted
from the code: R5 says the web UI has no TLS (there is `--tls`); T2 says
every state-changing request checks `Origin` (only when the header is
present, `auth.rs:760`); the storage doc says access tokens need no lookup
(D1). A threat model that is wrong in places is worse than a short one that
is right. Split it: `ARCHITECTURE.md`, `SECURITY.md` (threat model, kept
true), `CHANGELOG.md`.

### D9. Platform lock-in by construction

The master uploads its own binary to nodes, so master and nodes must share
architecture and a compatible glibc. Result: linux/amd64 only, Debian 13 and
Ubuntu 26.04 only, no arm64 (so no Raspberry Pi, no Graviton, no Apple
Silicon nodes). Publish per-target release binaries (musl builds remove the
glibc coupling entirely) and let the node download its own.

---

## 5. Process and supply chain

### 5.1 CI is red on `main`, and red code is being merged

- The `web` job fails at `pnpm size` on HEAD (`62c17fd`) and on the commit
  before it (`13ca3e9`). PR #2 was merged 22 seconds after its checks
  started, and the same commit's run on its branch had already failed
  two and a half hours earlier.
- Because `publish` needs every job, the `:edge` image has not been rebuilt
  since the last green run on 2026-11-08. The README's first line tells
  people to pull it.
- The limit that fails is described in `scripts/size.mjs` as "soft" and then
  calls `process.exit(1)` (line 173-179). A soft limit that fails the build
  is a hard limit nobody respects.

Fix the budget, then turn on branch protection: required checks, no direct
pushes to `main`.

### 5.2 Other gaps

| Gap | Why it matters |
|---|---|
| Actions pinned by tag (`actions/checkout@v4`, `docker/build-push-action@v6`, …) in a job with `packages: write` and `contents: write` | A moved tag runs in your release job. Pin to commit SHAs. |
| Base images by tag, no digest | Same, for the image. |
| No SBOM, no provenance, no signature on the image or the release tarball (only a `.sha256` beside it) | The node setup uploads this binary to servers and runs it as a service. `cosign` + `actions/attest-build-provenance`. |
| No Dependabot / Renovate; every dependency pinned exactly | Exact pins plus no bot means never updated. `genai`, `serde-saphyr` are pinned with `=`. |
| `deny.toml`: `unmaintained = "workspace"` | Only direct dependencies are checked. The one unmaintained crate in the tree (`ttf-parser`) is transitive and passes silently. Set it to `"all"` and list exceptions. |
| No `pnpm audit` in CI | Rust has `cargo deny`; the web side has nothing. |
| `dtolnay/rust-toolchain@stable` in CI, `rust:1.99` in the Dockerfile, `rust-version = "1.89"` declared, no `rust-toolchain.toml` | Three toolchains; the declared MSRV is never tested. |
| CI runs on both `push` and `pull_request` | Every PR commit runs twice. |
| `.idea/` is committed | Editor files in the repository. |
| `web/public/wall/*.txt` is scraped from Google's autocomplete endpoint (`web/scripts/harvest_queries.py`) and shipped in the binary | An undocumented endpoint, scraped against its terms, for decoration. Delete it or write the lists yourself. |
| Bus factor 1; 32 of 76 commits are authored as "Claude" | No second reviewer exists for an RCE service. At minimum, get one outside review of the node link and the approval path before anyone else runs it. |

---

## 6. What is missing and required

Required before a second person uses it:

1. **A sandbox for the agent shell** (S1). Landlock or bubblewrap on the
   node, with network off by default. Everything else in the threat model
   leans on a human clicking "allow".
2. **Schema migrations** (D2), before the first tag or the next published
   image, whichever comes first.
3. **Backup and restore.** There is no `backup` command and no documented
   procedure. Four SQLite files plus a blob directory whose keys live in one
   of those files: copy one without the other and the files are unreadable.
   Add `rizzy-harness backup` using `VACUUM INTO` plus the blob directory.
4. **Server limits** (S6): header and request timeouts, a connection cap,
   per-IP rate limiting on `/api/login` and `/api/setup`.
5. **Green CI and branch protection** (§5.1).
6. **A `SECURITY.md`** with a contact and a supported-versions line.

Required before exposing it beyond loopback:

7. **A real TLS story.** `--tls` is a self-signed certificate the user must
   trust by hand on every device. Either support `--tls-cert/--tls-key` and
   document a reverse proxy (with the `Host` allow-list and `Secure` cookie
   implications), or add ACME.
8. **A health endpoint** (`/healthz`) and a Docker `HEALTHCHECK`. None exists.
9. **SIGTERM handling.** Only Ctrl-C is handled (`cli/src/main.rs:386, 549`).
   The image works around it with `STOPSIGNAL SIGINT`; the systemd unit the
   provisioner writes (`provision/src/scripts.rs:339-341`) gets systemd's
   default SIGTERM, so nodes are never shut down gracefully.
10. **An audit log that is complete and readable.** Eight call sites write
    to the `audit` table (node joins, refresh reuse, provisioning, routines,
    artifacts). Logins, failed logins, account creation and deletion, role
    and grant changes, password resets, provider-key changes and tool
    approvals are not recorded, and no API or CLI reads the table.
11. **Metrics or at least structured request logs.** There is no request
    tracing layer; 500s are returned to the client and not logged.

Should have:

12. e2e tests in CI (`@playwright/test`).
13. A linter for the web code.
14. Generated API types (`ts-rs`/`specta`).
15. Pagination on the session list.
16. arm64 builds (D9).
17. Session revocation on open WebSockets (S5).

---

## 7. Hand-written code that an existing solution covers

"Replace" means the existing solution is better and the swap is cheap.
"Defensible" means the hand-written version is small, tested, and a library
would not clearly win.

### 7.1 Replace

| Hand-written | Where | Size | Use instead |
|---|---|---|---|
| JWT signing and verification | `master/src/jwt.rs` | 193 lines | Nothing (server-side sessions, D1), or `jsonwebtoken` |
| Refresh-token rotation, token families, KV abstraction with three backends | `master/src/auth.rs`, `kv.rs` | about 600 lines | `tower-sessions` with its SQLite store |
| Login and username-lookup throttles | `auth.rs:133-146, 537-580` | about 50 lines, unbounded | `governor` / `tower_governor` (keyed, bounded). Fixes S4 |
| Request timeouts, body limits, request logging, security headers | missing or in `auth.rs:776-788` | — | `tower-http` (already in the lock file through reqwest) |
| TOML editing by line matching | `http.rs:2405-2443` | 40 lines | `toml_edit`, written atomically |
| Shell quoting by deleting quotes | `provision/src/scripts.rs` | 3 sites | `shlex::try_quote` |
| Image dimension readers (PNG, JPEG, GIF, WebP, AVIF, BMP) | `master/src/sniff.rs:545-669` | about 125 lines | `imagesize`. The AVIF reader is a substring scan, not a box parser |
| HTML `<link>` scanner, entity decoding, tag stripping | `web/src/favicon.rs:70-209`, `sources.rs:98-150` | about 200 lines | `dom_query` (already in the tree through `dom_smoothie`), `html-escape`. A hand tokenizer on hostile HTML is the wrong place to save a dependency |
| Percent-encoding and decoding | `http/media.rs:241-254`, `blobs.rs:390-397` | 25 lines | `percent-encoding` (already in the tree) |
| Schema management | `CREATE TABLE IF NOT EXISTS` + JSON in `kv` | — | `rusqlite_migration` or `refinery` |
| SSH provisioning: users, sudoers, WireGuard, nftables, sshd, watchdog | `crates/provision`, `master/src/provision.rs`, `node_setups.rs` | 3,396 lines | Ansible, cloud-init, or a documented shell script the user runs. See D4 |
| Browser: server-state fetching and invalidation | `web/src/components/Shell.tsx` | 748 lines | TanStack Query |
| Browser: wire types kept in step by hand | `web/src/types.ts` | 858 lines | `ts-rs` / `specta` |
| Browser: `Select`, tabs, mention popup | `Select.tsx` and others | — | React Aria or Radix, or fix the ARIA wiring |
| Browser: e2e runner | `web/e2e/*.cjs` | 3,426 lines | `@playwright/test` |
| Browser: FNV-1a, four copies | see D7 | — | one copy |

### 7.2 Defensible, keep

| Hand-written | Where | Why it can stay |
|---|---|---|
| Blob file format: AES-256-GCM in STREAM segments, per-blob key, blob id as AAD | `master/src/blobs.rs` (771 lines with serving) | The construction is the standard one and uses `aws-lc-rs` primitives; range reads need segment access. But it is your own on-disk crypto format: write the format down, add test vectors, and consider RustCrypto's `aead::stream`, which is this exact scheme |
| HTTP `Range` parser | `blobs.rs:303-352` | Single range, table- and fuzz-tested |
| Cookie building and parsing | `auth.rs:647-705` | Values are hex or base64url only |
| Static asset gzip cache and ETags | `master/src/assets.rs:35-173` | Precompressing at build time would be simpler still |
| Routine template engine | `routines/template.rs` (599 lines) | Deliberately single-pass with a taint flag; a general engine (`minijinja`) would widen what webhook data can do |
| Webhook rate buckets | `routines/buckets.rs` | Three purpose-specific buckets with an injected clock |
| File-type sniffing beyond `infer` (SVG, Ogg, EBML, ELF/PE/Mach-O) | `sniff.rs` (the rest of its 1,158 lines) | `infer` does not cover these; bounds-checked and fuzzed |
| Public-address check for SSRF | `web/src/lib.rs:39-70` | Correct in design; close the gaps in L12 |
| TLS accept loop | `cli/src/net.rs` | 50 lines; `axum-server` is the alternative |
| Vector search in SQLite, Qdrant REST client | `memory/src/sqlite.rs`, `qdrant.rs` | Fact counts are per user and small; `qdrant-client` drags in gRPC for five calls |
| Repo map with PageRank | `crates/codemap` (368 lines) | No mature crate does this |
| WireGuard key generation | `provision/src/wg.rs:39-48` | Ten lines over `aws-lc-rs` X25519 |
| Browser: hash router, WebSocket replay client, charts, form validation mirroring Rust, `Combobox`, menus on native `<dialog>`/`popover` | `web/src/lib/routes.ts`, `live.ts`, `ui/*` | Small, tested, domain-specific |

---

## 8. What is done well

Worth keeping exactly as it is:

- **Bind policy** (`cli/src/net.rs`): loopback by default, refuses `0.0.0.0`,
  recognises WireGuard interfaces, detects host networking in a container.
- **`Host` allow-list on every request and `Origin` check** against DNS
  rebinding and cross-site WebSockets; `HttpOnly; SameSite=Strict` cookies;
  no token in a URL or in JS storage.
- **SSRF guard for model-chosen URLs** (`web/src/lib.rs:39-151`): filtering
  at the DNS resolver so the checked address is the connected address,
  redirects re-checked, bodies capped, plus a provenance rule that the URL
  must have appeared in user or tool text.
- **Stored files**: sniffed by magic bytes, served as attachments unless a
  safe raster type, with a per-response sandbox CSP and `nosniff`.
- **Webhooks**: 256-bit secrets, header only, stored hashed, constant-time
  compare, dummy compare for unknown ids, size and depth limits.
- **Authorization**: every handler checks owner or membership and answers
  404 for other people's ids; admins cannot read other users' chats.
- **First-run claim code** so a fresh server cannot be taken over by whoever
  reaches it first.
- **Markdown rendering**: no raw HTML, no remote images, no
  `dangerouslySetInnerHTML` anywhere, link hosts shown, bidi characters
  stripped.
- **`cargo deny`** with a permissive-licence allow-list and reasoned
  exceptions; `unsafe_code = "forbid"`; clippy with `-D warnings`.
- **Tests**: about 630 Rust tests including hostile-node and link-hardening
  suites, 776 web tests, fixture tests shared between Rust and TypeScript.

---

## 9. Order of work

| # | Do | Why first |
|---|---|---|
| 1 | Fix `pnpm size`, protect `main`, required checks | Nothing else is trustworthy while red merges are allowed |
| 2 | Keep the data directory out of every workspace; then sandbox the agent shell and allow-list its environment (S1) | The product is an RCE service; this is its only real containment |
| 3 | Random per-session `tool_output` ids; cap streamed shell output (S2, S3) | A few lines each; one is a cross-user leak, the other fills the disk |
| 4 | Login limits, server timeouts, WebSocket revocation (S4–S6) | Cheap, and all pre-auth or session-lifetime |
| 5 | Move PDF parsing out of process; fix the symlink write; harden git (S7–S9) | One upload or one hostile repository should not be enough |
| 6 | Migrations + backup command (D2, §6.3) | Before any data exists that you care about |
| 7 | Revert to server-side sessions; delete JWT, Redis, MariaDB (D1) | Removes about 600 lines of auth you have to get right |
| 8 | Connection pool, scoped queries, pagination (D3) | Before a second active user |
| 9 | Extractor-based auth, split `http.rs` (D6) | Makes the next 100 handlers safe by construction |
| 10 | Freeze or cut scope (D4) | Every feature kept is audited forever |
| 11 | Pin Actions and images, sign releases, add bots (§5.2) | The binary is pushed to other people's servers |
