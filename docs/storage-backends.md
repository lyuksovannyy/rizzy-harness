# Storage backends

What the server keeps where, what can be swapped today, and the plan for
MariaDB. The Docker templates in [`docker/`](../docker/) are built on this.

## Today

| Data | Where | Swappable |
|---|---|---|
| Sessions, events, users, workspaces, projects, routines, artifacts, media index | `master.db` (SQLite, `harness-store`) | not yet: see the port below |
| Chat search | FTS5 tables in `master.db` | with the port |
| Blobs (files, images) | the encrypted blob store in the data directory | no |
| Memory facts, keyword recall | `memory.db` (SQLite + FTS5) | `mem0` replaces it whole |
| Memory vectors | `memory.db`, or **Qdrant** (`HARNESS_MEMORY_BACKEND=qdrant`) | yes |
| Refresh tokens, revoked logins | **KV**: `master.db` (`local://`, default), in-process (`memory://`) or **Redis** (`redis://`) | yes (`HARNESS_KV_URL`) |
| Access tokens | nowhere: signed JWTs, checked by signature | n/a |

`HARNESS_DATABASE_URL` accepts `sqlite://` (or nothing). A `mysql://` or
`mariadb://` URL is refused at start with a message pointing here, so a
template that names MariaDB never silently runs on something else.

## Qdrant (memory vectors)

Facts stay in `memory.db`: their text, status (proposed or confirmed),
owner and keyword index. Qdrant holds one vector per fact:

- Collection `<HARNESS_QDRANT_COLLECTION>_<vector size>` (default prefix
  `harness_memory`), cosine distance. A new embeddings model with another
  size gets a new collection instead of failing.
- Point id = the fact id; payload `owner` and `model`, both indexed and
  always filtered on. Every hit is checked against `memory.db`, so a stale
  point can never bring back a deleted or unconfirmed fact.
- Vectors are computed on recall for facts that have none for the current
  model (`fact_indexed` tracks which have one). Editing a fact drops its
  vector; deleting a fact deletes its points from every collection.
- If Qdrant or the embeddings endpoint is down, recall falls back to
  keywords and the chat goes on.

## KV (refresh tokens)

Three keys, all with a time to live:

- `auth.refresh.<jti>`: `live`, then `rotated` for 30 s after use.
- `auth.family.<id>`: present when that login is revoked (logout, or a
  refresh token replayed after its grace period).
- The default `local://` stores them in `master.db`'s `kv` table with the
  expiry in the value, swept as it writes. `memory://` keeps them in the
  process: a restart logs everyone out and forgets logouts, so it suits
  tests more than servers.

## MariaDB port (planned)

The store is about 6.5k lines of SQLite-specific SQL behind
`Store::call(|s| s.method(..))`, used from about 300 places in the master.
The plan:

1. **Query builder.** Move `harness-store` to SeaQuery (the dialect-neutral
   builder SeaORM is built on) over SQLx, so one set of queries serves
   SQLite and MariaDB. This also makes the store async: `Store::call` would
   become an `async fn`, and its callers barely change.
2. **Schema and migrations.** Today's `CREATE TABLE IF NOT EXISTS` becomes
   numbered migrations per dialect: `INSERT OR IGNORE`, `ON CONFLICT`, `IS`
   comparisons, `secure_delete` and FTS5 need MariaDB equivalents.
3. **Search.** FTS5 (`chat_fts`, `bm25`) has no portable form. Put search
   behind a trait: FTS5 for SQLite, `FULLTEXT ... MATCH AGAINST` (InnoDB)
   for MariaDB, with the ranking tests run on both.
4. **Transactions.** `append` (the replication rule) and the workspace
   membership checks rely on SQLite's single writer. On MariaDB they need
   `SELECT ... FOR UPDATE` or serializable transactions; the hostile-node
   and workspace tests must pass on both.
5. **Tests.** Run the store and master test suites against both backends
   in CI, with MariaDB in a service container.
6. **Migration tool.** `rizzy-harness db copy --to mysql://...` moves an
   existing `master.db`.

Until then the Standard and Full templates run MariaDB only behind the
`mariadb` compose profile, and the harness keeps using SQLite.
