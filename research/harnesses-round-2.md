# AI coding harnesses: Round 2 research (source code)

**Research date:** 2026-11-10. **Status:** Round 2 complete, awaiting the owner's review. Nothing in our own code was changed as part of this research.

**What this is.** Round 1 surveyed what seventeen harnesses document. Round 2 read the source code of the thirteen that are open source, each cloned at one pinned commit, to find out how the mechanisms are actually built and where the code differs from the documentation. Claude Code, Claude Desktop, Cursor and Antigravity are closed source and are covered only by Round 1.

**The numbers.** 31 files of findings from 13 repositories; 1008 findings; 6179 GitHub permalinks pinned to a commit, every one verified by script to name the pinned commit, an existing path and an in-range line span. Independent reviewers then opened the cited code for 632 findings by their own count (the files carry 653 "checked" or "corrected" marks): none was found wrong outright and about 65 were corrected in place. Nothing was built or run: every behaviour here is read from code, not observed.

**How the study is laid out.**

| File | Contents |
|---|---|
| `research/harnesses-round-2.md` (this file) | The report: summary, method, architecture per harness, comparison tables per feature group, recurring flows, security, extensibility, trade-offs, what it means for us, limits, sources, and the Round 1 claims the code confirmed or refuted |
| [`harnesses-round-2-appendix.md`](harnesses-round-2-appendix.md) | Per feature group, copied unchanged from the synthesis: the patterns and their costs, every Round 1 claim tested against the code, the full recommendations for us, and what was not settled |
| [`round2/synth/`](round2/synth/) | Ten synthesis files, one per feature group. Each keeps every project's implementation as a separate account with its own permalinks: grouped, not merged |
| [`round2/`](round2/) | The 31 per-repository finding files with their review sections, the list of pinned commits, the briefs the agents worked to, and the citation checker |

**Judgement calls made without asking.** (1) One commit per repository, shallow clones; no history was read. (2) The clones were treated as untrusted data: read only, never built or run, and instructions that files in them address to coding agents were not followed. (3) Findings are grouped by feature but never merged across projects; the detailed evidence stays in the synthesis and per-repository files and this report indexes it. (4) Where a repository holds only part of a system (OpenHands' engine and container code, Warp's server and worker), the missing repositories were not cloned; section 11 lists them. (5) File names follow Round 1: `research/harnesses-round-2.md`.

**How to read a claim.** Each finding carries a confidence line: `traced end to end`, `read the definition` or `inferred`, plus `(checked in review)` or `(corrected in review: ...)` where a reviewer opened the code. Negative claims ("there is no X") rest on searches; they say `absence confirmed` only where a reviewer repeated the search.

## Contents

1. Executive summary
2. Research methodology and repository selection
3. Architecture overview per harness
4. Feature groups and implementation comparisons
5. Detailed source-code investigations
6. Execution flows and architectural patterns
7. Security and isolation mechanisms
8. Extensibility and integration mechanisms
9. Interesting implementation decisions and trade-offs
10. Potential applications to our project
11. Technical limitations and unanswered questions
12. Complete source references
13. Round 1 claims tested against the code

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

## 2. Research methodology and repository selection

Round 2 reads the source code of thirteen open-source harnesses. Each repository was cloned once, at one pinned commit, and every factual claim in this report points either to a line range at that commit or to a section of the synthesis files in `round2/synth/`. Twelve of the thirteen commits were made on 2026-11-10 (OpenCodex's timestamp, 2026-11-11T04:58:42+09:00, is the same day in UTC). The DeepSeek harness commit is from 2026-11-04.

### 2.1 Which repositories were read, and why these thirteen

Round 1 surveyed seventeen harnesses from public documentation only. Ten were named by the project owner and seven were added by a scouting stage (see section 2 of `research/harnesses-round-1.md`). Round 2 takes the thirteen whose source code is public.

| Origin in Round 1 | Read in Round 2 | Not read |
|---|---|---|
| Named by the owner (ten) | Codex, OpenCodex, OpenCode, DeepSeek harness, Oh My Pi, Pi | Claude Code, Claude Desktop, Cursor, Antigravity |
| Added by the scout (seven) | OpenHands, OpenClaw, goose, Hermes Agent, Zed, Warp, Cline | none |

Round 1 added the seven because the reader's harness is a self-hosted Rust harness with a web dashboard and a master and node split. The reasons recorded in Round 1:

| Addition | Reason recorded in Round 1 |
|---|---|
| OpenHands | The same product shape: a self-hosted web control centre over pluggable execution backends. |
| OpenClaw | The same topology: one gateway with paired nodes that declare capabilities, exec approvals and a web control UI. |
| goose | A mature Rust harness under foundation governance, ACP-first. |
| Hermes Agent | Strongest on memory and agent-written skills; a credential-injecting egress proxy. |
| Zed | Rust, per-tool permissions, and the protocol most harnesses now speak. Copyleft: ideas only. |
| Warp | Rust, a cloud agent fleet with a published API. AGPL client: ideas only. |
| Cline | The reference approval and checkpoint experience among open IDE agents, now also remote. |

Three Round 1 resolutions carry over and matter for how the findings are read.

- **OpenCodex** is `lidge-jun/opencodex`, a local provider gateway with a web dashboard. Round 1 chose it with medium-low confidence and asked the owner to confirm. The alternative, `RyensX/OpenCodex`, was not read in Round 2. Round 2 found that OpenCodex has no agent loop of its own; see section 2 of `round2/opencodex.md`.
- **OpenCode** is read at its V2 line only. The V1 engine package is not in the tree at the pinned commit; V1 survives as migration code. See section 1 of `round2/opencode.safety.md`.
- **Codex** is the open part of the Codex family (CLI, app-server, SDK). The IDE extension, the cloud service and the desktop app are not in the repository.

The pinned commits and the size of the findings per repository are below. "Files" is the number of finding files written; "Findings" and "Permalinks" are counts from `round2/synth/_parts/stats.json`. The last column counts findings that carry a review mark, as a script counted them: findings marked `checked in review`, and findings marked `corrected in review`.

| Project | org/repo | Pinned commit | Commit date | Files | Findings | Permalinks | Marked checked / corrected in review |
|---|---|---|---|---|---|---|---|
| Codex | `openai/codex` | `82e6bacb9c69` | 2026-11-10T19:52:49Z | 3 | 94 | 638 | 62 / 11 |
| OpenCode | `anomalyco/opencode` | `05d347d75054` | 2026-11-10T20:28:23Z | 3 | 96 | 498 | 67 / 4 |
| OpenClaw | `openclaw/openclaw` | `8bac830dcb8f` | 2026-11-10T20:27:14Z | 3 | 103 | 511 | 66 / 15 |
| Hermes Agent | `NousResearch/hermes-agent` | `95fc3e668fc1` | 2026-11-10T10:58:48-08:00 | 3 | 100 | 665 | 54 / 6 |
| Oh My Pi | `can1357/oh-my-pi` | `e325ffc73ab8` | 2026-11-10T21:11:46+01:00 | 3 | 101 | 590 | 68 / 5 |
| Pi | `earendil-works/pi` | `91d9719b4123` | 2026-11-10T21:22:05+01:00 | 3 | 95 | 598 | 63 / 7 |
| DeepSeek harness | `deepseek-ai/deepseek-harness` | `fcc0ad462172` | 2026-11-04T16:40:00+08:00 | 3 | 80 | 549 | 61 / 4 |
| goose | `aaif-goose/goose` | `14bf51817b79` | 2026-11-10T19:00:38Z | 3 | 105 | 635 | 63 / 3 |
| Cline | `cline/cline` | `500a40e7b766` | 2026-11-10T12:29:54-08:00 | 2 | 74 | 478 | 56 / 8 |
| OpenHands | `OpenHands/OpenHands` | `d7293dfd671e` | 2026-11-10T21:12:50+01:00 | 2 | 67 | 392 | 45 / 5 |
| Zed | `zed-industries/zed` | `3def8bad0b62` | 2026-11-10T18:56:15Z | 1 | 26 | 185 | 15 / 2 |
| Warp | `warpdotdev/warp` | `4b790f5fac9a` | 2026-11-10T14:14:26-05:00 | 1 | 29 | 214 | 15 / 1 |
| OpenCodex | `lidge-jun/opencodex` | `ddee2c20030c` | 2026-11-11T04:58:42+09:00 | 1 | 38 | 226 | 18 / 3 |
| **Total** | 13 repositories | | | **31** | **1,008** | **6,179** | **653 / 74** |

Reviewers' own section 8 headers add up to 632 checked items (the synthesis brief quotes the same figure). The script count of 653 marks is higher; the two were not reconciled. Section 2.5 gives the other discrepancies.

### 2.2 What was not read

**Closed source.** Claude Code, Claude Desktop, Cursor and Antigravity have no public source. Round 1 covered them from documentation only, and nothing in Round 2 confirms or refutes those documentation claims against code. Round 1 notes that the public `anthropics/claude-code` repository holds the changelog, plugins and examples, not the product source (see section 4 of `research/round1/00-targets.md`).

**Repositories that hold only part of the system.**

- **OpenHands.** `OpenHands/OpenHands` at the pinned commit is the Agent Canvas client and its launchers: a React and TypeScript browser UI, Node scripts that start and front the backend services, an Electron shell, a Dockerfile that layers the UI onto the upstream agent-server image, and a Helm chart. The repository's own note says Canvas is not responsible for executing agent actions ([architecture.md L14-L19](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/docs/architecture.md#L14-L19)). The agent server is pulled as a pinned PyPI package through `uvx` ([dev-safe.mjs L576-L596](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-safe.mjs#L576-L596)). Not in the repository: the agent loop, tool dispatch and executors, the security analyzers and confirmation enforcement, the agent server and its authentication, the per-conversation Docker runtime and its `docker run` flags, the warm sandbox logic, and the Automation, Cloud and Enterprise servers. Every OpenHands statement in this report is about the client, and a reviewer confirmed the structural claim by listing the tree (see section 8 of `round2/openhands.safety.md`).
- **Warp.** The agent loop is not in the client repository. Each model turn is one HTTP request to Warp's server ([impl.rs L16-L26](https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/agent/api/impl.rs#L16-L26)), and the wire schema lives in a separate crate ([Cargo.toml L351-L351](https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/Cargo.toml#L351-L351)). The server holds the system prompt, the model call, the tool-call decisions, model routing and summarisation; the client executes tool calls, applies its permission policy and redacts what it sends. See section 2 of `round2/warp.md`.

**Other repositories a complete study would need.** These are named in the finding files and were not cloned.

| Needed for | Repository or package | Where the file says so |
|---|---|---|
| OpenHands engine, analyzers, Docker runtime, agent server | `OpenHands/software-agent-sdk` | section 1 of `round2/openhands.safety.md` |
| OpenHands wire API | `OpenHands/typescript-client` | section 1 of `round2/openhands.safety.md` |
| OpenHands schedules and webhooks | `OpenHands/automation` | section 1 of `round2/openhands.safety.md` |
| OpenHands skills and plugins registry | `OpenHands/extensions` | section 1 of `round2/openhands.safety.md` |
| OpenHands Cloud and Enterprise servers | not identified | section 1 of `round2/openhands.safety.md` |
| Warp wire schema | `warpdotdev/warp-proto-apis` | section 6 of `round2/warp.md` |
| Warp self-hosted worker | `oz-agent-worker` (a separate Go repository) | section 6 of `round2/warp.md` |
| Hermes Agent relay connector | `NousResearch/gateway-gateway` | section 1 of `round2/hermes-agent.sessions.md` |
| OpenCode remote tunnel | `@opentunnel/client` (npm package) | section 6 of `round2/opencode.sessions.md` |
| Zed ACP schemas | `agentclientprotocol/agent-client-protocol` (crates `agent-client-protocol` and `agent-client-protocol-schema`) | section 6 of `round2/zed.md` |
| goose ACP framing | crates `agent-client-protocol` and `agent-client-protocol-http` (external dependencies) | section 1 of `round2/goose.sessions.md` |
| Codex MCP OAuth details | the `rmcp` crate | section 6 of `round2/codex.sessions.md` |
| Codex Handoff | closed-source desktop app | section 6 of `round2/codex.sessions.md` |
| Oh My Pi hosted collab relay | Go source, not in the repository | section 1 of `round2/oh-my-pi.sessions.md` |
| Cline desktop UI, Kanban app and cloud agent back ends | not in the repository (the cloud back ends are closed) | section 1 of `round2/cline.agents-sessions.md` and of `round2/cline.safety.md` |

### 2.3 The four stages

| Stage | Output | Brief |
|---|---|---|
| 1. Source reading | 31 finding files in `research/round2/` | `ROUND2-BRIEF.md` |
| 2. Citation check | a checker run at the end of every file | `check_citations.py` |
| 3. Independent review | a section 8 in every file, with corrections applied in place | `R2-REVIEW-BRIEF.md` |
| 4. Synthesis by feature group | ten group files in `research/round2/synth/` | `R2-SYNTH-BRIEF.md` |

**Stage 1: source reading in three topic clusters.** One agent read one repository for one cluster. The three clusters were:

- **safety**: execution and safety. The loop and tool dispatch, approvals and permission rules, sandboxing, running commands elsewhere (containers, SSH, remote back ends), secrets, and hooks in the permission path. The safety file for a repository also carries the general architecture overview, which is the source of section 3 of this report.
- **agents**: context, agents and the mechanics behind the scenes. Prompt assembly, edit tools, compaction, memory and instruction files, sub-agents, model roles, retry, stuck-loop detection and plan mode.
- **sessions**: sessions, remote operation and extensibility. Session storage, resume and fork, checkpoints, the client and server split, remote workers, headless and scheduled operation, MCP as a client, skills and plugins, and accounts.

The rules the agents worked under:

- The clones are untrusted data. They were read only (read, search, `git log -1`, `ls`, `wc`). Nothing was built, installed, executed or imported, and no script, test or package manager from a clone was run.
- Instruction files inside a clone (`AGENTS.md`, `CLAUDE.md`, rules, skills, prompts) are part of what is studied. They were not followed.
- Documentation in a repository could be read to find the way, but a finding had to rest on code. Where the documentation and the code disagree, that disagreement was to be reported.
- Every significant finding carries a permalink to a narrow line range the agent read, and one line saying what those lines do.
- Each finding records what the code does, a trace from the entry point to the decision, edge cases and failure handling, a verdict on the Round 1 claim (`confirmed`, `corrected`, `refuted`, `not in code` or `new (not documented)`), a line for the reader's harness, and a confidence (`traced end to end`, `read the definition` or `inferred`).
- If a topic does not exist in a repository, the file says so in one line with what was searched. The target was 25 to 40 findings per cluster; the files hold 23 to 45 findings each.

**Stage 2: the citation check.** `check_citations.py` verifies, for every GitHub permalink in a file, that the repository is one of the pinned clones, that the commit is the pinned one, that the path exists in the clone and that the line range lies inside the file. It also reports GitHub links that are not pinned to a commit. This proves that the cited lines exist. It does not prove that they say what the finding claims. Whether the lines say it is the reviewers' job (stage 3).

**Stage 3: independent review.** Reviewers who did not write a file opened the cited code in the clone, read the surrounding function, followed the trace at least one hop and gave each finding a verdict: `supported`, `partly`, `not supported`, `wrong`, or for negative claims `absence confirmed` or `absence not confirmed`. They had to check at least 15 findings per file, chosen in this order:

1. findings that refute or correct a Round 1 claim;
2. findings about approvals, sandboxing, credentials, authentication, remote workers, links and fencing, and findings the author marked as worth copying;
3. findings that state a number, a default, or "always", "never" or "nothing checks";
4. findings marked `inferred` or `read the definition` whose wording is categorical;
5. negative claims, which the reviewer had to search for in the clone before accepting.

Reviewers edited the file in place: they fixed every `partly` and `wrong` finding, marked each supported finding `(checked in review)`, marked each corrected finding `(corrected in review: <what changed>)`, and appended a section 8 with the count checked, a table of verdicts and a trust judgement for the unchecked rest. They did not add findings or touch other files.

The review selection is deliberately not random. It prefers the riskiest claims, so the checked set is biased toward documentation refutations, security-relevant mechanisms and numbers. The unchecked remainder is the descriptive part. Each section 8 states how far the reviewer trusts it.

**Stage 4: synthesis by feature group.** One agent per group merged the findings of all thirteen repositories into one file. The rules were:

- group, do not deduplicate: where five projects do something five ways, the file holds five separate accounts, each with its own permalinks;
- do not add findings that are not in the input files; a cited file in a clone may be opened only to resolve a contradiction between input files or to confirm a citation the agent is about to rely on;
- carry the trust label of each claim (`checked in review`, `corrected in review` with the corrected text, `unchecked`, `inferred`), and use `absence confirmed` only where a reviewer searched;
- where a repository holds only part of the system (OpenHands, Warp), say what is and is not in the code that was read.

Every group file has the same seven sections: scope, implementations by project, comparison, patterns and trade-offs, documentation versus code, advice for the reader's harness, and what is not settled.

| Group file | Subject | Size (bytes) | Permalinks |
|---|---|---|---|
| `01-loop-dispatch-durability.md` | loop, tool dispatch, durability | 207,668 | 531 |
| `02-approvals-permission-rules.md` | approvals and permission rules | 410,561 | 1,108 |
| `03-sandbox-containers-execution.md` | sandboxes, containers, execution | 129,739 | 303 |
| `04-credentials-secrets-auth.md` | credentials, secrets, authentication | 154,595 | 380 |
| `05-links-workers-protocols.md` | links, workers, protocols | 190,623 | 402 |
| `06-sessions-fork-checkpoints.md` | sessions, fork, checkpoints | 171,656 | 384 |
| `07-context-compaction-memory.md` | context, compaction, memory | 321,515 | 736 |
| `08-edit-tools-mechanics.md` | edit tools and mechanics | 169,693 | 347 |
| `09-subagents-models-retry.md` | sub-agents, models, retry | 209,072 | 395 |
| `10-extensibility-unattended.md` | extensibility, unattended operation | 211,286 | 566 |
| **Total** | | **2,176,408** | **5,152** |

### 2.4 The totals

- 13 repositories, 31 finding files, 2,711,151 bytes of findings.
- 1,008 findings and 6,179 permalinks (from `stats.json`).
- Reviewers' section 8 headers record 632 findings checked. The script count of review marks is 653 `checked in review` and 74 `corrected in review`.
- No section 8 table records a verdict of `wrong` or `not supported`. A count of table rows whose verdict begins with `partly` gives 65 (a count made while writing this section). The synthesis brief says about 55 findings were corrected in place.
- Ten group files, 2,176,408 bytes, 5,152 permalinks.

### 2.5 Judgement calls and limits

- **One commit per repository.** Each clone is shallow and holds one commit. Everything read is the state of the default branch at that commit. These repositories change daily, so a statement about a mechanism is a statement about that commit. The Zed clone has no history, so why its per-prompt checkpoint code was removed could not be examined; see section 6 of `round2/zed.md`.
- **Nothing was executed.** Every claim is a reading of code. A claim about an exploit (for example, an unauthenticated local page starting a session in Cline's hub) is a reading, not a demonstration. Reviewers could not run code either.
- **Two waves.** At most twenty agents can run at once, and 31 files were needed. The second-wave task file lists eight tasks: `goose.sessions`, `openhands.safety`, `openhands.agents-sessions`, `cline.safety`, `cline.agents-sessions`, `zed`, `warp` and `opencodex`. The files do not record the composition of the first wave.
- **OpenHands and Cline have two files, not three.** For both, the agents and sessions clusters were given to one agent as a single task. The task files do not record the reason. For OpenHands, the engine is not in the repository, so there is less to read in each cluster.
- **Zed, Warp and OpenCodex have one file each.** Their task files give a focused scope (agent-related crates for Zed; for Warp, a client whose server, control plane and cloud orchestration are closed services; for OpenCodex, a gateway that is not an agent harness), not a whole cluster.
- **Absence is weak evidence unless searched.** A "the repository has no X" claim carries `absence confirmed` only where a reviewer searched the clone. Elsewhere it means the author found none.
- **Documentation was a map, not evidence.** Where the files compare the code with Round 1, the Round 1 claim came from public documentation.
- **Known inconsistencies in the inputs.** The synthesis brief quotes 6,144 permalinks and about 55 corrections; `stats.json` counts 6,179 permalinks and 74 corrected marks, and the section 8 tables give 65 `partly` rows. The 653 versus 632 figures above are a fourth. The numbers in this report are the `stats.json` ones, with the others named where they appear.

## 3. Architecture overview per harness

This section describes each harness from the architecture sections of its finding files: the languages, the main packages, the process model, and where the loop, tool dispatch, permission decisions and compaction sit in the code. Each location carries a permalink. The architecture statements are the authors' reading of the entry points. A reviewer checked individual findings, not every sentence of an architecture section, so a statement here is unchecked unless it says otherwise.

### 3.1 Summary

| Project | Language | Process model | Where the loop runs | OS sandbox | Session storage |
|---|---|---|---|---|---|
| Codex | Rust | one binary; TUI and `exec` embed an app-server, or a long-lived `codex app-server`; optional exec-server | `core` crate, in the app-server process | yes | JSONL rollout per thread, SQLite index |
| OpenCode | TypeScript (Bun, Effect), one Rust PTY daemon | one server process per machine; TUI, web, desktop, CLI and SDK are clients | `packages/core`, in the server | no | SQLite; optional event table, off in the CLI |
| OpenClaw | TypeScript (Node), Rust node host crate | one Gateway process; nodes and cloud workers connect to it | the Gateway, or a cloud worker that dials it | containers only (opt-in) | SQLite (per-agent and shared databases) |
| Hermes Agent | Python | one gateway owner process per profile; CLI, TUI, dashboard and ACP are viewers | the gateway process (`AIAgent`) | containers only (Docker; also SSH, Modal and Daytona back ends) | SQLite `state.db` |
| Oh My Pi | TypeScript (Bun), Rust N-API addon | one process; RPC, ACP, print and collab embed it | in process, `packages/agent` | no | JSONL tree per session |
| Pi | TypeScript (Node) | one process per session | in process, `packages/agent` | no | JSONL tree per session |
| DeepSeek harness | TypeScript (Node), a small C launcher | one process of Cordis plugins; the Electron app spawns it | in the host process, `agent-loop` | yes (write confinement) | checksummed zstd JSONL frames |
| goose | Rust, Electron desktop client | one binary (CLI, `goose serve`, `goose acp`); desktop speaks ACP over WebSocket | state machine in the process that serves the session | no | SQLite |
| Cline | TypeScript (Bun, Node) | SDK in process, or a per-machine hub daemon over WebSocket | `AgentRuntime` in the calling process or in the hub | no | JSON files plus SQLite index |
| OpenHands | TypeScript and React client | browser dials Agent Servers; server in another repository | not in this repository | containers only (server side, not read) | not in this repository |
| Zed | Rust | editor process; external ACP agents as child processes | `Thread::run_turn` in the editor process | yes | not read (thread storage); edit log in memory |
| Warp | Rust | GUI app, TUI and CLI; the loop is on the vendor's server | server side, not in this repository | no (a Docker `sbx` sandbox behind a dogfood flag) | server holds the conversation; local SQLite table |
| OpenCodex | TypeScript (Bun), Rust relay binary | one process with up to four listeners | no agent loop; bounded helper loops inside a request | executor only (bubblewrap for Remote Workspace `exec`) | none read |

The sandbox column comes from the table "Mechanism per OS and behaviour when unavailable", and the storage column from the table "Storage, durability, integrity, resume, retention". See section 3.1 of `round2/synth/03-sandbox-containers-execution.md` and section 3.1 of `round2/synth/06-sessions-fork-checkpoints.md`.

### 3.2 Codex

**Languages and packages.** A Rust workspace of roughly 100 to 120 crates under `codex-rs/` (the two reading files count differently). The loop and most decisions are in `core`; the patch engine is the `apply-patch` crate; storage is `rollout`, `thread-store` and `state`; the protocol is `app-server-protocol`; the sandbox is `sandboxing` and `linux-sandbox`; the remote executor is `exec-server`.

**Process model.** One process (`codex` TUI, `codex exec` or `codex app-server`) embeds a JSON-RPC app-server that owns the loop for every loaded thread. The TUI and `exec` talk to it in process through one client facade ([lib.rs L1-L22](https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-client/src/lib.rs#L1-L22)). IDEs and SDKs use a separate `codex app-server` over stdio, a Unix socket or a WebSocket ([mod.rs L84-L89](https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/mod.rs#L84-L89)). If an environment (an exec-server) is selected, the loop stays where it is and only process, filesystem and MCP stdio work moves to the executor. On Linux a sandboxed command runs through a helper that starts bubblewrap and re-enters itself to apply seccomp.

**Where decisions live.**
- Loop: `run_turn` repeats "drain pending input, capture a step context, record the world-state diff, sample, run tools, decide follow-up" ([turn.rs L543-L554](https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/session/turn.rs#L543-L554)). One sampling step is at [turn.rs L647-L690](https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/session/turn.rs#L647-L690).
- Tool dispatch: a per-turn read and write lock; tools that declare parallel support take the read lock, all others the write lock ([parallel.rs L292-L304](https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/tools/parallel.rs#L292-L304)).
- Permissions: `ToolOrchestrator::run` handles skip, forbidden, needs approval and sandbox retry ([orchestrator.rs L164-L233](https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/tools/orchestrator.rs#L164-L233)). Command rules are in `core/src/exec_policy.rs` over the Starlark `execpolicy` crate; the model-based approver is `core/src/guardian/`.
- Compaction: the auto-compact limit is the smaller of the configured limit and 0.9 of the window ([context_window.rs L62-L120](https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/session/context_window.rs#L62-L120)), checked from `run_turn`.
- Storage: a storage-neutral trait ([store.rs L129-L135](https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/thread-store/src/store.rs#L129-L135)).

**Not in the repository.** The IDE extension, the cloud service and the desktop app. Handoff (moving a chat with its Git state) is closed-source desktop code (absence confirmed; see "Session moves" in `round2/synth/05-links-workers-protocols.md`). The remote-control relay and the expiry of its pairing codes are server side ([protocol.rs L21-L56](https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/remote_control/protocol.rs#L21-L56)). The Windows sandbox crates were not read beyond their dispatch.

### 3.3 OpenCode

**Languages and packages.** TypeScript on Bun, written in Effect. `packages/core` is the engine, `packages/server` the HTTP API, `packages/cli` the binary and headless `run`, `packages/codemode` a JavaScript-subset interpreter for the `execute` tool, `packages/plugin` the plugin types. `packages/process-daemon` is a Rust PTY daemon for user terminals, not on the agent tool path. Only the V2 line exists in the tree.

**Process model.** One server process owns every session, the loop, every tool and every plugin, and runs as the host user. It has three modes (foreground, a shared background service, and a private stdio mode) ([server-process.ts L23-L23](https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/cli/src/server-process.ts#L23-L23), [server-process.ts L87-L96](https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/cli/src/server-process.ts#L87-L96)). Child processes are spawned directly, detached into their own process group ([shell.ts L370-L382](https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell.ts#L370-L382)). The HTTP API is declared once in `packages/protocol` ([server.ts L56-L79](https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/protocol/src/groups/server.ts#L56-L79)). Built-in agents, tools, plan mode and the policy enforcer are internal plugins activated in a fixed order ([supervisor.ts L94-L98](https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/supervisor.ts#L94-L98)).

**Where decisions live.**
- Loop: `SessionRunner.drain` ([llm.ts L214-L222](https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/llm.ts#L214-L222)) and `runStep` ([llm.ts L235-L384](https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/llm.ts#L235-L384)).
- Tool dispatch: `Tool.snapshot` and `execute` in `core/src/tool.ts` ([tool.ts L389-L392](https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool.ts#L389-L392)). The registry authorises nothing; each built-in tool calls `Permission.assert` itself.
- Permissions: rules are a flat list where the last match decides ([permission.ts L184-L210](https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/permission.ts#L184-L210)).
- Compaction: the ceiling is the window minus a buffer ([compaction.ts L1163-L1186](https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L1163-L1186)), called from `runStep`.
- State: one SQLite file in WAL mode ([database.ts L42-L46](https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/database/database.ts#L42-L46)); sessions are projections of an event stream ([sql.ts L28-L80](https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/sql.ts#L28-L80)).

**Not in the repository.** The V1 engine package (only migration code remains) and the `@opentunnel/client` package used for the remote tunnel. The web app, TUI, desktop shell and the cloud console were not read.

### 3.4 OpenClaw

**Languages and packages.** TypeScript on Node in a pnpm monorepo: `src/` (Gateway, agents, infra, node host), `packages/` (`agent-core` is the loop, `gateway-protocol` the schemas), `extensions/` (plugins). A Rust node host exists in `crates/openclaw-node-host` ([Cargo.toml L1-L13](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/crates/openclaw-node-host/Cargo.toml#L1-L13)). The loop is adapted from Pi ([THIRD_PARTY_NOTICES.md L18-L26](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/THIRD_PARTY_NOTICES.md#L18-L26)).

**Process model.** One Gateway process owns sessions, runs the loop in process (the embedded runner) and runs tools. It can hand a turn to a cloud worker that dials back over WebSocket on `/__openclaw__/worker` ([server-http-upgrades.ts L257-L294](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server-http-upgrades.ts#L257-L294)); worker frames go to a dedicated handler ([worker-connection.ts L99-L100](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/ws-connection/worker-connection.ts#L99-L100)) and a closed dispatcher ([worker-connection-dispatch.ts L90-L121](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/ws-connection/worker-connection-dispatch.ts#L90-L121)). Paired nodes connect with `role: node`. State is two SQLite databases ([openclaw-agent-schema.sql L1-L4](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/state/openclaw-agent-schema.sql#L1-L4)).

**Where decisions live.**
- Loop: `runLoop` is an unbounded `while (true)` ([agent-loop.ts L250-L253](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/agent-core/src/agent-loop.ts#L250-L253)).
- Tool dispatch: an ordered `before_tool_call` chain ([agent-tools.before-tool-call.policy.ts L1-L7](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/agent-tools.before-tool-call.policy.ts#L1-L7)).
- Permissions: exec policy has three axes, host, security and ask ([exec-approvals-core.ts L13-L17](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/exec-approvals-core.ts#L13-L17)). The Gateway decides for its own host ([bash-tools.exec-host-gateway.ts L112-L130](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/bash-tools.exec-host-gateway.ts#L112-L130)); a node decides again ([invoke-system-run.ts L334-L357](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/invoke-system-run.ts#L334-L357)). The sandbox is the Docker or Podman CLI ([container-allocation.ts L54-L75](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/container-allocation.ts#L54-L75)).
- Compaction: threshold in [compaction-threshold.ts L1-L14](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/agent-core/src/harness/compaction/compaction-threshold.ts#L1-L14), driven from the session ([agent-session-compaction.ts L638-L695](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sessions/agent-session-compaction.ts#L638-L695)).

**Not in the repository, or not read.** The repository is about 1 GB and `src/agents` alone has more than 2,000 files; only the named paths were read. Windows paths, the OpenShell and Crabbox sandbox back ends and most of the Rust node host were not read.

### 3.5 Hermes Agent

**Languages and packages.** A large Python monorepo with no compiled component in what was read. One `AIAgent` class (`run_agent.py`) is the agent; background review, the curator, delegated children and the compaction summary are second `AIAgent` instances or auxiliary model calls, not a separate runtime. The system prompt is built in three tiers ([system_prompt.py L751-L826](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/system_prompt.py#L751-L826)).

**Process model.** The gateway process owns execution: the loop, the cron ticker and the Kanban dispatcher. The CLI, TUI, desktop, ACP adapter and dashboard are viewers that attach through a local control socket, mint a single-use 30 s ticket, and dial the gateway WebSocket ([control_socket.py L1-L9](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/control_socket.py#L1-L9)). At boot the gateway starts a new runtime epoch and fences lost turns ([session_authority.py L1450-L1467](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/session_authority.py#L1450-L1467)). A "terminal backend" decides only where shell commands run (local, Docker, SSH, Modal, Daytona) ([base.py L263-L282](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/base.py#L263-L282)). State is one SQLite `state.db` per profile at schema version 33 ([hermes_state_common.py L294-L295](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_common.py#L294-L295), [hermes_state_common.py L467-L568](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_common.py#L467-L568)).

**Where decisions live.**
- Loop: a `while` over the iteration count ([conversation_loop.py L1748-L1753](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/conversation_loop.py#L1748-L1753)), one tool round per model response ([conversation_loop.py L1781-L1798](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/conversation_loop.py#L1781-L1798)).
- Tool dispatch: `_dispatch_authorized_once` ([tool_executor.py L679-L752](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/tool_executor.py#L679-L752)). Command approval is not in this path; it is inside the `terminal` tool and `execute_code`.
- Permissions: `check_all_command_guards` ([approval.py L1276-L1323](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval.py#L1276-L1323)).
- Compaction: trigger computed in [context_compressor.py L2751-L2755](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/context_compressor.py#L2751-L2755), defaults in [config_defaults.py L563-L595](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/config_defaults.py#L563-L595).

**Not in the repository, or not read.** The relay connector (`NousResearch/gateway-gateway`, a separate Node repository), 28 platform adapters except the base one, the TUI and desktop clients, and the `vercel_sandbox` and `managed_modal` back ends beyond a search.

### 3.6 Oh My Pi

**Languages and packages.** TypeScript on Bun ([package.json L563-L565](https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/package.json#L563-L565)) with a Rust N-API addon, `pi-natives`, which exports the embedded bash engine `pi-shell`, PTYs, search, AST and diff work ([shell.rs L1-L26](https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-natives/src/shell.rs#L1-L26)). The edit engines are Rust in `crates/pi-edit` ([mod.rs L1-L3](https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/modes/hashline/mod.rs#L1-L3)). `packages/agent` is the loop, `packages/coding-agent` the product (session, tools, extensions).

**Process model.** One Bun process runs the TUI (or the RPC, ACP or print host), the loop, every tool and every extension. There is no executor process and no privilege boundary between the loop and its tools. Sub-agents are in-process sessions, moved to worker threads above eight concurrent ([settings.ts L438-L458](https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/task/settings.ts#L438-L458)). The separate server processes are optional: the auth broker, the auth gateway, the stats server, a browser relay, an LSP multiplexer and an external `ovnfs` daemon.

**Where decisions live.**
- Loop: `runLoopBody`, a `while (true)` bounded by an optional deadline, abort signals and a required-tool escalation cap ([agent-loop.ts L1679-L1685](https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/agent/src/agent-loop.ts#L1679-L1685), [agent-loop.ts L1733-L1743](https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/agent/src/agent-loop.ts#L1733-L1743)).
- Tool dispatch: `prepareToolCallDispatch` validates arguments, runs the `beforeToolCall` hook and revalidates revised arguments ([agent-loop.ts L3187-L3234](https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/agent/src/agent-loop.ts#L3187-L3234)).
- Permissions: every registry tool is wrapped in `ExtensionToolWrapper`, the only place the approval gate runs ([sdk.ts L3740-L3747](https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/sdk.ts#L3740-L3747), [sdk.ts L3889-L3894](https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/sdk.ts#L3889-L3894)); the decision is in [approval.ts L346-L390](https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/approval.ts#L346-L390).
- Compaction: a policy loop in `SessionMaintenance` ([session-maintenance.ts L203-L217](https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-maintenance.ts#L203-L217)) and an ordered method chain ([compaction-methods.ts L11-L51](https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/compaction-methods.ts#L11-L51)).

**Not in the repository.** The hosted collab relay; its Go source is not in the repository.

### 3.7 Pi

**Languages and packages.** A TypeScript monorepo (Node 22.19 or later): `packages/ai` (providers), `packages/agent` (the loop), `packages/coding-agent` (the `pi` CLI, session file, extensions), `packages/codemode` (QuickJS in WebAssembly), `packages/mcp`, `packages/durable` (an experimental second harness with its own storage), `packages/env` (an SSH executor with a Rust daemon) and `apps/p2` (an experimental web UI over `durable`).

**Process model.** One Node process per session runs the TUI or RPC front end, the loop, every tool and every extension. Shell commands are spawned as children. There is no server, no tool broker and no sandbox process. RPC mode is a child process driven over stdio; `AgentSessionRuntime` owns the current session and rebuilds it for new, resume, fork and import ([agent-session-runtime.ts L67-L95](https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/agent-session-runtime.ts#L67-L95)). Sub-agents exist only as an example extension that spawns another `pi` process.

**Where decisions live.**
- Loop: `runLoop` ([agent-loop.ts L172-L330](https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/agent/src/agent-loop.ts#L172-L330)).
- Tool dispatch: `prepareToolCall` ([agent-loop.ts L718-L787](https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/agent/src/agent-loop.ts#L718-L787)).
- Permissions: there is no permission system. The only hook point is `beforeToolCall` and `afterToolCall` ([agent-session.ts L714-L715](https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/agent-session.ts#L714-L715)). Trust is a project gate for resources, not a per-call decision ([project-trust.ts L47-L97](https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/project-trust.ts#L47-L97)).
- Compaction: `shouldCompact` ([compaction.ts L126-L130](https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/compaction/compaction.ts#L126-L130)), with the product logic in `agent-session.ts`.
- Session storage: a JSONL tree ([session-manager.ts L41-L62](https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/session-manager.ts#L41-L62)).

**Not in the repository.** There is no `pi-web-ui` package at this commit. The only web UI is `apps/p2`, which carries a status warning ([README.md L1-L24](https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/apps/p2/README.md#L1-L24)).

### 3.8 DeepSeek harness

**Languages and packages.** TypeScript on Node (ESM). Everything is a Cordis plugin, and a "profile" is a YAML list of plugin rows stacked from bundle patches. A small static C launcher, `landlock-run`, is the only native confinement code. Packages include `core/agent-loop`, `core/tools`, `sandbox`, `compaction`, `subagent`, `session` and `host/webserver`.

**Process model.** One process per `dsh` launch. In the `web` profile the base bundle (loop, tools, sandbox, approval, storage) and the web-app bundle (HTTP server, connection fence, gateway) share one process ([cordis.patch.yml L514-L533](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/bundle/base/cordis.patch.yml#L514-L533), [cordis.patch.yml L183-L189](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/bundle/web-app/cordis.patch.yml#L183-L189)). The Electron desktop app spawns that same host as a child process ([host-process.ts L205-L222](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/apps/desktop/src/host-process.ts#L205-L222)). The session store row is `session-persistence-jsonl` ([cordis.patch.yml L138-L141](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/bundle/base/cordis.patch.yml#L138-L141)).

**Where decisions live.**
- Loop: `Agent.turn()` appends `turn/start`, then loops `preStep` and `step` ([agent.ts L327-L433](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/agent-loop/src/agent.ts#L327-L433)).
- Tool dispatch: `executeToolBatch` ([batch.ts L50-L96](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/tools/src/batch.ts#L50-L96)) and `ToolExecutionPipeline` ([pipeline.ts L190-L233](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/tools/src/pipeline.ts#L190-L233)).
- Permissions and sandbox: sandbox mode from `sandboxPolicy.resolve()` ([index.ts L197-L205](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/sandbox/sandbox-policy/src/index.ts#L197-L205)), confinement from `sandbox.confine()` ([index.ts L174-L193](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/sandbox/sandbox/src/index.ts#L174-L193)), approval from `approval.request()` ([index.ts L242-L261](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/interaction/user-approval/src/index.ts#L242-L261)) and escalation from `approveEscalation()` ([escalation.ts L172-L206](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/sandbox/sandbox/src/escalation.ts#L172-L206)). In the shipped composition nothing asks for approval of ordinary tool calls (checked in review).
- Compaction: a listener on `agent/pre-step` compacts when the token meter crosses the threshold ([config.ts L68-L110](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/compaction/compaction-basic/src/config.ts#L68-L110)).

**Not in the repository, or not read.** The LiveShare relay is a separately deployed service ([worker.ts L1-L60](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/experimental/liveshare/src/owner-gateway/worker.ts#L1-L60)); the plugin market, skill loader and hook bridges were not read.

### 3.9 goose

**Languages and packages.** A Rust workspace, with an Electron and TypeScript desktop client under `ui/desktop`. `goose-agent` holds the generic state machine, `goose` the engine, the concrete operations, the built-in platform extensions and the ACP server, `goose-cli` the binary, `goose-context-management` compaction, `goose-provider-types` and `goose-providers` the provider layer and retry, `goose-mcp` the stdio built-ins, and `goose-roaming` the experimental peer transport.

**Process model.** One binary. The loop and its tools run in the process that hosts the session: the CLI, `goose serve` or `goose acp`. The desktop opens a WebSocket to `/acp` ([acpConnection.ts L136-L140](https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/ui/desktop/src/acp/acpConnection.ts#L136-L140)); `goose serve` listens on 127.0.0.1:3284 by default ([cli.rs L860-L905](https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-cli/src/cli.rs#L860-L905)). Built-in platform extensions are in-process objects and the others are MCP children or HTTP servers ([mod.rs L932-L968](https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/extension_manager/mod.rs#L932-L968)). Sub-agents are extra sessions in the same process. Storage is one SQLite file.

**Where decisions live.**
- Loop: `StateMachine::step` and `run` ([machine.rs L154-L212](https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-agent/src/machine.rs#L154-L212), [machine.rs L233-L248](https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-agent/src/machine.rs#L233-L248)).
- Tool dispatch: the ordered step list, with approval and the pre-tool hook before execution ([state_machine_builder.rs L127-L166](https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/state_machine_builder.rs#L127-L166)).
- Permissions: five inspectors in order ([services.rs L564-L592](https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/services.rs#L564-L592)).
- Compaction: two operations in the same step list ([state_machine_builder.rs L182-L225](https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/state_machine_builder.rs#L182-L225)); trigger in [mod.rs L21-L53](https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/context_mgmt/mod.rs#L21-L53).

**Not in the repository.** The `agent-client-protocol` and `agent-client-protocol-http` crates, where the ACP framing and connection identifiers live, are external dependencies. iroh internals were not read.

### 3.10 Cline

**Languages and packages.** TypeScript on Bun and Node. The SDK is layered `shared`, `llms`, `agents`, `core`; the apps are `apps/cli`, `apps/vscode`, `apps/cline-hub` and `apps/examples/desktop-app` (Tauri with a Bun sidecar). There is no Rust, no container code and no OS sandbox code in the product path.

**Process model.** `AgentRuntime` is a stateless loop for one agent; `SessionRuntime` in `core` wraps it with the mistake tracker, loop detector, extensions and hooks. Three runtime hosts exist: local (in process), hub (a per-machine daemon over WebSocket) and remote (a hub client at a given endpoint) ([host.ts L18-L43](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/runtime/host/host.ts#L18-L43)). The hub runs the loop in its own process ([hub-server-transport.ts L334-L343](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/hub-server-transport.ts#L334-L343)). The VS Code extension now runs on the SDK.

**Where decisions live.**
- Loop: `AgentRuntime.execute` ([agent-runtime.ts L849-L1040](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/agents/src/agent-runtime.ts#L849-L1040)).
- Tool dispatch: `prepareToolExecution` and `executePreparedTool` ([agent-runtime.ts L2432-L2694](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/agents/src/agent-runtime.ts#L2432-L2694)).
- Permissions: a per-tool `{enabled?, autoApprove?}` policy merged by `resolveToolPolicy` ([agent-runtime.ts L216-L234](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/agents/src/agent-runtime.ts#L216-L234)); the question to the user is the host's `requestToolApproval` callback.
- Compaction: a hook before each model request ([compaction-shared.ts L65-L82](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/context/compaction-shared.ts#L65-L82)); it is opt-in: the SDK core leaves it off unless the host enables it, and the CLI enables it.

**Not in the repository.** The desktop UI and Kanban apps, the cloud agent back ends (closed), and the JetBrains host side. The pre-SDK extension survives as dead or migration code.

### 3.11 OpenHands

**What the repository is.** `OpenHands/OpenHands` at the pinned commit is the Agent Canvas client: the npm package `@openhands/agent-canvas`, a React and TypeScript browser UI ([package.json L1-L6](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/package.json#L1-L6)), Node launchers, an Electron shell, a Dockerfile and a Helm chart. It states that Canvas does not execute agent actions ([architecture.md L14-L19](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/docs/architecture.md#L14-L19)).

**Process model.** The npm launcher starts, as host processes of the user, the agent server (bound to 127.0.0.1), the automation backend, a frontend and a Node ingress ([dev-with-automation.mjs L1194-L1210](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L1194-L1210)). The Docker all-in-one image runs the same pieces in one container ([entrypoint.sh L605-L620](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/docker/entrypoint.sh#L605-L620)). The agent server is a pinned PyPI package started through `uvx` ([dev-safe.mjs L576-L596](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-safe.mjs#L576-L596)). The browser dials one or more agent servers directly.

**Where decisions live.** The client composes the whole conversation specification: confirmation policy, analyzer, workspace kind, container limits, hooks, tools and secret references ([agent-server-adapter.ts L1206-L1245](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L1206-L1245)). Enforcement is on the server and is not visible. The loop, tool dispatch, permission enforcement and compaction are not in this repository.

**Not in the repository.** The agent loop, tools, condensers, the stuck detector, the security analyzers, the agent server and its authentication, the Docker runtime and its flags, and the Automation, Cloud and Enterprise servers. See section 2.2 for the repositories a complete study would need.

### 3.12 Zed

**Languages and packages.** Rust. `agent` holds the native agent (thread, loop, tools, permission decisions), `acp_thread` the UI-facing thread model that both the native and external agents feed, `agent_servers` the ACP client, `sandbox` the Seatbelt, Bubblewrap and WSL wrappers with `http_proxy`, `action_log` the per-thread record of agent edits, and `shell_command_parser` the command splitter.

**Process model.** One GUI process (GPUI). The Zed Agent runs inside the editor as a `Thread` entity, not as a server. External ACP agents are child processes over stdio JSON-RPC, or over an SSH remote. Native and external agents surface through one `AgentConnection` trait; its `truncate` method defaults to `None`, so only the native agent supports rewind ([connection.rs L296-L302](https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/acp_thread/src/connection.rs#L296-L302)). The sandbox flag is enabled for all users ([flags.rs L22-L28](https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/feature_flags/src/flags.rs#L22-L28), [flags.rs L107-L121](https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/feature_flags/src/flags.rs#L107-L121)).

**Where decisions live.**
- Loop: `Thread::run_turn` with a per-request loop; compaction is checked before every model request ([thread.rs L2959-L2990](https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/thread.rs#L2959-L2990)).
- Tool dispatch: the files read have no finding on dispatch or parallel calls.
- Permissions: regex rules per tool in `tool_permissions.rs` ([tool_permissions.rs L208-L260](https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/tool_permissions.rs#L208-L260)).
- Compaction: a threshold from the model's input capacity ([thread.rs L5004-L5028](https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/thread.rs#L5004-L5028)).

**Not in the repository, or not read.** The ACP message schemas (crates from crates.io), the LLM provider crates, MCP internals and Zed's own thread storage. The clone has one commit, so there is no history.

### 3.13 Warp

**Languages and packages.** Rust, an AGPL client. It builds one GUI app, a headless TUI (`crates/warp_tui`), and an `oz` or `warp` CLI that runs the same client code in SDK mode ([execution_mode.rs L9-L19](https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/warp_core/src/execution_mode.rs#L9-L19)).

**Process model.** The agent loop is not in this repository. The client sends each model turn as one HTTP request to Warp's server ([impl.rs L16-L26](https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/agent/api/impl.rs#L16-L26), [impl.rs L152-L168](https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/agent/api/impl.rs#L152-L168)) and receives a stream of protobuf events ([lib.rs L48-L89](https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/warp_multi_agent_client/src/lib.rs#L48-L89)). The wire schema is a separate crate ([Cargo.toml L351-L351](https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/Cargo.toml#L351-L351)). The client executes the returned tool calls locally (in the user's PTY, local file APIs, local MCP servers), applies its permission policy and sends the results back.

**Where decisions live.**
- Loop, system prompt, model routing and compaction: server side.
- Tool dispatch: per-action executors under `app/src/ai/blocklist/action_model/execute/`.
- Permissions: `BlocklistAIPermissions` ([permissions.rs L133-L140](https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/blocklist/permissions.rs#L133-L140)) over the `AIExecutionProfile` data ([ai_execution_profile.rs L352-L392](https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/cloud_object_models/src/ai_execution_profile.rs#L352-L392)).
- Local state: conversations are persisted locally in SQLite through diesel ([up.sql L1-L1](https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/persistence/migrations/2026-07-07-000000_add_summary_to_agent_conversations/up.sql#L1-L1)).

**Not in the repository.** The prompt, tool descriptions, compaction, model routing and fallback; the proto crate `warp-proto-apis`; the `oz-agent-worker` Go repository. See section 6 of `round2/warp.md`.

### 3.14 OpenCodex

**Languages and packages.** TypeScript on Bun, with a Rust relay binary, `native/ocx-child`, for the Remote Link feature. It is a provider gateway with a dashboard, not an agent harness.

**Process model.** One long-running process (`Bun.serve`) opens up to four listeners from one fetch handler: the public listener, an optional unauthenticated loopback listener, a hub-management listener bound to 127.0.0.1, and link and relay listeners. The ingress a request arrived on selects its route allow-list and its authentication policy ([index.ts L603-L609](https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/index.ts#L603-L609), [serve-options.ts L330-L364](https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/index/serve-options.ts#L330-L364)). Every inbound wire (Responses, Chat Completions, Anthropic Messages) is converted to an internal Responses request and run through one pipeline.

**Where decisions live.**
- Loop: there is none. Three bounded helper loops run inside a request: a web-search loop, the compaction summariser turn and a vision description call. Remote Workspace hosts an agent by starting the Codex app-server on the hub ([workspace-codex-runtime.ts L392-L414](https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-codex-runtime.ts#L392-L414)).
- Permissions: nothing asks. Read-only access is enforced in code by reducing the session's capabilities ([workspace-sessions.ts L406-L418](https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-sessions.ts#L406-L418)).
- Compaction: a helper on the data plane ([compact.ts L1777-L1802](https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/responses/compact.ts#L1777-L1802)).

**Not read.** The Rust `native/remote-workspace-helper`, the provider adapters beyond the OpenAI Chat wire, the `jev` combo decision back end and the web GUI. The identification of this project is a Round 1 judgement with medium-low confidence (see section 2.1).

## 4. Feature groups and implementation comparisons

The findings are organised into ten feature groups. Each group has its own synthesis file in [`round2/synth/`](round2/synth/), in which every project's implementation is a separate account with its own permalinks and review status. The comparison tables below are copied unchanged from those files; the accounts behind each row are in the file named under each heading.

### The agent loop, tool dispatch and durability

From [round2/synth/01-loop-dispatch-durability.md](round2/synth/01-loop-dispatch-durability.md).

Cell labels follow the project subsections. "None" means the files read report none (and where a reviewer confirmed the absence, the cell says so). "n/r" means not in the code that was read.

##### 3.1 Where the loop lives, and limits

| Project | Loop lives in | Step or turn cap (default) | Wall-clock or idle limit | Per-tool deadline | Provider retry |
|---|---|---|---|---|---|
| Codex | Rust; `run_turn` in the engine, in process (TUI, `exec`) or in a long-lived app-server daemon | None (absence confirmed); optional rollout token budget; reviewer-denial breaker | Stream idle 5 min; connection loss retried forever with a notice | Unified exec yields 250 ms to 30 s per call; background terminals 300 s default | 5 stream retries, 200 ms doubling, plus or minus 10% jitter; server advice first |
| OpenCode | TypeScript (Effect); `drain` in one server process per machine | `agent.steps` unset by default; when set, `toolChoice: none` plus refusal at dispatch | None reported | None in the loop | 10 retries, 2 s doubling to 10 s, jitter, retry-after up to 15 min; separate 1 h no-network budget |
| OpenClaw | TypeScript; `runLoop` in a Gateway process, or in a worker that dials the Gateway | None | 48 h per run; 120 s model-stream idle | n/r | n/r in the files read |
| Hermes Agent | Python; in the gateway owner process per profile | None (`max_turns: None`); sub-agents 250; 50 web searches and 50 sub-agents per turn | Cron inactivity watchdog 600 s | Foreground shell over 600 s promoted to background | 3 attempts, jittered, `Retry-After` honoured (heading only) |
| Oh My Pi | TypeScript plus Rust crates; in process, one session per process | None reported | None reported | n/r | 10 retries, 500 ms base, backoff capped 8 s, up to 25% jitter |
| Pi | TypeScript; in process; shipped path has no server | None (absence confirmed) | None | None in the loop; per-tool | Agent level 3 tries, 2 s doubling to 60 s, no jitter; provider level 0 |
| DeepSeek harness | TypeScript plugin composition in one host process | None (absence confirmed) | None | n/r | Per-provider policy, jitter, long `Retry-After` fails fast |
| goose | Rust; state machine in the process serving the session | 1000 assistant turns, then a canned "continue?" message | None in the loop | Shell 300 s default, model sets it, 0 disables | 3 retries, 1 s doubling to 30 s, jitter 0.8 to 1.2; not mid-stream |
| Cline | TypeScript SDK in process; hub is a local WebSocket host | None by default (SDK option `maxIterations`) | CLI `--timeout` is a host timer | `timeoutMs` metadata never read by the loop; shell 30 s | 3 retries, 1 s doubling to 15 s, no jitter; not after output streamed |
| OpenHands | Not in this repository (server SDK) | Client sends 500 and `stuck_detection: true` | n/r | n/r | n/r |
| Zed | Rust, in the editor process | n/r | n/r | Terminal output 16 KiB cap | Strategy by error kind: retry-after or backoff; 3 fixed for read/send errors |
| Warp | Server side; client executes tools | None client-side | n/r | n/r | 3 recovery attempts per request; fresh budget per tool-result round trip |
| OpenCodex | No agent loop; bounded helper loops inside a request | Web search 3 per turn, hard cap `maxSearches + 3` | Upstream stall 300 s (public upstreams) | 60 s per search | 3 attempts per logical request, charged to a workflow-root counter |

Evidence per row is in the project subsections. Sources: Codex `core/src/session/turn.rs`; OpenCode `session/runner/llm.ts`; OpenClaw `agent-loop.ts`; Hermes `agent/iteration_budget.py`; Pi `agent-loop.ts`; goose `ops_maxturns.rs`; Cline `agent-runtime.ts`; OpenHands `agent-server-adapter.ts`.

##### 3.2 Loop detection

| Project | Exists | On by default | Trigger and action | Where wired |
|---|---|---|---|---|
| Codex | No (absence confirmed) | n/a | Only a reviewer-denial breaker and a token budget | n/a |
| OpenCode | No in the step loop (absence confirmed; only the V1 `doom_loop` key). `/goal` reviewer has a stall counter | `/goal` only | 3 stalls of 50 rounds pause the goal | `plugin/goal.ts` |
| OpenClaw | Yes, rolling detector over 30 calls with outcome hashes; plus an identical-error breaker | Rolling: off. Error breaker: on | Warn 10, block 20, circuit breaker 30; third identical error ends the run | Admission before dispatch; fails closed if the check throws |
| Hermes Agent | Yes, signatures of tool, args hash and result; cycles of 2 to 4 over 64 entries; failure counters | Yes; warn only when attended, hard stop forced on for gateway and cron | Notice at 3rd repeat; exact-args failure warn 2, block 5; same-tool failure warn 3, halt 8 | Call site not traced (author note) |
| Oh My Pi | Only identical no-op edits (3 in a row) and stream rules fire-once | Yes | `NoChange` rejection after 3 | Edit tool |
| Pi | No | n/a | None | n/a |
| DeepSeek harness | Reminder only, identical consecutive calls | Yes (base bundle) | Notices at 3, 5, 8; never blocks; A, B, A, B not detected | `tools/post-execute` listener |
| goose | Exists but dead: `RepetitionInspector` has no limit, counts on a clone, flag not passed (checked in review) | No | None effective | Registered with `None` |
| Cline | Yes: mistake counter (SDK 6, CLI 3) and identical-call detector | Yes by code (docs say off) | 3rd identical call adds a notice; 5th forces the mistake limit, which asks the host callback | Runs from the `tool-started` event, after the call was issued |
| OpenHands | `stuck_detection: true` always sent; detector n/r | Yes | n/r | Server side |
| Zed, Warp | n/r | n/r | n/r | n/r |
| OpenCodex | Bounded helper loops only | n/a | Hard cap on its own search loop | `web-search/loop.ts` |

##### 3.3 Input during a turn

| Project | Steer, queue or reject | Where accepted input is stored | Persisted before acknowledgement | Survives a crash |
|---|---|---|---|---|
| Codex | `turn/steer` queued, drained at the next loop iteration (after in-flight calls); separate SQLite queue for idle dispatch | Steer: not described (unchecked). Queue extension: SQLite | Queue extension: yes; steer: unknown | Queue: yes; steer: not established |
| OpenCode | Inbox "promoted" at step boundaries | Not described | Not described | Not described |
| OpenClaw | Modes steer (default), followup, collect, interrupt; steer falls back to follow-up; cap 20, `summarize` | Channel messages: SQLite `channel_ingress_events` (unique key). `agent` turns: staged `session_pending_inputs` row. In-flight steering: in-memory queue | Yes for channel and `agent` paths; ACK sent after the claim | Yes for the SQLite paths; the in-memory steer queue only covers a failed run (corrected in review) |
| Hermes Agent | Intent `queue`, `steer` or `redirect` on a durable FIFO, 32 per session | SQLite `session_admissions` | Yes (row committed first, unique key) | Yes; a started-but-unsettled row becomes `unknown`, never replayed |
| Oh My Pi | RPC has `steer` and `follow_up`; steering is cooperative | Not described | Not described | Not described |
| Pi | Steering and follow-up queues, one at a time; `prompt()` while running throws | In-memory `PendingMessageQueue` | No (inferred) | No (inferred) |
| DeepSeek harness | Claimed from an inbox at each pre-step | Log (`user/message`, `agent/inbox/spliced`) once spliced; inbox partly process-local | Idempotent by `requestId` found in the log after splice | Only once spliced into the log (inferred) |
| goose | Rejected: a second run for the session returns `AgentRunExists` | n/a | n/a | n/a |
| Cline | `run()` while running throws; no steering found | n/a | n/a | n/a |
| OpenHands | Server side n/r; client sends over socket, falls back to HTTP, or holds in browser memory | Browser memory (Cloud, paused sandbox) | No for held messages | No for held messages |
| Zed, Warp, OpenCodex | n/r | n/r | n/r | n/r |

##### 3.4 What is durable before a tool runs, and what a restart does

| Project | Durable before a tool body runs | Flush rule | Turn running at crash |
|---|---|---|---|
| Codex | The model's tool-call item (recorded when its output item completes); no separate intent event; approval requests and begin events are not persisted | Writer flushes after every line | Not described for a crash; planned daemon restart writes `loaded-threads.json` and local turns can auto-continue |
| OpenCode | `Execution.Started` plus a claim row, in one transaction with the event; no per-tool intent | Event committed in the same transaction as projections | Boot sweep re-drains the turn with a synthetic "continue without repeating completed work" message; abort after more than 10 attempts; at-least-once |
| OpenClaw | Accepted input row; writer claim; no per-tool intent for in-process tools | CAS claim, fenced write inside the transaction | Fixed resume prompt, "check state before repeating"; lost worker: "effects may be unknown", never auto-continue; cron retries only if delivery had not started |
| Hermes Agent | Admission row `started` with epoch and generation; outbound reply obligation `attempting` before send | Claim and settle in one write transaction; settle names epoch and generation | Marked `unknown`, never requeued; person decides; unattended turns auto-settle `interrupted` |
| Oh My Pi | The appended message line | Synchronous append on the hot path; failure latches | Not described |
| Pi (shipped) | The user prompt, and each message on `message_end`; no intent | Append per finished message | Partial assistant message lost; dangling tool call not repaired (inferred) |
| Pi (pi-durable, experimental) | A checkpoint `{phase: execute, arguments, replay}` before the tool runs | Transactional commit per step | Running task goes back to `pending` with `restarts + 1`; tool re-run only if replay policy is `safe`, else "may have partially run" |
| DeepSeek harness | Every event up to the tool request, flushed first | Awaited flush before each model request, each top-level tool body, each pre-step; 200 ms write-behind otherwise | Resume closes the open turn: synthetic "outcome unknown" results, `turn/end` reason `interrupted`; nothing re-run |
| goose | Stored messages; approval decision persisted as a message before it takes effect | SQLite `BEGIN IMMEDIATE` per message | Re-derive from the conversation; unanswered confirmation continues; stale approvals expire with the extension lease |
| Cline | Assistant message in a JSON file rewritten whole (no atomic rename) | None | Not described; a crash mid-write can truncate the transcript |
| OpenHands, Warp, Zed, OpenCodex | n/r | n/r | n/r |

##### 3.5 Parallel calls, cancel and truncation

| Project | Parallel tool calls | Cancel of a running tool | Output truncation, and where it is enforced |
|---|---|---|---|
| Codex | Per-tool opt-in to a read lock; others exclusive; started while the stream continues | Aborts admission wait and handler | Per poll 10,000 tokens, 1 MiB per process, head and tail; recorder truncates old output |
| OpenCode | Each call forked immediately, no cap | Cooperative `context.cancel`, wait, then interrupt; plain decline is a defect | Bounded once at call time; 3 `length` stops fail the step |
| OpenClaw | Default parallel; sequential batches skip the tail on steering | `interrupt` aborts and waits for the writer to settle | 30% of window and 16k, 32k or 64k characters per result |
| Hermes Agent | 8 workers; prompts serialised; human wait excluded from deadline | Pending futures cancelled; running tools get 3 s | Spill to disk with per-result and per-turn budget |
| Oh My Pi | Shared by default, exclusive on resolver failure; steering aborts only `interruptible` tools | SIGTERM tree, grace, re-walk, SIGKILL | 50 KiB spill, head 20 KiB plus tail 20 KiB, artifacts 16 MiB |
| Pi | Preparation serial, execution `Promise.all` | One `AbortSignal`; process-tree kill; steering never interrupts | Each built-in tool (2000 lines, 50 KB); loop applies no cap |
| DeepSeek harness | Only when `isConcurrencySafe(args)` is exactly true; max 10 | Started bodies drain; skipped calls get synthetic results | Spill above 12,500 tokens, fails open |
| goose | Dispatched in order, result streams merged (see Section 7) | Race with a cancel token, drop the future, back-fill "interrupted" results | Shell 2,000 lines or 50,000 bytes, tail; collection unbounded; 200,000 characters per block |
| Cline | `Promise.all` for `parallel` tools; `run_commands` batch concurrent under one approval | One `AbortController`; group SIGKILL | 48,000 characters at the executor; 8,000 or 50,000 at request build; cache URI |
| OpenHands, Zed, Warp, OpenCodex | n/r (Zed: 16 KiB terminal cap) | n/r | OpenCodex Remote Workspace results 256 KiB |

##### 3.6 Unattended and scheduled runs

| Project | Mechanism | Approval behaviour | Missed or interrupted run |
|---|---|---|---|
| Codex | `codex exec` (same engine); SQLite queue; no scheduler in the engine | Approvals rejected as errors (`never`) | n/a |
| OpenCode | `schedule` table, loop in every server process | `run` rejects asks, or `--auto` answers once | Skip missed "every" slots, at most once per due row |
| OpenClaw | Cron in the Gateway; heartbeat job every 30 min | Headless runs deny; cron standing grants for an exact job | Restore finished; retry interrupted agent turn only if delivery had not started; no attempt counter |
| Hermes Agent | Cron JSON store, 60 s ticker, per-fire journal | `cron_mode: deny` by default; toolset denylist | Advance before dispatch (at-most-once); unknown outcome pauses the job; unattended `unknown` settles interrupted |
| Oh My Pi | None (print and JSON modes) | `yolo` default; fails closed only if a prompt would be needed | n/a |
| Pi | None | No permission system | n/a |
| DeepSeek harness | Host-resident schedule service; headless profile | `ask` with no answerer fails closed | Latest-only catch-up; at-least-once in the flush-to-receipt window (inferred) |
| goose | In-process cron, `schedule.json`, `--enable-scheduler` | Forced `Auto` mode | Not caught up; can overlap itself |
| Cline | Cron runner, hub | Default `yolo`, auto-approve | n/r |
| OpenHands | Separate Automation Server | n/r | n/r |
| Warp | Autonomous runs | "Would need approval" becomes `Denylisted` for every denial | n/r |

### Approvals and permission rules

From [round2/synth/02-approvals-permission-rules.md](round2/synth/02-approvals-permission-rules.md).

The cells condense the accounts in section 2. The permalinks and trust labels
are in section 2.

##### 3.1 Default mode and the rule model

| Project | Default mode (real) | Rule model and precedence | Can repository files loosen the user's settings? |
|---|---|---|---|
| Codex | **CLI/TUI:** reviewer `user`. **`exec`:** forces `Never`, so approvals are rejected. **Untrusted projects:** the internal `UnlessTrusted` mode. | **Grammar:** Starlark `prefix_rule` over the argv prefix. **Decision:** the maximum of allow < prompt < forbidden across all layers, plus managed requirements. **Load failure:** one broken file drops every user and project rule. | Project rules load as a layer, but they can only add to the maximum. Repository skills load without a trust check. |
| OpenCode | **Default:** allow everything (shell, edit, web, MCP, external directories); only reads of `.env` ask. **Headless `run`:** rejects asks. **`--auto`:** answers `once`. | **Grammar:** a flat `{action, resource, effect}` list; last match wins; no match asks. **Order:** configured deny is checked first. **Later layers win:** config layers, per-agent rules and HTTP session rules. **Admin policy:** a separate last pass. | Yes. Project config is a later layer. Project plugins load with no trust prompt. |
| OpenClaw | **Host exec:** `security=full`, `ask=off`. **Sandbox:** off. **Sandbox `deny` default:** does not block sandboxed exec. | **Axes:** three independent ones (host, security, ask). **Approvals file:** only tightens. **Session modes:** intersect to the stricter. **Exception:** admin `full` skips the floors. | Not shown in the files read. |
| Hermes Agent | **Default:** `smart`; the code falls back to `manual`. **Deny:** cron, `-q`, API and webhooks. **Auto-approve silently:** `-z` and a bare library context. | **Order:** hardline floor > `approvals.deny` globs > YOLO/off > `command_allowlist` > unattended modes > grants > guardian > human. **Config:** one `config.yaml` per profile. | No project layer described. Grants are written into the agent-reachable `config.yaml`. |
| Oh My Pi | **Default:** `yolo`; an invalid value also falls back to `yolo`. **ACP:** forces `always-ask` for bash unless auto-approve was chosen explicitly. | **Deny:** tool deny > user deny, then the mode ladder. **Restricted folders:** always prompt. | Yes. The project layer can set `yolo` and loosen everything except `bash.patterns`. |
| Pi | **Default:** no approval system; every call runs. | **Rules:** none. **Extensions:** `tool_call` handlers run in load order; the first block wins. | The trust gate only decides which project files load, and a repository cannot relax `defaultProjectTrust`. |
| DeepSeek harness | **Default:** `workspace-write` plus ask; no ordinary call prompts. **sdk-minimal:** `danger-full-access`. | **Rules:** none. **Mode:** approved-for-call > last `sandbox/mode` event > deployment default. | Not described. |
| goose | **Default:** `Auto` on every surface. **Scheduled jobs:** forced to `Auto`. | **Levels:** per tool name in the user-global `permission.yaml`: never_allow > always_allow > ask_before. **Inspectors:** can only tighten. | No project layer. Project plugins auto-enable, and the agent can rewrite its own policy files. |
| Cline | **CLI, Desktop and cron:** approve everything. **VS Code:** prompts only for shell commands. **ACP:** prompts. | **Policy:** `{enabled, autoApprove}` per tool name, plus `"*"`. **Missing policy:** allow. **Enterprise config:** fails open. | Workspace hooks, plugins and agents load untrusted. |
| OpenHands (client) | **Default:** `NeverConfirm`, unless the stored settings enable confirmation. **With confirmation and the `llm` analyzer:** `ConfirmRisky{HIGH, confirm_unknown}`. | **Policy:** one of three kinds, chosen by the browser per conversation. **Rules:** none. | Repository `hooks.json` is attached with no trust prompt. |
| Zed | **Global:** `default: confirm`. **Terminal:** OS-sandboxed by default for local projects. **External ACP agents:** use their own modes. | **Per-tool regex lists:** hardcoded `rm` deny > invalid regex = deny > substitution deny > deny > confirm > allow (every sub-command must match) > tool default > global default. | Not in the files read. Edits to `.zed/` always prompt. |
| Warp | **GUI:** commands `AlwaysAsk`. **TUI:** commands `AgentDecides`. **Headless:** "would ask" becomes `Denylisted`. | **Lists:** anchored regex allow and deny lists. **Precedence:** the denylist beats everything; the organisation list merges first. **Enforcement:** client-side only. | Not described. A self-declared `--sandboxed` argument weakens the policy. |
| OpenCodex | **Tool approvals:** none. **Remote Workspace:** `approvalPolicy: "never"`, sessions `accessMode: read-only`. | **Capabilities:** a set the executor declares, narrowed per session. | Not applicable. |

##### 3.2 Shell parsing, binding and "always"

| Project | Shell parsing before matching | What an approval is bound to | "Always" scope |
|---|---|---|---|
| Codex | **Splitting:** a tree-sitter-bash node and operator allow-list for `sh -c`. **Fallback:** anything else is one opaque argv. **Wrappers:** not unwrapped. | **Session key:** environment id, executable, canonical argv, cwd, tty, sandbox and extra permissions. **Patches:** per path. **Never cached:** MCP and network. | **Writes:** an `allow` prefix rule to `default.rules`. **Lasts:** permanently. **Side effect:** also skips the sandbox. |
| OpenCode | **Parser:** a hand-written POSIX and PowerShell scanner. **Splits:** chains, and scans `$(...)`. **Refuses:** eval forms and unparseable input. **Ignores:** redirects, declarations and wrappers. | The request id only. | **Stored:** in SQLite, permanently, project-wide, for all agents. **Pattern:** often `*` or a first-word prefix (`sudo *`). |
| OpenClaw | **Parser:** tree-sitter-bash WASM, limited to 128 KiB and 500 ms. **Unanalyzable:** substitutions, redirects, heredocs, control flow. **Parse failure:** prompts. | argv, cwd (realpath and inode), agentId, sessionKey, a hash of the environment overrides, the real-path executable (SHA-256 if mutable), nodeId and the requester device. | **Persisted entry:** the exact executable path plus a SHA-256 of cwd and argv[1..]. **Cron:** job-bound grants. |
| Hermes Agent | **Matching:** regex over several string variants. **Deny:** per-segment executable projection that sees through `sudo`, `env` and `sh -c`. **Fails closed:** only on size limits. | A pattern class (a rule description such as "recursive delete"). | **Writes:** the class to `command_allowlist`, for every session, permanently. **"Session":** in memory only. |
| Oh My Pi | **Patterns:** anchored `*` globs. **Allow:** the whole command, with no shell control characters. **Deny or prompt:** per segment. **Wrappers:** not unwrapped. | **Session answer:** `subject\0tier`; for bash the subject is the whole tool. **Cached grant:** `tier\0path`. | **Only in** `always-ask-cached` mode. **Stored:** SQLite, no expiry. **Never offered:** for critical calls. |
| Pi | None in core. | Not applicable. | Trust only: per folder, in `trust.json`. |
| DeepSeek harness | None. The string runs as `bash -c`. | One call id. Neither the request nor the audit event carries the arguments. | None. Only `allowed-once`. |
| goose | None. Regexes and models read the raw `command` string. | A once flag on the persisted tool-call id, without the arguments. It expires with the extension lease. | **Key:** the bare tool name. **Stored:** user-global `permission.yaml`, no expiry. |
| Cline | **Approvals:** none. **Plan-mode blacklist:** splits on separators; fails open on parse errors. | One `toolCallId`. One approval covers a whole `run_commands` batch. | None. Only live category toggles. |
| OpenHands (client) | None in this repository. | **None.** `{accept}` answers whatever is pending. | None. |
| Zed | **Parser:** a brush-parser POSIX AST for every shell, PowerShell included. **Redirects:** treated as pseudo-commands. **Wrappers:** not unwrapped. **Parse failure:** disables allow. | The tool plus a prefix regex of the command word and subcommand. | **Written to:** user `settings.json`, for all threads and projects, permanently. |
| Warp | **Decomposes:** `$( )`, `( )` and chains. **Strips:** a leading `KEY=VAL`. **Not handled:** wrapper unwrapping, `$VAR`. | **Per call.** The model's `is_risky` and `is_read_only` can pre-approve. | **Run-to-completion:** for the conversation. **Lasting rules:** the profile lists only. |
| OpenCodex | None. `exec` takes an argv only, inside bubblewrap. | Each RPC is bound to the handshake session id, root id and capability set. | A capability set for one session. |

##### 3.3 Model approvers, plan mode and sub-agents

| Project | Model approver and its failure mode | Model-supplied flags | Plan or read-only mode | Sub-agent inheritance |
|---|---|---|---|---|
| Codex | **Guardian:** a read-only sandboxed thread, 90 s default, risk thresholds. **Errors:** deny. **Timeout or over budget:** back to the human. **Breaker:** per turn. | `sandbox_permissions` and a justification on escalation. | **Prompt only.** The `explorer` role file is empty. | **Roles:** can only narrow; the parent's policy is re-applied last. **Depth:** 1. **Exception:** the memory agent runs with approvals off. |
| OpenCode | None found. | No such flags were found. | **Code-enforced:** edit tools only. **Prompt only:** shell, sub-agents, Code Mode. | **Rules:** a creation-time copy of the parent's session rules plus the child agent's own (often allow-all). **Depth:** 1. |
| OpenClaw | **`auto` reviewer:** temperature 0, 30 s, 16k characters, strict JSON. **Any failure:** ask. **Injection regex match:** deny. **On a node:** ask becomes deny. | None in the files read. | **No plan mode.** The `read-only` session mode is enforced in code. | **Inherits:** the parent's tool allow and deny envelopes. **Exec mode:** not in the files read. |
| Hermes Agent | **Auxiliary model:** sees flagged commands only, answers one word, temperature 0, 30 s. **Any failure:** to the human. **DENY:** overridable once. | None described. | Prompt only. | **Tools:** the parent's minus five. **Dangerous prompts:** auto-deny unless `subagent_auto_approve`. |
| Oh My Pi | **None in the approval path.** The TTSR judge only warns. | None described. | **Code-enforced:** write and edit. **Prompt only:** bash and eval. | **Forced `yolo`,** but the user's deny and prompt rules are kept. **Prompts:** fail closed. **Depth:** 2. |
| Pi | None. | None. | Example extension only. | Example only: a fresh process with the parent's environment. |
| DeepSeek harness | **Experimental Auto review:** the same model, never sees tool results. **Failure:** deny. **Timeout:** none. **Removing the plugin:** widens to full access. | `sandbox_permissions` plus a justification. `approvedBuilds` on `plugin_manager`. | Prompt only. | **In-process:** approval pinned to `never`. **External children:** a fixed, parent-independent permission. |
| goose | **SmartApprove:** fails closed. **Adversary reviewer:** fails open. **Injection classifier:** fails open, never sees tool output. | MCP `readOnlyHint` auto-allows. | Chat mode is enforced at execution, but the tools are still advertised. | **Mode:** inherited from the parent. **Approvals:** forwarded to the parent's user. **Depth:** 1. |
| Cline | None [absence confirmed]. | `requires_approval` removed. | **Code-enforced in part:** edit tools are removed. **Blacklist:** can be bypassed. **CLI:** the model can switch to act itself. | **Policies and approver:** none, so everything runs. **Nesting:** unbounded. |
| OpenHands (client) | Inline `security_risk`, probably; the rest is in the SDK. | `security_risk` on each action. | A separate planner conversation with three tools. | **Takes:** the global settings. **Limits:** no cap, no narrowing. |
| Zed | None. | A non-empty `reason` is required for sandbox escalation. | Not in the files read. | Not in the files read. |
| Warp | **None on the client.** The model's `is_risky=false` auto-runs the command. | `is_risky` and `is_read_only`. | Not in the files read. | **Native children:** inherit the profile. **Claude and Codex children:** launched with their safety flags off. |
| OpenCodex | None. | None. | `accessMode: read-only`, enforced on the executor. | Not in the files read. |

##### 3.4 Bypass paths, hooks and remote answers

| Project | Known bypass paths | Hook failure mode | Does a remote answer name the action? |
|---|---|---|---|
| Codex | `allow` rules skip the sandbox. `--ignore-rules`. A hook-trust bypass flag. The exec-server runs the caller's sandbox, or none. | **Fails open** on error, timeout and bad JSON. **Precedence:** deny wins. **Hook allow:** pre-approves the unsandboxed retry. | **Not shown.** It is a JSON-RPC response; binding to the action is [inferred]. **`HumanCommand`:** routed to opted-in connections only. |
| OpenCode | Plugin tools without `assert`. `` !`cmd` `` templates. Shell redirects. The server password is readable, which enables self-approval [inferred]. | **`permission.evaluate`:** can loosen asks. **A throw:** inferred to fail the call. | **No.** Any password or pairing-token holder answers by request id. |
| OpenClaw | Open defaults. Cron standing grants. A Gateway approval overrides the node's allow-list. | **Fails closed:** any throw in `before_tool_call`. | **Node exec:** yes (runId, node, device, binding, single use). **Chat `/approve` with an empty approver list:** anyone in the chat. |
| Hermes Agent | Container backends skip the floor and the prompts. `execute_code`. `-z`. | **`pre_tool_call` crash:** fails open. **`approve` escalation:** fails closed. | **Card:** shows the redacted command and stays deny-only until the full command is visible. **Request-id binding:** not in the files read. |
| Oh My Pi | `eval` kernels. MCP at the `write` tier. Project settings can set `yolo`. Auto-loaded project hooks and tools. | **Fails closed:** a throw, a 30 s timeout or an abort blocks. **After a rewrite:** policy is re-resolved. | Not in the files read. |
| Pi | `!` commands. Install scripts. Hook argument mutation without re-validation. | **`tool_call` throw:** blocks the call. **Observers:** fail open. | **RPC dialog:** answered by dialog id, showing the extension's message text (clone check, section 1). |
| DeepSeek harness | MCP. In-process tools. `approvedBuilds`. External children. | **A throwing listener:** fails the call closed. **The hook bridges:** not read. | **No.** The answer is `{clientId, eventId, outcome}`. |
| goose | Code Mode `tools.call` skips approval, inspectors and hooks. ACP-hosted agents. `readOnlyHint`. Writes to the policy files. | **Default:** fails open. **PreToolUse `on_failure: block`:** fails closed, timeouts included. | **Call id:** persisted first; conflicting answers rejected. **Arguments:** not bound. |
| Cline | Sub-agents. Hub client tools. MCP. Untrusted workspace hooks. Origin-only hub sessions. | **Fails open** on a crash, a timeout or bad output. A cancel aborts the whole run. | **Opaque `approvalId` only.** Any attached client may answer. |
| OpenHands (client) | Server-level bash. The session key is in the agent's environment. Client tools. `tool_module_qualnames`. | **Load failure or 5 s timeout:** all hooks are dropped. | **No.** The body is `{accept, reason?}`. |
| Zed | External ACP agents ignore `tool_permissions`. ACP `terminal/create` is unsandboxed. Allow-all skips the substitution check. | Not in the files read. | **ACP:** the agent's options are passed through. **Binding:** not shown. |
| Warp | Run-to-completion (default on). `--sandboxed`. CLI children with their safety flags off. | No hooks. | **ACP:** answered by policy, which prefers `AllowAlways`. |
| OpenCodex | An unauthenticated loopback listener. Executor-declared capabilities. | No hooks. | No remote answering. |

### Sandboxing and where commands run

From [round2/synth/03-sandbox-containers-execution.md](round2/synth/03-sandbox-containers-execution.md).

##### 3.1 Mechanism per OS and behaviour when unavailable

| Project | Linux | macOS | Windows | When unavailable | Enforcement level surfaced? |
|---|---|---|---|---|---|
| Codex | bwrap + seccomp (two stages); legacy Landlock refused for restricted policies | Seatbelt (profile not read) | restricted token or MXC, only if enabled; otherwise the rule fallback prompts | Linux: the helper panics, nothing runs. Exec-server: refusal string. WSL1: refused | not found in the input |
| OpenCode | none | none | none | n/a | n/a |
| OpenClaw | Docker or Podman container (opt-in) | same | same | `sandbox: required`: throws. Otherwise host exec | not found |
| Hermes Agent | none locally; Docker, SSH, Modal or Daytona backends | same | same | n/a | n/a |
| Oh My Pi | none | none | none | n/a | n/a |
| Pi | none (Gondolin VM as an example extension) | none | none | n/a | n/a |
| DeepSeek harness | bwrap, then Landlock, probed in order | Seatbelt `(allow default)` | restricted token plus ACL SIDs | `SANDBOX_UNAVAILABLE`, nothing runs | computed (`full`/`partial`), stored on the result, shown to nobody |
| goose | none (Seatbelt removed) | none | Job Object (for killing only) | n/a | n/a |
| Cline | none | none | none | n/a | n/a |
| OpenHands | per-conversation Docker, upstream and not read | same | same | a failed probe falls back to a host workspace | not visible |
| Zed | bwrap (setuid refused) | Seatbelt `(deny default)` | bwrap inside WSL | Linux and WSL: a prompt (retry, run unsandboxed, deny). macOS: fails closed | the model is told when it ran unsandboxed |
| Warp | none (`sbx` Docker behind a dogfood flag) | none | none | n/a | `--sandboxed` is self-declared |
| OpenCodex | bwrap on the executor | native helper, probe fails | native helper, probe fails | `exec` not offered; file tools only | the capability list (`workspace.exec` present or not) |

##### 3.2 What the sandbox confines

| Project / mechanism | File reads | File writes | TCP/UDP | Unix sockets | Processes and signals |
|---|---|---|---|---|---|
| Codex bwrap+seccomp, network off | restricted-read policies start from `--tmpfs /`; deny paths masked; deny globs snapshotted at launch | writable roots only; `.git`, `.agents`, `.codex`, `.aws` read-only | netns; seccomp denies `connect`, `bind`, `sendto` and non-`AF_UNIX` `socket` | `connect` denied, so pathname stream sockets are blocked; `sendmsg` to a pathname datagram socket not denied (review); daemon directory masked | PID, user and IPC namespaces; `ptrace` and `process_vm_*` denied; `io_uring` denied |
| Codex, network on | as above | as above | open (unless proxy mode) | reachable except masked directories | `AF_VSOCK` and `io_uring` denied |
| DeepSeek bwrap | not confined | roots, plus a private `/tmp` | not confined | not confined (inferred) | `--unshare-pid` |
| DeepSeek Landlock | not confined | roots, plus the shared `/tmp` | not confined | not confined | none |
| DeepSeek Seatbelt | not confined | roots | not confined | not confined | none |
| Zed bwrap | `--ro-bind / /` (all readable) | exact writable binds; protected paths re-bound read-only (missing paths skipped) | netns when blocked or proxied; HTTP(S) proxy for host lists | not stated in the input for Linux | user, pid, ipc, uts and cgroup namespaces |
| Zed Seatbelt | all reads allowed | writable subpaths; protected denies last | none, the proxy port, or all | none allowed | Mach-service allow-list |
| OpenCodex bwrap | `/usr`, `/bin`, `/lib*`, a short `/etc` list, toolchain roots | one `/workspace` bind | `--unshare-net` always | host sockets not mounted (inferred) | pid, ipc and uts namespaces; no user namespace, no `cap-drop`, no seccomp |
| OpenClaw container | container filesystem plus validated binds | `--read-only` root, sized tmpfs, binds | `--network none` by default | Docker socket binds rejected | `--cap-drop ALL`, `no-new-privileges`, `--pids-limit 1024` |
| Hermes Docker | container filesystem plus persistent home and workspace binds | the same binds (as root with `DAC_OVERRIDE`) | on by default | not addressed | `cap-drop ALL` plus 3 to 5 added back; `no-new-privileges`; pids 2048 if cgroups allow |
| rizzy PR 4 Landlock (our code) | `SYSTEM_READ` list plus node.toml `read`; `/proc` readable | session folder, per-session `TMPDIR`, node.toml `write` | network off: TCP connect and bind denied (ABI 4); UDP and DNS open | pathname sockets open below ABI 9; abstract sockets scoped (ABI 6, best-effort) | signals scoped (ABI 6, best-effort); `no_new_privs` required |

##### 3.3 Container invocations: fixed flags and who controls them

| Flag or property | OpenClaw | Hermes Docker | goose `--container` | Warp `sbx` | OpenHands | rizzy design §4.6 |
|---|---|---|---|---|---|---|
| Who builds the invocation | Gateway, from `openclaw.json` | Hermes, from config | the user (an existing container) | the client, with fixed arguments | upstream SDK (not read) | the node, from `node.toml` |
| Image | config; default `openclaw-sandbox:trixie-slim` | config | the user's | Docker Sandboxes | `OH_CONVERSATION_IMAGE` env | allow-list pinned by digest |
| Pull | `--pull=never` | not stated | n/a | not stated | not visible | `--pull=never` |
| User | image default (inferred) | root, unless `--user` | the user's | not stated | not visible | `--userns=keep-id --user <uid>:<gid>` |
| Capabilities | `--cap-drop ALL` (default) | `ALL` dropped, then `DAC_OVERRIDE`, `CHOWN`, `FOWNER` (+`SETUID`, `SETGID`) | n/a | not stated | not visible | `--cap-drop=all` |
| `no-new-privileges` | always | yes, unless `docker_snap_compat` | n/a | not stated | not visible | yes |
| Root filesystem | `--read-only` (default) | writable | n/a | not stated | not visible | `--read-only` |
| tmpfs | `/tmp` 1 GB, `/var/tmp` and `/run` 256 MB | `/tmp` 512 MB, `/var/tmp` 256 MB, `/run` 64 MB | n/a | not stated | not visible | `/tmp` 1 GB, `/run` (no size) |
| Network | `none` (default); host and `container:*` rejected | on (default) | n/a | not stated | not visible; the launcher binds `0.0.0.0` so containers reach the host | pasta or none; host only with `allow_host_network` |
| Limits | `--pids-limit 1024`, `ulimit core=0`; memory and CPU optional | pids 2048, 1 CPU, 5120 MB if cgroups allow | n/a | not stated | the client sends unbounded memory and CPU | memory, swap, CPUs, pids from `node.toml` |
| Mounts | validated binds (blocked list, roots, realpath through the ancestor) | persistent home and workspace; read-only skills and creds; cwd optional | n/a | an empty host directory plus a read-only init directory | `/workspace` | one environment volume, `relabel=private` |
| Environment | `--env-file`, config-declared only | name-only `-e KEY` (values not in argv) | `-e K=V` on argv (leaks into `ps`) | not stated | every saved secret plus the session key | `--env` allow-listed names |
| Pass-through args | three `dangerously*` booleans | `docker_extra_args` appended last, override anything | n/a | none | n/a | none (by design) |
| Recreate on flag change | `createArgsEpoch` label | labels (task, profile, egress, env fingerprints) | n/a | n/a | n/a | not specified |
| Checks skipped inside | approval (sandboxed exec runs without one by default) | hardline floor, sudo guard, dangerous patterns | n/a | n/a | hooks skipped for container workspaces | none planned |

##### 3.4 Remote executors: what the executing side decides

| Project | Executor | Transport and authentication | Executor-side policy | Refusals |
|---|---|---|---|---|
| Codex | `codex exec-server` | Noise IK through a relay, registry-pinned key; direct `--listen` with optional auth | none: the caller's sandbox intent is materialised; no intent means no sandbox | "sandbox intent cannot be enforced on this executor" |
| DeepSeek harness | SSH helper | OpenSSH with fixed flags; TLS-PSK streams | runs its own platform sandbox chain for `confine` | `SANDBOX_UNAVAILABLE`; reported enforcement is self-reported |
| Pi (pi-env) | Rust daemon over SSH stdio | the SSH login only | none: any path, any command | unknown ops (`EINVAL`) only |
| OpenCodex | Remote Workspace executor | WSS bearer plus signed ECDH, AES-GCM | path, identity and hard-link checks; `exec` only with an OS sandbox; roots chosen on the executor | many (see section 2) |
| OpenClaw | paired node | Gateway WebSocket | own `security`/`ask`/allow-list; plan binding; policy-snapshot check | `SYSTEM_RUN_DENIED`; but a Gateway approval overrides the node allow-list |
| Hermes Agent | SSH, Modal, Daytona | `accept-new` SSH; vendor SDKs | none (guards skipped for cloud backends) | none |
| Cline | remote Cline hub over SSH | strict host keys, tunnel, token | a full remote agent with its own settings | its own |
| OpenCode | remote OpenCode server over SSH; empty driver seam | SSH tunnel plus service password | a full remote server | its own |
| OpenHands | DigitalOcean microVM agent-server | bearer port-forward tunnel | upstream, not read | not visible |
| goose, Oh My Pi, Zed, Warp | none read (OMP SSH is file I/O only; Warp's worker is not in the repository) | n/a | n/a | n/a |

##### 3.5 How child processes are killed, and what survives

| Project | Kill mechanism | What survives |
|---|---|---|
| Codex | `--die-with-parent`, `--new-session`, PID namespace; the inner stage forwards signals; LRU of 64 processes terminates pruned ones | not traced |
| OpenClaw / Hermes containers | `--init`; container lifetime (`sleep infinity`); Daytona cancel stops the sandbox | Hermes reuses containers across processes by label |
| Hermes local | `start_new_session` plus an owner-death supervisor | not stated |
| Oh My Pi | SIGTERM to the group and descendants, grace, re-walk, SIGKILL (unchecked) | a double-fork that changes session (inferred) |
| Pi CLI | SIGKILL to the process group; SIGSTOP first on macOS; tracked detached children killed on SIGHUP/SIGTERM | everything, if Pi itself is SIGKILLed |
| Pi pi-env | the daemon kills every process group it started on stdin EOF or 30 s of silence | n/a |
| DeepSeek SSH | the helper lease expires and remote managed processes are cleaned up | n/a |
| goose | `kill(-pgid, SIGKILL)`; Windows Job Object created suspended; `Drop` kills | `setsid` children; background work after a normal exit; everything if goose crashes (no PDEATHSIG for the shell) |
| Cline | SIGKILL to the detached group; `taskkill /T /F` | `setsid` grandchildren; human-detached commands; hook timeouts kill the direct child only |
| OpenCodex | bwrap `--die-with-parent --new-session`; kill on output overflow, timeout or abort | not stated |
| Zed | bwrap `--die-with-parent`; ACP children killed on drop | not stated |
| OpenCode | PTY daemon keeps terminals across server restarts | terminals, by design |

### Credentials, secrets and authentication

From [round2/synth/04-credentials-secrets-auth.md](round2/synth/04-credentials-secrets-auth.md).

##### 3.1 Keys, environment, redaction

| Project | Keys at rest | Keys reachable from the shell | Child environment (shell / MCP) | Redaction |
|---|---|---|---|---|
| Codex | MCP OAuth in keyring, else a user-readable file | Yes: `OPENAI_API_KEY` inherited by default; with the broker, a dummy | Inherit all; `*KEY*` excludes off by default; fixed always-strip list for launch secrets; Git vars scrubbed for plugin Git | Four regexes, not on tool output |
| OpenCode | SQLite JSON, 0600 | Yes: env, and the DB file; server password in env outside stdio mode | Whole `process.env`; workspace drivers start empty (none ship) | None |
| OpenClaw | Auth profiles in SQLite; config/`.env` readable | Host exec: yes for provider keys in env; sandbox: no; GitHub token on workers: yes | Inherit minus ~110 injection keys and registered credential names; sandbox: declared only; SSH: `*_API_KEY` etc. blocked | Worker credentials and GitHub tokens registered for log redaction |
| Hermes Agent | `auth.json` pools; per-profile homes | Terminal: unlisted secrets yes; managed keys no; Docker with iron-proxy: tokens only | Terminal blocklist; `execute_code` and MCP allow-list | On by default; pattern plus env-dump pass; sentinel on file reads |
| Oh My Pi | `agent.db` SQLite, 0600, unencrypted | Yes: env and `agent.db` | Whole env minus project `.env` values and Git location vars | Opt-in obfuscation to the provider, reversed before tools run |
| Pi | `auth.json` 0600 on creation | Yes: env and `auth.json`; `--api-key` in argv | Whole env; MCP inherits | None in core |
| DeepSeek harness | `.credentials.yaml` 0600, refuses loose mode; cwd `.env` accepted | Env scrubbed by name; store file readable by confined bash | Name-pattern scrub (`KEY|PASSWORD|SECRET|TOKEN`, `DSH_*`), MCP same | Resolved MCP credential values only |
| goose | Env first, keyring blob, silent plaintext fallback | Yes: env-supplied keys | Whole env; extension `envs` filtered by 31-key disallow list | None found |
| Cline | `providers.json`/`secrets.json` 0600 | Yes: env and file | Whole env for shell, hooks, plugins, MCP | Telemetry and marketplace output only |
| OpenHands (client) | Key files 0600; Docker: key beside ciphertext | The master session key is put in the agent's env on purpose | Server side, not in repo | Server side, not in repo |
| Zed | Not studied | Not studied | Inherited (terminal and ACP agents) | Not studied |
| Warp | OS keychain, then sent to Warp per request | Not shown (client) | MCP inherits whole env | Client regex, off by default |
| OpenCodex | Plaintext `config.json` unless keychain; OAuth plaintext 0600 | Gateway: n/a (no agent shell); Remote Link child: link key only | Remote Workspace `exec`: bwrap `--clearenv` plus `HOME`, `PATH`, `LANG`, `LC_ALL` | Logs refuse any configured credential value; fail closed |

##### 3.2 Brokers, proxies, workers, servers, trust

| Project | Broker or proxy, and coverage | Worker credentials | Server authentication model | Trust gate |
|---|---|---|---|---|
| Codex | Network-proxy credential broker (off by default, MITM), dummy env values, host-bound; netns bridge makes it the only path on Linux | Exec-server: registry auth via stdin, Noise IK with pinned key; caller's sandbox applies | WS: capability token or HS256 JWT at upgrade, `Origin` 403; loopback unauthenticated; non-loopback refused for app-server only; Unix socket by file mode | Project trust for config, hooks, exec policy; hook hash in user layer; skills ungated |
| OpenCode | None | None (SSH tunnel to a per-machine server) | One password (Basic, cookie or `?auth_token=`), `===` compare; pairing token HMAC of the password | None |
| OpenClaw | Secret egress proxy with sentinels and allowed hosts (cooperative); inference brokered by model ref | 32 bytes, domain-separated hash bound to the turn claim, 10 min TTL, per-frame revalidation, owner-epoch fencing | Challenge nonce, Ed25519 device proof over token and scopes; startup token; device tokens plaintext | Not read this round |
| Hermes Agent | iron-proxy token swap with `require: true`; Docker only; own model calls bypass | Relay HMAC token, 300 s | Same-user control socket, single-use 30 s tickets in a subprotocol; non-loopback dashboard needs auth; API server one key | Regex scanner at install; context files blocked on hits |
| Oh My Pi | Auth broker hands out keys; auth gateway proxies with digest-stored scoped clients (no expiry); wildcard CORS | None | Static bearers, `Set.has`; `--no-auth` | Hard-coded open |
| Pi | None (documented third-party only) | pi-env: none beyond SSH | stdio only; `apps/p2` one-use token, Host and Fetch-Metadata fence | Per-directory boolean, no content binding, context files ungated |
| DeepSeek harness | None | None | Launch token in URL, then 30-day HMAC cookie; Host, `Sec-Fetch-Site`, Origin fence; wildcard bind refused | `approvedBuilds` set by the model |
| goose | None | Roaming: key in trust book, re-read per connection, revocation closes live connections | Shared secret, header or `?token=`, constant time; `--dangerously-unauthenticated` | Project plugins auto-enabled |
| Cline | None | Provider key sent per session to a remote hub | Per-start token **or** loopback `Origin`; approvals answerable by anyone | None (home-only lane excepted) |
| OpenHands (client) | MARS bridge keeps a cloud token server-side | n/a in repo | One shared key, injected into HTML on loopback, kept in `localStorage`; first-frame WS auth | Hooks ungated; plugin checkbox |
| Zed | Egress allow-list proxy, no credential substitution | n/a | Not studied | ACP registry checksum optional |
| Warp | Model calls through Warp's server with the user's key | In uncloned repo | Server side | Project MCP never auto-starts |
| OpenCodex | Full model-call gateway with admission, budgets, scopes; Remote Link relay strips caller credentials | Pair code 60 bits, hash stored, deleted before validation; bearer hashed, no expiry | None on loopback; mandatory key off loopback; admin token distinct from data keys | n/a |

### Links, workers and client protocols

From [round2/synth/05-links-workers-protocols.md](round2/synth/05-links-workers-protocols.md).

##### 3.1 Topology, identity and refusal

"Loop" is the agent loop (model calls and tool-call decisions); "tools" is where tool calls
execute. "Refuses" is what the executing side can refuse on its own authority, whatever the
controlling side sends.

| System | Loop runs | Tools run | Who dials | Transport | Pairing and identity | Executing side refuses |
|---|---|---|---|---|---|---|
| Codex app-server | app-server | app-server, or an exec-server | client to server | stdio JSONL, `unix://`, `ws://` JSON-RPC | capability token or HS256 JWT at upgrade; none on loopback | n/a (single process) |
| Codex exec-server | harness | executor | executor to relay (`--remote`); or harness to `--listen` / `ssh ... --listen stdio` | relay WebSocket + Noise IK, 128 streams | executor key pinned from a registry; registry authorises harness key | nothing: the sandbox comes from the request; none means unsandboxed |
| Codex remote control | local app-server | local | app-server to vendor relay | WebSocket v3, segmented envelopes | enrolment token with `expires_at`; relay host allow-listed | n/a |
| OpenCode service | server | server (or SDK `WorkspaceDriver`) | client to server | HTTP + SSE | one shared password; pairing code to password-derived HMAC token | driver only executes; decision stays on server |
| OpenCode SSH (desktop) | remote server | remote server | desktop over SSH, then `ssh -L` | HTTP through tunnel | password read from remote `service.json` over SSH | n/a |
| OpenClaw cloud worker | worker (inference brokered by Gateway) | worker | worker to Gateway WS; Gateway also SSHes in | WebSocket JSON frames, closed method set | setup code binds box to environment; per-dispatch credential | worker checks the hello matches its request; model choice approved by Gateway |
| OpenClaw node | Gateway | node (`system.run` and others) | node to Gateway WS | WebSocket, protocol 4 | Ed25519 device proof over nonce + token; operator approval or SSH auto-approval | `system.run`: own security/ask, plan binding, PATH, policy drift; but a forwarded approval satisfies its allow-list |
| Hermes viewers (CLI, TUI, ACP) | gateway | gateway | viewer to gateway | Unix socket for a ticket, then WebSocket | peer uid; 30 s single-use ticket in a subprotocol | ACP viewer never runs tools |
| Hermes relay | gateway | gateway | gateway to relay | WSS, NDJSON | HMAC upgrade token (300 s), rotating secrets | n/a |
| Hermes SSH / Modal / Daytona | host | remote | host over SSH or SDK | `ssh bash -c` / SDK exec | `accept-new` (TOFU) | nothing |
| Oh My Pi RPC / ACP | embedded process | same | parent spawns child | NDJSON / ACP over stdio | pipe ownership | n/a |
| Oh My Pi collab | host | host | host and guests to relay | WSS, AES-256-GCM sealed frames | room key and write token in link fragment | guest prompts run under host's mode; read-only peers refused |
| Pi pi-env | controller | daemon | controller over SSH | length-prefixed binary frames on stdio | SSH strict host keys, private known-hosts, `scanHostKey` | nothing (no root, no allow-list) |
| DeepSeek SSH | local | remote helper | local over SSH | framed JSON RPC (protocol 3); TLS-PSK per stream | SSH config; self-reported helper hash | helper runs its own sandbox chain; reported enforcement trusted |
| DeepSeek web | host process | host process | browser to host | HTTP POST + one WebSocket mux | launch token to signed cookie | n/a |
| goose serve | server | server | client to server | HTTP / WebSocket ACP | one shared secret (header or `?token=`) | n/a |
| goose roaming | host | host | peer to host (iroh QUIC) | QUIC streams carrying ACP | key in a trust book; fingerprint compared out of band | host forces cwd on new/load/fork (bypassable by update) |
| Cline hub | hub | hub, or a client's contributed executors | client to hub | one WebSocket, JSON frames | per-start token, or loopback `Origin` | n/a; client sets tool policies |
| Cline SSH remote | remote hub | remote hub | desktop over SSH, then `ssh -L` | hub WebSocket through tunnel | token printed on remote stdout | n/a |
| OpenHands Canvas to Agent Server | Agent Server (not in repo) | Agent Server, or browser for client tools | browser to server | WebSocket envelope + REST | one shared key, first message | not in repo |
| Zed (ACP client) | external agent | agent, or Zed fs/terminal | Zed spawns agent | ACP stdio | none | fs limited to worktrees; terminal unconfined |
| Warp client | Warp server (not in repo) | client | client to Warp server | HTTPS POST + SSE protobuf | bearer | client policy gate |
| OpenCodex Remote Workspace | hub | executor | executor to hub | WSS + per-session signed ECDH, AES-GCM | single-use code to bearer (hash stored); pinned hub key | paths, symlinks, hard links, root identity, CAS writes, `exec` without sandbox; intersects capabilities |
| OpenCodex Remote Link | child's Codex | child | child relay to Home through SSH tunnel | HTTP | link token; mutual HMAC challenge | strips caller credentials |

##### 3.2 Fencing, credentials, resume, frames, listener auth

| System | Fencing of stale workers | Credential per dispatch | Resume mechanism | Durable vs transient frames | Listener authentication |
|---|---|---|---|---|---|
| Codex app-server | file lock per thread (one host) | none | `thread/resume` by id/path/history; rejoin running thread | none distinguished on the wire | non-loopback refused without auth; loopback none; Origin 403; no post-upgrade expiry |
| Codex exec-server | one attachment per session id, 80 s retention; no epoch | rotating executor credentials over stdin (registry) | `resumeSessionId`; `process/read` cursor; relay `Resume{next_seq}` | relay `Data{seq}` with ack bits | `--listen` auth optional, non-loopback allowed |
| Codex remote control | server side, not in repo | enrolment token | `x-codex-subscribe-cursor` | envelope `seq_id` | outbound only |
| OpenCode | none; restart sweep assumes owner dead | none | clients refetch REST; log API (`after`, watermark, `log.synced`) exists but `persist` off | durable event table opt-in; SSE volatile | password required; `===`; `?auth_token=` accepted; SDK router unauthenticated |
| OpenClaw cloud worker | global `owner_epoch` revalidated per request; writer claim CAS checked inside the write | 32 bytes, domain-tagged hash bound to turn claim, 10 min | same claim-bound credential; same call id; commit ledger `(session, epoch, seq)` caches replies | `transcript.commit` (sequenced, ledgered) vs `live-event` | opaque `invalid-handshake`, failures rate-limited |
| OpenClaw node | (pairing revision; per-run approval binding) | none | not traced | `event` frames carry optional `seq` | device proof; token default; mode `none` exists |
| Hermes gateway | `runtime_epoch` + per-session `runtime_generation`, checked at settle | none | lost turns marked `unknown`, never replayed | admissions durable before ack | control socket peer uid; non-loopback dashboard needs auth; API key required |
| Hermes relay | 4401 after handshake is terminal | HMAC token, 300 s | connector redelivers unacked `bufferId` | `inbound_ack` after durable admission | n/a (dials out) |
| Oh My Pi | OS file lock per session; sibling file on conflict | none | RPC `get_entries {since}`; collab snapshot then entries | v2 chunking; collab sealed | RPC none (pipe); broker one bearer `Set.has`; gateway digests, `--no-auth` |
| Pi pi-env | pipe lifetime; 30 s silence kills process groups | none (SSH) | none: lost requests are `lost: true`, handles die | n/a | SSH only; token is a sync marker |
| DeepSeek | `flock` with no expiry (one host); SSH helper lease 30 s | TLS-PSK key per stream | `follow` snapshot per reopen + client gap repair; no cursor | durable events vs assistant frames with `revision` | Host/Origin fence + cookie; wildcard bind refused; launch token in URL |
| goose | none | none | `session/load` replays whole conversation (or tail) | n/a | secret required unless `--dangerously-unauthenticated`; `?token=` accepted |
| goose roaming | trust book re-read per connect; 2 s revocation poll closes connections | none | as above | n/a | QUIC-TLS key identity |
| Cline hub | none (single daemon) | provider key sent per session (SSH) | `stream.subscribe {sinceSequence}` from a 7-day log | log is a replay aid; messages on disk canonical | token or loopback `Origin`; ProtoBus gRPC unauthenticated |
| OpenHands | not in repo | per-conversation `session_api_key` on Cloud | `after_seq` with gap-tolerant cursor; `sync` frame names the range | `durable` / `transient` / `delta` frames, explicitly | shared key, first message; ingress has none |
| Zed | n/a | n/a | n/a | n/a | no listener |
| Warp | server side | BYO keys sent in every request; HPKE-sealed managed secrets | inbox `since=<seq>` with cursor stored server-side | event `sequence` | client is never a listener (ACP bridge one-time token) |
| OpenCodex Remote Workspace | latest connection wins per device; bearer has no expiry | none per dispatch; device bearer | none: sessions die with the connection | counter-bound encrypted frames | pairing route unauthenticated (Origin refused, rate-limited); agent route bearer |

##### 3.3 Listeners that can run without authentication

| Project | Listener | Condition | Citation |
|---|---|---|---|
| Codex | app-server `ws://` | loopback bind, no auth configured (default) | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/websocket-auth/src/lib.rs#L91-L96 |
| Codex | exec-server `--listen ws://IP:PORT` | any bind, auth optional | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server-cli/src/command.rs#L56-L160 |
| Codex | daemon control socket | always; protected by directory mode only | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/unix_socket.rs#L148-L246 |
| OpenCode | in-memory SDK router | built without a password | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/fetch.ts#L34-L36 |
| OpenCode | `/auth/connect/:code` | always (the code is the credential) | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/middleware/authorization.ts#L87-L94 |
| OpenClaw | Gateway | auth mode `none` | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/ws-connection/handshake-auth-helpers.ts#L165-L187 |
| Hermes | control socket | same-uid peer only, no secret | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/control_socket.py#L411-L425 |
| Oh My Pi | auth gateway | `--no-auth` (with `Access-Control-Allow-Origin: *`) | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/cli/auth-gateway-cli.ts#L452-L468 |
| Oh My Pi | browser relay | no token configured | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/browser/relay/server.ts#L130-L160 |
| Oh My Pi, Pi | RPC and ACP modes | always (stdio; the parent is the boundary) | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/modes/rpc/rpc-mode.ts#L401-L738 |
| goose | `goose serve` | `--dangerously-unauthenticated`, any bind | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-cli/src/cli.rs#L1821-L1834 |
| Cline | hub WebSocket | loopback bind plus a loopback `Origin` header | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/hub-websocket-server.ts#L676-L722 |
| Cline | core ProtoBus gRPC `127.0.0.1:26040` | always | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/vscode/src/standalone/protobus-service.ts#L11-L36 |
| Cline | hub dashboard `127.0.0.1:8787` | loopback default, no `ROOM_SECRET` | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/cline-hub/src/options.ts#L72-L84 |
| OpenHands | ingress | always (auth left to upstreams) | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/ingress.mjs#L185-L226 |
| OpenCodex | data plane | loopback bind (no token read) | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/auth-cors.ts#L337-L339 |
| OpenCodex | unauthenticated loopback companion | when configured (logged warning) | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/index.ts#L790-L802 |
| OpenCodex | `/remote-workspace/pair` | always (the code is the credential; Origin refused, rate-limited) | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/index/serve-options.ts#L484-L513 |

Health and version probes are open almost everywhere (Codex `/readyz` `/healthz`, goose
`/health` `/status`, Cline `/health` `/version`, Oh My Pi broker `/v1/healthz`).

### Session storage, resume, fork and checkpoints

From [round2/synth/06-sessions-fork-checkpoints.md](round2/synth/06-sessions-fork-checkpoints.md).

##### 3.1 Storage, durability, integrity, resume, retention

| Project | Storage format | Durable by default | Integrity and locking | Resume method | Retention default |
|---|---|---|---|---|---|
| Codex | JSONL "rollout" per thread, typed items, per-file ordinal; SQLite index | Yes for normal threads; file created lazily; `--ephemeral` opts out | Advisory file lock per thread plus 30 s coordinator; revision from path, size, times, inode | Reverse replay from newest full compaction; rollback markers; rejoin a running thread | None; zstd after 7 days; `archived_sessions/` |
| OpenCode | SQLite projections are truth; optional `event` table | Projections yes; event log no in the CLI (`persist` false) | Seq under per-aggregate mutex inside the same transaction; no fencing token | Execution claim written with `Started`; boot sweep sends a "continue" message; 10-attempt cap | None for sessions; tool-output files age out |
| OpenClaw | SQLite: nodes, windows (generations), `transcript_events` (JSON or zstd) | Yes | Writer claim by CAS, checked inside fenced writes; global owner epoch for workers | Fixed "check state before repeating" prompt; pending-input table | 30 days, 5,000 entries, 10 GiB budget, `enforce` |
| Hermes Agent | SQLite `state.db`, mutable rows, soft delete | Yes | `transcript_revision` triggers; runtime epoch plus generation fence; WAL refused on risky storage | Deepest descendant with messages; restore cwd; `unknown` turns never replayed | Prune and archive off by default (`retention_days: 90` when on) |
| Oh My Pi | JSONL tree, 256-byte title slot, header, `id`/`parentId` entries | Yes | OS file lease per session; hot append vs atomic rewrite | Replay file; leaf = last entry unless `activeLeafId`; cwd fallback | Nothing deleted; `omp gc` opt-in |
| Pi | JSONL tree, header, `id`/`parentId` | Yes once a message exists (`--no-session` in memory) | No lock, no fsync; migrations rewrite in place without a temp file | Leaf = last entry; reopen by mtime, path or id | None found |
| DeepSeek harness | JSONL as checksummed zstd frames, header first, contiguous `seq` | Yes; write-behind 200 ms plus flush before model request and tool body | Frame checksums, contiguity; `flock` with no expiry; no hash chain | Lock, read all, append synthetic closers | None found |
| goose | SQLite `messages` rows, no seq | Yes | `BEGIN IMMEDIATE`, timestamp clamp; in-memory one-run guard only | `session/load` replays conversation; pending approvals re-validated | None |
| Cline | JSON files per session plus SQLite index | Yes | Whole-file `writeFileSync` (torn write possible); hub log separate | Reload messages; compaction sidecar checked by prefix hash | Hub log 7 days / 64 MiB; sessions none found |
| OpenHands | Not in this repository | Not in this repository | n/a | Client: `after_seq` cursor, highest contiguous seq | n/a |
| Zed | Thread DB not read; action log in memory only | Action log: no | n/a | n/a | n/a |
| Warp | Local SQLite table named; server holds the conversation | Not settled | n/a | Not settled | n/a |
| OpenCodex | Nothing read | n/a | n/a | n/a | n/a |

##### 3.2 Fork, branch, active-branch record, export and import

| Project | Fork representation | Cut rule | What the fork carries | Active-branch record | Export and import |
|---|---|---|---|---|---|
| Codex | Pointer: `history_base {rollout_id, end_ordinal, end_byte_offset}` over the parent's immutable file; revert = new file under the same thread id, path swapped by CAS | `lastTurnId`, `beforeTurnId` | Conversation only; `thread/revert` does not touch files | The file the SQLite row points to (inferred) | Not described |
| OpenCode | Copy of settled message rows into a new `session_v2` row; `fork_session_id`, `fork_boundary` | Newest or a given message; unfinished rows skipped | Conversation; cost zeroed; no files | None (revert deletes rows after the boundary) | Projection-based export; `--sanitize`; import refuses an existing id |
| OpenClaw | Copy of completed prefix; refused if over a token ceiling; windows with `reason` and `previous_session_id` | Completed prefix | Conversation | Window generations | Not described |
| Hermes Agent | Copy of active rows in one transaction; `parent_session_id`, `_branched_from` | A message id, extended over its tool results; parent must be idle | Conversation; approval reset to `ask`; no files | `active` flag per row | `jsonl`, `md`, `html`, `trace`; trace redacts and aborts if redaction fails |
| Oh My Pi | In-file leaf move (`/tree`); new file for `/branch` and `/fork` (`parentSession` in header) | User message for `/branch` | Conversation path; artifacts on `/fork` | Last physical entry unless `activeLeafId` | HTML export; sealed `/share` with redaction; imports Claude Code and Codex |
| Pi | In-file leaf move; new file for fork or clone (root-to-leaf path only; `parentSession` = path) | `before` a user message or `at` | One path, siblings dropped | Last entry in the file (not stored) | HTML with all branches, JSONL current branch; no redaction found |
| DeepSeek harness | Copy of exact prefix into a new log; `session/end-seed` marker; `isSeeded`, `inheritedEventCount` | Any seq; open turn closed with `forked` closers; web default is the last completed turn | Whole prefix on disk; forked workspace if the parent had one | Not applicable (linear logs) | ZIP of canonical JSONL; no redaction |
| goose | Copy of the whole conversation, then optional timestamp truncation; `"<name> (copy)"` | Timestamp; no run check | Conversation only | None | JSON, YAML, Markdown, HTML; imports Claude Code, Codex, Pi |
| Cline | Restore with messages starts a new session with fork metadata; old session kept | A user turn not folded into a compaction | Messages plus (optionally) the workspace restore | None | Imports Claude Code, Codex, OpenCode |
| OpenHands | Server-side fork from `from_event_id`; client copies metadata | Event id | No files | Not in this repository | Server trajectory download |
| Zed | Not in the input | n/a | n/a | n/a | n/a |
| Warp | Server fork of the conversation on handoff | n/a | Plus a workspace snapshot | n/a | n/a |
| pi-durable | Parent pointer over linear conversations, nothing copied; documents copied as of the fork | Entry id | Documents per fork policy | Per conversation | Not described |

##### 3.3 Checkpoints and undo

| Project | Mechanism | Trigger | What a checkpoint scans | Shell-made changes | Restore safeguards | Cost guards |
|---|---|---|---|---|---|---|
| Codex | None (removed; old records stripped on read) | n/a | n/a | n/a | n/a | n/a |
| OpenCode | Shadow Git store in the data dir; alternates to project objects; index seeded from the project index; tree id = snapshot id | Start and end of each model step | `diff-files` plus `ls-files --others` over every file (O(tree)) | Yes (tree diff) for git projects; ignored files and files outside the project no | Stage then clear or commit; busy refusal while running; only attributed paths restored; user edits to attributed files overwritten | 2 MiB untracked cap; temp index renamed; coalescing; fingerprint shortcut; `untrackedCache`, `manyFiles`, index v4; packing; no retention |
| OpenClaw | Refs `refs/openclaw/snapshots/<id>`; worker-result refs | Worktree removal; worker turns | Not described | Not described | Strict ref regex; nested repos refused; GC resolves uncertainty to keep | 7-day idle GC; owner check |
| Hermes Agent | One bare shadow repo, ref and index per project, isolated env | Before `write_file`/`patch`; before `terminal` if the command text matches a regex; once per directory per turn; off by default | Whole project tree via `git add` into the project index, with exclusions | Only when the regex matches the command; safe restore skips shell-changed files not in the ledger | Pre-rollback snapshot; safe mode with write ledger; nested repos refused; oversize files kept | 50,000-file skip; 10 MB per file; 20 snapshots; 500 MB; 7-day sweep; `/` and `$HOME` ancestors refused; containers excluded |
| Oh My Pi | None for files; `checkpoint`/`rewind` rewind the conversation | n/a | n/a | n/a | n/a | n/a |
| Pi | None built in; example extension uses `git stash create` in memory | Each `turn_start` | Tracked files only | No (untracked ignored) | `stash apply` offered on fork only with a UI | None |
| DeepSeek harness | None; per-turn tree recorder for display only (private index and object dir) | Turn start and end | `git add --all` into a private index | Yes for git paths (tree diff); outside git only file-tool edits | No restore exists | 30 s per git command; output cap; 500 files per summary |
| goose | None | n/a | n/a | n/a | n/a | n/a |
| Cline | `git stash create` commits in the user's repo under `refs/cline/checkpoints/<session>/<run>`; untracked files as a third parent via a private index | First model call of each user run; opt-in core, on in CLI and VS Code | Tracked changes via stash; untracked non-ignored via `ls-files` and a persistent private index | Yes (tree-based); ignored files and anything outside the repo no | Snapshot of the present first; `HEAD` guard; `update-ref` compare-and-swap; `clean -fd` only if untracked were captured; messages restore forks a session | 64 MiB `ls-files` buffer; persistent index; no size, count or total cap; refs removed only with the session |
| OpenHands | Not in this repository | n/a | n/a | n/a | n/a | n/a |
| Zed | Per-prompt checkpoints removed; archive checkpoint (two commits via a temp index) on thread archive; in-memory per-buffer baselines for review | Archive; first agent touch of a buffer | Archive: `git add --all` in a temp index copy; baselines: only buffers the tools touched | No: no hook from `terminal` | Reject restores base text; rewind rejects all unreviewed edits; archive restore needs HEAD at the original commit | Baselines cost nothing per repository size; archive has no timeout |
| Warp | Handoff snapshot (patches and manifest), not undo | `/handoff` | Repos derived from the last 500 tool calls: written paths plus cwds | Only paths the agent wrote | Refuses while a command or child is active; read-only path gate | 100 files, 25 MiB per file, 25 per request |
| OpenCodex | Nothing read | n/a | n/a | n/a | n/a | n/a |

### Context: prompts, compaction, instruction files, memory

From [round2/synth/07-context-compaction-memory.md](round2/synth/07-context-compaction-memory.md).

Cells are short; the account in section 2 has the detail and the permalinks. "n/d" means the input files do not describe it; "not in repo" means the code that was read does not hold it.

##### 3.1 Compaction: trigger, measurement, summariser

| Project | Trigger and numbers | How size is measured | Summariser model | Window check for the summariser |
|---|---|---|---|---|
| Codex | `min(config, 0.9 x window)`; pre-turn, mid-turn, post-turn (off by default); keeps user messages up to 20,000 tokens; remote V2 keeps up to 64,000 | n/d (status over active tokens; optional prefix baseline) | Same turn model, no tools; OpenAI and Azure use an opaque server-side item | n/d; downshift compacts on the previous model first |
| OpenCode | Before each step at `window - max(15% of window, 16k)` (16k floor only for windows of 32k or more); keeps newest 15,000 tokens; unknown window means it never fires | Last provider-measured count plus length/4 for the tail; images 1,500, PDFs 2,000 | Same model; `agents.compaction.model` overrides | None described; request is fitted to size by a ladder |
| OpenClaw | After each response at `min(window - reserve, 0.75 x window)`; reserve `max(configured, 20k)` capped at 25% of window; keeps 20,000 | Provider usage; estimate from last good usage after an error | Session model; `compaction.model` unless model-locked; fallback chain | None found |
| Hermes Agent | `(window - max_tokens) x 0.50`, floor 0.75 under 512K, cap 85% of usable window; lean tail 2.5% of window clamped 10K..25K | Provider prompt tokens; rough estimate before first response; 413 scored in serialised bytes | `compression` auxiliary slot (main model if unset); 300 s timeout floor | Feasibility probe; trigger lowered to the summariser window; hard error below a minimum |
| Oh My Pi | Window minus max(15% of window, 16,384), or a percentage, or fixed; keep 20,000; summary cap 16,384; 0.8 hysteresis; mid-turn on | Token count floored by a local estimate of stored conversation | `compactionModel`, current, role models, then largest-context model | Handled by selection; no explicit check |
| Pi | `tokens > window - 16,384`; keep about 20,000 | Last provider usage plus chars/4; usage older than the last compaction ignored; image = 4,800 chars | Session's own model | None; no input-size guard |
| DeepSeek harness | `min(0.8 x window, window - reserved output - 65,536)` before every step; keep 16% of message budget | Token meter over the logged request header (route-specific) | Conversation's own route unless overridden | Window of the routed model must be known; model switch compacts first or refuses |
| goose | `min(0.8 x window, 225,000)` before every inference; no verbatim tail, last user text only | Last provider `total_tokens` plus estimate; tiktoken `o200k_base` if no usage | Session model, thinking off, cache writes off | Staged tool-result removal (0, 10, 20, 50, 100%) if the summariser overflows |
| Cline | Before each request at 90% of input limit (about 81% of window when only the window is known); target 70% of trigger; keep last 20,000 estimated tokens; off in core, on in CLI | About 3 chars/token plus images, scaled down by the provider-count ratio (cap 4x); unknown limit means no auto compaction (128,000 fallback for manual and overflow) | Session model unless configured; thinking off; 8,192-token output | n/d; only "input budget exhausted" skips |
| OpenHands | Client `max_size` 240 (planner 100, `keep_first` 6); trigger unit not in repo | not in repo | Planner condenser has its own usage id; rest not in repo | not in repo |
| Zed | Before every request at 90% (or tokens used or remaining) of capacity; auto only if capacity is at least 80,000; keep last 80,000 bytes of user messages | Provider usage of the previous request | `agent.compaction_model`, else thread model | None found |
| Warp | not in repo (server) | not in repo | not in repo | not in repo |
| OpenCodex | The calling client decides; gateway only forwards or runs the summary turn | None | The routed target model | None |

##### 3.2 Compaction: request shape, fallbacks, shrink check, overflow, log

| Project | Summary request reuses the main prefix? | Staged fallback | Check that it shrank | Context-length error | Record in the log and failure midway |
|---|---|---|---|---|---|
| Codex | Full history plus one appended user message, no tools (cache intent not stated, inferred) | Oldest-item drop inside compaction | n/d | In compaction: drop oldest and retry; in sampling: mark full, end turn; guardian: one compact and retry | Compaction item with `replacement_history`; atomic replace; failure leaves history unchanged |
| OpenCode | Yes by design: same system, tools, history plus a final user message | `deliver` ladder: unchanged, history minus tail, flattened transcript clipped over five levels; shrink to 85/70/50/35% on "too long" | Not described (the ladder checks the input, not the output) | Compact once per step then restart the step; no recovery after output started | Message with `keepID`; `Compaction.Ended` advances the instruction epoch; failed auto compaction fails the step |
| OpenClaw | n/d | Safeguard: audit and regenerate (max 3), then structured fallback marked degraded | Quality audit of content, not size | Pre-exits, up to 3 compactions, one tool-result truncation pass | Transcript generation with reason `compaction` (inferred); failure state not described |
| Hermes Agent | No (single user message) | Failure ladder: retry on main model, cooldowns, abort-and-preserve for terminal errors, else deterministic summary | "Ineffective" breaker after two confirmed no-shrink compactions | 413: compress, strip images, retry; "too long": adopt provider limit; "output cap": clamp; max 3 | In place on the same session id; old rows `active=0, compacted=1`; abort leaves history unchanged |
| Oh My Pi | n/d | Method chain `remote, snapcompact, handoff, shake, soft`, each failure falls to the next | Yes: a summary that would not reduce context, or a shake that reclaims too little, falls through | Drop failed turn; optional model promotion; else compaction with reason `overflow` | `compaction` entry with `firstKeptEntryId`, `tokensBefore`, `method`; nothing deleted |
| Pi | No (`cacheRetention: "none"`, serialised text) | None | None | One compact-and-retry per run; failed attempt hidden by `context_edit`; second failure stops | `compaction` entry; old entries kept; failure leaves the log untouched except omission edits |
| DeepSeek harness | Yes: same system prompt, tools and shadowed messages, instruction as final user message | Prune, then compact with zero retention, then retry | Yes: summary rejected unless its framed estimate is strictly smaller than the span | `CONTEXT_WINDOW_EXCEEDED`: prune, compact with zero retention, retry (1 per idle period) | `compaction/start`, `summary`, replacing `user/message`, `end`; failure appends `end` with error; orphan blocks until `session/end-seed` |
| goose | No (flattened transcript in a system prompt) | Tool-result removal inside the summary request | n/d (absence not confirmed) | Hide error, compact, rerun; up to 3 per turn | Old rows kept, `agent_visible` flipped; summary row flagged |
| Cline | No (its own summariser request) | `agentic` then deterministic `basic`; overflow uses only `basic` or a strictly smaller custom compactor | Yes: retry refused unless the request shrank | One forced compaction and one retry | Transcript untouched; compacted view in sidecar `<id>.compaction.json` with source-prefix hash |
| OpenHands | not in repo | not in repo | UI waits for a token drop after manual condense | not in repo | `Condensation` event; log keeps everything |
| Zed | Yes when the same model: tools kept with `tool_choice: none` | None | n/d | None (error shown) | `Message::Compaction(Summary)` inserted; older messages stay for UI |
| Warp | not in repo | not in repo | not in repo | Client shows `ContextWindowExceeded` as an error | Local `agent_conversations` summary column (contents unknown) |
| OpenCodex | The internal turn goes through the normal pipeline | None | Structure only (non-empty, no tool calls) | Combo failover hops to the next target | None |

##### 3.3 Prompt assembly, cache strategy, pruning

| Project | Where prompt text lives | How dynamic state is kept out of the cached part | Old tool output |
|---|---|---|---|
| Codex | Per-model `instructions_template` inside the bundled `models.json`; sections in code | Per-section snapshots; only changed sections rendered as diffs into history; world state persisted in the rollout | Middle-cut at record time (10,000 tokens); never pruned later |
| OpenCode | `system.txt` (15 lines) plus per-model overlay files chosen by substring | Keyed instruction sources: frozen baseline in `system[1]`, changes appended as system messages, frozen in durable events; re-based at compaction | None; call-time cut (2,000 lines or 50 KiB, full text on disk 7 days); images bounded |
| OpenClaw | Code builders, no templates | Everything volatile placed below `SYSTEM_PROMPT_CACHE_BOUNDARY`; prefix cached by hash | Live cap 30% of window; optional cache-TTL soft trim then hard clear, frozen and recorded |
| Hermes Agent | Code plus data in `agent/`; three tiers | Stable, context, volatile tiers built once and rebuilt only at compaction; four Anthropic breakpoints; review fork reuses parent bytes | Model-free projection at compaction; call-time spill (100K chars, 1,500-char preview) |
| Oh My Pi | Handlebars templates | Static blocks first, one trailing `<project-context>` block; pruning waits for a cold cache | `supersedeReads`, `dropUseless` each turn, cache-aware; snapcompact caps results at 2,000 chars |
| Pi | Code sections; tool snippet next to each tool | Prompt and tool changes are system-message patches appended to the transcript | None between compactions; per-tool caps (2,000 lines, 50 KB) |
| DeepSeek harness | Ordered named sections from a table, per-deployment YAML | Dynamic facts in a per-step `runtime-context` user message, appended only when changed; system prompt re-committed only on change | Model-free prune of results over 8,192 chars; spill over 12,500 tokens |
| goose | `system.md` template (39 lines) plus keyed extras sorted by name | Volatile data in a persisted trailing `<turn-context>` message | Spill over 200,000 chars; optional LLM tool-pair summaries (off) |
| Cline | One template with slots; per-model difference only in the tool list | Mode tag on each user message; stale-read rewrites batched at 64 KB | Caps at executor and at request build with a recoverable `cline://cache` URI |
| OpenHands | Server (not in repo) | not in repo | not in repo |
| Zed | not in the study | Only the compaction request keeps tools to reuse the prefix | None separate; terminal capped at 16 KiB |
| Warp | Server (not in repo) | not in repo | not in repo |
| OpenCodex | Client | not applicable | None |

##### 3.4 Instruction files

| Project | Files and order | Budget | Trust |
|---|---|---|---|
| Codex | Global `AGENTS.override.md` or `AGENTS.md`; then root to cwd, one file per directory (`AGENTS.override.md`, `AGENTS.md`, fallbacks) | 32 KiB combined, warn on cut; global file separate; thread-level 10,000 tokens, rejected not cut | Untrusted project returns nothing; root markers only from non-project config layers |
| OpenCode | Global `AGENTS.md`, ancestors to project root, `instructions` globs and URLs; nested files injected after `read` | None found | No project trust prompt found; remote URL must be listed in config |
| OpenClaw | Six workspace files: `AGENTS.md, SOUL.md, IDENTITY.md, USER.md, BOOTSTRAP.md, MEMORY.md` | 20,000 each, 60,000 total; `AGENTS.md` cut with a policy digest | Per-session-type filter (`MEMORY.md` dropped for group, channel, sub-agent, cron); no trust gate described |
| Hermes Agent | First type wins: `.hermes.md`, `AGENTS.md` chain, `CLAUDE.md`, cursor rules | 20,000 chars per file or 6% of window (20K..500K); head 70%, tail 20% | Regex threat scan; a hit replaces a repo context file with `[BLOCKED]` |
| Oh My Pi | Many harness formats by priority, one file per depth, paragraph dedupe, `@path` imports | None reported | No project trust gate (`isProjectTrusted()` always true) |
| Pi | Global, then every ancestor to the filesystem root; one of five names per directory | None | Context files load regardless of project trust |
| DeepSeek harness | `AGENTS.md`, `CLAUDE.md` and `.local` variants; user-global then root to cwd; delivered as user messages | 64 KiB per chain, 1 MiB per file | No trust gate in code read |
| goose | `.goosehints`, `AGENTS.md`; global then git root to cwd; `@` imports | Main file uncapped; imports depth 3, 1 MiB | No trust prompt; Unicode tag strip only |
| Cline | Workspace `AGENTS.md`, `.clinerules`, `.cline/rules`, global `AGENTS.md`, global dirs | None found | No trust prompt for workspace plugins and hooks |
| OpenHands | Server (not in repo); skills from a bundle plus server; `.openhands/hooks.json` | not in repo | No prompt for hooks; fails open |
| Zed, Warp, OpenCodex | not covered | not covered | not covered |

##### 3.5 Memory

| Project | What is stored | Size limits | Who writes | Human approval | Review, ledger, rollback | Session search |
|---|---|---|---|---|---|---|
| Codex | `MEMORY.md`, `memory_summary.md`, rollout summaries, skills; ad-hoc notes | Injected summary 2,500 tokens | Background two-phase pipeline; chat note tool (off by default) | None | None described | None in findings |
| OpenCode | No memory feature found | n/a | n/a | n/a | n/a | None found |
| OpenClaw | `MEMORY.md`, `memory/*.md`, short-term recall | `MEMORY.md` 10,000 chars | Model tools, flush turn before compaction, nightly dreaming job | None | 25% removal guard on consolidation; Skill Workshop changes versioned and undoable | `memory_search` hybrid vector and keyword |
| Hermes Agent | `MEMORY.md` 2,200 chars, `USER.md` 1,375 chars; agent-written skills | As stated | Chat model; background review fork; curator | Off by default; when on: inline prompt or staging queue; background review always staged | Curator ledger with before/after hashes, per-entry rollback, tarball snapshots | FTS5 `session_search`, no LLM |
| Oh My Pi | Five back ends (default off): local, Hindsight, Mnemopi, Sharpshooter | Outbox: 1,000 undelivered events per destination | Automatic extraction or turn counter | None | None reported | Not found |
| Pi | No memory feature found | n/a | n/a | n/a | n/a | Not found |
| DeepSeek harness | No model-written memory (absence confirmed) | n/a | n/a | n/a | n/a | Optional full-text index, in-memory in the web profile |
| goose | `<category>.txt` files, local and global | None described | Chat model | "Confirm with user" in the prompt only | None | None |
| Cline | Not found | n/a | n/a | n/a | n/a | Not found |
| OpenHands | SDK (not in repo) | not in repo | not in repo | not in repo | not in repo | Event search through the server |
| Zed, Warp, OpenCodex | not covered | not covered | not covered | not covered | not covered | not covered |

### Edit tools and the mechanics behind the scenes

From [round2/synth/08-edit-tools-mechanics.md](round2/synth/08-edit-tools-mechanics.md).

Labels in cells follow section 2. "None found" means the author searched and the file says so; it is `absence confirmed` only where section 2 says so.

##### 3.1 Edit tools

| Project | Edit format | Match ladder | Staleness check | Read-before-edit | Atomicity | Post-edit step | Tracking for review |
|---|---|---|---|---|---|---|---|
| Codex | Patch envelope (`apply_patch`), multi-file, move, delete | 4 tiers, tier-first, first match from a cursor; no uniqueness | None; approval not bound to content hash | None | Verify all in memory, then apply sequentially from re-parsed text; partial writes stay | None found (hooks only) | None (file undo removed) |
| OpenCode | `edit` string replace; `patch` envelope for GPT-family models | `edit`: exact, Unicode fold, trailing-ws lines; `patch`: exact, trimEnd, trim, trim+Unicode | None (process-local per-path lock) | None | `edit` single file; `patch` prepares all, writes sequentially, reports partial | Formatter by extension (first success wins); LSP is a TODO | Shadow Git tree captured around each model step, path attribution, stage/commit undo |
| OpenClaw | `edit` multi-edit; `apply_patch` | Exact, then NFKC/trailing-ws/quote fold used for lookup only; unique; error lists similar lines | None; per-file queue; re-reads after write to verify | None | Per file (queue + verify) | None found | Worktree snapshot git refs (unchecked) |
| Hermes | `patch` (replace and V4A), `write_file` | 9 strategies incl. indentation and similarity (0.5/0.7, 0.8); `replace_all` refused for similarity | `patch`: mtime warning only; `write_file`: hard refusal | `write_file` enforced; `patch` warns | Not read | Lint output delta (new errors only); LSP unread | Shadow git per project, write ledger fed by file tools only; off by default |
| Oh My Pi | Hashline (default), `apply_patch`, `patch`, `replace`, `sloppy` | `replace`: exact, 0.99/0.98/0.975/0.97, fuzzy 0.95 with 0.97 and 0.08 margin | Hashline: 16-bit file tag; stale tag rebased if provably safe, else refused | Hashline: seen-line guard in code; `replace`: prompt only | Hashline: all sections staged before any write; `sloppy`: partial | Not traced | Conversation-only rewind; no file undo |
| Pi | `edit` multi-edit, matched against the original | Exact, then NFKC/ws/quote fold of the whole file; unique; whole touched lines rewritten from normalised text | None; per-file queue | None | All edits in memory, one write; failure leaves file untouched | None | None built in |
| DeepSeek | `edit` literal; `apply_patch` off | None (literal, CRLF-normalised); unique | CAS inside the lock via `dev:ino:size:mtimeNs:ctimeNs`, **plugin disabled in shipped base** | Same plugin, disabled; prompt still asks | Atomic write under per-target lock | None found | Per-turn git tree recorder, memory only, no restore |
| goose | `edit`, `write` | None (exact, unique) | None | None (no read tool) | `fs::write`, not atomic | None (`AfterFileEdit` hook exists) | None |
| Cline | `editor` replace; `apply_patch` (per model) | `editor`: none; patch: exact, trailing-ws, trim, similarity 0.66 | None | None | Patch: reject whole patch before writing, then sequential writes | None found | `git stash create` commits under private refs, per user turn; restore transaction |
| OpenHands | In another repository | Not read | Not read | Not read | Not read | Not read | Not read |
| Zed | `edit_file` fuzzy line matcher, streamed into the buffer | Exact, then DP alignment with Levenshtein 0.8 per line and 0.8 whole-match ratio, line hint | Soft: mtime vs last read, appended to failure text | None (prompt only) | Applied to buffer, saved on completion even on later-edit failure | Optional format-on-save | In-memory action log with per-buffer baselines, per-hunk keep/reject |
| Warp | On server | Not read | Not read | Not read | Not read | Not read | Diff review UI in client (documentation only) |
| OpenCodex | `write_file` with `expectedSha256` | n/a (whole file) | Hard: SHA-256 CAS | Implicit (hash needed) | Temp file + atomic rename, serial chain | None | None |

##### 3.2 Match ladders, tier by tier

`E` exact, `RS` trailing whitespace ignored, `T` trim both sides (ignores indentation), `U` Unicode fold (curly quotes, dashes, odd spaces; NFKC in Pi/OpenClaw), `S` similarity score. "Unique?" is whether a match found at the winning tier must be unique.

| Project / tool | E | RS | T | U | Other tiers | Unique? | Reports tier to the model? |
|---|---|---|---|---|---|---|---|
| Codex `apply_patch` | 1 | 2 | 3 | 4 (with T) | retry without trailing empty line | No (first match from cursor) | No |
| OpenCode `edit` | 1 | 3 (line-wise) | no | 2 | none | Yes; `replaceAll` | Tool description tells the model |
| OpenCode `patch` | 1 | 2 | 3 | 4 | none | Not stated | No |
| Pi `edit` | 1 | in fuzzy space | in fuzzy space | in fuzzy space (NFKC) | none | Yes (counted in normalised space) | No |
| OpenClaw `edit` | 1 | in fuzzy space | in fuzzy space | in fuzzy space (NFKC) | none | Yes | Not stated |
| Hermes `patch` | 1 | `line_trimmed` 2 | `whitespace_normalized` 3, `indentation_flexible` 4 | `unicode_normalized` 7 | `escape_normalized`, `trimmed_boundary`, `block_anchor` (0.5/0.7), `context_aware` (0.8) | Yes, else lists locations; no `replace_all` after similarity | Not stated |
| Oh My Pi `replace` | 1 | 0.99 | 0.98 | 0.97 | comment-prefix 0.975; fuzzy prefix and substring at 0.95 | Yes: fuzzy winner needs 0.97 and a 0.08 margin | Not stated |
| Cline `apply_patch` | 1 | fuzz 1 | fuzz 100 | canonicalised first | similarity 0.66 (fuzz 1000), EOF penalty | Not stated | **Yes, fuzz level in the success text** |
| Zed `edit_file` | 1 | no | no | no | Levenshtein 0.8 per line, 0.8 ratio, costs 1/3/10 | Yes via line hint, else line numbers | Internal `is_exact`; user sees diff |
| DeepSeek, goose, Cline `editor` | 1 | no | no | no | none | Yes | n/a |

##### 3.3 Hash-anchored edits: described versus implemented

| Property | How it was described (Round 1) | What the code does (Oh My Pi) | Label |
|---|---|---|---|
| Hash granularity | Every line carries a short content hash (`2:f1|...`) | One 16-bit xxh32 tag **per file**; lines are bare numbers | checked in review |
| What a tag detects | A changed file makes hashes mismatch and the edit is rejected "before corruption" | Tag mismatch triggers recovery; rejected only if a uniform rebase cannot be proved | corrected in review |
| Per-line protection | Implied by per-line hashes | The **seen-line guard**: every anchored line must have been shown in full by a read | checked in review |
| Collision | Not described | 1 in 65,536; a colliding live tag skips recovery entirely, guarded only by seen lines | checked in review (consequence inferred) |
| After restart | Not described | Snapshots and seen-line provenance are in memory and are lost; only over-long lines are refused | checked in review |
| Universality | Hashline presented as the edit format | Default, but substring-selected models (kimi, deepseek, glm flash, qwen 3.8 and others) get `replace` instead | checked in review |
| Read-before-edit | Prompt: "Tool errors on edit before read" | Enforced for hashline (tag and seen rows); not enforced in `replace` | absence confirmed |

##### 3.4 Shell execution mechanics

| Project | Spawn | Default timeout; model-settable? | Kill | Background | After normal exit | Output cap (model sees) | Environment |
|---|---|---|---|---|---|---|---|
| Codex | Unified exec, resumable processes | 250 ms to 30 s yield per call; 300 s background max | Pruned process terminated; table of 64, 8 recent protected | `write_stdin` polls | Held in process table | 10,000 tokens per call; 1 MiB per process buffer, head and tail | Inherits (safety file) |
| OpenCode | `-c`, detached group, `stdin` ignore | 2 min; background none | 3 s force kill | `background` flag; Rust PTY daemon survives restarts | Not stated | 2,000 lines / 50 KiB, full output file, 7-day cleanup | Not scrubbed |
| OpenClaw | Gateway/node/sandbox host | 48 h per run | Not read | Not read | Not read | 30% of window, hard cap 16k/32k/64k chars | Inherited minus injection keys |
| Hermes | Local; `base.py` | Over 600 s foreground promoted to background | Process group; owner-death supervisor | `background`, trailing `&`; `notify_on_complete` | Tracked | Spill at 100,000 chars; 1,500-char preview | Not covered here |
| Oh My Pi | Persistent native shell per key; fork for concurrent | 300 s default, 1 to 3600 s, **0 disables** | SIGTERM tree, grace, re-walk, SIGKILL; session quarantined | Auto after 60 s; `async`; services | Shell retained while jobs live | Spill at 50 KiB; 20 KiB head + 20 KiB tail; artifact cap 16 MiB | Inherited (safety file) |
| Pi | `spawn(shell, ["-c"], detached)` | **None** | SIGKILL group (SIGSTOP first on macOS) | None | Detached children killed on Pi SIGHUP/SIGTERM only | 2,000 lines / 50 KB tail, full file | Full parent |
| DeepSeek | Fresh shell (docs) | Documented only | `SANDBOX_UNAVAILABLE` mapping | Documented only | Not read | Spill at 12,500 tokens | Name-pattern scrub (safety file) |
| goose | `-c`, own group (Job Object on Windows) | 300 s; model sets `timeout_secs`; **0 disables**; no upper bound | `kill(-pgid, SIGKILL)` on timeout, cancel, drop | None | **Disarmed**: backgrounded work survives; `setsid` escapes; no PDEATHSIG for shell | 2,000 lines / 50,000 bytes, 8 rotating files, tail 50 lines; collection unbounded | Full inherited |
| Cline | `spawn`, `detached: true` | 30 s (metadata says 60 s, not enforced) | SIGKILL group; `setsid` escapes | Human-only "Proceed While Running", `unref` + log | Detached survives the run | 48,000 chars, 24k head + 24k tail | Full inherited |
| Zed | Terminal tool | Not read | Not read | Terminal threads | Not read | 16 KiB; `head_lines`/`tail_lines` | Not read |
| Warp | User's own PTY | Agent-set poll delay up to 120 s | Not read | Long-running blocks, control hand-off | Not read | Not read | User's shell env |
| OpenCodex | `bwrap` argv, no shell | 1 to 60 s | Kill on timeout, abort, output overflow; `--die-with-parent` | None | Not applicable | 256 KiB | `--clearenv` plus 4 variables |
| rizzy-harness (`main`) | `bash -c`, own process group, allow-listed env | 600 s per tool call; `timeout_secs` capped at 3600 | SIGTERM, 2 s, SIGKILL on the group | None | Not in inventory | 30,000 chars head and tail; stream 1 MiB; filtered text 2 MB retrievable | Allow-listed |

##### 3.5 Read, search and tool-result caps

| Project | Read / search caps | Whole-result cap | Overflow handling |
|---|---|---|---|
| Pi | `read` 2,000 lines / 50 KB head; `grep` plus 500 chars per match line | Each built-in tool; none in the loop or for extension tools | Bash spills full output to temp file |
| Cline | Reads 2,000 lines, 2,000 chars per line, 48,000 chars per window; search 48,000 per query | 8,000 chars (50,000 for reads) re-cut at request build; 6 MB aggregate | `cline://cache/...` URI, 16 MiB cache; stale re-reads rewritten |
| OpenCode | Read pages 2,000 lines / 50 KiB (Round 1 documentation) | 2,000 lines / 50 KiB per result | Full output file, 7 days; images 25 to 15 MiB |
| OpenClaw | None in findings | min(30% window, 16k/32k/64k chars) | Truncate with "narrow args" note |
| Hermes | `read_file` pinned to infinity (exempt from spill); `search_files` 4th identical call refused | 100,000 chars; 200,000 per turn | Preview 1,500 chars plus path |
| DeepSeek | `read` exempt | 12,500 tokens | Head/tail plus spill file; **fails open to inline** |
| goose | No read or search tool | 200,000 chars generic; shell 2,000 lines / 50,000 bytes | Temp file, 8 slots |
| Codex | Not in findings | 10,000 tokens per exec call | Middle truncation marker |
| rizzy-harness (`main`) | `read` 400-line window; 32 MiB streaming cap only on `integration/gate3`; `grep` 200 results | 30,000 chars | `tool_output` re-read by id |

##### 3.6 Tracking agent edits for review or undo

| Project | Mechanism | Granularity | Covers shell-made changes? | Cost control | Persisted? |
|---|---|---|---|---|---|
| OpenCode | Shadow git, tree before and after each model step | Per step, path list | Only inside git tree; ignored files and outside-project are not | Seeded index, alternates, coalescing, packing; O(tree) per step; no retention | In the store |
| Hermes | Shared bare repo, ref and index per project | Before write-class tool, or regex-matched destructive command; one per dir per turn | Partly (regex); **restore ledger ignores them** | 50,000-file skip, 10 MB file drop, 20 snapshots, 500 MB, 7 days; off by default | Disk |
| Cline | `git stash create` + untracked third parent, private refs | Once per user turn | Yes (whole tree), not gitignored | Persistent private index; no size caps | Disk |
| DeepSeek | Git trees at turn start and end, private index, alternates | Per turn | Yes within git | 30 s per git call, 500 files, output cap | Memory only |
| Zed | In-memory action log, baseline at first touch | Per buffer | **No** | Nothing scanned | **No** |
| OpenClaw | Worktree snapshot refs | Per worktree | Not read | Fail to keep | Refs |
| Codex, goose, Pi, Oh My Pi (files) | None | | | | |
| rizzy-harness (`main`) | `git add -A` into a private index of a shadow repo, before and after each agentic turn, latest 10 kept | Per turn, whole tree | Yes | Count cap only; scans the whole tree | Disk |

### Sub-agents, orchestration, model roles, fallback and retry

From [round2/synth/09-subagents-models-retry.md](round2/synth/09-subagents-models-retry.md).

##### 3.1 Sub-agents

Trust labels are in section 2; cells give the main fact only. "Shared" means the child works in the parent's directory.

| Project | Creation | Context given | Permission relation to parent | Isolation | Result return | Caps (real defaults) |
|---|---|---|---|---|---|---|
| Codex | In-process child thread from `spawn_agent`; guard object cleans up a cancelled spawn | Prompt only; `fork_context: true` copies full history | Inherits; a role patch can only narrow; parent policy re-applied last | None found (shared) | Last assistant message in a status value, plus a queue-only note in the parent's mailbox; no schema | Depth 1; 6 threads (V1) or 3 children (V2); no time or token budget |
| OpenCode | Child session row with `parentID`, run as a job | Fresh; prompt only | Child agent's rules plus the parent session's rules (session rules last); "read-only" `explore` allows shell | None found | Last completed assistant text in `<subagent>`; background mode posts a synthetic message that wakes the parent | Depth 1 (`experimental.subagent_depth`); no concurrency cap found; optional per-agent `steps` |
| OpenClaw | Child session plus gateway `agent` run | Isolated by default; `fork` copies transcript; `AGENTS.md` only | Inherits parent's allow and deny lists; hard deny list for hidden children; leaves cannot spawn | Optional `worktree` at session creation; idle GC 7 days, keeps when unsure | Four modes (`announce`, `collector`, `quiet`, `thread-direct`); steered into the parent's turn or a direct turn with an idempotency key | Depth 5; 5 active children per spawner; width-8 lane per spawner |
| Hermes Agent | `AIAgent` objects built on the parent's thread, run on a thread pool | Goal, parent-written `CONTEXT`, workspace path, project context files; no memory | Parent's toolsets minus 5 tools; dangerous commands auto-deny (opt-in approve) | Opt-in worktree (off by default), silent degrade off the local backend | Summary only: redacted, 24,000-char cap, JSON with `status` and `exit_reason`; background results re-enter as a message | 10 concurrent; depth 1 (no ceiling); 250 iterations per child; 50 spawns per turn |
| Oh My Pi | In-process `task` tool (main thread, then workers) | Role prompt plus parent-written `context`; no parent conversation; `AGENTS.md` excluded | Forced `yolo`; user deny and prompt rules still apply; prompts fail closed | Opt-in overlay clone with patch-back | Explicit `yield` tool, schema-validated | Depth 2; concurrency 32; 200 requests soft (1.5x stop); idle 420 s; no wall clock |
| Pi (example) | Spawned `pi` process | Task plus agent file | None: full env, default tools | None | First text of last message, 2,000 lines or 50 KB | 8 tasks, 4 at once; no depth limit |
| DeepSeek harness | In-process activation; or Codex, Claude Code, ACP process | Spawn: none; fork: parent's log up to last `turn/end` | Sandbox override copied; approval pinned `never`; written into the child's log; external children use fixed config independent of the parent | None (experimental worktree tool) | `subagent-settled` notice to the parent (queued or steered); final text or `outputSchema` | Depth 1; 8 active per tree; workflow 1,000 total and `min(16, cores-2)` concurrent |
| goose | Child session record; a pipeline step runs it | Fresh plus task; own system prompt | Inherits mode; child approvals forwarded to the parent's user | None (working dir inside parent's) | `final_output` tool, default `{summary}`; hidden user message | Depth 1; 5 concurrent; no wall clock; 1,000 turns |
| Cline | In-process `SessionRuntime` | `systemPrompt` and `task` only | None: no policies, no approval callback | None | `{text, iterations, finishReason, usage}` | None; declared `timeoutMs` never read |
| OpenHands (client only) | Browser executes `launch_child_conversation`, a new conversation on the Agent Server | Task brief only | Parent's profile; policy from global settings; no narrowing | Worktree by default, silent fallback to shared (agent told) | Hidden user message with child id and status | None in the client |
| Zed | Not in the input | | | | | |
| Warp (client only) | Client opens a hidden pane (Oz, Claude Code, Codex, OpenCode) or cloud child, with a server task row | Not seen; mail via a server bus | Inherits profile; CLI children launched with their permission systems off | None seen | Mailbox messages | No numeric cap; depth is a feature flag |
| OpenCodex | None (gateway) | | | | | Send budget keyed by workflow root |

##### 3.2 Models, fallback, retry, cooldowns

| Project | Roles or slots | Fallback trigger | Retry numbers | `Retry-After` | Cooldowns and pools |
|---|---|---|---|---|---|
| Codex | No slot table; child model: call, config, then parent; role can pin | None (only startup, compaction retry, one WebSocket-to-HTTPS switch) | 200 ms doubling, plus or minus 10%; 5 stream retries (request 4); connection loss retries forever 5 s to 60 s | Sets the wake instant; cannot extend the count | None found |
| OpenCode | Session, agent, default; hidden `title`, `compaction`, (`summary` unused); small model by family list | None | 10 retries, 2 s to 10 s, jittered (about 84 s); no-network 2 s to 30 s up to 1 hour; 3 after timeouts | Raises the delay, capped at 15 minutes; `x-should-retry` overrides | None |
| OpenClaw | Primary plus `fallbacks`; `subagents.model` | Per candidate, with cooldown decision (skip, probe, suspend) | Not read | Rate-limit cooldown is provider timing only | Per reason, persisted: transient 30 s, 1 min, 5 min; billing 10 min to 24 h; auth_permanent 10 min to 1 h; sticky fallback 10 min idle, 30 min cap |
| Hermes Agent | Main plus 19 `auxiliary.<task>` slots, each with `fallback_chain` | Rate limit and billing at once; transport after 2 retries; auth after refresh | 3 attempts, jittered 2 s to 60 s; aux `transient_retries` 2 | Honoured, capped; 429 adaptive | Primary cooldown 60 s to 4 h; pool: 401 5 min, 429 1 hour, sole credential 60 s; per (credential, model) |
| Oh My Pi | 16 roles; `@role` aliases; per-key chains | Retryable errors when `modelFallback`; not stale usage limit, thinking loop, first socket drop | 10 retries, 500 ms doubling, cap 8 s, up to 25% downward jitter | Above 300 s fails fast | Failing selector suppressed for retry-after, reason backoff, or 5 min |
| Pi | Scoped list with thinking level; virtual-model router | None built in; router sees reason `retry` | Agent layer 3, 2 s doubling, cap 60 s, no jitter; provider layer 0 | Dormant by default | None |
| DeepSeek harness | Per-agent route in the log; title route | None | 5, 500 ms, cap 10 s, plus or minus 10% | Longer than max: give up | None |
| goose | One per session; sub-agent model; toolshim; two classifiers | None | 3 (4 attempts), 1 s doubling, cap 30 s, 0.8 to 1.2 jitter; empty response 3 | Used exactly | None |
| Cline | None | None | 3, 1 s, 2 s, 4 s, cap 15 s, no jitter | Not handled in the findings | None |
| OpenHands | Profiles, `title_llm_profile`, classifier router | Not in the client | Snapshot only: `num_retries 5`, multiplier 8, min 8 s, max 64 s | Not in the client | Not in the client |
| Zed | Thread model plus `compaction_model` | None | Rejections: `retry_after` or exponential; read and send errors 3 at a fixed delay | Honoured | None |
| Warp | Server-side | Server-side | 3 per request; delays not in the findings | Not in the client | None |
| OpenCodex | Routing ladder; named combos | `decideComboFailure`: hard stops, target-local hops, statuses | 3 attempts on 5xx per logical request including recovery legs | Cooldown duration: `Retry-After` (cap 1 day) first | Persisted absolute expiry; default 60 s, spent windows 10 min, rate limit 5 s; pool strategies |

### Extensibility and unattended operation

From [round2/synth/10-extensibility-unattended.md](round2/synth/10-extensibility-unattended.md).

"Not stated" means the findings read do not say; it is not a claim that the project lacks it. Trust labels are in section 2.

##### 3.1 MCP transports, authentication and stdio environment

| Project | Transports | OAuth | Environment given to a stdio server | Annotations and approval | Limits |
|---|---|---|---|---|---|
| Codex | stdio, Streamable HTTP (`rmcp`); stdio can run on an executor | loopback callback with `state`; CIMD or DCR registration; keyring with file fallback | not stated | not stated | 30 s start, 300 s call, 8 MiB stdio line |
| OpenCode | stdio (through the location's `Environment`), Streamable HTTP | on by default; code not read | not stated (the agent shell gets full `process.env`) | no annotations used; every call asserts `<server>_<tool>` | 30 s / 30 s / 12 h, 16 MiB frame |
| OpenClaw | stdio, SSE, Streamable HTTP | per operator or per requester identity | not read | not read | not read |
| Hermes | stdio, HTTP (httpx) | not read | **allow-list**: `PATH, HOME, USER, LANG, LC_ALL, TERM, SHELL, TMPDIR`, `XDG_*`, plus the server's own `env:` | cached rows with missing annotations treated as write-capable; description scan warns only | 10 MiB HTTP body cap |
| Oh My Pi | stdio, HTTP, SSE | PKCE `S256`, loopback callback, DCR; keyed by profile and server URL | **full inheritance** (`{...Bun.env, ...config.env}`) | all MCP tools `write` tier regardless of annotations | 30 s call, 250 ms start-up window, 30-day tool cache |
| Pi | stdio, Streamable HTTP (SSE refused) | not stated (a provider-token option refused in project config) | **full inheritance** (`inheritEnv` exists, never set) | not stated; calls pass `tool_call` hooks | 16 MiB message, 60 s, 20 KiB result cut |
| DeepSeek | stdio, Streamable HTTP (static headers) | none (absence confirmed) | **scrubbed**: `scrubbedParentEnv()` plus configured env | none: no MCP code produces `ask` | reconnect 500 ms to 30 s, 10 attempts |
| goose | stdio, builtin, platform, Streamable HTTP (+ Unix socket); no SSE | `client_id`, `client_secret_key`, `scopes` in config | inherited plus configured; configured names filtered through a 31-key disallow list | **trusts server `readOnlyHint`** in SmartApprove | 300 s |
| Cline | stdio (own parsers), SSE (default when `type` omitted), Streamable HTTP | tokens refreshed on connect, no interactive flow; stored in a settings file | **full inheritance** | no per-tool `autoApprove`; all MCP tools approved by default in the IDE | not stated |
| OpenHands | config only (stdio, SSE, HTTP); client in SDK | OAuth2 probe in UI | not in this repository | not in this repository | not in this repository |
| Zed | not read | not read | ACP agents: full inheritance | none for external agents | not read |
| Warp | stdio, Streamable HTTP, SSE (`rmcp`) | OAuth, secure storage | **full inheritance** (no `env_clear`) | allow-list and deny-list by server | not stated |
| OpenCodex | no MCP client | n/a | n/a | n/a | n/a |

##### 3.2 Tool deferral, and how the change is recorded

| Project | Mechanism | Default | How a change reaches the model without breaking the prompt cache |
|---|---|---|---|
| Codex | BM25 `tool_search`; per-server `omit_tools_from` | on for models whose catalog row has `supports_search_tool` | `list_changed` revision in transport order; typed world-state sections diffed per step and persisted |
| OpenCode | Code Mode catalog (2,000-token budget, round-robin per namespace) plus an in-code `search()` | MCP and plugin tools are Code Mode unless `codemode:false` | keyed instruction sources hashed; durable `session.instructions.updated` event with frozen text; later changes appended as `system` messages |
| OpenClaw | tool search (`tools` or `directory` mode) | on (`enabled: setting ?? true`) | byte-stable cached prefix; change recording not read |
| Hermes | three bridge tools (`tool_search`, `tool_describe`, `tool_call`) | on whenever a deferrable tool exists | stateless catalog rebuilt at every assembly; skills index re-rendered only at compaction |
| Oh My Pi | `xd://` devices driven through `read` and `write`; no search tool | `tools.xdev` true, docs mode `catalog` | tool arrays sorted by name; tool cache keyed by config hash |
| Pi | per-server and per-tool exposure: `direct`, `model-only`, `codemode`, `deferred`, `hidden`; BM25 `tool_search` | `codemode` | system-message delta in the transcript with `toolsAdded` and `toolsRemoved`; restored on resume, `/tree` and fork |
| DeepSeek | none: every schema every step | n/a | whole-generation swap, all of a server's tools or none |
| goose | `enable_tools` lookup, names only | off (`GOOSE_TOOL_DEFERRAL=lookup` to enable) | enabled set recomputed by scanning the conversation; deferred names sorted; per-turn lease freezes the catalog |
| Cline | none | n/a | rules re-read per request (cache effect not examined) |
| Warp | none in the client | n/a | n/a |
| OpenHands, Zed, OpenCodex | not visible, not read, n/a | | |

##### 3.3 Code mode and the approval path

| Project | Runtime and isolation | Default | Do inner calls go through approval? | Limits |
|---|---|---|---|---|
| OpenCode | own interpreter over `acorn`, in the server process; language-level only; host `fetch` | on for MCP and plugin tools | **yes**: each nested call runs through the same `run` and its own `assert`; `fetch` asserts `webfetch` and re-checks redirect hops | caps on strings, arrays, promises, depth; timeout between steps |
| Pi | QuickJS-wasm in a worker thread, frozen built-ins; language-level only | tool inactive; MCP auto-activates it | **yes**: the tool pipeline is re-entrant (`runToolCall`); `tool_call` hooks see nested calls with the parent id | 256 MiB, two workers; **deadline `Infinity` unless the script sets one** |
| goose | Boa interpreter, embedded; no fs, network, shell | off (Cargo feature and runtime) | **no**: `lease.call` directly; inspectors and `PreToolUse` hooks skipped; one approval for `execute_javascript` | 2 M instructions, 4 workers, 300 s |
| Hermes | `execute_code`: per-session Python kernel on the host (local backend) | tool present | whole-script approval only on gateway and ask surfaces; script's `terminal()` calls are guarded per call, direct `subprocess` is not | env allow-list is the only containment |
| DeepSeek | `workflow`: JavaScript in a `vm` context inside the sandboxed PTC process under the caller's file policy; refuses to run if policy cannot be enforced | tool exists; shipped default not stated | not stated | 1,000 children, 5 s synchronous, no overall deadline |
| Codex | Code Mode runtime; `code_mode_only` set for 11 of 12 bundled models though the feature flag is off | by catalog row | not traced | not traced |
| OpenClaw, Oh My Pi, Cline, Warp, Zed, OpenHands | none found or not read | | | |

##### 3.4 Skill loading

| Project | Sources | What is in the prompt | Budget | Trust gate | Executed at load? |
|---|---|---|---|---|---|
| Codex | config layers, plugins, `.agents/skills` from cwd to project root | listing format not in findings | not stated | **none** (absence confirmed), repo skills load though project layer is untrusted | nothing found; scripts go through sandbox and approval |
| OpenCode | `.opencode`, `.claude`, `.agents`, remote `index.json` (same origin only) | id, name, description, filtered by permission | **none** | none found | skills: no; custom commands: `` !`cmd` `` runs unchecked |
| OpenClaw | six tiers; no ancestor walk | catalog, bodies on demand | **150 skills, 18,000 characters**, degrades to compact form | gating by OS, binaries, env; no trust prompt found | no; Skill Workshop writes skills by default |
| Hermes | skills directory, hub | 60-character descriptions | 60 characters each | install-time regex scan; agent writes: opt-in scan and opt-in approval | no; skill-declared env names cannot register managed credentials |
| Pi | `.pi/skills`, `.agents/skills` up to git root, user dirs | name, description, absolute path | **none** (500 skills means 500 entries) | project skills gated by path-keyed trust | no |
| Cline | `.clinerules`, `.cline`, `.claude`, `.agents`, user dirs | names inside the `skills` tool description | not stated | none | not stated |
| OpenHands | build-time catalog plus user and project skills from the server | decided server-side | n/a | allow-list for catalog, deny-list for user skills | n/a |
| Oh My Pi | capability registry reads skills from many harnesses' directories | not detailed | not stated | none | not stated |
| goose, DeepSeek, Zed | not read | | | | |

##### 3.5 Plugin and hook trust gate

| Project | What executable code a repository can bring | Gate | What a hook's trust is bound to |
|---|---|---|---|
| Codex | hook commands, plugins | project trust level disables the project layer; per-hook trust state in the user layer only | hash of `{event, matcher, handler}`; plugin tree hash re-verified at execution; **not the bytes of a script a repo hook calls** |
| OpenCode | in-process plugins from `.opencode`, `.claude`, `.agents`; command templates | none; admin `plugin.use` policy only | nothing |
| OpenClaw | `before_tool_call` plugin hooks; install trust not read | not read | not read |
| Hermes | in-process plugins; shell hooks | install-time regex scanner (blocks `dangerous`); shell hooks need first-use consent per (event, command) | (event, command) string |
| Oh My Pi | TS and JS hooks and tools imported from `.claude/`, `.codex`, plugins; project MCP | **none** (`isProjectTrusted()` is always true); `--trusted-extension` opt-in narrowing | nothing |
| Pi | `.pi` extensions, project `mcp.json`, skills, packages | path-keyed trust (`trust.json`), decided by user-level code first | nothing: a path boolean |
| DeepSeek | installed plugins in the host process | `plugin_manager` escalation per call | not read |
| goose | `.agents/plugins` hooks and MCP servers | **none**; auto-enabled on discovery | nothing |
| Cline | `.cline/hooks`, `.cline/plugins` | **none** (home-only lane for Agent Plugins) | nothing |
| OpenHands | `.openhands/hooks.json` | none; plugin-launch checkbox is a one-shot UI gate | nothing |
| Warp, Zed, OpenCodex | not applicable; Warp's cloud driver passes `--dangerously-bypass-hook-trust` to Codex | | |

##### 3.6 Hook failure mode

| Project | Hook crash, timeout or bad output | Who can loosen a decision |
|---|---|---|
| Codex | **fail open** for `PreToolUse` and `PermissionRequest` (run marked `Failed`); deny wins over allow | a `PermissionRequest` hook allow approves and pre-approves the sandbox retry |
| OpenCode | `permission.evaluate` has no failure channel (throw becomes a defect; inferred fail of the call); only `tool.execute.before` may fail | hooks can change ask to allow |
| OpenClaw | **fail closed**: "Tool call blocked because before_tool_call hook failed" | hooks can rewrite parameters |
| Hermes | plugin hook exception **fail open**; `approve` escalation fail closed; shell hooks fail open unless `fail_closed` | a hook may only escalate to the human gate (`approve`), or `block`; `modify` happens before the tool's own approval check |
| Oh My Pi | **fail closed**: timeout or throw blocks, 30 s | none: approval is re-resolved on the rewritten input |
| Pi | `tool_call` **fail closed**; all other events open; arguments mutable with no re-validation | none in the seam itself |
| goose | **fail open** unless `PreToolUse` with `on_failure: block`; invalid matcher silently drops the rule | policy deny wins |
| Cline | file hooks **fail open** on every failure; `cancel` stops the whole run; plugin hooks ignored after errors | `review` flag parsed and unused |
| OpenHands | load failure starts the conversation with **no hooks** | not in this repository |
| DeepSeek | `beforeTool` in pi-durable not applicable; its own hook bridges not read (unsettled) | n/a |

##### 3.7 Unattended authorisation, default mode, catch-up, overlap

| Project | How an unattended run is authorised | Mode when no human is there | Catch-up after downtime | Overlap | Dispatch journal / idempotency |
|---|---|---|---|---|---|
| Codex | `exec`: same engine, approval policy `Never`, approval and user-input requests rejected; no engine scheduler | reject | n/a | n/a | per-thread durable queue, dispatched when idle |
| OpenCode | `Unattended` ruleset: an ask is allowed only if the ruleset allows every resource, else denied; `run` rejects asks, `--auto` answers "once" | deny unless explicitly allowed | **skip** missed "every" slots, no burst | not stated | not stated |
| OpenClaw | cron standing grant bound to job, config revision and operation, minted by an earlier `allow-always`, consumed at spawn; falls back to a prompt | prompt or deny; risky shapes always prompt | restart: conditional retry, **no counter** | not stated | delivery queue records `platform_send_started_at`; webhook idempotency cache in memory only |
| Hermes | `cron_mode: deny`; toolset denylist; agent cannot set model or provider; prompt tripwire; scripts only under one directory | **deny** | **at-most-once**: advance `next_run_at` before dispatch | admission through the per-session queue; parallel pool | per-fire journal keyed by hash of job and request, fsynced; pause on unknown outcome |
| Oh My Pi | none (no scheduler); headless throws if a prompt is needed but default mode is `yolo` | yolo | n/a | n/a | n/a |
| Pi | none (no scheduler, no permission system) | allow | n/a | n/a | durable tasks: restart limit 5 |
| DeepSeek | schedule delivers a user message; webhook session gets a `read-only` preset as a log event; headless `ask` with no answerer fails closed | preset-dependent | **latest-only**, once | slot held until the agent is idle | receipt committed after durable flush; messageId matched in inbox; webhook `deliveryId` stored, never checked |
| goose | recipe schedule | **`Auto`** regardless of user mode | none | **allowed** (only `paused` checked) | none |
| Cline | cron spec; unknown mode becomes `yolo`; `autoApproveTools` default true; agent-created tasks start `pending_approval` | **yolo** | not settled | not stated | not stated |
| OpenHands | automation server (not in repo); runs as creator or a service account; default timeout 600 s | not in this repository | not in this repository | not in this repository | not in this repository |
| Warp | autonomous run: anything that would ask returns `Denylisted`; ACP route allows all but denylist and protected paths; foreign CLI children launched with their own permissions off | mixed | not in this repository | not in this repository | not in this repository |
| OpenCodex | Remote Workspace hub agent: `approvalPolicy: "never"` with a fixed capability set | fixed set | n/a | n/a | n/a |

##### 3.8 Importing other harnesses' configuration and transcripts

| Project | Configuration read from other harnesses | Transcripts imported |
|---|---|---|
| Oh My Pi | capability registry reads skills, rules, commands, **hooks**, custom tools, MCP servers, context files, settings from Claude, Codex, Gemini, OpenCode, Cline, Cursor, Windsurf, VS Code, GitHub, `.agents`, plugins; TS and JS hooks and tools become executable | Claude Code and Codex JSONL |
| goose | agents from `.claude/agents`, `~/.claude/agents`, `.agents/agents` | Claude Code, Codex, Pi JSONL (CLI and desktop) |
| Cline | skills from `.claude` and `.agents` | Claude Code, Codex, OpenCode |
| OpenCode | skills, agents, plugins from `.claude` and `.agents` roots | none found |
| Warp | global third-party MCP configs (`~/.claude.json`, Codex, Agents) auto-spawn if a toggle is on; project-scoped servers never auto-spawn | none |
| Pi, Codex, OpenClaw | `.agents/skills` | none found |
| Hermes, DeepSeek, OpenHands, Zed, OpenCodex | none found | none found |

## 5. Detailed source-code investigations

The investigations themselves are the files below. A finding in a per-repository file has: what the code does, a trace from entry point to decision with a permalink per step, edge cases and failure handling, the Round 1 claim it confirms or corrects, what it means for us, and a confidence line. The synthesis files regroup those findings by feature without merging projects.

### By feature group

| Feature group | Synthesis file | Size | Permalinks |
|---|---|---|---|
| The agent loop, tool dispatch and durability | [`round2/synth/01-loop-dispatch-durability.md`](round2/synth/01-loop-dispatch-durability.md) | 202 KB | 531 |
| Approvals and permission rules | [`round2/synth/02-approvals-permission-rules.md`](round2/synth/02-approvals-permission-rules.md) | 400 KB | 1108 |
| Sandboxing and where commands run | [`round2/synth/03-sandbox-containers-execution.md`](round2/synth/03-sandbox-containers-execution.md) | 126 KB | 303 |
| Credentials, secrets and authentication | [`round2/synth/04-credentials-secrets-auth.md`](round2/synth/04-credentials-secrets-auth.md) | 150 KB | 380 |
| Links, workers and client protocols | [`round2/synth/05-links-workers-protocols.md`](round2/synth/05-links-workers-protocols.md) | 186 KB | 402 |
| Session storage, resume, fork and checkpoints | [`round2/synth/06-sessions-fork-checkpoints.md`](round2/synth/06-sessions-fork-checkpoints.md) | 167 KB | 384 |
| Context: prompts, compaction, instruction files, memory | [`round2/synth/07-context-compaction-memory.md`](round2/synth/07-context-compaction-memory.md) | 313 KB | 736 |
| Edit tools and the mechanics behind the scenes | [`round2/synth/08-edit-tools-mechanics.md`](round2/synth/08-edit-tools-mechanics.md) | 165 KB | 347 |
| Sub-agents, orchestration, model roles, fallback and retry | [`round2/synth/09-subagents-models-retry.md`](round2/synth/09-subagents-models-retry.md) | 204 KB | 395 |
| Extensibility and unattended operation | [`round2/synth/10-extensibility-unattended.md`](round2/synth/10-extensibility-unattended.md) | 206 KB | 566 |

### By repository

| Per-repository file | Findings | Permalinks | Marked checked or corrected in review |
|---|---|---|---|
| [`round2/cline.agents-sessions.md`](round2/cline.agents-sessions.md) | 37 | 260 | 24 |
| [`round2/cline.safety.md`](round2/cline.safety.md) | 37 | 218 | 32 |
| [`round2/codex.agents.md`](round2/codex.agents.md) | 29 | 308 | 19 |
| [`round2/codex.safety.md`](round2/codex.safety.md) | 28 | 149 | 27 |
| [`round2/codex.sessions.md`](round2/codex.sessions.md) | 37 | 181 | 16 |
| [`round2/deepseek-harness.agents.md`](round2/deepseek-harness.agents.md) | 27 | 158 | 22 |
| [`round2/deepseek-harness.safety.md`](round2/deepseek-harness.safety.md) | 25 | 163 | 23 |
| [`round2/deepseek-harness.sessions.md`](round2/deepseek-harness.sessions.md) | 28 | 228 | 16 |
| [`round2/goose.agents.md`](round2/goose.agents.md) | 39 | 222 | 19 |
| [`round2/goose.safety.md`](round2/goose.safety.md) | 35 | 154 | 23 |
| [`round2/goose.sessions.md`](round2/goose.sessions.md) | 31 | 259 | 21 |
| [`round2/hermes-agent.agents.md`](round2/hermes-agent.agents.md) | 45 | 294 | 21 |
| [`round2/hermes-agent.safety.md`](round2/hermes-agent.safety.md) | 23 | 189 | 17 |
| [`round2/hermes-agent.sessions.md`](round2/hermes-agent.sessions.md) | 32 | 182 | 16 |
| [`round2/oh-my-pi.agents.md`](round2/oh-my-pi.agents.md) | 35 | 175 | 18 |
| [`round2/oh-my-pi.safety.md`](round2/oh-my-pi.safety.md) | 32 | 143 | 32 |
| [`round2/oh-my-pi.sessions.md`](round2/oh-my-pi.sessions.md) | 34 | 272 | 18 |
| [`round2/openclaw.agents.md`](round2/openclaw.agents.md) | 38 | 208 | 19 |
| [`round2/openclaw.safety.md`](round2/openclaw.safety.md) | 34 | 166 | 29 |
| [`round2/openclaw.sessions.md`](round2/openclaw.sessions.md) | 31 | 137 | 18 |
| [`round2/opencode.agents.md`](round2/opencode.agents.md) | 38 | 170 | 19 |
| [`round2/opencode.safety.md`](round2/opencode.safety.md) | 30 | 134 | 29 |
| [`round2/opencode.sessions.md`](round2/opencode.sessions.md) | 28 | 194 | 19 |
| [`round2/opencodex.md`](round2/opencodex.md) | 38 | 226 | 18 |
| [`round2/openhands.agents-sessions.md`](round2/openhands.agents-sessions.md) | 35 | 237 | 20 |
| [`round2/openhands.safety.md`](round2/openhands.safety.md) | 32 | 155 | 25 |
| [`round2/pi.agents.md`](round2/pi.agents.md) | 30 | 209 | 17 |
| [`round2/pi.safety.md`](round2/pi.safety.md) | 36 | 212 | 31 |
| [`round2/pi.sessions.md`](round2/pi.sessions.md) | 29 | 177 | 15 |
| [`round2/warp.md`](round2/warp.md) | 29 | 214 | 15 |
| [`round2/zed.md`](round2/zed.md) | 26 | 185 | 15 |

## 6. Execution flows and architectural patterns

This section describes the execution patterns that recur across the thirteen codebases. Each is written as a flow, followed by the projects that implement it and one permalink per project. Trust labels follow the synthesis files: `checked in review`, `corrected in review` (the corrected text is used here), `unchecked` (the author's reading only, no reviewer opened it) and `inferred` (the author's inference). `absence confirmed` means a reviewer searched the clone for the thing said to be missing. A project that does not appear in a flow has no finding for it in the files read.

### 6.1 The loop: ordered stateless steps over a persisted log, or an in-memory loop that writes a transcript

**Flow A, ordered steps over a persisted log.**
1. Load the session from storage.
2. Walk an ordered list of steps. The first step that applies runs, and its effect is written back as a message or an event.
3. Stop when a step yields. The model call is the last step in the list, so every earlier step that can act does so first.
4. After a restart, run the same loop again. The state it needs is already in the log.

- **goose.** `StateMachine::step` walks the step list and applies the first step that returns `Applied`; `run` reloads the session and repeats while the state is `Continuing` ([machine.rs L154-L212](https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-agent/src/machine.rs#L154-L212), [machine.rs L233-L248](https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-agent/src/machine.rs#L233-L248)). The step order is the security pipeline: slash commands, approval, the pre-tool hook, skills, recipes, execution, then compaction, retry and the stop hook, with inference last ([state_machine_builder.rs L127-L166](https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/state_machine_builder.rs#L127-L166), [state_machine_builder.rs L182-L225](https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/state_machine_builder.rs#L182-L225)). The crate README states the design: behaviour is a function of the persisted conversation ([README.md L1-L8](https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-agent/README.md#L1-L8)). `unchecked`. An approval decision is persisted as a message before it takes effect, and a restart continues an unanswered confirmation ([tool_confirmation.rs L87-L141](https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/state_machine/tool_confirmation.rs#L87-L141), [load_session.rs L367-L455](https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/acp/server/load_session.rs#L367-L455)). `checked in review`.
- **DeepSeek harness.** `Agent.turn()` appends `turn/start`, then repeats `preStep` (claim inbox messages, resolve the tool set, run the `agent/pre-step` waterfall) and `step` (stream one response, then run its tool calls), appending `step/start` and `step/end` ([agent.ts L327-L433](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/agent-loop/src/agent.ts#L327-L433)). Every model-visible item is a session event, and compaction rewrites the replayable message list without deleting log events; see "DeepSeek harness" in `round2/synth/06-sessions-fork-checkpoints.md`. `unchecked` for the loop description.
- **OpenCode.** `drain` promotes queued input and loops `advanceToStep` and `runStep` ([llm.ts L214-L222](https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/llm.ts#L214-L222)). Each durable event is committed under a per-aggregate mutex in one SQLite transaction that also runs the projectors ([bus.ts L372-L412](https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/bus.ts#L372-L412)). The `event` table is written only when persistence is on, and the CLI server never turns it on ([bus.ts L186-L192](https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/bus.ts#L186-L192), [bus.ts L214-L214](https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/bus.ts#L214-L214)), so the stored truth is the projection tables. `checked in review`.

**Flow B, in-memory loop that writes a transcript.**
1. Hold the history in memory.
2. Stream a model reply and run the tool calls it contains.
3. Append the finished messages to a transcript (a file or database rows).
4. After a restart, rebuild memory from the transcript and start a new loop.

| Project | Loop | What is written, and when |
|---|---|---|
| Codex | `run_turn` ([turn.rs L543-L554](https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/session/turn.rs#L543-L554)) | a JSONL line per item, flushed after every line ([recorder.rs L2034-L2074](https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rollout/src/recorder.rs#L2034-L2074)); `unchecked` |
| Pi | `runLoop`, two nested loops ([agent-loop.ts L172-L330](https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/agent/src/agent-loop.ts#L172-L330)) | each message on `message_end` ([agent-session.ts L1212-L1233](https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/agent-session.ts#L1212-L1233)); `checked in review` |
| OpenClaw | `runLoop`, unbounded ([agent-loop.ts L250-L253](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/agent-core/src/agent-loop.ts#L250-L253)) | SQLite transcript rows under a writer claim ([session-bootstrap.ts L543-L590](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-runner/run/session-bootstrap.ts#L543-L590)); `corrected in review` |
| Oh My Pi | `runLoopBody` ([agent-loop.ts L1679-L1685](https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/agent/src/agent-loop.ts#L1679-L1685)) | one synchronous line per entry ([session-manager.ts L1946-L1989](https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-manager.ts#L1946-L1989)); `unchecked` |
| Hermes Agent | a `while` over the iteration count ([conversation_loop.py L1748-L1753](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/conversation_loop.py#L1748-L1753)) | SQLite rows plus a durable input queue ([hermes_state_common.py L467-L568](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_common.py#L467-L568)) |
| Cline | `AgentRuntime.execute` ([agent-runtime.ts L849-L1040](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/agents/src/agent-runtime.ts#L849-L1040)) | the whole messages file rewritten with a plain `writeFileSync` ([session-manifest-store.ts L220-L240](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/session/stores/session-manifest-store.ts#L220-L240)); `checked in review` |

**What the choice costs.** In Flow A the restart path is the normal path, so there is no separate recovery code to keep correct; goose and DeepSeek both build recovery on re-reading the log. The cost is that every decision must be written before it is used, which is the rule in section 6.2. In Flow B a crash loses whatever the loop held only in memory. Pi loses a partial assistant message and does not repair a tool call that has no result (the second part is `inferred`). Cline can truncate its messages file on a crash because the write is not atomic.

### 6.2 Append, flush, then act, and what a crashed turn becomes

**Flow.**
1. Append the intent or the decision to the log.
2. Flush it (commit, `fsync`). If the flush fails, do not proceed.
3. Perform the effect: send the model request, or run the tool body.
4. Append the result.
5. After a crash, the log may show an effect that began and has no result. The design must decide what that turn becomes.

**Projects that write before acting.**
- **DeepSeek harness.** A checkpoint policy awaits `sessions.flush` before the model request, before every top-level tool body and at each pre-step; a failed flush rejects, so the adapter or tool body is never invoked ([index.ts L29-L38](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/session/session-checkpoint-policy/src/index.ts#L29-L38), [index.ts L63-L83](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/session/session-checkpoint-policy/src/index.ts#L63-L83)). The first batch is written through a temporary file, `fsync`, `link()` and a directory `fsync`; later batches append and `fsync` ([index.ts L1284-L1324](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/session/session-persistence-jsonl/src/index.ts#L1284-L1324)). Between barriers events are write-behind, up to 200 ms, so a hard crash can lose that window; nested tool dispatches skip the barrier. `checked in review`.
- **OpenCode.** `Execution.Started` and a claim row commit in one transaction with the event ([execution.ts L73-L82](https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/execution.ts#L73-L82), [execution.ts L130-L151](https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/execution.ts#L130-L151)). There is no per-tool intent. `checked in review`.
- **Hermes Agent.** An admission row moves from `queued` to `started` with the runtime epoch and generation, in one write transaction ([hermes_state_runtime.py L206-L242](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_runtime.py#L206-L242), [hermes_state_runtime.py L245-L274](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_runtime.py#L245-L274)). `corrected in review`.
- **Pi (the experimental `pi-durable` package).** A tool call commits `{phase: "execute", arguments, replay}` before it runs, with `replay` defaulting to `"unsafe"` ([tool.ts L195-L198](https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/durable/src/harness/tool.ts#L195-L198), [tool.ts L201-L227](https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/durable/src/harness/tool.ts#L201-L227)). It is not the shipped CLI path. `checked in review`.
- **goose.** The approval decision is stored as a message first, and a conflicting second decision is rejected ([tool_confirmation.rs L87-L141](https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/state_machine/tool_confirmation.rs#L87-L141)). `checked in review`.

**What each design does with the crashed turn.**

| Family | Projects | What the turn becomes |
|---|---|---|
| Close the turn as "outcome unknown", re-run nothing | DeepSeek harness | On resume the log is read under the lock; every unanswered tool call gets a synthetic error result and `turn/end` gets the reason `interrupted` ([index.ts L783-L816](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/agent-loop/src/index.ts#L783-L816), [repair.ts L53-L98](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/session/src/repair.ts#L53-L98)). `checked in review` |
| Re-run only what the tool declares safe | `pi-durable` | A running task returns to `pending` with a restart counter, and a tool in phase `execute` re-runs only if both the stored and the current policy say `safe` ([scheduler.ts L450-L492](https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/durable/src/harness/scheduler.ts#L450-L492)). `checked in review` |
| Re-drive the turn with a prompt | OpenCode, OpenClaw | OpenCode publishes a synthetic "continue without repeating completed work" message, and gives up after more than 10 attempts ([restart.ts L16-L34](https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/execution/restart.ts#L16-L34), [restart.ts L76-L99](https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/execution/restart.ts#L76-L99)); `checked in review`. OpenClaw injects a fixed text telling the agent to check state before repeating ([session-restart-resume.ts L78-L85](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/session-restart-resume.ts#L78-L85)); `unchecked` |
| Mark `unknown`, never requeue, a person decides | Hermes Agent | At start, every `started` admission from an older epoch becomes `unknown` and is not requeued ([hermes_state_runtime.py L453-L464](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_runtime.py#L453-L464), [hermes_state_runtime.py L663-L694](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_runtime.py#L663-L694)); unattended turns settle themselves as interrupted ([session_unattended_unknown.py L41-L66](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/session_unattended_unknown.py#L41-L66)). `corrected in review` for the first, `checked in review` for the second |
| Re-derive from the log and continue | goose | An unanswered confirmation continues; if the extension lease is gone the call gets a synthetic "approval expired" error instead of running ([tool_confirmation.rs L148-L186](https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/state_machine/tool_confirmation.rs#L148-L186)). `checked in review` |
| Mark the worker lost | OpenClaw cloud worker | `WorkerNodeLostError` says tool effects may be unknown and `canContinue` is false ([worker-node-loss.ts L3-L15](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/worker/worker-node-loss.ts#L3-L15)). `checked in review` |
| Nothing described, or the partial work is lost | Codex, Oh My Pi, Pi (shipped path), Cline | Codex drops approval requests and begin events from the log ([policy.rs L16-L52](https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rollout/src/policy.rs#L16-L52)) and repairs a torn final line ([recorder.rs L2019-L2032](https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rollout/src/recorder.rs#L2019-L2032)), `unchecked`; Pi loses a partial assistant message ([agent-session.ts L1212-L1233](https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/agent-session.ts#L1212-L1233)), `checked in review`; Cline's whole-file rewrite can truncate the transcript ([session-manifest-store.ts L220-L240](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/session/stores/session-manifest-store.ts#L220-L240)), `checked in review` |

A reviewer noted a limit on the DeepSeek design. The flush guards the start of the model request and of the tool body, not what the tool does after its body starts, so a crash during a tool still leaves "outcome unknown" on resume. Hermes Agent is the only design in the files that separates an `unknown` execution from an `interrupted` one.

### 6.3 Input that arrives during a turn: steer, queue or reject

**Flow.**
1. The user or another system sends input while a turn is running.
2. The harness chooses one of three policies: reject it, queue it and deliver it at a step boundary, or interrupt the turn.
3. Where the accepted input is stored decides what a crash does to it: a durable row survives, an in-memory queue does not.

**Reject.**
- **goose.** A second run for the same session fails with `AgentRunExists`; the one-run guard is in memory in one process ([active_run.rs L38-L66](https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/active_run.rs#L38-L66), [session_runtime.rs L98-L145](https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/session_runtime.rs#L98-L145)). Whether the user message is stored before the rejection is not described. `unchecked`.
- **Cline.** A second `run()` while a run is active throws; no steering or queue was found. See "Cline" in `round2/synth/01-loop-dispatch-durability.md`.
- **Pi (core loop).** `prompt()` while a run is active throws, and the session layer turns a mid-run message into a steer or a follow-up chosen by the caller (see "Pi" in `round2/synth/01-loop-dispatch-durability.md`). `checked in review`.

**Queue and deliver at a step boundary, never kill a tool.**
- **Codex.** `turn/steer` queues a user message that is drained at the top of the next loop iteration, after the in-flight tool calls finish ([turn.rs L774-L785](https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/session/turn.rs#L774-L785)). Whether the steered text reaches the rollout before the acknowledgement is not established. `unchecked`. A separate SQLite queue (at most 100 per thread) lets another process enqueue work for idle threads ([service.rs L90-L105](https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/ext/queue/src/service.rs#L90-L105)). `checked in review`, with the correction that the engine has no scheduler.
- **Pi.** Steering and follow-up queues, one message at a time by default, polled after each completed turn ([agent-loop.ts L184-L226](https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/agent/src/agent-loop.ts#L184-L226), [agent.ts L149-L180](https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/agent/src/agent.ts#L149-L180)). They are in-memory. Steering does not interrupt tools. `checked in review`.
- **OpenCode.** `drain` and `advanceToStep` promote queued inbox input at step boundaries ([llm.ts L61-L75](https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/llm.ts#L61-L75)). Where the inbox is stored is not described in the files read.
- **DeepSeek harness.** The inbox is claimed at each pre-step. A prompt carries a client `requestId`, and accepted ids are recovered by scanning the log for the spliced message, so a repeated prompt returns `accepted: true` ([commands.ts L321-L324](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/api/session-controller/src/commands.ts#L321-L324), [commands.ts L389-L413](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/api/session-controller/src/commands.ts#L389-L413)). `unchecked`.
- **Oh My Pi.** The RPC set includes `steer` and `follow_up`. Steering is cooperative: only tools flagged `interruptible` are aborted ([agent-loop.ts L113-L129](https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/agent/src/agent-loop.ts#L113-L129), [agent-loop.ts L3360-L3374](https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/agent/src/agent-loop.ts#L3360-L3374)). Storage of the steered text is not described.

**Choose among steer, follow-up, collect and interrupt.**
- **OpenClaw.** A policy is resolved per active run (inline command, session mode, channel, global, then `steer`), with a 500 ms debounce, a cap of 20 and an overflow policy of `summarize` that writes a durable "dropped" receipt per dropped input ([settings.ts L10-L41](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/auto-reply/reply/queue/settings.ts#L10-L41)). The core loop drains an in-memory queue at fixed checkpoints ([agent-loop.ts L250-L272](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/agent-core/src/agent-loop.ts#L250-L272), [pending-message-queue.ts L53-L131](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/agent-core/src/pending-message-queue.ts#L53-L131)). Accepted input is stored before the acknowledgement in the channel ingress table and the staged pending-input row ([ingress-queue.kernel.ts L145-L258](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/channels/message/ingress-queue.kernel.ts#L145-L258), [agent-run-user-turn.ts L402-L426](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/agent-turn/agent-run-user-turn.ts#L402-L426)). `checked in review` for the stored-before-acknowledgement part; the steering queue itself is in memory and was `corrected in review` from an earlier "safe under crashes".

**Durable queue with idempotent admission.**
- **Hermes Agent.** Each inbound message is first committed as a `session_admissions` row, unique on principal, target session and request id, with an intent of `queue`, `steer` or `redirect`; at most 32 per session ([hermes_state_common.py L597-L615](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_common.py#L597-L615), [hermes_state_runtime.py L126-L177](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_runtime.py#L126-L177)). A retried delivery is an exact replay. `corrected in review`.

**Input held on the client.**
- **OpenHands (client).** A message goes over the open socket, falls back to HTTP if the socket has delivered no frame yet, and is held in browser memory while a Cloud sandbox is paused ([conversation-websocket-context.tsx L1550-L1566](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/contexts/conversation-websocket-context.tsx#L1550-L1566)). A held message is lost if the tab closes. `checked in review`.

### 6.4 The control side and the execution side

**Flow.**
1. One side dials the other. The executing side dials out, or the controlling side dials in over SSH.
2. The executing side proves who it is. The controlling side admits it.
3. The controlling side sends work. What it sends differs: a sandbox intent, a model reference, a credential, a tool policy.
4. The executing side applies its own rules to each request and refuses what they forbid.
5. Both sides must settle what happens when a worker is replaced, reconnects, or its connection drops mid-request.

**Executing side dials out, and the control side fences stale workers.**
- **OpenClaw cloud worker.** The worker dials the Gateway on `/__openclaw__/worker`; a setup code binds the box to an environment at enrolment ([device-pairing-cloud-worker.ts L29-L89](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/device-pairing-cloud-worker.ts#L29-L89), [worker-connection-endpoint.ts L45-L86](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/worker/worker-connection-endpoint.ts#L45-L86)). The Gateway also dials the box over SSH for bootstrap, tunnel control and workspace transfer ([ssh.ts L259-L291](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/ssh.ts#L259-L291)). The first frame carries an admission block checked in a fixed order, with a constant-time credential compare and opaque rejections ([admission.ts L79-L186](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/admission.ts#L79-L186)). Each dispatch has a credential of 32 random bytes, stored as a domain-tagged hash bound to the turn claim, with a 10 minute TTL ([credential.ts L9-L60](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/credential.ts#L9-L60)). The fence is a global owner epoch, computed as the maximum of all environment and commit epochs plus one ([store-mutations.ts L30-L50](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/store-mutations.ts#L30-L50)) and re-read on every worker request ([admission.ts L188-L218](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/admission.ts#L188-L218), [worker-turn-rpc.ts L691-L716](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/worker-turn-rpc.ts#L691-L716)). The worker treats `credential-replaced` and `owner-epoch-mismatch` as terminal. Transcript writes go through `worker.transcript.commit`, ledgered by session, epoch and sequence ([transcript-commit.runtime.ts L324-L366](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/transcript-commit.runtime.ts#L324-L366), `unchecked`), and the writer claim is re-read inside the write transaction ([session-accessor.sqlite-transcript-write-snapshot.ts L104-L130](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/config/sessions/session-accessor.sqlite-transcript-write-snapshot.ts#L104-L130)). The worker socket accepts only a closed list of methods ([worker-admission.ts L54-L66](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/gateway-protocol/src/schema/worker-admission.ts#L54-L66)). Model calls are by reference: the Gateway approves the `modelRef` and keeps the key ([inference-runtime.ts L122-L153](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/inference-runtime.ts#L122-L153)). `corrected in review` for the epoch and the writer claim; `checked in review` for the rest. A GitHub token does reach the worker ([launch-descriptor.ts L183-L211](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/worker/launch-descriptor.ts#L183-L211)).
- **OpenClaw node.** The node connects with `role: node` and re-decides `system.run` with its own policy, but an approval forwarded by the Gateway satisfies the node's allow-list; only a local `security=deny` holds ([invoke-system-run.ts L121-L141](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/invoke-system-run.ts#L121-L141), [exec-policy.ts L60-L117](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/exec-policy.ts#L60-L117)). `checked in review`.
- **Codex exec-server.** The executor dials a registry and relay (`--remote`), with a Noise handshake to a key pinned from the registry ([noise_channel.rs L1-L37](https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/noise_channel.rs#L1-L37), `unchecked`). The control side sends the policy: `process/start` carries a portable sandbox intent, and a request with no `sandbox` runs unsandboxed ([protocol.rs L374-L414](https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server-protocol/src/protocol.rs#L374-L414), [process_sandbox.rs L142-L172](https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/process_sandbox.rs#L142-L172)). No executor-side ceiling exists (`absence confirmed`). Fencing is one attachment per session id with an 80 s retention, so it stops a second client but not a stale one ([session_registry.rs L15-L130](https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/server/session_registry.rs#L15-L130)); there is no epoch (`absence confirmed`). The direct `--listen` mode accepts an unauthenticated non-loopback bind ([command.rs L56-L160](https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server-cli/src/command.rs#L56-L160)). `checked in review`, `corrected in review` for the listener.
- **OpenCodex Remote Workspace.** The executor dials the hub with a bearer from a single-use pairing code ([workspace-hub.ts L377-L425](https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-hub.ts#L377-L425)); the hub closes any earlier connection for the same device, so the fence is "latest connection wins", with no generation in each message and no replay across reconnects ([workspace-hub.ts L427-L463](https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-hub.ts#L427-L463), [workspace-device.ts L386-L450](https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-device.ts#L386-L450)). `corrected in review`. The executor refuses on its own authority: paths must be relative and contained, symlinks and multiply linked files are refused, the root identity is rechecked before each operation, writes are compare-and-swap, and `exec` is refused without an OS sandbox ([workspace-executor.ts L122-L166](https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-executor.ts#L122-L166), [workspace-command-runner.ts L375-L436](https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-command-runner.ts#L375-L436)). `corrected in review`. It also intersects the requested capabilities with what it allows ([workspace-agent-connection.ts L286-L330](https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-agent-connection.ts#L286-L330), `unchecked`).
- **Hermes Agent relay.** The gateway dials out to a relay with a short-lived signed token, and sends `inbound_ack` only after the message is admitted to the durable queue, so the connector redelivers unacknowledged frames ([ws_transport.py L971-L1004](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/relay/ws_transport.py#L971-L1004)); only the gateway side was traced. `unchecked`.

**Controller dials in over SSH, and the pipe is the fence.**
- **Pi `pi-env`.** The daemon is a framed stdio tool server. It kills every process group it started after 30 s of silence or on stdin EOF, a request in flight when the pipe drops fails with `lost: true`, and handles do not survive a reconnect ([connection.ts L421-L436](https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/env/src/connection.ts#L421-L436), [main.rs L532-L547](https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/env/daemon/src/main.rs#L532-L547)). The daemon refuses nothing: paths and commands go straight to the OS ([main.rs L137-L282](https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/env/daemon/src/main.rs#L137-L282)). `checked in review`, `absence confirmed` for the missing policy.
- **DeepSeek SSH.** The loop stays local and a pinned helper runs remotely; the helper holds a 30 s lease, and loss of the SSH connection is final: pending requests reject and an ambiguous request is never replayed ([index.ts L279-L328](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/ssh/ssh/src/index.ts#L279-L328)). The helper runs its own sandbox chain, and the remote's reported enforcement level is trusted ([index.ts L9-L31](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/ssh/sandbox-ssh/src/index.ts#L9-L31)). `checked in review`.
- **Cline hub over SSH.** The CLI is uploaded, a loopback hub is started on the remote host and tunnelled to; the loop and tools run remotely, and the provider key is sent in each `session.create` ([remote-environments.ts L325-L400](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/remote/remote-environments.ts#L325-L400), [chat-session.ts L921-L980](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/examples/desktop-app/sidecar/chat-session.ts#L921-L980)). The client also sets the tool policies ([session-handlers.ts L497-L560](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/handlers/session-handlers.ts#L497-L560), `inferred` for the consequence); the hub has no fencing (a single daemon). `checked in review`.

**One owner, other clients attach.**
- **goose roaming.** A peer connects over QUIC and needs its key in the host's trust book, which is re-read on every inbound connection and refuses on a read error; a watcher polls it every 2 s and closes the connections of removed keys ([trust.rs L19-L125](https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-roaming/src/trust.rs#L19-L125), [node.rs L324-L355](https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-roaming/src/node.rs#L324-L355)). The host forces the working directory on new, load and fork, but the working-directory update bypasses it ([manage_sessions.rs L4-L30](https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/acp/server/manage_sessions.rs#L4-L30)). `checked in review`.
- **Hermes viewers.** A viewer mints a ticket on a local socket (peer uid checked, 30 s, single use) and dials the gateway with it in a WebSocket subprotocol ([control_socket.py L1-L9](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/control_socket.py#L1-L9)). The gateway fences its own restarts with a runtime epoch and a per-session generation checked at settle ([hermes_state_runtime.py L206-L242](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_runtime.py#L206-L242)). `checked in review` for the ticket; `corrected in review` for the fence.

**Designs that assume the old owner is dead.**
- **OpenCode.** The boot sweep finds sessions with a claim and re-drives them; the file's own doc comment says the sweep assumes every orphaned claim's owner is dead, and nothing in the data is a lease or a token ([execution.ts L130-L151](https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/execution.ts#L130-L151), [restart.ts L16-L34](https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/execution/restart.ts#L16-L34)). `checked in review`.
- **goose.** The one-run guard is in memory in one process ([active_run.rs L38-L66](https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/active_run.rs#L38-L66)); the files report no fencing. `unchecked`.
- **File locks without expiry.** Codex takes an advisory lock per thread ([writer_lock.rs L45-L95](https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rollout/src/writer_lock.rs#L45-L95)); DeepSeek takes `flock` with no expiry ([lease.ts L70-L116](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/session/session-persistence-jsonl/src/lease.ts#L70-L116)); Oh My Pi takes an OS lock per session ([session-manager.ts L1157-L1182](https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-manager.ts#L1157-L1182)). All three fence only processes that share a filesystem. The first two are `checked in review`; Oh My Pi's is `unchecked`.

| Fencing approach | Projects | What it covers | What it misses |
|---|---|---|---|
| None, assume the old owner is dead | OpenCode, goose | restarts of a single process | two processes on one database, a network split |
| Pipe or lease lifetime | pi-env, DeepSeek SSH | a dead controller cannot leave a live executor | the session cannot survive a disconnect |
| File lock with no expiry | Codex, DeepSeek, Oh My Pi | two processes on one filesystem | other hosts, network filesystems |
| One attachment plus retention | Codex exec-server | a second client | a stale client that still holds the socket |
| Latest connection wins | OpenCodex Remote Workspace | a reconnect replaces a dead socket | frames already read from the old socket |
| Epoch re-checked per request, plus a writer check inside the write | OpenClaw cloud worker | a stale worker after re-assignment or rotation, duplicate commits | writes that carry no expected writer |
| Epoch plus per-session generation checked at settle | Hermes Agent | a restart's lost turns, a stale settle | a remote writer (it is one host) |

The table is from "Fencing mechanisms" in `round2/synth/05-links-workers-protocols.md`, where OpenClaw's is the only design that fences a remote executor's writes.

### 6.5 Resume: replay, snapshot plus gap repair, cursors, durable and transient frames

**Flow.**
1. The client or the process comes back and names what it already has.
2. The server returns what is missing: a replay from a base, a snapshot, or the events after a cursor.
3. The receiver removes duplicates and repairs gaps.
4. Frames that are never persisted (deltas, progress) are not replayed.

**Replay from the newest compaction.**
- **Codex.** `reconstruct_rollout` picks the newest compaction that carries a full replacement history as the base and scans the suffix backwards; a rollback marker means "drop the newest N user turns" ([rollout_reconstruction.rs L205-L260](https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/session/rollout_reconstruction.rs#L205-L260)). `unchecked`.
- **Oh My Pi.** `buildSessionContext` walks parent links from the leaf and replays the compaction entry ([session-context.ts L284-L323](https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-context.ts#L284-L323)). `checked in review` for the leaf rule; `unchecked` for the compaction replay.
- **goose.** `session/load` replays the stored conversation to the client, optionally only the tail, and restarts a turn if a confirmation is pending ([load_session.rs L367-L455](https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/acp/server/load_session.rs#L367-L455)). `checked in review`.

**Snapshot per reconnect, with gap repair on the client.**
- **DeepSeek harness.** The `follow` stream yields one snapshot (a message-aligned window and a cursor), then durable events from a buffer armed before the snapshot was read, then live frames. The request carries no resume cursor (the documentation says it does), so on reconnect the client takes a fresh snapshot and repairs any break in the sequence by paging ([listener.ts L176-L248](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/api/session-controller/src/history/listener.ts#L176-L248), [journal-stream.ts L305-L330](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/api/gateway/src/client/journal-stream.ts#L305-L330)). `unchecked`.
- **OpenCode (first-party clients).** On `server.connected` the clients re-fetch state over REST and then apply live events; the live stream is volatile, with a 4,096-entry queue per subscriber that fails on overflow ([event-feed.ts L66-L68](https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/event-feed.ts#L66-L68)). `checked in review`.

**Cursors over sequence numbers.**
- **OpenHands (client).** The socket asks for `after_seq=<n>`; the per-socket cursor is the highest sequence number with nothing missing below it, and it steps over a hole after more than 1,024 later sequences ([session-seq-cursor.ts L1-L63](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/utils/session-seq-cursor.ts#L1-L63), [conversation-websocket-context.tsx L793-L823](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/contexts/conversation-websocket-context.tsx#L793-L823)). `checked in review`.
- **Cline.** The hub appends every event to a SQLite table with an autoincrement sequence before fan-out, and `stream.subscribe {sinceSequence}` replays pages and then tails live; the log is a 7-day replay aid, and the messages on disk are canonical ([hub-event-log.ts L78-L90](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/hub-event-log.ts#L78-L90), [browser-websocket.ts L373-L450](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/browser-websocket.ts#L373-L450)). `checked in review`.
- **OpenCode (log API).** `GET .../log?after=<seq>&follow=<bool>` captures a watermark, subscribes before reading, pages the rows, emits a marker and then follows ([bus.ts L829-L878](https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/bus.ts#L829-L878)). The shipped clients do not use it, and the table behind it is empty unless persistence is on. `checked in review`.
- **Codex exec-server and remote control.** The executor keeps a process table per session and the client reads missed output with `process/read` from a sequence cursor ([client_recovery.rs L57-L62](https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/client_recovery.rs#L57-L62)); remote control resumes with an `x-codex-subscribe-cursor` header ([websocket.rs L70-L73](https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/remote_control/websocket.rs#L70-L73)). `checked in review` for the first, `unchecked` for the second.
- **Warp.** Agent events arrive on a stream with a monotonic `sequence`, and the client stores its cursor on the server so a restored driver does not replay ([ai.rs L3715-L3730](https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/server/server_api/ai.rs#L3715-L3730)). `unchecked`.

**Acknowledge after the durable commit.** Hermes Agent sends `inbound_ack` only after the message is admitted ([ws_transport.py L971-L1004](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/relay/ws_transport.py#L971-L1004)); OpenClaw acknowledges after the claim is owned ([ingress-queue.kernel.ts L145-L258](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/channels/message/ingress-queue.kernel.ts#L145-L258)). A crash before the commit leaves the frame unacknowledged, and the sender redelivers.

**Durable and transient frames.**
- **OpenHands** names the distinction on the wire: `durable` frames carry a `seq` and are replayed, while `transient` frames and item deltas are never persisted or replayed ([session-frames.ts L1-L79](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/types/agent-server/session-frames.ts#L1-L79)). `checked in review`.
- **OpenClaw** separates `worker.transcript.commit` (sequenced and ledgered) from `worker.live-event` ([worker-admission.ts L54-L66](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/gateway-protocol/src/schema/worker-admission.ts#L54-L66)). `unchecked` for the commit ledger.
- **OpenCode** treats its live bus as volatile by contract ([bus.ts L128-L134](https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/bus.ts#L128-L134)). `checked in review`.
- **Codex** drops deltas, begin and update events and approval requests from the rollout ([policy.rs L16-L52](https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rollout/src/policy.rs#L16-L52)). `unchecked`.

### 6.6 Fork, and how the active branch is recorded

**Flow.**
1. Choose a cut point (a message, a turn or a sequence number).
2. Create the child. The child either points at the parent's history, copies it, or shares the file.
3. Settle any turn that was open at the cut.
4. Record which branch is active, or leave it to be inferred.

**Parent pointers inside one file.**
- **Oh My Pi.** Every entry has an `id` and a `parentId`. `branch(id)` moves the in-memory leaf only, and the next append becomes a sibling ([session-manager.ts L4048-L4060](https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-manager.ts#L4048-L4060)). The leaf after a load is the last physical entry unless that entry carries `activeLeafId`, which is set only on off-branch appends ([session-entries.ts L373-L397](https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-entries.ts#L373-L397), [session-manager.ts L3595-L3628](https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-manager.ts#L3595-L3628)). `/branch` and `/fork` write a new file with `parentSession` in the header ([session-manager.ts L4144-L4226](https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-manager.ts#L4144-L4226)). `checked in review`, with the `/tree` no-op rule `corrected in review`.
- **Pi.** The same tree in one file. The active leaf is not stored: on load it is the last non-header entry, and `branch` only changes the in-memory leaf ([session-manager.ts L1104-L1123](https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/session-manager.ts#L1104-L1123), [session-manager.ts L1590-L1604](https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/session-manager.ts#L1590-L1604)). After `/tree` to an older node with no summary, a restart before the next message reopens on the most recently written entry, which is on a different branch (`inferred`, called sound by the reviewer). `fork` and `clone` write a new file with only the root-to-leaf path and `parentSession` set ([session-manager.ts L1643-L1738](https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/session-manager.ts#L1643-L1738)). `checked in review` for the leaf rule, `unchecked` for the new-file fork.

**Pointers over immutable files.**
- **Codex.** In paginated history mode `thread/fork` stores `history_base {rollout_id, end_ordinal_exclusive, end_byte_offset}` in the child's metadata; the child file starts empty and reads the parent up to that offset ([protocol.rs L3463-L3477](https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/protocol/src/protocol.rs#L3463-L3477), [paginated_fork.rs L18-L60](https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/thread-store/src/local/paginated_fork.rs#L18-L60)). `thread/revert` creates a new file under the same thread id and swaps the SQLite row's path by compare-and-set, deleting the new file and returning `Conflict` if the swap fails ([revert_thread.rs L15-L45](https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/thread-store/src/local/revert_thread.rs#L15-L45), [revert_thread.rs L138-L156](https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/thread-store/src/local/revert_thread.rs#L138-L156)). The active history is the file the row points to (`inferred`). `unchecked` for the fork, `checked in review` for the revert.
- **Pi (`pi-durable`).** A fork "sees its parent's entries up to `entryId`" and nothing is copied; documents are copied under a fork policy ([forks.ts L19-L50](https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/durable/src/session/forks.ts#L19-L50)). `checked in review` for the package's other behaviour; the fork description is the author's reading.

**A child log seeded with an inherited prefix.**
- **DeepSeek harness.** `buildForkSeed` copies events up to the boundary into the child's own file, appends a `session/end-seed` marker, and closes any open turn with `forked` closers; the child records `inheritedEventCount` ([fork.ts L21-L30](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/session/src/fork.ts#L21-L30), [commands.ts L230-L312](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/api/session-controller/src/commands.ts#L230-L312)). Logs are linear, so there is no active-branch record (`inferred`). `checked in review`, including the refutation of "fork inside an open turn".
- **OpenCode.** A new `session_v2` row records `fork_session_id` and `fork_boundary`, and the settled message rows up to the boundary are copied under new ids, skipping unfinished rows ([session.ts L339-L382](https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session.ts#L339-L382), [projector.ts L121-L160](https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/projector.ts#L121-L160)). There is no active-branch record; revert deletes the rows after the boundary. `unchecked`.
- **Hermes Agent.** `branch_in_transaction` requires the parent to be idle and clones the active rows up to the boundary in one transaction, writing `parent_session_id` ([hermes_state_mutation_branch.py L12-L31](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_mutation_branch.py#L12-L31), [hermes_state_mutation_branch.py L52-L104](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_mutation_branch.py#L52-L104)). Rewind sets the model-side rows to `active=0` under a compare-and-swap on the visible tail ([hermes_state_rewind.py L64-L77](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_rewind.py#L64-L77)). `unchecked`.
- **OpenClaw.** The completed prefix is copied, and the fork is refused as `too-large` if the parent exceeds a token ceiling for the destination model ([session-create-fork.ts L26-L70](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/session-create-fork.ts#L26-L70)). `unchecked`.
- **goose.** `copy_session` copies the whole conversation into a new row, and ACP `session/fork` can then delete messages from a timestamp on in the copy ([session_manager.rs L2965-L3013](https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/session/session_manager.rs#L2965-L3013), [fork_session.rs L1-L60](https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/acp/server/fork_session.rs#L1-L60)). `unchecked`.
- **Cline.** Restoring with messages starts a new session seeded with the messages up to a user turn and records fork metadata; the old session is kept ([session-versioning-service.ts L233-L346](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/session/session-versioning-service.ts#L233-L346)). `unchecked`.
- **OpenHands (client).** The client calls the server's fork with a `from_event_id`; the fork internals are server side ([agent-server-conversation-service.api.ts L1301-L1352](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/conversation-service/agent-server-conversation-service.api.ts#L1301-L1352)). `unchecked`.

Most forks in these files carry the conversation only. Codex says its revert does not touch local files, and OpenCode, Hermes Agent, goose and OpenHands copy no file state. Exceptions: DeepSeek's fork carries a forked workspace if the parent had one, Cline's restore can also restore the workspace, and Warp's handoff forks the server conversation together with a workspace snapshot. See "Fork, branch, active-branch record, export and import" in `round2/synth/06-sessions-fork-checkpoints.md`.

### 6.7 Checkpoints: what is captured, and what each design scans and misses

**Flow.**
1. At a trigger (the first model call of a run, each model step, before a risky tool, or on request), capture the state of the tree.
2. Store it: as commits in the user's repository, in a separate repository, or as an in-memory baseline.
3. To restore, capture the present first, check that the restore target still applies, and write the files back.
4. Accept what the capture misses.

| Mechanism | Projects | Trigger | What it scans | Shell-made changes |
|---|---|---|---|---|
| Stash-shaped commits in the user's repository | Cline | first model call of each user run | tracked changes via stash; untracked non-ignored files via a private index | yes (tree-based); ignored files and files outside the repository no |
| A shadow Git repository, full scan | OpenCode, DeepSeek (display only) | each model step; turn start and end | OpenCode: `diff-files` plus `ls-files --others` over every file; DeepSeek: `git add --all` into a private index | yes for git paths |
| A shadow repository, triggered by the tool | Hermes Agent | before `write_file` or `patch`, and before a `terminal` call whose command text matches a regex | the project tree via `git add` into a per-project index | only when the regex matches |
| A write ledger | Hermes Agent (restore) | fed by `write_file` and `patch` | a path restores only if its hash equals the one the agent last wrote | no: shell-changed files are skipped, with exceptions |
| An in-memory per-file baseline | Zed (action log) | first agent touch of a buffer | only buffers the tools touched | no |
| Conversation only | Oh My Pi, Hermes `/undo` | n/a | n/a | n/a |
| None | Codex, goose, Pi (shipped) | n/a | n/a | n/a |

**Stash commits in the user's repository (Cline).** At the first model call of a run, a hook builds a stash-shaped commit with `git stash create`; if there are untracked, non-ignored files, it adds them as a third parent built through a private index file. The commit is kept reachable under `refs/cline/checkpoints/<session>/<run>` ([checkpoint-hooks.ts L535-L649](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hooks/checkpoint-hooks.ts#L535-L649)). Restore is a transaction: stash the present behind a transaction ref, refuse if `HEAD` is not the checkpoint's commit, move `HEAD` with a compare-and-swap `update-ref`, `reset --hard`, `clean -fd` only if untracked files were captured, then `stash apply` ([checkpoint-restore.ts L50-L154](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/session/checkpoint-restore.ts#L50-L154), [checkpoint-restore.ts L376-L478](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/session/checkpoint-restore.ts#L376-L478)). Round 1 said Cline uses a shadow repository; the code shows stash commits in the user's own repository, and a reviewer corrected a qualifier about non-git directories. `corrected in review`. The Pi example extension keeps an in-memory map of `git stash create` refs per turn; it ignores untracked files, loses the map on restart and offers `stash apply` only on fork ([git-checkpoint.ts L10-L48](https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/examples/extensions/git-checkpoint.ts#L10-L48)). `checked in review`.

**A shadow repository.**
- **OpenCode.** A separate Git directory under the data dir borrows the project's objects through alternates and seeds its index from the project's index; a capture lists tracked and untracked changes against a temporary index, skips untracked files over 2 MiB, and takes `write-tree` as the snapshot id, at the start and end of each model step ([snapshot.ts L89-L105](https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/snapshot.ts#L89-L105), [git.ts L503-L520](https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/git.ts#L503-L520), [step.ts L76-L79](https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/step.ts#L76-L79)). Undo stages the current tree as the original, restores exactly the files assistant steps touched after the boundary, and commits by deleting the later rows ([revert.ts L22-L56](https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/revert.ts#L22-L56)). A user edit to a file an assistant step also touched is overwritten (`unchecked`). `checked in review` for the 2 MiB cap and the step wiring.
- **Hermes Agent.** One bare repository holds a ref and a per-project index for each working directory, and every git call sets `GIT_DIR`, `GIT_WORK_TREE` and `GIT_INDEX_FILE` and nulls the global config ([checkpoint_manager.py L13-L39](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/checkpoint_manager.py#L13-L39), [checkpoint_manager.py L400-L438](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/checkpoint_manager.py#L400-L438)). Checkpoints are off by default and are taken before `write_file` and `patch`, and before a `terminal` call whose command text matches a destructive-command regex, at most once per directory per turn ([tool_executor.py L1030-L1046](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/tool_executor.py#L1030-L1046), [checkpoint_manager.py L1052-L1080](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/checkpoint_manager.py#L1052-L1080)). A reviewer found the regex catches `git rm` but not `bash -c "rm ..."`, `find -delete`, `python -c` or `tee`. `corrected in review`.
- **DeepSeek harness.** A recorder takes git tree snapshots at the start and end of each top-level turn through a private index and object directory, for display only; there is no restore ([git.ts L120-L170](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/deliverables/workspace-changes/src/git.ts#L120-L170)). `absence confirmed` for restore.
- **Zed.** The per-prompt checkpoints were removed. The surviving archive checkpoint writes two commits, one for the staged state and one for the working tree through a temporary copy of the index with `git add --all`, and restores with `read-tree` only if `HEAD` is still the original commit ([repository.rs L3085-L3135](https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/git/src/repository.rs#L3085-L3135)). `checked in review`; the cost reasoning is `inferred`.
- **OpenClaw.** `snapshotWorktree` builds a commit with a temporary index under `refs/openclaw/snapshots/<worktreeId>`; nested repositories are refused ([snapshot-inventory.ts L406-L434](https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/worktrees/snapshot-inventory.ts#L406-L434)). What it scans and its cost guards are not described. `unchecked`.

**A write ledger.** Hermes restores in safe mode: it takes a pre-rollback snapshot, stages the current tree, and restores a changed path only if its current hash equals the hash recorded when the agent last wrote it, or the file is missing; other paths are listed as skipped user edits ([checkpoint_manager.py L958-L984](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/checkpoint_manager.py#L958-L984), [checkpoint_manager.py L986-L1050](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/checkpoint_manager.py#L986-L1050)). The ledger is fed only by `write_file` and `patch`. A file the agent wrote and the shell then deleted is restored, and an empty ledger means a full restore. `corrected in review`.

**An in-memory per-file baseline (Zed).** `ActionLog` keeps, for each buffer the agent touched, the text from before the first touch and a patch of unreviewed edits; accept folds edits into the baseline and reject writes the baseline back ([action_log.rs L168-L192](https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/action_log/src/action_log.rs#L168-L192)). Rewinding a thread rejects all unreviewed edits in the whole log, not only those after the chosen message ([acp_thread.rs L8028-L8070](https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/acp_thread/src/acp_thread.rs#L8028-L8070)). The terminal tool does not feed it. It costs nothing per repository size and sees nothing the shell changes. `checked in review`.

**A snapshot for transfer, not undo (Warp).** `/handoff` derives the touched repositories from the last 500 tool calls (paths the agent wrote plus each working directory) and uploads a diff snapshot with caps of 100 files and 25 MiB per file ([pipeline.rs L440-L461](https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/blocklist/handoff/pipeline.rs#L440-L461), [touched_repos.rs L312-L357](https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/blocklist/handoff/touched_repos.rs#L312-L357)). It does not restore. `unchecked`.

**Conversation only and none.** Oh My Pi's `checkpoint` and `rewind` tools branch the conversation with a summary and revert no files ([checkpoint.ts L53-L131](https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/checkpoint.ts#L53-L131)); `checked in review`. Codex records the files `apply_patch` touched per turn for display and has no restore path ([turn_diff_tracker.rs L1-L22](https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/turn_diff_tracker.rs#L1-L22)); `absence confirmed`. goose has no file checkpoint, rewind or undo (`absence confirmed`).

**What the designs share.** Every design that restores files takes a safeguard before writing: Cline snapshots the present first and guards on `HEAD`, Hermes takes a pre-rollback snapshot and consults the ledger, OpenCode stages the original so that a restore can be undone, and Zed requires `HEAD` to match. Cost guards differ widely: OpenCode caps untracked files and reuses a temporary index; Hermes skips directories over 50,000 candidate files and caps snapshots; Cline has a persistent private index but no size, count or total cap. See "Checkpoints and undo" in `round2/synth/06-sessions-fork-checkpoints.md` for the full table of guards.

### 6.8 Where to read next

The patterns above are summarised from the synthesis files, which hold the full per-project accounts and the advice for the reader's harness. For the loop and durability, see `round2/synth/01-loop-dispatch-durability.md`; for links, workers and fencing, `round2/synth/05-links-workers-protocols.md`; for sessions, fork and checkpoints, `round2/synth/06-sessions-fork-checkpoints.md`. Each has a section "For rizzy-harness" and a section "Not settled".

## 7. Security and isolation mechanisms

This section says what the code of each harness enforces when the agent wants to run something, and what it leaves to the model, the user or nobody. It draws on groups 2, 3 and 4 (`round2/synth/02-approvals-permission-rules.md`, `03-sandbox-containers-execution.md`, `04-credentials-secrets-auth.md`) and the reviewers' corrections. Trust labels (`unchecked`, `inferred`, `not settled`) are carried where the input carries them; everything else was traced by a Round 2 author and, for most of what is cited here, re-opened by a reviewer.

### Summary table

The cells condense sections 3.1 to 3.4 of `round2/synth/02-approvals-permission-rules.md`, sections 3.1 and 3.3 of `round2/synth/03-sandbox-containers-execution.md` and section 3.1 of `round2/synth/04-credentials-secrets-auth.md`. The permalinks are in the per-project text below.

| Project | Default approval mode | Approval bound to | OS sandbox | Child environment | Model approver, failure mode | Known bypass |
|---|---|---|---|---|---|---|
| Codex | CLI/TUI: reviewer `user` (a human answers); `exec` forces `Never` (asks rejected) | env id, executable, canonical argv, cwd, tty, sandbox and extra permissions | Linux bwrap + seccomp; macOS Seatbelt; Windows opt-in | inherited; `OPENAI_API_KEY` reaches commands | Guardian: error denies, timeout returns to the human | an `allow` rule runs the command outside the sandbox |
| OpenCode | base `*:* allow`; only `.env` reads ask | request id only | none | whole `process.env` | none | `` !`cmd` `` templates; HTTP session rules appended last |
| OpenClaw | host exec `security=full`, `ask=off`; sandbox off | argv, cwd, agent, session, env-override hash, executable (hashed if user-modifiable), node, device | opt-in Docker or Podman container | inherited minus injection keys on host; declared only in the sandbox | `auto` reviewer: any failure asks | a Gateway approval overrides the node's allow-list |
| Hermes Agent | `smart`; `-z` and library context auto-approve | a pattern class | none locally; Docker, SSH, Modal, Daytona back ends | terminal blocklist; allow-list for `execute_code` and MCP | auxiliary model: failure goes to the human | container back ends skip the hard floor; `execute_code` on the host kernel |
| Oh My Pi | `yolo`; sub-agents forced to `yolo` | `subject\0tier` (session), `tier\0path` (cached grant) | none | whole env minus project `.env` values | none in the approval path | project settings can set `yolo` |
| Pi | no approval system | not applicable | none | whole env | none | `!` commands; hook argument mutation |
| DeepSeek harness | `workspace-write`; no ordinary call asks | one call id, no arguments | writes only (bwrap, Landlock, Seatbelt) | scrubbed by name pattern | opt-in Auto review, not mounted by any shipped bundle: failure denies | MCP and in-process tools never ask |
| goose | `Auto` | persisted call id, no arguments; "always" keyed on tool name | none (Seatbelt removed) | whole env | SmartApprove fails closed; adversary and injection classifier fail open | Code Mode inner calls skip approval, inspectors and hooks |
| Cline | CLI, Desktop and cron approve everything | opaque `approvalId`; any client may answer | none | whole env | none | sub-agents run with no policy; Origin-only hub sessions |
| OpenHands (client) | `NeverConfirm` unless settings enable confirmation | nothing: `{accept}` answers whatever is pending | upstream Docker, not read | session key placed in the agent's environment | inline model-set `security_risk` (`unchecked`) | server-level bash outside any conversation |
| Zed | `confirm` | tool plus a prefix regex | Linux bwrap, macOS Seatbelt, Windows via WSL | inherited (terminal and ACP agents) | none | external ACP agents ignore `tool_permissions` |
| Warp | GUI `AlwaysAsk`; TUI `AgentDecides` | per call; model flags can pre-approve | none | MCP inherits whole env | none on the client; model's own `is_risky` | run-to-completion drops the user denylist |
| OpenCodex | no tool approvals; Remote Workspace `approvalPolicy: "never"` | session id, root id and capability set per RPC | bwrap on the executor | `--clearenv` plus four names | none | unauthenticated loopback listener |

### Project by project

**Codex.** The rule engine takes the maximum of allow, prompt and forbidden over every matching rule, so a deny anywhere wins: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/execpolicy/src/policy.rs#L577-L595. An approval in a session is keyed on the environment, executable, canonical argv, cwd and the requested permission delta: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/tools/runtimes/unified_exec.rs#L87-L98. "Always" writes an `allow` prefix rule, and an explicit `allow` rule returns `Skip { bypass_sandbox: true }`, so the remembered command runs outside the sandbox: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/exec_policy.rs#L492-L512. `codex exec` defaults to `Never` and rejects approval requests: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec/src/lib.rs#L622-L640. Untrusted projects still run under the internal `UnlessTrusted` mode: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/tools/sandboxing.rs#L195-L236.

The Linux sandbox is bubblewrap followed by seccomp in a two-stage helper. PATH entries the sandbox can write are skipped and the bundled `bwrap` is digest-pinned: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/sandboxing/src/bwrap.rs#L190-L207. The seccomp filter is a denylist with default allow; with network off it denies `connect`, `bind`, `sendto` and non-`AF_UNIX` sockets, but `sendmsg` and `recvmsg` are in no deny list, so a pathname datagram socket stays reachable (reviewer correction; not tested): https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/seccomp.rs#L53-L65. Landlock is refused for restricted policies; the code's stated reason is only to "isolate app-server sockets", and the claim that Landlock cannot block `connect` is the author's inference: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/linux_run_main.rs#L386-L393. The exec-server refuses a command whose sandbox cannot be enforced: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/process_sandbox.rs#L226-L230. `workspace-write` has network off by default: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/protocol/src/protocol.rs#L1294-L1316.

The child environment is inherited with `ignore_default_excludes = true`, so `OPENAI_API_KEY` reaches commands: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/config/src/shell_environment_policy.rs#L133-L136. The Guardian reviewer fails closed on prompt-build, session and parse failures: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/ext/guardian-reviewer/src/completion.rs#L86-L170. The app-server refuses an unauthenticated non-loopback listener; the exec-server's own `--listen` WebSocket has no such refusal, and loopback is unauthenticated for both: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/websocket.rs#L160-L175. Hook trust hashes the hook's declaration, not the repository script it calls, and `PreToolUse` hooks fail open on error, timeout or bad JSON: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/hooks/src/engine/discovery.rs#L972-L998 and https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/hooks/src/events/pre_tool_use.rs#L211-L300.

**OpenCode.** Rules are one flat `{action, resource, effect}` list; the last match wins and no match asks: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/permission.ts#L94-L104. The base policy allows everything except reading `*.env`, so the shell never asks by default: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/schema/src/agent.ts#L46-L51. Only the administrator `policies` pass is a deny-only ceiling, run after saved rules: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/plugin/policy.ts#L98-L113. Session rules sent over HTTP are appended last, so any password holder can widen a session: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/handlers/session.ts#L101-L120. "Always" saves patterns that are often `*` or a first word: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/permission.ts#L317-L363. Plan mode denies edit tools only; shell, sub-agents and `execute` stay allowed: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/plan.ts#L33-L42. A sub-agent gets its agent's rules plus a creation-time copy of the parent's session rules: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session.ts#L311-L314. There is no OS sandbox: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell.ts#L370-L382. Outside stdio mode the server password stays in the environment every shell inherits, which would let a prompt-injected agent answer its own approvals (`inferred`): https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/cli/src/server-process.ts#L85-L97.

**OpenClaw.** Host exec ships `security=full, ask=off`, and the sandbox's `deny` default does not block sandboxed exec: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/exec-defaults.ts#L171-L188. The sandbox itself is off by default: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/config.ts#L212-L227. A one-off approval is the strongest binding in the set: argv, working directory, agent, session, a hash of the environment overrides, the node and the requester device: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/system-run-approval-binding.ts#L72-L90. The executable is hashed only when the file or a path component is writable or owned by the user: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/system-run-mutable-file-policy.ts#L19-L66. Resolution is re-checked before launch, with a small hash-to-exec window: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/system-run-approval-binding.ts#L550-L620. "Allow always" hashes only the working directory and the arguments after the first; the executable is matched by a path pattern: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/exec-command-resolution.ts#L222-L231. A paired node re-evaluates `system.run`, but a Gateway approval satisfies its allow-list; only a local deny holds: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/exec-policy.ts#L60-L117. The `auto` reviewer runs at temperature 0, 30 s, strict JSON; any failure asks, and an injection regex match denies: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/exec-auto-reviewer.ts#L396-L516. Login or interactive shells skip the reviewer: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/bash-tools.exec-host-gateway.ts#L631-L643. A chat `/approve` with an empty approver list authorises anyone in the chat: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/plugin-sdk/approval-auth-helpers.ts#L78-L81.

The container is the most carefully built in the set: `--init`, `no-new-privileges` and `--pull=never` always; `--read-only`, `--network none`, `--cap-drop ALL` and `--pids-limit 1024` by default; a recreate epoch when flags change: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/config.ts#L91-L117. Bind sources are validated lexically and on the real path, and three `dangerously*` booleans relax the checks: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/validate-sandbox-security.ts#L23-L53. Worker boxes dial out for the agent link, but the Gateway also SSHes into them: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/ssh.ts#L259-L291. A turn-scoped GitHub token reaches the worker and its shell: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/worker/launch-descriptor.ts#L183-L211. The secret egress proxy is cooperative, not enforced by a namespace (`inferred`): https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/secrets/egress-proxy/stream-substitution.ts#L26-L31.

**Hermes Agent.** The default mode is `smart`; the code falls back to `manual` if unset: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval_context.py#L307-L315. `smart` reviews only regex-flagged commands, and its DENY can be overridden once by a present human: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval.py#L882-L911. Cron, `-q`, API and webhook runs deny; `-z` and a bare library context auto-approve: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval.py#L1300-L1308. `approvals.deny` globs see through `sudo`, `env` and `sh -c`: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval_detection.py#L1623-L1664. Piping a URL into `sh` is an approvable dangerous pattern, not part of the hard floor: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval_detection.py#L124-L157. "Always" stores a pattern class in `command_allowlist` in the agent-reachable `config.yaml`: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval.py#L474-L487.

Container back ends skip the hard floor and the dangerous-command prompts and run only the deny globs: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval.py#L1161-L1175 and https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval.py#L1282-L1287. The "host access" exception does not look at a `-v` in `docker_extra_args`, which are appended last and can override anything: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/terminal_tool.py#L145-L162. Docker drops all capabilities and adds back three to five, with a pids limit of 2048 when cgroups allow: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/docker.py#L336-L343. The local terminal back end has no OS sandbox: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/local.py#L1149-L1168, and neither does `execute_code`, which runs a host kernel: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/code_execution_tool.py#L787-L802. The iron-proxy credential swap is wired into Docker only, through proxy environment variables, and the agent's own model calls bypass it: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/docker.py#L786-L812. A binary on PATH wins over the pinned proxy: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/proxy_sources/iron_proxy.py#L162-L177. A hook may escalate to the human or block, never approve: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/plugins.py#L1198-L1227; a crashing `pre_tool_call` hook fails open: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/tool_executor.py#L662-L676.

**Oh My Pi.** The default approval mode is `yolo`, and an invalid value also falls back to it: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/settings.ts#L314-L319 and https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/approval.ts#L97-L113. Sub-agents are forced to `yolo` with the user's deny and prompt rules kept: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/task/executor.ts#L1259-L1262. Project settings are merged with no key filter, so a repository can set the approval mode, the policy and the restricted folders: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/config/settings.ts#L2800-L2814. Project trust is hard-coded true: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/types.ts#L926-L937. The `bash.patterns` model is sound in shape: deny and prompt match per segment, allow must cover the whole command: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/bash.ts#L311-L321. Hooks fail closed on a throw or a 30 s timeout: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/runner.ts#L2338-L2364, and approval is re-resolved on the rewritten input: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/wrapper.ts#L355-L374. No OS sandbox was found (absence confirmed): https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/file-write-fallback.ts#L1-L15. The auth broker accepts any `--bind` without warning: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/cli/auth-broker-cli.ts#L145-L157.

**Pi.** There are no modes, rules or prompts; the shell is a plain spawn with the user's permissions: https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/agent-session.ts#L719-L743 and https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/tools/bash.ts#L124-L131. A throwing `tool_call` handler blocks the call: https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/agent/src/agent-loop.ts#L735-L786, but handlers mutate arguments in place with no re-validation: https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/docs/extensions.md#L981-L986. Project trust is a path-keyed boolean with no content binding: https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/trust-manager.ts#L193-L215, and context files load regardless of it: https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/resource-loader.ts#L679-L685.

**DeepSeek harness.** The deployment default is `workspace-write`: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/bundle/base/cordis.patch.yml#L243-L262. The pipeline reaches approval only when a plugin returns `ask`, and no shipped bundle has one that does for an ordinary call: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/tools/src/pipeline.ts#L197-L206. When approval is reached it fails closed and grants only once: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/interaction/user-approval/src/index.ts#L293-L333. The sandbox restricts writes only; reads and the network are not confined: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/sandbox/sandbox-local/src/profiles.ts#L11-L60. The computed enforcement level (`full` or `partial`) is stored on the tool result and reaches no client: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/shell/bash-sandbox/src/index.ts#L115-L124. With no back end the call fails `SANDBOX_UNAVAILABLE`, with no pass-through: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/sandbox/sandbox-local/src/index.ts#L499-L524. Plan mode is prompt-only: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/plan/plan-mode/src/index.ts#L1-L22. The automatic reviewer is opt-in, added by its own patch and mounted by no shipped bundle: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/experimental/auto-review/cordis.patch.yml#L1-L3. Child processes have credential-style names scrubbed: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subprocess/subprocess/src/index.ts#L40-L75, but the credential store and the cookie-signing secret sit in files the confined shell can read (see 4.1 and 4.2 in `round2/synth/04-credentials-secrets-auth.md`).

**goose.** The default mode is `Auto`: https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/config/base.rs#L1316-L1323. An approval is a once flag on the persisted call id, without the arguments, and it expires with the extension lease, so a restart cannot run a stale one: https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/state_machine/ops_toolcalling.rs#L999-L1039. "Always" is keyed on the bare tool name, so "always" on `shell` allows every command everywhere: https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/state_machine/tool_approval.rs#L108-L126. SmartApprove fails closed: https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/permission/permission_judge.rs#L41-L89, but an MCP server's own `readOnlyHint` auto-allows: https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/permission/permission_inspector.rs#L172-L176. The adversary reviewer fails open: https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/security/adversary_inspector.rs#L446-L464; the injection classifier is off by default and covers `shell` only: https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/security/mod.rs#L46-L55 and https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/security/scanner.rs#L132-L144; a failing inspector is dropped for the batch: https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/tool_inspection.rs#L84-L118. A tool called inside Code Mode skips approval, inspectors and pre-tool hooks: https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/code_execution.rs#L172-L194. There is no sandbox in the code; the macOS Seatbelt feature was removed and only a blog post remains (absence confirmed): https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/documentation/blog/2026-02-23-goose-v1-25-0/index.md#L19-L34. `--container` covers stdio and built-in extensions; the Developer shell stays on the host: https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/extension_manager/mod.rs#L932-L948. Nothing restricts `--dangerously-unauthenticated` to loopback: https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-cli/src/cli.rs#L1821-L1834.

**Cline.** The CLI auto-approves by default: https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/cli/src/main.ts#L885-L895. No model-set `requires_approval` field is honoured; only a legacy string remains: https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/tools/schemas.ts#L133-L157. `.clineignore` is not enforced; its controller is constructed only in a test and an optional example hook: https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/vscode/src/core/ignore/ClineIgnoreController.ts#L155-L228. One approval covers a whole concurrent `run_commands` batch: https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/tools/definitions.ts#L180-L196. Policies never reach a sub-agent and there is no depth or count cap: https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/tools/team/delegated-agent.ts#L120-L147. The plan-mode blacklist admits `python -c` in its own comment: https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/tools/command-guard.ts#L10-L17, and the CLI model can leave plan mode itself with an unchecked "user approved" claim: https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/cli/src/runtime/interactive/mode.ts#L31-L68. There is no OS sandbox; `CLINE_SANDBOX` only moves the data directory: https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/cli/src/utils/helpers.ts#L583-L605. The hub accepts a socket on a valid token or on a loopback `Origin`; Origin-only connections are refused only provider, MCP and cross-workspace commands, while `session.create` takes `cwd` and tool policies from the payload: https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/hub-websocket-server.ts#L676-L722 and https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/handlers/session-handlers.ts#L497-L560. `approval.respond` checks only the approval id: https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/handlers/approval-handlers.ts#L123-L161. Provider keys are stored unencrypted: https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/services/storage/provider-settings-manager.ts#L186-L214.

**OpenHands (client only).** The agent engine is in another repository; only the web client was read. Canvas sends `NeverConfirm` unless stored settings enable confirmation: https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L777-L789. An approval sends only `{accept, reason?}`, with no action id, and answers whatever is pending: https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/event-service/event-service.types.ts#L1-L4. The session key that unlocks the server is placed in the agent's shell environment: https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L975-L985, and key holders can run host commands outside any conversation: https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/bash-service/bash-service.api.ts#L24-L44. In the whole-stack Docker image the agent and the keys share one uid: https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/docker/Dockerfile#L188-L206. On a loopback bind the key is embedded in the page: https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/static-server.mjs#L389-L407.

**Zed.** Precedence is hard-coded `rm` deny, then deny, confirm, allow (every sub-command must match), tool default, global default: https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/tool_permissions.rs#L208-L260. "Always" writes a prefix regex into user settings: https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/thread.rs#L6985-L7075. The terminal sandbox is on for everyone: https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/feature_flags/src/flags.rs#L107-L121. On Linux bubblewrap makes every file readable and re-binds protected paths read-only, skipping a path that does not exist yet: https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/sandbox/src/linux_bubblewrap.rs#L246-L352. When bubblewrap is unavailable Linux prompts (retry, run unsandboxed, deny) and macOS fails closed: https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/tools/terminal_tool.rs#L722-L846. Escalation needs a non-empty `reason`, and the tool result says when a command ran unsandboxed: https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/tools/terminal_tool.rs#L678-L705. `fetch` is gated in-process, not OS-sandboxed: https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/tools/fetch_tool.rs#L45-L60. External ACP agents ignore `tool_permissions`, despite a settings comment that says otherwise: https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent_servers/src/acp/v1.rs#L10172-L10225, and Zed path-checks their fs requests but not their terminals: https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent_servers/src/acp/v1.rs#L10549-L10590.

**Warp (client only).** The GUI default for commands is `AlwaysAsk`; `AgentDecides` is the TUI default: https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/cloud_object_models/src/ai_execution_profile.rs#L399-L431. Under "Agent decides" the model's own `is_risky` and `is_read_only` booleans decide, unverified by the client: https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/blocklist/permissions.rs#L972-L1008. Run-to-completion defaults on, drops the user denylist and auto-allows reads, writes and MCP; only the organisation denylist stays: https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/blocklist/permissions.rs#L938-L956. There is no local OS sandbox, and `--sandboxed` is a self-declared flag: https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/warp_cli/src/agent.rs#L621-L623. Unattended child runs launch other vendors' agents with their safety flags disabled and trust pre-seeded: https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/pane_group/pane/local_harness_launch.rs#L120-L137.

**OpenCodex.** It has no tool approvals of its own; the Remote Workspace agent runs `approvalPolicy: "never"`, and the executor's capability set is the real policy: https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-codex-runtime.ts#L500-L513. Each RPC is bound to the session id, root id and capability set on the executor: https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-rpc.ts#L298-L346. `exec` runs in bubblewrap with `--clearenv` and an unshared network, and is offered only when a sandbox probe passes; writes need no probe: https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-command-runner.ts#L23-L47 and https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-device.ts#L372-L384. An unauthenticated loopback companion listener lets any local user spend the hub's credentials, with a start-up warning: https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/index.ts#L790-L802.

### Comparison

**Approval defaults.** Nine of the thirteen run ordinary commands without asking by default: OpenCode, OpenClaw host exec, Oh My Pi, Pi, DeepSeek harness, goose, Cline (CLI, Desktop, cron), OpenHands through Canvas and OpenCodex. Only Codex (interactive), Hermes Agent, Zed and Warp's GUI ask out of the box. Where the default is open, the approval code is not the boundary; containment comes from a sandbox or from nothing (see 4.1 in `round2/synth/02-approvals-permission-rules.md`).

**What an approval is bound to.** Ordered weakest to strongest (see 4.3 in `round2/synth/02-approvals-permission-rules.md`): nothing (OpenHands); an opaque id any authenticated client may answer (Cline, OpenCode, DeepSeek harness); a persisted call id that dies with the extension lease (goose); a tool name or pattern class (goose "always", Hermes Agent, Zed, Oh My Pi); canonical argv, cwd and the permission delta (Codex); and OpenClaw's binding, which adds agent, session, node, device, an environment-override hash and the executable's hash when the user can modify it. "Always" is permanent and wide in Zed, Codex, Hermes Agent, goose and OpenCode, and narrow only in OpenClaw. Codex is the one case where "always" also removes the sandbox.

**Rule precedence.** Two models exist. A lattice (Codex `max()`, Zed's deny > confirm > allow) is order-independent. An ordered list (OpenCode, last match wins) lets a later, wider layer override an earlier one, and needs a separate administrator pass to restore a ceiling (links above).

**Where a repository can loosen the user's settings.** Oh My Pi (project settings merged with no key filter; project MCP on by default, https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/config.ts#L131-L134), OpenCode (project config is a later layer, and project plugins load with no trust prompt, https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/plugin/source.ts#L125-L136), goose (project plugins and hooks auto-enable, https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/plugins/discovery.rs#L123-L152), OpenHands (repository `hooks.json` attached with no trust prompt, https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/hooks-service.ts#L17-L58) and Cline (workspace hooks and plugins, see 3.5 in `round2/synth/10-extensibility-unattended.md`). Codex is the strongest: project trust disables project config, hooks and exec policies, https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/config/src/loader/mod.rs#L1100-L1170, and trust state is read only from the user layer. Its gaps are the unhashed hook script and repository skills, which load without a trust check: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/config/src/state.rs#L742-L757.

**Model-based approvers and how they fail.**

| Project | Reviewer | On error | On timeout | Source |
|---|---|---|---|---|
| Codex | Guardian | deny | back to the human | link in the Codex paragraph |
| OpenClaw | `auto` reviewer | ask | ask | link in the OpenClaw paragraph |
| Hermes Agent | smart approver | human | human | see 4.4 in `round2/synth/02-approvals-permission-rules.md` |
| goose | SmartApprove | deny | not stated | link in the goose paragraph |
| goose | adversary | allow | not stated | link in the goose paragraph |
| goose | injection classifier | allow | not stated | see 4.4 in `round2/synth/02-approvals-permission-rules.md` |
| DeepSeek harness | Auto review (opt-in, unmounted) | deny | no timeout | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/experimental/auto-review/src/index.ts#L670-L693 |

Self-assessment is a different thing and is worse: the acting model labels its own call and the label lowers the gate (Warp `is_risky`; OpenHands inline `security_risk`, `unchecked`; goose trusting a server's `readOnlyHint`). A prompt-injected model sets whichever label gets it through. Cline removed its model-set flag. A model-supplied request for wider access, checked by the normal path and a human once (Zed's `reason`, Codex and DeepSeek harness `sandbox_permissions`), widens nothing on its own (see 4.4 in `round2/synth/02-approvals-permission-rules.md`).

**OS sandboxes.** Four projects ship an OS-level sandbox for commands:

| Project | Mechanism | Covers | Does not cover |
|---|---|---|---|
| Codex | bwrap + seccomp (Linux), Seatbelt (macOS) | restricted reads from a tmpfs root, writable roots, protected `.git`, `.agents`, `.codex`, `.aws`, network namespace, socket syscalls, `ptrace`, `io_uring` | `sendmsg` to a pathname datagram socket; deny globs are a launch-time snapshot; any command matched by an `allow` rule |
| Zed | bwrap (Linux), Seatbelt deny-default (macOS), bwrap in WSL | writes, network (proxy for host lists), protected paths | every file readable on Linux; protection of a path that does not exist yet; ACP agent terminals; `fetch` |
| DeepSeek harness | bwrap, then Landlock, then Seatbelt allow-default | writes to roots | reads, network, Unix sockets |
| OpenCodex | bwrap on the executor | minimal read-only system mounts, one `/workspace`, unshared network, cleared environment | no user namespace, `cap-drop` or seccomp |

Hermes Agent and OpenClaw offer containers instead (see "Containers" below).

Seven projects have no OS sandbox at all: OpenCode, Oh My Pi, Pi, goose, Cline, Warp and Hermes Agent's local back end (links in the paragraphs above). Pi and Cline say outright that sandboxing is the user's job; Warp's `--sandboxed` and Cline's `CLINE_SANDBOX` and `CLINE_PLUGIN_MODE=sandbox` use the word for things that confine nothing (https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/runtime/tools/worker-sandbox.ts#L267-L276). OpenHands runs a per-conversation Docker container upstream; that code was not in the repository read.

**Containers and what defeats them.** The container is only as strong as who sets its flags, whether pass-through arguments exist, and whether the harness relaxes its own checks inside (see 3.3 and 4.E in `round2/synth/03-sandbox-containers-execution.md`). OpenClaw fixes the dangerous flags and validates binds, with three named `dangerously*` escapes. Hermes Agent appends `docker_extra_args` last, so an operator line can override anything, does not count a `-v` there as host access, runs as root with `DAC_OVERRIDE` unless `--user` is set, and turns its safety floor off inside. goose's `--container` does not contain the shell. OpenHands' client sends unbounded memory and CPU and puts every saved secret plus the control-plane key into the container.

**Secrets within reach of the agent's shell.** In OpenCode, Oh My Pi, Pi, goose, Cline, Codex by default and OpenClaw host exec, provider keys are in the agent's environment, and every one of them also keeps keys in a file the same user can read (see 4.1 in `round2/synth/04-credentials-secrets-auth.md`). Name-based scrubbing (DeepSeek harness, Hermes Agent's terminal) misses names it does not know and is defeated by the readable store file. Allow-lists (Hermes Agent's `execute_code` and MCP, OpenClaw's sandbox, OpenCodex `--clearenv`) do not have that failure. Brokers that swap a dummy for the key (Codex, OpenClaw, Hermes Agent) are only as good as their coverage: only Codex pairs the broker with a network namespace that makes it the only path; OpenClaw's and Hermes Agent's are cooperative, and Hermes Agent's covers Docker only.

**Authentication of local servers and hubs.** Weakest first (see 4.5 in `round2/synth/04-credentials-secrets-auth.md`): no authentication on loopback (Codex app-server and exec-server, OpenCodex data plane, Oh My Pi `--no-auth`, goose `--dangerously-unauthenticated`); `Origin` as authentication (Cline); one shared secret, sometimes accepted in a URL (OpenCode `?auth_token=`, https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/middleware/authorization.ts#L13-L40; goose `?token=`, https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/acp/transport/auth.rs#L9-L36); a launch token then a signed cookie with a Host and Origin fence (DeepSeek harness, https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/client/connection/src/browser-auth.ts#L182-L199); single-use 30 s tickets over a same-user socket (Hermes Agent, non-loopback dashboard refused without auth, https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/main_dashboard.py#L640-L662); device keys signing a challenge plus the token and scopes (OpenClaw, see 3.2 in `round2/synth/04-credentials-secrets-auth.md`). Refusing a wider bind without auth is common; the Codex exec-server and goose show the gap when one listener of several is left out.

**Trust gates hard-coded open.** Oh My Pi's `isProjectTrusted()` returns true (link above). OpenCode, goose, Cline and OpenHands hooks have no gate at all. Pi's gate is a path boolean. Codex's gate is per layer, with hashes the repository cannot write, but script bytes and skills are not covered. None binds trust to the content of everything that will execute (see 4.7 in `round2/synth/04-credentials-secrets-auth.md`).

**Paths that bypass the approval path.**

| Project | Path | Permalink |
|---|---|---|
| Codex | an `allow` rule runs outside the sandbox; `--dangerously-bypass-hook-trust` | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/exec_policy.rs#L492-L512 ; https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/utils/cli/src/shared_options.rs#L61-L64 |
| OpenCode | `` !`cmd` `` command templates run with no permission call, also on user-typed arguments | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/plugin/command.ts#L190-L215 |
| OpenCode | denying `execute` turns Code Mode tools into direct tools | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool.ts#L265-L270 |
| goose | Code Mode inner calls skip approval, inspectors and hooks | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/code_execution.rs#L172-L194 |
| Hermes Agent | `execute_code` on the host kernel; container back ends skip the floor | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/code_execution_tool.py#L787-L802 |
| Cline | sub-agents run with no policy; one approval per command batch | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/tools/team/delegated-agent.ts#L120-L147 |
| OpenHands | server-level bash for key holders | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/bash-service/bash-service.api.ts#L24-L44 |
| Warp | run-to-completion drops the user denylist | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/blocklist/permissions.rs#L938-L956 |
| Zed | ACP agents ignore `tool_permissions` | https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent_servers/src/acp/v1.rs#L10172-L10225 |
| OpenClaw | a Gateway approval overrides the node allow-list | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/exec-policy.ts#L60-L117 |
| Pi | `!` user commands; installer scripts not disabled | see 3.4 in `round2/synth/02-approvals-permission-rules.md` |

### The recurring failure modes

**1. Model-based guards and policy hooks that fail open.** A guard that stops enforcing when it crashes, times out or returns junk is silent exactly when it is needed.
- goose: the adversary reviewer (https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/security/adversary_inspector.rs#L446-L464), any failing inspector (https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/tool_inspection.rs#L84-L118) and the extension malware check (https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/extension_malware_check.rs#L422-L440).
- Codex: `PreToolUse` hooks (https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/hooks/src/events/pre_tool_use.rs#L211-L300).
- Hermes Agent: a crashing `pre_tool_call` hook (https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/tool_executor.py#L662-L676) and the memory and skill write gate on import failure (https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/write_approval.py#L201-L216).
- Cline: file hooks on every failure (https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hooks/hook-file-hooks.ts#L415-L454). OpenHands: a hook load failure drops all hooks (https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/hooks-service.ts#L30-L42).
- DeepSeek harness: removing the Auto review plugin widens to full access (see 3.3 in `round2/synth/02-approvals-permission-rules.md`).
- The counter-examples: Codex's Guardian denies on error, OpenClaw's reviewer asks, OpenClaw's `before_tool_call` chain blocks on any throw (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/agent-tools.before-tool-call.policy.ts#L397-L408), Oh My Pi and Pi block on a throwing hook.

**2. Trust in a component's own claims about itself.**
- An MCP server's `readOnlyHint` auto-allows in goose (https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/permission/permission_inspector.rs#L172-L176).
- The model's `is_risky` and `is_read_only` decide in Warp (https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/blocklist/permissions.rs#L972-L1008), and its `--sandboxed` flag is self-declared (https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/warp_cli/src/agent.rs#L621-L623).
- The Cline CLI model can end plan mode by claiming the user approved (https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/cli/src/runtime/interactive/mode.ts#L31-L68).
- The DeepSeek harness SSH helper reports its own hash (https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/ssh/ssh/src/index.ts#L312-L318).
- Executors that trust the caller: the Codex exec-server materialises the caller's sandbox intent, and no intent means no sandbox; an OpenCodex executor's capabilities are self-declared at enrolment (see 4.H in `round2/synth/03-sandbox-containers-execution.md`). OpenClaw's node trusts a Gateway approval over its own allow-list (link above).

**3. Safety skipped inside containers.**
- Hermes Agent turns off its hard floor and prompts on container back ends (https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval.py#L1161-L1175), ignores `-v` in pass-through arguments (https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/terminal_tool.py#L145-L162), and disables checkpoints there (https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/checkpoint_manager.py#L940-L952).
- OpenClaw runs sandboxed exec without an approval by default (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/exec-defaults.ts#L171-L188).
- OpenHands skips hooks for container workspaces (see 3.3 in `round2/synth/03-sandbox-containers-execution.md`).
- The assumption behind all three is that the container is the boundary. It is not one when the container holds the credentials, the workspace and the network.

**4. The agent holding a credential to its own control plane.**
- OpenHands puts the master session key in the agent's environment on purpose (https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L975-L985), and an approval names no action.
- OpenCode leaves the server password in the shell environment outside stdio mode (`inferred` self-approval, https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/cli/src/server-process.ts#L85-L97).
- DeepSeek harness keeps the cookie-signing secret in a file the confined shell can read; Oh My Pi's broker token file is unprotected (see 4.2 in `round2/synth/04-credentials-secrets-auth.md`).
- Codex's unauthenticated loopback listeners let any local process, including the agent's own shell with network on, drive the agent (https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/websocket.rs#L160-L175).
- The counter-examples: OpenCode's stdio mode and Cline's core capture the spawn secret into memory and delete it from the environment before any child starts; Codex has an always-strip list for launch secrets (see 4.2 in `round2/synth/04-credentials-secrets-auth.md`).

**5. `Origin` or network position treated as authentication.**
- Cline is the one project that authenticates on `Origin` alone: a loopback `Origin` header, which any local process can set, admits a socket that can create sessions with its own tool policies and answer approvals; the end-to-end exploit was not run (https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/hub-websocket-server.ts#L676-L722).
- The wider pattern is loopback position as authentication: OpenCodex's companion listener (https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/index.ts#L790-L802), Codex loopback listeners, OpenHands embedding the key in the page on a loopback bind (https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/static-server.mjs#L389-L407), and OpenClaw's SSH-verified auto-approval, which rests on network position and a shell profile (`inferred`, https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/node-pairing-ssh-verify.ts#L190-L240).
- Used correctly, `Origin` is a cross-site check on top of a credential: Codex returns 403 on a bad `Origin` at upgrade after checking a token (https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/websocket.rs#L99-L130), and DeepSeek harness fences Host and Origin after a launch token.

**6. Repository files that widen the user's policy.**
- Oh My Pi: project settings set the approval mode (https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/config/settings.ts#L2800-L2814); project `.claude/hooks/` are imported as executable code (https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/discovery/claude.ts#L377-L396).
- goose: project plugins and hooks auto-enable (https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/plugins/discovery.rs#L123-L152).
- OpenCode: project plugins load in process with no prompt (https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/plugin/source.ts#L125-L136).
- OpenHands: repository `hooks.json` (https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/hooks-service.ts#L17-L58).
- Codex: a trusted hook's repository script can change under it (https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/hooks/src/engine/discovery.rs#L972-L998), and repository skills load from untrusted projects (https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/config/src/state.rs#L742-L757).
- Pi: context files load regardless of trust (https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/resource-loader.ts#L679-L685).

**7. Unattended means "approve".** Hermes Agent's `-z` and library context, Cline's cron and any unknown mode, goose's scheduled jobs forced to `Auto`, and Warp's run-to-completion all turn the absence of a human into permission (see 4.8 in `round2/synth/02-approvals-permission-rules.md`). The opposite choice exists in the same set: Hermes Agent's cron, `-q`, API and webhook runs deny, Oh My Pi's headless runs throw when a prompt is needed, OpenCode's headless `run` rejects, and Warp's headless "would ask" becomes a denial.

For our design these patterns read as one rule: the node's local policy decides, nothing the model, a repository, a server or a caller says about itself can lower it, and every guard in the path fails closed.

## 8. Extensibility and integration mechanisms

This section covers group 10 (`round2/synth/10-extensibility-unattended.md`): MCP clients, how tool lists are kept out of the context, code mode, skills, plugins and hooks, scheduled and unattended runs, and the client protocols other programs use to drive a harness. Every one of these is a way for code or text the user did not write to reach the agent, so the questions throughout are the same: who may add it, what environment it gets, and whether its calls pass through the same approval path as everything else. Trust labels from the input are carried; group 10 has more `unchecked` rows than the safety groups, and they are marked.

### Comparison table

Cells condense sections 3.1 to 3.7 of `round2/synth/10-extensibility-unattended.md`; permalinks are in the text that follows.

| Project | MCP transports | Environment of a stdio server | Server annotations | Tool list out of context | Code-mode inner calls approved? | Repository plugins and hooks | Hook failure | Unattended default |
|---|---|---|---|---|---|---|---|---|
| Codex | stdio (also on a remote executor), Streamable HTTP | not stated | not stated | BM25 `tool_search`, per model | not traced | gated by project trust; declaration hashed, script not | open | `exec`: approvals rejected |
| OpenCode | stdio, Streamable HTTP | not stated (shell gets all of `process.env`) | not used; every call asserts `<server>_<tool>` | Code Mode catalog with a 2,000-token budget | yes | in-process, no trust prompt | `permission.evaluate` can loosen | `Unattended` ruleset denies unless allowed |
| OpenClaw | stdio, SSE, Streamable HTTP | not read | not read | tool search, on by default | none found | not read | closed | standing grant bound to the job, else prompt |
| Hermes Agent | stdio, HTTP | allow-list of eight names, `XDG_*`, configured env | missing annotations treated as write-capable; description scan only logs | three bridge tools | `execute_code` whole-script approval on some surfaces only | in-process plugins; shell hooks need consent | plugin hooks open | `cron_mode: deny` |
| Oh My Pi | stdio, HTTP, SSE | full inheritance | ignored; every MCP tool is `write` tier | `xd://` devices | none | hooks and tools imported from `.claude/` and `.codex`; trust hard-coded open | closed | `yolo` |
| Pi | stdio, Streamable HTTP (SSE refused) | full inheritance | not stated | exposure modes: `direct`, `deferred`, `codemode`, `hidden` | yes | path-keyed trust | `tool_call` closed | no scheduler, no permission system |
| DeepSeek harness | stdio, Streamable HTTP | scrubbed by name | not applicable: MCP never asks | none | not stated | installed plugins in the host process | not read | depends on the preset |
| goose | stdio, built-in, platform, Streamable HTTP | inherited, configured names filtered | `readOnlyHint` auto-allows | names only plus an enable call, off by default | no: inner calls skip approval, inspectors and hooks | `.agents/plugins` auto-enabled | open unless `on_failure: block` | `Auto` |
| Cline | stdio, SSE (when `type` is omitted), Streamable HTTP | full inheritance | no per-tool `autoApprove` | none | no code mode | workspace hooks and plugins, no gate | open | `yolo` |
| OpenHands (client) | configured only; client in the SDK | not in this repository | not in this repository | not visible | not applicable | `.openhands/hooks.json`, no trust prompt | load failure drops all hooks | not in this repository |
| Zed | not read | ACP agents: full inheritance | none for external agents | not read | not applicable | not applicable | not read | not applicable |
| Warp (client) | stdio, Streamable HTTP, SSE | full inheritance | allow and deny lists by server | none in the client | not applicable | project MCP never auto-starts | no hooks | headless asks become denials; ACP allows all but the denylist |
| OpenCodex | no MCP client | not applicable | not applicable | not applicable | not applicable | not applicable | no hooks | Remote Workspace `approvalPolicy: "never"` with a fixed capability set |

### MCP clients

**Transports and limits.** Codex uses `rmcp` for stdio and Streamable HTTP, with an 8 MiB stdio line cap, 30 s start-up and 300 s per call (`unchecked`): https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/config/src/mcp_types.rs#L658-L690 and https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rmcp-client/src/bounded_stdio_transport.rs#L24-L26. A Codex stdio server can run on a remote executor chosen by `environment_id`: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rmcp-client/src/executor_process_transport.rs#L1-L20. OpenCode spawns stdio servers through the location's `Environment`, so they run wherever execution runs: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/mcp/stdio.ts#L10-L16. Pi rejects legacy SSE: https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/mcp/src/config.ts#L124-L125. Cline treats an omitted `type` as SSE, with no fallback: https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/mcp/config-loader.ts#L172-L206. Oh My Pi advertises only `roots` and refuses sampling and elicitation: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/client.ts#L96-L112.

**OAuth.** Codex runs its callback listener on `127.0.0.1` of the machine running Codex, so a browser on another machine cannot reach it, and falls back from the keyring to a file any process of the same user can read (`unchecked`): https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rmcp-client/src/perform_oauth_login.rs#L379-L410 and https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rmcp-client/src/oauth.rs#L1-L17. OpenClaw can use the operator's or the requester's identity for an HTTP server (`unchecked`): https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/mcp-transport.ts#L82-L200. OpenCode's optional connection pool is keyed on URL, header fingerprint, OAuth config and credential: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/mcp/pool.ts#L55-L79. DeepSeek harness has no MCP OAuth at all (absence confirmed): https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/mcp/mcp-client/src/tools.ts#L126-L195.

**The environment given to a stdio server.** Three approaches (see 4.1 in `round2/synth/10-extensibility-unattended.md`):
- *Allow-list.* Hermes Agent starts a server with `PATH, HOME, USER, LANG, LC_ALL, TERM, SHELL, TMPDIR`, `XDG_*` and the server's own configured env: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/mcp_tool_config.py#L199-L207. DeepSeek harness drops credential-shaped names: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/mcp/mcp-client/src/transport.ts#L15-L23.
- *Inherit and filter what the config adds.* goose refuses 31 dangerous names (`LD_PRELOAD`, `NODE_OPTIONS`, `PYTHONPATH`) in the configured env, but the inherited provider keys still reach the child: https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/extension.rs#L91-L122.
- *Full inheritance.* Oh My Pi (`{...Bun.env, ...config.env}`): https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/transports/stdio.ts#L593-L596. Pi, whose library has an `inheritEnv === false` switch the coding agent never sets: https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/mcp/src/transports/stdio.ts#L98-L110. Cline: https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/mcp/client.ts#L457-L463. Warp: https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/mcp/src/runtime.rs#L235-L256. Zed, for external ACP agents (see 3.1 in `round2/synth/10-extensibility-unattended.md`). In all of these a provider key in an environment variable reaches every MCP server.

**Who may start a server.** No project makes the node's local policy the only place that can add a stdio server (see 4.2 in `round2/synth/10-extensibility-unattended.md`). Warp comes nearest: project-scoped MCP files never auto-spawn, global ones do: https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/mcp/file_based_manager.rs#L396-L416. Oh My Pi enables project MCP config by default: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/config.ts#L131-L134. In DeepSeek harness any holder of the owner cookie can add a stdio server through the settings RPC, which makes the cookie a code-execution credential (see 4.2 in `round2/synth/10-extensibility-unattended.md`). goose's deeplink allow-list is a prefix match in the desktop installer only: https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/ui/desktop/src/components/ExtensionInstallModal.tsx#L177-L204, and its OSV malware check fails open: https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/extension_malware_check.rs#L422-L440.

**Trusting what a server says about itself.** goose lets a server's `readOnlyHint` grant auto-approval in SmartApprove: https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/permission/permission_inspector.rs#L172-L176. Oh My Pi ignores annotations and puts every MCP tool in the `write` tier, which auto-approves in `write` mode: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/tool-bridge.ts#L733-L735. Hermes Agent treats a cached row with missing annotations as write-capable, and its tool-description injection scan has its result discarded, so it only logs: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/mcp_tool_registration.py#L288-L300. OpenCode asserts a permission `<server>_<tool>` on every call regardless of annotations: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/mcp.ts#L36-L68. DeepSeek harness has no approval hook in its MCP code, so MCP calls never ask: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/mcp/mcp-client/src/tools.ts#L126-L195. Cline's documented per-server `autoApprove` lists do not exist in the SDK schema: https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/mcp/config-loader.ts#L46-L56.

### Keeping tool lists out of the context

Seven designs appear (see 4.4 in `round2/synth/10-extensibility-unattended.md`):

| Design | Project | How it works | Permalink |
|---|---|---|---|
| Search tool, switched per model | Codex | MCP tools registered `Deferred` when the model's catalog row has `supports_search_tool`; BM25 index, 8 results (`unchecked`) | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/mcp_tool_exposure.rs#L136-L170 |
| Search tool, always on | OpenClaw | `enabled: setting ?? true`, 8 results, 20 maximum (`unchecked`) | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/tool-search-config.ts#L5-L25 |
| Search bridge | Hermes Agent | `tool_search`, `tool_describe`, `tool_call` | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/tool_search.py#L1-L8 |
| Per-server exposure property | Pi | `direct`, `deferred`, `codemode` (default), `hidden` | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/extensions/mcp/tools.ts#L46-L48 |
| Code mode as the default surface | OpenCode | one `execute` tool plus a catalog filled round-robin per namespace up to 2,000 tokens, then `search()` inside the script | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/codemode/catalog.ts#L50-L57 |
| Names only plus an enable call | goose | off by default; names derived from the conversation; off under Code Mode | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/state_machine/ops_toolcalling.rs#L344-L352 |
| Unmounting behind a device | Oh My Pi | `xd://` devices driven through `read` and `write`, default on | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/settings.ts#L1014-L1040 |

DeepSeek harness, Cline and Warp have no deferral and send every schema every step; Warp copies every connected server's tools into each request (`unchecked`): https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/agent/api.rs#L267-L345.

How the change is recorded matters as much as the mechanism, because a tool loadout that is not in the log cannot be reproduced on resume or fork, and a loadout edited into the head of the prompt breaks the prompt cache. Codex counts `tools/list_changed` as a revision "in transport order" (`unchecked`): https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rmcp-client/src/tool_list_changes.rs#L1-L40. OpenCode publishes a durable `session.instructions.updated` event holding frozen rendered text, and appends later changes as `system` messages (`unchecked`): https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/instruction-state.ts#L49-L89. Pi records activation as a system-message delta with `toolsAdded` and `toolsRemoved`, restored on resume, `/tree` and fork (see 3.2 in `round2/synth/10-extensibility-unattended.md`). goose derives the enabled set by folding the conversation. Hermes Agent rebuilds the catalog statelessly at every assembly.

### Code mode and the approval path

Isolation is language-level everywhere it exists; whatever tools the script holds run with full host authority (see 4.5 in `round2/synth/10-extensibility-unattended.md`). What separates the designs is whether an inner call is a normal tool call.

- **Re-entrant pipeline (correct).** OpenCode runs each nested call through the same `run`, with its own `assert`: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/codemode/tool.ts#L126-L154. Pi re-enters the tool pipeline through `runToolCall`, and `tool_call` hooks see nested calls: https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/agent-session.ts#L719-L742.
- **Bypass.** goose's inner `tools.call` reaches `lease.call` directly, so one approval of `execute_javascript` covers every shell and MCP call it makes, with no inspector or `PreToolUse` hook: https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/code_execution.rs#L172-L194.
- **Whole-script approval.** Hermes Agent's `execute_code` is approved as a whole on gateway and ask surfaces only, and runs a Python kernel on the host: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/code_execution_tool.py#L787-L802.
- **Not settled.** Codex's bundled catalog sets `code_mode_only` for 11 of 12 models although the feature flag is off; its isolation and whether inner calls pass approval were not traced (`unchecked`): https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/tools/mod.rs#L66-L76.

Two further holes in the good designs. OpenCode's guest `fetch` re-checks each redirect hop against deny rules but has no loopback or private-range guard and no response cap: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/codemode/fetch.ts#L17-L56; and denying `execute` makes its tools direct instead of removing them: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool.ts#L265-L271. Pi lets the script set its own wall-clock deadline, infinite by default: https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/extensions/codemode/execute.ts#L555.

### Skills

All projects that load skills use the same shape: a short catalog in context, the body fetched on demand. Nothing in any skills loader executes at load time (see 4.6 in `round2/synth/10-extensibility-unattended.md`). The differences:

- **Budget.** OpenClaw caps the catalog at 150 skills and 18,000 characters and degrades to shorter descriptions rather than dropping skills: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/skills/loading/skill-prompt-limits.ts#L10-L40. Hermes Agent cuts descriptions to 60 characters. OpenCode and Pi have no budget.
- **Gate for repository skills.** Pi gates project skills by its path-keyed trust: https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/project-trust.ts#L47-L97. Codex walks `.agents/skills` from the cwd to the project root: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/ext/skills/src/host_roots.rs#L192-L260, and its layer iteration includes disabled (untrusted) layers, so an untrusted repository's skills are still discovered (absence of a check confirmed): https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/config/src/state.rs#L742-L757. OpenCode has no trust prompt for project skills, and remote skill sources are trusted by URL origin with no content hash (`unchecked`): https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/skill/discovery.ts#L83-L135. A hostile repository skill is therefore prompt injection in the first request.
- **Who may write skills.** OpenClaw's Skill Workshop is enabled by default and saves skills after a background review with no human step, versioned and undoable after the fact: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/extensions/skill-workshop/openclaw.plugin.json#L6-L24.

### Plugins, extensions and hooks

**Where they load from and whether loading is gated.** Five stances (see 4.7 in `round2/synth/10-extensibility-unattended.md`):
- *None.* Oh My Pi imports TypeScript and JavaScript hooks and tools from the project's `.claude/hooks/`, `.codex` and plugin directories as executable modules, behind a trust check hard-coded true: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/discovery/claude.ts#L377-L396 and https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/runner.ts#L1814-L1814. OpenCode dynamically imports plugins from the project's `.opencode`, `.claude` and `.agents` into the server process: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/module.ts#L168-L184. goose auto-enables `.agents/plugins`: https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/plugins/discovery.rs#L123-L152. Cline loads workspace hooks and plugins with no trust code (see 3.5 in `round2/synth/10-extensibility-unattended.md`). OpenHands attaches `.openhands/hooks.json`: https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/hooks-service.ts#L17-L58.
- *Path boolean.* Pi: a later `git pull` that adds `.pi/extensions/` to a trusted folder loads without a prompt: https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/trust-manager.ts#L193-L215.
- *Content hash of the declaration.* Codex hashes `{event, matcher, handler}`, reads trust state only from the user and session-flag layers, and re-verifies a SHA-256 of a plugin's tree before each run: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/hooks/src/engine/discovery.rs#L981-L1027, https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/hooks/src/config_rules.rs#L10-L30 and https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/hooks/src/engine/plugin_trust.rs#L28-L50. For a non-plugin hook such as `command = "./scripts/check.sh"` only the command string is hashed, so the repository can change the script under an approved hook; hooks from remote workspace plugins are trusted automatically: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server/src/effective_plugin_change.rs#L80-L102.
- *Install-time scanner.* Hermes Agent's regex scanner is evidence for review, not a boundary; plugins run in process by default: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/config_defaults.py#L1715.
- *Home-only discovery.* Cline's Agent Plugins lane and Warp's project-scoped servers.

In-process plugin code (OpenCode, Pi, Oh My Pi, Cline worker mode, Hermes Agent default) has the server's rights and credentials. Cline's `CLINE_PLUGIN_MODE=sandbox` is a child process with a copy of the environment and no restriction: https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/runtime/tools/subprocess-sandbox.ts#L308-L327. Distribution is weak in places: Pi installs npm or git packages with scripts enabled: https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/package-manager.ts#L2026-L2048, and Oh My Pi's marketplace `auto` setting upgrades unprompted: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/plugins/marketplace-auto-update.ts#L36-L48. Codex scrubs `GIT_DIR`, `GIT_CONFIG*` and similar from background Git operations so a hostile project cannot redirect them (plugins `unchecked`): https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core-plugins/src/git_policy.rs#L7-L50.

**How hooks fail and what they may do.** Fail-open is the majority (see 4.8 in `round2/synth/10-extensibility-unattended.md`):
- Codex: an error, timeout, other non-zero exit or invalid JSON marks the hook `Failed` and does not block: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/hooks/src/events/pre_tool_use.rs#L211-L300. A `PermissionRequest` hook's allow approves and also pre-approves the unsandboxed retry; any deny wins: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/hooks/src/events/permission_request.rs#L153-L173.
- goose: fail-open unless a `PreToolUse` hook sets `on_failure: block`: https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/hooks/mod.rs#L185-L191.
- Cline: file hooks allow on every failure, and their only control, `cancel`, stops the whole run: https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hooks/hook-file-hooks.ts#L415-L454.
- Fail-closed: OpenClaw blocks on any throw in the `before_tool_call` chain: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/agent-tools.before-tool-call.policy.ts#L397-L408. Oh My Pi blocks on a throw or a 30 s timeout: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/runner.ts#L2338-L2364. Pi blocks on a throwing `tool_call` handler: https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/extensions/runner.ts#L1256-L1274.
- Capability: Hermes Agent lets a hook only escalate to the human or block, never approve: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/plugins.py#L1198-L1227. Oh My Pi re-resolves the full approval gate against a rewritten input: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/wrapper.ts#L355-L374. OpenCode's `permission.evaluate` can turn ask into allow but never sees configured denies: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/permission.ts#L190-L201.

**Commands outside the gate.** OpenCode runs every `` !`cmd` `` in a custom command template through the shell with no permission call, after argument substitution, so user-typed arguments execute too: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/plugin/command.ts#L190-L215.

### Scheduled and unattended runs

Three authorisation models and a bad default (see 4.9 in `round2/synth/10-extensibility-unattended.md`):
- **Deny by default with an explicit allow set.** OpenCode's `Unattended` ruleset allows an ask only if it allows every resource of the request: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/permission.ts#L59-L65. Headless `run` rejects, or answers `once` with `--auto`: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/cli/src/run/noninteractive.ts#L97-L112. Hermes Agent's cron defaults to `cron_mode: deny`: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/config_defaults.py#L1658-L1662. Codex `exec` rejects approval requests: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec/src/lib.rs#L622-L640.
- **A grant bound to the thing authorised.** OpenClaw mints a standing grant bound to the job, the config revision and the operation, consumed at spawn, falling back to a prompt (`unchecked`): https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/bash-tools.exec-host-gateway.ts#L460-L529.
- **Approve everything.** Cline's cron runner sets `{"*": {autoApprove: true}}` and maps any unknown mode to `yolo`: https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/cron/runner/cron-runner.ts#L84-L108. goose forces scheduled jobs to `Auto`, does not catch up missed runs and allows overlap: https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/scheduler/full.rs#L142-L183. Oh My Pi's headless default is `yolo`. Warp's ACP route allows everything but the denylist and protected paths, preferring `AllowAlways`: https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/agent_sdk/driver/harness/acp/policy.rs#L80-L117.

**Catch-up, overlap and journals.**
- Hermes Agent advances `next_run_at` before dispatch (at-most-once): https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/cron/scheduler_tick.py#L62-L72. It fsyncs a per-fire journal and pauses a job whose outcome is unknown (admission journal `unchecked`; see 3.7 in `round2/synth/10-extensibility-unattended.md`). A cron-spawned agent cannot create jobs, which the code calls loop prevention, not a security boundary: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/cron/scheduler.py#L418-L439.
- OpenCode skips missed "every" slots rather than bursting (`unchecked`): https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/schedule.ts#L151-L156.
- DeepSeek harness catches up latest-only and commits a receipt after a durable flush (`unchecked`): https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/schedule/schedule/src/runtime.ts#L196-L227. Its webhook stores the delivery id and never checks it: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/experimental/webhook/src/index.ts#L126-L160.
- OpenClaw recovers an interrupted run under conditions but with no attempt counter: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/cron/store/run-recovery.kernel.ts#L191-L239. Its delivery queue refuses blind replay of a send that may have started (`unchecked`): https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/outbound/delivery-queue-recovery.ts#L597-L625. Its webhook idempotency cache is a process `Map`, so a retry after restart runs again (`unchecked`): https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/hooks-request-handler.ts#L220-L256.
- Cline's agent-created tasks start `pending_approval`, the only human step on agent-created schedules found: https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/tasks/store/sqlite-task-store.ts#L264-L266.

### Client protocols as an integration surface

Several harnesses can be driven, or can drive others, through a protocol. Each is an approval surface, because whoever answers the protocol's permission request is the approver.

- **ACP as a client of other agents.** Warp implements an ACP client for headless and cloud runs that drives Claude, Codex, Gemini and Pi: https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/agent_sdk/driver/harness/acp/bridge.rs#L1-L22, with the auto-approving policy above. Zed hosts external ACP agents but does not apply its own `tool_permissions` to them: https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent_servers/src/acp/v1.rs#L10172-L10225. goose maps its modes onto ACP providers, and its `auto` skips Claude's own checks: https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/providers/amp_acp.rs#L69-L75.
- **ACP as a server.** goose serves ACP over HTTP and WebSocket at `/acp`: https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-cli/src/cli.rs#L860-L905. Hermes Agent's `hermes acp` is a viewer of the gateway: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/acp_adapter/entry.py#L205-L215, and reports a timeout separately from a denial: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/acp_adapter/permissions.py#L117-L124. Oh My Pi's ACP mode treats its default `yolo` as always-ask for bash: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-tools.ts#L1174-L1217. OpenHands' client has no handling of ACP permission requests: https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/types/agent-server/core/events/acp-tool-call-event.ts#L27-L60.
- **Own RPC.** Codex's app-server routes approvals marked as needing a human only to connections that opted in: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server/src/outgoing_message.rs#L414-L430; its daemon socket is host-local, 0700 directory and 0600 socket: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/unix_socket.rs#L322-L352. Codex as an MCP server was removed: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/cli/src/main.rs#L166-L175. Oh My Pi's RPC is NDJSON with 1 MiB v1 and 64 MiB v2 chunks: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/modes/rpc/rpc-frame.ts#L5-L9. Cline's hub lets any attached client answer an approval (see section 7).

**Importing other harnesses' configuration.** Data imports are harmless when they pass the same gates: goose converts Claude Code, Codex and Pi transcripts: https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/session/import_formats/mod.rs#L150-L157; Cline converts Claude Code, Codex and OpenCode transcripts (`unchecked`): https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/services/session-import/index.ts#L1-L29. Executable imports are not: Oh My Pi reads hooks and custom tools from other harnesses' directories and runs them in process, so cloning a repository is code execution before any approval (link above).

### Worth copying

| Mechanism | Project | Why | Permalink |
|---|---|---|---|
| Allow-listed environment for stdio MCP servers | Hermes Agent | a provider key in the harness environment does not reach the server | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/mcp_tool_config.py#L199-L207 |
| Every MCP call asserts a `<server>_<tool>` permission | OpenCode | annotations never decide | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/mcp.ts#L36-L68 |
| Connection pool keyed on the credential | OpenCode | a shared connection never mixes credentials | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/mcp/pool.ts#L55-L79 |
| Refusing sampling and elicitation | Oh My Pi | a server cannot drive the model | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/client.ts#L96-L112 |
| Project-scoped MCP files never auto-spawn | Warp | starting a program is a recorded decision | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/mcp/file_based_manager.rs#L396-L416 |
| Local BM25 tool search | Codex | no extra model call (`unchecked`) | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/mcp_tool_exposure.rs#L136-L170 |
| Tool-list changes as durable, frozen, appended events | OpenCode, Pi | reproducible on resume and fork; prompt cache survives (`unchecked` for OpenCode) | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/instruction-state.ts#L49-L89 |
| Code-mode inner calls through the normal pipeline | Pi, OpenCode | code mode cannot defeat the approval path | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/agent-session.ts#L719-L742 |
| Skill catalog capped by count and characters, degrading not dropping | OpenClaw | bounded context cost | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/skills/loading/skill-prompt-limits.ts#L10-L40 |
| Hook trust as a hash held where the repository cannot write, re-verified at run time | Codex | a repository cannot approve its own hook | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/hooks/src/config_rules.rs#L10-L30 |
| Hooks may escalate or block, never approve | Hermes Agent | a hook cannot widen policy | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/plugins.py#L1198-L1227 |
| Approval re-resolved after a hook rewrites the call | Oh My Pi | a rewrite cannot slip past a prompt | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/wrapper.ts#L355-L374 |
| Hooks fail closed | OpenClaw | a broken guard stops the call | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/agent-tools.before-tool-call.policy.ts#L397-L408 |
| Git environment scrubbed for background Git | Codex | a hostile checkout cannot redirect Git (`unchecked`) | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core-plugins/src/git_policy.rs#L7-L50 |
| Unattended ruleset: allow only what is listed | OpenCode | no human means deny | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/permission.ts#L59-L65 |
| Standing grant bound to job, revision and operation | OpenClaw | an unattended approval covers only what was approved (`unchecked`) | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/bash-tools.exec-host-gateway.ts#L460-L529 |
| Advance the schedule before dispatch | Hermes Agent | a crash cannot fire a job twice | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/cron/scheduler_tick.py#L62-L72 |
| Agent-created tasks start `pending_approval` | Cline | the agent cannot schedule itself | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/tasks/store/sqlite-task-store.ts#L264-L266 |
| Refuse blind replay of a send that may have started | OpenClaw | no duplicate external side effect (`unchecked`) | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/outbound/delivery-queue-recovery.ts#L597-L625 |

### To avoid

| Mechanism | Project | Why | Permalink |
|---|---|---|---|
| Full environment inheritance for MCP children | Oh My Pi, Pi, Cline, Warp | every server gets every key | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/transports/stdio.ts#L593-L596 |
| A server's `readOnlyHint` granting approval | goose | a hostile server mislabels a destructive tool | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/permission/permission_inspector.rs#L172-L176 |
| A scanner whose result is discarded | Hermes Agent | looks like a control, is not one | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/mcp_tool_registration.py#L288-L300 |
| Prefix or name allow-lists for servers | goose | a command string is not an executable identity | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/ui/desktop/src/components/ExtensionInstallModal.tsx#L177-L204 |
| Code-mode inner calls outside approval | goose | one approval covers every call the script makes | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/code_execution.rs#L172-L194 |
| Denying the code tool exposes its tools | OpenCode | the denial widens the surface | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool.ts#L265-L271 |
| A script-set deadline | Pi | the script decides how long it runs | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/extensions/codemode/execute.ts#L555 |
| Repository skills loaded from untrusted projects | Codex | prompt injection in the first request | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/config/src/state.rs#L742-L757 |
| Agent-written skills with no human step | OpenClaw | the agent edits its own instructions | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/extensions/skill-workshop/openclaw.plugin.json#L6-L24 |
| Executable hooks imported from other harnesses' directories | Oh My Pi | cloning a repository runs code | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/discovery/claude.ts#L377-L396 |
| Auto-enabling discovered plugins | goose | trust must be a recorded decision | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/plugins/discovery.rs#L123-L152 |
| In-process plugins | OpenCode | they hold every credential the process holds | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/module.ts#L168-L184 |
| A path-keyed trust boolean | Pi | a later pull changes what it covers | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/trust-manager.ts#L193-L215 |
| Hashing the hook declaration but not the script | Codex | the script can change under the approval | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/hooks/src/engine/discovery.rs#L972-L998 |
| Fail-open hooks in the approval path | Codex, goose, Cline | a broken guard silently stops enforcing | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/hooks/src/events/pre_tool_use.rs#L211-L300 |
| Command templates run outside the permission flow | OpenCode | user and model text becomes shell | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/plugin/command.ts#L190-L215 |
| "sandbox" naming for crash isolation | Cline | the name promises a property it lacks | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/runtime/tools/subprocess-sandbox.ts#L308-L327 |
| Unattended mode equal to auto-approve | Cline, goose | no human means "yes" | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/cron/runner/cron-runner.ts#L84-L108 |
| Idempotency state in memory, or an id stored and never checked | OpenClaw, DeepSeek harness | a retry after restart runs twice | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/experimental/webhook/src/index.ts#L126-L160 |
| An ACP client that auto-approves the agents it drives | Warp | the foreign agent's own gate is removed | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/agent_sdk/driver/harness/acp/policy.rs#L80-L117 |

## 9. Interesting implementation decisions and trade-offs

Each entry states a choice one project made, what it gave up, the evidence, and the trust label the synthesis attaches to it. "Label not stated" means neither `patterns.md` nor the passage I checked in the group file gives a review label; it does not mean the claim was reviewed. Most entries set two projects that chose opposite sides against each other. The fuller tables are in section 4 of each group file; `patterns.md` is the extract this section was built from.

### Who owns a session, and what a crash leaves behind

**9.1 Fence the writer, or assume the old owner is dead.** OpenClaw compares the active writer's run id inside the write transaction (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/config/sessions/session-accessor.sqlite-scope.ts#L79-L97, corrected in review: the check is skipped when a write carries no expected writer). OpenCode has no lease or fence; its restart sweep assumes every orphaned claim's owner is dead, which only its managed-server protocol guarantees (https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/execution/restart.ts#L76-L99, checked in review). OpenClaw pays for an epoch, a per-dispatch credential and a check inside every write. OpenCode pays nothing until two server processes share one database.

**9.2 Fence the settle, on one host.** Hermes Agent bumps a per-session generation when it claims input and accepts a settle only with a matching epoch and generation; at start every `started` row of an earlier epoch becomes `unknown` and is never requeued (https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_runtime.py#L206-L242, https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_runtime.py#L453-L464, label not stated). It gains a clean classification of lost turns. It gives up anything across hosts: it fences settlement, not a remote writer. Codex and DeepSeek harness use file locks with no expiry, which protect one filesystem only (4.3 in `round2/synth/05-links-workers-protocols.md`).

**9.3 After a crash: close as "unknown", re-drive, or replay by policy.** DeepSeek harness closes the open turn with synthetic results that say the outcome is unknown and must not be retried blindly (https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/session/src/repair.ts#L31-L41, checked in review). OpenCode re-drives the turn with "continue without repeating completed work" and gives up after 10 attempts (same `restart.ts` link above): at-least-once, so a started shell command can run again. pi-durable commits a checkpoint with `replay: safe|unsafe` before the tool runs and re-runs only safe tools (https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/durable/src/harness/tool.ts#L201-L227, checked in review), at the cost of every tool declaring a policy; it is experimental and not wired into the shipped CLI.

**9.4 The log is the truth, or the log is optional.** DeepSeek harness, Codex, Pi, Oh My Pi and OpenClaw fold an append-only log. OpenCode has a durable event log but `persist` defaults to false and the shipped CLI server never sets it (https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/bus.ts#L186-L192, checked in review), so its destructive revert has no recovery source. The log costs unbounded growth: only OpenClaw bounds disk by default (4.1 in `round2/synth/06-sessions-fork-checkpoints.md`).

**9.5 Input during a turn: reject, queue in memory, or admit durably.** goose, Cline and our `main` reject a prompt while a turn runs: nothing is lost, the user waits. Hermes Agent commits each inbound message as a `session_admissions` row with a unique key before acting, so a retried delivery is an exact replay (4.5 in `round2/synth/01-loop-dispatch-durability.md`, label not stated). Pi and OpenClaw's in-flight steering list hold queued input in memory, which protects against a failed run but not a process crash (corrected in review for OpenClaw).

### Approvals and policy

**9.6 What an answer is bound to.** Cline's hub lets any authenticated client answer by approval id (https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/handlers/approval-handlers.ts#L123-L161, checked in review). OpenClaw binds a one-off approval to argv, working directory, agent, session, node, requester device and a hash of environment overrides, re-checks for drift before launch and consumes it once (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/system-run-approval-binding.ts#L72-L90; a small window between hashing and exec remains, checked in review). The id is cheap and lets a stale or forged answer run the wrong action. The binding costs canonicalisation code on both ends.

**9.7 How wide "always" is.** Zed writes a prefix regex to user settings with no expiry (https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/pattern_extraction.rs#L53-L95, checked in review). OpenClaw's "allow always" hashes the working directory and the arguments after the first; the executable is matched by a path pattern and hashed only if the user can modify it (corrected in review; 4.3 in `round2/synth/02-approvals-permission-rules.md`). Codex adds a side effect neither has: a remembered `allow` rule also runs the command outside the sandbox (https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/exec_policy.rs#L492-L512, checked in review). Wide grants cost fewer prompts and leak authority across projects; narrow grants cost more prompts.

**9.8 Precedence: a lattice or an ordered list.** Codex takes the maximum decision over all matches, so a deny anywhere wins regardless of order (https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/execpolicy/src/policy.rs#L577-L595, confirmed). OpenCode's last match wins (https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/permission.ts#L94-L104, confirmed), so a later, wider layer silently overrides an earlier one, and OpenCode needs a separate administrator pass to restore a ceiling (https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/plugin/policy.ts#L98-L113).

**9.9 A parse error in the rules: drop them, or refuse.** Codex loads its rules with a warning on failure, and the failure drops every user and project rule (https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/exec_policy.rs#L729-L744, confirmed). That keeps the program usable and silently removes the deny rules a user wrote. On shell parse failure the same project chose the opposite: an unanalysable command is one opaque argv and is not auto-allowed (4.2 in `round2/synth/02-approvals-permission-rules.md`).

**9.10 Hooks fail closed, or fail open.** Oh My Pi blocks the call when a hook throws, times out or aborts, and re-resolves approval on the rewritten input (https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/runner.ts#L2338-L2364). Codex's `PreToolUse` hooks can deny but fail open on error (https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/hooks/src/events/pre_tool_use.rs#L211-L300, checked in review). Fail-open keeps a broken hook from stopping work and turns a crashing policy hook into no policy.

**9.11 Trust a hook by its declaration, its path, or nothing.** Codex hashes the hook declaration and the plugin bundle and stores trust where the repository cannot write, but does not hash the repository script the hook runs (https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/hooks/src/engine/discovery.rs#L972-L998, corrected). Pi keys trust on a path boolean, so a later `git pull` that adds code to a trusted folder loads without a prompt (4.7 in `round2/synth/10-extensibility-unattended.md`). Oh My Pi, OpenCode, goose, Cline and OpenHands have no trust step. None binds trust to the content of everything that will execute.

**9.12 The model rates its own risk, or asks for more.** Warp skips both the allow-list and the redirection check when the model's `is_risky` is `Some(false)` (https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/blocklist/permissions.rs#L972-L1008, checked in review). Zed lets the model request wider rights with a reason, and the request widens nothing until the normal check and a person approve it (https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/tools/terminal_tool.rs#L678-L705, checked in review). The first saves prompts and hands the gate to a prompt-injected model; the second costs one prompt per escalation.

**9.13 Unattended means deny, or means approve.** Hermes Agent defaults `cron_mode` to `deny` and withholds the scheduling toolset from cron agents (https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/config_defaults.py#L1636-L1662). goose runs every scheduled job in `Auto` whatever the user configured (https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/scheduler/full.rs#L881-L907, checked in review), and Cline's cron runs, plus any unknown mode, become yolo (https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/cron/runner/cron-runner.ts#L84-L108, checked in review). Deny costs failed routines that a person must widen; approve costs whatever the job's prompt can be talked into.

### Sandboxes and containers

**9.14 Namespaces plus seccomp, or Landlock alone.** Codex builds the filesystem view with bubblewrap and adds a seccomp denylist that, with network off, blocks `connect`, `bind`, non-`AF_UNIX` sockets, `ptrace` and all `io_uring` calls (https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/seccomp.rs#L183-L301). It is the only design in the set that blocks UDP and Unix-socket connects with network off; the reviewer found `sendmsg` and `recvmsg` in no deny list, and the filter supports x86_64 and aarch64 only. It refuses legacy Landlock with a panic that says bubblewrap is required "to isolate app-server sockets" (https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/linux_run_main.rs#L386-L393, corrected in review: that Landlock cannot block `connect()` is the author's inference). DeepSeek harness's Landlock launcher needs no helper binary but sets no network or scope rights and grants read everywhere, so it confines writes only (https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/native/system/packages/entry/src/main.c#L1-L36, checked in review).

**9.15 Who controls the container flags.** OpenClaw validates every bind source on the real path of its nearest existing ancestor and rejects host networking and unconfined seccomp or AppArmor (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/validate-sandbox-security.ts#L23-L53; that a host-exec agent could still edit the config is `inferred`). Hermes Agent appends operator `docker_extra_args` last, so a `-v /:/host` overrides everything (https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/docker.py#L741-L745, label not stated), and its host-access predicate ignores that mount (checked in review). Pass-through arguments buy flexibility and make the harness's own checks advisory.

**9.16 Relax checks inside the container, or keep them.** Hermes Agent skips its hardline floor, the sudo guard and dangerous-pattern prompts on container backends (https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval.py#L1161-L1175, checked in review). That saves prompts on the assumption that the container is the boundary. A container with network can still push to a remote or exfiltrate, which the assumption ignores.

**9.17 Where the provider key lives.** Seven projects keep the key in the agent's own process and environment (OpenCode, Oh My Pi, Pi, goose, Cline, Codex by default, OpenClaw host exec), where every shell command can read it. OpenClaw keeps it on the Gateway and brokers inference by model reference, refusing an unapproved model or a `@profile` suffix (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/inference-runtime.ts#L122-L153). The control-side position is the only one where a compromised worker cannot spend a key except through the controller's rules, and it costs a bounded data path on the controller (4.1 in `round2/synth/04-credentials-secrets-auth.md`).

### Checkpoints, history and fork

**9.18 Full-tree snapshot, write ledger, or in-memory baseline.** OpenCode snapshots into a shadow store seeded from the user's index, which covers shell-made changes and still costs a scan of the tree per capture (https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/git.ts#L503-L520, checked in review; its cost guards `unchecked`). Hermes Agent's safe restore uses a ledger fed only by file-tool writes, so a shell-changed file is skipped by the default restore (https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/checkpoint_manager.py#L986-L1050, corrected in review; exceptions: a file written by a file tool then deleted by shell is restored, an empty ledger means a full restore, and container writes are not recorded at all). Zed keeps a per-buffer baseline at first agent touch in memory (https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/action_log/src/action_log.rs#L168-L192, checked in review): cost proportional to files touched, blind to the shell, lost on restart. Our checkpoint is the first shape.

**9.19 Checkpoints in the user's repository, or beside it.** Cline builds stash-shaped commits in the user's own repository under `refs/cline/checkpoints/...`, with a private index that persists so git's stat cache skips unchanged files (https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hooks/checkpoint-hooks.ts#L535-L649, checked in review). It is cheap, needs no second repository, and pollutes the user's object database with no size or count cap. A shadow repository (OpenCode, Hermes Agent, ours) leaves the user's refs and config alone and pays a second store.

**9.20 Fork as a pointer, a copy, or a branch in the same file.** DeepSeek harness copies an exact prefix into the child's own log, closes any open turn with `forked` closers and marks the inherited count (https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/session/src/fork.ts#L21-L30, checked in review): cost proportional to the prefix, and the parent can be deleted. Pi branches inside one file by moving a leaf that it never stores; the leaf after load is the last physical entry, so a move with no following write is lost on restart (https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/session-manager.ts#L1104-L1123; the restart consequence is `inferred`, the reviewer agreed). Codex uses a pointer over an immutable file and pins the parent only during prepare (4.3 in `round2/synth/06-sessions-fork-checkpoints.md`).

**9.21 Record the active history, or infer it.** Codex is the only project that records the active history explicitly: a revert writes a new file and swaps the pointer by compare-and-set (https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/thread-store/src/local/revert_thread.rs#L138-L156, checked in review). Pi and Oh My Pi infer it from the last entry, which costs nothing and loses navigation (9.20). Nobody writes a "head moved" event.

### Context and compaction

**9.22 A summary request that keeps the cache, or one that does not.** OpenCode sends the primary agent's own system parts, tools and history plus one final user message, and says in a comment that this is for cache reuse (https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L451-L470, checked in review). Pi sends its summary with `cacheRetention: "none"` and a serialised transcript (https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/compaction/compaction.ts#L618-L638; that this re-bills the full history uncached is `inferred`). The cache-preserving form is the largest request the session makes, so the trigger must leave room for it. No finding measures the hit rate.

**9.23 When to compact.** Hermes Agent triggers at 0.50 of the window (raised to 0.75 for windows under 512K) and Codex at 0.9 (4.2 in `round2/synth/07-context-compaction-memory.md`). The two numbers are nearly a factor of two apart and no project shows a measurement that favours one. An early trigger costs summaries and lost detail; a late one costs overflow recovery.

**9.24 After a failed summary: fall back without a model, or end the turn.** Cline's `basic` compaction keeps every typed user prompt, collapses dropped tool work into a notice and keeps the last three assistant texts, with no model call (https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/context/basic-compaction.ts#L452-L512, corrected in review); during overflow it never uses its agentic summariser, whose own request could overflow. Codex's sampling path marks usage full and ends the turn (4.6 in `round2/synth/07-context-compaction-memory.md`). A deterministic tier is crude and always succeeds.

### Edit tools

**9.25 A seen-line guard, fuzzy matching, or neither.** Oh My Pi's hashline edits anchor on a per-line tag and refuse lines the model never saw, with a 565-line rebase path for stale tags (https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/modes/hashline/recovery.rs#L503-L565); its authors disable it for eight model families, and its tag is 16 bits (a live collision bypassing recovery is `inferred`). Cline's `apply_patch` accepts a similarity score of 0.66 as its last rung and reports the fuzz level (https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/tools/executors/apply-patch-parser.ts#L347-L430, checked in review). Zed accepts 0.8 per line (https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/tools/edit_session/streaming_fuzzy_matcher.rs#L254-L256, corrected in review). The guard costs a snapshot store and model fit; fuzzy matching costs wrong-place edits on repetitive code.

**9.26 Normalise, then rewrite, or normalise, then splice.** Pi and OpenClaw both fold text with NFKC to match. Pi works in normalised space and widens each replacement to whole lines, silently rewriting touched lines to their normalised form and not saying fuzzy matching was used (https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/tools/edit-diff.ts#L176-L214, checked in review). OpenClaw splices the replacement into the original bytes and errors on an ambiguous Unicode boundary (`unchecked`; 4.2 in `round2/synth/08-edit-tools-mechanics.md`). Same fold, different damage.

**9.27 Hard staleness check, or one that ships off.** OpenCodex's `write_file` takes an `expectedSha256` and refuses on mismatch (https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-rpc.ts#L162-L207, checked in review). DeepSeek harness built a compare-and-swap inside the per-path lock and ships the policy disabled, with no bundle re-enabling it (https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/fs/fs-observation-policy/src/index.ts#L78-L103, checked in review). The check costs a read record per session; leaving it off costs edits built from a stale picture of the file.

### Sub-agents, retry and extensions

**9.28 A child's authority: narrow-only, or forced open.** Codex makes a role a patch type whose fields cannot express widening and re-applies the parent's live policy last (https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/agent/child_config.rs#L197-L218, checked in review). Oh My Pi gives every child a `yolo` overlay, keeping only the user's deny rules, so approving the `task` call approves what the child runs (https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/task/executor.ts#L1259-L1278, label not stated). OpenCode sits between: a child gets its agent's rules plus a one-time copy of the parent session's rules made at creation (https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session.ts#L311-L314, corrected).

**9.29 Classify retries by structure, or by message text.** Codex has one function from error to policy that never retries auth, invalid requests, context overflow or quota (https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/protocol/src/error.rs#L445-L511). Pi decides with a roughly 100-pattern regex over the error text, a separate quota regex winning first (4.9 in `round2/synth/09-subagents-models-retry.md`); its provider layer defaults to 0 retries, so `Retry-After` is read nowhere (corrected in review). Text matching covers any provider and breaks when a message changes.

**9.30 A code-mode script's inner calls: through the gate, or around it.** Pi's `runToolCall` is re-entrant, so every call a script makes is a normal tool call with its own checks (https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/agent/src/agent-loop.ts#L596-L670, checked in review). goose's Code Mode reaches `lease.call` directly, so one approval of the script covers every shell and MCP call inside it (https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/code_execution.rs#L172-L194). Hermes Agent starts a stdio MCP server from eight named variables plus `XDG_*` and the server's own `env:` (https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/mcp_tool_config.py#L199-L207, checked in review), while Oh My Pi, Pi, Cline and Warp pass the full environment, so a provider key reaches every server (4.1 in `round2/synth/10-extensibility-unattended.md`). In both pairs the cheap choice moves authority to code nobody approved.

## 10. Potential applications to our project

This is one ranked plan built from section 6 of the ten group files, with the overlap between groups removed. Where two groups made the same recommendation (fencing appears in groups 01, 05 and 06; credential reach in 02, 03, 04 and 05; checkpoint cost in 06 and 08), it appears once, under the group that holds the fullest evidence.

Read it with four caveats:

- Sizes (small, medium, large) are our estimates of the change, not findings.
- `CLAUDE.md` puts a feature freeze in force until Gate 3 of `FIX_PLAN.md`. Part (a) is repairs to what exists. Parts (b) and (c) are mostly new work, and their cost against the plan is not assessed here.
- Security code (auth, the node link, tools, the approval path) gets the full stress test and an abuse-case test written first.
- The synthesis authors did not read the branches of pull requests 4, 6 and 7. Statements about those branches come from the task brief.

The invariants are named in short form: **node decides**, **master runs no tools**, **one writer**, **private chats**, **no secret in event, URL or log**, **loopback default**, **light process**.

### (a) Fix in what is already built

Ranked by the harm a missing fix allows.

**A1. Approvals bound to an action digest, and a narrow "allow again".** `main` and PR 4. Medium. Security code.

- **What.**
  - `ApprovalRequested` gains `action_digest`, computed on the node. It is a SHA-256 over a length-prefixed canonical encoding of: session, node and call id; tool; canonical input; working directory by real path; environment beyond the allow-list; sandbox profile; and a hash of the node policy snapshot.
  - `approval.respond` must echo the digest. The node rejects a mismatch, consumes an approval on first use and asks again if its policy changed between request and answer.
  - The master accepts an answer only from a browser session of the chat's owner, never from the node link.
  - Replace `AllowSession`-by-tool-name with a key on canonical argv, working directory and permission delta. Hold it on the node and write it as an event. It ends with the session, never passes to a child, and is never cached for network access or escalation.
- **Copy from.**
  - OpenClaw's policy-snapshot re-check: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/invoke-system-run.ts#L349-L357
  - OpenClaw's replay guard: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/node-invoke-system-run-approval.ts#L197-L252
  - OpenClaw's executable binding: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/system-run-approval-binding.ts#L366-L458
  - Codex's session key: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/tools/runtimes/unified_exec.rs#L87-L98. This is `unchecked`: re-read it before copying the fields.
- **Invariants.**
  - Node decides: an `Allow` can turn an `ask` into a run, never a `node.toml` deny. OpenClaw's node is the counter-example: a forwarded approval satisfies its own allow-list (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/exec-policy.ts#L60-L117).
  - Private chats: an empty approver list must mean "the owner only". OpenClaw's empty list means anyone in the chat (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/plugin-sdk/approval-auth-helpers.ts#L72-L89).
- **Depends on.** Nothing. The five abuse tests are listed under item 1 of section 6 in `round2/synth/02-approvals-permission-rules.md`.
- **Round 1.** Round 1 already recommended argv binding. Round 2 adds three things: the echoed digest, the owner check and single use. It also shows that OpenClaw's "allow always" is narrower and odder than documented: it hashes the working directory and the arguments after the first, and the executable is hashed only if the user can modify it.

**A2. Fence the node link and every mirrored append.** `main`. Medium. Security code.

- **What.**
  - Add `nodes.link_gen`, bumped when a connection is accepted. The master's reader pump checks it on every inbound frame and drops frames from a superseded connection.
  - Add `sessions.owner_epoch`, drawn from a counter that never goes backwards. It is bumped on archive, delete, node revocation and the close of a fork source, and not on a reconnect of the same node.
  - Check one predicate inside the mirror transaction: owner, epoch, active state and `seq = last_seq + 1`.
  - Handle resends by hash. An equal hash is acknowledged as a duplicate. A different hash is refused as `diverged`, which covers a node restored from an old disk.
  - On a `fenced` reply the node stops the session, kills the running tool and marks the session read-only.
- **Evidence.**
  - Our reconnect cancels the old connection and installs the new one with no generation on its frames (`crates/master/src/nodes.rs:207`).
  - The full design and its tests are in 6.1 of `round2/synth/05-links-workers-protocols.md`.
  - OpenClaw's per-request re-check: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/admission.ts#L188-L218
  - OpenClaw's never-reused epoch: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/store-mutations.ts#L30-L50
  - OpenClaw's terminal `owner-epoch-mismatch`: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/worker/worker-connection-contract.ts#L22-L26
  - OpenClaw's epoch bumps only on attach or from certain states, so we must write our own bump list rather than copy theirs.
- **Invariants.** One writer. This turns the invariant from a convention into a check.
- **Depends on.** PR 4's bounded inbound channel holds up to 8 parsed messages. That queue is exactly where frames from a replaced connection can wait (test D5).
- **Round 1 overturned.** Round 1 named OpenClaw and OpenCodex together as fencing models. OpenCodex's Remote Workspace is "latest connection wins" (`attachConnection`), the same as ours. Only OpenClaw fences.

**A3. Keep every control-plane credential out of the agent's reach, and test it.** PR 4. Small to medium. Security code.

- **What, as tests written first.**
  - A confined `cat` of `node.key`, `$CREDENTIALS_DIRECTORY/node.key`, the data directory and `node.db` fails.
  - Grant keys never appear in `env`, `/proc/self/environ` or `/proc/$PPID/environ` of a shell.
  - `env_pass = ["*"]` and `env_pass = ["*_KEY", "HARNESS_*"]` still pass no provider key and no link credential.
  - Injection variables (`LD_*`, `NODE_OPTIONS`, `BASH_ENV`, `GIT_*` executables) cannot be passed.
  - The node process is made non-dumpable through a safe `prctl` wrapper, for example `rustix`, which needs the dependency check.
  - On the master, the embedded local node's shell cannot read `api-token`, `secrets.toml`, `keys/secrets.key` or `master.db`.
- **Copy from.**
  - Codex's always-strip list: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/protocol/src/shell_environment.rs#L14-L28
  - Hermes Agent's fail-closed passthrough check, added after advisory GHSA-rhgp-j443-p4rf: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/env_passthrough.py#L46-L79
  - OpenClaw's blocked keys: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/host-env-security.ts#L38-L65
  - Codex has process hardening but does not apply it to its main binary (absence confirmed in review): https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/process-hardening/src/lib.rs#L7-L61
- **Why it ranks this high.** The worst failures in group 04 are a harness's own credentials in the agent's reach. OpenHands puts its session key in the agent's environment (https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L975-L985). DeepSeek harness's cookie-signing secret is readable by confined bash (see item 1 of section 6 in `round2/synth/04-credentials-secrets-auth.md`).
- **Invariants.** No secret in event, URL or log. Master runs no tools: the embedded node is the case where the master's own secrets meet a shell.
- **Open.** No code read says whether Landlock alone stops `ptrace` or `/proc/<pid>/mem` of the node process (test D11).

**A4. Replace the admin-equivalent API token with per-user tokens.** `main`. Medium. Security code.

- **What.** Per-user tokens, stored as hashes, scoped and revocable, in place of the one long-lived admin-equivalent token.
- **Evidence.** The same single shared bearer appears in Hermes Agent, goose, OpenHands and OpenCodex (item 4 of section 6 in `round2/synth/04-credentials-secrets-auth.md`).
- **Invariants.** Private chats, "for admins too". Today the token can see every chat, so the invariant is not met on `main`.
- **Depends on.** Nothing. The design note does not cover it.

**A5. Never run tool calls from a response that stopped on the length limit.** `main` (`crates/agent-native`). Small.

- **What.** Fail every tool call in a message whose finish reason is `length`. Fail the step after three consecutive `length` stops, as OpenCode does.
- **Evidence.** Pi fails every such call, because its streamed arguments are completed by a JSON salvage parser (https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/agent/src/agent-loop.ts#L273-L279, https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/agent/src/agent-loop.ts#L488-L513, checked in review). A truncated `write` or `bash` argument loses data.
- **Invariants.** None touched.
- **Test.** A provider fixture that ends a `write` call mid-JSON.

**A6. "Append, flush, then act" as a tested rule.** `main` and PR 4. Small to check, medium if it fails.

- **What.** The event recording "tool call X with arguments A is about to run" must be durable on the node before the tool body starts. A failed append means the tool never runs. Interrupted calls are closed with wording that says the outcome is unknown.
- **Copy from.**
  - DeepSeek harness's awaited, fail-closed flush: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/session/session-checkpoint-policy/src/index.ts#L63-L83 (checked in review)
  - DeepSeek harness's closer wording: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/session/src/repair.ts#L31-L41
  - OpenClaw's lost-worker rule, never auto-continue: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/worker/worker-node-loss.ts#L3-L15
- **Invariants.** One writer.
- **Depends on.** PR 4's bounded link. If the append is queued behind back-pressure rather than committed locally, the rule is false.
- **Tests.** D7 and D8.

**A7. Make the approval path's failure modes fail closed.** `main` and PR 4. Small each.

- **What.**
  - The rules or policy file fails to parse: refuse to start.
  - An unknown setting value: treat it as the strictest value.
  - A shell construct cannot be analysed: ask, and deny when unattended.
  - No person can answer: deny.
  - The approver callback is missing or throws: deny.
  - The audit event cannot be appended: reject the request.
  - The sandbox is unavailable: refuse. PR 4 already does this.
  - In every case, return the real reason to the model.
- **Evidence.** The table under item 3 of section 6 in `round2/synth/02-approvals-permission-rules.md`.
  - Model to copy, DeepSeek harness's `user-approval`: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/interaction/user-approval/src/index.ts#L224-L261
  - Counter-example, Codex dropping every user rule on a parse failure: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/exec_policy.rs#L729-L744
- **Invariants.** Node decides. One writer: the audit append failing rejects the request.

**A8. Agent roster enforced on the node.** PR 6. Small to medium.

- **What.**
  - The node refuses, at execution, any tool outside the agent's `tools:` list, even when the model names it.
  - Scout and Reviewer have no write tool, and `bash` is absent or under a read-only Landlock profile.
  - A child's authority is the intersection with the parent's effective policy, recomputed at every call rather than copied at creation.
  - Depth is checked at spawn, including for a custom agent that has `delegates`.
  - PR 4's "allow for this turn" does not pass into a child. That this is the safer reading is our inference; test it.
  - A writer child whose worktree cannot be created fails the spawn. OpenHands and Hermes Agent fall back to the shared tree, which puts two writers on one tree.
- **Evidence.**
  - Codex's `explorer.toml` is empty: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/agent/role.rs#L316-L376
  - OpenCode's `explore` allows `shell *`: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/agent.ts#L80-L93
  - OpenCode's creation-time copy: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session.ts#L311-L314
  - Codex's last-step re-apply: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/agent/child_config.rs#L197-L218
- **Invariants.** Node decides. PR 6's server-side check is master-side narrowing, and the node must check again.

**A9. Loop guard, limits and a result ceiling in the dispatcher.** `main`. Small to medium.

- **Loop guard.** Hash the tool and its arguments, and the outcome too. Warn at the third repeat and block at the fifth. Stop hard on routines and only warn in attended chats.
  - The decision is made at admission, before the call runs, and an error in the guard blocks.
  - The guard's state is derived from the log fold, so a restart rebuilds it.
  - Copy Hermes Agent's thresholds: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/tool_guardrails.py#L26-L66
  - Copy OpenClaw's admission point: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/tool-loop-admission.ts#L103-L109
  - Do not repeat goose's guard, which counts on a clone and never accumulates: https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/tool_monitor.rs#L99-L134
- **Limits.** Move the hard-coded cap of 50 model calls, a per-turn wall clock and a maximum tool deadline into `node.toml` `[limits]`.
  - At the cap, keep the tool schemas, send `tool_choice: none` and refuse any late call (OpenCode; 6.3 in `round2/synth/01-loop-dispatch-durability.md`).
  - Clamp any model-supplied timeout, and never let 0 mean infinite.
- **Result ceiling.** One cap on every tool result at the dispatcher, proportional to the context window. OpenClaw uses 30% of the window, with 16k, 32k or 64k characters. Cap while reading, not after.
- **Invariants.** Node decides: the limits live in `node.toml`. Light process: the cap applies while reading.

**A10. Pull request 7, retry.** Small.

- **What to adopt.**
  - One function from error to policy, on structured fields.
  - Never retry after visible output was streamed or a tool ran.
  - Cap `Retry-After`; Oh My Pi fails fast above 300 s.
  - A separate patient schedule for no network.
  - Billing errors are not retried.
  - The same helper for title, summary and compaction calls.
  - A per-turn send counter shared by children.
  - Empty responses as their own retry class.
  - Cancel aborts the sleep.
- **Copy from.**
  - Codex's classification: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/protocol/src/error.rs#L445-L511
  - Oh My Pi's replay-safety rule: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/turn-recovery.ts#L1566-L1605
  - OpenCodex's per-request accounting, which is `unchecked`: 6.5 in `round2/synth/09-subagents-models-retry.md`.
- **Round 1 overturned.** Round 1 said copy Pi's retry defaults. Pi's provider layer defaults to 0 retries, so `Retry-After` is read nowhere, and Pi has no jitter despite a comment saying it does (4.9 in `round2/synth/09-subagents-models-retry.md`). PR 7's numbers (3 retries, 2 s base, 60 s cap) sit inside the range the code shows. The model to copy is Codex's classification, not Pi.

**A11. Pull request 7, compaction.** Medium.

- **What to adopt.**
  - A cache-preserving summary request, with leftover headroom in the trigger.
  - Fallback tiers, ending in a model-free summary and then dropping the oldest history.
  - Abort and preserve the session unchanged on terminal failures.
  - A check that compaction actually shrank the context.
  - A breaker after two compactions that did not help.
  - Estimator correction from the provider's real count.
  - One log event, written after the summary exists.
  - Instruction files re-injected after compaction.
  - Redaction applied before a summary is stored.
- **Copy from.**
  - OpenCode's request: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L451-L470 (checked in review)
  - Cline's `basic`: https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/context/basic-compaction.ts#L452-L512 (corrected in review)
  - Cline's single forced retry: https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/agents/src/agent-runtime.ts#L1378-L1423
  - Hermes Agent's abort-and-preserve: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/context_compressor.py#L4080-L4175
  - Hermes Agent's breaker: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/context_compressor.py#L3015-L3022
  - DeepSeek harness's shrink check: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/compaction/compaction-basic/src/region.ts#L431-L468
  - Cline's estimator: https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/context/compaction.ts#L211-L230
- **Caution.** PR 7's "reads in pieces" summariser is a hypothesis. No finding describes a map-then-merge summariser as a primary path (absence not confirmed). Keep it as the lowest tier.
- **Round 1 overturned.** Round 1 said copy Pi. Pi's summary request is uncached (https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/compaction/compaction.ts#L618-L638) and Pi has no shrink check.
- **Invariants.** One writer: compaction is an event appended by the session actor. No secret in event: the summary is redacted.

**A12. Seccomp under Landlock, and a protected `.git`.** PR 4 follow-up. Medium. Security code.

- **What.** Install Codex's `Restricted` rule set inside `sandbox-exec`, after `restrict_self` and immediately before `exec`:
  - With network off: `socket` only for `AF_UNIX`; deny `connect`, `bind`, `listen`, `accept`, `sendto`, `sendmmsg` and `recvmmsg`.
  - Always: deny `io_uring` and `AF_VSOCK`; deny `ptrace` and `process_vm_*`.
  - Denials return `EPERM`, never `SIGSYS`.
  - An unknown architecture is refused in `required` mode.
  - Decide `sendmsg` explicitly. Codex leaves `sendmsg` and `recvmsg` open (reviewer correction). Either deny it and test the toolchain, or list it as a named residual in `PLAN.md`.
  - Keep `.git/hooks` and `.git/config` read-only in the session folder, including when `.git` did not exist at start.
  - Show in the node status and the tool result which rules are active.
- **Evidence.**
  - Codex's filter: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/seccomp.rs#L183-L301
  - The full rule table is in 6.2 and 6.3 of `round2/synth/03-sandbox-containers-execution.md`.
- **Invariants.** Node decides. No `unsafe` in our code, and the seccomp crate needs the dependency check.
- **Why a follow-up PR.** We suggest stacking it on PR 4 rather than adding it to PR 4, so the outside review of PR 4 is not reset. That is our judgement.
- **Round 1 overturned.** Round 1 said copy DeepSeek harness "for honesty" about `full` and `partial` enforcement. Its Landlock launcher reports `full` on ABI 6 and later while confining writes only, and no client reads the value (https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/native/system/packages/entry/src/main.c#L70-L94, checked in review).
- **Round 1 softened.** Round 1 said Codex rejects Landlock because it cannot isolate Unix sockets. The code states only "isolate app-server sockets"; the socket reason is the author's inference.

**A13. A seen-version guard on `edit` and `write`.** `main`. Medium.

- **What.**
  - A per-session table keyed by canonical path, holding a SHA-256, `dev:ino:size:mtime_ns:ctime_ns`, the ranges shown in full and a bounded snapshot of the shown text.
  - The version is checked inside the per-path lock, before matching.
  - `edit` without a prior read is refused, and `write` over an existing file needs a full read.
  - A stale file follows the ladder in 6.1 of `round2/synth/08-edit-tools-mechanics.md`: apply only when the matched span is byte-identical to what the model saw; otherwise refuse and return a bounded diff.
  - Hashes go into the `read` events, so a restart rebuilds the table by fold.
  - Writes are atomic: temp file, `fsync`, rename.
  - Multi-edit batches are matched against the original content.
  - Matching stops at a one-to-one Unicode fold, and the result reports which tier matched.
- **Copy from.**
  - DeepSeek harness's check inside the lock: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/fs/fs-local/src/index.ts#L250-L277
  - Hermes Agent's full-read rule: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/file_tools_write_guards.py#L166-L173
  - Pi's batch: https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/tools/edit-diff.ts#L343-L406
- **Evidence.** Our `edit` is exact-replace with no staleness check (verified).
- **Invariants.** One writer: the table is a fold over the log. Light process: snapshot text goes on disk, hashes stay in memory.
- **Depends on.** A11. Clearing "ranges shown in full" at compaction is `inferred` and unmeasured.

**A14. Checkpoint restore safeguards and a recorded head.** `main`. Medium.

- **What.**
  - Snapshot the present before restoring, so a restore can itself be undone.
  - Send `expected_tree` with the request and refuse when the live tree differs, unless the restore is forced.
  - Refuse during a turn, and when another session shares the worktree.
  - Refuse when the tree holds a nested repository.
  - Keep files no snapshot holds, such as files over the size cap.
  - For "from that turn on", restore only paths the turns being undone touched.
  - Validate every ref name and tree id against the node's own shadow repository.
  - Append a `HeadMoved` event so a rewind survives a restart.
  - Take no snapshot for a turn that only read.
  - Add timing and coverage fields to the `Checkpoint` event.
- **Copy from.**
  - Cline's restore transaction: https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/session/checkpoint-restore.ts#L50-L154
  - Hermes Agent's nested-repository refusal: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/checkpoint_manager.py#L1376-L1385
  - Hermes Agent's handling of oversize files: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/checkpoint_manager.py#L1429-L1452
  - OpenCode's attributed paths: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/revert.ts#L22-L56
  - Codex's compare-and-set of the active history: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/thread-store/src/local/revert_thread.rs#L138-L156
- **Invariants.** One writer: the node actor writes `HeadMoved`. Node decides: a restore names a tree that exists on the node, never a path.
- **Depends on.** Tests D3, D4 and D13 before any change to cost.

**A15. Keep streaming deltas out of the event log, and resume by cursor.** `main` and PR 4. Medium.

- **What.** Split frames into two kinds.
  - Durable frames carry `(session_id, owner_epoch, seq, hash)`. The node sends them from a cursor over its own log, and the master acknowledges only after its mirror commits.
  - Transient deltas carry no `seq`, are coalesced latest-wins and are dropped first under back-pressure. The durable message supersedes them.
  - The browser subscribes first, then replays to a captured watermark, then follows.
  - Per-socket queues are bounded, and a lagging client is disconnected.
- **Copy from.**
  - OpenHands' frame envelope: https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/types/agent-server/session-frames.ts#L1-L79
  - OpenCode's log contract: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/bus.ts#L829-L878 (checked in review)
  - Codex's rule to reject new requests but never drop responses: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/mod.rs#L231-L262
- **Invariants.** One writer. Light process: no unbounded buffer on either side. It fixes AUDIT D5.
- **Depends on.** A2. First settle in code whether deltas are still written to the log on the current branches (test D16).

### (b) Change in the design note before building

Each item names the section of `docs/nodes-credentials-environments.md` it affects.

**§4.1 Data model.**

- Add `nodes.link_gen`, `sessions.owner_epoch` and `session_forks.hash_at_through_seq` (A2).
- State the AES-GCM associated data for `workspace_secrets`: bind `workspace_id` and `name`, so a row copied to another workspace or name fails to open. Warp binds actor, name and type into its sealing: https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/managed_secrets/src/envelope.rs#L33-L80.
- Store only hashes of any bearer the master issues. OpenClaw hashes worker credentials but keeps device tokens in plaintext (6.5 in `round2/synth/05-links-workers-protocols.md`).
- Invariant: no secret in event, URL or log.

**§4.2 Model aliases and slots.**

- Model "strict" as `Option<Vec<_>>`, so a defined empty fallback list fails instead of switching. That is OpenClaw's `fallbacksOverride = []`, `unchecked`.
- Add `timeout` and reasoning effort to each slot, after Hermes Agent's slot shape.
- Keep slot names as strings in a set the master validates, not a protocol enum.
- A slot failure must not fail the turn.
- A slot or child model on a different provider needs its own grant (6.4 in `round2/synth/09-subagents-models-retry.md`).

**§4.3 Granting model access.**

- **TTL.** Replace the 6-hour `expires_ms` with minutes, renewed by the master while the turn is live. Drop the grant on `grant.drop`, at turn end, on link loss after the turn and on a newer grant.
- **Binding.** Bind the grant to `(node_id, session_id, owner_epoch, turn req_id)`. The node refuses it for any other session, and the master refuses to send it to a node of another workspace.
- **Scope.** Check scope against the models resolved after alias expansion, not the names the agent spec carries: https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/admission-model-scope.ts#L8-L29.
- **Minting.** A new grant cancels in-flight calls that used the old one: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/credential-broker.ts#L105-L126.
- **Revalidation.** Check the grant on every `session.prompt`. Codex checks bearer credentials once at upgrade and documents that expiry is not enforced after: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/websocket-auth/src/lib.rs#L286-L330.
- **The proxy.** For the reserved `ModelAccess::Proxy`, write the bounds first: stream the body once, a shared byte budget with 503 and `Retry-After`, a pull stream, a retry budget on the logical request, failover only before output. OpenCodex is the reference, and several of its findings are `unchecked` (item 3 of section 6 in `round2/synth/04-credentials-secrets-auth.md`). Its "queue of one" holds only for the generic streaming bridge.
- **Embedded node.** The local node that runs in the master process must not be able to read the master's secrets (A3).
- Invariants: node decides (a grant is refused when `model_keys` is `local` or `none`); no secret in event, URL or log.

**§4.4 Adding a node with a pair code.**

- Consume the code before validating anything else, so a rejected request still burns it: https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-hub.ts#L377-L425.
- Refuse requests carrying an `Origin` header.
- Rate-limit by the kernel-observed peer address, and answer every failure with one opaque reason.
- Send the code in a POST body, never a link.
- Show a 128-bit fingerprint in groups on both sides, not 16 hex characters. goose uses eight groups: https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-roaming/src/card.rs#L56-L75.
- Invariants: no secret in a URL; loopback default.

**§4.5 `node.toml` additions.**

- Add `[limits]` for steps, a per-turn wall clock and a tool deadline maximum (A9).
- Add a checkpoint time budget and a byte cap (A14).
- Container caps a session may lower and never raise.
- No `extra_args` key, now or later.
- Make "systemd credentials, else a 0600 file" an explicit, logged choice at join, not a fallback on error. OpenCodex fails closed with "no plaintext fallback".
- Invariant: node decides.

**§4.6 The container the node builds.**

- **Argv.** Adopt the argv in 6.1 of `round2/synth/03-sandbox-containers-execution.md`. It adds the following to the current note:
  - an argv-hash label, with the container recreated on mismatch;
  - sized `nosuid,nodev,noexec` tmpfs on `/run`;
  - `--ulimit core=0`;
  - `--network=none` by default;
  - `--pull=never` with the image pinned by digest;
  - a check that the created id is 64 hex digits;
  - `--env NAME` with the value in the `podman` child's environment;
  - one mount whose source is validated against the data directory, `$XDG_RUNTIME_DIR` and `~/.ssh`.
- **Evidence.** OpenClaw's validation: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/validate-sandbox-security.ts#L23-L53. Hermes Agent's appended extras are the hole to avoid: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/docker.py#L741-L745.
- **Self-test.** Add `podman inspect` assertions: no privilege, empty `CapEff`, one mount, a read-only root, the network mode, the label.
- **Kill.** Keep the three-step kill. `podman stop` is the guarantee. Stop a session's containers as soon as its actor is lost.
- **Same checks inside.** Approval, the safety floor and checkpoints are identical for host and container sessions. Hermes Agent skips its checks inside containers (9.16).
- **Network on.** Do not ship `network = "on"` until test D14 shows what rootless `pasta` exposes on the host's loopback.
- Invariants: node decides; loopback default.

**§4.7 Forge operations.**

- Hand forge tokens only to the git child, scoped to `[forge] hosts` and short-lived.
- Expect the agent to be able to read them whenever it can run git. OpenClaw's worker GitHub token is readable by the agent (corrected finding).
- Strip `GIT_DIR`, `GIT_CONFIG*` and `GIT_WORK_TREE` for every forge git call (item 5 of section 6 in `round2/synth/04-credentials-secrets-auth.md`).

**§4.8 Deleting an environment.**

- Deletion must stop unattended runs, not only interactive ones.
- Deletion bumps `owner_epoch` for every affected session, so late appends from a node are refused (A2).

**§4.9 Fork.**

- **Lineage.** Record lineage inside the child's log. The child's first event is `SessionForked { parent, root, parent_node_id, through_seq, cut, lossy, files, files_ref }`, followed by `history_imported` batches marked `inherited` and a `SeedEnded` marker.
- **Cut point.** `through_seq` is the highest contiguous mirrored seq, never an unmirrored tail. The default cut is the last completed turn.
- **Open turns.** A cut inside an open turn closes it with fork-specific closers.
- **Fresh child state.** The child starts at `ask` with no grants and no "allow for this turn". Usage counters start at zero.
- **Files.** A same-node fork can seed its worktree from the checkpoint tree of the cut turn (`files: "checkpoint"`). A cross-node fork stays on the forge route.
- **Evidence.** DeepSeek harness's fork: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/session/src/fork.ts#L21-L30. Further detail is in 6.2 of `round2/synth/05-links-workers-protocols.md` and items 1 and 8 of section 6 in `round2/synth/06-sessions-fork-checkpoints.md`.
- **Round 1 overturned.** Round 1 proposed a Pi-style parent pointer over our log. The child runs on a different node and cannot read the parent's file, and Pi's path-only parent link orphans on rename. Copy semantics are the fit.
- Invariant: one writer.

**§6 How this breaks.**

- Replace "a second connection replaces the first" with the fence in A2.
- Keep "revoke after 3 flaps in 10 minutes", and make the revocation bump every affected session's epoch.

### (c) Build next, in this order

Each step is a feature, so it needs the owner's decision to break the freeze. The order follows risk and dependency. A step does not start until the safeguards listed for it are in the same change.

| # | Build | Safeguards from day one | Evidence | Depends on |
|---|---|---|---|---|
| 1 | Unattended authorisation for routines and webhooks | A per-routine allow set created by a person, content-hashed and recorded as an event. Anything outside it is denied with the real reason. The schedule advances before dispatch, and an idempotency record is fsynced before the first model call; an unknown outcome pauses the routine. An agent-created routine starts `pending_approval`. Webhook idempotency keys live in the database. Never auto-approve | Step 1 of 6.1 in `round2/synth/10-extensibility-unattended.md`; 6.7 in `round2/synth/01-loop-dispatch-durability.md` (Hermes Agent `cron/scheduler_tick.py`, `cron/scheduler_authority.py`) | A7, A9 |
| 2 | Workspace nodes, pair codes, per-turn grants (design note) | The §4.1, §4.3 and §4.4 changes above; abuse tests for a grant used by another session and a prompt after node removal | 6.1 and 6.6 in `round2/synth/05-links-workers-protocols.md` | A2, A3, A15 |
| 3 | Environments (rootless Podman, worktree per session) | The §4.6 argv, the `podman inspect` self-test, stopping containers on actor loss, identical checks inside | 6.1 to 6.5 in `round2/synth/03-sandbox-containers-execution.md` | Step 2; tests D14 and D15 |
| 4 | Fork by copy with lineage | The §4.9 items; refuse "continue elsewhere" during a running turn | 6.2 in `round2/synth/05-links-workers-protocols.md` | A2, A14 |
| 5 | Web and memory tools on agentic nodes (today they are registered in the chat runtime only) | Memory approval enforced in the write path and failing closed. A staged write carries the base version of the entry. Shared memory stores live outside the shell's writable set. Threads that touched external content are excluded from automatic extraction. Web tools keep URL provenance and the SSRF resolver | 6.2 in `round2/synth/07-context-compaction-memory.md`; Round 1 section 8, table stakes item 4 | PR 4 network block |
| 6 | Event vocabulary for context changes | A prompt-section delta event and a tool-loadout event, folded by node and UI; sorted tool names; changes appended, never edited into the prompt head | Step 2 of 6.1 in `round2/synth/10-extensibility-unattended.md` | A11 |
| 7 | Skills (text only) | Node-controlled roots; repository skills only after a per-project trust decision bound to content; a capped catalog; the body fetched through the approval path; nothing executes at load | Step 3 of 6.1 in `round2/synth/10-extensibility-unattended.md`; OpenClaw https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/skills/loading/skill-prompt-limits.ts#L10-L40 | Step 6 |
| 8 | MCP client with deferral | Only `node.toml` adds a stdio server. Its environment comes from the PR 4 allow-list. It runs inside the session's sandbox or container. Approval is per server and tool, bound to the executable's digest, and a `readOnlyHint` only tightens. Size and time caps. Deferral is switched on by measured tool-list size, with activation recorded as a loadout event. OAuth tokens are granted per turn like provider keys | Step 4 of 6.1 in `round2/synth/10-extensibility-unattended.md`; Hermes Agent https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/mcp_tool_config.py#L199-L207 | A1, steps 3 and 6 |
| 9 | Hooks | Node only; under the PR 4 sandbox; trust bound to the bytes of every referenced file and stored where only the node owner writes; fail closed on crash, timeout, bad output or a bad matcher; a hook may deny or escalate, never approve; policy re-resolved after a rewrite | Step 5 of 6.1 in `round2/synth/10-extensibility-unattended.md`; Oh My Pi https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/wrapper.ts#L355-L374 | Step 8 |
| 10 | Input during a turn | The message is persisted with an idempotency key before it is acknowledged, delivered at a step boundary, never kills a running tool, and a dropped message gets a receipt | 6.10 in `round2/synth/01-loop-dispatch-durability.md` | A6 |
| 11 | Per-command rules, then a plan mode | A maintained parser (`tree-sitter-bash` or `brush-parser`, after the dependency check). Allow rules must cover every segment; deny rules hit any segment after unwrapping wrappers. Rules are never a click and never lift the sandbox. Plan mode is enforced in the registry, the dispatcher and a read-only Landlock profile, and only the user can leave it | Items 2, 5 and 9 of section 6 in `round2/synth/02-approvals-permission-rules.md` | A1, A12 |

Plugins as bundles and code mode come after step 9, if at all. Code mode is the one feature in the set that can defeat the approval path (goose, 9.30).

### (d) Tests to write against our own code now

Each settles an open risk. Most should fail or surprise on `main`.

| # | Experiment | What it settles | Blocks |
|---|---|---|---|
| D1 | The agent runs `setsid sleep 1000`, then the call is cancelled | Whether a detached child survives our process-group kill. goose and Cline confirm the leak class; for Oh My Pi and Pi it is `inferred` | Choice of cgroup, PID namespace or a re-walk (A12, §4.6) |
| D2 | `kill -9` the node while `bash` runs a long child; restart | Whether children outlive the node. `PR_SET_PDEATHSIG` from `pre_exec` needs `unsafe`, so a supervisor or cgroup may be required | A6, §4.6 kill ladder |
| D3 | Snapshot, rewrite history in the user's repository, run `git gc --prune=now`, restore | Whether alternates let a shadow-repository checkpoint lose objects. OpenCode's version of this risk is `inferred` | A14; whether to copy blobs into our own store |
| D4 | Time a checkpoint over synthetic trees of 1k, 10k, 100k and 500k files, warm and cold cache, with 0, 10 and 1,000 changed files | The real cost of our full-tree scan; nobody in the input measured it | Any checkpoint budget in `node.toml` |
| D5 | Queue two event frames on connection A, connect B for the same node before A's frames are dispatched | Whether frames from a replaced connection are still applied after `nodes.rs:207` installs the new one | A2 |
| D6 | Archive a session while its node is offline; reconnect | Whether a stale node can append to a closed session | A2, §4.8 |
| D7 | `kill -9` the node during a `bash` call; restart | Whether the log holds the call's arguments and a closing event that says "unknown" | A6 |
| D8 | Fill the disk, then start a tool call | Whether a failed append stops the tool | A6 |
| D9 | A provider fixture ending a `write` call mid-JSON with `finish: length` | Whether we run truncated arguments | A5 |
| D10 | A sandboxed shell runs `env`, reads `/proc/self/environ` and `/proc/$PPID/environ` during a turn with a grant | Whether a grant or link key reaches the agent | A3, §4.3 |
| D11 | A sandboxed shell reads `/proc/$PPID/mem` and tries `gdb -p` on the node | Whether Landlock alone stops memory access to the node; no input settles it | A3 (`PR_SET_DUMPABLE`) |
| D12 | A sandboxed command connects to every socket the node and master open (master WebSocket, node link, MCP bridges, the Podman socket) | Whether below ABI 9 a confined process can reach an unauthenticated local socket | A12, step 8 |
| D13 | Rewind to an earlier turn, restart the node, open the session | Whether files and conversation disagree after a restart, which would show that `HeadMoved` is needed | A14 |
| D14 | Start a `network = "on"` rootless container with `pasta`; connect to `127.0.0.1:7777` and the node's sockets | What rootless networking exposes on the host loopback; no input covers it | §4.6 network row |
| D15 | Kill the `podman exec` client during a long command | Whether the signal reaches the process inside the container; this is open in the design note too | §4.6 kill ladder |
| D16 | Stream a long answer and count events in the log | Whether deltas are still stored as events on the current branches (the inventory and the brief disagree) | A15 |
| D17 | The node streams output at the link cap while a cancel and an approval answer arrive | Whether a full queue blocks control messages beyond the grace period | PR 4 back-pressure |
| D18 | A Scout calls `write` by name although it is not in its `tools:` list | Whether the node refuses at execution, not only by hiding the tool | A8 |
| D19 | Compact a session whose tool results contain a provider key | Whether the summary stores the secret | A11 |
| D20 | `git add -A` over a tree containing a nested `.git` directory | What our checkpoint captures and what a whole-tree restore would do | A14 |

### (e) Do not copy

| Do not copy | Project that shows why | Evidence | Invariant at stake |
|---|---|---|---|
| A model-set flag that lowers the gate | Warp's `is_risky == Some(false)` skips the allow-list | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/blocklist/permissions.rs#L972-L1008 | Node decides |
| An allow rule that lifts the sandbox | Codex `Skip { bypass_sandbox: true }` | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/exec_policy.rs#L492-L512 | Node decides |
| Dropping every rule on a parse error | Codex | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/exec_policy.rs#L729-L744 | Node decides |
| Hooks that fail open | Codex `PreToolUse` | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/hooks/src/events/pre_tool_use.rs#L211-L300 | Node decides |
| Unattended equals auto-approve | goose scheduled jobs in `Auto`; Cline cron yolo | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/scheduler/full.rs#L881-L907 ; https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/cron/runner/cron-runner.ts#L84-L108 | Node decides |
| Pass-through container arguments | Hermes Agent `docker_extra_args` appended last | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/docker.py#L741-L745 | Node decides |
| Relaxing checks inside a container | Hermes Agent skips its floor and prompts | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval.py#L1161-L1175 | Node decides |
| A control-plane key in the agent's environment | OpenHands session key | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L975-L985 | No secret in event, URL or log |
| An empty approver list meaning "anyone in the chat" | OpenClaw | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/plugin-sdk/approval-auth-helpers.ts#L72-L89 | Private chats |
| Repository files that loosen user settings | Oh My Pi project layer merged with no key filtering | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/config/settings.ts#L2800-L2814 | Node decides |
| Children forced to the most permissive mode | Oh My Pi `yolo` overlay | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/task/executor.ts#L1259-L1278 | Node decides |
| Code-mode inner calls that skip the gate | goose `lease.call` | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/code_execution.rs#L172-L194 | Node decides |
| Similarity matching at low thresholds | Cline 0.66, Zed 0.8 per line | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/tools/executors/apply-patch-parser.ts#L347-L430 ; https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/tools/edit_session/streaming_fuzzy_matcher.rs#L254-L256 | None; a correctness risk |
| NFKC folding that rewrites whole lines | Pi `widenReplacementsToLines` | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/tools/edit-diff.ts#L176-L214 | None; a correctness risk |
| Checkpoints as uncapped refs in the user's repository | Cline | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hooks/checkpoint-hooks.ts#L535-L649 | Light process (disk) |
| A durable log that is off by default | OpenCode `persist` false in the shipped server | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/bus.ts#L186-L192 | One writer |
| Latest connection wins as the only fence | OpenCodex Remote Workspace, and our `main` | 6.1 in `round2/synth/05-links-workers-protocols.md` | One writer |
| Full environment inheritance for MCP children | Oh My Pi, Pi, Cline, Warp | 4.1 in `round2/synth/10-extensibility-unattended.md` | No secret in event, URL or log |
| A guard that ships disabled, or a read-only role that is only a name | DeepSeek harness observation policy; Codex `explorer.toml`; OpenCode `explore` | Section 1, "The general finding" | Node decides |

### Round 1 recommendations that Round 2 changes

| Round 1 said | Round 2 shows | Evidence |
|---|---|---|
| Copy Pi's retry defaults | Pi's `Retry-After` is dormant by default, and it has no jitter; copy Codex's classification | A10 |
| Copy Pi's compaction | Pi's summary is uncached and unchecked for shrinkage; copy OpenCode, DeepSeek harness and Cline | A11 |
| Copy DeepSeek harness's `full`/`partial` honesty | It reports `full` while confining writes only, and nobody reads the value | A12 |
| Codex rejects Landlock because it cannot isolate Unix sockets | The code says only "isolate app-server sockets"; the socket reason is inferred | 9.14 |
| Fence stale nodes like OpenClaw and OpenCodex | OpenCodex is latest-connection-wins; only OpenClaw fences | A2 |
| Fork with a Pi-style parent pointer | A copy with lineage in the child's log fits a child on another node | §4.9 above |
| Copy OpenCode's plan-mode enforcement | OpenCode's plan denies edit tools only; shell, sub-agents and execute stay allowed (corrected) | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/plan.ts#L33-L42 |
| Copy Pi's separate project trust gate | Pi's gate is a path boolean, and project context files load regardless of trust | 4.7 in `round2/synth/10-extensibility-unattended.md`; 4.9 in `round2/synth/07-context-compaction-memory.md` |
| OpenClaw stores steered input before acknowledging it | True for the channel and `agent` paths that were checked; the in-flight steering list is in memory | 4.5 in `round2/synth/01-loop-dispatch-durability.md` |
| Zed removed per-prompt checkpoints because they hung large repositories | The code does not say why; the reason is inferred from the release note | 4.6 in `round2/synth/08-edit-tools-mechanics.md` |

## 11. Technical limitations and unanswered questions

### What Round 2 cannot tell us

**One commit, no history.** Each repository was read at one pinned commit, and the commit dates are all between 2026-11-04 and 2026-11-11 (see `stats.json`). Round 2 cannot say what changed before that commit, why a design was dropped, or whether a bug found here is already fixed upstream. The clearest case is Zed: the clone is shallow, so why per-prompt checkpoints were removed cannot be read, and the large-repository reason is inferred from a release note.

**Nothing was built or run.** Every behaviour in this report was read from code, not observed. This covers bypasses such as Cline's `Origin`-only hub session, self-approval through a readable OpenCode or OpenHands credential, and Oh My Pi's `bash -c` past a deny rule. It covers the cost claims too: nobody timed a checkpoint, a compaction or a cache hit. Where the report says "fails closed" or "is refused", it means the code path reads that way.

**Review coverage is partial.** The study produced 1,008 findings and about 6,180 permalinks, and a script confirmed that every permalink exists. Reviewers opened the cited code for 632 findings. None was wrong outright, and about 65 were corrected in place. The other findings were read by one investigator only, and the group files mark them `unchecked`. `stats.json` counts 653 findings marked checked, against the review brief's 632; this section did not reconcile the two figures. Several recommendations in section 10 rest partly on `unchecked` findings. The main ones are Codex's session approval key, OpenCodex's send budget, stream preflight and value-based log guard, Hermes Agent's deny-only approval card, and OpenClaw's standing grants.

**Absences rest on searches.** "Nothing in the code read" means a search of the files the investigator opened found nothing. Only findings marked `absence confirmed` had a reviewer repeat the search. Examples of unconfirmed negatives that matter: no map-then-merge summariser in any project, no shrink check in OpenClaw or goose compaction, no instruction-file size cap in Oh My Pi or Cline, no OS sandbox for OpenClaw host exec.

**Some repositories hold only part of a system.** These parts were not available, and no claim about them should be taken from this report:

| Project | What is missing | Where it lives |
|---|---|---|
| OpenHands | The engine: agent loop, `docker run` invocation and hardening flags, condenser, stuck detector, `file_editor`, Agent Server auth and event persistence, Automation Server, which names `sanitized_env()` strips | `OpenHands/software-agent-sdk` (not cloned) |
| Warp | Server-side loop, prompts, compaction, routing and fallback, inbox durability, self-hosted worker | Warp's server, `warpdotdev/oz-agent-worker`, `warp-proto-apis` (not cloned) |
| Agent Client Protocol | The schema that Zed, goose, Hermes Agent, Oh My Pi, DeepSeek harness and Warp implement | `agentclientprotocol/agent-client-protocol` (not cloned) |
| Tunnels and relays | OpenCode `--remote` encryption; Codex remote-control relay, registry and pairing expiry; DeepSeek harness LiveShare relay; Hermes Agent relay connector buffering; Oh My Pi collab relay; goose's iroh relay and what it can see | `@opentunnel/*` packages and hosted vendor services |
| OpenCodex | The Home side of Remote Link, traced only in part; the macOS and Windows helper; the Claude Code and Pi remote-workspace runtimes | Not read in this round |
| Cline | Per-model system prompts beyond the two templates in this repository | Cline's Desktop and Kanban repositories (not read) |
| OpenClaw | ClawHub policy, plugin install trust, the browser sandbox's CDP token | In the repository but not read |
| Codex | Seatbelt base profile and the Windows restricted-token and MXC backends (not read beyond dispatch); `credential_broker.rs` (not opened by the reviewer) | In the repository |
| Zed | The agent loop and sub-agents were outside the files read; so were instruction files and memory | Outside this study's scope |

**Closed products.** Claude Code, Claude Desktop, Codex Desktop, Cursor and Antigravity are closed source. They are known only from Round 1's documentation survey. Round 1 recommendations that rest on them, such as Claude Code's `defer` for unattended approvals, are neither confirmed nor refuted here.

**Reviewer cautions about categorical wording.** Several earlier summaries overstated a mechanism, and the reviewers corrected them:

- OpenClaw's sub-agent lane width of 8 is per spawning session, not gateway-wide.
- OpenClaw's in-run steering queue is in memory.
- OpenClaw's owner epoch bumps only on attach or from certain states.
- OpenCodex's "queue of one" holds only for the generic streaming bridge.
- DeepSeek harness's automatic reviewer is opt-in and mounted by no shipped bundle.
- OpenCode's sub-agents get a one-time copy of the session's rules at creation.
- Hermes Agent's restore blind spot has three exceptions.
- Codex's refusal of an unauthenticated non-loopback listener covers the app server only.

For OpenClaw, one review says to trust constants and structure and to re-check anything that says a lane, queue or claim is global, durable or unsettled. Read every "only", "never" and "all" in the group files with that caution. One passage in `patterns.md` still says OpenClaw's "always" binds "the exact executable"; the reviewed reading is in 9.7.

### Open questions, grouped, with what would settle each

**Loop and durability**

- Whether goose runs tool calls in a batch in parallel or in sequence. The two findings files cite the same lines and disagree. Running a two-tool batch with timestamps would settle it.
- Whether steered input is stored before acknowledgement in Codex, OpenCode, Oh My Pi and DeepSeek harness. The entries for Codex and DeepSeek are inferences. A crash right after the acknowledgement would settle it.
- What a crash mid-turn does in Codex, Oh My Pi, Cline and Pi's shipped session. Kill each during a tool call and resume.

**Approvals**

- How OpenHands enforces `ConfirmRisky`, and whether its security analyser makes its own model call. Settled by reading `software-agent-sdk`.
- How Warp's server sets `is_risky`. Settled by Warp's server code, which is unavailable.
- Whether OpenCode's `permission.evaluate` hook fails closed when it throws. Settled by reading `plugin/promise.ts`.
- Whether a bubblewrap start failure in Codex is misread as a sandbox denial and offered an unsandboxed retry. `denial.rs` matches keywords; a forced start failure would settle it.

**Sandbox and containers**

- What rootless `pasta` exposes to a container with network on, in particular the host loopback where the master listens. Settled by test D14.
- Whether killing the `podman exec` client reaches the process inside. Settled by test D15.
- Whether Podman's default seccomp profile denies `io_uring` and `AF_VSOCK`. Settled by reading the profile, or by a syscall probe inside a container.
- Whether an addressed `sendmsg` to a pathname datagram socket passes Codex's `Restricted` filter, and whether Landlock ABI 9 covers datagram sends. Settled by a probe under each.
- Why Zed moved from Landlock to bubblewrap. The code states no reason; the project's history would.

**Credentials**

- Whether Landlock stops `ptrace` and `/proc/<pid>/mem` of the node process from a confined child. Settled by test D11.
- Whether `systemd-creds` works for user services. Settled by a test on our side.
- Whether OpenClaw's secret egress proxy is bypassed by a process that ignores proxy variables. This is inferred, because no namespace was found on that path. A raw-socket probe would settle it.
- Whether the OpenCodex numbers in item 3 of section 6 in `round2/synth/04-credentials-secrets-auth.md` hold. Re-read `combo-stream-preflight.ts`, `request-send-budget.ts` and `configured-credentials.ts` before copying them.

**Links and protocols**

- Whether our master applies frames already queued from a replaced connection. Settled by test D5.
- Whether deltas are still written to our log on the current branches. Settled by test D16.
- What Codex does when no connection qualifies for a human-only approval. Settled by reading the rest of `outgoing_message.rs` after L430.
- Whether OpenClaw's transcript-commit ledger works as read. It was not checked in review; a reviewer pass would settle it.

**Sessions and checkpoints**

- Whether `git gc` in the user's repository can break a snapshot that reaches objects through alternates. Settled by test D3.
- What a full-tree checkpoint costs on a large repository. Settled by test D4.
- Whether git's built-in filesystem monitor or an inotify flag is a safe "tree unchanged" signal. Nothing in the input confirms either; a test that edits files in every way the shell can would settle it.
- Whether Pi's `/tree` navigation without a later write survives a restart. This is inferred; branch, exit and resume would settle it.
- Whether hooks or filter drivers can fire in Hermes Agent's checkpoint store. A hostile `.gitattributes` plus a global filter would settle it.

**Context and compaction**

- Which trigger fraction is right. The projects range from 0.5 to 0.9, and no project measured.
- How often a cache-preserving summary request hits the cache.
- Whether a summary keeps the facts that matter. Only OpenClaw audits headings and identifiers.
- Whether clearing the "seen" table at compaction helps or only adds re-reads.

All four need measurement on our own sessions after pull request 7 lands.

**Edits**

- Whether a 16-bit hashline collision bypasses recovery in Oh My Pi. Two texts that share a tag would settle it.
- Whether Hermes Agent's `patch` uses the byte snapshot or only the mtime. Settled by tracing the call.
- What post-edit diagnostics Oh My Pi and Hermes Agent run. Settled by reading their LSP layers.

**Sub-agents and retry**

- OpenClaw's per-attempt retry and per-run retry budget, which were not read.
- Whether Zed's native agent has a sub-agent tool.
- OpenHands' sub-agent depth and permission inheritance, which live in `software-agent-sdk`.
- Whether pull request 7 already has jitter, a `Retry-After` cap, a no-network schedule and per-reason classes. Settled by reading our own branch, which the synthesis authors did not.

**Extensibility**

- Whether a stdio MCP child inherits the harness environment in Codex and OpenCode. Settled by a test server that prints its environment.
- Whether Cline's file hooks run under `--yolo`, which the code and the docs disagree on. Settled by running it.
- Whether workflow children in DeepSeek harness and code-mode inner calls in Codex go through approvals. Settled by tracing those paths.
- How the OAuth authorisation code, which arrives in a redirect URL, fits our rule of no secret in a URL. This is our own design question, to decide before MCP OAuth.

### Which conclusions in section 10 to build on, and which to test first

**Solid enough to build on now.** These rest on reviewed code, on our own verified facts, or on a direct consequence of our invariants:

- The approval digest, owner check and single use (A1). The OpenClaw binding was checked in review. Only the Codex session-key fields need a re-read.
- The two-counter fence and the hash rule for resends (A2). OpenClaw's fence was checked; our reconnect without a generation is verified in our code.
- The credential-reach tests (A3) and the environment tests. They are tests, so they settle themselves.
- Per-user API tokens (A4).
- Refusing tool calls after a `length` stop (A5).
- Fail-closed handling in the approval path (A7).
- Node-side enforcement for the agent roster (A8).
- The retry classification (A10).
- The cache-preserving summary request, the shrink check and the model-free fallback tier (A11).
- The seen-version guard design (A13).
- The restore safeguards and the `HeadMoved` event (A14).
- Durable versus transient frames (A15).
- Every design-note change in part (b) except those named below.
- The whole "do not copy" table.

**Wait for an experiment.**

- Any change to how often or how widely we checkpoint, the degradation ladder and any `node.toml` budget: wait for D4, and for D3 on the alternates risk.
- The kill strategy for host and container commands (cgroup, PID namespace or re-walk): wait for D1, D2 and D15.
- `network = "on"` for containers: wait for D14.
- The `sendmsg` rule and the exact seccomp set: wait for a toolchain test under the filter.
- The non-dumpable process as a complete defence: wait for D11.
- The append, flush and act rule (A6) is right, but whether our code already meets it needs D7 and D8.
- Trigger fractions, the "pieces" summariser and clearing the seen table at compaction: wait for measurement after pull request 7.
- The OpenCodex-based proxy numbers (§4.3): re-read the `unchecked` code first.
- Retry accounting across children, which rests on an `unchecked` OpenCodex finding.

## 12. Complete source references

Every citation in this study is a GitHub permalink of the form `https://github.com/<org>/<repo>/blob/<commit>/<path>#L<start>-L<end>`, pinned to the commits below. They are attached to the findings they support, in the per-repository files (section 5) and the synthesis files (section 4). `round2/check_citations.py` re-verifies any of those files against the local clones.

| Project | Repository | Pinned commit | Commit date | Branch |
|---|---|---|---|---|
| codex | https://github.com/openai/codex | `82e6bacb9c6950d3b56cb153847fa80bdd698053` | 2026-11-10 | main |
| opencodex | https://github.com/lidge-jun/opencodex | `ddee2c20030c1f0ec852202256be877397e8f396` | 2026-11-11 | dev |
| opencode | https://github.com/anomalyco/opencode | `05d347d75054bf27778750ad5f2cd30336e81d8f` | 2026-11-10 | dev |
| deepseek-harness | https://github.com/deepseek-ai/deepseek-harness | `fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159` | 2026-11-04 | master |
| oh-my-pi | https://github.com/can1357/oh-my-pi | `e325ffc73ab86e03bdd20a91b77d8e2b3964c906` | 2026-11-10 | main |
| pi | https://github.com/earendil-works/pi | `91d9719b4123a83816249d7a7917c75126bb55af` | 2026-11-10 | main |
| openhands | https://github.com/OpenHands/OpenHands | `d7293dfd671e23dab182fe71c247ff05f48e780b` | 2026-11-10 | main |
| openclaw | https://github.com/openclaw/openclaw | `8bac830dcb8ff99055ba71558dd369a989cc7881` | 2026-11-10 | main |
| goose | https://github.com/aaif-goose/goose | `14bf51817b79f6e98ca73c6eda61c11d9d5cc892` | 2026-11-10 | main |
| hermes-agent | https://github.com/NousResearch/hermes-agent | `95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02` | 2026-11-10 | main |
| zed | https://github.com/zed-industries/zed | `3def8bad0b62dbda02d5b67b00df96fe8d3957bd` | 2026-11-10 | main |
| warp | https://github.com/warpdotdev/warp | `4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39` | 2026-11-10 | master |
| cline | https://github.com/cline/cline | `500a40e7b76692153906b44dd68732520a966663` | 2026-11-10 | main |

Totals: 6179 permalinks across the 31 per-repository files; the ten synthesis files reuse them. Closed-source products (Claude Code, Claude Desktop, Cursor, Antigravity) have no source references; what is known about them is in the Round 1 report with documentation URLs.

## 13. Round 1 claims tested against the code

Round 1 read documentation; Round 2 read the code at one pinned commit per repository. Every Round 1 claim that a Round 2 file checked against the code is a row in one of the ten tables of `round2/synth/_parts/docs-vs-code.md`, published in full in the appendix. This section counts them, lists every refutation, and selects the corrections and confirmations a design decision rests on.

### Counts by verdict

Counted from the table rows of `docs-vs-code.md`, one row per claim per group.

| Feature group | Rows | Confirmed | Corrected | Refuted | Not in code | Not settled | New | Settled or answered | Other |
|---|---|---|---|---|---|---|---|---|---|
| 1. Agent loop, tool dispatch, durability | 41 | 24 | 11 | 3 | 1 | 0 | 1 | 0 | 1 |
| 2. Approvals and permission rules | 210 | 107 | 56 | 13 | 2 | 0 | 32 | 0 | 0 |
| 3. Sandboxing and where commands run | 46 | 27 | 17 | 0 | 1 | 0 | 0 | 1 | 0 |
| 4. Credentials, secrets, authentication | 46 | 17 | 24 | 2 | 0 | 1 | 2 | 0 | 0 |
| 5. Links, workers, client protocols | 73 | 41 | 24 | 3 | 3 | 1 | 1 | 0 | 0 |
| 6. Sessions, resume, fork, checkpoints | 71 | 51 | 14 | 4 | 2 | 0 | 0 | 0 | 0 |
| 7. Context, compaction, instructions, memory | 123 | 85 | 27 | 1 | 3 | 1 | 3 | 3 | 0 |
| 8. Edit tools and mechanics | 40 | 16 | 16 | 3 | 4 | 1 | 0 | 0 | 0 |
| 9. Sub-agents, models, fallback, retry | 47 | 17 | 21 | 7 | 1 | 0 | 0 | 1 | 0 |
| 10. Extensibility and unattended operation | 70 | 46 | 18 | 2 | 1 | 0 | 0 | 3 | 0 |
| **Total** | **767** | **431** | **228** | **38** | **18** | **4** | **39** | **8** | **1** |

How the rows were counted, and where the tables differ:

- The verdict column is free text. Each row is counted under the first verdict word. So "confirmed (corrected detail)", "confirmed with correction" and "confirmed (weaker)" count as confirmed; "refuted for retry, confirmed for fallback" and "refuted (app-server), corrected for exec-server" count as refuted; "refuted (not in code)" counts as refuted.
- Group 2 uses lower-case headers and is the only large group that marks undocumented behaviour as `new` (32 rows). Its source notes one re-labelling: Hermes Agent's `execute_code` row, "not in code" in the repository file, was entered as "refuted".
- Group 7 uses "unsettled" (counted as not settled) and "answered"; groups 3, 9 and 10 use "settled" for a Round 1 open question the code answered.
- Group 8 capitalises its verdicts and uses "Not found" and "Not in code read" (counted as not in code).
- Group 1 has one row, Cline's loop-detection default, with the verdict "docs and code disagree (read, not run)" (counted as other).
- A claim can appear in several groups. The 38 refuted rows are 26 distinct claims: Cline's `requires_approval` appears in three tables, goose's `--max-tool-repetitions` in three, and Codex's listener claim in three.

Overall, a little over half the checked claims held as written, about three in ten needed a correction, and one in twenty was contradicted by the code.

### Refuted

Every distinct Round 1 claim the code contradicts, most consequential for our design first.

| Project | What the documentation or Round 1 said | What the code shows | Permalink |
|---|---|---|---|
| Cline | The model sets `requires_approval` per command; no fixed allowlist | No such field in the SDK or the extension; only a legacy string and the docs page | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/tools/schemas.ts#L133-L157 |
| OpenCode | Denying `execute` removes Code Mode and the tools reachable only through it | The tools become direct tools | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool.ts#L265-L270 |
| OpenCode | The `explore` agent is read-only | `shell *` is allowed; read-only shell is prompt-only | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/agent.ts#L80-L93 |
| goose | Sub-agents are disabled in non-autonomous modes; default delegation timeout 5 minutes | The child inherits the mode and approvals are forwarded; no mode check; no sub-agent timeout | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/summon.rs#L416-L425 |
| Hermes Agent | `execute_code` is sandboxed beyond environment stripping | The local back end runs a host kernel; no sandbox | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/code_execution_tool.py#L787-L802 |
| Hermes Agent | Piping a URL into `sh` is on the hardline floor | It is an approvable dangerous pattern | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval_detection.py#L124-L157 |
| Codex | Non-loopback WebSocket listeners are unauthenticated by default "during rollout" | The app-server refuses to start without auth; the exec-server's `--listen` has no such refusal (so: refuted for the app-server, corrected for the exec-server) | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/websocket.rs#L160-L175 |
| Cline | `yoloModeAllowed: false` disables the YOLO toggle organisation-wide | No code reads it | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/shared/src/remote-config/schema.ts#L199-L201 |
| Cline | MCP entries have `autoApprove` tool lists | No such field in the SDK schema; approval goes by tool-name policy | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/mcp/config-loader.ts#L46-L56 |
| OpenHands | No plan mode | A read-only planner sub-conversation with three tools and `NeverConfirm` | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L1352-L1363 |
| Warp | "Agent decides" is the default for shell commands | GUI default is `AlwaysAsk`; "Agent decides" is the TUI default only | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/cloud_object_models/src/ai_execution_profile.rs#L399-L431 |
| goose | `--max-tool-repetitions` limits repeated tool calls | The flag is not wired; the inspector is registered with no limit | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/tool_monitor.rs#L99-L134 |
| Cline | "Spoke" workers run the loop, owned by the daemon | The hub runs the loop in-process; no spoke code | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/hub-server-transport.ts#L334-L343 |
| Cline | Checkpoints use a shadow Git repository separate from the project | Stash commits in the user's own repository under `refs/cline/checkpoints/...` | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hooks/checkpoint-hooks.ts#L624-L640 |
| Cline | A checkpoint commit after each tool use | One snapshot per user run, at the first model call | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hooks/checkpoint-hooks.ts#L658-L708 |
| Cline | Checkpoints work on folders that are not Git repositories | Not for the built-in snapshotter (a host callback can) | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hooks/checkpoint-hooks.ts#L515-L533 |
| DeepSeek harness | A fork cut inside an open turn is rejected | Accepted; the open turn is closed with `forked` closers | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/session/src/fork.ts#L21-L30 |
| Hermes Agent | Default turn cap 500 (sub-agents 50) | Unlimited by default; sub-agents 250 | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/config_defaults.py#L63-L66 |
| goose | No generic retry or fallback | A generic retry exists in the provider layer; no fallback chain | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-provider-types/src/retry.rs#L90-L158 |
| OpenCode | (Round 1 synthesis) per-agent fallback chains with a `model_fallback` event | None exists in OpenCode | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/model.ts#L81-L121 |
| OpenCodex | Cooldowns are memory-only | Combo cooldowns are persisted to `combo-cooldowns.json` | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/combos/cooldown-store.ts#L1-L25 |
| Oh My Pi | Every line is tagged with a short content hash | One 16-bit xxh32 tag per file; lines are bare numbers | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/store.rs#L70-L91 |
| OpenClaw | Workspace files include `TOOLS.md` and `HEARTBEAT.md` | Neither is in the injected set | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/workspace-bootstrap-policy.ts#L30-L37 |
| Oh My Pi | Ten model roles including `designer` | 16 built-in roles, no `designer` | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/config/model-roles.ts#L56-L91 |
| Cline | A notification fires after 30 s for auto-approved commands | The 30 s is the default timeout; no notification on the SDK path | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/tools/executors/bash.ts#L837-L843 |
| OpenCodex | Remote Workspace may be the `ocx-child` relay | `ocx-child` is Remote Link | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/native/ocx-child/src/lib.rs#L1-L20 |

### Corrected in ways that matter

Corrections that change a design conclusion, not a number.

| Project | What the documentation or Round 1 said | What the code shows | Permalink |
|---|---|---|---|
| OpenClaw | The node command gate is on the Gateway; the node decides | The node re-evaluates `system.run`, but a Gateway approval satisfies its allow-list; only a local deny holds | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/exec-policy.ts#L60-L117 |
| OpenClaw | Cloud workers are outbound-only | The worker dials out, but the Gateway SSHes into the box for bootstrap and sync | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/ssh.ts#L259-L291 |
| OpenClaw | GitHub publication through a Gateway broker; no GitHub credential on the worker | A turn-scoped GitHub token reaches the worker and its shell | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/worker/launch-descriptor.ts#L183-L211 |
| OpenClaw | The writer claim is checked in the same transaction as every transcript write | Only for fenced writes; the claim is last-writer-wins, not a lock | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/config/sessions/session-accessor.sqlite-transcript-write-snapshot.ts#L104-L130 |
| OpenClaw | An approval is bound to argv, cwd, env and the pinned executable | Also agent, session, node and device; env is a hash of the overrides only; "allow always" hashes only cwd and the arguments after the first | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/system-run-approval-binding.ts#L72-L90 |
| Codex | Hooks are trusted by hash; a changed hook needs review again | True for the declaration and plugin bundles; not for a script a repository hook calls | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/hooks/src/engine/discovery.rs#L972-L998 |
| Codex | The `explorer` role is read-only | `explorer.toml` is empty; no restriction | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/agent/role.rs#L316-L376 |
| Codex | Code Mode is under development and off by default | The flag is off, but the bundled catalog sets `code_mode_only` for most models | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/tools/mod.rs#L66-L76 |
| Codex (as cited in `harnesses-round-1.md`) | Our Landlock residuals (pathname sockets, UDP, DNS) "are why Codex rejects Landlock" | The code names only socket isolation from the app-server; the rest was our inference | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/linux_run_main.rs#L386-L393 |
| OpenCode | A durable per-session event log with sequence numbers | The code exists, but `persist` defaults to false and the shipped CLI never sets it | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/bus.ts#L214-L214 |
| OpenCode | Plan mode denies edits and asks before bash | Edit tools only; shell, sub-agents and `execute` allowed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/plan.ts#L33-L42 |
| OpenCode | A custom sub-agent uses only its own permissions | Its agent's rules plus a creation-time copy of the parent's session rules | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session.ts#L311-L314 |
| OpenCode | The web UI is password-protected by default | One shared secret, no accounts, also accepted as `?auth_token=` | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/middleware/authorization.ts#L13-L40 |
| Hermes Agent | The hardline floor holds even under YOLO, everywhere | Skipped on container back ends | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval.py#L1282-L1287 |
| Hermes Agent | Headless modes default to deny | True for cron, `-q`, API and webhooks; `-z` and a bare library context auto-approve | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval.py#L1300-L1308 |
| Hermes Agent | The iron-proxy covers remote and Docker sandboxes | Docker only; the agent's own model calls bypass it | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/docker.py#L786-L812 |
| DeepSeek harness | Approval resolution is a stage for every tool call | Reached only when a plugin returns `ask`; none does by default | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/tools/src/pipeline.ts#L197-L206 |
| DeepSeek harness | Sandbox back ends report `full` or `partial` enforcement | Computed and stored on the tool result, shown to nobody | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/shell/bash-sandbox/src/index.ts#L115-L124 |
| goose | The Code Mode JavaScript runtime is isolated | Isolated, but inner `tools.call` skips approval, inspectors and hooks | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/code_execution.rs#L172-L194 |
| goose | An opt-in macOS Desktop Seatbelt sandbox | Removed; only a blog post remains | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/documentation/blog/2026-02-23-goose-v1-25-0/index.md#L19-L34 |
| goose | `--container` runs extensions in Docker | Stdio and built-in extensions only; the Developer shell stays on the host | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/extension_manager/mod.rs#L932-L948 |
| goose | `--dangerously-unauthenticated` is for local use only | Nothing restricts it to loopback | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-cli/src/cli.rs#L1821-L1834 |
| Cline | No hub authentication is documented | A per-start token, or a loopback `Origin` that any local process can send | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/hub-websocket-server.ts#L676-L722 |
| Cline | `beforeTool` hooks can skip, rewrite or force approval; VS Code needs "Enable Hooks" | True for in-process hooks only; file hooks can only cancel the run and allow on every failure; VS Code hooks default on | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hooks/hook-file-hooks.ts#L415-L454 |
| Cline | `CLINE_PLUGIN_MODE=sandbox` restricts plugins | Crash isolation only | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/runtime/tools/worker-sandbox.ts#L267-L276 |
| OpenHands | `ConfirmRisky` is the default confirmation policy | Canvas sends `NeverConfirm` unless stored settings enable confirmation | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L777-L789 |
| OpenHands | Secret values stay out of the agent's reach | The session key, which can answer approvals, is in the agent's environment and secret store | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L975-L985 |
| Zed | (not documented) how tool permissions apply to external agents | A settings comment claims deny and confirm precedence; no code consults `tool_permissions` for ACP agents | https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent_servers/src/acp/v1.rs#L10172-L10225 |
| Warp | ACP support appears only in changelog wording | An ACP client is implemented, with an auto-approving permission policy | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/agent_sdk/driver/harness/acp/mod.rs#L1-L16 |
| OpenCodex | Remote Workspace is read-only by default; writing needs a command-sandbox probe | The probe gates `exec`; read and write are always offered; read-only is the hub's per-session default | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-device.ts#L372-L384 |

Two "new" rows, behaviour the documentation never mentioned, belong next to these because they change the same conclusions: Codex runs a command matched by an explicit `allow` rule outside the sandbox (https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/exec_policy.rs#L492-L512), and Oh My Pi merges project settings with no key filter, so a repository can loosen the approval mode (https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/config/settings.ts#L2800-L2814).

Our own design note `docs/nodes-credentials-environments.md` §7 cited two documentation-level patterns that the code changes: Cline's "key sent per session, never written" is confirmed only in part, with no lifetime on the remote found; "inference proxied by the control side" is confirmed for OpenClaw, Oh My Pi's gateway and OpenCodex, and OpenClaw also keeps a worker-local mode (see "Credentials, secrets and authentication" in `round2/synth/_parts/docs-vs-code.md`).

### Confirmed and now safe to build on

Documented mechanisms the code bears out, which our design leans on.

| Project | Mechanism | Permalink |
|---|---|---|
| Codex | Rules take the most restrictive decision across all matches | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/execpolicy/src/policy.rs#L577-L595 |
| Codex | A custom agent cannot override sandbox, approval policy, provider or MCP; the parent's policy is re-applied last | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/agent/child_config.rs#L197-L218 |
| Codex | Project trust gates project config, hooks and exec policies | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/config/src/loader/mod.rs#L1100-L1170 |
| Codex | Linux sandbox is bubblewrap then seccomp, two stages | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/linux_run_main.rs#L265-L321 |
| Codex | A command whose sandbox cannot be enforced is refused | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/process_sandbox.rs#L226-L230 |
| Codex | `.git`, `.agents`, `.codex`, `.aws` are protected | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/protocol/src/permissions.rs#L49-L54 |
| Codex | WebSocket auth happens at the HTTP upgrade; a bad `Origin` gets 403 | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/websocket.rs#L99-L130 |
| Codex | The daemon Unix socket is host-local: 0700 directory, 0600 socket | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/unix_socket.rs#L322-L352 |
| OpenClaw | Each dispatch mints a credential; only a domain-tagged hash bound to the turn claim is stored | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/credential.ts#L41-L60 |
| OpenClaw | Owner-epoch fencing rejects stale workers, revalidated per request | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/admission.ts#L188-L218 |
| OpenClaw | A closed, default-reject method allow-list on the worker route | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/gateway-protocol/src/schema/worker-admission.ts#L54-L66 |
| OpenClaw | Provider keys stay on the Gateway; workers request inference by model reference | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/inference-model.ts#L91-L124 |
| OpenClaw | Executable resolution re-checked before launch; drift denies | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/system-run-approval-binding.ts#L550-L620 |
| OpenClaw | Accepted input is stored before the Gateway acknowledges it | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/state/openclaw-agent-schema.sql#L897-L914 |
| Hermes Agent | `approvals.deny` runs before YOLO on all back ends and sees through `sudo`, `env`, `sh -c` | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval_detection.py#L1623-L1664 |
| Hermes Agent | Dangerous commands denied in cron by default | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/config_defaults.py#L1658-L1662 |
| Hermes Agent | A non-loopback dashboard fails closed without auth; `--insecure` is ignored | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/main_dashboard.py#L640-L662 |
| Hermes Agent | Stdio MCP servers get a safe environment baseline | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/mcp_tool_config.py#L199-L207 |
| Hermes Agent | A crashed turn is marked `unknown` and never replayed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_runtime.py#L453-L464 |
| DeepSeek harness | Approval fails closed and grants only once; `never` is enforced before any answerer | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/interaction/user-approval/src/index.ts#L293-L333 |
| DeepSeek harness | A failed audit append rejects the request | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/interaction/user-approval/src/index.ts#L224-L261 |
| DeepSeek harness | No sandbox back end means `SANDBOX_UNAVAILABLE`, never a pass-through | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/sandbox/sandbox-local/src/index.ts#L499-L524 |
| DeepSeek harness | One live writer per session (`flock`) | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/session/session-persistence-jsonl/src/lease.ts#L70-L116 |
| Oh My Pi | Hooks cannot bypass approval; policy re-resolved after a rewrite | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/wrapper.ts#L355-L374 |
| Pi | MCP calls, including nested code-mode calls, pass through the normal tool pipeline | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/agent-session.ts#L719-L742 |
| Pi | pi-env keeps credentials local; only the tools move | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/env/src/remote-env.ts#L55-L58 |
| OpenCode | Nested Code Mode tools enforce their own permissions | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/codemode/tool.ts#L126-L145 |
| Zed | Precedence: built-in, deny, confirm, allow, tool default, global default | https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/tool_permissions.rs#L208-L260 |
| Zed | Sandbox grants once, for the thread, or always | https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/sandboxing.rs#L292-L400 |
| OpenCodex | One-time pairing code; the hub stores only a hash of the bearer; rate-limited | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-hub.ts#L377-L412 |
| OpenCodex | Model scope checked against the resolved route, after aliases | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/admission-model-scope.ts#L71-L76 |

### What this says about reading documentation

The errors are not random. Five kinds of claim went wrong most often.

**Enforcement that is prompt-only.** Documentation describes a mode by its intent; the code often enforces it with a sentence in the system prompt. Codex plan mode (https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/tools/handlers/plan.rs#L87-L91) and its `explorer` role, OpenCode's `explore` agent and the shell in its plan mode, Hermes Agent's plan mode (https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/plan_prompt.py#L1-L27), DeepSeek harness's plan mode, and OpenCodex's "never falls back to the hub filesystem", which is an instruction plus the absence of a local tool path (https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-tools.ts#L212-L225). Cline's plan-mode command blacklist says in its own comment that `python -c` gets through.

**Defaults.** The documented default was wrong or applied to another surface: Hermes Agent's turn cap, Warp's "Agent decides", OpenHands' `ConfirmRisky`, Codex's Code Mode (off by flag, on by catalog), OpenCode's durable log (`persist` false), Cline's hooks (default on, https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/vscode/src/core/hooks/hooks-utils.ts#L6-L8), Cline's checkpoints (opt-in in the core, on in the CLI, https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/cli/src/runtime/defaults.ts#L17-L24) and Cline's loop detection (docs say off; the code runs it unless the setting is exactly `false`, read not run, https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/runtime/orchestration/session-runtime-orchestrator.ts#L1524-L1554).

**Described as on, but off or not wired.** goose's `--max-tool-repetitions`; Cline's `yoloModeAllowed` and `.clineignore`; goose's `GOOSE_CONTEXT_STRATEGY`, which no code reads (https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/state_machine/ops_compaction.rs#L105-L116); goose's argument-hashed permission store, which is dead code (https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/permission/permission_store.rs#L79-L115); DeepSeek harness's read-before-edit check, disabled in the shipped base bundle (https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/bundle/base/cordis.patch.yml#L292-L294); Cline's hub roles, which are a type no authorisation reads (https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/shared/src/hub.ts#L225-L230); DeepSeek harness's enforcement level, computed and shown to nobody.

**Features removed.** goose's Seatbelt sandbox; Cline's VS Code YOLO toggle (https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/vscode/src/hosts/vscode/vscode-to-file-migration.ts#L282-L313) and its model-set `requires_approval`; OpenHands' microagents, renamed to skills (https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/utils/skill-scope.ts#L11-L19). The documentation pages still describe them; Cline's docs page still describes the old flag (see "Edit tools and the mechanics behind the scenes" in `round2/synth/_parts/docs-vs-code.md`).

**Words that promise a property the code lacks.** `CLINE_SANDBOX` relocates the data directory; `CLINE_PLUGIN_MODE=sandbox` is crash isolation; Warp's `--sandboxed` is a flag the caller sets about itself (https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/warp_cli/src/agent.rs#L621-L623).

The 39 `new` rows add a sixth lesson: the most security-relevant behaviour was often undocumented rather than misdocumented. Codex's `allow` rule leaving the sandbox, OpenCode's HTTP session rules appended last (https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/handlers/session.ts#L101-L120), OpenHands' server-level bash (https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/bash-service/bash-service.api.ts#L24-L44), goose trusting `readOnlyHint` (https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/permission/permission_inspector.rs#L172-L176) and Warp launching other agents with their safety flags off (https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/pane_group/pane/local_harness_launch.rs#L120-L137) were all absent from the documentation.

**Reviewer-confirmed in Round 1, refuted by the code.** Round 1's independent reviewers re-opened the documentation URLs and confirmed that the documentation said what the survey reported. Seven claims they marked `reviewed` are contradicted by the code:

| Claim | Round 1 file where it is labelled `reviewed` | Round 2 refutation |
|---|---|---|
| Cline: the model sets `requires_approval`; a notification after 30 s | `round1/synth/01-loop-tools-editing-shell.md` (Cline `run_commands` row); `round1/synth/10-config-licence-distinctive.md` | see "Refuted" above |
| goose: sub-agents are disabled in non-autonomous modes | `round1/synth/02-permissions-sandbox-security.md` | see "Refuted" above |
| OpenCode: denying `execute` removes the tools reachable only through it | `round1/synth/01-loop-tools-editing-shell.md`; `round1/synth/06-extensibility.md` | see "Refuted" above |
| OpenCode: `explore` is read-only | `round1/synth/04-planning-subagents-background.md` | see "Refuted" above |
| Cline: a shadow Git repository commits after each tool use | `round1/synth/05-sessions-git.md`; `round1/synth/10-config-licence-distinctive.md` | see "Refuted" above |
| Hermes Agent: iteration budget default 500 | `round1/synth/07-models-observability.md` | see "Refuted" above |
| Oh My Pi: ten model roles including `designer` | `round1/synth/07-models-observability.md`; `round1/synth/04-planning-subagents-background.md` | see "Refuted" above |

The reviewers did their job: the documentation does say these things. The lesson is about what a documentation review can establish. It confirms what a vendor wrote, not what the program does, and the claims it confirmed most confidently were the ones about safety modes and defaults, which are exactly the claims the code contradicted most often. For anything our design copies from another harness, the evidence must be a line of code at a pinned commit.
