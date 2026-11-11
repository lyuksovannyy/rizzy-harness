# Gate 3 fix review 3, 2026-11-11: R.1 reworked, the rest of the web UI, the new approval card

Fourth pass. Read detached, nothing committed to the branches, no product code
changed.

- Part 1: `fix/R.1-sandbox-findings` at `f1f9fd3` (pull request 36; the rework
  is `f83a87a`).
- Part 2: the web UI components the earlier passes had not read, on `main`.
- Part 3: `fix/R.6-approval-card` at `e34ecfe` (`b41b046` W-1, `0be0e45` W-2 and
  W-3).

Same scale and the same rule for proofs: a test that passes is a gap that is
still there.

## 1. Verdict

| Finding | First severity | Result |
|---|---|---|
| S-1 (node git runs what a session configured) | Critical | **Closed.** |
| S-2 (a sub-agent's folder swapped for a link) | High | **Closed.** |
| S-3 (`core.worktree` moves checkpoints and undo) | High | **Closed.** |
| R1-1 (`commondir` puts a sub-agent in another repository) | High | **Closed.** |
| R1-2 (swap during the worktree fill) | Medium | **Closed** for anything a session can start. The author's inverted test proves where the folder is, not that the fill cannot be raced; see section 3. |
| R1-3 (child may write the user's object store) | Note | **Closed** (read-only grant). |
| W-1 (approval card) | High | **Closed with a remaining gap** (W-4, W-5, Low). |
| W-2 (other port shown as the same host) | Low | **Closed** (by reading). |
| W-3 (hidden characters in titles and node text) | Low | **Closed** as far as read. |

**R.1 is fit to merge after R1b-1**, which the rework introduced and which a
sub-agent triggers without anyone clicking. R1b-2 is the author's own stated
residual, wider than stated; it should not wait long, and its smallest fix is
small. **R.6 is fit to merge**; W-4 and W-5 can follow.

New findings: 2 Medium (R1b-1, R1b-2), 4 Low (R1b-3, R1b-4, W-4, W-5), 5 Note.

## 2. New findings

### R1b-1. Medium. A restore holds every blob it writes in the node's memory

Severity: one sub-agent with a shell makes the node process (on the embedded
node, the server) allocate as many bytes as it wrote, with no race and no click.

- Introduced by the rework: `checkout-index` wrote the files itself; now
  `restore_paths` (`crates/node/src/git.rs:1948-1981`) first calls
  `blob_contents` (`:1857-1924`), which reads every blob of the plan through
  `cat-file --batch` into a map and returns it whole. The "groups of at most
  64 MiB" bound one git call's output, not what is kept; a blob larger than a
  group goes alone, whole. `run_raw` holds the call's output as well, so the
  peak is about twice the content.
- Reached without a person: a sub-agent's result is applied at the end of the
  delegation (`SubRepo::apply`, `:2305-2345`, `restore_paths` at `:2343`). A
  sparse file costs the child no disk and the checkpoint store a few
  kilobytes.
- Established: **proven by test**,
  `review_r1b_1_a_restore_holds_every_blob_it_writes_in_memory`
  (`crates/node/src/git_review_fixes3.rs` in the patch): two sparse files of
  80 MiB, and all 160 MiB are in the returned map at once.
- Smallest fix: write one blob at a time, streamed: `cat-file --batch` read
  incrementally into `at::Top::write` (which already writes to a temporary
  name and renames), or `cat-file blob <id>` per file with its stdout copied to
  the opened descriptor. Refuse a plan above a stated total, as `read` and
  `grep` have caps.
- Threat row: T9 ("no tool can wedge a node"), and the "process stays light"
  invariant.

### R1b-2. Medium (by reading; not built). The snapshot read window feeds a sub-agent's folder, with no undo and no person

- The author states the residual: a snapshot reads the folder by path, so a
  folder swapped for a link while it runs can put a file from outside into the
  checkpoint, and an undo would write it back.
- What the statement leaves out: the same snapshot is a sub-agent's base.
  `SubRepo::base` (`git.rs:2182-2190`) snapshots the parent's folder, and
  `make_folder` writes exactly that tree into the child's folder
  (`:2241-2242`), where the child reads it with `read`. A process the parent
  left running (the recorded `setsid` survivor) can write the parent's folder
  during that snapshot. So the path from "git read through a link" to "the
  agent reads the bytes" needs no undo and no click.
- What bounds it: git itself checks leading components for links before it
  reads a path, so the swap has to land between two system calls inside one
  git process. That is narrower than R1-2's window. Nothing else bounds it:
  the snapshot's git is not sandboxed, and nothing in that path excludes the
  node's data directory.
- Established: **shown by reading.** I did not build it in this pass, so the
  rating is by mechanism: Medium, and High if a build shows it lands as
  readily as R1-2 did.
- Smallest fix: start the node's snapshot git through the same Landlock
  wrapper the shell uses, granted read on the folder, read on the borrowed
  object store and write on its own shadow directory only. The kernel then
  judges every open by where it really leads, and no timing matters. The root
  fix is the one already planned (ADOPTION_PLAN T.8): nothing a session started
  survives its command, so nobody is left to swap anything.
- Threat row: T33's stated residual; wider than recorded.

### R1b-3. Low. `user_repo` reads session-writable files by path after checking them by path

`read_small` (`crates/node/src/git/user_repo.rs:58-64`) `lstat`s the file for
type and size and then calls `read_to_string(path)`: no `O_NOFOLLOW`, no
`O_NONBLOCK`, and the size limit applies to what was there at the `lstat`, not
to what is read. `HEAD`, the ref file and `packed-refs` are in the session's
own `.git`, which its shell may write. `find` runs on the async path without
`spawn_blocking` (`git.rs:570`), at every checkpoint. The file tools closed this
exact shape (`harness_tools::open_regular` and `read_small`: flags, `fstat` on
the handle, `take(cap + 1)`); the node's own reader of the same kind of file is
weaker than the model-facing one. `packed-refs` is also read whole up to
64 MiB per call (`:118`). Shown by reading. Fix: use the same open-then-check
helper with a hard `take`, run `find` in `spawn_blocking`, lower the cap.

### R1b-4. Low. `at::remove_tree` uses two descriptors and a stack frame per level

`remove_tree` (`crates/node/src/git/at.rs:175-189`) recurses with the directory
open and a second handle from listing `/proc/self/fd/N`. A directory nested a
few hundred deep where a restore must put a file makes the restore fail
part-way with `EMFILE`, and for that moment the node is short of descriptors.
Shown by reading. Fix: iterate with an explicit stack and a depth limit, and
list through the descriptor already held.

### W-4. Low. The middle of a long input is approved unread: said, not prevented

The new card shows the first ~700 and last ~500 characters of a text over
1,500 characters or 40 lines, states how much is not shown, and adds "You are
approving N characters you have not expanded". Both Allow buttons stay enabled.
Almost every real `write` is over the limit, so the line will be on nearly every
card and will stop being read. **Proven by test** (case 1 of
`web/src/components/review-gate3-card.test.tsx`): content with complete-looking
head and tail and a command in the middle; the card says so and Allow works.
Fix: while anything is collapsed, the primary button reads "Show all, then
allow" and expands; allowing unread takes a second, differently worded control.

### W-5. Low. Characters the card neither marks nor counts

`showHidden` (`web/src/md/hiddenChars.ts`, `SHOWN_SRC`) is a list. Not on it,
each rendering as nothing or as a blank: U+2800 (braille blank), U+FFF9 to
U+FFFB, U+1D159, U+1BCA0 to U+1BCA3, U+13430. A run of 600 U+2800 in a command
is neither a marked character nor a `⟨600 …⟩` run. **Proven by test** (case 2).
Nothing is hidden by it now that the text wraps with no height limit; it leaves
a blank stretch before the part that matters. Fix: mark by Unicode class
instead of by list: every `\p{Cf}`, `\p{Cc}` (but tab and line feed), `\p{Zs}`
other than U+0020, `\p{Zl}`, `\p{Zp}`, plus the blank-looking letters as now.

### Notes

- N-1. A word wholly in another script is not named (the author says so);
  case 3 shows `./сору` with four Cyrillic letters gets no note. In a command
  or a path, naming any letter outside ASCII costs nothing and needs no table.
- N-2. Text shaped like the card's own markers (`⟨500 spaces⟩`, the "not
  shown" sentence, `<U+0020>`) is shown as it is; only colour tells it from a
  real marker (case 4). It hides nothing. Escaping `⟨`, `⟩` and a literal `<U+`
  in content would make the brackets mean one thing.
- N-3. The card renders the tool call's input, not the approval event's. On
  the node both come from the same value and so does what runs
  (`crates/agent-native/src/lib.rs`: `ToolCallStarted { input: call.input.clone() }`,
  `approve(.., &call.input, ..)`, `tool.run(.., call.input.clone())`, and the
  same pair for `delegate`); the master stores and replays payloads unchanged;
  the fold attaches an approval to the last tool item with its `call_id`
  (`web/src/fold.ts`, `lastTool`), which is the call just started even when a
  model repeats an id. So the claim holds for an honest node. A cheap guard
  for the day it stops holding: compare the two inputs in the fold and say so
  on the card when they differ.
- N-4. A child session's `cwd` is a path inside the data directory and goes
  into `session_created` and the system prompt, so the model and the chat
  learn where the data directory is.
- N-5. `sessions_dir` (`crates/node/src/lib.rs:175-178`) is the checkpoint
  store's parent plus the name `sessions`, and `NodeRuntime::start` removes it
  unconditionally. In the shipped binary that is `<data>/sessions`. Tie it to
  the data directory explicitly (a field of `NodeConfig`) and wipe only a
  folder that holds a marker the node wrote.

## 3. What I checked and found sound

**Part 1, by running**
- First review's proofs against `f1f9fd3`: S-3 fails (the root stays the
  session folder); the S-1 and S-2 sandbox proofs fail (no write to the main
  `.git`, no rename of the folder). The author's inverted S-1 and S-2 runtime
  tests use my scenarios with the assertions inverted and pass.
- My R1-1 and R1-2 proofs no longer compile: `make_worktree` and
  `sandbox::worktree_git` are gone. The author's
  `commondir_never_puts_a_sub_agent_in_another_repository` asserts the same
  property (no folder in, no read of, no change to the other repository) and
  passes. `no_swap_during_the_child_folder_fill_…` watches the old location,
  where nothing is made any more, so it would pass even if the fill could be
  raced; what closes R1-2 is the grant code below.
- The author's undo race test,
  `a_folder_swapped_for_a_link_during_an_undo_is_never_written_through`, ported
  to the previous state of the branch (`f21f02e`), **fails there** (try 39,
  "wrote or deleted through the link") and passes on `f1f9fd3`. It does catch
  the old behaviour.

**Part 1, by reading**
- No git of the node names the user's repository: every call goes through
  `command()` with `--git-dir <shadow>`; `rev-parse` and `worktree` in the
  user's repository are gone (grep of `git.rs`, `upkeep.rs`, `session.rs`,
  `artifacts.rs`); `config --global --get core.excludesFile` runs with the
  shadow as git directory (`git.rs:492-531`).
- `user_repo::find`: a `.git` that is a file or a link, or holds `commondir`
  or `gitdir`, is not a repository and ends the walk there; a ref name is
  followed only under `refs/heads/` with plain components; the object store is
  borrowed only as a real, unlinked directory with no alternates of its own.
- The carve-out. A child's folder is `<data>/sessions/<id>/files`, made 0700
  by the node. The only ways in are `OwnedFolder`: `refuse_protected` lets a
  path through only when its real location is under the caller's own `top`
  (`crates/tools/src/lib.rs`, about `:312-318`), then the usual workspace test applies;
  `Confinement::command` skips the data-directory check only for a workspace
  under that same `top` and grants nothing else there
  (`crates/tools/src/sandbox.rs`, about `:368-400`); grep and glob exempt only entries
  under it. A session reloaded from the log has no `owned` and is refused as
  protected; `session.create` from the master cannot name such a folder. The
  child's grant on the user's repository is read-only on `objects`. A writing
  sub-agent of a sub-agent is refused before any session exists
  (`session.rs:1534`). `rel` inside the child's folder must resolve inside it
  (`:1583-1590`).
- `git/at.rs`: the top is opened once with `O_NOFOLLOW`; every component is
  `openat` with `O_DIRECTORY|O_NOFOLLOW`; a link or file in the way is unlinked
  and replaced, never followed; files go to a temporary name and are renamed
  in the opened directory; deletes are `unlinkat` there.
- The regressions asked about: `core.worktree` is ignored because nothing of
  the node reads the user's config any more; gitfile, linked or `commondir`
  repositories get plain-folder checkpoints at the session folder and refuse
  writing sub-agents with the reason; `cwd_outside_roots_is_refused` uses
  `/usr` because `/` is now refused earlier, as holding the data directory.
  The other changed assertions follow the redesign (`/sessions/`,
  `cat-file --batch`).
- The borrowed object store: the node asks for objects by ids it hashed from
  the folder, and for `HEAD`'s id only to build the child's index; a planted
  object can change what a later restore writes into that same folder, which
  T19 records.

**Part 2: the web UI not read before**
- `md/UiHost.tsx`, `ui/UiBlock.tsx`: a block is drawn only when the server's
  validated list has the same ordinal, hash and type with a spec; otherwise it
  stays code. The card sits in a frame with a "from" line in the harness's
  words. The renderers (`Form`, `Choices`, `Input`, `Chart`, `Progress`) have
  no `href`, `src` or URL; their one network call is `api.prompt` with a reply
  of ids and values to the same chat. A form can look like anything inside its
  frame; it can only send a reply to its own chat. Secret-asking fields are
  refused in Rust by a word list that errs towards refusing
  (`crates/core/src/ui/secret.rs`).
- `md/hast.tsx`, `md/Code.tsx`: highlighter output becomes `span`s with class
  names matching `[\w-]+` and text; every other element and attribute is
  dropped.
- `Table.tsx`, `Conversation.tsx`, `Composer.tsx`, the routine, agent,
  settings, node, search, memory and project components: grepped for dynamic
  `href`, `src`, `style`, `action`, frames and embeds; none takes a model or
  node string (one grid style from a count, one colour from a palette).
- **Answer to the question that matters:** I found no way for a model's or a
  node's text to run script in the origin or reach `localStorage`. The CSP
  (no inline or evaluated script, same-origin only) stands behind that.

**Part 3: the new card**
- My six W-1 cases, as written, all fail on `e34ecfe`; three of them fail first
  on the renamed button, so I checked the branch's inverted file case by case:
  same inputs, assertions inverted, all pass. Its first case has the command at
  the end, which the tail now shows; the middle is W-4.
- Text wraps with no height limit; runs of blanks become counted markers;
  hidden characters become codes in a marked span; the path has its own line
  and the card says only what a path makes knowable (absolute, lexically
  outside, climbing through `..`) and nothing about being inside; every key of
  any tool's input is a row, nested ones included; the buttons follow the
  input; "for this turn" appears only when the request carries `turn_allow`.
- W-2: `hostOf` shows the port and a link to another port of the page's own
  machine always asks (`web/src/md/links.ts`, `leavesApp`).

## 4. What I did not check

- R1b-2 was not built; R1b-3 and R1b-4 were not reproduced.
- `git/upkeep.rs` after the rework; `crates/node/src/artifacts.rs` changes;
  the merge resolutions in `session.rs` beyond `isolate`, `checked_folder` and
  the grandchild rule; `crates/cli/tests/sub_agent_sandbox.rs` (not run).
- The second half of `user_repo`'s consumers (`describe`, `changed_since`).
- `Conversation.tsx` and `Composer.tsx` were grepped and read in part, not
  line by line; `Form.tsx`, `Choices.tsx`, `Chart.tsx` likewise. No browser.
- `0be0e45` beyond `links.ts` and the tool card (the sidebar and shell
  changes for W-3).
- No full test suites, `clippy`, `cargo deny`, `pnpm build` or `pnpm size`.

## 5. Proofs

`docs/security-review/proofs/2026-11-gate3-fixes-3.patch` adds tests only; each
passes, and passing is the gap.

```bash
# R1b-1, on fix/R.1-sandbox-findings
git checkout --detach fix/R.1-sandbox-findings
git apply --include='crates/*' docs/security-review/proofs/2026-11-gate3-fixes-3.patch
cargo test -p harness-node --lib review_r1b_1

# W-4, W-5 and the two notes, on fix/R.6-approval-card
git checkout --detach fix/R.6-approval-card
git apply --include='web/*' docs/security-review/proofs/2026-11-gate3-fixes-3.patch
cd web && COREPACK_ENABLE_DOWNLOAD_PROMPT=0 CI=1 pnpm exec vitest run src/components/review-gate3-card.test.tsx
```

Run here on 2026-11-11: R1b-1 passes on `f1f9fd3`; the four card cases pass on
`e34ecfe`.

## 6. The three things I am least sure of

1. R1b-2's rating. It rests on reading git's behaviour and the code, not on a
   run; it could be harder to land than I assume, or easier.
2. The carve-out in the data directory. I traced the three places that let a
   path through and found each tied to the caller's own folder, but I did not
   run the real sandbox against a child's folder (`sub_agent_sandbox.rs`).
3. The web UI answer rests on greps and partial reads for the two largest
   components; a sink written in a way my patterns miss would not show.
