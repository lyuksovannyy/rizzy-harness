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

1. Try the exclusive data-directory lock.
   Free: the server is stopped; hold the lock to the end (no server can start meanwhile) and everything below is trivially consistent.
   Held by `serve` or `node run`: online mode, same steps, no lock.
2. `VACUUM INTO` of `master.db` through a read-only connection, into the destination.
3. Hard-link every file the snapshot's `blobs` names into `<data>/backup-staging/` (same file system, by construction; 0700; one backup at a time per data directory, by an flock on `<data>/backup.lock`; removed on every way out but SIGKILL, and a leftover is removed by the next backup; it is outside `blobs/`, so neither the start-up sweep nor the hourly one reads it, which a test checks after a SIGKILL).
   The links are only ever opened to read: a link shares the live file's inode, and nothing chmods or writes it.
   Where linking fails for another reason than a missing file (another mount, no links), the file is copied instead, which widens the window the retry covers.
   A file that is missing had its row deleted after the snapshot: discard the links, go back to 2, at most 5 times, then fail with nothing kept ("try again, or stop the server and back up then").
4. `VACUUM INTO` of `memory.db`, `chat.db`, `node.db` (after `master.db`, rule 3).
   Then, in the copies of `chat.db` and `node.db`, every chat the `master.db` copy does not know is removed (`drop_unknown_chats`): it began after `master.db` was copied, and the master writes a chat's owner, workspace and project into `master.db` only once the node has made it, so it would come back without an owner, out of its owner's reach (PR A's test found this; the spike, with API-token chats that have no owner, could not).
   A sub-agent's chat under a kept chat stays (it inherits its owner).
   The command says how many it left out.
   This needs the copy at this build's schema version (it is opened as a store); otherwise it is skipped with a line saying so.
5. Copy the plain files and `keys/` (after the databases).
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

**A finding outside this row:** `Store::checkpoint` (`crates/store/src/lib.rs:2005`) ignores the `busy` column of `wal_checkpoint(TRUNCATE)`, and it runs holding the store's only connection.
With any outside reader (this backup, `sqlite3`, a future `doctor` check) it blocks every `master.db` call for up to `busy_timeout` (5 s) and then silently leaves the WAL untruncated, which is the step the blob deletes rely on to drop a deleted file's key from disk (`storage.rs` `unlink` comment).
PR A fixed it in its own commit: the checkpoint runs with a zero busy timeout, returns whether it truncated, logs a missed truncate once and remembers it, and the next checkpoint or the hourly storage sweep (`retry_truncate`) makes it (`crates/store/tests/checkpoint.rs`, which failed on the old code: the checkpoint waited 5.0 s, and the WAL was left untruncated without a word).

**Why not the server.** A request to the running server (HTTP with the API token, or a socket in the data directory) could hold `commit`, make `unlink` take it, and so be exact without a retry.
It would add a route that writes every secret to disk, an address the CLI must discover (the bind address is written nowhere today), and code in the master for something a second process does with no server change.
The retry converged on the first or second attempt in every run; offline mode is exact.
Recommended: the CLI. If the lead or owner wants exactness under any delete rate, the server route is the follow-up, not the start.

## Restore

`rizzy-harness restore <backup> --data-dir <new>`:

- **Target:** absent, or an empty directory (a mount point or a container volume is often exactly that, which is why nothing is renamed over it); "empty" means no entry at all, or only the `lock` file restore itself creates.
  Restore takes the target's exclusive lock (role `restore`), so a server cannot start on it meanwhile, and a running server's directory is refused twice over (not empty, locked).
- Empty means no entry at all but the `lock` file: a lone `lost+found` makes it not empty, simply, and the refusal says so.
- **First** it writes `<new>/RESTORING`; **last** it removes it.
  A failure empties the directory again (the lock file stays), and only if that fails does `RESTORING` stay.
  `serve` and `node run` refuse a directory with that file ("an interrupted restore: empty the directory and run restore again"), so a half-restored directory never starts.
- **Before any byte is copied** it reads `MANIFEST.json` and refuses: no manifest (incomplete), an unknown format, a database whose `user_version` is newer than this build's, any listed path outside the fixed shape (top-level names from a fixed list, `keys/<name>`, `agents/<relative>`, `blobs/<xx>/<32 hex>.rzb`; no `..`, no absolute path, no empty segment), a listed file that is missing, of another size or another SHA-256, or is not a regular file (`O_NOFOLLOW`; a symlink in the backup is refused, not followed).
  Files in the backup that the manifest does not list are not copied, and restore says how many it ignored.
- Every file is written 0600 and every folder 0700, whatever the backup's modes; the data directory itself through the existing `data_dir` (0700, owner checked).
- The restored `master.db` gets `DELETE FROM ui_sessions`: every login is dropped and everyone signs in again (cookies are only hashes there; the API token stays, see below).
- Then each database passes `integrity_check`, `foreign_key_check` and the migration preflight (`migrate::check_file`), and its version must be the manifest's; an older one is left for the first `serve` to migrate (fix plan 4.3), which a test checks with databases built at schema version 1.

**What a restored copy is:** the same server.
Same accounts and passwords, chats, files, routines, connections: sealed tokens open on any machine, because the key is `keys/secrets.key`, a file in the backup, not anything of the machine's (`sealed.rs` header; a `master.db` without its key opens nothing, and a key of another server is refused at start by `forge::open`).
Same identity: remote nodes pin `keys/master-cert.der` and find the master by the address in their `remote.toml`, and the master pins their keys in `nodes`, so they reconnect as soon as that address reaches the restored machine; their chats' logs refill the mirror (rule 3).

**What it is not:** logged in (dropped above); holding checkpoints (above; on the restored server an undo of an earlier turn answers 409 "this checkpoint is no longer kept (only the latest 100 changes are)", a sentence written for pruning: a follow-up should name the restore); holding a remote node's own state (above).
**Two copies at once are one identity twice.** A restored copy started beside a running original accepts the same nodes, the same API token and the same join tokens, and runs the same routines: a routine with `catch_up = once` fires a late run at the first start (`docs/routines.md`, "Missed runs"), and every routine runs on both, so anything it does outside (a pull request, a message) happens twice.
Restore prints this warning; `--pause-routines` sets every routine `enabled = 0` in the copy for a trial restore.
**A remote node restored from an older backup** is the case the master already detects: its replayed events differ from the mirror and those chats freeze until deleted (`AppendOutcome::Diverged`; research round 2 appendix, "A node restored from a backup").
The README must say: restore a node's data directory only when the node is gone, and expect its chats that moved on since the backup to stop.

## The backup as a secret

It holds every account's password hash, every chat, every file's key next to the file, every provider key and the token key.
- The destination must be absent or an empty directory owned by this user and not a symlink (`lstat`); it is created 0700 (`DirBuilder` mode, never visible under the umask); files are created 0600 with `create_new` inside it, so nothing can be planted to redirect a write.
- Nothing in it is printed or logged: the command prints counts, sizes, time and the destination.
- No encryption in this change: nothing trivial and dependency-free does it well.
  The README says so and points to an encrypted volume or an encrypting backup tool (restic, borg, age) over the directory.
- The manifest's hashes catch truncation and bit rot, not an attacker: anyone who can write the backup can rewrite the manifest too, and then owns the server on restore.
  A backup is to be stored like the data directory itself.

## Format

A plain directory that mirrors the data directory (inspectable, `rsync`-able, readable by `sqlite3`), with `MANIFEST.json`: format 1, the harness version, the time, each database with its `user_version`, and every file with its path, size and SHA-256.
Not an archive: an archive needs a streaming writer and reader and its own traversal checks for nothing the directory lacks, and every tool an operator would use to encrypt or ship it works on a directory.
OpenClaw writes `.tar.gz` with a verify command and refuses a non-empty restore target (`research-repos/openclaw/src/commands/backup-restore.ts:66`); its SQLite snapshots run a read transaction and end in `VACUUM INTO` (`src/infra/sqlite-snapshot.ts`, 640 to 700).
We take the empty target, the read-only snapshot and the verified manifest, and skip the archive.
OpenClaw notes that `VACUUM` may renumber implicit rowids; ours are only used for `ORDER BY rowid` (audit, routine triggers, clones) and the FTS tables keep their own ids, and `a_snapshot_keeps_implicit_rowids` checks the bundled SQLite keeps every `audit` rowid through `VACUUM INTO` after deletions.

## A copy before each migration (the row's second half)

The read-only preflight (`check_schemas`, `main.rs:204`) already reads every database's version before any is opened, under the exclusive lock.
When any of them is older than this build, it first writes the four databases with `VACUUM INTO` to `<data>/pre-migration/` with a manifest (from-versions, build), replacing the previous set: one set kept, removed only when the next migrating start replaces it, sized like the databases (blobs are not copied: a migration does not touch them).
If the copy cannot be made (disk full) the start stops before anything is migrated.
Rolling back: stop the new build, run `rizzy-harness restore --pre-migration` (puts the four files back, removing the migrated files' `-wal` and `-shm`, which must never meet an older file), start the old build.
It is small (the preflight already has the versions; the snapshot code is PR A's), and it gives `docs/node-update.md` §6.3 its second option.
It ships as PR B, after PR A, because its restore reuses PR A's checks.

## Measured

USB disk of this machine, debug build, the server running, 61 media files (a 302 MB data directory): `backup` 311.5 MB in 19.0 s (peak RSS 53 MB, most of it the binary's own pages), `backup --verify` 8.8 s (47 MB), `restore` 13.4 s (48 MB).
A release build hashes faster; these are upper figures.
Copies stream through one 1 MiB buffer: a 2 GB file raises the peak RSS by far less than 64 MB (`a_large_file_streams_through_the_buffer`).

## PR B, a copy before each migration (left)

As above: the four databases into `<data>/pre-migration/` before a migrating start, one set kept, a start that cannot write it stops (decision 4), `restore --pre-migration` to roll back; tests: a version-5 fixture opens, the copy holds version 5, the rollback restores it and the old schema opens; a full disk stops the start before any migration.

**Later, not now:** a server route (exact under any delete rate), `--with-checkpoints`, encryption, scheduled backups (OpenClaw has them).

## Decisions

The session lead's (2026-11-25), each open to the owner's override:

1. **The CLI as a second process** runs the backup; a server route is a follow-up only if exactness under any delete rate is wanted.
2. **The API token stays in the backup**; the README says a backup is as sensitive as the server itself.
3. **Routines after a restore are as they were**, with the printed warning and `--pause-routines`.
4. **A migration whose copy cannot be written stops the start** (PR B).
5. **PR B comes right after PR A.**
