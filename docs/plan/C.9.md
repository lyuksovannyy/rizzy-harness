# C.9

## Decision

Staleness guard and match ladder for `edit`

## What the note settles

Per-session table by canonical path: SHA-256, `dev:ino:size:mtime_ns:ctime_ns`, ranges shown, a bounded snapshot on disk; checked in the per-path lock before matching; `edit` needs a read, `write` over a file needs a full read; a stale file is edited only when the matched span is byte-identical, else refused with a bounded diff; hashes in `read` events for the fold; batches match the original; the ladder stops at a one-to-one Unicode fold and reports the tier; what compaction does to "ranges shown". `08` §6.1, §6.2.

## Tests it commits to

T.2's list.

