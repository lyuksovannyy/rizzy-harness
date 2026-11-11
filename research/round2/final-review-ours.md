# Final review: what section 10 and the executive summary say about our code

Scope: every item in section 10 (a) to (e) of `research/harnesses-round-2.md`
(`synth/_narrative/10-applications.md`) and the seven items of the executive summary
(`synth/_narrative/01-executive-summary.md`), checked against the code on `main`,
`integration/gate3` (PR 4), `feat/agent-roster` (PR 6) and
`fix/provider-retry-and-compaction` (PR 7), and against
`docs/nodes-credentials-environments.md` on this branch. Nothing was built or run; every
statement below is from reading code with `git show`/`git grep`. Branch prefixes: `main:`,
`g3:` (integration/gate3), `p6:` (feat/agent-roster), `p7:` (fix/provider-retry-and-compaction).

## Summary

Verdicts on statements about our code (items that make no claim about us are counted as n/a):

| Verdict | Count | Items |
|---|---|---|
| accurate | 9 | A4, A12 (absence of seccomp), A13, §4.8, §4.9, §6, (c) step 5, exec 5, exec 7 |
| partly | 22 | A1, A2, A6, A7, A8, A9, A11, A14, A15, §4.1, §4.3, §4.4, §4.5, §4.6, §4.7, (c) step 1, (e) "latest connection wins", exec 1, 2, 3, 4, 6 |
| already done | 2 | A3 (on g3, almost all of it), A10 (on p7, most of it) |
| wrong about our code | 1 | A5 |
| cannot tell | 0 | (one sub-point, instruction re-injection after compaction in A11, is not settled) |
| n/a (no claim about us) | 30 | §4.2, (c) steps 2-4 and 6-11, 20 rows of (e) |

**Drop or rewrite.**

1. **A1: drop the action digest, keep the narrowing of "allow for this turn".** The approval
   id is already minted on the node, bound to one pending call, consumed once and refused
   for any other session (`g3:crates/node/src/session.rs:972-1049`). The master already
   accepts an answer only from a principal that can see the chat, i.e. its owner
   (`g3:crates/master/src/http.rs:1435-1451`, `visible_session` at `:317-327`). A digest the
   node computes and the master echoes defends against a master that lies, and a lying
   master can approve anything anyway (`FIX_PLAN.md` §5: no signed approvals, `PLAN.md` R1).
   What is real: the allowance is keyed on the tool name, so approving `bash ls` twice lets
   `bash rm -rf ~/project` run unasked for the rest of the turn
   (`g3:crates/agent-native/src/lib.rs:707`, `:1209-1224`). Rewrite A1 as "key the turn
   allowance on the canonical argument (argv for bash, path for write/edit)"; small.
2. **A5: the premise is wrong.** A call cut mid-JSON never reaches a tool: genai 0.7.2
   leaves unparseable arguments as a JSON string, and our dispatcher refuses any non-object
   (`main:crates/agent-native/src/lib.rs:1100`, `g3:…/lib.rs:1203-1208`). What remains is
   small: no finish-reason is surfaced (`Completion` has none,
   `g3:crates/agent-native/src/provider.rs:82-86`), so the model gets "arguments must be a
   JSON object, got: <the truncated text>" instead of "you hit the output limit".
3. **A2: downgrade from "fix in what is built" to a precondition for (c) steps 2 and 4.**
   Frames left in the old connection's queue (at most `FROM_NODE_QUEUE` = 8,
   `g3:crates/link/src/lib.rs:68`) are still applied after a reconnect
   (`g3:crates/master/src/lib.rs:819-821`), but every append is checked for the owning node,
   a tombstone and a contiguous `seq` (`crates/store/src/lib.rs:514-553`, both branches), and
   both connections carry the same node's log. Nothing corrupts today. The surviving gaps are
   narrower: a duplicate `seq` is accepted without comparing content
   (`store/src/lib.rs:546-548`), so a cloned key or a node restored from an old disk
   diverges silently; and a revoked node's queued frames are still applied. Keep the
   content hash on duplicates (small); build the epoch with the features that move sessions.
4. **A12: the ".git read-only inside the session folder" bullet cannot be done with
   Landlock.** Landlock rules only add rights, and a right on a directory covers everything
   beneath it; once the session folder is read-write, `.git/hooks` is too. It needs a mount
   namespace (read-only bind over `.git`) or a different design. Size goes from medium to
   large, or the bullet becomes a named residual. The file tools already refuse `.git` writes
   (`g3:crates/tools/src/fs.rs:668-677` test).
5. **A3: mostly already done on PR 4; drop `rustix`/`PR_SET_DUMPABLE` as a requirement.**
   Tests exist for the API token, the SSH key, the data dir listing
   (`g3:crates/cli/tests/sandbox.rs:101`), `/proc/$PPID/environ` and `/proc/$PPID/mem`
   (`:186`), signals (`:357`), pathname Unix sockets (`pathname_unix_sockets_need_abi_9`),
   and the environment allow-list on the embedded node
   (`g3:crates/cli/tests/tool_env.rs:83`). Landlock already denies ptrace-class access across
   domains, which is what D11 asks. If non-dumpable is still wanted, `nix` 0.31 with the
   `process` feature is already a dependency (`g3:Cargo.toml:72`); no new crate.
6. **A15: conflicts with `FIX_PLAN.md` 4.8** (measure delta growth, then an ADR, nothing
   changed before it). The browser half is already done: the socket subscribes before it
   replays (`g3:crates/master/src/ws.rs:1-7`, `:319`, `:354-372`). D16 needs no experiment:
   deltas are log events on every branch (`g3:crates/agent-native/src/lib.rs:765`, `:1275`,
   appended and fsynced by `g3:crates/node/src/session.rs:199-238`; node.db is
   `Durability::Full`, `g3:crates/cli/src/main.rs:517`).

**Priority changes.**

- **Up: A6.** Settled by reading, and it fails open: `Session::emit` logs and drops an
  append error (`g3:crates/node/src/session.rs:240-244`) and `TurnHost::emit` returns `()`,
  so a tool runs after its `tool_call_started` failed to persist
  (`g3:crates/agent-native/src/lib.rs:927-947`). The report's worry about PR 4's back-pressure
  is the wrong way round: the event is committed locally first, and a full link only delays
  the tool (`session.rs:231-236`). Small fix in security code.
- **Up: A8's last bullet.** Our own code does what the report blames on OpenHands and Hermes
  Agent: a writer sub-agent whose worktree cannot be made, or whose folder is not a git
  repository, works in the parent's folder (`g3:crates/node/src/session.rs:676-686`,
  `:789-798`; same on `main:671` and `p6`). The rest of A8 is already done.
- **Down: A1** (see above), **A2** (see above), **A15** (behind 4.8).
- **Down: §4.4's 128-bit fingerprint.** The note's 16 hex characters are 64 bits; matching
  them needs about 2^64 key generations inside a 10-minute pairing window. Fine to lengthen,
  not an urgent design defect.

## Part (a): fix in what is already built

| Item | What the report says about us | What the code shows | Verdict | Recommendation still right? | Size check | Fix or feature | Invariant / freeze |
|---|---|---|---|---|---|---|---|
| A1 approvals bound to a digest; narrow "allow again" | Answer names an approval id; allowance keyed on tool name; needs digest, single use, owner-only acceptance | Approval id: `new_id()` UUIDv7 minted on node, oneshot per call, removed after use, looked up per session (`g3:crates/node/src/session.rs:972-1049`, `g3:crates/node/src/lib.rs:629-640`). Master: `visible_session` + `can_see` = owner only, except the API token (`g3:crates/master/src/http.rs:1435-1451`, `g3:crates/master/src/auth.rs:137-143`). Allowance: per-turn `HashSet<String>` of tool names, skips `approve` entirely (`g3:crates/agent-native/src/lib.rs:707`, `:1209-1224`; `main:…/lib.rs:625`, `:1118`). PR 4: offered and honoured only after a first approval in the turn (`g3:session.rs:997-1035`); on `main` it is honoured on the first. Node policy is loaded once at start, so "policy changed between request and answer" cannot happen | partly | Narrowing the key: yes. Digest/echo: no (no threat it stops given R1). "Never from the node link": already true | Small for the key change, not medium | Fix | Approval path is security code; changing it before Gate 3 resets the requested outside review of the approval path |
| A2 fence the link | Reconnect cancels old connection, no generation on frames (`nodes.rs:207`) | `g3:crates/master/src/nodes.rs:207-217` as stated. A per-node `generation` exists in memory (`g3:crates/master/src/lib.rs:759-781`) but is used only to decide who marks the node offline (`:822-830`). Old link's queue drains into `apply` (`:819-821`, pump ends on `stop`, `g3:crates/link/src/pump.rs`). `apply` → `Store::append` checks owner node, tombstone, `seq <= head` = duplicate, `seq != head+1` = gap (`crates/store/src/lib.rs:514-553` on both branches). Duplicates are not compared by content. Revoke cancels the link but queued frames still apply (`g3:nodes.rs:57-66`). Session ownership never moves today, and there is no archive. `main` used unbounded channels (`main:crates/link/src/server.rs:23-24`) | partly | Content hash on duplicate: yes, small. Epoch: needed only when ownership can move (fork, workspace nodes, environments), so build with those | Medium for the full fence; small for the hash | Fix (hash); precondition for features (epoch) | One writer; schema change is in-place before first tag, fine |
| A3 control-plane credentials out of reach | Write tests first; prctl via `rustix` | Already on g3: `ssh_key_and_api_token_are_permission_denied` (`g3:crates/cli/tests/sandbox.rs:101`), `the_harness_environment_is_not_readable_through_proc` (`:186`, reads `/proc/$PPID/environ` and `/proc/$PPID/mem`), `a_nested_sandbox_exec_gains_nothing` (`:237`), `signals_to_the_harness_are_denied` (`:357`), `bash_cannot_read_keys_beside_the_project` (`g3:crates/cli/tests/data_dir.rs:175`), `embedded_node_shell_sees_only_allowed_variables` (`g3:crates/cli/tests/tool_env.rs:83`). `HARNESS_*` and provider-key names are never passed even with `env_pass = ["*"]` (`g3:crates/tools/src/env.rs:33-36`, `:115-125`). Not covered: `node.key` read on a `node run` node; grants and `$CREDENTIALS_DIRECTORY` do not exist yet. Blocking `LD_*`/`BASH_ENV` in `env_pass` protects nothing against the agent, which can set them in its own command | already done (most) | Add one `node run` test for `node.key`; drop the injection-variable and prctl bullets or justify them against a different attacker | Small | Fix (test) | None; `rustix` unnecessary, `nix` `process` already present (`g3:Cargo.toml:72`) |
| A4 per-user tokens | One long-lived admin-equivalent token sees every chat | Created by `load_or_create_token` = 32 random bytes hex, 0600 (`g3:crates/master/src/auth.rs:180-182`, path `g3:crates/cli/src/main.rs:633-635`); compared in constant time (`auth.rs:561-563`); any `Authorization: Bearer` that matches becomes `Principal::api_token()` = `Role::Admin`, `can_agentic`, `user_id: None` (`auth.rs:121-128`, `:740-751`); `can_see` returns true for `None` (`auth.rs:137-143`); scrubbed from events (`g3:crates/cli/src/main.rs:244-248`); printed by `rizzy-harness token` (`main.rs:864-868`). Same on `main` (`main:crates/master/src/auth.rs:138-143`, `:399-401`) | accurate | Yes, but per-user tokens are a feature. Fix inside the freeze: scope the existing token (no chat read, or require a user header), or record the exception in the threat model | Medium | Feature (per-user tokens); fix (scoping) | Violates "chats private, for admins too" today; freeze for the feature |
| A5 tool calls from a length stop | We run truncated arguments; a truncated `write` loses data | genai 0.7.2 keeps unparseable or cut-off arguments as `Value::String` (Anthropic `streamer.rs:503-522`, OpenAI `streamer.rs:226-244` in the registry copy); dispatcher refuses any non-object (`main:crates/agent-native/src/lib.rs:1100`, `g3:…/lib.rs:1203-1208`). No finish reason in `Completion` (`g3:…/provider.rs:82-86`); no counter of length stops. Earlier, complete calls in the same message still run | wrong about our code | Rewrite: surface the stop reason, say "output limit reached" without echoing the truncated text, stop after N | Small | Fix (quality) | None |
| A6 append, flush, then act | Must be checked; depends on PR 4's link | Order holds: `tool_call_started` is appended (node SQLite, `Durability::Full`) before `run_tool` (`g3:crates/agent-native/src/lib.rs:927-947`; `g3:crates/node/src/session.rs:199-238`; `g3:crates/cli/src/main.rs:517`). Fails open: `emit` drops errors (`session.rs:240-244`). Back-pressure delays the tool, never skips the local commit (`session.rs:231-236`). Crash recovery closes the turn and denies pending approvals (`session.rs:159-196`); an unfinished call becomes "[no result: the turn was interrupted]" when history is rebuilt (`g3:…/lib.rs:377`), not a log event | partly | Yes: make `TurnHost::emit` fallible for the tool-start and approval events and refuse on error | Small | Fix | One writer; tools/approval path security code |
| A7 approval failure modes fail closed | List of cases; sandbox one done in PR 4 | Policy parse error refuses start, unknown keys and values rejected (`g3:crates/node/src/policy.rs:9-10`, `:186-195`). Approver dropped → Deny (`g3:session.rs:1023-1027`). Routines never `Auto` (`g3:crates/master/src/routines/runner.rs:95-99`) and give up after `approval_timeout_hours` (`:1344-1356`). Sandbox unavailable refuses (`g3:crates/node/tests/runtime.rs:446`). Not done: audit append failure (see A6); "deny on unanalysable shell" does not apply (no per-command rules exist) | partly (most done) | Only the append-failure case remains, and it is A6 | Small | Fix | None |
| A8 roster enforced on the node | Node must refuse unlisted tools; Scout/Reviewer read-only; intersection; depth; no allowance inheritance; writer without worktree fails | Node-side refusal at execution: `offered` check in `run_tool` (`g3:crates/agent-native/src/lib.rs:1194-1201`), test `agent_tool_list_is_enforced_and_author_set` (`:1397`). Scout and Reviewer have no write tool and no bash (`p6:crates/agents/builtin/scout.md:5`, `reviewer.md:5`). Depth one: a child has no `delegate` (`p6:crates/master/tests/orchestration.rs` `teamlead_delegates_in_parallel_within_caps`). Allowance: a child is a new `Session` with its own `turn_approved` (`g3:session.rs:110`, `:154`) and a new per-turn set. Not done: writer child falls back to the parent's folder (`g3:session.rs:676-686`, `:789-798`). Children inherit `mode` (`session.rs` `run_child`), no recomputed intersection with a node-side agent policy (the spec comes from the master) | partly | Only the worktree-failure bullet is still needed, and it is about our code, not only OpenHands/Hermes | Small | Fix | Node decides (spec is master-sent; node.toml has no per-agent narrowing) |
| A9 loop guard, limits, result ceiling | Cap of 50 hard-coded; limits to `node.toml`; clamp timeouts; one result cap | `MAX_STEPS = 50` (`g3:crates/agent-native/src/lib.rs:34`, `:740`). `[limits]` already exists with `tool_stream_bytes` (`g3:crates/node/src/policy.rs:139-150`). Model timeout clamped to 1..3600 s, so 0 never means infinite (`main:crates/tools/src/bash.rs:118`, `g3:bash.rs:126-130`), but a model can ask for 3600 s above the 600 s default (`g3:…/lib.rs:89`). Result cap: 30,000 chars head+tail in every `ToolOutcome` constructor (`g3:crates/tools/src/lib.rs:38`, `:466-480`); bash keeps a bounded copy while reading (`bash.rs:180-188`). No loop guard | partly | Loop guard and step/wall-clock keys: yes. Result ceiling: exists; proportional sizing is optional | Small to medium | Feature (guard), fix (timeout ceiling) | Freeze for the guard |
| A10 PR 7 retry | Adopt a list | Done on p7: retry only when nothing was streamed (`p7:crates/agent-native/src/lib.rs:810-825`); `Retry-After` honoured and capped at 60 s (`p7:…/retry.rs` `delay`); quota/billing 429 and 402 not retried (test `only_passing_failures_are_retried`); cancel aborts the sleep (`retry::wait`); summary calls use the same helper (`p7:…/lib.rs:1276-1302`); jitter. Not done: fail fast on a long `Retry-After` (we cap and retry early), a no-network schedule, a per-turn counter shared by children, empty-response class | already done (most) | Remaining items are tuning; the one with user effect is "fail fast above N s" | Small | Fix | None |
| A11 PR 7 compaction | Adopt a list; "reads in pieces" is a hypothesis | p7: summary is a separate request with `cache: false` (`p7:…/lib.rs` `compact`, about `:1360-1368`); pieces at turn boundaries are the primary path; on failure it stops and sends history unchanged; `HistorySummarized` emitted after the text exists; estimator uses the provider's `context_tokens` (`p7:…/compact.rs` `history_tokens`); known secrets scrubbed at append (`g3:session.rs:202-207`). No shrink check, no breaker. Instruction re-injection: not settled (system prompt is rebuilt per turn) | partly | Shrink check and breaker: yes. Cache-preserving request: a cost choice, not a defect | Medium | Fix | None |
| A12 seccomp under Landlock, protected `.git` | No seccomp; keep `.git` read-only | No seccomp anywhere (`g3:crates/tools/src/sandbox.rs`). Landlock grants rw on the session folder, which covers `.git` beneath it; Landlock has no deny rules | accurate | seccomp: yes, as a follow-up after Gate 3. `.git` read-only: not possible with Landlock alone (mount namespace or named residual) | Medium for seccomp; large for `.git` | Feature (hardening) | `unsafe` forbidden: a seccomp crate that keeps `unsafe` internal (e.g. `seccompiler`, Apache-2.0) needs the dependency check; freeze |
| A13 seen-version guard | Exact-replace, no staleness check | `g3:crates/tools/src/fs.rs:195-221`: read, count matches, replace, write; `write_nofollow` truncates in place, no temp+rename, no fsync (`:226-240`) | accurate | Yes; the cheap part first (atomic write, `write` over an existing unread file refused). Exact single match already catches most stale edits | Medium | Feature (guard), fix (atomic write) | Freeze for the guard |
| A14 checkpoint restore safeguards | List of ten | Done: restore refuses during a turn (`g3:crates/node/src/session.rs:586-588`, `main:574`); compares live tree with `expected` and refuses unless forced (`g3:crates/node/src/git.rs:732-750`); restores only paths changed between `from` and `expected` (`:727-731`, `:756-771`); refs are node-minted, never paths; restore recorded as `Checkpoint{phase: Restored}` events (`session.rs:640-655`). Not done: snapshot of live state before a forced restore; nested repository; shared worktree check; no-op turns; timing fields | partly | Remaining bullets yes; `HeadMoved` only if D13 shows a disagreement (rewind is files-only today) | Medium | Fix | One writer kept |
| A15 deltas out of the log, resume by cursor | Settle whether deltas are logged; split frames | Deltas are log events on all branches (coalesced, `g3:…/lib.rs:526`, `:765`, `:1275`), fsynced. Browser subscribes, then replays, then follows (`g3:crates/master/src/ws.rs:1-7`, `:319`, `:354-372`); lag triggers catch-up from the mirror (`:409-423`) | partly | Browser part already done. The split is the decision `FIX_PLAN.md` 4.8 reserves for an ADR after measuring | Medium to large | Feature | Conflicts with 4.8 order and the restart acceptance test (`g3:crates/cli/tests/acceptance_restart.rs`) |

## Part (b): change in the design note before building

| Section | What the report says | What the note (and code) shows | Verdict | Still right? | Notes |
|---|---|---|---|---|---|
| §4.1 data model | Add `link_gen`, `owner_epoch`, fork hash; state AES-GCM AAD; store only hashes of bearers | Note has none of the three columns (`docs/nodes-credentials-environments.md:72-125`) and no AAD. Join tokens are already stored hashed (`g3:crates/master/src/nodes.rs` `token_hash`); pair codes "compared as hashes" (note `:226`) | partly | Epoch with the features that move sessions (A2); AAD yes (cheap); hashing already the pattern | Warp (AGPL) cited for AAD: idea only |
| §4.2 aliases and slots | Strict empty list, timeouts, string slots | Design proposals; no claim about code | n/a | Fine | — |
| §4.3 grants | TTL of minutes; bind to session; check every prompt | 6 h `expires_ms` (note `:197`) accurate. Grant already carries `session_id` and is sent only to nodes of the session's workspace (`:178`, `:192-194`); `grant.drop` already at turn end (`:183`) | partly | TTL and per-prompt check: yes. Binding and workspace check: already in the note | — |
| §4.4 pairing | Burn code first; refuse `Origin`; rate-limit by peer; POST body; 128-bit fingerprint | Note: single use, 10 min, 5 wrong codes close the dialog, 4 per source address, typed into an authenticated dialog (`:210-226`). Fingerprint 16 hex = 64 bits (`:219`); today's UI does the same (`g3:crates/master/src/nodes.rs:237`). Pairing runs on the node link, not HTTP, so `Origin` does not apply to the node side | partly | Fingerprint length: low priority. Burn-first: yes for the master's code check | — |
| §4.5 `node.toml` | Add `[limits]`; checkpoint budget; no `extra_args`; explicit systemd choice | `[limits]` already exists in code (`g3:crates/node/src/policy.rs:139-150`). No `extra_args` in the note. systemd credentials are an explicit `--systemd-creds` flag (`:227-231`), though §9 calls the file a "fallback" (`:529`) | partly | Add the step/time keys; say "explicit flag" in §9 | — |
| §4.6 container | Adopt a stricter argv; self-test; kill; network | Note already: fixed argv, allow-list values only, `--pull=never` by digest, cap-drop, no-new-privileges, read-only, three-step kill, self-test (`:278-308`, `:472-475`). Default `network = "on"` (`:255`) | partly | `--network=none` default, ulimit, tmpfs flags, argv label: yes | Needs Podman to test (D14, D15) |
| §4.7 forge | Token only to the git child; agent can read it whenever it can run git; strip `GIT_*` | Forge git uses the hardened, cleared environment (`g3:crates/node/src/git.rs:52`, `KEEP_ENV`). For host sessions the sandboxed shell cannot read another process's environment (`g3:crates/cli/tests/sandbox.rs:186`); in a container it can | partly | "Readable whenever it can run git" holds for container sessions only | — |
| §4.8 deletion | Stop unattended runs; bump epoch | The note does not mention routines | accurate | Yes | — |
| §4.9 fork | Lineage in the child's log; cut rules | Note: `session.seed` → `history_imported`, `session_forks(through_seq)` (`:406-412`) | accurate | Yes | — |
| §6 | Replace "second connection replaces the first" | Note `:446-450` as stated | accurate | Yes, together with A2's narrowed scope | — |

## Part (c): build next

| Step | Claim about us | What the code shows | Verdict | Fix or feature | Freeze |
|---|---|---|---|---|---|
| 1 Unattended authorisation | "Never auto-approve" as a safeguard | Already true: routines clamp `Auto` to `Ask` (`g3:crates/master/src/routines/runner.rs:95-99`) and time out (`:1344-1356`) | partly (one safeguard exists) | Feature | Yes |
| 2 Workspace nodes | none | — | n/a | Feature | Yes |
| 3 Environments | none | — | n/a | Feature | Yes |
| 4 Fork | none | — | n/a | Feature | Yes |
| 5 Web and memory tools on agentic nodes | Registered in the chat runtime only | Accurate: web tools and `remember` go to the chat runtime (`g3:crates/cli/src/main.rs:288-302`); the node gets `builtin()` plus `graph_query` (`:492-495`, `g3:crates/tools/src/lib.rs:499-511`). Consequence the report misses: PR 6's Teamlead delegates to Researcher, whose only tools are `web_search`, `web_fetch`, `remember` (`p6:crates/agents/builtin/researcher.md:5`, `teamlead.md:5-7`), so on an agentic node the Researcher child has no tools | accurate | Feature (or a PR 6 fix: do not offer Researcher as a delegate on agentic nodes) | Yes |
| 6-11 | none | — | n/a | Feature | Yes |

## Part (d): tests

"Runs without containers or Node" means `cargo test` on a Linux developer machine with git, bash and a Landlock kernel.

| # | Runs without containers or Node? | Where | Existing coverage | Notes |
|---|---|---|---|---|
| D1 setsid escape | Yes (needs `setsid` from util-linux) | `crates/tools/src/bash.rs` tests | `dropping_the_call_kills_the_process_tree` (`g3:bash.rs:587`) covers a same-group child only | Expected to fail: `killpg` on the original group (`g3:bash.rs:21-37`, `:213-235`) cannot reach a child that called `setsid`; no PID namespace or cgroup |
| D2 kill -9 node, children survive | Yes | `crates/cli/tests/acceptance_restart.rs` (SIGKILL harness exists) | None for children | Expected to fail for the same reason as D1 |
| D3 alternates + `git gc` | Yes | `crates/node/src/git.rs` tests | None | Shadow borrows the user's objects (`g3:git.rs:568-587`); plausible failure |
| D4 checkpoint timing | Yes, as an `#[ignore]` benchmark (500k files is slow) | `crates/node` | None | `SNAPSHOT_TIMEOUT` 60 s already bounds it (`g3:git.rs:45`) |
| D5 queued frames from a replaced link | Yes, in process | `crates/master/tests/link_hardening.rs` or `remote_nodes.rs` | None | Expected result: applied, but harmless when both links carry the same log (see A2); add a divergent-content variant |
| D6 archive while offline | Premise does not exist: there is no archive in code | — | Delete while offline is tombstoned (`AppendOutcome::Deleted`, `g3:crates/master/src/lib.rs:1334-1383`) | Rewrite as "delete while offline, reconnect, node sends events" |
| D7 kill -9 during bash | Yes | `crates/cli/tests/acceptance_restart.rs` | `kill_mid_stream_restart_and_replay_exactly`, `kill_while_waiting_for_approval` cover the log, not an open tool call | Closing is done at history build, not as an event (`g3:…/lib.rs:377`) |
| D8 full disk | Yes without a real full disk: a store that fails appends, or a read-only DB | `crates/node/tests/runtime.rs` | None | Expected to fail (A6) |
| D9 truncated JSON | Yes | `crates/agent-native/src/lib.rs` tests with the mock provider | Object check exists, no test found | Expected to pass (refused) |
| D10 env and `/proc/*/environ` | Yes | `crates/cli/tests/sandbox.rs` | Already covered: `:186`, `tool_env.rs:83` | Grants do not exist yet |
| D11 `/proc/$PPID/mem`, ptrace | Yes | `crates/cli/tests/sandbox.rs` | Already covered: `:186` reads `/proc/$PPID/mem` | Add `gdb -p` only if gdb is on CI |
| D12 sockets | Yes for Unix and TCP; Podman socket needs Podman | `crates/cli/tests/sandbox.rs` | Partly: `pathname_unix_sockets_need_abi_9` | TCP to the master port is allowed by design with network on |
| D13 rewind then restart | Yes | `crates/node/tests/runtime.rs` | None | Rewind is files only and recorded as events, so a disagreement is unlikely |
| D14 rootless pasta | No (Podman) | — | — | — |
| D15 `podman exec` kill | No (Podman) | — | — | — |
| D16 deltas in the log | Settled by reading: yes | — | `acceptance_restart.rs` already counts `assistant_delta` events | Drop as an experiment |
| D17 control messages under a full link | Yes | `crates/link` tests or `crates/node/tests/slow_restore.rs` | Partly: `cancel_during_a_slow_restore_is_answered_within_a_second`; the pump reads and writes concurrently (`g3:crates/link/src/pump.rs`) | — |
| D18 Scout calls `write` | Yes | — | Already covered: `agent_tool_list_is_enforced_and_author_set` (`g3:crates/agent-native/src/lib.rs:1397`), `g3:crates/master/tests/agents_http.rs` | Drop |
| D19 secret in a summary | Yes | `crates/agent-native` or `crates/node/tests/runtime.rs` | None | Known secrets are scrubbed at append (`g3:session.rs:202-207`); expected to pass for known secrets only |
| D20 nested `.git` | Yes | `crates/node/src/git.rs` tests | None | `git add -A` records a nested repository as a gitlink, or fails if it has no commit |

## Licence notes

- All "copy from" sources are permissive per `research/round1/00-targets.md` and `round2/00-repos.tsv`: Codex (Apache-2.0), OpenCode (MIT), OpenClaw (MIT), Hermes Agent (MIT), DeepSeek harness (MIT), Cline (Apache-2.0), goose (Apache-2.0), Pi and Oh My Pi (MIT), OpenHands (MIT; its enterprise directory is unverified, and the cited `scripts/` and `src/types/` paths are outside it), OpenCodex as `lidge-jun/opencodex` (MIT, not the AGPL `RyensX/OpenCodex`). Copying code from them still needs `THIRD_PARTY_NOTICES.md`.
- **Warp (AGPL-3.0)** is cited in §4.1 as the model for binding AAD into sealing. Take the idea only; do not port `envelope.rs`. Its other appearances are in "do not copy".
- **Zed (GPL)** appears only in "do not copy". No conflict.

## Weaknesses the report does not mention

1. Writer sub-agents silently share the parent's folder when no worktree can be made or the folder is not a git repository: `g3:crates/node/src/session.rs:676-686`, `:789-798` (also `main:671`, p6). The report names this as other projects' bug.
2. Event appends fail open: `g3:crates/node/src/session.rs:240-244`; the tool still runs (`g3:crates/agent-native/src/lib.rs:927-947`).
3. A revoked node's queued frames (up to 8, plus one reserved) are still applied after revoke, since `append` checks the owner, not revocation: `g3:crates/master/src/nodes.rs:57-66`, `g3:crates/master/src/lib.rs:819-821`.
4. A duplicate `seq` is accepted without comparing content, so two processes holding one node key, or a node restored from an old disk, interleave into one mirrored log without any error: `crates/store/src/lib.rs:546-548` (both branches).
5. The API token bypasses chat privacy and the Origin requirement by design, and nothing records which requests used it beyond the generic audit: `g3:crates/master/src/auth.rs:19-21`, `:137-143`, `:740-751`.
6. The model can raise a bash timeout to an hour regardless of the node's default: `g3:crates/tools/src/bash.rs:126-130`.
7. PR 6's Researcher has no usable tools when delegated on an agentic node: `p6:crates/agents/builtin/researcher.md:5`, `g3:crates/cli/src/main.rs:492-495`.
8. Every coalesced assistant delta is an fsynced append on the node (`Durability::Full`, `g3:crates/cli/src/main.rs:517`), plus a full mirror append on the master; that is write amplification `FIX_PLAN.md` 4.8 should measure, not only bytes.
9. A forced restore overwrites live changes that no snapshot holds: `g3:crates/node/src/git.rs:732-750` skips the check and nothing snapshots first.
10. `edit` and `write` truncate the target in place with no temp file or fsync: `g3:crates/tools/src/fs.rs:226-240`; a crash mid-write loses the user's file (A13 mentions atomic writes inside a larger feature; this part is a small fix on its own).

## Not settled

- Whether genai's OpenAI-Responses and Gemini adapters can hand back a cut-off call as a valid object (Gemini passes `args` as given). Settle with a fixture per adapter (D9).
- Whether project instruction files survive compaction (A11): the system prompt is rebuilt per turn, but I did not trace where instruction files enter it.
- D3, D20 and D1 outcomes are predicted from code, not run.
