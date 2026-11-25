# Store readers and the paged chat list

FIX_PLAN 4.5 (audit D3), stage 0: inventory, measurement, design; since slice 1 also what that slice built and measured.
Sections 1 and 2 are stage 0's (no production code was changed by it); where slice 1 differs from stage 0's design, the text says so.
The throwaway bench (`bench_readers.rs`, an in-crate `#[ignore]` test) and its two logs are kept in `.claude/handoff-2026-11-15/b-store45/` (git-ignored); every number below comes from them.

## Verdict first

The row asks for two things: scoped, paged session queries, and a pool of read-only connections beside the writer.
The measurement says the first one meets the plan's bar on its own, by a factor of more than 200, on today's single connection.
The second one is not needed for the bar, costs about 2.2 MB of RSS per reader per database file, and would not fix the slowest things that hold the lock today, which are not reads at all.

**Do A** (scoped keyset queries, page-sized annotations, no whole-table reads on request paths, the slow FTS merge out of the delete).
**Don't build B** (the reader pool) now; the trigger for revisiting it is written down below.

## 1. Inventory

Read at `origin/main` 3b2fa9a5.

### How the store is reached

`Store` is `Arc<Mutex<Connection>>` plus a truncate flag (`crates/store/src/lib.rs:353`).
Every method takes `self.lock()` (`lib.rs:849`) for its own duration; no lock is held between two method calls, even inside one `Store::call` closure.
Callers reach it through `Store::call` (`lib.rs:858`), which runs the closure under `tokio::task::spawn_blocking`; about 500 `.call(` sites in `crates/master` and `crates/node`.
So no async worker ever blocks on the mutex: a slow holder parks blocking-pool threads, and every other store call (append, login, auth check, attention) queues behind it.
The server process holds three such stores: `master.db` (the mirror), `chat.db` (the chat runtime) and `node.db` (the embedded node) (`crates/cli/src/main.rs:385, 482, 949`).

### What the row assumes and is already true

- `updated_ms` is a column (`crates/store/schema/6.sql`, table `sessions`), maintained by `append_in` in the append's own transaction (`lib.rs` around 1000; a `handoff_written` event does not move it).
  No backfill is needed.
- The index `sessions_owner ON sessions(owner_id, updated_ms)` exists (`lib.rs:403`, `schema/6.sql:535`).
- `sessions_of(owner)` already filters by owner in SQL (Gate 3 review L-3, `lib.rs:2716`).
  What it lacks is the workspace, a limit and a cursor.

### Callers of whole-table or whole-account session reads

| Where | Call | Path | What it actually needs |
|---|---|---|---|
| `http/sessions.rs:122` `list_sessions` | `sessions_of(owner)`: all of the caller's chats, every workspace | request (every sidebar load, and every `sessions_changed` nudge, which fires on each turn start and end: `lib.rs:2001`) | one page in one workspace |
| same handler | `unread(owner, viewer)` (`lib.rs:2402`) | request | unread marks of the page's rows; today it scans the whole `sessions` table (plan: `SCAN s`, the `?1 IS NULL OR` defeats the owner index) |
| same handler, `projects::links` (`master/src/projects.rs:291`) | `project_links(None)`: every chat's project link, **every user's** | request | the page's links |
| same handler, `routines::annotate` (`routines/results.rs:52` → store `routines.rs:1317`) | `routine_marks(ids)`: one query per listed chat | request | the page's marks, and the "hide seen run chats" filter, which today runs after the fetch and so must move into SQL before a page can have a fixed size |
| `http/projects.rs:154` `own_chats` | `sessions_of` + `links` | request (projects list) | a `COUNT(*) GROUP BY project` |
| `http/projects.rs:611` `board_json` | `sessions()`: **every chat of every user** | request (each board view) | title and owner of the cards' chats |
| `http/projects.rs:998` `chat_owners` | `sessions()` | request (project memory) | owner of the facts' source chats |
| `http/memory.rs:327` `chat_owners` (used at 399, 502) | `sessions()` | request (workspace memory) | same |
| `search.rs:532` (no words) | `sessions_of` + `routine_marks` of all, then skip/take | request | a page (it already pages with OFFSET in Rust) |
| `search.rs:525` (words) | `session(id)` per hit | request | one join |
| `search.rs:561` `chat_results` | `project_links(Some(id))` per row | request | one `IN`; each call scans `projects` (plan: `SCAN p`) because of `?1 IS NULL OR` |
| `http/workspaces.rs:448` `delete_workspace` | `sessions_of` **per member**, filtered in Rust by workspace | request (rare) | one query: chats in workspace W |
| `http/workspaces.rs:641` remove member | `sessions_of(member)`, filtered in Rust | request (rare) | chats of U in W |
| `http/users.rs:254` delete user | `sessions_of(user)` | request (rare) | ids only |
| `workspaces.rs:735` `cancel_revoked` | `sessions_of` **per user** | background (`sweep_revoked` spawns it) | running or backgrounded chats of those users |
| `workspaces.rs:759` `cancel_revoked_on` | `sessions_on(node)` | background (link up) | fine as is (scoped by node) |
| `callbacks.rs:559` `start` | `sessions_updated_since` + one read per chat | start | fine (one pass at start; 1.3 ms full scan measured) |
| `node/src/lib.rs:701` | `sessions()` | start, on the node's own `node.db` | fine (the node's own chats, once) |
| `store/src/backup.rs:197` | `sessions()` | offline CLI (`restore`) | fine |

After A, `s.sessions()` is left only in the last two rows, neither on a request path, which is what the row asks.

### N+1 loops (the audit names routines and members; each verified)

1. **Routines:** `routine_marks` (`store/src/routines.rs:1317`) runs one indexed lookup per listed chat under one lock.
   It is the single most expensive part of today's list: 70 ms for 10,000 chats (below).
2. **Members:** `GET /api/workspaces/{id}/members` (`http/workspaces.rs:486`) calls `member_json` per member, which makes two more store calls each (`Prefs::load` and `profile::image_version`, `http/workspaces.rs:161, 169`): 1 + 2N `spawn_blocking` round trips.
   Cheap per call; it is a roster of tens, not thousands.
   Fix with one `kv` read by prefix and one `profile_images` read for the workspace, in the same slice as the other members loops.
3. **Members, sessions:** `delete_workspace` loops `sessions_of` per member, and `cancel_revoked` loops it per user (table above).
4. **Search:** `session(id)` per FTS hit and `project_links(Some(id))` per result (table above); not named by the audit.

### Store work held under the lock that is slow

Measured on the 50,000-chat database below; each blocks every other store call for its whole length.

| What | Where | Time | Notes |
|---|---|---|---|
| `delete_session` | `lib.rs:1523-1527` | **584–658 ms** | `INSERT INTO chat_fts(chat_fts) VALUES('optimize')` merges the whole FTS index on every delete of an indexed chat. Routine retention deletes up to 100 run chats an hour (`routines/results.rs:22`), each one a 0.6 s stall. Same pattern in `artifacts.rs:455` and `media.rs:333` (smaller indexes, not measured). |
| `search` with two common words | `lib.rs:1351` | 170 ms | bm25 + snippet over the user's matches. |
| today's `list_sessions` store side | four calls above | 89 ms | 70 of them in `routine_marks`. |
| `sessions()` | `lib.rs:2706` | 52 ms | every chat of every user; three request paths call it. |
| `checkpoint` | `lib.rs:2036` | 3.4 ms | zero busy timeout already, never waits for a reader. |

### Methods that read and then write and rely on the one lock

Every method that writes keeps the writer; this list is what must never be split across a reader and the writer.
Found by scanning each `Store` method for both a `SELECT` and a write (a heuristic; the slice that introduces any read/write split must re-check by reading).
Most use a transaction; the ones marked "no tx" rely only on the mutex for atomicity.

- `lib.rs`: `append_in`, `rename_session`, `rebuild_search_index`, `delete_session_leaving`, `node_join`, `attachment_put`, `attachments_take_owned`, `claim_session`, `create_user`, `delete_user`, `ensure_personal_workspaces`, `personal_workspace`, `remove_member`, `set_exec_ceiling`, `set_member_level`, `personal` (no tx).
- `projects.rs`: `update_project`, `link_new_session` (no tx), `add_column`, `update_column`, `delete_column`, `reset_columns`, `add_card`, `update_card`, `append_card_note`, `delete_card`, `create_board_template`.
- `routines.rs`: `routine_create`, `routine_update`, `webhook_claim`, `run_finish_counted`, `runs_purge` (no tx; a single statement).
- `artifacts.rs`: `artifact_ingest`, `artifact_commit`, `artifact_update`, `artifact_pin`, `artifacts_hourly`.
- Others: `chat_upload_put`, `chat_uploads_take`, `connection_put`, `chat_mark_foreign` (no tx), `connector_add`, `connector_update`, `connector_account_put`, `media_delete`, `media_uses_add`, `plugin_access_put`, `plugin_access_forget_agent`, `profile_image_set`, `start_clone`, `fail_clone`, `session_file_make`, `write_fork`, `stored_skill_create`, `stored_skill_delete` (no tx), `workspace_agent_put`, `backup::on_restore`.

Multi-call closures are **not** atomic today either: `remove_member` then `member_count` then `delete_workspace` in `http/workspaces.rs` (after 641) are three lock takes.
A reader pool would not make that worse, but it is worth knowing that "one lock" never meant "one transaction" across calls.

## 2. Measurement

### Setup

- 5 users, one team workspace with all five, 5 projects each; 50,000 chats, 10,000 per user, `updated_ms` spread over a year.
- 19 events per chat (`session_created`, then three turns of `turn_started`, `user_message`, `tool_call_started`, `tool_call_finished`, `assistant_message`, `turn_completed`): 950,000 events, text from a 60-word vocabulary, all messages indexed in `chat_fts`.
- 5 % of chats mapped to the team workspace, 20 % in a project, 10 % "finished, unread", 182 of user 0's chats are routine runs.
- File: 1,649 MiB, WAL mode, `synchronous = NORMAL`, on tmpfs (so disk speed is out of the numbers; the owner's USB disk would make writes slower, not reads of a warm cache).
- Release build (the dev profile builds SQLite at `-O0`, which is not what runs). Build: 17.5 s.
- This machine: 16 threads, other builders compiling at the same time.
- Appends: one `tool_call_finished` to an existing chat through `Store::append`, 0.5 ms apart.
  The "contended" reader runs its query in a loop with 1 ms between runs (request traffic, not a hot loop; see the hot-loop note).

### Numbers

| Case | p50 | p99 | max |
|---|---|---|---|
| (a) `sessions_of(user)`, 10,000 rows | 13.7 ms | 15.2 ms | 15.4 ms |
| (a) `unread(user)` | 2.6 ms | 2.6 ms | 2.6 ms |
| (a) `project_links(None)`, 9,939 rows, all users | 2.3 ms | 2.3 ms | 2.3 ms |
| (a) `routine_marks`, 10,000 lookups | 69.8 ms | 70.3 ms | 71.2 ms |
| (a) **today's handler, store side (the four)** | **88.7 ms** | 88.9 ms | 89.2 ms |
| (a) `sessions()`, 50,000 rows | 51.9 ms | 52.1 ms | 54.3 ms |
| append, nothing else running | 18 µs | 29 µs | 91 µs |
| (b) append while `sessions_of` runs | 71 µs | 13.4 ms | 15.1 ms |
| (b) **append while today's handler runs** | 80 µs | **70.3 ms** | 71.2 ms |
| (c) **first page (50), owner only, one connection** | **56 µs** | 76 µs | 262 µs |
| (c) first page, owner + Personal workspace | 64 µs | 79 µs | 101 µs |
| (c) first page, owner + team workspace (5 % of the owner's chats) | 0.95 ms | 1.13 ms | 1.41 ms |
| (c) first page, owner + a workspace with none of them (walks all 10,000) | 8.2 ms | 8.3 ms | 8.3 ms |
| (c) page 101, by cursor | 56 µs | 65 µs | 85 µs |
| (c) the page's unread, links, marks (50 ids, three `IN` queries) | 85 µs | 109 µs | 220 µs |
| (c) **append while page + annotations run, one connection** | 59 µs | **314 µs** | 611 µs |
| (c) append while the walk-all workspace page runs, one connection | 224 µs | 8.4 ms | 9.9 ms |
| (B) append while today's full list runs on a separate read-only connection | 18 µs | 80 µs | 137 µs |
| owner-less (API token) first page, no index, top-N sort over 50,000 (sqlite3 CLI) | 3–4 ms | | |

The query plan of (c): `SEARCH s USING INDEX sessions_owner (owner_id=? AND updated_ms<?)`, then `USE TEMP B-TREE FOR LAST TERM OF ORDER BY` (the `session_id` tie-break, sorted only within equal `updated_ms`), with `LIMIT` stopping the walk.
The existing index is enough; a wider `(owner_id, updated_ms, session_id)` index buys nothing measurable.

**Reading these against the bar** ("first page under 20 ms and does not block a concurrent append"):

- (c) on today's one connection: the page takes 0.056 ms, page plus annotations 0.14 ms, and a concurrent append waits at most 0.6 ms (p99 0.3 ms).
  **The bar is met without a reader pool**, by a factor of about 140 on time.
- The pool (B) takes the remaining tail (0.3 ms) to the no-reader baseline (0.08 ms).
  That is the whole of what it buys for the list once the list is paged.
- The worst workspace case (a sparse workspace walked to its end) is 8 ms; it is bounded by the owner's own chat count, and slice 2 below bounds it further.

**Hot-loop note.** The first run had the reader loop with no pause (`bench-run1-hotloop.log`): with a 0.1 ms page query back to back, an append waited up to **1.8 s** (p50 303 ms).
`std::sync::Mutex` is not fair: a thread that releases and immediately re-locks usually wins.
A real server does not loop one query back to back, but it does have up to 512 blocking-pool threads queuing on the same mutex, and a burst of tabs refetching after a nudge is the same pattern for a moment.
That is an argument for a fair lock (or for keeping writers off the readers' lock), not for more connections; it is noted as an open question, not designed here.

### Memory (the 50 MB idle-RSS invariant)

Fresh process, the 1.6 GB database, a full list and an events scan on each connection:

| `cache_size` | RSS of 4 read-only connections, opened | after a scan on each | per connection |
|---|---|---|---|
| default (-2000, i.e. 2 MB) | +1.1 MB | +9.0 MB | **2.2 MB** |
| -512 (512 KB) | +1.1 MB | +2.8 MB | 0.7 MB |

The process holds three stores, so a pool of N readers costs 3 × N × 2.2 MB at the default cache: 26 MB for four readers, half the idle budget.
At -512 it is 8.5 MB for four, and every reader then reads more from the OS page cache.

## 3. Design

### A: scoped keyset queries on the one connection (chosen)

- `Store::sessions_page(q: &PageQuery) -> (Vec<SessionRow>, Option<Cursor>)`, with `owner: Owner` (`Owner::User(id)` from the caller's session, or `Owner::Everyone` for the API token only; an enum so that a forgotten `None` never means every account), `workspace: Option<&str>` (the effective workspace, as `EFFECTIVE_WS` computes it today), `top_level: bool` (`parent_id IS NULL AND incognito = 0`, what the sidebar shows), `hide_seen_runs: bool` (the routine filter, moved into SQL as a `NOT EXISTS` on `routine_runs`), `after: Option<Cursor>` (a typed `updated_ms` and session id), `limit`.
  `WHERE s.owner_id = ?1 AND (s.updated_ms < ?2 OR (s.updated_ms = ?2 AND s.session_id < ?3)) … ORDER BY s.updated_ms DESC, s.session_id DESC LIMIT ?n + 1`; the extra row tells whether there is a next page.
- `unread_of(owner, viewer, ids)`, `project_links_of(owner, ids)`, `routine_marks_of(owner, ids)` and `children_of(owner, parents)`, one statement each over the ids bound as one JSON array (`json_each`, so no variable limit), each joined to `sessions` on the owner, so they answer only for the owner's chats whatever ids a caller passes.
  They sit beside the old functions: the per-id `routine_marks`, `unread` and `project_links(None)` stay, and the master still calls them, until slice 2 switches the list over.
  `children_of` returns every descendant at any depth (sub-agents nest), up to 16 levels and 2,000 rows, see Children below.
- `sessions_in(owner, workspace)` (ids and the few fields the members and revocation paths use) for `delete_workspace`, member removal and `cancel_revoked`, one query for a set of users.
- `session_owners(ids)` for the two `chat_owners` and `board_json`; `project_chat_counts(owner, workspace)` for `own_chats`.
- The `'optimize'` merge leaves `delete_session` (and the artifact and media deletes): deleted terms stay as FTS5 tombstones, which search already skips, and the hourly maintenance runs a bounded `('merge', N)` instead.
  The privacy reason for the merge (secure delete of a deleted chat's words) holds as long as the hourly merge runs; how long a deleted chat's terms may linger in index pages is a question for the owner (below).
- No new connection, no new pragma, no new crate in `Cargo.lock`: harness-store now depends on `base64` (already a workspace dependency, for the cursor), and the workspace's rusqlite turns on its `trace` feature (no extra crate; tests count statements with it).
  One migration, an index (7, added in slice 1 for the page's children).

### B: A plus N read-only connections (rejected for now)

What it would take, answered so the next person does not start over:

- **Shape.** `Store { writer: Arc<Mutex<Connection>>, readers: Arc<ReaderPool> }`, `Store::read(|c| …)` and `Store::write(|c| …)`, readers opened `SQLITE_OPEN_READ_ONLY | SQLITE_OPEN_NO_MUTEX` with `PRAGMA query_only = ON` (the pattern `store/src/backup.rs:16` and `migrate.rs:237` already use).
- **Hand-rolled or a crate.** `deadpool-sqlite` 0.14.0 (2026-08-26, MIT OR Apache-2.0) depends on `rusqlite ^0.40`: it cannot link beside our 0.41.1 (two `libsqlite3-sys`), so it is out.
  `r2d2_sqlite` 0.36.0 (2026-11-09, MIT, `rusqlite ^0.41`) fits our version and is 16 days old, so it passes the two-week rule; 0.37.x needs rusqlite 0.42.
  It pulls `r2d2` 0.8.10, last released 2022-06-21, with `scheduled-thread-pool` (a background thread for idle reaping we would not use) and `parking_lot`.
  For a fixed set of 2 to 4 connections, a `Mutex<Vec<Connection>>` with a `Condvar` is about 60 lines and no new crate; it would be the choice. `cargo deny` was not run (not installed here).
- **Read-your-own-write.** In WAL, a read transaction sees every transaction committed before it began.
  Our writes commit before `lock()` is released, and each reader call starts a fresh implicit transaction, so a handler that writes and then reads sees its write, provided no reader keeps a statement open across calls (`prepare_cached` statements are reset when dropped).
  Reads that decide a write (any method in the list in §1) stay on the writer, inside the same lock.
- **`open_in_memory`.** A private in-memory database cannot be shared; tests would need `file:…?mode=memory&cache=shared` (shared-cache mode, which SQLite discourages and which changes locking) or a temp file.
  The simple rule: an in-memory store has no readers and `read` uses the writer.
  That also means most tests would never exercise the pool.
- **FTS5.** Reads from `chat_fts` on a reader are fine; `'optimize'` and `'merge'` are writes and stay on the writer.
- **Checkpoint starvation.** `wal_checkpoint(TRUNCATE)` cannot reset the WAL while any reader holds a snapshot; our checkpoint already never waits and retries hourly (`lib.rs:2036`).
  Readers of short queries make that rare; a slow search on a reader (170 ms) is exactly the case that makes it happen.
- **Backup and restore.** `backup` reads `master.db` from another process with `VACUUM INTO`; more readers in the server change nothing for it.
  `restore` runs with the server stopped (`RESTORING` marker); the copy before a migration happens in `Store::open` before any reader would be opened.
  More open file handles do not matter on Linux; on Windows they would block renames, which we do not support.
- **The node's store.** `node.db` is written by the session actors and read by few paths; a pool there buys nothing.
- **Idle memory.** 2.2 MB per reader per store at the default cache, 0.7 MB at `cache_size = -512` (measured above).
- **Async.** Nothing changes: readers run under the same `spawn_blocking`.

Why not now: after A, the list holds the lock for 0.14 ms; the pool's only gain on the bar is 0.3 ms of append tail.
It does not touch the slowest lock holders, which are a write (`delete_session`'s merge, 0.6 s) and a search that A can bound with a `LIMIT` and a statement-time budget.
It costs memory against a hard invariant and a second code path most tests cannot reach.

**Trigger to revisit B:** a measured read on a request path that still holds the lock for more than 5 ms after A (search is the likely one), at a size we care about, that cannot be bounded in SQL.
Then: one reader per store, hand-rolled, `cache_size = -512`, search only.

## 4. The API change

### Today

`GET /api/sessions?workspace=&routine_runs=` (`http/sessions.rs:102`) returns `{ "sessions": [row…] }`, every chat of the caller in the active workspace, newest first; each row has the `session_json` fields plus `project_id`, `unread`, `routine`, `background`.
The owner is the caller (`who.user_id`); the API token (no user) gets every chat, optionally in one workspace.
`api.sessions` (`web/src/api.ts:162`) has one caller, `Shell.tsx:229` (`refreshSessions`), which runs on mount, on every `sessions_changed` socket message (content-free, `ws.rs:619`), and after starting a chat.

### What the UI does with the whole list, and what breaks with one page

| Consumer | Uses | With only the first page |
|---|---|---|
| `ProjectNav.tsx:215` sidebar tree | top-level non-incognito rows, children nested by `parent_id`, rows grouped by `project_id`, a client-side "show more" cap per group | a project group shows only its loaded chats; a child whose parent is not loaded is dropped (as today); fine with "load more" at the bottom |
| `Shell.tsx:417` `listed` → `useFinishedSignal` (`lib/finished.ts`) | newly unread rows, numbered by place | fine: a chat that just finished moved to the top |
| `Shell.tsx:422` the backdrop's topic titles and banned titles | every title | shows the loaded titles only (cosmetic) |
| `Shell.tsx:460` `followRoute` `listed` | whether the open chat is in the list | an unloaded chat triggers the existing lookup (`api.session`), which keeps it in this workspace; works, one more request |
| `Shell.tsx:478` `openRow` | the open chat's row | falls back to the lookup row (exists) |
| `Shell.tsx:632` `subAgentRunning` | children of the open chat | needs the open chat's sub-agents: the page response carries every descendant of its own parents, and the open chat's children come from its lookup |
| `lib/drafts.ts:80` `pruneDrafts` | every chat not listed is asked about with `GET /api/sessions/{id}` | **breaks quietly**: every draft of a chat beyond page 1 costs one request on every refresh, i.e. on every turn start and end; run it on the first load only |
| incognito chats | listed today, hidden by the sidebar | leave the page (`top_level`); the open one comes from the lookup |
| search | server-side (`search.rs`) | unaffected; its no-words branch moves to `sessions_page` |
| routine run chats | hidden unless `routine_runs=1` | the filter moves into SQL so a page is full |

### New shape

`GET /api/sessions?workspace=W&limit=50&after=<cursor>` → `{ "sessions": [...], "children": [...], "next": "<cursor>" | null }`.

- **Cursor:** opaque to the client, unpadded base64url of 25 bytes (a version byte, `updated_ms` as a big-endian `i64`, the session id's 16 bytes): 34 characters; keyset, never `OFFSET`.
  Parsed strictly (`session_pages::Cursor::parse`: length cap, version, exact length, one spelling); anything else is 400.
  Slice 1 changed this from the text form `"1:" + updated_ms + ":" + session_id`, whose base64 is 70 to 79 characters and so never fits the 64-byte cap below.
  It is not signed: it only says where to start inside a `WHERE` that the server builds from the caller, so a forged cursor can only skip the caller's own chats.
- **Limit:** default 50, clamped to 1..=200 (a larger value is clamped, not refused; `limit=10^9` reads 200).
- **Owner:** always `who.user_id`; there is no owner parameter.
  The API token keeps its every-chat listing, paged by the same cursor without the owner term (3–4 ms full scan at 50,000; no index added).
- **Workspace:** as today, `ws::scope` checks membership before any query.
- **Children:** every sub-agent chat below a chat on this page, at any depth (sub-agents of sub-agents nest, and today's sidebar shows them all), in one recursive query, so nesting and `subAgentRunning` keep working.
  The owner is checked at every step, so a chain stops at a chat of somebody else; the depth is capped at 16 (the node has no limit of its own; the cap also ends a corrupt `parent_id` loop) and the rows at 2,000 (ten under each of 200 parents), past which the oldest are left out and the response says so (`truncated`).
  Deliberately unlike today: a child is returned whatever its own effective workspace, since it is shown nested under its parent (today the list drops a child whose own mapping differs from the parent's workspace, so it vanishes from both places).
  Measured in slice 1: without an index on `sessions.parent_id` this query walked all of the owner's chats, 5.5 ms at 10,000 (p99 append wait behind the page 5.9 ms).
  Migration 7 adds the partial index `sessions_parent ON sessions(parent_id) WHERE parent_id IS NOT NULL`, and the query names it (`INDEXED BY`): with an owner term SQLite still picks `sessions_owner` over a new index, and `INDEXED BY` makes an unusable index an error instead of a silent walk.
  With it (direct children only, before the review): 0.03 ms for a page's children, p99 append wait behind page, annotations and children 0.22 ms; the recursive form is measured in slice 1 below.

### Live updates

The socket keeps sending content-free `sessions_changed` (T58: it says nothing about whose chat changed).
The client refetches from the top with `limit = min(loaded, 200)` and replaces that many rows; rows loaded beyond 200 stay until scrolled again.
A chat outside the loaded pages that changes moves to the top (its `updated_ms` moved) and so appears; one deleted elsewhere disappears on the next refetch if it is within the first 200, else on scroll or a 404 when opened.

### Migration 7

Stage 0 said none was needed, and none is for the page: `updated_ms` exists and is maintained, `sessions_owner` exists, and the keyset query uses it.
The page's children were missed: migration 7 (slice 1) adds `sessions_parent` (see Children above).
It only creates an index, so it needs no backfill and a crash halfway leaves version 6 (the runner's transaction); `master.db`, `chat.db` and `node.db` all open through `DB_SCHEMA` and all get it.
Its cost is the start after the upgrade: like every version change, it first takes the full pre-migration copy of all three databases (`crates/cli/src/premigration.rs`: a byte copy, then `VACUUM INTO`), about twice their size written once; at the 1.6 GB of section 2 that is some 3 GB on the owner's USB disk, before the server opens.

## 5. Slices

Each slice is reviewable alone and starts with a test that fails on the old code.

1. **Store: page queries and set-shaped annotations.**
   `sessions_page`, `unread_of`, `project_links_of`, `routine_marks` as one `IN`; the `#[ignore]` 50,000-chat bench from this note, committed, with the plan's numbers as its assertions (run by hand and in a nightly job if there is one; 17.5 s in release, much longer in a debug build, too slow for every CI run).
   Failing first, and deterministic for CI: a test that runs `EXPLAIN QUERY PLAN` on the page query and asserts `SEARCH s USING INDEX sessions_owner`, no `SCAN s`; a test that 120 chats come back as 50 + 50 + 20 with no row twice and none lost across equal `updated_ms`; a test that `routine_marks` of 500 ids is one statement (count via `trace` or a profile hook).
   **This slice meets the plan's benchmark** (measured: 0.06 ms page, 0.3 ms p99 append wait).
   Built as `crates/store/src/session_pages.rs` (`sessions_page`, `unread_of`, `project_links_of`, `routine_marks_of`, `children_of`, `Cursor`; the bench is `session_pages::bench`).
   Measured in release on its own 50,000-chat database after the review fixes: first page 0.11 ms (p99 0.14 ms), page 101 0.11 ms, unread, links and marks of a page 0.08 ms (p99 0.10 ms), every descendant of the page (`children_of`, recursive) 0.05 ms (p99 0.07 ms; 5.5 ms before migration 7), page with every annotation p99 0.31 ms, p99 append wait behind it 0.25 ms (behind today's list 18 ms).
   The owner term in the three id-list annotations is a filter only (`+s.owner_id`): as a plain term SQLite walked all of the owner's chats through `sessions_owner`, 16 ms per page at 10,000 (the bench caught it; the plan test now refuses `sessions_owner` there).
2. **`/api/sessions` paged** (`http/sessions.rs`, no growth of `http.rs`).
   Tests: another user's cursor returns only the caller's chats; a cursor from workspace A used in B lists B's chats from that position and nothing of A's; `limit=1000000000` returns 200; a malformed cursor is 400; seen routine run chats never shorten a page; children of the page's parents arrive.
3. **UI: load more on scroll** (state in a hook, not in `Shell.tsx`), refetch policy, `pruneDrafts` on first load only, `subAgentRunning` from `children` and the lookup.
   Tests in `web/` for the merge of a refetch with loaded pages; an e2e script that creates 120 chats with the demo provider and scrolls.
   First-load bytes: the hook replaces existing code; measured with `pnpm size` before and after.
4. **The other whole-table and N+1 callers:** `board_json`, both `chat_owners`, `own_chats`, the search no-words branch and `chat_results`, `delete_workspace`, member removal, user delete, `cancel_revoked`, the members roster (`member_json`'s two calls per member).
   Test: a store wrapper or SQLite `trace` counts statements per request for a roster of 20 and a board of 50 cards, constant in N.
5. **The FTS merge out of the delete path**, into the hourly maintenance with a bounded `('merge', N)`.
   Test first: deleting a chat on a 20,000-message index takes under 50 ms (today 0.6 s at 50,000 chats), and the deleted chat's words are not found by search right after (tombstones) and are gone from the index pages after the hourly pass.
   Needs the owner's answer on how long deleted words may stay in index pages.

Not done from the row, and why:

- **The reader pool and `Store::read`/`Store::write`.** Measured not needed for the bar; costs RSS against an invariant; does not fix the slowest holders. Trigger written in §3.
- **A new index on `(owner_id, updated_ms)`.** It exists.

## 6. Abuse cases

| Case | What happens |
|---|---|
| A user pages another user's chats with a forged cursor | The owner term comes from the session, never from the request; the cursor only moves the start inside the caller's own rows. Test in slice 2. |
| A cursor from workspace A replayed in workspace B | The workspace term comes from `ws::scope` (membership checked); the cursor is a position only, so B's chats after that position are listed, which the caller may see anyway. Not bound to a workspace on purpose: binding would need a MAC and buys nothing. |
| `limit=10^9`, `limit=0`, negative | Clamped to 1..=200 by the server; parse failure of a non-number is 400 (axum's `Query`). |
| A malformed or huge cursor | Strict parse with a length cap (64 bytes before decoding); 400. |
| A reader used to write (`ATTACH`, a pragma, an `INSERT`) | No reader in A. If B is ever built: `SQLITE_OPEN_READ_ONLY` refuses writes to the main file and `query_only` refuses every write statement; `ATTACH` is not reachable because no SQL text comes from a request (all queries are constants with bound parameters). |
| A slow reader holding the WAL | No reader in A. Today's equivalent is any long statement under the one lock: search (170 ms) and the delete merge (0.6 s); slices 4 and 5 bound them. With B, a reader's snapshot would also hold back `wal_checkpoint(TRUNCATE)`; our checkpoint never waits and retries hourly, so the cost is WAL growth, not a stall. |
| A burst of refetches after a nudge (many tabs) | Each is a page (0.14 ms of lock); the mutex is unfair (hot-loop note), so a sustained burst can still delay appends; measured 0.6 ms max at a 1 ms request interval. |

## 7. Open questions for the owner

1. Deleted chats' words: today every delete merges the whole FTS index (0.6 s stall at 50,000 chats) so that their terms leave the index pages at once. Is "gone from search at once, gone from the index pages within the hour" acceptable?
2. The sidebar's project groups would show only loaded chats. Acceptable, or should each project group show its own newest N (one more query per expanded project)?
3. The mutex is unfair (an append waited 1.8 s behind a back-to-back reader in the hot-loop run). Worth a fair lock now (for example `parking_lot::Mutex` with fair unlocks, a new dependency), or wait for a real symptom?
4. Do we agree to drop the reader pool from 4.5 and leave the row's text with the trigger in §3?
