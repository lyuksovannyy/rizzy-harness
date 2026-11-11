## 1. Executive summary

This summary was rewritten after the final review, which checked every statement the report makes about our own code against the code on `main` and on pull requests 4, 6 and 7 (`round2/final-review-ours.md`). That review changed the ranking: two of the first draft's top items were already largely done, one was wrong about us, and one it had ranked low turned out to be the most serious. Section 10 keeps the original items with their corrections marked in place.

### What should change next, in order

1. **A tool must not run when its event failed to save.** The tool-start event is written before the tool runs, but an append failure is logged and ignored, so the tool runs anyway (`crates/node/src/session.rs:240-244` on `integration/gate3`). That breaks the rule the whole design rests on: the log is written first. DeepSeek harness gates every model request and tool body on a flush that must succeed (see "Flush before effect" in `round2/synth/01-loop-dispatch-durability.md`). Fail the step instead.

2. **Key "allow for this turn" on what was approved.** It is keyed on the tool name (`crates/agent-native/src/lib.rs:1209-1224`), so approving one command approves every later one in the turn. Codex keys a remembered decision on canonical argv, working directory and permission delta (https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/tools/runtimes/unified_exec.rs#L87-L98, `unchecked`). The rest of what the first draft asked for already exists: an approval id is minted on the node, bound to one pending call, used once, and accepted only from the chat's owner (`crates/node/src/session.rs:972-1049`). An echoed digest would only guard against a lying master, which can approve anything anyway, so it is dropped.

3. **Refuse a repeated sequence number whose content differs.** The mirror already rejects a frame from the wrong node, after a delete, or out of order (`crates/store/src/lib.rs:514-553`), so a replaced connection cannot corrupt a log today. But a duplicate sequence number is accepted without comparing content (`:546-548`): two copies of one node key, or a node restored from an old disk, produce a mixed log with no error. A revoked node's queued frames, at most eight, are also still applied. The full fence (a generation per connection, an epoch per session) belongs with workspace nodes and fork, where OpenClaw is the only model in the set (6.1 in `round2/synth/05-links-workers-protocols.md`).

4. **Give sub-agents their isolation or refuse.** A sub-agent that may write falls back to its parent's folder when it gets no worktree (`crates/node/src/session.rs:676-686`, `:789-798`). The report missed this; the final review found it.

5. **Finish the loop's limits.** `node.toml` already has `[limits]`; the step cap of 50 is still a constant and there is no repeated-call guard. The model can also raise a bash timeout to an hour. A tool call cut off by the length limit is already refused; what is missing is telling the model why.

6. **Add a shrink check and a breaker to pull request 7's compaction.** It already keeps history unchanged on failure and corrects its estimate from the provider's counts. A cache-preserving summary request (OpenCode, DeepSeek harness) is a cost improvement, not a correction.

7. **Make `edit` and `write` safe.** They overwrite in place with no temporary file, and `edit` has no staleness check. Seven of the eleven edit implementations read have none either (6.1 in `round2/synth/08-edit-tools-mechanics.md`).

8. **Measure checkpoints before changing them.** Ours scans the whole tree every turn. No project bounds a full-coverage checkpoint without losing coverage. Whether `git gc` in the user's repository can break a shadow-repository restore is untested, and a forced restore discards changes that no snapshot holds.

9. **Tighten the design note in two places:** a grant lifetime of minutes bound to the session's epoch instead of six hours, and a recorded residual that Landlock cannot make `.git` read-only inside a writable session folder. Its container invocation is already fixed with no pass-through.

### The general finding

The widest gap between documentation and code is in enforcement. A guard exists, but it is off, unwired, or a label. Three examples:

- OpenCode documents its `explore` agent as read-only; its rules allow `shell *`, and the read-only shell is a prompt (https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/agent.ts#L80-L93, refuted).
- goose has a `--max-tool-repetitions` flag that is not wired: the inspector is registered with no limit (https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/tool_monitor.rs#L99-L134, checked in review).
- DeepSeek harness's read-before-edit guard ships `disabled: true` while its prompt still tells the model to read first (https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/bundle/base/cordis.patch.yml#L292-L294, checked in review).

A second pattern holds across every remote design read: whenever the controlling side sends policy, it can widen the executing side (6.3 in `round2/synth/05-links-workers-protocols.md`). Our "node decides" invariant is on the right side of that.

The same lesson applied to this report. Its first draft described our own code from a summary and got a third of its statements about us partly wrong. Test what we ship, not what we describe.

### How far this can be trusted

Round 2 read thirteen repositories, each at one pinned commit: 31 files, 1,008 findings and about 6,180 permalinks, every one checked by script to exist. Reviewers opened the cited code for 632 findings: none was wrong outright and about 65 were corrected in place. The other findings were not independently checked.

A final review then checked the report's own prose. Of about 510 statements in the narrative sections, about 40 had drifted from what the code supports, about 20 cited the wrong lines, and five rows in the table of refuted Round 1 claims were removed because Round 1 had never made them. Of 34 statements about our own code, 9 were accurate, 22 partly right, 2 described work already done and 1 was wrong.

Nothing was built or run, so every behaviour here is read from code, not observed. Claims that something is absent rest on searches. Several repositories hold only part of a system (OpenHands' engine and container code, Warp's server and worker, the Agent Client Protocol schema, hosted relays). Closed products (Claude Code, Claude Desktop, Cursor, Antigravity) appear only through Round 1's documentation survey. Section 11 lists what is open and which conclusions should wait for an experiment.
