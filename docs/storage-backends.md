# Storage backends

What the server keeps where, what can be swapped today, and the future
plan for MariaDB. The Docker templates in [`docker/`](../docker/) are built on this.

## Today

| Data | Where | Swappable |
|---|---|---|
| Sessions, events, users, workspaces, projects, routines, artifacts, media index | `master.db` (SQLite, `harness-store`) | not yet: see the port below |
| Chat search | FTS5 tables in `master.db` | with the port |
| Blobs (files, images) | the encrypted blob store in the data directory | no |
| Memory facts, keyword recall | `memory.db` (SQLite + FTS5) | `mem0` replaces it whole |
| Memory vectors | `memory.db`, or **Qdrant** (`HARNESS_MEMORY_BACKEND=qdrant`) | yes |
| Browser logins | `ui_sessions` in `master.db`: the SHA-256 of the session cookie, the user, and an expiry that slides with use | with the port |

There is no database or cache setting: SQLite in the data directory is the
only database, and one server process owns it.

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

## MariaDB port (future plan: Phase 6 of FIX_PLAN.md)

Not scheduled. The trigger is a deployment that needs more than one server
process or a managed database (see Phase 6 of [`FIX_PLAN.md`](../FIX_PLAN.md)).
Redis returns with it and not before: with one process a login is one
indexed lookup in SQLite, and a shared cache only earns its place when
several processes must see the same logins. Until then no template starts
either service.

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
