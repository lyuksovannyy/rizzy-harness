## 9. Limitations and research gaps

### What this round cannot tell us

**Documentation is not code.** Round 1 read public pages only, including for the open-source harnesses. Every mechanism in this report is "as documented". Where a vendor says a check is fail-closed, an approval is bound to argv, or a log has one writer, nobody has confirmed that the code does it. Docs also lag releases: Claude Code's weekly digest stops at Week 37 while the changelog runs on, OpenHands' release notes stop three versions behind GitHub, and several READMEs describe older behaviour than their docs (see "The pattern across files and reviews" in `round1/synth/_parts/overhyped.md`).

**Pages were mostly read through a summarising fetch tool.** Most surveys say so in their identity section (Antigravity, Cursor, goose, OpenClaw, OpenCode, OpenHands, Pi, Hermes Agent). The tool returns a model-written summary, not the page. Reviewers worked around it by asking for specific facts and quotes, but wording, defaults and numbers in unreviewed rows passed through a summariser once and were never compared with the raw page.

**Only about a quarter of entries were checked.** About 626 items were re-opened in review, against 2,393 entry headings (about 2,130 feature entries by the reviewers' own counts). The rest are `unreviewed`. Reviewers judged the unchecked remainder broadly reliable, with errors in small details (a default, a version, a cap) rather than invented features, but the check rate differs a lot per file: OpenCodex had 21 of about 33 entries checked, Claude Code 161 items against about 805 entries.

**"Nothing documented" claims are the least reliable evidence.** They mean a survey found no page. At least one was wrong: goose's "no sandbox documented" was not supported, since search snippets describe an opt-in macOS `GOOSE_SANDBOX` whose page now returns 404 (corrected in review, still unverified). Another hid a removal: OpenCode's session sharing was not undocumented but discontinued in v1.19.1. The goose reviewer asked for a second look at every "nothing documented" in that survey. Every count of the form "N of 16" in this report is a lower bound.

**Status labels were sometimes inferred.** Status was the most common correction: Claude Code agent view and background sessions from GA to research preview, OpenClaw cloud workers from GA to beta, Warp Factories to Early Access, OpenHands CLI from "deprecated" to "no longer actively maintained", Codex hooks from GA to no label. Where a page has no label, surveys sometimes wrote GA. Treat an unreviewed status label as a guess.

**Duplicate entries in the Claude Code survey disagree.** Produced by thirteen sub-agents, the file has several entries for one feature that differ: two dates for when project `defaultMode: bypassPermissions` started being ignored (v2.1.257 versus Week 36), an auto-mode classifier entry stale relative to a later one, and three conflicting idle-compaction entries. The synthesis merged them and took the reviewed values, but unreviewed duplicates may still carry the wrong one.

**Some summary sections were not updated after review.** Reviews corrected feature entries in place but often left the survey's own distinctive and relevance sections untouched. Known stale text: Claude Code's sections 3 and 5 still say each session gets a single-use work order (it is one runner per work order, with a 120-second spawn lease); Claude Desktop's section 5 still says the SSH helper survives the app quitting (the claim was removed); OpenClaw's section 5 still says cloud workers use an outbound-only connection (not stated by the page). The synthesis used the corrected versions; anyone reading the survey files directly should not.

**Changelog-derived dates and numbers are weak.** Version and date claims taken from changelogs were only partly reachable by reviewers (Claude Code's changelog and model-config pages were read to their first 100K characters only). Examples that did not hold: Claude Code idle compaction "about 54 minutes", Codex "WSL1 dropped at 0.115" and "GPT-6.1 Sol default in 0.161.0", the Codex presets 8,000-byte limit. Zed's checkpoint-removal dates were corrected in review.

**Closed products can only ever be described from their docs.** Claude Code, Claude Desktop, Cursor and Antigravity are closed; Warp's services and the Codex IDE extension, cloud and desktop are closed. Round 2's source reading cannot cover them. For Claude Code a few defaults can be printed by running the CLI (`claude auto-mode defaults` and similar).

### Documentation that was never opened

The important gaps, per harness, from the surveys' unverified sections and the review omissions:

| Harness | Not opened, truncated or unreachable |
|---|---|
| Codex | No tools reference page exists, so the full built-in tool list is unknown; notifications, computer history and Record and Replay pages; managed configuration seen as headings only; remote executors and `codex exec-server` (no page); the relay behind remote control is undescribed. |
| OpenCodex | The image and video bridge guides and the full provider matrix; the GitHub releases page returned 502, so only two release notes were read. |
| OpenCode | SCIM and workspace pages; V1 IDE and V1 Enterprise pages; the V2 config API page returned 404. |
| Claude Code | Most provider and gateway entries (Bedrock, Vertex, Foundry, spend limits, OpenTelemetry) were outside review 2's sample; the changelog and model-config page were read only in part; which model runs compaction and WebFetch extraction is not stated on the pages reached. |
| Claude Desktop | The last 25,000 characters of the main desktop page; memory import and export; MCP tunnels and MCP Apps detail pages (titles only); session sidebar, worktree defaults, PR monitoring and diff review were not checked. |
| DeepSeek harness | The last 25,000 characters of the generated tool catalogue (so default tool sets are unconfirmed) and the config catalogue (so compaction thresholds and the default sandbox mode are unknown). Several subsystem pages exist only on GitHub. |
| Oh My Pi | No docs pages for stats, SSH or `eval`; themes and key bindings not read; no changelog page in the sitemap. |
| Pi | No tools reference page located (edit matching rules unknown); `pi-chat` not opened; example extensions read by name only. |
| Cursor | SSO, SCIM, OpenTelemetry export and analytics API pages; audit logs and spend limits seen as index titles; Shared Transcripts, Origin, Voice Chat and Design Mode pages. |
| Antigravity | The pricing page and precedence tables; relay host and encryption for Remote Control and Remote Agents are not described. |
| OpenHands | Routing, fallback and subscription guides (index only); how approvals are presented in Canvas; whether the Enterprise server code is public and under what licence. |
| OpenClaw | The CHANGELOG; the managed-worktrees page was truncated (30K characters unread); sub-agent depth defaults subpage; embedding and Gateway-client pages. |
| goose | The sub-agents page returned a 391-byte stub; the sandbox page returns 404; the Developer extension's tool list and edit mechanism. |
| Hermes Agent | Remote config, multi-gateway, voice and webhook pages; the credential-pool page; the tail of the hooks page; the MCP config reference. |
| Zed/ACP | The hosted model list and prices; several ACP RFDs (usage, compaction, custom endpoint, proxy chains) seen as titles only. |
| Warp | The last parts of several `llms-txt` dumps; the ACP support claim (changelog wording only); worker and SDK licences. |
| Cline | SSO and member pages; the enterprise API reference; OpenTelemetry events and environment pages; what the hub dashboard shows; no browser tool page found. |

### Where our own "ours" statements are thin

- **Everything describes `main`.** The inventory was read at commit `a268048`. Pull request 4 and `feat/agent-roster` appear only in its "unmerged branches" section and in the synthesis as "branch only". This report states the branch position where it matters, but the synthesis files do not always.
- **Some synthesis files read only part of the inventory.** Context and memory (`03`) read inventory sections 4 and 5 plus keyword hits, so its "not in the inventory" means "not found in what was read". Configuration and licence (`10`) read sections 17 and 18 plus greps, and therefore reports our project licence as undefined; it is MIT. Planning (`04`), UI (`08`), models (`07`) and sessions (`05`) read named sections only. Permissions (`02`), extensibility (`06`) and remote (`09`) read the whole file.
- **Some synthesis text is stale against the branches.** `03-context-memory.md` calls audit item S12 (project members activating project facts) open; pull request 4 closes it by letting only a manager activate a shared project fact.
- **The inventory itself was not executed.** "Works" means code and tests exist, not that anything was observed working. It does not state which model runs history summarisation, and it does not measure checkpoint cost on a large repository.

### What is solid and what is not

Solid enough to act on now:

- **Execution-side authority is unique to us.** Every documented split hands policy to the control side; this rests on reviewed rows for Claude Code, Cursor, Warp and OpenClaw and on our own code.
- **Our table-stakes gaps are real.** No shell sandbox on `main`, no overflow-aware compaction, no provider retry, no steering, no web tools on agentic nodes: these come from our own code, and the field counts behind them are large enough that a few wrong "nothing documented" cells do not change the conclusion.
- **The field's own vendors say permission layers are not a security boundary.** These are reviewed quotes from Claude Code, Cursor, Oh My Pi, Cline, OpenClaw, OpenHands and Pi.
- **The credential pattern is moving to "workers never hold the model key".** Several independent, mostly reviewed rows (OpenClaw corrected to beta, Hermes corrected, Codex, Warp, Claude Code cloud).

Do not rely on these until Round 2:

- Any exact default, threshold or version from an unreviewed row, especially numbers from changelogs.
- Any "nothing documented" cell, and any count built from them.
- How strong a documented enforcement is in practice: OpenClaw's argv binding and its time-of-check gap, Codex's Linux sandbox profile and its network default, OpenCode's shell scanner, Hermes' hardline matcher, Codex's hook-trust hash. All are open source and settleable.
- Oh My Pi README claims (hashline benefits, round-robin credentials, typed results) and other vendor claims without a mechanism.
- Anything about Claude Code, Cursor or Antigravity that their docs leave unstated: wire protocols, VM isolation technology, classifier rules, relay encryption.
