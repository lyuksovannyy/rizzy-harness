# X.42

## Adds

Undo and the branch: a note when HEAD moved; putting back what an undo replaced (the real-use trial of 2026-11-19, items 10, 16 and 18: a turn's "files changed" included what `git checkout` changed, so an Undo there would rewrite another branch's files with nothing said; "Undo anyway" left the overwritten edit reachable only through the API; 215 `WARN genai::…::streamer: EMPTY CHOICE CONTENT` lines in the log).
Not done: the "held by a commit made since" message after an undo and HEAD on every checkpoint event (`docs/undo-and-stash.md` §2.1, §2.2: still wanted); the UI cannot tell a pruned checkpoint from a kept one without asking the node, so "Put back" shows until the node says "no longer kept" (the normal Undo works the same way).

## Safeguards and abuse tests

**Done on branch `fix/undo-branch-note`** (PLAN.md `checkpoint` row, T33; README "Every agentic turn is checkpointed"; `docs/undo-and-stash.md` §2.2). (1) The `after` checkpoint carries `head_moved: {from, to}` (branch or detached, commit) only when HEAD's branch or commit differs from the turn's start, read from `.git` by `user_repo::head_at` (files only, no git process, no link followed, a hostile `.git/config` never read); names cut to 200 bytes and cleaned of controls, line separators and bidi controls.
The Undo block and the changes panel say it in one sentence per case (branch switched; commits on the same branch; detached variants) and what an undo does about it;
Undo is unchanged. (2) After an undo the turn's row offers "Put back what this undo replaced": the existing restore route with the id the undo saved (`before_restore`), through the same refusals and "Undo anyway"; both folds keep, per saved id (last 100), which turns were undone just before that restore, and restoring a saved id returns those flags (review of PR 180: the first rule, clearing by `saved`, was wrong after two undos and after undoing a put-back). (3) The `genai` OpenAI streamer's per-reply WARN is down to errors in the default log filter (`log_env` in `crates/cli/src/main.rs`); `HARNESS_LOG` naming `genai` brings it back.
Tests: `crates/node/tests/head_moved.rs`, `git/user_repo.rs` (`head_at_reads_files_and_refuses_redirects`), `git.rs` (name cleaning), `crates/core/src/view.rs`, `crates/core/tests/fold_fixture.rs` with `web/src/fold.test.ts`, `web/src/components/headnote.test.tsx`, `crates/cli/src/main.rs` (`the_empty_choice_warning_is_quiet_by_default_only`).

