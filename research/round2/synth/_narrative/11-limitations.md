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
