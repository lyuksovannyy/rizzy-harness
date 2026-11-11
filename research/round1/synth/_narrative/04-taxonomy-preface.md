## 4. Consolidated feature taxonomy

### How the 332 features are organised

The surveys used 23 areas. The synthesis grouped them into ten files, each holding the evidence tables for its features:

| File in `round1/synth/` | Areas | Features |
|---|---|---|
| `01-loop-tools-editing-shell.md` | 1-4: agent loop, built-in tools, editing, shell | 61 |
| `02-permissions-sandbox-security.md` | 5, 21: permissions, sandboxing, security and privacy | 34 |
| `03-context-memory.md` | 6, 7: context management, memory and instructions | 26 |
| `04-planning-subagents-background.md` | 8-10: planning, sub-agents, background and scheduled work, plus model slots | 42 |
| `05-sessions-git.md` | 11, 12: sessions, checkpoints, git | 29 |
| `06-extensibility.md` | 13: MCP, skills, hooks, plugins, SDKs, protocols | 35 |
| `07-models-observability.md` | 14, 19: models and providers, usage, cost, logs, telemetry | 25 |
| `08-ui-browser-collaboration.md` | 15, 16, 18: UI surfaces, browser and computer use, teams | 24 |
| `09-remote-distributed-architecture.md` | 17: remote, headless, CI, distributed execution | 33 |
| `10-config-licence-distinctive.md` | 20, 22, 23: configuration, licence, everything else | 23 |

### How to read a feature entry

Each feature in a synthesis file has the same shape:

- **A name and "What it is"**: one to three vendor-neutral sentences.
- **Variants**: the differences in mechanism that matter (who decides, where it runs, the default, what it fails to cover). Similar names often hide different mechanisms; this paragraph is where that is stated.
- **One row per harness**, each with its own mechanism, status label, evidence URL and check value. Evidence is never merged: six harnesses with the same feature have six rows and six URLs.
- **An "ours" row**, taken only from `round1/own-rizzy-harness.md` (branch `main`, cited as `path:line`), or "not in the inventory".

Each file also has an availability matrix (`Y` documented and released, `B` beta, experimental, preview or partial, `P` planned or vendor-claim only, `-` nothing documented, `?` unverified), and sections on what is common, what only one harness has, ours against the field, corrections, and Round 2 questions. Those sections are excerpted later in this report.

### Check values

| Value | Meaning |
|---|---|
| `reviewed` | A reviewer re-opened the cited page and found the claim as written. |
| `corrected` | A reviewer found a detail wrong (a default, a status, a scope) and the corrected text is the one used. |
| `unreviewed` | Nobody checked it. Most rows are in this state; reviewers sampled about a quarter of all entries. |
| `unverified` | The survey or the review could not confirm it on a primary page, often because a page was missing, truncated or only seen as a search snippet. |

### Status labels

`GA`, `beta`, `experimental`, `preview`, `deprecated`, `planned`, `unclear`, as the vendor's page states them. Where a page gives no label, the row says so ("GA (no label)" or "unclear"). Status labels were the most common thing reviews corrected (for example Claude Code agent view from GA to research preview, OpenClaw cloud workers from GA to beta), so treat an unreviewed status as a guess. For ours: `works`, `partial`, `stub`, `docs-only`, `none`, and "branch only" for work that exists only on an unmerged branch.

### "Nothing documented" is not "absent"

A `-` or "nothing documented" means the survey found no page describing the feature. It does not mean the harness lacks it. This is the weakest kind of evidence in the study: at least one such claim was wrong (goose's sandbox), and another hid a removal (OpenCode's session sharing was discontinued, not undocumented). Read counts such as "12 of 16" as lower bounds.

The full list of features, with a one-line definition each, follows.
