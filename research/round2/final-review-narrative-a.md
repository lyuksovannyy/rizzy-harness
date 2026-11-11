# Final fidelity review of narrative sections 1, 9 and 10 (part A)

Method: each cited permalink was opened in `.claude/research-repos/<slug>` with surrounding code, then compared with the reviewed finding in `research/round2/<slug>.*.md` and the synthesis group files. Pointers to synthesis headings were resolved to their permalinks. Statements about rizzy-harness itself were skipped. Nothing was built or run.

## Counts

The totals are approximate tallies of statements that cite a permalink or name another project's behaviour; the per-verdict split of the changed items is exact (see the table below).

| File | Statements checked | faithful | drifted | wrong project/mechanism | not supported by cited lines |
|---|---|---|---|---|---|
| `01-executive-summary.md` | 12 | 11 | 0 | 0 | 1 |
| `09-decisions-tradeoffs.md` | 52 (30 decisions, 52 other-project statements) | 36 | 10 | 1 | 5 |
| `10-applications.md` | 96 | 86 | 6 | 0 | 4 |

A statement with a right claim but a permalink whose lines do not show it is counted as "not supported by the cited lines" and was fixed by moving the permalink. About 20 distinct statements were changed. The checker reports 0 invalid citations for all three files (7, 53 and 82 citations).

## Changes

| File / item | Statement | What the code shows | Verdict |
|---|---|---|---|
| 01 and 9.27 | DeepSeek read-before-edit guard "ships `disabled: true`" (permalink on guard logic) | `disabled: true` is in `packages/bundle/base/cordis.patch.yml` L292-L294; claim true, lines wrong. The "no bundle re-enables" part rests on a search of bundle, preset and apps YAML only | not supported by cited lines (permalink fixed) |
| 9.25 | Oh My Pi hashline "anchors on a per-line tag", "565-line rebase path" | One 16-bit xxh32 tag per file, anchors are bare line numbers, the seen-line guard is the per-line protection; 565 was a line number | wrong mechanism |
| 9.25, table (e) | Zed "0.8 per line" at L254-L256 | L254-L256 is a range-level matched-ratio check; per-line `fuzzy_eq` threshold is L368-L379 | not supported by cited lines (permalink fixed) |
| 9.27 | OpenCodex `expectedSha256` refuses on mismatch (workspace-rpc.ts L162-L207) | Those lines are RPC `invoke`; the check is `assertWritePrecondition` in workspace-executor.ts L230-L243 | not supported by cited lines (permalink fixed) |
| 9.30 | Pi `runToolCall` is re-entrant (agent-loop.ts L596-L670) | L596-L670 is the parallel batch executor; `runToolCall` is L800-L829 | not supported by cited lines (permalink fixed) |
| 10 A11 | DeepSeek shrink check (region.ts L431-L468) | L431-L468 is a surface-stability check; the "summary not smaller" comparison is L413-L422 | not supported by cited lines (permalink fixed) |
| 10 A13 | Hermes "full-read rule" (file_tools_write_guards.py L166-L173) | Those lines refuse writes to Hermes config; the full-read blocker is L560-L597 | not supported by cited lines (permalink fixed) |
| 10 A12 | DeepSeek reports `full` "on ABI 6 and later" (main.c L70-L94) | `full` starts at `MAX_ABI` = 5; probe report is L264-L297 | drifted |
| 9.12, table (e) | Warp skips allow-list when `is_risky == Some(false)` | Only in `AgentDecides` mode with the feature flag on (on in normal builds) | drifted |
| 9.28, table (e) | Oh My Pi child `yolo` overlay "keeps only user deny rules" | User approval policy map still applies, prompts fail closed (no UI), a per-spawn override can change the mode | drifted |
| 9.21 | Codex "only project" recording active history explicitly | Synthesis says "explicitly and atomically"; Hermes (`active`) and goose (`agent_visible`) use per-row flags | drifted |
| 9.23 | Hermes 0.50 vs Codex 0.9, "nearly a factor of two" | Hermes floors at 0.75 for windows under 512K (most models); gap is 2x only for 512K+ windows | drifted |
| 9.7 | Codex remembered `allow` runs outside the sandbox | Any explicit allow matching every segment; policy with denied reads is the exception | drifted |
| 9.13 | Hermes withholds scheduling toolset from cron agents | Denied unless `cron.allow_agent_scheduling: true`; shown in `cron/scheduler.py`, not the cited lines | drifted |
| 9.16 | Hermes skips floor and prompts "on container backends" | Docker without host bind-mounts, Modal, Daytona, Vercel sandbox; Docker with bind-mounts takes the normal flow | drifted |
| 9.24 | Cline fallback "after a failed summary" | `basic` runs when agentic throws; an empty summary skips compaction | drifted |
| 9.30, table (e) | goose Code Mode skips the gate | Code Mode is off by default | drifted (qualifier added) |
| 9.30 | Hermes MCP env "eight named variables plus XDG" | Also variables from an external secret source, and a Windows list | drifted |
| 10 A1 | OpenClaw empty approver list "means anyone in the chat" | Helper returns authorized with an implicit same-chat tag; whether every channel honours it was not traced | drifted |
| 10 A8 | OpenHands and Hermes "fall back to the shared tree" | OpenHands falls back on a failed worktree and tells the agent; Hermes defaults to shared, isolation is opt-in and degrades off the local backend | drifted |
| 10 A9 | Hermes thresholds "warn at third, block at fifth" | Warn after 2, notice on the third identical call, block after 5; same-tool failures 3 and 8; hard stops default on only for unattended platforms | drifted |
| 10 A10 | Pi "has no jitter" | True for the agent layer; the dormant provider layer has jitter | drifted |

## How far the unchanged prose can be relied on

The permalinks mostly open on the right code and the claims match the reviewed findings. Mechanisms, numbers (0.66, 0.8, 10 attempts, 3 retries, 150 skills, 18,000 characters) and the approval, sandbox and hook findings held up. The failures were of three kinds: a permalink one function away from the claim (6 cases, fixed), a flat statement that dropped a mode or default condition (13 cases, fixed), and one genuine mechanism error (hashline). Residual risk: statements that cite a synthesis heading instead of a permalink were checked against the synthesis text and sampled in code, not all opened in code; the review labels in the source finding files ("checked in review") were trusted where the cited lines agreed with the claim. A planner can rely on section 10's "copy from" pointers after these fixes, but should reopen the `unchecked` ones (Codex session key, OpenCodex per-request accounting, OpenClaw `fallbacksOverride`) before copying fields.
