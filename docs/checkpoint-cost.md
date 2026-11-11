# What a turn's checkpoints cost (experiment B.3)

Every agentic turn takes two snapshots of the folder ("before", "after") into
the node's shadow repository, then lists what changed and prunes old
checkpoints. This note measures what that costs as the folder grows. The
benchmark is `checkpoint_cost` in `crates/node/src/git_cost_bench.rs`
(ignored; it lives inside the crate because the `git` module is private):

```bash
B3_DIR=$(mktemp -d -p /some/real/disk) B3_SIZES=1000,10000,100000 \
  cargo test --release -p harness-node --lib checkpoint_cost -- --ignored --nocapture
```

`B3_SIZES` (file counts), `B3_KINDS` (`plain`, `repo`), `B3_RUNS` (default 5)
and `B3_RUNS_BIG` (for 250,000 files and more) control the run. It removes
what it created, also on a panic (not when the process is killed).

## Method

- Trees are generated from the path: about 50 files per directory, depth 3 to
  6, 1 to 8 KiB each, five files of 1 MiB, plus a `.gitignore` excluding
  `ignored_deps/`, which holds as many 1 KiB files again as the tree.
- Two kinds of folder: **repo** (a git repository, everything committed; the
  shadow borrows its objects through `objects/info/alternates`) and **plain**
  (no repository; the shadow stores everything).
- The functions are the session's own (`snapshot_full`, `changed`, `prune`,
  `restore_diff`, `restore_plan`, `secure_blobs`, `restore_paths`), called in
  the session's order with `checkpoints_keep` = 10. Each cell is the median
  of 5 runs after one warm-up run, with min and max in brackets (a few cells
  use 3 runs: marked in the text). Times are milliseconds.
- "Turn" rows are measured in a session that already holds 10 checkpoints, so
  every `prune` drops one and runs `git gc` (steady state). The first turns of
  a session do not.
- The step breakdown runs the git commands of `stage_without_nested` one by
  one with a timer (the same private helpers, no `GIT_TRACE2_PERF`).
- Restores undo an edit of N spread-out files, and the benchmark checks the
  folder's bytes afterwards.

## Machine

| | |
|---|---|
| CPU | AMD Ryzen 7 9800X3D, 8 cores / 16 threads |
| RAM | 31.2 GiB (page cache about 11 GiB during the runs; free memory often under 500 MiB) |
| Disk | **rotational**, USB-attached Seagate ST500LT012 (2.5", 5400 rpm class), `/sys/block/sda/queue/rotational` = 1 |
| Filesystem | ext4 (`/dev/sda1`), same device as the repository; not a tmpfs |
| Temporary directory | `mktemp -d -p /run/media/lebron/PROJ` (a directory made per run, removed after) |
| git | 2.56.0, Linux 7.2.14 |
| Load | **high and not mine**: another process (a game) used about 4 cores throughout. `/proc/loadavg` (1 min) was 10.98 at the start and 10.23 at the end of the run behind the tables, between 11 and 14 at the per-kind checks. The task says to rerun above about 4; the load never came down, so the tables are from a busy machine. Run 2 (same code, load 5 to 15) agrees with run 3 within the spread shown here. |

This is a slow, shared disk and a loaded CPU, so these numbers are probably
worse than a developer laptop with an NVMe drive. They are not a lower bound.

**Cold cache is not available.** `/proc/sys/vm/drop_caches` is root-only
(mode `--w-------`, I run as uid 1000) and I did not try to get root. Every
table below is warm: the tree was created or read just before. The line
"first run in a fresh process" is the warm-up of each series, which is not
reported. Two accidents did show cold behaviour, see "What was not
measured".

## Results (warm)

Median [min-max] in ms. 1,000 / 10,000 / 100,000 files, each with the same
number of ignored files again.

### Snapshots

| | plain 1k | plain 10k | plain 100k | repo 1k | repo 10k | repo 100k |
|---|---|---|---|---|---|---|
| first snapshot (empty shadow, includes creating it) | 121 [117-125] | 534 [531-544] | 13,091 [5,999-16,879] | 42 [41-43] | 187 [187-211] | 1,878 [1,650-2,267] |
| first, ignored dir absent (3 runs) | 101 | 532 | 5,546 | 42 | 185 | 1,405 |
| first, repo not borrowing (3 runs) | - | - | - | 99 | 539 | 5,736 [5,518-6,612] |
| repeat, nothing changed | 11 | 26 | 201 [169-484] | 11 | 25 | 166 [160-272] |
| repeat, ignored dir absent | 11 | 24 | 171 | 11 | 24 | 169 |
| after 1 file edited | 12 | 30 | 353 [335-512] | 13 | 30 | 360 [293-428] |
| after 10 files edited | 13 | 31 | 310 [268-405] | 16 | 33 | 305 [274-425] |
| after 1,000 files edited | 112 | 106 | 326 [313-559] | 108 | 106 | 368 [358-474] |
| after 1,000 new files | 65 [50-108] | 108 | 521 [298-540] | 96 | 85 | 471 [332-658] |
| index rebuild from empty (after a failed `write-tree`) | 29 | 144 | 1,480 [1,436-1,496] | 35 | 153 | 1,484 [1,470-1,561] |

At 1,000 files "1,000 edited" is every file. The plain 100,000 first
snapshot is the noisiest number here: its minimum (6.0 s) is close to the
timed single run (5.8 s) and to the hash step alone (4.0 s, below), and its
median is twice that because writing 100,000 loose objects to this disk
stalls. Run 2 gave 8.2 s [5.0-15.5]. The unchanged repeat (166-201 ms) is
about as noisy as the edited ones: editing 1 file does not cost more than
editing 10 within the spread, and both cost about 150 ms more than editing
nothing at 100,000 files (see "which step dominates").

### A whole turn, as the session does it

`repo_for` + before + (10 files edited) + after + `changed` + `prune`,
session already holding 10 checkpoints. Parts are medians.

| | plain 1k | plain 10k | plain 100k | repo 1k | repo 10k | repo 100k |
|---|---|---|---|---|---|---|
| turn, 10 files changed | 371 [350-415] | 669 [546-716] | 3,978 [3,854-5,785] | 295 [293-328] | 384 [360-438] | 1,094 [1,059-1,149] |
| of which before / after | 13 / 13 | 34 / 32 | 296 / 288 | 14 / 15 | 30 / 32 | 298 / 300 |
| of which `changed` | 3 | 4 | 4 | 3 | 4 | 4 |
| of which `prune` | 338 | 597 | 3,310 | 259 | 313 | 404 |
| turn, read only (nothing changed) | 324 [313-368] | 635 [557-701] | 3,664 [3,409-4,919] | 291 [246-335] | 352 [335-369] | 757 [715-802] |
| of which before / after / `prune` | 10 / 10 / 297 | 25 / 25 / 577 | 160 / 184 / 3,322 | 11 / 11 / 262 | 25 / 24 / 298 | 165 / 185 / 400 |
| first turn of a session (10 changed; a first snapshot, nothing to prune) | 114 | 573 | 6,064 | 60 | 194 | 1,797 |

`prune` is `git gc --prune=now` whenever a checkpoint group is dropped.
Without it (a turn in a session younger than 10 checkpoints, or one that
drops nothing), a turn is before + after + `changed`: about 29 / 70 / 590 ms
(plain) and 32 / 66 / 602 ms (repo) at 1k / 10k / 100k.

### Restore (the session's full path)

`exists`, `restore_diff`, pre-restore snapshot, `restore_plan`,
`secure_blobs`, `restore_paths`, snapshot after, `prune`.

| | plain 1k | plain 10k | plain 100k | repo 1k | repo 10k | repo 100k |
|---|---|---|---|---|---|---|
| restore 1 file | 389 [300-666] | 686 [625-760] | 4,201 [3,516-6,749] | 510 [445-692] | 632 [598-653] | 1,440 [1,248-1,520] |
| restore 10 files | 358 [334-378] | 678 [575-731] | 4,123 [3,533-7,876] | 520 [488-609] | 651 [597-1,016] | 1,324 [1,267-1,414] |
| restore 1,000 files | 1,084 [887-1,147] | 1,105 [1,008-1,170] | 5,040 [4,176-5,321] | 1,350 [1,205-1,443] | 1,161 [1,009-1,330] | 2,115 [2,037-2,363] |

For 10 files, medians of the parts: plain 100k: save-now 359, plan 5,
secure 1, write 3, after 426, `prune` 3,311; repo 100k: save-now 304, plan 5,
**secure 277**, write 4, after 322, `prune` 388. `secure_blobs` copies the
needed blobs out of the user's repository (`pack-objects`) and costs
170 to 400 ms even for one file when borrowing; in a plain folder it is
1 ms. `restore_diff` and `restore_paths` are never above 40 ms.

### Disk (the shadow repository)

Allocated size (`st_blocks`), fresh shadow. "After 10 turns" = the first
snapshot, then ten turns that each change 10 files, pruning as the session
does.

| folder | files | after first snapshot | after 10 turns of 10 files |
|---|---|---|---|
| repo, **borrowing** | 1k | 0.2 MiB (25 files) | 0.5 MiB |
| repo, **not borrowing** (same snapshots, `alternates` removed) | 1k | 8.6 MiB (1,104 files) | 4.1 MiB |
| plain | 1k | 8.6 MiB (1,104 files) | 4.1 MiB |
| repo, borrowing | 10k | 1.1 MiB | 1.3 MiB |
| repo, not borrowing | 10k | 51.1 MiB (10,598 files) | 21.8 MiB |
| plain | 10k | 51.1 MiB | 21.8 MiB |
| repo, borrowing | 100k | 9.7 MiB (25 files) | 9.9 MiB |
| repo, not borrowing | 100k | 478 MiB (103,982 files) | 198 MiB |
| plain | 100k | 478 MiB | 198 MiB |

The tree itself is 278 MiB apparent at 100k (text that compresses well; real
source and binaries will differ). After the first `gc` the loose objects are
one pack, which is why "after 10 turns" is smaller than "after first".

### Step breakdown at 100,000 files (median of 5, ms)

Each git command run separately with a timer, same arguments as the session.

| step | plain: first | plain: 10 edited | repo: first | repo: 10 edited |
|---|---|---|---|---|
| `ls-files --stage` (twice) | 1 + 16 | 21 + 20 | 1 + 21 | 20 + 21 |
| `ls-files -o` (listing) | 52 | 56 | 58 | 56 |
| `add -u` | 2 | 71 | 2 | 61 |
| `update-index --add` (hash) | **4,043** | 0 | **1,147** | 0 |
| `write-tree` | 288 | 112 | 201 | 102 |
| `commit-tree` + `update-ref` | 4 | 5 | 5 | 5 |
| Rust-side parsing and splitting | 3 | 4 | 4 | 4 |
| copy of the shadow index | 0 | 2 | 0 | 2 |

The breakdown of the unchanged repeat sums to about 285 ms at 100,000 files
(`write-tree` 139, `add -u` 63, `ls-files -o` 42), more than the 160 to 200 ms
measured for the whole snapshot. I did not find why (the breakdown does not
save the index the way the real path does, so it may be re-validating more of
the tree each time). Read the per-step numbers as an upper bound and the whole
snapshot number as the truth.

## What the numbers mean

**When does a turn's checkpoint pass 100 ms, 1 s and 5 s?** Taking before +
after + `changed` only (what a turn costs while the session holds fewer than 10
checkpoints): about 30 ms at 1,000 files, 70 ms at 10,000, 600 ms at 100,000,
the same for plain and repo folders. Linear between the measured points that is
100 ms at about 15,000 files and 1 s at about 170,000. I did not measure the
5 s point; extrapolating the same line gives roughly 850,000 files, but see the
timeout answer: a plain folder cannot be reached at that size. In steady
state, with `prune` running `git gc` on every turn, the cost starts above
100 ms at 1,000 files (260 to 370 ms) and rises with folder size: it passes
1 s between 10,000 and 100,000 files for a plain folder (0.67 s at 10,000,
4.0 s at 100,000, so 5 s at roughly 120,000 to 150,000 by the same line) and at
about 100,000 for a repository (1.09 s). Most of that is `prune`, not the snapshots.

**Is a read-only turn worth skipping?** At 1,000 files, no: the two unchanged
snapshots are 20 ms. At 10,000 files they are about 50 ms. At 100,000 files they
are 340 to 350 ms, and the group they leave behind then triggers a `gc` that
costs 0.3 to 3.3 s, so a read-only turn costs 0.7 s (repo) to 3.7 s (plain)
there, to record that nothing happened. Skipping is therefore worth it for big
folders, and mostly through the `prune`. To skip safely the node would have to
know that nothing could have written to the folder during the turn: every tool
the turn called is known not to write (not the shell, not anything that can
start a process, no background command from an earlier turn still running), and
no one else changed the folder. A shell command can write anywhere, so "no
shell command ran" is the condition, not "the model said it only read". If it
is not known, the "after" snapshot has to be taken: it is what an undo restores
and what `changed` compares. The measured unchanged snapshot (10 to 200 ms) is
cheap compared with a wrong "nothing changed". I did not measure the cost of
checking for outside changes (for example with file-system change
notifications); the numbers do not say whether that check would be cheaper than
a snapshot.

**Does the 60-second snapshot timeout get close?** `SNAPSHOT_TIMEOUT` (60 s,
`git.rs`) is not near any warm measurement up to 100,000 files: the slowest
snapshot there was 16.9 s (a plain first snapshot, 28% of the limit) and every
snapshot of an already-snapshotted folder was under 1 s. It is passed at larger
sizes and on a busy disk, and I saw it fail: see the defect below. On a plain
250,000-file folder the first snapshot hit the 60 s timeout, and when I timed
the same steps without it, the single `update-index --add` step passed the 120 s
`GIT_TIMEOUT` as well. By hand (no harness, warm cache) that one step took 81 s
for 250,000 files, writing 1.2 GB of loose objects; `ls-files -o` took 0.14 s
and `write-tree` 1.1 s. 100,000 files took 4 s. That is 20 times the time for
2.5 times the files, so it is not linear: the likely cause is the disk's write
speed once the dirty-page limit is passed, which I did not prove.

**What does not borrowing cost, for a repository folder?** Disk: 9.7 MiB
against 478 MiB after the first snapshot at 100,000 files (about 50 times;
1.1 against 51 MiB at 10,000), and 9.9 against 198 MiB after ten more turns (the
`gc` packs it and it compresses). It also leaves 103,982 files in the shadow
until the first `gc`. First snapshot: 1.9 s against 5.7 s at 100,000 files, 187
against 539 ms at 10,000, 42 against 99 ms at 1,000 (about 3 times). Time
per turn is unchanged afterwards, and for plain folders `prune` is the larger
cost (3.3 s against 0.4 s at 100,000). What borrowing costs is on restore:
`secure_blobs` takes 170 to 400 ms to copy the blobs it needs, and does nothing
for plain folders. The price of borrowing is the O11 risk (an old checkpoint
unrestorable after the user rewrites and prunes their history); the price of
not borrowing is the numbers above, and the first-snapshot failure described
below becomes reachable for repositories as well (a repository folder at 100,000
files would behave like the plain one).

**Which single step dominates at 100,000 files?** Method: each git command timed
separately with the same arguments the session uses (table above). For the
first snapshot it is the hashing, `update-index --add`: 4.0 s of about 4.5 s for
a plain folder (it also writes every object) and 1.15 s of about 1.6 s for a
repository (it hashes, the objects exist in the borrowed store). For a snapshot
of an already-snapshotted folder no step dominates: `write-tree` (100 to 140
ms), `add -u` (60 to 70), `ls-files -o` (about 55) and two `ls-files --stage`
(about 20 each) are each a fifth to a third of the 300 ms. The listing is not
the cost, and the ignored directory with 100,000 files is free: unchanged
snapshots took 166 to 201 ms with it and 169 and 171 ms without (the first
snapshot's differences, up to 2 times for a plain folder, are the disk noise
described above: the minimums are within 10%). For a whole turn at 100,000
files in steady state the dominant step is `prune`'s `git gc`: 3.3 s of 4.0 s
for a plain folder, 0.4 s of 1.1 s for a repository, and it is 260 to 340 ms
even at 1,000 files.

**Is a per-node checkpoint budget setting warranted?** The numbers do not
argue for a time budget that limits the snapshots of folders up to 100,000
files: they cost 0.3 s each there, 20 times less than the 60 s limit. What they
argue against is two other things. First, `prune` runs `git gc --prune=now` on
every turn once 10 checkpoints exist, which is the largest cost at every size
measured (260 ms at 1,000 files, 3.3 s at 100,000 for a plain folder); that is a
fix to the code (collect less often, or not every turn), not a setting.
Second, a first snapshot of a large plain folder on a slow disk does not finish
inside the fixed timeouts (250,000 files: did not finish in 60 s, nor in 120 s
for one git command), so there a setting for the timeout or for a file-count
limit (refuse to checkpoint with a clear message rather than fail after
a minute) would change what happens. That is a budget on the first snapshot, not
on every turn, and I measured it on one machine with a slow disk under load. On
an NVMe drive the line will be elsewhere; I cannot say where from these numbers.

## Defects found while measuring

1. **A snapshot that times out leaves lock files in the shadow.** In a run at
   100,000 plain files while the disk was busy (the machine was starting a
   game) and again at 250,000 files, the snapshot hit the 60 s timeout, the
   `git` process group was killed, and `harness-index-<id>.lock` (0 bytes)
   stayed in the shadow directory, one per killed git. Evidence: `LEFTOVER`
   lines in the benchmark's output (two `.lock` files after one timed-out
   first snapshot, which also hit the 120 s limit for a second git). Nothing
   removes them. They are harmless today (each name is used once) but grow by
   one or two per timeout. The benchmark's `leftovers` check reports them.
2. **A first snapshot of a large plain folder cannot succeed.** At 250,000
   files it timed out, and so did the retry (see above). The turn runs without
   a checkpoint and the session says so; nothing here is silent. Whether the
   second attempt succeeds (the objects written by the killed one exist) I did
   not test.
3. Not a defect but worth knowing: `prune` runs `git gc --prune=now` on every
   turn once the session holds 10 checkpoints, and takes 0.26 to 3.3 s of every
   turn at the sizes measured.

## What was not measured

- **500,000 files.** I started it. The tree took 134 s to generate, and then
  the read pass over it was still going after about 19 minutes at roughly 190
  file reads per second (the page cache had been evicted by memory pressure
  on the machine), when I stopped it. A 250,000-file plain folder fails in the
  first snapshot (above), so 500,000 would not have produced numbers for the
  later operations without changing production code.
- **250,000 files**: only the first snapshot of the plain folder (failed, twice)
  and, for a repository, nothing. A cold read of the tree (the benchmark's
  "read every file once" step, 8 threads) took 1,022 s (about 245 files per
  second, 4 ms per file) after the page cache lost it. That is the one cold
  number I have and it was an accident, not a controlled cold cache. The same
  step at 100,000 files, warm, took 55 ms.
- **Cold cache** in the sense of the task: not available (no root).
- **A quiet machine.** Everything ran next to a game using about four cores.
  Cells above about 20% spread (the plain 100,000 first snapshot, restores at
  100,000, the 1,000-file runs at 100 to 300 ms) are noisy; do not read a
  difference of that size into them.
- **An SSD.** This disk is rotational. The single-file operations will be
  faster and the first snapshot's write stalls will be smaller on an SSD.
- The two sides of the unchanged-repeat breakdown (285 ms summed against
  160 to 200 ms measured) are not reconciled.
