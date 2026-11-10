## 1. Executive summary

### What should change next, in order

1. **Bind every approval to the action it approves.** Our answer names an approval id, and "allow for this turn" is keyed on the tool name; pull request 4 narrows when that allowance applies, not what it covers. Codex keys a remembered decision on canonical argv, working directory and permission delta (https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/tools/runtimes/unified_exec.rs#L87-L98, `unchecked`), and OpenClaw binds a one-off approval to argv, working directory, agent, session and a hash of environment overrides, consumes it once and asks again when the node's policy changed (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/invoke-system-run.ts#L349-L357). We need a digest computed on the node, echoed by the answer, used once, and accepted at the master only from the chat's owner. This is the most important fix in the round (section 6, item 1 in `round2/synth/02-approvals-permission-rules.md`).

2. **Fence the node link before building nodes per workspace or fork.** A reconnecting node replaces the old connection with no generation on its frames (`crates/master/src/nodes.rs:207`). Of thirteen projects, only OpenClaw fences a remote executor's writes (6.1 in `round2/synth/05-links-workers-protocols.md`). The design note's per-turn grants and its fork both assume a fence that does not exist yet.

3. **Close cheap holes in the loop on `main`.** Refuse every tool call from a response that stopped on the length limit (Pi does: https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/agent/src/agent-loop.ts#L488-L513); prove the tool-start event is durable before the tool runs (DeepSeek harness); make the failure modes in our approval path fail closed; move step and time limits into `node.toml`; add a repeated-call guard derived from the log.

4. **Change pull request 7's compaction model.** Round 1 said copy Pi. Pi's summary request is sent with `cacheRetention: "none"` (https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/compaction/compaction.ts#L618-L638) and has no check that the summary shrank anything. Copy OpenCode's and DeepSeek's cache-preserving request, Cline's model-free fallback and DeepSeek's shrink check instead.

5. **Measure checkpoints before changing them.** No project in the set bounds a full-coverage checkpoint below a scan of the whole tree without losing coverage, and nobody measured it (item 4 of section 6 in `round2/synth/06-sessions-fork-checkpoints.md`). The risk that `git gc` in the user's repository breaks a shadow-repository restore through alternates is untested.

6. **Tighten the design note in four places:** grants with a TTL of minutes bound to the session's owner epoch, not six hours; a fixed container invocation with no pass-through arguments; pairing that burns the code before validating anything else; and a 128-bit key fingerprint.

7. **Add a staleness guard to `edit` and `write`.** Seven of the eleven edit implementations read have none; of the guards that exist, two only warn and one ships disabled (6.1 in `round2/synth/08-edit-tools-mechanics.md`).

### The general finding

The widest gap between documentation and code is in enforcement. A guard exists, but it is off, unwired, or a label. Three examples:

- OpenCode documents its `explore` agent as read-only; its rules allow `shell *`, and the read-only shell is a prompt (https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/agent.ts#L80-L93, refuted).
- goose documents `--max-tool-repetitions` as a loop guard; the flag is not wired and the inspector is registered with no limit (https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/tool_monitor.rs#L99-L134, refuted).
- DeepSeek harness's read-before-edit guard ships `disabled: true` while its prompt still tells the model to read first (https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/fs/fs-observation-policy/src/index.ts#L78-L103, checked in review).

A second pattern holds across every remote design read: whenever the controlling side sends policy, it can widen the executing side (6.3 in `round2/synth/05-links-workers-protocols.md`). Our "node decides" invariant is on the right side of that. The lesson: test what we ship, not what we document.

### How far this can be trusted

Round 2 read thirteen repositories, each at one pinned commit: 31 files, 1,008 findings and about 6,180 permalinks, every one checked by script to exist. Reviewers opened the cited code for 632 findings. They found none wrong outright and corrected about 65 in place. The other findings were not independently checked. Nothing was built or run, so every behaviour here is read from code, not observed. Claims that something is absent rest on searches. Several repositories hold only part of a system (OpenHands' engine and container code, Warp's server and worker, the Agent Client Protocol schema, hosted relays). Closed products (Claude Code, Claude Desktop, Cursor, Antigravity) appear only through Round 1's documentation survey. Section 11 lists what is open and which conclusions should wait for an experiment.
