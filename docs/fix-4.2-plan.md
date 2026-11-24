# FIX_PLAN 4.2: inventory and plan (stage 0)

Status: plan, 2026-11-24, read at `cc411909` (origin/main). Done since: the split (two pull requests) and pull request 3, "Errors" (branch `fix/save-errors`: scrubbed 500 log, `SaveError`, config and secrets writes on the blocking pool, `tokio::fs` unlinks, the `std::fs` source check). Left: pull request 4, the keyed limiter.

Row 4.2: "Split `http.rs` by area into `http/*`; one `ApiError` enum that logs server errors and maps them; one keyed rate-limit module (`governor`) replacing the five throttles; file IO in handlers moved to `spawn_blocking`. Done when: `http.rs` holds the router and shared types only. M"

## 0. Verdict in one paragraph

Two of the four parts are still worth doing, one in a smaller form than the row says, and one is mostly done already.
**The split: do it**, as pure moves in two pull requests; `http.rs` goes from 2,085 to about 490 lines, and about 270 of those are the route table.
**The error enum: do it differently.** The part that matters (no internal text in a 500, the cause logged with a request id) was done in 3.6 by `response::finish`, at the response, which is stronger than an enum because it also covers 500s nobody built through `ApiError`.
What is left is small: scrub secrets from the logged 500 body, and stop the config-save path from returning a server file path in a 400.
Turning the tuple struct into an enum would rewrite 308 call sites for no change a user or operator could see: don't.
**`governor`: don't.** It passes the age and licence rule, but it is a token bucket (GCRA) that cannot express the login throttle (reserve, give back on success, clear on success, bounded keys), adds five to eight crates including a third `hashbrown`, and its key store has no capacity bound.
The row's goal, one keyed module, is met by our own `FailureMap` (already written, bounded, tested) taking over the two hand-written copies of the same idea.
**Blocking IO: mostly obsolete.** Uploads, blob serving, store calls, PDF/Office extraction and made files already run in `spawn_blocking`; what remains is writing `config.toml` and `secrets.toml` (a few KB each) on the async thread, folded into the error pull request.

## 1. What each part was for, and whether the evidence still holds

**D6 (structure).** The audit (`AUDIT.md` D6) measured `http.rs` at 2,464 lines holding "router, error type, sessions, users, memory config, SSH provisioning, graphify, the classifier and a TOML editor", authentication opt-in by a hand-written `require`, five hand-written throttles and blocking file IO at `providers.rs:234-255`, `http/providers.rs:276-277`, `http.rs:2406-2442`.
Today: 2,085 lines; authentication is a route layer plus extractors (4.1, `docs/auth-by-construction.md`), so the opt-in half is gone; users, SSH and the TOML editor are gone from `http.rs`; sessions, memory config, the classifier and a graphify status handler are still there; `http/` has 48 files (13,772 lines).
The `http.rs:2406-2442` IO no longer exists; `providers.rs` `persist` (now `providers.rs:228`) still writes `config.toml` synchronously.

**L4 (errors).** "Store errors (`sqlite: …`) and file paths are returned to clients in 500 bodies and not logged" (`http.rs:230-240`).
Fixed in 3.6: `crates/master/src/response.rs` `finish` is the outermost layer; every 500 (from `ApiError` or a hand-built tuple) has its body logged with a 16-hex request id and replaced by `{"error":"internal error","request_id":…}`.
The evidence no longer holds for 500s. Two remainders, see §3.

## 2. Inventory: `crates/master/src/http.rs` (2,085 lines, no tests in the file)

| Lines | Kind | Size |
|---|---|---|
| 1-16 | imports | 16 |
| 17-64 | `mod` list (48 modules) | 48 |
| 66-332 | `router`, `router_with`, `assemble` (layers: timeouts, `auth::guard`, `response::finish`), `public()`, `private()`, `private_routes()` | 267 |
| 334-419 | `ApiError(StatusCode, String)`, `IntoResponse`, `From` for `HashError`, `CommandError`, `StoreError`, `WsError`, `AccountError`; `ApiResult` | 86 |
| 421-481 | shared helpers: `visible_session`, `acting_session`, `agentic_gate`, `parse_sid` | 61 |
| 483-2085 | handlers and their bodies/helpers | 1,603 |

Handlers still in `http.rs` (line counts include doc comments and the body struct) and where each goes:

| Handler(s) | Route(s) | Lines | Goes to |
|---|---|---|---|
| `setup_status`, `setup`, `session_response`, `login`, `logout` | `GET/POST /api/setup`, `POST /api/login`, `POST /api/logout` (public) | 6+23+13+18+20 = 80 | new `http/account.rs` (registration stays in `public()`: the route walk reads `fn public` in `http.rs`) |
| `me`, `change_password` | `GET /api/me`, `POST /api/me/password` | 25+38 = 63 | `http/account.rs` |
| `nodes`, `admin_nodes`, `node_join_token`, `revoke_node` | `GET /api/nodes`, `GET /api/admin/nodes`, `POST /api/admin/nodes/join`, `DELETE /api/admin/nodes/{id}` | 21+10+19+13 = 63 | new `http/nodes.rs` |
| `ScopeQuery`, `settings_scope`, `scoped_resolver`, `models`, `agent_json`, `agents` | `GET /api/models`, `GET /api/agents` | 108 | `http/agent_edits.rs` (already serves `POST /api/agents` and `/api/agents/{id}`; 473 → about 580) |
| `notices`, `features`, `get_savings`, `put_savings`, `get_graphify` | `GET /api/notices`, `GET /api/features`, `GET/PUT /api/admin/savings`, `GET /api/admin/graphify` | 12+21+25+14 = 72 | new `http/server_settings.rs` |
| `transcribe` | `POST /api/transcribe` | 39 | new `http/voice.rs` |
| `SearchQuery`, `search`, `search_kinds`, `kinds_shown` | `GET /api/search` | 113 | new `http/search.rs` |
| `ClassifierState`, `classifier_key`, `get_classifier`, `put_classifier` | `GET/PUT /api/me/classifier` | 77 | new `http/classifier.rs` |
| `session_json`, `WorkspaceQuery`, `query_flag`, `SessionsQuery`, `list_sessions`, `get_session`, `delete_session`, `events`, `CreateBody`, `create_session`, `create_checked`, `attention`, `mark_seen` | `GET/POST /api/sessions`, `GET/DELETE /api/sessions/{id}`, `/events`, `/seen`, `GET /api/attention` | about 350 | `http/sessions.rs` (52 lines today, rename only) → about 400 |
| `PromptBody`, `give_back_files`, `prompt` (193 alone), `check_reply_body`, `cancel`, `set_agent`, `approval`, `restore_turn`, `select_variant` | `/api/sessions/{id}/prompt`, `/cancel`, `/agent`, `/approvals/{approval_id}`, `/turns/{turn}/restore`, `/variants` | about 380 | new `http/turns.rs` |
| `memory_cfg_json`, `refresh_embedder`, `get_memory_cfg`, `MemoryCfgBody`, `embed_failure`, `put_memory_cfg` | `GET/PUT /api/admin/memory` | 245 | new `http/memory_settings.rs` (not `http/memory.rs`: that is the memory store's 812 lines) |
| `runtime()` | helper used by 5 files, 33 calls | 11 | stays (shared) |

What "router and shared types only" leaves: imports and mods (about 75), the router (267), `ApiError` (86), the four session helpers (61), `runtime` (11): **about 490-510 lines.**
That is the floor unless the route table is also split into per-area `routes()` functions, which would take it to about 350 at the cost of the one place that shows every route; the route walk does not need it. I would stop at about 500.
Items used from other files must become `pub(super)` and be imported from their new home: `query_flag` (2 files), `WorkspaceQuery`, `ScopeQuery`, `settings_scope`, `agent_json` (2), `refresh_embedder` (`http/providers.rs:312`), `create_checked`, `CreateBody` (3).

## 3. Inventory: errors

- **Shape:** `pub(crate) struct ApiError(StatusCode, String)` (`http.rs:334`); `into_response` writes `{"error": msg}`, adds `Retry-After` when the text equals `auth::BUSY`, and a `code` field when it equals `forge_repos::LISTING_BUSY` (string comparison decides the response shape).
- **Constructions:** 308 `ApiError(` in 46 files (47 in `http.rs`, 261 in `http/`, none outside); 200 `(StatusCode::…,` tuples in `http*` overall; about 55 local constructor helpers (`bad`, `forbidden`, `conflict`, `not_found`, `unknown`, `refused`, `err`, `store_err`, …), of which `bad` exists 7 times, `conflict` 4, `forbidden` 4.
- **Other error types handlers return:** `Response` built by hand (`hooks.rs` `answer`/`error`, `media.rs:291` 429 with `Retry-After: 5`, `favicon.rs`, `ws.rs:276`), `LoginError`/`ClaimError` with their own `IntoResponse` (`auth.rs:470`, `auth.rs:760`).
- **5xx in `http*`:** `INTERNAL_SERVER_ERROR` 22 sites, `BAD_GATEWAY` 22, `SERVICE_UNAVAILABLE` 32, `GATEWAY_TIMEOUT` 8, `INSUFFICIENT_STORAGE` 4. The 500s with dynamic text: `From<CommandError>` (`Store`), `From<StoreError>` (all but `Refused`/`Conflict`), `AccountError::Internal`, `memory_err` (`http/memory.rs:61`), `projects.rs:253,383,393,466,952`, `projects/folder.rs:184,199`, `providers.rs:293`, `http.rs:1855`. All are replaced by `response::finish` before they reach the client and logged there once with the request id.
- **502/503/504** are not touched by `finish`; I read every 502/504 site: fixed sentences, the node's own refusal, or an upstream provider's message (`agent_edits.rs:422`, `http.rs:1744` `embed_failure`, `http.rs:2000` transcription, `connections.rs:173`, `custom_connectors.rs:485` with the host only). Provider errors are built with `reqwest::Error::without_url()` (`agent-native/src/models.rs:95,135`), so a key in a URL does not reach them. These texts are meant for the admin who configured the provider; I would leave them.
- **Leak 1 (4xx with a server path).** `ProviderRuntime::save`/`apply` (`providers.rs:256,266`) return one `anyhow::Error` for both validation and the `config.toml` write (`config.rs:786`: `writing /…/config.toml.tmp`). Callers map all of it to 400 with the text: `http.rs:2065` (savings), `http/storage.rs:85`, `http/workspaces.rs:804`, and `http/providers.rs:310` with `{e:#}` (the full chain, including the OS error). `http.rs:1855` maps the same call to 500. Admin-only routes, so the reader is the server's own admin: low severity, but it is exactly L4's "file paths returned to clients", in a 400 that `finish` does not catch.
- **Leak 2 (log).** `response::internal` logs the 500 body verbatim. No 500 body I found carries a secret today, but nothing stops one: the master's `SecretSet` (`providers.rs:181`) is not applied. The invariant is "no secret in a log line".
- **Double logging:** `acting_session` (`http.rs:450`) and others `warn!` and then return a 500 that `finish` logs again, without the request id in the first line. Harmless; not worth a change on its own.

## 4. Inventory: throttles and bounds

Rate limits (time-based), the row's subject:

| # | What | Key | Numbers | Structure, eviction | Where checked | Tests |
|---|---|---|---|---|---|---|
| 1 | Failed logins (`auth.rs:563-565`, `auth/limits.rs` `FailureMap`) | lowercased username, after format check | 5 failures / 15 min sliding | `HashMap<Box<str>, Vec<Slot>>`, capacity 65,536 keys, evicts expired then oldest; reserve under one lock, give back on store error or cancel, cleared on success | public route; after the 16 KiB body limit and JSON parse; before the hash permit (read-only check) and again after (reserve) | 7 unit tests in `auth/limits.rs`; `login_limits_http.rs` (50 parallel guesses record 5; malformed names leave no entry; 1 MiB username refused before parsing) |
| 2 | Failed member adds, "no such user" (`auth.rs:574-598`, used by `http/workspaces.rs:507-530`) | caller's user id | 10 / 15 min sliding | `HashMap<String, Vec<i64>>`, **no key bound and empty vectors never removed** (bounded only by the number of accounts); a hand copy of `FailureMap`'s reserve/release | after the caller layer and after the JSON body | `auth.rs:1193` unit; `workspaces_http.rs`, `accounts_http.rs` |
| 3 | Webhooks (`routines/buckets.rs`) | unknown ids: one server-wide bucket; wrong secret: per trigger; calls: per trigger | 100 / 15 min; 20 / 15 min; `hook_rate_per_minute` (config) with burst 5 | integer token buckets on the routine clock, exact `Retry-After` in ms; hourly `prune` drops full buckets (`routines/results.rs:163`); keys only for triggers that exist | public route, the secret is the credential; after the 64 KiB body limit, before anything about the routine shows (`hooks.rs:1-20`) | 4 unit tests in `buckets.rs`; `routines_http.rs` |
| 4 | Artifact publishes from a node (`artifacts.rs:72-92`) | node id | 30 `artifact_staged` / hour sliding | `HashMap<String, VecDeque<Instant>>`, no key eviction (bounded by known nodes) | node link, not HTTP | `artifacts.rs:542` |
| 5 | Dictation quota (`voice.rs:53-110`) | account | 200 requests and 100 MB per UTC day, one at a time | fixed calendar-day counters, cleared at day change | after the caller layer and the 25 MB body | `voice.rs` unit, transcription tests |

The audit's fifth, "upload and favicon slots", is not a rate limit, and it missed #5.

Concurrency bounds (how many at once, released on drop), **not throttles in the row's sense; they stay as they are**:
upload slots per account and total (`storage.rs:131`), favicon fetches (8 total, 2 per user, `favicon.rs:32-36`), Argon2 permits (4, `auth/limits.rs:243`), PDF/Office helper per account and total (`pdf.rs:104-155`), attached-file reads (`attached.rs:133-225`), WebSocket connections (8 per user, `ws.rs:61`), node handshakes (`nodes.rs:127`), accepted connections (`serve.rs:111`), routine runs (`routines/mod.rs:185`), forge listings (`forge.rs:260`), connector lookups (`connectors/mod.rs:652-675`), per-node semaphores in `recall.rs`, `board.rs`, `connectors/call.rs`, `stored_skills.rs`, `artifact_pull.rs`, and eight "waiting" tables with a total and a per-account cap (`diff.rs` 32/4, `files.rs` 32/4, `folder_preview.rs` 16/1, `git_state.rs` 32/2, `skills_menu.rs` 32/1, `bg_output.rs` 32/4, `project_folder.rs`, `project_mcp.rs`).
A rate limiter would be wrong for these: they bound work in flight, and a token bucket would refuse a person whose earlier requests already finished.
The eight waiting tables are the same 40 lines eight times (`diff.rs:100-150` is the pattern); a generic `Waiting<A>` would remove real duplication, but it is node-link code outside this row: a separate task if wanted.

## 5. Inventory: blocking work in handlers

Already off the async threads: every store call (`Store::call` is `spawn_blocking`, `store/src/lib.rs:842`), upload writing and the shard rename with fsync (`storage.rs:448,615`), blob serving and decryption (`blobs.rs:534`, `media.rs:357`), attached-file parsing (`attached.rs:423,496`), made files (`made.rs:222`), uploads (`uploads.rs:174`), skill files (`http/skills.rs:217`), Argon2 (`auth/limits.rs` `run`), PDF and Office extraction (a helper process). Thumbnails are made by the browser (`put_thumb`), not decoded here; profile images are only header-checked (`http/profile.rs:79-85`).

Still on the async thread:

| Where | What | Size |
|---|---|---|
| `ProviderRuntime::persist` (`providers.rs:228`, `config.rs:786`) via `save`/`apply`, from 10 handlers (`http.rs:1854,2064`, `http/storage.rs:84`, `http/workspaces.rs:803`, `http/providers.rs` `apply` ×6, `http/pricing.rs:32`, `http/agent_edits.rs:352`) | write `config.toml.tmp`, rename | a few KB, no fsync |
| `secrets::set` (`http/providers.rs:296,299,327`) | read, write, rename `secrets.toml` with mode checks | a few KB |
| `sealed.rs` key file (first `seal` per process) | read or create a 32-byte key | once |
| `http/attachments.rs:410`, `http/media.rs:315` | `remove_file` of a refused temp upload | one unlink |
| `attachments::extract` text path (`attachments.rs:110-115`) | NUL scan and UTF-8 check of a text upload | up to the text cap, milliseconds |

None of these is a measured problem: page-cache writes of a few KB. On this project's slow USB disk a rename can stall when the disk is busy, which is the only argument for moving them.

## 6. The plan

### A. The split (fast lane: pure moves)

Two pull requests, not one per area: 95 commits touched `http.rs` in the last two weeks, and each pull request is a rebase for every open branch that edits it.

- **PR 1 (S):** `account.rs`, `nodes.rs`, `server_settings.rs`, `voice.rs`, `search.rs`, `classifier.rs`, the agents list into `agent_edits.rs`. About 615 lines moved. One area per commit.
- **PR 2 (S-M):** `sessions.rs`, `turns.rs`, `memory_settings.rs`. About 975 lines moved. `prompt` and `approval` are on the approval path, so this one gets an independent reviewer even though it is a move.

A commit may change only: the `mod` line, `pub(super)` on moved items, `use` lines, and the path in `.route(…)`. Proof, per commit:
1. `git diff --color-moved=plain --color-moved-ws=allow-indentation-change` shows every removed handler line as moved; a reviewer sees no uncoloured line inside a function body.
2. The route walk (`route_walk_http.rs`) follows merges from `http.rs` and resolves `module::handler` paths (it reads every file and fails on an unmerged one), and the member walk (`member_walk_http.rs` `Code::resolve`) resolves `use`, `super::` and `crate::` paths: both re-derive `ADMIN_ONLY`, `ANY_LOGIN`, `BYTE_ROUTES` from the code and compare with the written lists, which must not change.
3. The golden walk (`the_layer_answers_as_require_did`, `auth_golden.txt`, 4,298 lines) must pass unchanged.
4. `cargo test -p harness-master` (every `*_http.rs` area test), workspace clippy, fmt.
5. `wc -l crates/master/src/http.rs` in the pull request body; after PR 2 about 500.

### B. Errors (full lane, small)

Keep `ApiError(StatusCode, String)` and the layer. Changes:
1. **Scrub the 500 log.** `response::internal` passes `detail` through the master's `SecretSet` (`providers.rs:181`) before `tracing::error!`. Test first: a handler that returns a 500 whose body holds a known secret; capture the log (a test subscriber writing to a buffer) and assert the secret is absent. Fails today.
2. **Typed save errors.** `ProviderRuntime::save`/`apply` return `SaveError { Invalid(String), Io(anyhow::Error) }`. `From<SaveError> for ApiError`: `Invalid` → 400 with today's exact text; `Io` → 500 with the cause in the body (the layer logs it and sends the fixed sentence). The four `map_err(|e| ApiError(BAD_REQUEST, e.to_string()))` and `bad(format!("{e:#}"))` become `?`. Test first: make the data directory's `config.toml` unwritable (a directory at `config.toml.tmp`), `PUT /api/admin/savings`: today 400 with the path; after, 500 `internal error` with a request id. Validation 400s keep their bytes (existing `settings_http.rs`, `provider_http.rs`, `storage_http.rs` tests).
3. **The persist moves to `spawn_blocking`** in the same change (part D): `save`/`apply` become async and run `persist` and `secrets::set` on the blocking pool; **Correction after review:** the order of writes is NOT safe by itself once the write awaits. `limits::enforce` cuts a request after 30 s, which drops the handler future and its lock guard while the blocking write goes on (before, the sync write never yielded and could not be cut). So the whole change is one *section*: a spawned task that owns an owned lock guard (`lock_owned`) from the first write to the switch of the running configuration, and the handler only awaits it (`ProviderRuntime::section`; proof: `save_cancel_http.rs`, which holds the write up on a named pipe so the timeout lands inside it). `Config::save` and `secrets::set` also take a process-wide mutex, so the fixed temp file names are never used by two writers even without the lock. Only writes (and the section's own reads: `secrets::get_off_thread`, `build`) moved to the blocking pool; `state_json`'s `key_state`/`resolve_key`, `refresh_embedder`, `embedder`, the sealed-key read and the text check stay on the async worker.

Not doing: an enum with a variant per status (308 sites rewritten, every 4xx sentence re-typed, no behaviour change); per-site logging (the layer already logs once with the id, and cannot be forgotten); turning the `BUSY`/`LISTING_BUSY` string comparisons into variants (two sites, works, tested).
Optional, only if the owner wants the duplication gone: `ApiError::bad/forbidden/conflict/not_found(msg)` constructors replacing the about 25 identical local helpers, one file per commit, byte-identical bodies. It is churn across 20 files that the two walks and area tests cover; I would skip it unless those files are being touched anyway.
What breaks: nothing a client sees except the four save paths (400 → 500 when the disk write fails). No test asserts a 500 body's text (they would already fail against `finish`).

### C. Rate limits: one module of our own, not `governor`

**`governor` against the dependency rule** (crates.io and docs.rs, read 2026-11-24, crate name only in requests):
- Latest 0.10.4, released 2025-12-16 (11 months old: passes the two-week rule); 0.10.3 the same day, 0.10.2 2025-11-13. Licence MIT (permissive). About 97.7 million downloads; maintained but slow (one release burst in the last 11 months).
- Normal dependencies of 0.10.4: `cfg-if`, `hashbrown ^0.16`, `nonzero_ext`, `portable-atomic`, `smallvec`, `spinning_top`, `web-time`; with default features (`std`, `dashmap`, `jitter`, `quanta`) also `dashmap ^6.1`, `futures-sink`, `futures-timer`, `futures-util`, `getrandom ^0.3`, `parking_lot`, `quanta`, `rand ^0.9`. Against our `Cargo.lock`: **new crates** `governor`, `nonzero_ext`, `spinning_top`, `hashbrown 0.16` (we already carry 0.12 and 0.17), and with defaults `dashmap`, `quanta` (and its `raw-cpuid` on x86), `futures-timer`. Five new crates with `default-features = false, features = ["std"]`, eight or more with defaults. This is from the published dependency list compared with our lock file; I did not run `cargo tree` or `cargo deny`.
- Keyed API: `RateLimiter::keyed`/`dashmap`/`hashmap`, `check_key`, `retain_recent` (drops keys whose state equals a fresh one; a manual call), `len`, `shrink_to_fit`; custom clocks are supported. **No bound on the number of keys** (a flood of distinct keys grows the map until the next `retain_recent`), **no way to give a cell back**, **no per-key reset**.
- Semantics: GCRA, i.e. a token bucket.

Per throttle:

| # | Verdict | Reason |
|---|---|---|
| 1 Login | **keep `FailureMap`** | Needs reserve-then-release (a cancelled request or a store error gives the slot back), clear on success, "5 failures", and a hard key bound. `governor` has none of the four; a token bucket would count successes and let a legitimate user be locked by their own logins. Security code: no change. |
| 2 Member-add lookups | **move onto `FailureMap`** | It is a hand copy of the same reserve/release idea without the key bound and with empty entries kept forever. Same numbers (10 / 15 min); gains the bound and drops empty keys. |
| 3 Webhooks | **keep** | Already one module, integer-exact, with a configurable per-trigger rate, a millisecond `Retry-After`, the routine clock and an hourly prune. `governor` would fit the shape, but replaces 100 tested lines with a dependency, and the config-driven rate needs the limiter rebuilt on change. No gain. |
| 4 Artifact publishes | **move onto `FailureMap`** (reserve then `failed()` at once = count an event) | Same sliding-window count. Note: `FailureMap` takes wall-clock ms; the throttle uses `Instant` today. Pass a monotonic ms (`Instant` since process start) so a clock jump changes nothing. |
| 5 Dictation quota | **keep** | A daily count-and-bytes quota resetting at UTC midnight, plus one-at-a-time: not a rate. |

So: `FailureMap` moves from `auth/limits.rs` to its own module (`crates/master/src/keyed_limit.rs`, since `limits.rs` is the timeout table), unchanged; the lookups and the artifact throttle use it. Three of the four time-based limits share one bounded module; the webhook buckets stay because they are a different algorithm with a reason. That is the row's intent without the row's crate.
Tests first: lookups: 70,000 distinct callers each fail once → the map holds at most its capacity, and a caller whose failures expired leaves no entry (both fail today: unbounded, empty vectors kept). Every existing login, lookup, webhook and artifact test stays as it is and must pass unchanged.

### D. Blocking work

Folded into B.3 (`save`/`apply`/`secrets::set`). The two `remove_file` calls become `tokio::fs::remove_file`. The sealed key read and the text check stay (once per process; milliseconds).
Catching a regression: there is no test that fails when a handler blocks the runtime, and a timing test would be a flake under this machine's load. A cheap check instead, in the style of the route walk: a test that reads `http.rs` and `http/**/*.rs` (outside `#[cfg(test)]`) and fails on `std::fs::`. It catches a handler doing file IO itself, not a helper it calls; that is its honest limit.

### E. Order, size, lane

| PR | What | Size | Lane | Proof | What can go wrong |
|---|---|---|---|---|---|
| 1 | Split part 1 (7 areas) | S | fast | moved-lines diff, route walk, member walk lists, golden walk, area tests | rebase conflicts with open branches; a missed `pub(super)` is a compile error, not a bug |
| 2 | Split part 2 (sessions, turns, memory settings) | S-M | fast, but an independent review (approval path) | same | same; `prompt` is the busiest function in the file, so the most conflicts |
| 3 | Errors: scrubbed 500 log, typed save errors, persist in `spawn_blocking`, `tokio::fs` unlinks, the `std::fs` source check | S | full | the two failing-first tests; existing settings/provider tests unchanged | `save` becoming async touches 10 call sites; a lock held across the await is the edit lock, which is meant to be |
| 4 | `FailureMap` as the shared keyed module; lookups and artifact throttle on it | S | full (auth) | the bound test failing first; all login/lookup/webhook/artifact tests unchanged | lookup semantics: today a released slot is matched by timestamp (`release_lookup` with `now_ms`), `FailureMap` by reservation id; strictly better (two adds in the same millisecond can no longer release each other's slot), but a behaviour change in auth code: say so in the PR |

Total about two days, which matches the row's M. PRs 3 and 4 are independent of 1 and 2 and can go first if the split must wait for open branches.

Would not do: `governor`; an error enum; per-route routers to shrink `http.rs` below about 500; per-handler `spawn_blocking` wrappers; touching the concurrency bounds.

### F. Obsolete or in conflict

- "Logs server errors" is done (3.6, `response.rs`), and done better than the row's design.
- "Five throttles": the audit's list mixed rate limits with concurrency bounds and missed the dictation quota; there are four time-based limits plus one quota.
- "`governor`": fails on fit and dependency weight, not on the rule's age or licence. The row says "(`governor`)"; FIX_PLAN rule 6 already says named crates are candidates to check.
- "File IO in handlers moved to `spawn_blocking`": the cited sites are gone; what is left is a few KB of config writes.
- No part conflicts with an invariant. The split must keep `fn public` in `http.rs` (the route walk reads it there).

## 7. Questions for the session lead or the owner

1. Drop `governor` and use our `FailureMap` as the one keyed module (the row's text names the crate). Recommendation: yes.
2. Keep `ApiError` as a tuple struct, with the remaining work being the scrubbed log and typed save errors. Recommendation: yes; skip the optional constructor clean-up.
3. When to land the split, given how many branches touch `http.rs`. Recommendation: PRs 3 and 4 first; the two split PRs at a moment the lead picks when few branches are open, each merged the day it is opened.
4. A generic `Waiting<A>` for the eight node round-trip tables: a separate task outside 4.2, or not at all. Recommendation: not now.

## 8. Decisions

The session lead decided on 2026-11-24, on the questions in section 7. They are deviations from the wording of the FIX_PLAN row, and the owner may overrule them.

1. **No `governor`.** Its keyed store has no bound on keys, cannot give a slot back and cannot clear one key, so it cannot express the login rule. Our own `FailureMap` becomes the one keyed limiter (a later pull request).
2. **`ApiError` stays a tuple struct.** 500s are already logged once with a request id and answered with a fixed sentence (task 3.6). The remaining error work is the scrubbed log and typed save errors (a later pull request), not a rewrite of 308 sites.
3. **`http.rs` ends at about 500 lines.** It keeps the route table, the layers, `ApiError` and the shared session helpers. The route table is not split.
4. **The eight node round-trip waiting tables are not merged now.**
