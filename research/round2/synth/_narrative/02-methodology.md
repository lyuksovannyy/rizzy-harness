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
