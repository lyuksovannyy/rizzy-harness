# Final fidelity review of narrative sections 13, 07 and 08

Reviewer: one pass over `research/round2/synth/_narrative/13-round1-claims.md`, `07-security-isolation.md` and `08-extensibility.md`, checked against the clones at the pinned commits. Counts are approximate where a row was checked by following several permalinks. Nothing was built or run.

## Statements checked

| File | Checked | Of which changed |
|---|---|---|
| 13 | 26 "Refuted" rows (code and Round 1 provenance), 31 "Corrected" rows, 8 prose passages | 5 rows removed, 12 rows reworded, 4 prose passages |
| 07 | about 56 table cells (Codex, OpenCode, OpenClaw, Hermes Agent, Oh My Pi, DeepSeek harness, goose, Cline), about 45 sentences of the per-project paragraphs, 7 failure-mode sub-sections (about 35 bullets) | 17 edits |
| 08 | about 64 table cells for the same eight projects, about 30 other statements, 20 "worth copying" rows, 20 "avoid" rows | 13 edits |

Verdict tally over the changed items: `drifted` 24, `not supported by the cited lines` 11 (five rows whose claim Round 1 never made, six permalinks that did not show the claim), `wrong project or wrong mechanism` 1 (OpenCode `execute` denial presented as an approval bypass). No statement was found to be flatly false.

## Refuted rows that were themselves wrong

| Row | What Round 1 actually said | Action |
|---|---|---|
| Warp: "Agent decides is the default for shell commands" | "Agent decides is the CLI default ... App default per permission not stated" (`round1/synth/02` line 69). The code agrees for the TUI/CLI. | Removed; prose now says the GUI default is `AlwaysAsk`, which Round 1 had not stated. |
| goose: `--max-tool-repetitions` limits repeated calls | "listed but not explained" | Removed from the table; the flag is described as unexplained in Round 1 and not wired in code. |
| Cline: checkpoints work on non-Git folders | The sentence is about our own design (`round1/synth/05` line 606), not about Cline | Removed. |
| OpenCode: per-agent fallback chains with `model_fallback` | That sentence describes rizzy-harness (`round1/own-rizzy-harness.md`), not OpenCode | Removed. |
| Hermes Agent: `execute_code` sandboxed beyond env stripping | Round 1 said only that the child environment is stripped | Removed; the note on the re-labelling was amended. |

## Changed statements

| File | Statement | What the code shows | Verdict |
|---|---|---|---|
| 13 | OpenHands "no plan mode" | Round 1 wrote "nothing found", not "no plan mode"; code has a read-only planner conversation | drifted (reworded, marked as a new finding) |
| 13 | Hermes turn cap "500 (sub-agents 50)" | Round 1 recorded a 50 versus 250 conflict; code: parent unlimited, children 250, 50 stale | drifted |
| 13 | OpenCodex "cooldowns are memory-only" | Round 1 only asked the question | drifted |
| 13 | Cline "no notification on the SDK path" | searched, not found; not run | drifted (softened) |
| 13 | Cline "no spoke code" | `SpokeRecord` and related types exist; no code creates a spoke | drifted |
| 13 | OpenClaw `TOOLS.md`/`HEARTBEAT.md` | Round 1 flagged that the page's summary lists them and its file map does not | drifted |
| 13 | Codex `explorer` "read-only" | wording came from Round 1's synthesis of rosters, not the Codex page; `explorer.toml` is zero bytes (confirmed) | drifted |
| 13 | Codex Landlock row quoted a sentence Round 1 did not write | Round 1: "cannot isolate Unix sockets, exactly our residual"; code: "app-server sockets" only | drifted |
| 13 | Hermes hardline floor "skipped on container back ends" | skipped for isolated back ends; Docker with host bind-mounts keeps the guards; operator deny rules still run | drifted |
| 13 | OpenCode web UI password "corrected" | password claim holds; only the `?auth_token=` query value is new | drifted |
| 13 | OpenHands "secret values stay out of the agent's reach" | Round 1 said "out of chat, agent events and tool arguments"; session key is in the agent environment via the launcher and Docker entrypoint | drifted |
| 13 | OpenHands `ConfirmRisky` default | the Canvas client sends `NeverConfirm`; upstream SDK default not read | drifted (scope) |
| 07 | Codex `exec` "forces" `Never` | `Never` is the default override, rebuilt for auto-review | drifted |
| 07 | Codex Guardian "timeout returns to the human" | timeout is a `TimedOut` decision whose instructions let the agent retry once or ask; only input-budget overflow falls back to the normal approval path | drifted (table, reviewer table, paragraph) |
| 07 | seccomp and bwrap permalinks | pointed at mode selection and PATH search; deny list is at `seccomp.rs` L198-L233, digest pin in `launcher.rs` | not supported by the cited lines |
| 07 | OpenCode "password stays in the environment" | only an environment-supplied password; service mode stores it in a 0600 file | drifted (two places) |
| 07 | goose `readOnlyHint` auto-allows | only when mode is SmartApprove | drifted (three places) |
| 07 | OpenCode `execute` denial listed under "paths that bypass the approval path" | tools become direct and each still asserts its own permission | wrong mechanism |
| 07 | Codex repository skills, `state.rs` L742-L757 | lines are layer accessors; the unit test in `host_roots_tests.rs` and `host_roots.rs` L127-L190 show the untrusted layer's skills root | not supported by the cited lines (repointed) |
| 07 | Oh My Pi `.claude/hooks/` "imported as executable code" | only `.ts` and `.js` files | drifted |
| 07 | Oh My Pi project trust "hard-coded true" | a compatibility stub; the code says there is no trust gate | drifted |
| 07 | Hermes "memory and skill write gate" fails open on import failure | the import-failure path is in `memory_tool.py`; the gate is opt-in; skill path not checked | not supported by the cited lines |
| 07 | Codex loopback listeners reachable from the agent's shell | inference, not run | drifted (labelled) |
| 08 | Hermes MCP environment allow-list | also admits variables from an external secret source | drifted |
| 08 | OpenClaw tool search "always on" | `enabled: setting ?? true`, can be turned off | drifted |
| 08 | goose `readOnlyHint` auto-allows | SmartApprove only | drifted |
| 08 | OpenCode `execute` denial as a "hole" | tools stay individually gated | wrong mechanism |
| 08 | Codex hook trust "re-verified at run time" | plugin bundles only; the declaration hash is computed at discovery | drifted |
| 08 | Codex skills `state.rs` permalink (two places) | see 07 | not supported by the cited lines (repointed) |
| 08 | Cline "agent-created tasks start `pending_approval`" | agent-tool tasks only; hub client commands create approved tasks | drifted |
| 08 | Oh My Pi "behind a trust check hard-coded true" | no gate; compatibility stub | drifted |
| 08 | OpenClaw webhook idempotency permalink | pointed at authentication; the `Map` is at L102-L160 | not supported by the cited lines (repointed) |
| 08 | Hermes "a crash cannot fire a job twice" | true, but the cost is at-most-once (a crash can lose a run) | drifted (qualified) |

## How far the unchanged prose can be relied on

The mechanism claims in all three files are sound. Of the roughly 300 statements checked, about one in eight needed a change, and most changes narrowed a flat statement or repaired a permalink that did not show the claim; none reversed a finding. The pattern is the one the brief predicted: a condition lost ("SmartApprove only", "only an environment-supplied password"), a timeout folded into a denial, and Round 1 credited with claims it never made (five of the 26 "refuted" rows). Negative claims I searched for held: no `requires_approval` field in Cline, no sandbox code in goose, no `tool_permissions` lookup for Zed's ACP agents, no trust check in Codex's skills crate, no `designer` role in Oh My Pi, no spoke implementation in Cline.

Trust the "Refuted" and "Corrected" tables now with one caution: the counts table at the top of file 13 comes from `docs-vs-code.md`, which I did not touch, so it still counts the five removed rows; the prose says so. Treat the `unchecked` items in file 08 (Codex limits and OAuth, the cron journals, OpenClaw delivery queue, instruction-state events) as unverified; I did not re-open them. Permalinks in files 07 and 08 that I did not follow (for example the Warp, Zed and OpenHands paragraphs of file 07 beyond the failure modes) are covered only by the per-repository reviews.
