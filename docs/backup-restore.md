# Backup and restore

FIX_PLAN 4.4 (review R43-3, `docs/node-update.md` §6.3).
PR A built `backup`, `backup --verify` and `restore` (`crates/cli/src/backup.rs`, `crates/store/src/backup.rs`, PLAN.md T103, README "Back up and restore"); PR B, the copy before each migration, is left.
The stage-0 spike's scripts and logs are in `.claude/handoff-2026-11-15/b-backup/` (git-ignored); the spike numbers below come from them.

## The row as written does not work

The row says the CLI "takes the storage `commit` lock".
That lock is a `tokio::sync::Mutex<()>` inside the server process (`crates/master/src/storage.rs:124`, field `commit` of `Storage`): a second process cannot take it.
The only cross-process lock is `<data>/lock` (`crates/cli/src/lock.rs`), which `serve`, `node run` and `node join` hold exclusively for their whole life, so a CLI beside a running server cannot take that one either.
The lock would not be enough even inside the process: `storage::unlink` (`storage.rs:650`), which every media, artifact and avatar delete calls after its row is gone, does not take `commit`, so a file can vanish between a snapshot that still has its row and the copy of the file.

What works instead, proved by the spike against the real binary: the CLI does the whole backup as a second process, with no lock of the server's, using three facts of our own code.

1. **A blob file never changes once it is committed.** It is written under `blobs/tmp`, renamed once to `blobs/<xx>/<id>.rzb` with a fresh id (`storage.rs` `commit`), and only ever deleted after its row (`blobs.rs` header: a renamed file does not even open).
   So a hard link taken while the file exists is a correct copy for ever.
2. **Every row that names a blob has a foreign key to `blobs`** (`crates/store/schema/6.sql`: `media_items`, `artifact_versions`, `chat_uploads`, `session_images`, `session_files`, `profile_images`, `blob_previews`, all `ON DELETE CASCADE`).
   So "every blob the snapshot needs" is exactly `SELECT blob_id FROM blobs` of the snapshot.
3. **A node's log is never behind the master's mirror of it.**
   The node appends to its own log first and sends afterwards; on reconnect the mirror is filled from the node log (observed in the spike below).
   So a mirror older than the node logs heals itself, and a mirror newer than a node log freezes that chat.

## Inventory of the data directory

Taken from the code and from a real `serve` on an empty temporary data directory (spike, `inventory.txt`).
`master.db`, `chat.db` and `node.db` are three files of one schema (`harness_store::DB_SCHEMA`, `user_version` 6 today); `memory.db` has its own (`harness_memory::DB_SCHEMA`, version 1).
All four are WAL (`Store::open`, `crates/store/src/lib.rs:795`; `SqliteMemory::open`, `crates/memory/src/sqlite.rs:119`), and the files are created 0644 inside the 0700 directory (observed).

| Path | Written by | Holds | In a backup |
|---|---|---|---|
| `master.db` | master (`main.rs:324`, `Durability::Normal`) | accounts, password hashes, login sessions (`ui_sessions`), workspaces, projects, routines, connections with sealed tokens, nodes and their pinned keys, join tokens, the mirror of every chat's events, blob rows with each blob's AES key, attachments' text, audit | yes, `VACUUM INTO`, first |
| `chat.db` | the master's chat runtime, node `master` (`main.rs:415`, `Full`) | the authoritative log of every plain chat | yes, after `master.db` |
| `node.db` | the embedded node `local` (`main.rs:882`, `Full`) | the authoritative log of every coding chat on this machine | yes, after `master.db` |
| `memory.db` | built-in memory (`providers.rs:484`) | remembered facts, their index | yes |
| `blobs/<xx>/<id>.rzb` | master (`storage.rs`) | Media, Artifacts, chat images, made documents, avatars: each sealed with its own key, which lives only in `master.db` | yes, the files `master.db`'s snapshot names |
| `blobs/tmp/` | uploads in flight | partial files | no (emptied at every start) |
| `config.toml`, `secrets.toml`, `node.toml` | the UI, the operator | settings; provider keys (0600); the node's policy | yes |
| `keys/secrets.key` | master (`sealed.rs`) | the key that seals forge and connector tokens in `master.db`: a plain file, not tied to the machine | yes, after the databases |
| `keys/master-cert.der`, `master-key.der` | `--node-listen` (`crates/link/src/tls.rs:35`) | the identity remote nodes pin | yes |
| `keys/ui-cert.pem` and its key | `--tls` | the UI's self-signed certificate browsers trust | yes |
| `keys/node.key`, `remote.toml` | `node join` (`main.rs:1115`, `1122`) | a remote node's identity and its master's address and fingerprint | yes (on a node) |
| `api-token` | `main.rs:1207` | the machine owner's root key | yes (decision 2) |
| `agents/`, `AGENTS.md`, `project-mcp.json` | the operator; repository MCP approvals | agent definitions, node instructions, approved repository MCP servers | yes |
| `claim-code` | first start | the one-time code of an unclaimed install | no (made again while no account exists) |
| `lock` | every `serve`/`node` start | holder hint | no |
| `*-wal`, `*-shm` | SQLite | the live WAL | no (`VACUUM INTO` folds it in) |
| `uploads-stage/`, `artifacts-stage/`, `projects-staging/`, `sandbox-tmp/` | node (`crates/node/src/lib.rs:713`, `artifacts.rs:52`, `policy.rs:509`, `crates/tools/src/sandbox.rs:2364`) | work in flight | no |
| `graphify/` | `main.rs:844` | graphify's state | no (rebuilt) |
| `checkpoints/<hash>.git` | node (`main.rs:888`, `docs/undo-and-stash.md`) | shadow repositories of agentic turns; they borrow objects from the user's repositories through `alternates` | no (see below) |

**Not in the data directory, so not in any backup:** the project folders and repositories the chats work in (the default new-project folder is `~/rizzy-projects`, observed); `.env` or `HARNESS_ENV_FILE` (it can hold provider keys) and every `HARNESS_*` variable; skill roots such as `~/.agents/skills`; an `[instructions] user_file` outside the data directory; a Qdrant memory index and any service behind a URL (transcriber, search); the browser's own state.
**On a remote node's machine, so not in the master's backup:** that node's `node.db` (the authoritative log of its chats; the master has only the mirror), its `node.toml`, `keys/node.key`, `remote.toml`, its checkpoints and its workspaces.
The same command run there with that node's `--data-dir` backs those up.

**Checkpoints are left out.** A shadow repository is useless without the repository it borrows from, and copying one to another machine leaves `alternates` pointing at paths that do not exist.
A restored server keeps every chat and every turn's diff text; what it loses is undo and rewind of turns made before the backup, which A.16 refuses when the content is gone (`docs/undo-and-stash.md`, "All or nothing"; `undo_after_a_restore_without_checkpoints_is_refused_in_words` checks the 409 and that the folder is untouched).

## Consistency while the server runs

A consistent backup here means: each database is one transaction's view; every blob row in the `master.db` copy has its file, byte for byte; no node log copy is behind the mirror copy; key files are at least as new as the databases that use them.
Files uploaded in the seconds between the `master.db` snapshot and the node-log snapshots can be named by a chat event and missing from the copy: the restored chat shows that one image or file as gone.
That is the price of rule 3, and the spike shows the other order is far worse.

**The algorithm** (one process, `rizzy-harness backup <dir>`):

0. Refuse first, before anything is opened: a data directory holding `RESTORING` (an interrupted restore; every command but `doctor` refuses it, `doctor` reports it); a database, or the `-wal`, `-shm` or `-journal` beside it, that is not a regular file (SQLite's own open would block on a FIFO).
1. Try the exclusive data-directory lock.
   Free: the server is stopped; hold the lock to the end (no server can start meanwhile) and everything below is trivially consistent.
   Held by `serve` or `node run`: online mode, same steps, no lock.
   **The source is only ever read.** Online, each database is read by a read-only connection (a reader writes only its read marks into the running server's `-shm` index, as every reader does).
   Offline, the stopped server's files (database, `-wal`, `-shm`) are first copied byte for byte into the staging folder and read from there: a test with the bundled SQLite showed that a read-only connection on a crashed database changes its `-shm` and, when the `-shm` is missing, makes one beside the source, so even read-only is not "untouched"; the private copy, opened read-write, also recovers a WAL whose `-shm` is gone.
   The offline backup leaves every source file byte-identical (`an_offline_backup_changes_nothing_and_keeps_the_wal`, `-wal` and `-shm` included; only the hint in `lock` changes) and keeps the rows only the WAL held.
   Online, a database that needs a writer's recovery is refused with a sentence that says what is true: the `-shm` cannot be written by this user, or the directory's lock holder (named from its hint: a `restore`, a `node join`) is not a server that keeps the index initialised; and whether the reader made a `-shm` (it may); a running server's files never need it.
   **Offline needs room:** the copies need the databases' size free on the data directory's disk plus a margin (64 MiB or a tenth), checked first; the refusal gives both numbers. An online backup makes no such copy.
   Either mode needs file locks on the data directory's file system (the staging lock); without them the backup is refused with a sentence.
   Every database's schema is checked before any is copied (`migrate::check_file`): one of a newer build is refused with the source untouched.
2. `VACUUM INTO` of `master.db`, into the destination.
3. Hard-link every file the snapshot's `blobs` names into `<data>/backup-staging/` (same file system, by construction; 0700; one backup at a time per data directory, by an flock on `<data>/backup.lock`, and a file system without locks is refused with a sentence; removed on every way out but SIGKILL, and a leftover is removed by the next backup; it is outside `blobs/`, so neither the start-up sweep nor the hourly one reads it, which a test checks after a SIGKILL).
   The links are only ever opened to read: a link shares the live file's inode, and nothing chmods or writes it.
   Where linking fails for another reason than a missing file (another mount, no links), the file is copied instead, which widens the window the retry covers.
   A file that is missing had its row deleted after the snapshot: discard the links, go back to 2, at most 5 times, then fail with nothing kept ("try again, or stop the server and back up then").
4. `VACUUM INTO` of `memory.db`, `chat.db`, `node.db` (after `master.db`, rule 3).
   Then the copies are settled as of the `master.db` copy (`settle_copies`; all at this build's schema version, else skipped with a printed line):
   - a chat whose row in the `master.db` copy has no owner but still has its create intent in `kv`, not expired at the snapshot's moment (`intent_expired` and the TTL are shared with the master in `harness_store::mirror_keys`; an expired intent is dropped and the chat stays ownerless, exactly as the live late claim leaves it), (the master stamps the owner and drops the intent in two statements; the drop now comes after the stamp, so a copy between them always finds the intent) is stamped from the intent, as the master's late claim does; an ownerless chat with an intent that names another node or nobody is left out;
   - a chat in a node log's copy the `master.db` copy does not know began after it: left out (the master writes a chat's owner only once the node has made it, so it would come back out of its owner's reach; PR A's test found this); a sub-agent's chat under a kept chat stays;
   - a chat of `master` (`chat.db`) or `local` (`node.db`) in the `master.db` copy that its log's copy no longer has was deleted meanwhile: removed from the `master.db` copy by the master's own forget path (`Store::forget_session`: rows, tombstone, the auto-model mark and the callback note); `memory.db` facts that name the chat as their source stay, as a live delete leaves them.
   The command says how many chats each rule changed.
5. Copy the plain files, `keys/` and `agents/` (after the databases).
   A symbolic link among these fixed items (or inside `keys/` or `agents/`, one level) is followed: its target must be a regular file of at most 16 MiB (or, for the two folders, a folder), judged before it is opened, opened non-blocking and checked again; the content goes in as a plain file and the manifest's `links` lists the item's name, never its target (a path outside the data directory tells where an operator keeps secrets).
   A dangling link, a link to a FIFO or folder, or a file over the cap fails the backup naming the item: a backup is never silently without its sealing key (the reviewer's case: a symlinked `keys/` used to be skipped and the restored server could not open its sealed tokens; `symlinked_configuration_is_followed_into_the_backup` restores a server that opens one).
   A database or `blobs/` behind a link is refused: SQLite keeps a database's `-wal` and `-shm` beside the target, not beside the link.
   A sub-folder of `agents/` and any unknown name at the top are listed in the manifest's `skipped` and said by `backup`, `--verify` (which then does not call the backup complete) and `restore`.
6. Copy the staged blobs into the destination (a real copy, streamed through one 1 MiB buffer, hashing as it goes); remove the staging folder; decrypt every copied file once and check it against the SHA-256 and size of its row.
7. Every file is created 0600 with `O_EXCL|O_NOFOLLOW` under `.<name>.partial`, synced, renamed, and every folder synced; `VACUUM INTO` writes into an empty 0600 file made that way (SQLite accepts an empty file and keeps its mode, checked by a test).
   Write `MANIFEST.json` last (temp name, fsync, rename, fsync the folder): a backup without it is incomplete and restore refuses it.
   The command prints the manifest's SHA-256 for the operator to note.
   A failure or SIGINT/SIGTERM removes what was written (the folder too when the backup made it).

**The spike** (Python driving the real binary over HTTP with the API token on a temporary data directory and HOME; tmpfs; scripts in the handoff folder):

- A writer uploaded PNG media, deleted media and prompted a chat in a loop (about 30 requests a second) while backups ran.
  Every backup that followed the order above restored cleanly: the restored server (the real binary, port 0, new HOME) served 102 of 102 and 773 of 773 media files with the SHA-256 of their rows, listed every chat, took a new prompt in each, and logged no warning.
  `PRAGMA integrity_check` ok and `foreign_key_check` empty on every copy; `user_version` kept by `VACUUM INTO` (6 and 1).
- **The order is load-bearing.** A backup that took `chat.db` one second before `master.db` restored into a server that logged "two writers sent different events under one seq (...) this session's mirror is frozen until it is deleted" and left that chat stuck (mirror at seq 1000, node log at 899).
  The reverse case is fine: 28 chats created between the `master.db` and `chat.db` snapshots were learned again by the restored mirror from the node log, all 51 listed and answering.
- **The race is real, and copying does not close it.** With the link step forced 3 s after the snapshot, the first attempt found 30 files deleted; the second attempt, with hard links (5 ms for 886 files), found none.
  The same run copying instead of linking (0.2 s per attempt) missed 21, 1, 1, 1 and 3 files and gave up: under a steady delete stream a copy never catches up, a link does.
- **Writers.** On tmpfs, HTTP latency did not move during the backups (max 65 ms during, 75 ms overall).
  On the USB disk (SQLite alone, same pragmas as `Durability::Full`), `VACUUM INTO` of a 274 MB database took 5.1 s; a one-row append, about 50 ms on that disk with `synchronous = FULL`, took up to 0.64 s while it ran (disk contention, not locking: WAL readers do not block writers), and one `wal_checkpoint(TRUNCATE)` waited 3.6 s for the backup's read to end.

**Retrying a missed truncate:** `master.db` in the hourly storage sweep; `chat.db` and `node.db` in each node runtime's existing one-minute loop (the background-command prune); a pending truncate leaves `<db>-truncate-pending` beside the file, which `doctor` reports.

**A finding outside this row:** `Store::checkpoint` (`crates/store/src/lib.rs:2005`) ignores the `busy` column of `wal_checkpoint(TRUNCATE)`, and it runs holding the store's only connection.
With any outside reader (this backup, `sqlite3`, a future `doctor` check) it blocks every `master.db` call for up to `busy_timeout` (5 s) and then silently leaves the WAL untruncated, which is the step the blob deletes rely on to drop a deleted file's key from disk (`storage.rs` `unlink` comment).
PR A fixed it in its own commit: the checkpoint runs with a zero busy timeout, returns whether it truncated, logs a missed truncate once and remembers it, and the next checkpoint or the hourly storage sweep (`retry_truncate`) makes it (`crates/store/tests/checkpoint.rs`, which failed on the old code: the checkpoint waited 5.0 s, and the WAL was left untruncated without a word).

**Why not the server.** A request to the running server (HTTP with the API token, or a socket in the data directory) could hold `commit`, make `unlink` take it, and so be exact without a retry.
It would add a route that writes every secret to disk, an address the CLI must discover (the bind address is written nowhere today), and code in the master for something a second process does with no server change.
The retry converged on the first or second attempt in every run; offline mode is exact.
Recommended: the CLI. If the lead or owner wants exactness under any delete rate, the server route is the follow-up, not the start.

## Restore

`rizzy-harness restore <backup> --data-dir <new>`:

- **After a failed restore** the directory is empty again (but `lock`): fix what the sentence names (or take another backup) and run `restore` again; after a killed one, follow the `RESTORING` sentence.
- **Target:** absent, or an empty directory (a mount point or a container volume is often exactly that, which is why nothing is renamed over it); "empty" means no entry at all, or only the `lock` file restore itself creates.
  Restore takes the target's exclusive lock (role `restore`), so a server cannot start on it meanwhile, and a running server's directory is refused twice over (not empty, locked).
- Empty means no entry at all but the `lock` file: a lone `lost+found` makes it not empty, simply, and the refusal says so.
- **First** it writes `<new>/RESTORING`; **last** it removes it.
  A failure (a damaged file, SIGINT, SIGTERM) empties the directory again (the lock file stays); a SIGKILL, a power cut or a failed cleanup leaves `RESTORING`.
  Every command that reads or writes a data directory (`serve`, `node join`, `node run`, `token`, `claim-code`, `graphify`, `backup`, `restore`) refuses a directory with that file, before touching anything, and says what to do: delete everything in it (`lock` and `backup.lock` too) or the directory itself, then restore again; `doctor` reports it.
  `the_advice_after_an_interrupted_restore_works` kills a restore halfway, follows the sentence word for word and restores again.
- Each file of the backup is judged before it is opened (a regular file, no link on the way or at it; a FIFO or device is refused without being opened, so nothing blocks and no device is touched), opened non-blocking, checked to be the file judged, then streamed: at most its declared size and one byte more are read (a file that grows is refused), and its SHA-256 is compared at the end; any mismatch names the file and the copy is undone.
- **Before any byte is copied** it reads `MANIFEST.json` and refuses: no manifest (incomplete), an unknown format, a database whose `user_version` is newer than this build's, any listed path outside the fixed shape (top-level names from a fixed list, `keys/<name>`, `agents/<name>`, `blobs/<xx>/<32 hex>.rzb`; no `..`, no absolute path, no empty segment), a listed file that is missing, of another size, or is not a regular file (a link, FIFO or device in the backup is refused, never followed or opened); the SHA-256 is checked while each file streams in (below).
  Files in the backup that the manifest does not list are not copied, and restore says how many it ignored.
- Every file is written 0600 and every folder 0700, whatever the backup's modes; the data directory itself through the existing `data_dir` (0700, owner checked).
- The restored `master.db` gets `DELETE FROM ui_sessions`: every login is dropped and everyone signs in again (cookies are only hashes there; the API token stays, see below).
- Then each database passes `integrity_check`, `foreign_key_check` and the migration preflight (`migrate::check_file`), and its version must be the manifest's; an older one is left for the first `serve` to migrate (fix plan 4.3), which a test checks with databases built at schema version 1.

**What a restored copy is:** the same server.
Same accounts and passwords, chats, files, routines, connections: sealed tokens open on any machine, because the key is `keys/secrets.key`, a file in the backup, not anything of the machine's (`sealed.rs` header; a `master.db` without its key opens nothing, and a key of another server is refused at start by `forge::open`).
Same identity: remote nodes pin `keys/master-cert.der` and find the master by the address in their `remote.toml`, and the master pins their keys in `nodes`, so they reconnect as soon as that address reaches the restored machine; their chats' logs refill the mirror (rule 3).

**What it is not:** logged in (dropped above); holding checkpoints (above; on the restored server an undo of an earlier turn answers 409 "this checkpoint is no longer kept (only the latest 100 changes are)", a sentence written for pruning; the node cannot cheaply tell a restored directory from a pruned checkpoint (the checkpoint store is made again for the folder at the first use), so the sentence stays and the README's restore section says what it means after a restore); holding a remote node's own state (above).
**Two copies at once are one identity twice.** A restored copy started beside a running original accepts the same nodes, the same API token and the same join tokens, and runs the same routines: a routine with `catch_up = once` fires a late run at the first start (`docs/routines.md`, "Missed runs"), and every routine runs on both, so anything it does outside (a pull request, a message) happens twice.
Restore prints this warning; `--pause-routines` sets every routine `enabled = 0` in the copy for a trial restore.
**A remote node restored from an older backup** is the case the master already detects: its replayed events differ from the mirror and those chats freeze until deleted (`AppendOutcome::Diverged`; research round 2 appendix, "A node restored from a backup").
The README must say: restore a node's data directory only when the node is gone, and expect its chats that moved on since the backup to stop.

## The backup as a secret

It holds every account's password hash, every chat, every file's key next to the file, every provider key and the token key.
- The destination must be absent or an empty directory owned by this user and not a symlink (`lstat`); it is created 0700 (`DirBuilder` mode, never visible under the umask); files are created 0600 with `create_new` inside it, so nothing can be planted to redirect a write.
  It must not be the data directory or lie inside or around it, judged by name and by the folders' device and inode along both ancestor chains (a bind mount of the data directory under another name is caught; not tested: no unprivileged mount here).
- Nothing in it is printed or logged: the command prints counts, sizes, time and the destination.
- No encryption in this change: nothing trivial and dependency-free does it well.
  The README says so and points to an encrypted volume or an encrypting backup tool (restic, borg, age) over the directory.
- The manifest's hashes catch truncation and bit rot, not an attacker: anyone who can write the backup can rewrite the manifest too, and then owns the server on restore.
  A backup is to be stored like the data directory itself.

## Format

A plain directory that mirrors the data directory (inspectable, `rsync`-able, readable by `sqlite3`), with `MANIFEST.json`: format 1, the harness version, the time, each database with its `user_version`, and every file with its path, size and SHA-256.
Not an archive: an archive needs a streaming writer and reader and its own traversal checks for nothing the directory lacks, and every tool an operator would use to encrypt or ship it works on a directory.
OpenClaw writes `.tar.gz` with a verify command and refuses a non-empty restore target (`research-repos/openclaw/src/commands/backup-restore.ts:66`); its SQLite snapshots run a read transaction and end in `VACUUM INTO` (`src/infra/sqlite-snapshot.ts`, 640 to 700).
We take the empty target, the snapshot that never writes its source and the verified manifest, and skip the archive.
OpenClaw notes that `VACUUM` may renumber implicit rowids; ours are only used for `ORDER BY rowid` (audit, routine triggers, clones) and the FTS tables keep their own ids, and `a_snapshot_keeps_implicit_rowids` checks the bundled SQLite keeps every `audit` rowid through `VACUUM INTO` after deletions.

## A copy before each migration (the row's second half)

Built in PR B (`crates/cli/src/premigration.rs`), reworked after its review (B1, B2, H1, M1 to M3).

**When a set is made, kept or refused** (`before_start`, at `serve`, `node run` and `node join`, holding the lock, after `check_schemas`, before any database is opened):
- A database counts when present with tables; it is *behind* when its version is above 0 and below the build's, or 0 with tables (4.3 adopts such a file as version 1 and migrates on; its `before` is recorded as 0).
- *Upgraded*: at least one of the set's databases present, and every present database at the set's `target` (a database the set lacks at the build's version); a directory holding only a set is not upgraded (`a_set_alone_is_no_first_run`). Then the set is only a way back: a database deleted on purpose is made new as always (`after_a_good_upgrade_a_deleted_database_is_made_again`).
- Refused first, unless upgraded: a database the set names is missing while an upgrade is under way; the sentence gives every way out (put it back; roll back; or delete `pre-migration/` and start once with `--migrate-without-copy`, the start being otherwise refused for mixed versions; `mid_upgrade_a_missing_database_is_refused_with_every_way_out` follows each of the three and the server starts). For `serve`, no `master.db` while stored files, `*.failed-upgrade-*` files or (unless upgraded) `pre-migration/` are there. No first-run claim code is made over such a directory.
- Nothing behind: nothing is done.
- **Kept:** only while a migration is under way: a set whose `target` is this build's versions, with every database at that set's `before` or `target` version (a database the set lacks only at the build's version, made by the failed start), and **at least one database no longer at its `before`**, and every file's hash checked (logged before and after, with the set's size). An older build refuses such a directory, so nothing can have changed the others since. The migration resumes; a retried or looping start never replaces the only good set (`failing_starts_keep_the_first_set`, `a_resumed_migration_keeps_the_set`).
- **Made fresh** whenever every database is still at one consistent pre-upgrade state, whether or not an older set with the same target exists: the older build may have written since (after a rollback), and keeping the old set would lose that at the next rollback (`a_second_rollback_keeps_what_the_older_build_wrote`). A start whose first migration fails leaves every database at `before` (the migration is one transaction), so the next start makes a fresh set from the same state: the same content.
- **Made:** only from one consistent state: `master.db`, `chat.db` and `node.db` (one schema, always migrated together) at one version, 0 with tables counting as 1; `memory.db` has its own schema and is alone in its family.
- **Refused:** mixed store versions with no matching set (no older build left that state together); `--migrate-without-copy` overrides, logged.
- A copy that cannot be written stops the start before anything is migrated (`an_unwritable_copy_stops_the_start`, a partial folder this user cannot empty; mutation-checked: ignoring the error makes it fail).
- Free space: the databases' bytes (with `-wal`, `-shm`, `-journal`) three times (the byte copy, the snapshot, and SQLite's temporary space for `VACUUM INTO`, counted at the database's size) plus 64 MiB or a tenth; numbers in MiB.
The copy is the offline backup's: a byte copy of each database with its `-wal`, `-shm` and `-journal` into `pre-migration.partial/work/`, `VACUUM INTO` from it, a `MANIFEST.json` (kind, build, time, `before`, `target`, sizes and SHA-256), the complete set swapped in by rename (`pre-migration.old` for the moment of the swap). A database a read-only reader may not read (a hot journal) has its version read from such a copy.
A start that made or kept a set and then fails, for any reason, prints the rollback advice when a database is no longer at its `before` version (`any_failure_after_a_migration_names_the_rollback`, a debug-only hook failing the start after `master.db` was opened).

**The rollback** (`restore --pre-migration`, under the lock): it reads and hashes the set, shows what it will do (the set's time, age and build, each database's version in the set and now, the stored files without a row in the set) and stops unless `--yes`. Then, staged: free space; `RESTORING` with a rollback's mark (every command but this one refuses it, with advice to run it again); each copy written beside its database as `<db>.rollback-partial` and synced; each live database with its `-wal`, `-shm` and `-journal` put aside as `<file>.failed-upgrade-<ms>` (0600) and its copy renamed into place; the stored files without a row in the set moved into `blobs.failed-upgrade-<ms>/` (kept, so the upgraded state can be put back); the versions checked; the mark removed last. The mark holds the rollback's time stamp: a rerun reuses it, never sets aside a database whose bytes are the set's copy (one an interrupted run placed), and lists everything set aside under that one stamp; the README's way back to the upgraded state (the server stopped; the older build's live `-wal`, `-shm` and `-journal` moved beside as `*.older-build-<time>` first, since an older WAL meeting the upgraded database makes it unreadable; the files, not the shard folders, which already exist) is run word for word, with a live WAL of the older build left beforehand, in `a_rollback_killed_at_any_step_can_be_run_again`. The set is never changed or removed by a rollback, so after an interruption at any stage the rollback run again finishes from it (`a_rollback_killed_at_any_step_can_be_run_again`, killed after each stage through a debug-only pause).
**The blob store cannot be wiped from an empty table** (its own commit): if `master.db` names no stored file while files lie in `blobs/`, neither the start's sweep nor the hourly one deletes them (a new server has neither; deleting every file removes the files with their rows); the log says so.
A stored file deleted after the upgrade is named by a restored row but gone: a Media download answers 500 ("cannot open a stored file" in the log), and an Artifact version is marked failed at the start (`fail_missing`).
`backup` leaves `pre-migration/`, the `.failed-upgrade-` files and folders and the `.rollback-partial` files out and says so; restore's path shape refuses them; `doctor` prints a line with the set's date, versions and size.

**Left over (known hazards, not changed here):** the blob guard covers only an empty table. An older `master.db` placed beside a newer blob folder (a hand-made rollback, a restore of one file) still loses, at the next start, the stored files it does not name: the start deletes files without a row. Moving such files aside at start instead of deleting them would change every server's start and is left for its own change. The guard's error is logged once per process run, and `doctor` prints a line while it holds.

**A master and its remote nodes are rolled back separately.** The link's hello carries only the protocol version (`harness_protocol::PROTOCOL_VERSION`, refused by `nodes::protocol_refusal` when it differs), never a schema version, so builds with different schemas talk as long as the protocol matches. A master rolled back while its nodes are not has a mirror older than their logs: the nodes resend what it lacks, and a chat begun on a node after the upgrade comes back without its owner's stamp (only the API token sees it), as with a restored backup. A node rolled back while its master is not has a log older than the master's mirror: its chats that moved on since the upgrade freeze ("two writers sent different events under one seq"). Roll a node back only together with its master, or not at all.

## Measured

USB disk of this machine, debug build, the server running, 61 media files (a 302 MB data directory): `backup` 311.5 MB in 19.0 s (peak RSS 53 MB, most of it the binary's own pages), `backup --verify` 8.8 s (47 MB), `restore` 13.4 s (48 MB).
A release build hashes faster; these are upper figures.
Copies stream through one 1 MiB buffer: a 2 GB file raises the peak RSS by far less than 64 MB (`a_large_file_streams_through_the_buffer`).

## PR B, a copy before each migration (built)

As above: the four databases into `<data>/pre-migration/` before a migrating start, one set kept, a start that cannot write it stops (decision 4), `restore --pre-migration` to roll back; tests: a version-5 fixture opens, the copy holds version 5, the rollback restores it and the old schema opens; a full disk stops the start before any migration.

**Later, not now:** a server route (exact under any delete rate), `--with-checkpoints`, encryption, scheduled backups (OpenClaw has them).

## Decisions

The session lead's (2026-11-25), each open to the owner's override:

1. **The CLI as a second process** runs the backup; a server route is a follow-up only if exactness under any delete rate is wanted.
2. **The API token stays in the backup**; the README says a backup is as sensitive as the server itself.
3. **Routines after a restore are as they were**, with the printed warning and `--pause-routines`.
4. **A migration whose copy cannot be written stops the start** (PR B).
5. **PR B comes right after PR A** (done: `feat/pre-migration-copy`).
