# Synthesis 02: permissions, approvals, sandboxing (Area 5) and security and privacy (Area 21)

Round 1, synthesis only. Date 2026-11-10. No web access was used; every fact below comes from the survey files named in Scope.

## 1. Scope

Areas covered: Area 5 (permissions, approvals and sandboxing) and Area 21 (security and privacy). Where a survey filed a security mechanism under another area (shell environment policy in Area 4, hooks in Area 13, remote links in Area 17, managed settings in Area 18, telemetry exporters in Area 19, repo-ignored keys in Area 20), it was read and used here because it belongs to these features.

Files and line ranges read:

| File | Lines read |
|---|---|
| `codex.md` | 100-147 (tools: `request_permissions`, web search), 169-200 (Area 4 env policy, user shell), 201-345 (Area 5), 616-645 (app-server auth, hooks), 794-841 (remote, cloud secrets), 854-881 (managed requirements), 884-897 (OTel, Statsig), 926-930, 942-958 (Area 21), 978-982, 986-1002 (s3), 1023-1041, 1050-1082 (s6, s7), 1159-1197 (review) |
| `opencodex.md` | 55-57, 139-160, 180-187, 195-202, 220-237, 298-330 (review) |
| `opencode.md` | 148-178, 303-307, 364-368, 433-439, 458-470, 492-512, 562-611 |
| `claude-code.md` | 532-1108 (Area 5, whole), 3575-3580, 3640-3651, 3718-3723 (Area 17), 4013-4036 (Area 18 managed settings), 4342-4345 (OTel events), 4583-4588 (repo-ignored keys), 4643-5054 (Area 21, whole), 5202-5340 (s3 to s7) |
| `claude-code.review-1.md`, `-2.md`, `-3.md` | whole (review-2 searched for the security-relevant rows) |
| `claude-desktop.md` | 70-101, 359-369, 428-440, 469-480, 503-528, 560-608 |
| `deepseek-harness.md` | 117-160, 313-318, 327-331, 371-381, 425-449, 453-464, 491-510, 533-547, 571-595, 616-678 |
| `oh-my-pi.md` | 125-153, 454-471, 510-527, 549-563, 584-610, 620-664 |
| `pi.md` | 121-152, 521-540, 561-573, 595-618, 662-698 |
| `cursor.md` | 73-108, 234-238, 273-277, 366-382, 427-440, 456-468, 500-523, 595-643 |
| `antigravity.md` | 78-137, 400-404, 438-457, 494-508, 531-559, 584-619 |
| `openhands.md` | 5, 91-122, 351-355, 429-442, 459-471, 493-516, 542-593 |
| `openclaw.md` | 102-169, 476-492, 548-573, 596-610, 633-660, 749-789 |
| `goose.md` | 84-139, 422-433, 488-501, 518-530, 552-578, 646-678 |
| `hermes-agent.md` | 1-12, 119-123, 143-216, 413-417, 589-593, 643-656, 679-693, 715-738, 739-782 (source list), 783-821 |
| `zed-acp.md` | 59-74, 173-178, 196-200, 261-310, 346-520 |
| `warp.md` | 121-160, 497-520, 546-559, 581-606, 633-671 |
| `cline.md` | 24-28, 69-73, 75-124, 369-399, 438-451, 476-488, 511-534, 593-630 |
| `own-rizzy-harness.md` | whole |

Evidence shorthand expanded (each survey defines its shorthand; the expansion below is the survey's own definition, not an invention):
- OpenHands `D:` = `https://docs.openhands.dev` (`openhands.md:5`), so `D:/sdk/guides/security.md` is written here as `https://docs.openhands.dev/sdk/guides/security.md`.
- Oh My Pi `R` = `https://github.com/can1357/oh-my-pi` (README) (`oh-my-pi.md:5`).
- Hermes Agent paths are relative to `https://hermes-agent.nousresearch.com/docs/` (`hermes-agent.md:3`), so `/user-guide/security` is written as `https://hermes-agent.nousresearch.com/docs/user-guide/security`.
- Codex `docs.openai.com`-style links are as written in the survey (`learn.chatgpt.com/docs/...`).

Could not read / limits: `claude-code.md` outside the line ranges above was not read (Areas 1-4, 6-16, 18-20 only where cited). Claude Code changelog-derived version claims remain only partly checked by its reviewers. Status labels marked "GA" in several surveys are the survey's inference where the vendor page carries none (reviewers say so for Antigravity, OpenClaw, Codex).

Check-column values: `reviewed` (the entry says verified/confirmed in review), `corrected` (corrected in review; corrected text used), `unreviewed`, `unverified` (the survey or review could not confirm it).

## 2. Features

### Area 5. Permissions, approvals and sandboxing

#### F1. Approval modes and the default mode

What it is: a named, session-wide setting that decides which tool calls run without asking, which ask a human, and which are refused. The default mode is what a new user gets without touching anything.

Variants: (a) prompt-by-default (Codex on-request with sandbox, OpenCode "no match means ask", Zed `confirm`, Cursor allowlist/auto-review, ours `ask`); (b) model-decides-by-default (Claude Code `auto` since v2.1.283 for interactive sessions, Cursor Auto-review recommended, Hermes `smart`, Cline model-set `requires_approval`); (c) run-everything-by-default (Oh My Pi `yolo`, goose Autonomous, Cline CLI `--auto-approve` true, OpenClaw host exec `security=full`/`ask=off`, Pi has no permission system at all). The deciding difference is whether the default is enforced by the harness or delegated to a model, and whether a sandbox sits behind the "no prompt" path.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Approval policies `on-request` (ask only to cross the sandbox boundary), `never`, and a granular policy toggling prompt categories; `untrusted` is retired as a policy and survives as a per-project `trust_level`. Onboarding recommends Auto (workspace-write plus on-request) for Git folders and read-only otherwise. | GA; `untrusted` retired | https://learn.chatgpt.com/docs/agent-approvals-security.md | reviewed |
| OpenCode | No named modes: an ordered rule list per agent with allow/deny/ask; no matching rule means ask. Base policy allows all, asks on reading `*.env`; shipped agents (plan, explore) add deny rules. | GA | https://opencode.ai/docs/permissions/ | reviewed |
| Claude Code | Six modes: `default` (Manual), `acceptEdits`, `plan` (reads, plus classifier-approved commands when auto mode is available), `auto` (classifier), `dontAsk` (deny anything that would prompt), `bypassPermissions`. Built-in starting mode is `auto` for interactive terminal and VS Code sessions from v2.1.283; Manual for `-p`/SDK (from v2.1.302), HIPAA orgs, or when any settings file disables auto. Project settings cannot start a session in auto or bypass. | GA | https://code.claude.com/docs/en/permission-modes.md, https://code.claude.com/docs/en/permissions.md | corrected (plan-mode classifier detail added by review-1) |
| Claude Desktop | Desktop UI modes Manual (default), Accept edits, Plan, Auto, Bypass; cloud sessions offer only Accept edits, Plan, Auto. Cowork has Manual, Auto (reviewer checks each action) and Skip; permanent deletion always needs explicit permission. | GA | https://code.claude.com/docs/en/desktop, https://support.claude.com/en/articles/13345190-get-started-with-claude-cowork | reviewed |
| DeepSeek harness | Permission presets bundle a sandbox mode with an approval policy: `workspace-write` (ask) and `danger-full-access` (never ask); `auto` is registered by the Auto review integration; approval policy is `ask` (default) or `never`. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/permission-presets, https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/approval | reviewed |
| Oh My Pi | `tools.approvalMode`: `always-ask`, `write`, `yolo`; the built-in default is `yolo` (nothing prompts); docs suggest `write`. | GA | https://omp.sh/docs/approvals | reviewed |
| Pi | Documented non-feature: no approval before tool calls, runs with the launching account's permissions; the substitute offered is a container or an extension-built gate. | GA (deliberate design) | https://pi.dev/docs/latest/security, https://github.com/earendil-works/pi | reviewed |
| Cursor | Run Modes: Auto-review (recommended; allowlisted calls run, others sandboxed if possible, else an LLM classifier decides), Allowlist, Run Everything. CLI `approvalMode` `allowlist`/`auto-review`/`unrestricted`. Cloud Agents never ask for approval. | GA | https://cursor.com/docs/agent/security/run-modes, https://cursor.com/docs/cli/reference/configuration | reviewed |
| Antigravity | App presets Request Review, Default (sandbox on, ask outside), Auto, Turbo (everything allowed); CLI modes `default`, `accept-edits`, `auto`, `plan` and a `toolPermission` setting whose value list and `request-review` default were not confirmed on the modes page. | GA | https://antigravity.google/docs/permissions, https://antigravity.google/docs/cli/modes | corrected |
| OpenHands | Confirmation policies `AlwaysConfirm`, `NeverConfirm`, `ConfirmRisky` (threshold HIGH, `confirm_unknown` true), settable per conversation over the API. The architecture page names ConfirmRisky the default; the guide page says the default is not stated. | GA | https://docs.openhands.dev/sdk/arch/security.md, https://docs.openhands.dev/sdk/guides/security.md | reviewed |
| OpenClaw | Exec `mode` deny/allowlist/ask/auto/full plus per-session modes `read-only`, `guarded` (Ask me), `workspace` (Auto-review), `full`. Default host exec is unrestricted (`security=full`, `ask=off`), stated as the intended single-operator default. | GA | https://docs.openclaw.ai/tools/exec-approvals, https://docs.openclaw.ai/gateway/permission-modes | reviewed |
| goose | Autonomous (default; no approval, can modify and delete files), Manual Approval, Smart Approval, Chat Only; per-session mode. Subagents only work in Autonomous. | GA | https://goose-docs.ai/docs/guides/managing-tools/goose-permissions.md | reviewed |
| Hermes Agent | `approvals.mode` `smart` (default: auxiliary LLM decides), `manual`, `off` (= `--yolo`); prompts time out after 300 s and expiry denies. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/security | reviewed |
| Zed/ACP | Zed Agent `agent.tool_permissions.default` confirm/allow/deny, default `confirm`. ACP itself standardises only the prompt (`session/request_permission`), not a mode. | GA | https://zed.dev/docs/ai/tool-permissions.md, https://agentclientprotocol.com/protocol/v1/tool-calls.md | reviewed |
| Warp | Per-action autonomy (Agent decides, Always ask, Always allow, Never) inside agent profiles; "Agent decides" is the CLI default for commands and reads; all "Always allow" is full autonomy. App default per permission not stated. | GA | https://docs.warp.dev/agents/capabilities/agent-profiles-permissions/ | reviewed |
| Cline | IDE per-category auto-approve toggles (prompting by default) and opt-in YOLO; CLI `--auto-approve` defaults to true (false in ACP mode); scheduled runs default to yolo. | GA | https://docs.cline.bot/features/auto-approve.md, https://docs.cline.bot/cli/cli-reference.md, https://docs.cline.bot/cli/scheduling.md | reviewed |
| rizzy-harness (ours) | `ask_all`, `ask` (default; mutating tools), `auto`; the effective mode is `min(requested, node max)` so a request can only tighten. Mutating status is per tool, so `ask` asks for every `bash` call including `ls`. | works | `crates/core/src/policy.rs:5-31`, `crates/node/src/policy.rs:226-232` | from own inventory |

#### F2. Per-tool and per-command permission rules

What it is: a rule language that allows, asks or denies specific tools, commands, paths, domains or MCP tools, with a defined precedence.

Variants: precedence is "deny beats ask beats allow regardless of specificity" in Claude Code, Antigravity, Codex profiles (deny > write > read) and Cursor CLI; "most restrictive wins" in Codex execpolicy; "last match wins" (with any-deny-denies across resources) in OpenCode; "ordered list, first match" in Antigravity SDK and Oh My Pi `bash.patterns`. Matching is on literal word prefixes (Antigravity, Codex), globs on the whole command text (Claude Code, Hermes fnmatch), regular expressions (Zed, Warp, OpenClaw `argPattern`) or resolved binary paths (OpenClaw). Only Claude Code documents parameter-level deny rules (`Tool(param:value)`) and explains why allow rules cannot use them.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Starlark `.rules` files with `prefix_rule(pattern, decision=allow/prompt/forbidden, justification, match, not_match)`; most restrictive decision wins; `match`/`not_match` are inline tests checked at load; `codex execpolicy check` tests rules. Separate beta permission profiles give per-path `read/write/deny` (deny > write > read, more specific path wins). MCP tools gated by annotations and per-tool `approval_mode`. | experimental (rules), beta (profiles) | https://learn.chatgpt.com/docs/agent-configuration/rules.md, https://learn.chatgpt.com/docs/permissions.md, https://learn.chatgpt.com/docs/extend/mcp.md | reviewed (rules), corrected (profiles) |
| OpenCode | Ordered `{action, resource, effect}` list; `*` matches anything including `/`; last match wins; across several resources any deny denies, else any ask asks. Agent rules append after global rules. Plugins can define new action names. | GA | https://opencode.ai/docs/permissions/ | reviewed |
| Claude Code | `allow`/`ask`/`deny` rules `Tool` or `Tool(specifier)`, evaluated deny then ask then allow, specificity never reorders. Bare-name deny removes the tool from context. Deny/ask (not allow) can match scalar parameters (`Agent(model:opus)`); primary content fields cannot be matched that way. Read/Edit rules use gitignore syntax with four anchors; WebFetch `domain:` rules; MCP `mcp__server__tool`. | GA | https://code.claude.com/docs/en/permissions.md | reviewed |
| Claude Desktop | Inherits Claude Code rules; Cowork applies whole-tool `Bash`/`WebFetch` deny rules to its workspace tools but not allow rules (noted in the Claude Code survey). | GA | https://code.claude.com/docs/en/permissions.md | unreviewed |
| Oh My Pi | Per-tool `tools.approval.<name>` allow/prompt/deny (deny always wins) and ordered `bash.patterns`; tools self-declare read/write/exec, undeclared custom tools are exec, MCP tools are write-tier. | GA | https://omp.sh/docs/approvals | reviewed |
| Pi | No built-in rules; extensions can block or mutate tool calls; example `permission-gate.ts`, `protected-paths.ts` and third-party permission packages exist. | GA (mechanism); rules third-party | https://pi.dev/docs/latest/extensions, https://pi.dev/packages | unreviewed |
| Cursor | CLI `Type(pattern)` rules in `allow`/`deny`: Shell (first token, `command:args`), Read, Write, WebFetch (domains), Mcp (`server:tool`); deny beats allow; only permissions can be set per project. IDE: plain-English `allow_instructions`/`block_instructions` tune the classifier. | GA | https://cursor.com/docs/cli/reference/permissions, https://cursor.com/docs/agent/security/run-modes | unreviewed (CLI), reviewed (IDE) |
| Antigravity | Resources `action(target)` (`read_file`, `write_file`, `read_url`, `execute_url`, `command`, `mcp`) in Deny/Ask/Allow lists; Deny > Ask > Allow; explicit rules beat presets. `command` matches literal word prefixes or anchored `regex:`. Allow rules also feed sandbox mounts and the network allowlist (App docs). SDK has ordered `allow`/`deny`/`ask_user` policies with undocumented multi-match semantics. | GA | https://antigravity.google/docs/permissions, https://antigravity.google/docs/sdk/policies | reviewed (rules), corrected (sandbox wiring is App-only) |
| OpenHands | No allow/deny rule language; `PatternSecurityAnalyzer` regexes return only a risk level. Cookbook PreToolUse hooks give command blacklists (heuristic, fail open). | GA (analyzers); cookbook examples | https://docs.openhands.dev/sdk/guides/security.md, https://docs.openhands.dev/cookbook/command-blacklist.md | reviewed / corrected |
| OpenClaw | Per-agent allowlist globs on resolved binary paths with optional regex `argPattern`; `safeBins` auto-allow stdin-only filters with per-binary flag profiles (`awk`, `sed`, `jq` always denied, interpreters must not be added). Approvals can only tighten config. | GA | https://docs.openclaw.ai/tools/exec-approvals, https://docs.openclaw.ai/tools/exec-approvals-advanced | reviewed |
| goose | Per tool Always Allow / Ask Before / Never Allow, overriding the mode, only in Manual and Smart modes; stored in `permission.yaml` and `permissions/tool_permissions.json`. | GA | https://goose-docs.ai/docs/guides/managing-tools/tool-permissions.md, https://goose-docs.ai/docs/guides/config-files.md | reviewed |
| Hermes Agent | `approvals.deny` fnmatch globs (applied before YOLO, to all backends) and a permanent `command_allowlist`; `hermes approvals suggest` mines history into proposals, never applies them, never proposes destructive classes. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/security | reviewed (deny), unreviewed (allowlist, suggest) |
| Zed/ACP | Per-tool `always_allow`/`always_deny`/`always_confirm` Rust-regex lists (case-insensitive); targets: command string, path, source and destination, URL, query, SKILL.md path, `mcp:<server>:<tool>`. Precedence: built-in rules, deny, confirm, allow, tool default, global default. | GA | https://zed.dev/docs/ai/tool-permissions.md | reviewed |
| Warp | Command allowlist (empty by default) and denylist (pre-populated with e.g. curl, wget, rm, eval), both regex; denylist matches always prompt and beat allowlist and "Always allow"; MCP allow/deny lists. | GA | https://docs.warp.dev/agents/capabilities/agent-profiles-permissions/ | reviewed |
| Cline | IDE per-category toggles (read project/all, edit project/all, safe/all commands, browser, MCP); SDK per-tool `toolPolicies` where unlisted tools are enabled and auto-approved. CLI has no built-in allow/deny list (use a PreToolUse hook). | GA | https://docs.cline.bot/features/auto-approve.md, https://docs.cline.bot/sdk/guides/permission-handling.md | reviewed |
| rizzy-harness (ours) | Only `denied_tools` (whole tools) in `node.toml`; no per-command, per-path or persistent allow/deny rules. | partial (`denied_tools`), none (rules) | `crates/node/src/policy.rs:9-105`, `crates/node/src/session.rs:957-975` | from own inventory |

#### F3. Shell-command parsing: chains, wrappers and hidden-execution constructs

What it is: how a command rule copes with `a && b`, pipes, subshells, `$(...)`, wrappers like `timeout`, and flags that run other programs (`git -c core.pager=...`).

Variants: three designs. (1) Split linear chains and require every segment to be allowed, but treat anything with substitution, variables, redirects or control flow as one opaque command needing an exact match (Codex tree-sitter, Antigravity, OpenClaw rejects substitutions and redirects outright). (2) Parse fully and look inside subshells and substitutions for deny rules (Claude Code, Hermes "sees through" `sudo`/`env`/`sh -c`). (3) No parsing: regex on the raw string (Warp) or first token only (Cursor CLI). OpenCode refuses to run unparseable input at all. Claude Code and Antigravity both document known bypasses (`git -C . push`, `/usr/bin/curl`).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `bash -lc` scripts split with tree-sitter only for plain linear chains (`&&`, or-operators, `;`, pipes); redirects, substitutions, variables, globs or control flow make it one opaque invocation. | experimental | https://learn.chatgpt.com/docs/agent-configuration/rules.md | reviewed |
| OpenCode | Built-in scanner for Bash, Zsh, POSIX sh, PowerShell extracts every subcommand and `cd` target; unparseable or dynamically evaluated commands error and do not run; directory inference is best effort. | GA | https://opencode.ai/docs/permissions/ | reviewed (shell list not re-checked) |
| Claude Code | Split on `&&`, or-operators, `;`, pipes, `&`, newlines; every subcommand must be allowed; deny/ask also apply inside subshells, command substitution, loop bodies. Fixed wrapper stripping (`timeout`, `nice`, `nohup`, flagless `xargs`...); env runners (`npx`, `docker exec`, `devbox run`) are not stripped. Exec wrappers and `find -exec/-delete` can never be prefix-approved. Commands over 10,000 characters or unparseable always prompt. Docs: a Bash rule is "not a security boundary" (`/usr/bin/curl`, `sh -c`, `git -C . push` slip past). Redirect and `tee` targets are checked as file writes. | GA | https://code.claude.com/docs/en/permissions.md | reviewed |
| Oh My Pi | `bash.patterns` deny/prompt rules match one segment of a compound command; allow must match the whole command. | GA | https://omp.sh/docs/approvals | reviewed |
| Cursor | CLI Shell rules match the first token (`command:args`); chain handling not documented. | GA | https://cursor.com/docs/cli/reference/permissions | unreviewed |
| Antigravity | `$(...)`, backticks, process substitution, brace expansion, non-literal command names, network/fd redirects and sub-command flags (`git -c core.pager=`, `tar --to-command`) disable prefix matching for the whole line; pipelines, chains, simple redirects and wrappers (`timeout`, `nohup`, `nice`, `env`) are prefix-matched per segment. `command(git)` lets `git status && git log` run; `git log $(whoami)` prompts. | GA | https://antigravity.google/docs/permissions | reviewed |
| OpenClaw | Chains need every segment allowed; redirections and command substitution are rejected; `allow-always` stores the inner executable behind `env`/`nice`/`timeout`. Self-stated gap: substitutions made during a remote approval wait for inner shell commands are not detected. | GA | https://docs.openclaw.ai/tools/exec-approvals-advanced | reviewed |
| Hermes Agent | Deny globs see through `sudo`, `env`, `nohup` and `sh -c` payloads; hardline floor fails closed on unparseable quoting; docs say the matcher is not a full shell interpreter. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/security | reviewed |
| Zed/ACP | Terminal chains (`&&`) are split and each part checked "across many shells" (shell list not re-checked in review). | GA | https://zed.dev/docs/ai/tool-permissions.md | reviewed |
| Warp | Regex on the command string; no parsing documented. | GA | https://docs.warp.dev/agents/capabilities/agent-profiles-permissions/ | reviewed |
| rizzy-harness (ours) | None: no command rules exist, so no parsing. | none | n/a (`own-rizzy-harness.md` section 3, "No per-command rules") | from own inventory |

#### F4. What an approval is bound to

What it is: when a user clicks "allow", what exactly is remembered: the tool, a command prefix, the exact argv, the working directory, the executable's identity, and for how long (once, turn, session, forever).

Variants: OpenClaw binds to exact argv, cwd, env and a pinned executable path (plus a content hash for writable executables) and re-checks resolution before launch: the strongest binding documented. Codex and OpenCode persist a prefix rule ("execpolicy amendment", "always" saves patterns). Claude Code saves per repository and command, but offers "don't ask again" only when the prompt can show everything the saved rule would allow. DeepSeek offers only allowed-once. ACP leaves the store to the agent. Ours binds to the tool name for one turn.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Command approvals return `accept`, `acceptForSession`, `decline`, `cancel` or `acceptWithExecpolicyAmendment` (approve and persist a prefix rule); requests carry thread and turn ids, reason, proposed amendment and network context. TUI approvals go to `~/.codex/rules/default.rules`. | GA | https://learn.chatgpt.com/docs/app-server.md | reviewed |
| OpenCode | `once`, `always` (saves the tool's proposed patterns as project-scoped allow rules, never overrides a configured deny), `reject` (rejects this and every other pending request in the session). | GA | https://opencode.ai/docs/permissions/ | reviewed |
| Claude Code | "Don't ask again" persists per tier: Bash per repository and command, WebFetch per repository and domain, file edits until session end; written to `.claude/settings.local.json` at the repo root (shared across worktrees). Approving a compound command saves one rule per subcommand (max 5). Offered only when the prompt shows everything the rule would allow. Auto-mode "Recently blocked" retries add no rule and are re-classified. | GA | https://code.claude.com/docs/en/permissions.md, https://code.claude.com/docs/en/auto-mode-config.md | unreviewed (tiers), reviewed (retry) |
| DeepSeek harness | Outcomes allowed-once, rejected, cancelled, unavailable; anything else than allowed-once denies; no "allow always" documented. The sandbox-widening retry must be granted for that exact call. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/approval, https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/shell | reviewed |
| Antigravity | In an Ask prompt for files or URLs the user can edit the target string (e.g. widen a file to its directory) before Allow; not for terminal commands. | GA | https://antigravity.google/docs/permissions | unreviewed |
| OpenClaw | `allow-always` entries bind to exact argv and cwd; approved node-host runs bound to cwd, argv, env (when present) and pinned executable path; the Gateway binds every resolved command-segment executable before review and re-checks before launch, a changed resolution denies. Cron "always allow" mints a standing grant tied to job, command, cwd, env (optional `grantExpiryDays`). | GA | https://docs.openclaw.ai/tools/exec-approvals, https://docs.openclaw.ai/tools/exec-approvals-advanced | reviewed |
| goose | Runtime decisions recorded in `permissions/tool_permissions.json` (managed automatically); granularity not stated. | GA | https://goose-docs.ai/docs/guides/config-files.md | reviewed |
| Hermes Agent | CLI choices once, session, always, deny; gateway yes/no replies or buttons. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/security | reviewed |
| Zed/ACP | Zed card offers once / always-for-tool / always-for-pattern (MCP tool-level only). ACP: options `allow_once`, `allow_always`, `reject_once`, `reject_always` are UI hints; the agent stores remembered decisions and should re-ask if the command or resolved target changes. | GA | https://zed.dev/docs/ai/tool-permissions.md, https://agentclientprotocol.com/protocol/v1/tool-calls.md | reviewed |
| rizzy-harness (ours) | "Allow for this session" lives in a local variable of one `run_turn`: it covers one turn and keys on tool name only, so one click allows every later `bash` command of that turn (AUDIT L10); label overstates it. Fixed only on unmerged `integration/gate3` (honoured only after a first approval, recorded node-side). | partial | `crates/agent-native/src/lib.rs:625`, `:1104-1122`, `web/src/components/ToolCard.tsx:234-236` | from own inventory |

#### F5. Model-based approvers and risk classifiers

What it is: a second model (or the main model) decides whether an action runs, instead of or before a human.

Variants: who decides (a separate reviewer agent: Codex, Claude Code, Cursor on its backend, goose, OpenClaw, Hermes auxiliary LLM; the acting model itself rating its own action inline: OpenHands `security_risk`, Cline `requires_approval`, Warp "Agent decides"); what it sees (Claude Code strips tool results; Codex excludes hidden reasoning; goose sees the task, recent messages and the call); what happens on failure (fail closed: Codex, Claude Code server review, Hermes timeout; fail open: goose); circuit breakers (Codex 3 consecutive or 10 of 50 denials; Claude Code 3 consecutive or 20 total; OpenClaw 3 consecutive denials escalate to a human). Cursor, Claude Code and goose state in their docs that this is not a guarantee or not a security boundary.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `approvals_reviewer = "auto_review"` sends eligible requests to a separate reviewer agent that sees a compact transcript and the exact request, may make read-only checks, never sees hidden reasoning. Prompt-build, session and parse failures fail closed. Turn interrupted after 3 consecutive or 10-of-50 denials (first denial for cyber models). `/approve` grants one retry of one exact denied action, re-reviewed. Policy local or managed. | GA (desktop availability varies) | https://learn.chatgpt.com/docs/sandboxing/auto-review.md, https://learn.chatgpt.com/docs/agent-approvals-security.md | reviewed |
| Claude Code | Auto mode: a separate classifier model reviews actions after rules resolve; read-only actions and working-dir edits auto-approve; broad code-exec allow rules are dropped while in auto. Client-side requests strip tool results; a server-side probe scans incoming tool results. Prose rule tiers `hard_deny`/`soft_deny`/`allow` plus `autoMode.environment` trusted-infrastructure entries, read only from user/managed/`--settings`. Pauses to prompting after 3 consecutive or 20 total blocks. Server-side review folded into the main model request; no verdict means deny. Docs: "does not guarantee safety"; a developer `allow` can override an org `soft_deny`, so not a hard policy boundary. | GA (server review in staged rollout) | https://code.claude.com/docs/en/permission-modes.md, https://code.claude.com/docs/en/auto-mode-config.md | reviewed |
| Claude Desktop | Desktop Auto mode (Claude Code classifier, needs a recent model) and Cowork Auto (a reviewer checks each action and blocks unsafe ones). | GA | https://code.claude.com/docs/en/desktop, https://support.claude.com/en/articles/13345190-get-started-with-claude-cowork | reviewed |
| DeepSeek harness | "Automatic authorization review" integration answers approval requests and registers the `auto` preset; reviewer model, rules and prompt not documented. | experimental | https://deepseek.com/harness, https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/permission-presets | reviewed (existence); mechanism unverified |
| Cursor | Auto-review: an LLM classifier on Cursor's backend (Gemini 3.5 Flash Lite, fallback Claude 4.5 Haiku) decides non-allowlisted, non-sandboxable calls; may run read-only ReadFile/Grep/Glob/ListDir on the user's machine. Docs say it is not a security boundary. | GA | https://cursor.com/docs/agent/security/run-modes | reviewed |
| Antigravity | Auto mode evaluates commands that must leave the sandbox: safe, reversible ones (build, test, lint) run, risky ones prompt; review includes repository status (changelog v1.4.9). Mechanism (separate model or heuristics) not stated. | GA in later versions; "Preview" label and business unavailability unverified | https://antigravity.google/docs/permissions, https://antigravity.google/docs/changelog | unverified (status detail) |
| OpenHands | `LLMSecurityAnalyzer` adds a required `security_risk` argument to non-read-only tool schemas so the acting model rates each action inline; Pattern, PolicyRail and Ensemble analyzers add defence in depth; analyzers return only a risk level, no hard deny; `conversation.execute_tool()` bypasses analysis. Pages disagree on whether a separate LLM is also used. | GA, documented limits | https://docs.openhands.dev/sdk/guides/security.md, https://docs.openhands.dev/sdk/arch/security.md | reviewed |
| OpenClaw | Exec `mode=auto` and session mode "Auto-review": an LLM reviewer grants, denies or escalates; three consecutive denials escalate to a human; login/interactive shell wrappers skip the reviewer and need a human. | GA | https://docs.openclaw.ai/gateway/permission-modes, https://docs.openclaw.ai/tools/exec-approvals | reviewed |
| goose | Smart Approval (criteria not described). Adversary mode: a reviewer on the configured model sees the task, recent messages and the call, returns ALLOW/BLOCK against `~/.config/goose/adversary.md`; only `shell` reviewed by default; reviewer errors fail open; enabled by the file existing. | unclear (no label) | https://goose-docs.ai/docs/guides/security/adversary-mode.md | reviewed |
| Hermes Agent | `smart` default: an auxiliary LLM auto-approves low risk, auto-denies dangerous, escalates uncertain; hardline floor and deny globs apply first. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/security | reviewed |
| Warp | "Agent decides": the agent chooses whether to ask; for diffs it behaves like "Always ask". | GA | https://docs.warp.dev/agents/capabilities/agent-profiles-permissions/ | reviewed |
| Cline | The model sets `requires_approval` per command; no fixed allowlist; safety is model-judged. | GA | https://docs.cline.bot/features/auto-approve.md | reviewed |
| rizzy-harness (ours) | Judge-model auto-approval designed in PLAN; no code. | docs-only | `PLAN.md:143-152` | from own inventory |

#### F6. Hardcoded floors no mode can lift

What it is: a fixed set of commands or actions that stay prompted or refused even in the most permissive mode.

Variants: Hermes and Claude Code keep their floor even under YOLO/bypass; Oh My Pi skips its critical-pattern checks in yolo; Warp's user denylist is bypassed by "Run until completion" by default (only the admin denylist holds). Claude Code's floor is the most detailed (critical-path `rm` with quoting-aware expansion inside `sh -c`); Zed's is deliberately tiny.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | "Actions no mode auto-approves": explicit ask rules, org `ask` connector tools, `AskUserQuestion` and `requiresUserInteraction` MCP tools, critical-path `rm`/`rmdir` removals (root, home, working dir and parents, variables that may be empty, detection inside subshells and `sh -c`), "Dangerous rm operation not ruled out" for unparseable commands. Two-minute countdown in auto/bypass terminal sessions. An opt-out env var exists only from the launch environment. Docs: the check can miss commands and is not an OS control. | GA | https://code.claude.com/docs/en/permission-modes.md | reviewed |
| Oh My Pi | Built-in bash critical-pattern checks (recursive deletion of absolute paths, curl-to-shell, shutdown, sensitive account files), skipped in yolo. Provider-originated computer-use safety checks always need interactive confirmation, even in yolo. | GA | https://omp.sh/docs/approvals | reviewed (patterns), unreviewed (provider checks) |
| Cursor | Browser Protection, File-Deletion Protection and External-File Protection can force approval even in auto modes. | GA | https://cursor.com/docs/agent/security/run-modes | unreviewed |
| Hermes Agent | Hardline blocklist refused even under YOLO/off with no override flag: `rm -rf /` variants, fork bomb, `mkfs` on mounted root, `dd` to physical disks, piping untrusted URLs into `sh`. Supervised-gateway guard stops the agent restarting its own gateway. MCP elicitation and per-chat `spend_limit` also cannot be skipped by YOLO. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/security, https://hermes-agent.nousresearch.com/docs/user-guide/features/mcp | reviewed |
| Zed/ACP | Hardcoded protection only for recursive forced delete of `/`, home, cwd, parent; applies to the terminal tool only. | GA | https://zed.dev/docs/ai/tool-permissions.md | reviewed |
| Warp | Admin-set denylist entries can never be bypassed; the user denylist is bypassed by the "Run until completion" shortcut by default (a setting turns that off). | GA | https://docs.warp.dev/agents/capabilities/agent-profiles-permissions/, https://docs.warp.dev/knowledge-and-collaboration/admin-panel/ | corrected |
| rizzy-harness (ours) | None documented. | none | not in the inventory | from own inventory |

#### F7. Protected paths inside writable areas

What it is: paths that stay write-protected even where the agent may otherwise write, because writing them turns into code execution or policy change (git hooks, agent config, shell rc files).

Variants: enforced by the OS sandbox (Codex, Claude Code sandbox list, Cursor, Zed, Antigravity), by the permission layer only (Claude Code permission-level list, ours for file tools), or both (Claude Code keeps two separate lists). Claude Code also deletes new top-level `HEAD`/`objects`/`refs` after each sandboxed command to stop a crafted bare repo.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Under `workspace-write`, `<root>/.git` (including a resolved gitdir), `.agents`, `.codex`, `.aws` stay recursively read-only. | GA | https://learn.chatgpt.com/docs/agent-approvals-security.md | reviewed |
| Claude Code | Permission-level list (`.git`, `.vscode`, `.idea`, `.husky`, `.claude` with exceptions, shell rc, `.gitconfig`, `.npmrc`, `.mcp.json`, settings files...) never auto-approved except in bypass. Separate sandbox-enforced list (settings, `.claude/hooks`, `agents`, `skills`, `.git/hooks`, `.git/config`, bare-repo files, most of `~/.claude`) that allow rules cannot override. | GA | https://code.claude.com/docs/en/permission-modes.md, https://code.claude.com/docs/en/sandboxing.md | reviewed |
| Cursor | Sandbox denies `.git/config`, `.git/hooks`, `.vscode`, `.cursorignore`. | GA | https://cursor.com/docs/agent/security/run-modes | corrected |
| Antigravity | Sandbox blocks `~/.ssh` and `.env`; changelog notes "protect more credentials" and plugin OAuth token protection. | GA | https://antigravity.google/docs/sandbox | corrected |
| Zed/ACP | Sandbox: git metadata never writable. ACP advises clients to ask before writing files that control execution or approval (hooks, settings, MCP config). | unclear (sandbox); GA (ACP guidance) | https://zed.dev/docs/ai/sandboxing.md, https://agentclientprotocol.com/protocol/v1/file-system.md | reviewed |
| rizzy-harness (ours) | `write`/`edit` refuse `.git/**` and do not follow a final symlink; the data directory is refused by every file tool in every mode. Not enforced for `bash` on `main` (bash can read the data dir). | partial | `crates/tools/src/lib.rs:111-181`, `crates/tools/src/fs.rs:222-262` | from own inventory |

#### F8. Workspace boundary for file tools

What it is: the folder set the agent's built-in file tools may touch without asking, and what happens outside it.

Variants: prompt outside (Claude Code Manual, Cursor Workspace read mode, Antigravity `allowNonWorkspaceAccess` off), refuse outside in every mode (Claude Code `block*OutsideWorkingDirectories`, ours unless `allow_outside_workspace`), allow outside by default (OpenCode V2 `external_directory`, Cursor read "System" default, Pi). Claude Code's docs and Claude Desktop's local-access page both say the boundary covers file tools, not shell commands, unless the sandbox enforces it.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `workspace-write` covers cwd and temp dirs; `writable_roots` adds directories. | GA | https://learn.chatgpt.com/docs/sandboxing.md | corrected |
| OpenCode | `external_directory` checked before read/edit outside the project; allowed by default in V2. | GA | https://opencode.ai/docs/permissions/ | reviewed |
| Claude Code | Launch directory plus `--add-dir`/`additionalDirectories`; `blockReadsOutsideWorkingDirectories` / `blockWritesOutsideWorkingDirectories` (v2.1.314+, file tools only) refuse outside paths in every mode including bypass. Security page: an approved Bash command can write anywhere; OS enforcement needs the sandbox. Symlinks: both requested and resolved paths checked; TOCTOU re-check at open. | GA | https://code.claude.com/docs/en/permissions.md, https://code.claude.com/docs/en/security.md | reviewed |
| Claude Desktop | Cowork access scoped to folders the user attaches; resolved paths checked so symlinks and `..` cannot escape; only the user may attach a network drive; admins set `allowedWorkspaceFolders` with per-folder rw/ro, read-only enforced for file tools only. | GA | https://claude.com/docs/third-party/claude-desktop/local-access.md | corrected |
| Pi | The working folder does not restrict access (documented). | GA (non-feature) | https://pi.dev/docs/latest/security | reviewed |
| Cursor | "Read access" (3.23+): System (default) reads outside the workspace; Workspace requires approval outside it and hides non-workspace paths from sandboxed commands; admins can enforce Workspace. | GA | https://cursor.com/docs/agent/security/run-modes | reviewed |
| Antigravity | `allowNonWorkspaceAccess` default off; Request Review preset is workspace-only; Turbo gives the full filesystem. | GA | https://antigravity.google/docs/permissions | corrected |
| OpenClaw | Session permission modes bound to the session root or workspace; sandbox `workspaceAccess` none/ro/rw (values on an unchecked subpage). | GA | https://docs.openclaw.ai/gateway/permission-modes, https://docs.openclaw.ai/gateway/sandboxing | reviewed (modes), unreviewed (values) |
| Warp | Admin directory allowlist where agents may read without asking (Business and Enterprise). | GA | https://docs.warp.dev/knowledge-and-collaboration/admin-panel/ | unreviewed |
| Cline | Separate toggles "read/edit project files" and "read/edit all files". | GA | https://docs.cline.bot/features/auto-approve.md | reviewed |
| rizzy-harness (ours) | Session cwd must be inside a `workspace_roots` entry (default `$HOME`) and must not contain or sit inside the data dir, rechecked before every prompt; file tools walk paths component by component with lstat, refuse dangling or escaping symlinks. Not enforced for `bash`. | works | `crates/node/src/policy.rs:155-215`, `crates/tools/src/lib.rs:111-181` | from own inventory |

#### F9. Administrator and managed policy layers

What it is: a policy source that outranks the user's own settings, set by an organisation or machine administrator.

Variants: delivery by MDM/OS policy and system files (Codex, Claude Code, OpenCode, Hermes `/etc/hermes`), by a vendor console polled over the network (OpenCode Console about once a minute, Claude Code hourly, Cursor team dashboard, Warp Admin Panel, Cline remote config), or by a self-hosted config server (Codex `config_server`, Claude apps gateway). OpenCode's policies "only tighten"; Claude Code adds strict-value-wins ratchets where a lower scope may tighten a managed value. Claude Code explicitly says server-managed settings are a client-side control, not a security boundary. Cline's remote config does not reach CLI or Desktop. Ours inverts the direction: the node's own file is the authority over the central server.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `requirements.toml`/MDM/Agent Security enforce allowed approval policies, sandbox modes, reviewers, profiles (omitted ones denied, including future built-ins), web search modes, deny-read paths, command rules, managed hooks, network requirements; MDM > Agent Security > system file. Optional self-hosted `[config_server]` over HTTPS. | GA | https://learn.chatgpt.com/docs/enterprise/managed-configuration.md, https://learn.chatgpt.com/docs/enterprise/configuration-server.md | unreviewed |
| OpenCode | Separate `policies` (`provider.use`, `tool.use`, `mcp.use`, `plugin.use`...) that only tighten (a policy allow can lift an earlier policy deny but grants nothing on its own); macOS managed prefs > system file > Console > global/project files; a denied `tool.use` overrides "Allow always". Not a sandbox for plugin code. | GA | https://opencode.ai/docs/policies/ | corrected |
| Claude Code | Managed settings via server (hourly), MDM (30 min), files, HKCU fallback; first-wins or merge; `allowManagedPermissionRulesOnly`, `disableBypassPermissionsMode`, `disableAutoMode`, sandbox locks, MCP and marketplace controls, `allowManagedHooksOnly`. Strict-value-wins exceptions let lower scopes tighten some keys. Server-managed settings are "not a security boundary" (user can edit cache, run old binaries); MDM recommended. | GA | https://code.claude.com/docs/en/managed-settings.md, https://code.claude.com/docs/en/settings.md, https://code.claude.com/docs/en/server-managed-settings.md, https://code.claude.com/docs/en/admin-setup.md | reviewed |
| Claude Desktop | Team/Enterprise control Bypass and Auto through org policy (`disableBypassPermissionsMode`, `disableAutoMode`); 3P admins set `allowedWorkspaceFolders`; Cowork skip-all-approvals follows the org setting. | GA | https://code.claude.com/docs/en/desktop, https://claude.com/docs/third-party/claude-desktop/local-access.md | reviewed |
| Cursor | Team dashboard config overrides user and project `permissions.json`; team policy and hardcoded rules win over `sandbox.json`; admins can enforce Workspace read mode; enterprise MDM hooks. | GA | https://cursor.com/docs/agent/security/run-modes, https://cursor.com/docs/hooks | reviewed |
| Antigravity | Business/enterprise accounts use "previous settings" instead of presets; Remote Control disable-able by policy. | GA | https://antigravity.google/docs/permissions, https://antigravity.google/docs/remote-control | unreviewed |
| OpenHands | Agent Profiles scope secrets (all/none/selected, server-enforced); RBAC and SAML are Enterprise/Cloud only. | GA | https://docs.openhands.dev/openhands/usage/agent-canvas/agent-profiles.md | unreviewed |
| OpenClaw | Operator scopes and named roles with scope ceiling, sandbox inherit/required and model policy; docs call scopes a guardrail within one trust domain, not tenant isolation. | GA | https://docs.openclaw.ai/gateway/operator-scopes | reviewed |
| goose | `GOOSE_ALLOWLIST` URL to an admin-hosted YAML; extension install rejected unless its command matches exactly (HTTPS-only from v1.57.0). Gates install, not runtime. | GA (no label) | https://goose-docs.ai/docs/guides/allowlist.md | reviewed |
| Hermes Agent | Root-owned `/etc/hermes` `config.yaml`/`.env` override user files per key; enforcement is filesystem permissions only; no signed files or MDM; subprocesses can set different env values. | GA, v1 | https://hermes-agent.nousresearch.com/docs/user-guide/managed-scope | unreviewed |
| Warp | Admin Panel enforces Apply Diffs, Create Plans, Execute Commands, Read Files, command lists and a non-bypassable denylist team-wide. | GA (Business, Enterprise) | https://docs.warp.dev/knowledge-and-collaboration/admin-panel/ | unreviewed |
| Cline | Remote config gives VS Code/JetBrains rules, providers, MCP controls, telemetry lock; CLI gets only rules and workflows; Desktop none. YOLO enterprise control: one page says all interfaces, the matrix says none. | GA | https://docs.cline.bot/enterprise-solutions/configuration/remote-configuration/overview.md | reviewed |
| rizzy-harness (ours) | `node.toml` on each node is the authority (unknown keys rejected; `max_approval_mode`, `workspace_roots`, `denied_tools`, `allow_outside_workspace`, `env_pass`...); no HTTP or protocol endpoint writes it. On the master, per-user `can_agentic` plus workspace role `agentic`, embedded node admin-only by default. No org-wide policy push. | works | `crates/node/src/policy.rs:9-105`, `crates/master/src/workspaces.rs:18-75`, `crates/master/src/config.rs:149-165` | from own inventory |

#### F10. Repository config cannot widen policy

What it is: settings committed in a cloned repository (or created by the agent inside it) must not grant themselves more access.

Variants: ignore specific keys from repo files (Codex blocklist, Claude Code repo-ignored keys and `autoMode` exclusion, Claude Code `CLAUDE_CODE_ENABLE_CFC` ignored), defer repo keys until the user trusts the folder (F28), or refuse to start (DeepSeek refuses if a project `.env` sets proxy variables). Oh My Pi loads everything from the repo by default.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Project `.codex/config.toml` cannot set `openai_base_url`, `chatgpt_base_url`, `model_provider(s)`, `notify`, `profile(s)`, `otel`, the realtime base URL or `apps_mcp_product_sku`; ignored with a warning. Untrusted projects skip project `.codex/` layers. | GA | https://learn.chatgpt.com/docs/config-file/config-advanced.md | corrected |
| Claude Code | Project files cannot set `autoMode`, `defaultMode` auto/bypass, `skipDangerousModePermissionPrompt`, `sandbox.filesystem.disabled`, mask/TLS settings, OTel exporters, config-dir env vars and more; some variables are read only from the launch environment. A repo `.mcp.json` cannot expand credential env vars into a remote server URL (F22). | GA | https://code.claude.com/docs/en/settings.md, https://code.claude.com/docs/en/settings-reference.md, https://code.claude.com/docs/en/auto-mode-config.md | reviewed (autoMode), unreviewed (env list) |
| DeepSeek harness | A project `.env` cannot set proxy variables (DSH refuses to start). | preview | https://deepseek-harness.github.io/deepseek-harness/en/guide/network-proxy | unreviewed |
| Oh My Pi | Opening a repo loads its settings, extensions, hooks, tools, commands, skills, rules and MCP config; only narrowing flags exist, no trust prompt. | GA (as flags) | https://github.com/can1357/oh-my-pi | unreviewed |
| rizzy-harness (ours) | The agent reads no repository config or instruction file at all, and protocol/HTTP cannot widen `node.toml`; git for checkpoints runs with cleared env and no system/global config. | works (by absence) | `crates/node/src/policy.rs:9-105`, `crates/node/src/git.rs:41-60` | from own inventory |

#### F11. OS sandbox for agent shell commands

What it is: kernel-enforced confinement (filesystem, network, sometimes process) of the commands the agent runs.

Variants per platform: macOS everyone uses Seatbelt (`sandbox-exec`). Linux: bubblewrap plus seccomp (Codex, Claude Code with `socat` relay, Zed, Antigravity per its enterprise page), Landlock plus seccomp (Cursor; DeepSeek offers both bwrap and Landlock), and Codex explicitly rejects Landlock for filesystem-restricted policies because it cannot isolate app-server Unix sockets. Windows: Codex has MXC and two legacy backends, Antigravity AppContainer (CLI only), DeepSeek ACL/restricted-token; Claude Code and Zed have nothing native. Coverage: only spawned shell (Claude Code, Cursor, Codex), shell and fetch (Zed), or the whole process (Claude Code `srt`, beta). Default: on (Antigravity App, Cursor default run mode uses it when possible), off (Claude Code, Antigravity CLI).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Modes `read-only`, `workspace-write` (default for local work), `danger-full-access`, applying to spawned commands. macOS Seatbelt plus curated platform policy; Linux/WSL2 bubblewrap plus seccomp (first `bwrap` on PATH or bundled helper needing unprivileged user namespaces; AppArmor steps for Ubuntu 24.04); Landlock rejected for filesystem-restricted policies. Windows MXC (Windows 11 24H2/25H2), legacy `elevated` (low-privilege users, ACLs, firewall) and `unelevated`. `codex sandbox` runs a command under the same policy. Network-off default for workspace-write was not confirmed. | GA | https://learn.chatgpt.com/docs/sandboxing.md, https://learn.chatgpt.com/docs/permissions.md, https://learn.chatgpt.com/docs/windows/windows-sandbox.md | corrected (network default, WSL1 claim removed) |
| OpenCodex | Remote Workspace executor runs commands inside bubblewrap on Linux only; write needs a command-sandbox probe. | experimental | https://opencodex.me/guides/remote-workspace/ | reviewed |
| OpenCode | Nothing documented in the V2 pages opened; safety is rule- and scanner-based. | nothing documented | https://opencode.ai/docs/permissions/ | unreviewed |
| Claude Code | Off by default. Wraps Bash, PowerShell, Monitor and children: macOS Seatbelt; Linux/WSL2 bubblewrap plus `socat` relay (optional seccomp for Unix sockets); WSL1 and native Windows unsupported. Default: writes to working dir, per-user temp and added dirs; reads most of the machine including `~/.ssh`; network only via local proxy with empty allowlist; environment inherited including secrets. Hooks, MCP servers, LSP, mods, `!` commands and built-in file/web tools run outside it. `srt` (`@anthropic-ai/sandbox-runtime`) wraps the whole process (beta research preview). Docs: "reduces risk but is not a complete isolation boundary". | GA (sandbox), beta (`srt`) | https://code.claude.com/docs/en/sandboxing.md, https://code.claude.com/docs/en/sandbox-environments.md | reviewed |
| Claude Desktop | Cowork shell commands run in a hardened local VM (Linux beta QEMU+KVM; Windows OS virtualization) or in cloud mode; Code tab uses Claude Code's sandbox where supported. | GA, partial platform coverage | https://code.claude.com/docs/en/desktop-linux, https://claude.com/docs/third-party/claude-desktop/overview.md, https://claude.com/docs/third-party/claude-desktop/local-access.md | corrected (two Cowork modes) |
| DeepSeek harness | Filesystem-only confinement `read-only`, `workspace-write`, `danger-full-access`; Linux bwrap and Landlock, macOS Seatbelt, Windows ACL/restricted token, SSH remote backend with the same policy. Network and process visibility explicitly outside the sandbox vocabulary. Backends report `full` or `partial` enforcement. Default mode not stated. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/sandbox | reviewed |
| Oh My Pi | No OS sandbox; subagent filesystem clone/overlay isolation is "not a security sandbox"; docs advise a restricted account or container for unattended jobs. | nothing documented | https://omp.sh/docs/subagents, https://omp.sh/docs/approvals | reviewed |
| Pi | None built in; see F15 for documented isolation patterns. | GA (non-feature) | https://pi.dev/docs/latest/security | reviewed |
| Cursor | Terminal commands read/write in the workspace only, network restricted. macOS Seatbelt (Cursor 2.0+); Linux Landlock plus seccomp (kernel 6.2+, Landlock v3, unprivileged userns; UID mapped to 0 in namespace); AppArmor package for remote/CLI when userns restricted. Windows not documented. A `bubblewrap` status value appears unexplained. | GA | https://cursor.com/docs/agent/security/run-modes | corrected |
| Antigravity | Linux namespaces (bubblewrap named only on the enterprise page), macOS Seatbelt, Windows AppContainer (CLI only; Windows 11 24H2 plus Aug 2026 update). Writable: project, temp, build caches; blocked `~/.ssh`, `.env`; no network by default. No VM or Docker. App on by default, CLI off (`auto` mode turns it on). | GA (personal accounts) | https://antigravity.google/docs/sandbox, https://antigravity.google/docs/enterprise | corrected |
| OpenHands | No OS sandbox of the host process; isolation is by container backend (F15). | n/a | https://docs.openhands.dev/openhands/usage/agent-canvas/architecture.md | reviewed |
| OpenClaw | Tool execution can move into Docker, Podman, SSH, OpenShell or Crabbox sandboxes (F15); off by default; "not a perfect security boundary". | GA (scorecard rates sandbox tools M3 Beta) | https://docs.openclaw.ai/gateway/sandboxing | reviewed |
| goose | Search snippets describe an opt-in macOS-only `GOOSE_SANDBOX` (Seatbelt plus local egress proxy, v1.25.0) for Desktop; the page 404s; an AAIF post says goose has no sandbox by default. Nothing for CLI, Linux, Windows. | unclear | https://goose-docs.ai/docs/guides/sandbox (snippet only), https://aaif.io/blog/give-goose-sandbox-powers-with-opensandbox-and-mcp | unverified |
| Hermes Agent | No OS sandbox for the local backend; isolation through container/remote terminal backends (F15). | n/a | https://hermes-agent.nousresearch.com/docs/user-guide/features/tools | reviewed |
| Zed/ACP | Zed Agent `terminal` and `fetch` only: macOS Seatbelt with an HTTP/HTTPS proxy for approved hosts; Linux bubblewrap (non-setuid); Windows only inside WSL. Default reads allowed, writes to project dirs and temp, git metadata never writable, network blocked. Not external agents, Terminal Threads, language servers, Git client or the normal terminal. ACP does not sandbox agents. | unclear (no maturity label) | https://zed.dev/docs/ai/sandboxing.md, https://agentclientprotocol.com/protocol/v1/security.md | reviewed |
| Warp | Nothing documented for local agents (they run in the user's terminal session); cloud runs use fresh containers. | nothing documented (local) | https://docs.warp.dev/agents/, https://docs.warp.dev/platform/architecture/ | unreviewed |
| Cline | `CLINE_SANDBOX`/`CLINE_SANDBOX_DATA_DIR` "enable sandbox mode" for session storage; whether it is an OS sandbox is not stated; no command sandbox documented. | unclear | https://docs.cline.bot/cli/cli-reference.md | reviewed |
| rizzy-harness (ours) | None on `main`: `bash` runs as the node's OS user and can read the data directory (AUDIT S1, README "The shell is not confined yet"). Unmerged `integration/gate3` has a Landlock sandbox via `rizzy-harness sandbox-exec`: read-write only session folder and per-session TMPDIR, read-only system dirs, optional `network = "off"` (TCP connect/bind only), `mode = required` default with self-test, fail closed; UDP/DNS and pathname Unix sockets below ABI 9 not covered. Codex rejects Landlock for exactly the socket gap. | none (main); exists only on an unmerged branch | `README.md` "The shell is not confined yet", AUDIT S1; branch: `crates/tools/src/sandbox.rs:48`, `:66`, `:79` | from own inventory |

#### F12. Behaviour when the sandbox is unavailable

What it is: what happens on a machine where the sandbox cannot start (old kernel, user namespaces blocked, unsupported OS).

Variants: fail closed (Codex macOS refuses to run; DeepSeek `SANDBOX_UNAVAILABLE`, "silent passthrough never legal"; Claude Code only with `failIfUnavailable`; ours on the branch), degrade to asking (Cursor Linux, Antigravity `proceed-in-sandbox` to `request-review`), or run unconfined (Claude Code default, Zed Linux with a warning, Claude Desktop Code tab on Windows, Windows native for Zed).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | If a policy cannot be enforced, Codex refuses to run the command rather than run it unsandboxed (stated for macOS on the permissions page). | GA | https://learn.chatgpt.com/docs/permissions.md | reviewed |
| Claude Code | Default: commands silently run unsandboxed when unsupported or dependencies are missing. With `sandbox.failIfUnavailable: true` startup exits, and later failures refuse each command ("Sandbox is required but failed to initialize"), `dangerouslyDisableSandbox` ignored. SDK default for `failIfUnavailable` (survey says true) is unverified. | GA | https://code.claude.com/docs/en/sandboxing.md, https://code.claude.com/docs/en/agent-sdk/typescript.md | reviewed (CLI), unverified (SDK default) |
| Claude Desktop | On Windows and hosts lacking sandbox dependencies the folder allowlist does not confine shell commands; `blockReadsOutsideWorkingDirectories` turns such reads into prompts. A changelog fix covers Cowork falling back to cloud when an admin turned off the local VM. | GA | https://claude.com/docs/third-party/claude-desktop/local-access.md | reviewed |
| DeepSeek harness | No usable backend: confinement fails with `SANDBOX_UNAVAILABLE`; partial enforcement reported per backend (older Landlock ABIs, Windows hard-link and unconfined-read gaps). | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/sandbox | reviewed |
| Cursor | On Linux an unmet kernel requirement falls back to asking approval. | GA | https://cursor.com/docs/agent/security/run-modes | corrected |
| Antigravity | Default preset falls back to Request Review where the sandbox is unavailable (Linux containers without userns, Windows 10). | GA | https://antigravity.google/docs/permissions, https://antigravity.google/docs/sandbox | corrected |
| Zed/ACP | Linux bubblewrap may fall back to unsandboxed with a warning; Windows native commands unconfined. | unclear | https://zed.dev/docs/ai/sandboxing.md | reviewed |
| rizzy-harness (ours) | `main`: no sandbox, so n/a. Branch: `[sandbox] mode = required` refuses agentic sessions where Landlock (ABI >= 3) is not enforced; PDF helper refuses PDFs without Landlock. | none (main); branch only | branch: `crates/node/src/policy.rs:56`, `crates/cli/src/main.rs:487-553`, `crates/master/src/pdf.rs:31-44` | from own inventory |

#### F13. Sandbox network egress controls

What it is: limiting where sandboxed commands may connect, usually through a host-side proxy and a domain allowlist.

Variants: hostname allowlists at a forward proxy without TLS inspection (Claude Code, Codex, Zed, Cursor modes, Antigravity driven by `read_url` rules); per-command reviewed host lists (Claude Code auto mode); TLS-intercepting proxy with credential injection (Hermes iron-proxy, Claude Code `tlsTerminate` experimental). Claude Code and Codex both document that the proxy does not cover their own built-in web tools, MCP or model traffic, and that DNS-rebinding protection is best effort. DeepSeek puts network outside its sandbox entirely.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Opt-in `features.network_proxy`: allowlist-first domain rules (deny wins), local/private destinations blocked unless exact literals allowed, best-effort DNS check (reduces rebinding, does not eliminate it), SOCKS5 and HTTP listeners on loopback, Unix socket allowlist. Does not filter web search, apps, MCP, browser, cloud or Codex's own model traffic. With network on and proxy off, egress is unrestricted. Managed `review_domains` forces review. | experimental (off by default) | https://learn.chatgpt.com/docs/agent-approvals-security.md, https://learn.chatgpt.com/docs/permissions.md | reviewed |
| Claude Code | Linux network namespace with no connectivity; macOS allows only the proxy connection. Host-side proxy checks hostname against allowed/denied domains, refuses names resolving only to local addresses (incl. 169.254.169.254) after allowlisting; UDP, QUIC, ICMP cannot leave. `strictAllowlist` refuses instead of prompting. In auto mode each command can carry extra hosts reviewed with it and opened only for that command. Domain filtering has no TLS inspection, so broad domains allow exfiltration and domain fronting. | GA | https://code.claude.com/docs/en/sandboxing.md | reviewed |
| Claude Desktop | Chat file-creation sandbox egress: off, package managers only, package managers plus owner domains, or all minus a blocklist (Team default package managers; new Enterprise off). MCP integrations reach outside regardless. Cowork egress follows an allowlist. | GA | https://support.claude.com/en/articles/12111783-create-and-edit-files-with-claude | corrected |
| DeepSeek harness | Network is outside the sandbox vocabulary; `web_fetch` allowed without confirmation in every mode; SSRF checks block private addresses but not exfiltration to a public URL (docs say so). | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/web | reviewed |
| Cursor | Network modes allowlist only, allowlist plus defaults (default), allow all; `sandbox.json` user and project files merge, team policy wins. | GA | https://cursor.com/docs/agent/security/run-modes | corrected |
| Antigravity | No network by default; `read_url` domains become the outbound allowlist (App docs); optional `sandboxProxy` HTTP proxy in the CLI. | GA | https://antigravity.google/docs/sandbox, https://antigravity.google/docs/permissions | corrected |
| Hermes Agent | Optional iron-proxy (F22) with default-deny upstream allowlist and SSRF CIDR denies; Docker compose recipe with an `internal: true` network and a dual-homed egress gateway (DNS can still resolve externally). | GA (optional) | https://hermes-agent.nousresearch.com/docs/user-guide/egress/iron-proxy, https://hermes-agent.nousresearch.com/docs/user-guide/egress/network-isolation | corrected / unreviewed |
| Zed/ACP | macOS proxy for approved hosts, network blocked by default; grants `network_hosts`/`allow_all_hosts` scoped once/thread/always. Proxy-ignoring tools (SSH, raw sockets) are a documented gap. | unclear | https://zed.dev/docs/ai/sandboxing.md | reviewed |
| rizzy-harness (ours) | `main`: none (agentic sessions reach the network only through unconfined `bash`). Branch: `network = "off"` blocks TCP connect/bind via Landlock; UDP/DNS not covered; no allowlist proxy. | none (main) | branch: `crates/tools/src/sandbox.rs:48` | from own inventory |

#### F14. Model-requested widening and sandbox escape hatches

What it is: a sanctioned way for the agent to ask to run one command with more access than the sandbox gives.

Variants: one-shot, exact-call grants (DeepSeek, Codex `request_permissions`), parameter on the call (Claude Code `dangerouslyDisableSandbox`, disabled by strict mode), sender-gated exec outside the sandbox (OpenClaw elevated), scoped user grants (Zed once/thread/always). Claude Code documents that an allow rule matching a command also approves its unsandboxed retry.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `request_permissions` tool asks for a subset of filesystem or network permissions (surfaced as `item/permissions/requestApproval`); custom agents can disable it. User `!` shell commands run outside the sandbox with full access. | GA | https://learn.chatgpt.com/docs/app-server.md, https://learn.chatgpt.com/docs/agent-configuration/subagents.md | unreviewed |
| Claude Code | `dangerouslyDisableSandbox` retry after a sandbox failure; prompts in Manual, classifier in auto, denied in dontAsk; `allowUnsandboxedCommands: false` (strict) ignores it; a user/managed `false` beats a project `true` from v2.1.285. `excludedCommands` run fully unsandboxed (docs warn about interpreters). | GA | https://code.claude.com/docs/en/sandboxing.md | reviewed |
| DeepSeek harness | Model requests a strictly wider one-shot retry with `sandbox_permissions` and `justification`; `ctx.approval` must grant that exact call. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/shell | unreviewed |
| Antigravity | Auto mode evaluates commands that must leave the sandbox (F5). | GA | https://antigravity.google/docs/permissions | unverified (status detail) |
| OpenClaw | `tools.elevated` lets `exec` run outside the sandbox for senders in `allowFrom`; exec-only, cannot bypass deny or a creator role's required sandbox; `/elevated full` skips approvals for the session. | GA | https://docs.openclaw.ai/gateway/sandbox-vs-tool-policy-vs-elevated | unreviewed |
| Zed/ACP | `agent.sandbox_permissions` grants `write_paths`, `allow_fs_write_all`, `allow_unsandboxed` scoped once/thread/always. | unclear | https://zed.dev/docs/ai/sandboxing.md | reviewed |
| rizzy-harness (ours) | Not in the inventory. | none | not in the inventory | from own inventory |

#### F15. Container, VM and remote-executor isolation

What it is: running the agent's tools (or the whole agent) in a container, microVM or separate machine so the host is the boundary.

Variants: vendor-hosted per-task VMs (Codex Cloud, Claude Code cloud sessions, Cursor Firecracker microVMs in a separate AWS account, Warp cloud containers); local per-conversation containers (OpenHands hardened Docker, Hermes Docker backend, OpenClaw Docker/Podman); local VM (Claude Desktop Cowork QEMU/KVM, Pi's Gondolin extension); documentation-only guidance (Pi, Claude Code isolation comparison, Codex devcontainer, DeepSeek SAFETY.md). Hermes and Codex make the container the boundary and drop approvals inside it; Codex warns a malicious repo can then exfiltrate credentials from the container.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Where Docker blocks namespaces or seccomp, docs say make the container the boundary and run `danger-full-access` inside; warn a malicious repo can exfiltrate Codex credentials. Codex Cloud: per-task workspace; legacy two-phase runtime removed secrets before an offline agent phase. | GA (guidance), cloud GA | https://learn.chatgpt.com/docs/agent-approvals-security.md, https://learn.chatgpt.com/docs/cloud.md | unreviewed (devcontainer), reviewed (cloud) |
| OpenCodex | Remote Workspace: agent on the hub, files from one approved root and bubblewrap commands on an executor; disconnected executor never falls back to the hub filesystem; read-only default. | experimental | https://opencodex.me/guides/remote-workspace/ | corrected |
| Claude Code | Isolation comparison: Bash sandbox, `srt` whole-process runtime (beta), dev container, custom container, VM, Anthropic cloud VM. Guidance: bypass mode only inside a container/VM as non-root; untrusted repos in a dedicated VM. Secure-deployment guide lists hardened Docker flags, gVisor, Firecracker with vsock-only networking. Self-hosted runners shift isolation to the operator (ephemeral container per session, capacity 1). | GA (guidance); self-hosted beta | https://code.claude.com/docs/en/sandbox-environments.md, https://code.claude.com/docs/en/agent-sdk/secure-deployment.md, https://code.claude.com/docs/en/self-hosted-environments-deploy.md | reviewed |
| Claude Desktop | Cowork local VM (Linux QEMU+KVM needing `/dev/kvm` and `/dev/vhost-vsock`) or cloud mode; cloud Code sessions in isolated VMs with credentials outside. | GA | https://code.claude.com/docs/en/desktop-linux, https://code.claude.com/docs/en/claude-code-on-the-web | corrected |
| DeepSeek harness | SAFETY.md recommends disposable VMs or containers for untrusted workloads; nothing built in. | preview | https://github.com/deepseek-ai/deepseek-harness/blob/master/SAFETY.md | unreviewed |
| Pi | Documented patterns: whole Pi in Docker (warns env vars visible, mounted `~/.pi/agent` exposes credentials); Docker Sandboxes (`sbx`) with a host proxy holding the key; OpenShell with filesystem/process/network/credential policies; Gondolin extension routing built-in tools to a local QEMU micro-VM (inherits host env, not a credential boundary). | GA (docs), example extension | https://pi.dev/docs/latest/containerization | unreviewed |
| Cursor | Cloud agents: Firecracker microVM per agent in a separate AWS account; egress allow-all / defaults plus allowlist / allowlist only; isolation claims are vendor-claim. | GA | https://cursor.com/docs/cloud-agent/security, https://cursor.com/docs/cloud-agent/security-network | corrected |
| OpenHands | Backends: host process (no isolation); whole-stack Docker; per-conversation Docker (host UID/GID, all caps dropped, `no-new-privileges`, API on 127.0.0.1, memory/CPU/PID limits, own keys per container); Kubernetes one shared pod; Cloud/Enterprise per-conversation Sysbox pods. Client-supplied memory/CPU override env limits and are not capped. Whole-Docker mode needs `--privileged` to run containers. | GA | https://docs.openhands.dev/openhands/usage/agent-canvas/backend-setup/docker-execution.md, https://docs.openhands.dev/openhands/usage/agent-canvas/backend-setup/kubernetes.md | reviewed |
| OpenClaw | Docker, Podman, SSH, OpenShell, Crabbox sandboxes; only tool execution moves, the Gateway stays on the host; modes `off`/`non-main`/`all`; off by default; GitHub identity excluded unless allowed. Crabbox cloud workers (beta) with Gateway-held keys. | GA (OpenShell no label; cloud workers beta) | https://docs.openclaw.ai/gateway/sandboxing, https://docs.openclaw.ai/gateway/openshell, https://docs.openclaw.ai/gateway/cloud-workers | reviewed / corrected |
| goose | Optional `--container` runs extensions in Docker (devcontainer workflows). | unclear | https://goose-docs.ai/docs/getting-started/using-extensions.md | unreviewed |
| Hermes Agent | `terminal.backend` local (default), docker, ssh, modal, daytona, vercel_sandbox, plugin backends; all file and exec tools go through the environment's `execute()`. Docker: cap-drop ALL with three added back, `no-new-privileges`, pids 256; dangerous-command checks skipped on container backends; checkpoints disabled there. SSH recommended because the agent "can't modify its own code". | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/tools, https://hermes-agent.nousresearch.com/docs/user-guide/security | reviewed / corrected |
| Warp | Cloud runs: fresh isolated container per run, Warp-hosted or self-hosted. | GA | https://docs.warp.dev/platform/architecture/ | reviewed |
| rizzy-harness (ours) | Master image only; nodes cannot be containerised by design (the node runs tools on the machine that owns the code). | none (nodes) | `Dockerfile:1-25`, `compose.yaml` | from own inventory |

#### F16. Sub-agent permission inheritance

What it is: whether a delegated agent can be more permissive than its parent.

Variants: inherit-and-cannot-escalate (Codex, Claude Code: bypass never applied from a subagent's frontmatter, ours: same approval mode), parent approval is the boundary (Oh My Pi subagents run as yolo but keep the deny map; Cline approving `spawn_agent` approves everything underneath), subagent's own rules only (OpenCode), delegation gated by mode (goose: subagents only in Autonomous).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Subagents inherit sandbox and approval settings including `/permissions` and `--yolo` changes; custom agent files cannot override `sandbox_mode`, `approval_policy`, `model_provider`, `mcp_servers`; non-interactive approvals fail back to the parent. | GA | https://learn.chatgpt.com/docs/agent-configuration/subagents.md | reviewed |
| OpenCode | A custom subagent uses only its own permissions, not its parent's. | GA | https://opencode.ai/docs/permissions/ | reviewed (base); glob details not re-checked |
| Claude Code | Parent bypass/acceptEdits/auto propagate regardless of frontmatter; plan stays plan; under default/dontAsk the frontmatter applies but `bypassPermissions` cannot escalate (v2.1.267+). Auto classifier reviews subagent work and final report. Agent teams: teammates start in the lead's mode including bypass; plan approvals auto-approved. | GA (teams experimental) | https://code.claude.com/docs/en/sub-agents.md, https://code.claude.com/docs/en/agent-teams.md | reviewed |
| Oh My Pi | Subagents run tier decisions as yolo (approval of the parent `task` call is the boundary) but inherit `tools.approval` (deny blocks, prompt rejects); workers cannot pause for approval. | GA | https://omp.sh/docs/approvals | reviewed |
| Antigravity | Custom subagent frontmatter `commandExecutionPolicy` (default `sandbox`); approvals bubble up. | GA | https://antigravity.google/docs/permissions (subagent entry in survey Area 9) | reviewed |
| goose | Subagents are disabled in non-autonomous modes. | GA | https://goose-docs.ai/docs/guides/managing-tools/goose-permissions.md | reviewed |
| Cline | Approving the `spawn_agent` call approves everything underneath. | GA | https://docs.cline.bot/features/subagents.md | reviewed |
| rizzy-harness (ours) | `delegate` child session on the same node with the same approval mode; approvals happen inside the child; depth one. | works | `crates/agent-native/src/lib.rs:233-317`, `crates/node/src/session.rs:654-763` | from own inventory |

#### F17. Unattended and headless approval behaviour

What it is: what a run with nobody to click does when an action needs approval.

Variants: deny and continue (Claude Code `--permission-prompts none`, `denyInsteadOfPrompting`; Hermes headless modes; OpenClaw `askFallback=deny`; Oh My Pi fails closed), defer until a human answers (Claude Code hook `defer`; ours parks for 24 h), auto-approve everything (Cline CLI default and schedules, DeepSeek Python SDK pins full access, Cursor Cloud Agents never ask). Claude Code's `-p` historically could start in auto; since v2.1.302 it starts in Manual and denies.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | In non-interactive runs a needed subagent approval fails back to the parent; `codex exec` read-only default noted in review. | GA | https://learn.chatgpt.com/docs/agent-configuration/subagents.md | reviewed |
| Claude Code | `-p`/SDK start in Manual (v2.1.302+) and deny anything needing approval unless a hook, `canUseTool` or `--permission-prompt-tool` approves; `--permission-prompts none` and `permissions.denyInsteadOfPrompting` deny and continue; `onFailure: "block"` hooks (Oct 2026); PreToolUse `defer` ends the turn with the call pending. | GA | https://code.claude.com/docs/en/headless.md, https://code.claude.com/docs/en/settings-reference.md, https://code.claude.com/docs/en/changelog.md | reviewed (modes), unreviewed (changelog items) |
| DeepSeek harness | Python SDK pins `danger-full-access`; GitHub PR review sessions run with the `read-only` preset. | preview | https://deepseek-harness.github.io/deepseek-harness/en/guide/python-sdk, https://deepseek-harness.github.io/deepseek-harness/en/guide/github-review | reviewed |
| Oh My Pi | Headless without a UI fails closed when a prompt is needed. | GA | https://omp.sh/docs/approvals | reviewed |
| Cursor | Cloud Agents never ask for approval. | GA | https://cursor.com/docs/agent/security/run-modes | reviewed |
| OpenClaw | `askFallback` default `deny`; cron approvals go only to connected exec-approval clients. | GA | https://docs.openclaw.ai/tools/exec-approvals | reviewed |
| Hermes Agent | `cron_mode`, `single_query_mode`, `unattended_mode` (webhook, msgraph, API server) default deny. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/security | reviewed |
| Cline | CLI `--auto-approve` defaults to true; scheduled runs approve their own tool calls in every mode and default to yolo; Telegram connector without `--allowed-user-id` or a hook auto-approves all messages. | GA | https://docs.cline.bot/cli/cli-reference.md, https://docs.cline.bot/cli/scheduling.md, https://docs.cline.bot/cli/connectors.md | reviewed |
| rizzy-harness (ours) | Agentic routines run only in `ask`/`ask_all`: a command waits for a human click, cancelled after 24 h; `auto` is rejected for routines. | works | `crates/master/src/routines/spec.rs:131-141`, `runner.rs:95` | from own inventory |

#### F18. Approvals delivered to another device or channel

What it is: answering an approval prompt from a phone, web dashboard or chat app while execution stays on the host.

Variants: vendor relay (Claude Code Remote Control, Antigravity reverse tunnel, Codex remote control pairing), chat-channel replies with approver lists (OpenClaw `/approve <id>`, Hermes buttons with `approvers`, Claude Code channel relay where any allowlisted sender can approve), self-hosted web UI (ours). Claude Code withholds one-tap approval for any request the remote UI cannot render in full.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `codex remote-control` starts the daemon with short-lived pairing codes; admins can disable with `allow_remote_control = false`; CLI overlay shows approvals from inactive agent threads with the source label. | GA | https://learn.chatgpt.com/docs/developer-commands.md, https://learn.chatgpt.com/docs/agent-configuration/subagents.md | reviewed |
| Claude Code | Remote Control (execution stays local; claude.ai login required). Channel permission relay: a channel declaring the capability gets `permission_request` with sanitised fields and replies allow/deny; first answer wins; only tool approvals relay. One-tap approval withheld for `requiresUserInteraction` tools. | GA (RC), research preview (relay) | https://code.claude.com/docs/en/remote-control.md, https://code.claude.com/docs/en/channels-reference.md, https://code.claude.com/docs/en/mcp.md | reviewed (RC), unreviewed (relay) |
| Claude Desktop | Remote Control onto Desktop/CLI sessions; Dispatch (limited beta) includes a 30-minute computer-use approval window. | GA (RC), limited beta (Dispatch) | https://code.claude.com/docs/en/remote-control, https://code.claude.com/docs/en/desktop | corrected |
| Antigravity | Web dashboard controls a session through a reverse tunnel opened by the host; approvals answerable from either end; relay host and encryption not described. | GA | https://antigravity.google/docs/remote-control | reviewed |
| OpenClaw | `exec.approval.requested` broadcast to operator clients; `approvals.exec` forwards prompts to chat targets; `/approve <id> allow-once/allow-always/deny`; native approval UIs on Discord, Slack, Telegram, Matrix and others check approver lists; resolving needs `operator.approvals`. | GA | https://docs.openclaw.ai/tools/exec-approvals | reviewed |
| Hermes Agent | Gateway accepts yes/no replies or buttons; `approvers` controls who may answer in shared chats. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/security | reviewed |
| Cline | Messaging connectors with a JSON allow/deny hook protocol; Discord reachable by anyone unless a hook is set. | GA | https://docs.cline.bot/cli/connectors.md | reviewed |
| rizzy-harness (ours) | Mutating call emits `approval_requested`, turn blocks until the web UI answers; "Waiting for you" attention list; cancel or restart denies; who may approve is rechecked on every approval. | works | `crates/node/src/session.rs:955-1012`, `web/src/components/Attention.tsx`, `crates/master/src/workspaces.rs:18-75` | from own inventory |

#### F19. Hooks and extensions in the permission path

What it is: user or admin code that runs before a tool call and can allow, deny, ask or rewrite.

Variants: the key difference is failure behaviour. Fail open by default: Cursor (unless `failClosed`), OpenHands cookbook hooks, Claude Code mods (unless `.catch`), Claude Code command hooks before `onFailure: "block"` (Oct 2026). Fail closed: Pi (a failing `tool_call` handler blocks the call). Precedence: Claude Code hook `allow` never overrides deny/ask rules, while a mod can approve past deny rules in unmanaged setups; Cursor merges hook sources with deny > ask > allow but last-response-wins for other fields; Antigravity adds `deny_unless_prior_grant` and `force_ask`.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | PreToolUse can deny (exit 2 or JSON), add context or rewrite input; `PermissionRequest` event; matching command hooks run concurrently; managed-only hooks possible. | GA (no label on page) | https://learn.chatgpt.com/docs/hooks.md | corrected |
| Claude Code | PreToolUse deny blocks in every mode including bypass; hook `allow` never overrides deny/ask rules; decisions combine deny > defer > ask > allow. Mods (`tool.check`, `tool.call`) can answer, rewrite or withhold; fail open unless `.catch`; without managed settings or a Team/Enterprise login a user mod can approve past deny rules and blocking hooks; mods are unsandboxed and can read `.env` regardless of Read deny. `@` file mentions bypass PreToolUse hooks. | GA | https://code.claude.com/docs/en/hooks-guide.md, https://code.claude.com/docs/en/permissions.md, https://code.claude.com/docs/en/plugins/mods/admin.md, https://code.claude.com/docs/en/plugins/mods/events.md | reviewed (with review-2 fail-open and guard caveats) |
| Pi | Extensions can block or mutate tool calls (`tool_call` event); a failing handler blocks that call. | GA | https://pi.dev/docs/latest/extensions | unreviewed |
| Cursor | Exit 2 blocks, other codes fail open unless `failClosed`; sources enterprise MDM, team, project, user, plugins; permission decisions deny > ask > allow regardless of source, other fields last response wins; loads Claude Code hooks. | GA | https://cursor.com/docs/hooks | corrected |
| Antigravity | PreToolUse returns `allow` (defers to next hook or default), `auto_approve`, `deny`, `ask`, `force_ask`, `deny_unless_prior_grant`, plus argument `overwrite` and `permissionOverrides`. | GA | https://antigravity.google/docs/hooks | reviewed |
| OpenHands | Cookbook PreToolUse hooks for command blacklists and workspace isolation are heuristic, cannot resolve symlinks and fail open; security docs name hook-based enforcement only as a future fix. | GA (hooks); cookbook | https://docs.openhands.dev/cookbook/workspace-isolation.md | corrected |
| Cline | `.clineignore` (deprecating, "not a security boundary") is replaced by a PreToolUse hook with a token-heuristic shell check (symlinks not resolved); CLI hooks are disabled under `--yolo`. | deprecated (clineignore) | https://docs.cline.bot/customization/clineignore.md | reviewed |
| rizzy-harness (ours) | No hooks. | none | `own-rizzy-harness.md` section 9 | from own inventory |

#### F20. Permission and approval decisions as logged events

What it is: every policy change, approval request and decision recorded in an append-only log or audit stream.

Variants: in the session log itself (DeepSeek `approval/asked`, `sandbox/mode`, `approval/policy`, `permission/preset` events, and a failed audit append rejects the request; ours), in OTel telemetry (Claude Code `tool_decision`, `safety_event`; Codex OTel tool decisions; Claude Desktop decision source), or in a security log (goose `SEC-{uuid}`).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | OTel export (off by default) includes tool decisions and tool results (capped at 2,048 bytes); `log_guardian_assessments` opt-in. | GA | https://learn.chatgpt.com/docs/agent-approvals-security.md | unreviewed |
| Claude Code | OTel `tool_decision` (decision, source, classifier verdict, mode) and `safety_event` (v2.1.307+) for every action stopped, prompted or bounced; `PermissionDenied` hook gets exact `tool_input`; `-p` reports `permission_denials`. | GA | https://code.claude.com/docs/en/monitoring-usage.md, https://code.claude.com/docs/en/headless.md | unreviewed |
| Claude Desktop | Per-tool decision source in telemetry (`config`, `hook`, `user_permanent`, `user_temporary`...). | GA | https://code.claude.com/docs/en/desktop | unreviewed |
| DeepSeek harness | Each request appends `approval/asked`; effective policy is the last `approval/policy` event; `sandbox/mode` and `permission/preset` events; a failed audit append rejects the request. Requests deliberately omit tool arguments. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/approval | reviewed |
| goose | Prompt-injection findings paused with a finding id; decisions logged with `SEC-{uuid}` ids. | unclear | https://goose-docs.ai/docs/guides/logs.md | reviewed |
| rizzy-harness (ours) | Every approval request and decision is a session event (one writer, contiguous `seq`); restart mid-turn denies pending approvals; SQLite `audit` table records node commands, joins, revokes (no UI). | works | `crates/node/src/session.rs:955-1012`, `:154-191`, `crates/store/src/lib.rs:293-297` | from own inventory |

### Area 21. Security and privacy

#### F21. Child-process environment scrubbing

What it is: removing secrets from the environment that the agent's commands inherit.

Variants: allow-list by default (ours), opt-in scrub (Claude Code `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB`), inherit-with-filters where the default passes tokens (Codex `ignore_default_excludes = true`), and no-forward for remote backends (Hermes Docker/SSH). Claude Code's sandbox inherits the environment including secrets by default.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `shell_environment_policy`: `inherit = all/core/none`, include/exclude globs, `set`; `ignore_default_excludes` defaults to true, so `*KEY*`, `*SECRET*`, `*TOKEN*` pass through unless the user opts in. | GA | https://learn.chatgpt.com/docs/config-file/config-advanced.md | reviewed |
| Claude Code | Sandbox inherits env including secrets. `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB=1` removes credentials (by name or value shape) from Bash, hook and stdio MCP environments, strips user:password from package index URLs, keeps GitHub tokens and proxy vars, and on Linux isolates Bash's PID namespace; set automatically by `claude-code-action` for non-write users. `headersHelper` from repo/plugin sources runs with credential-like env stripped. Exact scrub list not published. | GA | https://code.claude.com/docs/en/env-vars.md, https://code.claude.com/docs/en/sandboxing.md, https://code.claude.com/docs/en/mcp.md | unreviewed |
| DeepSeek harness | Child processes (bash, git, MCP) inherit proxy variables including any password in the URL. | preview | https://deepseek-harness.github.io/deepseek-harness/en/guide/network-proxy | unreviewed |
| OpenClaw | Skill env not injected into the sandbox; GitHub identity excluded from the sandbox by default. | GA | https://docs.openclaw.ai/tools/skills, https://docs.openclaw.ai/gateway/sandboxing | unreviewed |
| Hermes Agent | Docker and SSH forward no env by default (`docker_forward_env` empty); stdio MCP servers get a safe env baseline. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/security, https://hermes-agent.nousresearch.com/docs/user-guide/features/mcp | reviewed |
| rizzy-harness (ours) | Children start from an empty env plus `PATH HOME USER LOGNAME SHELL LANG LANGUAGE LC_* TERM COLORTERM TZ TMPDIR` and `env_pass` globs; provider key vars, `.env` names and `HARNESS_*` always stripped; `SSH_AUTH_SOCK` and tokens not passed unless listed. | works | `crates/tools/src/env.rs:1-135` | from own inventory |

#### F22. Credential placeholders and injection at an egress proxy

What it is: the agent's processes never hold the real secret; a proxy outside the agent's boundary adds it to outbound requests for allowed hosts.

Variants: sentinel-in-process plus TLS-terminating substitution (Claude Code sandbox `mask`, needs experimental `tlsTerminate`; Hermes iron-proxy opaque tokens; Pi's documented Docker Sandboxes placeholder); key never inside, attached by the vendor's proxy after the request leaves (Codex Cloud network secrets, Claude Code cloud network secrets, Claude Tag Agent Proxy, Cursor runtime secrets "never reach the model"); inference proxied by a central host so workers never get provider keys (OpenClaw cloud workers, Oh My Pi auth-gateway, OpenCodex hub, Warp). Review-3 stresses that Claude Code's sandbox masking and its cloud network secrets are different mechanisms. Ours does the opposite: every node holds provider keys.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Cloud network secrets: programs see a placeholder and the proxy substitutes the real value only for allowed HTTPS:443 destinations. Docs warn against job-wide `OPENAI_API_KEY` in CI and point to the Action's proxy. | GA | https://learn.chatgpt.com/docs/environments/cloud-environments.md, https://learn.chatgpt.com/docs/non-interactive-mode.md | reviewed |
| OpenCodex | Remote hub holds provider credentials; clients keep only a per-client data key; the loopback companion listener is unauthenticated (single-tenant host only). | GA | https://opencodex.me/guides/remote-hub/ | corrected |
| Claude Code | Sandbox `sandbox.credentials` `mode: "mask"` gives commands a per-session sentinel; the proxy substitutes on requests to allowed hosts; requires experimental `network.tlsTerminate`; honoured only from user/managed/`--settings`; covers sandboxed Bash only; macOS blocks masked files outright; a non-matching extract pattern leaves the real file readable. AWS SigV4 re-signing. Cloud: network secrets attached by Anthropic's agent proxy after requests leave the VM, never in env or files; never attached to GitHub, `api.anthropic.com`, public registries, setup scripts, telemetry; not in self-hosted or CMEK. | GA (mask with experimental `tlsTerminate`) | https://code.claude.com/docs/en/sandboxing.md, https://code.claude.com/docs/en/cloud-environments.md | corrected (review-3: two different mechanisms) |
| Claude Desktop | Cloud sessions keep credentials outside the VM. | GA | https://code.claude.com/docs/en/claude-code-on-the-web | unreviewed |
| Oh My Pi | `omp auth-gateway` forward proxy (default 127.0.0.1:4000, own bearer) so less-trusted clients call providers without receiving credentials; operator sees request and response content. `omp auth-broker` gives clients API keys and access tokens, refresh tokens stay on the broker; plain HTTP. | GA | https://omp.sh/docs/secrets | corrected |
| Pi | Documented: Docker Sandboxes keep the real provider key on the host and the sandbox sees a placeholder; OpenShell inference routing keeps model credentials outside. | GA (docs) | https://pi.dev/docs/latest/containerization | unreviewed |
| Cursor | Cloud runtime secrets kept out of transcript, tool output, commits and "never reach the model" (vendor statement); KMS-encrypted; 5-minute RS256 OIDC tokens via a local Unix socket and AWS role assumption avoid long-lived keys. | GA | https://cursor.com/docs/cloud-agent/setup, https://cursor.com/docs/cloud-agent/identity | reviewed |
| OpenClaw | Cloud workers: provider keys stay on the Gateway, inference proxied by model reference; GitHub publication via a Gateway broker; per-dispatch worker credential stored only as a hash. | beta | https://docs.openclaw.ai/gateway/cloud-workers/security-model | corrected |
| Hermes Agent | iron-proxy: host-side TLS-intercepting proxy (SHA-256-verified pinned binary) for Docker/remote sandboxes; sandbox holds opaque tokens in the normal provider env vars, proxy swaps them for real keys (from `.env` or Bitwarden) on allowlisted upstreams; binds the Docker bridge gateway (loopback on Docker Desktop), never 0.0.0.0. Stated limits: stolen CA key, redirected egress, raw sockets; Bedrock SigV4 and Vertex keep real credentials in the sandbox; Hermes' own LLM calls not rewritten. | GA (optional) | https://hermes-agent.nousresearch.com/docs/user-guide/egress/iron-proxy | corrected |
| Warp | Inference credentials never enter the sandbox for Warp Agent and BYO LLM; URL-based MCP credentials live with Warp's proxy, command-based ones in the execution environment. | GA | https://docs.warp.dev/platform/architecture/ | reviewed |
| OpenHands | SDK secret registry detects references in bash commands, injects them as env vars and masks values in output; `request_secret` tool saves a secret to the running conversation; secrets stay out of chat, events and tool arguments (docs). Values are in the process env, not behind a proxy. | GA | https://docs.openhands.dev/sdk/guides/secrets.md | unreviewed |
| rizzy-harness (ours) | None. Nodes call the provider directly with their own `config.toml` key, so every node holds provider keys; provider key env vars are stripped from tool children (F21). | none | `crates/cli/src/main.rs:213-300`, `:500-540` | from own inventory |

#### F23. Secret redaction in output, logs and telemetry

What it is: masking known secret values or secret-shaped strings before they reach the model, the transcript, logs or the vendor.

Variants: regex patterns, off by default (Warp); known-value scrubbing (ours, OpenHands registry); redaction in specific outbound channels (Claude Code permission relay, MCP error text, telemetry content off by default; Codex OTel prompts redacted by default). Most vendors say redaction is a backstop (Warp), a courtesy (ours) or absent from logs (Oh My Pi, OpenClaw `sessions_history`).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | OTel: prompts redacted by default; agent responses and guardian assessments opt-in; tool results capped at 2,048 bytes. | GA | https://learn.chatgpt.com/docs/config-file/config-advanced.md | unreviewed |
| Claude Code | Telemetry exports no prompt, response, tool input/output or file content by default (opt-in variables, ignored from project settings). Channel relay masks recognisable provider tokens (not unprefixed or multi-line keys). MCP failure detail credential-redacted and never includes expanded URLs. Transcripts are plaintext and any secret a tool prints lands there. | GA | https://code.claude.com/docs/en/monitoring-usage.md, https://code.claude.com/docs/en/channels-reference.md, https://code.claude.com/docs/en/mcp.md, https://code.claude.com/docs/en/claude-directory.md | unreviewed |
| Oh My Pi | No automatic secret redaction in logs, transcripts or exports documented; share redacts configured secrets by default. | GA | https://omp.sh/docs/secrets | unreviewed |
| OpenHands | Secret values masked by default in Settings and masked in command output by the SDK registry. | GA | https://docs.openhands.dev/openhands/usage/settings/secrets-settings.md, https://docs.openhands.dev/sdk/guides/secrets.md | unreviewed |
| OpenClaw | `sessions_history` returns text without secret masking. | GA | https://docs.openclaw.ai/concepts/session-tool | unreviewed |
| Hermes Agent | `security.redact_secrets` masks key-like strings in output and logs; commands flagged for curl/wget carrying secret variables. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/security | unreviewed |
| Warp | Regex masking of secrets, passwords, keys and PII before content goes to Warp or LLM providers; off by default; not applied to session sharing; Enterprise can enforce; telemetry has built-in redaction regardless; cloud doc calls it a backstop. | GA, off by default | https://docs.warp.dev/support-and-community/privacy-and-security/secret-redaction/, https://docs.warp.dev/platform/architecture/ | unreviewed |
| rizzy-harness (ours) | Known secret values scrubbed from tool output, model text and event payloads; split or encoded keys pass (AUDIT S11). Branch adds a stream scrubber holding back the tail across chunks and per-field JSON scrubbing. `Secret<T>` redacts `Debug`; headers never logged. | partial | `crates/agent-native/src/lib.rs:805`, `:849`, `crates/node/src/session.rs:194-215`, `crates/core/src/secret.rs`; branch `crates/core/src/secret.rs:109` | from own inventory |

#### F24. Storage of the harness's own credentials

What it is: where provider keys and login tokens live on disk.

Variants: OS keyring with plaintext fallback (Codex `auto`, Claude Code macOS Keychain, goose, Antigravity CLI, Claude Desktop Linux), plaintext 0600 file (Claude Code Linux, Oh My Pi SQLite, ours), external secret managers (Hermes 1Password/Bitwarden), unstated encryption (DeepSeek, Pi, Cline).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `cli_auth_credentials_store = file/keyring/auto/ephemeral`; `file` writes plaintext `~/.codex/auth.json` ("treat like a password"); MCP OAuth has its own store. | GA | https://learn.chatgpt.com/docs/auth.md | unreviewed |
| Claude Code | macOS Keychain (fallback `~/.claude/.credentials.json` 0600 when the Keychain rejects the write); Linux 0600 file, no libsecret integration documented; Windows file with profile ACLs; `apiKeyHelper` script. | GA | https://code.claude.com/docs/en/authentication.md, https://code.claude.com/docs/en/security.md | reviewed |
| Claude Desktop | Sign-in stored in the OS keyring on Linux. | GA | https://code.claude.com/docs/en/desktop-linux | unreviewed |
| DeepSeek harness | Keys write-only in the UI, stored in `$DSH_HOME/.credentials.yaml`, settings hold only an env-var reference, no remote method returns a secret; encryption at rest not documented. | preview | https://deepseek-harness.github.io/deepseek-harness/en/guide/providers, https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/credentials | corrected |
| Oh My Pi | Local SQLite `agent.db` (dir 0700, file 0600), not encrypted at rest. | GA | https://omp.sh/docs/secrets | unreviewed |
| Pi | `auth.json` in the agent dir; `models.json` values can use `!command`; encryption not stated. | GA | https://pi.dev/docs/latest/cli, https://pi.dev/docs/latest/models | unreviewed |
| Antigravity | CLI credentials in the OS keyring (fallback Google Sign-In); settings file written 0600. | GA | https://github.com/google-antigravity/antigravity-cli, https://antigravity.google/docs/settings | unreviewed |
| goose | Keys not read from `config.yaml`; system keyring preferred, plaintext `secrets.yaml` fallback; env vars take precedence. | GA | https://goose-docs.ai/docs/guides/config-files.md | reviewed |
| Hermes Agent | `.env`, 1Password, Bitwarden Secrets Manager or a command helper. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/security | unreviewed |
| Cline | `providers.json` under `~/.cline/data/settings/`; SSH never stores passwords or key contents; encryption not documented. | GA | https://docs.cline.bot/cli/cli-reference.md, https://docs.cline.bot/usage/cline-desktop-ssh.md | unreviewed |
| rizzy-harness (ours) | Keys by env-var name or 0600 `secrets.toml`; stored keys only sent to the base URL they were saved for; `api-token` file mode not enforced on read on `main` (AUDIT L7; branch refuses group/other-readable secret files). | works (L7 open) | `crates/master/src/secrets.rs:1-15`, `crates/core/src/secret.rs` | from own inventory |

#### F25. Prompt-injection defences for untrusted content

What it is: measures that stop text from web pages, tool output, files or other users from steering the agent into harmful actions.

Variants: (1) keep untrusted text away from the decider (Claude Code strips tool results from the classifier; WebFetch summariser model; Codex cached search by default); (2) mark content as untrusted (OpenClaw boundary markers, OpenHands memory wrapper, Claude Code paste marking, DeepSeek PR fields, OpenCodex search sidecar, ours webhook turns); (3) scan for injection patterns (goose pattern plus optional ML classifier, off by default; Hermes context-file scans; OpenHands analyzers; Claude Code server-side probe); (4) provenance on outbound URLs (ours, unique); (5) strip invisible characters (Claude Code, ours memory facts, Hermes command flags). Pi and OpenClaw state that prompt injection alone is out of scope for vulnerability reports.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Cached web search by default to reduce injection exposure; network proxy guards local destinations; browser content untrusted; Python SDK `ExternalMessage` lowers third-party content priority; auto-review targets exfiltration and credential probing. | GA | https://learn.chatgpt.com/docs/agent-approvals-security.md, https://learn.chatgpt.com/docs/codex-sdk.md | unreviewed |
| OpenCodex | Web-search sidecar wraps results as untrusted data; API returns `403 agent_consent_required` so an agent cannot star the repo for the user. | GA | https://opencodex.me/getting-started/for-agents/ | unreviewed |
| Claude Code | Layers: prompts in Manual; network commands never auto-approved; WebFetch runs a separate model over the page; "command injection detection" asks for any Bash it cannot fully analyse; tool results stripped from classifier input; a server-side probe scans tool results (not described). Invisible Unicode stripped from prompts with a review step; large pastes marked as pasted from elsewhere; bidi/invisible skill names refused; account-email egress asks first. | GA | https://code.claude.com/docs/en/security.md, https://code.claude.com/docs/en/interactive-mode.md, https://code.claude.com/docs/en/terminal-config.md | unreviewed |
| Claude Desktop | Computer use flags injection from on-screen content; browser per-site approval and safety classifiers in every mode; Chrome pauses at logins; Cowork risk warning. | GA | https://code.claude.com/docs/en/computer-use, https://code.claude.com/docs/en/desktop | unreviewed |
| DeepSeek harness | Webhook-triggered PR review sessions pass PR fields labelled as untrusted metadata, read-only preset, prompt forbids mutations. | preview | https://deepseek-harness.github.io/deepseek-harness/en/guide/github-review | reviewed |
| Pi | Prompt injection is out of scope for vulnerability reports unless a privilege-boundary bypass is shown. | GA | https://pi.dev/docs/latest/security | reviewed |
| Antigravity | CLI README warns of exfiltration and prompt injection; generative UI widgets in an isolated frame; no injection defence documented beyond the warning. | GA | https://github.com/google-antigravity/antigravity-cli, https://antigravity.google/docs/commands/generative-ui | unreviewed |
| OpenHands | Persistent memory wrapped as untrusted; analyzer scans for injection phrases; v1.34.0 fixed untrusted Markdown links; Canvas Apps explicitly unsandboxed. | GA | https://docs.openhands.dev/sdk/guides/persistent-memory.md, https://docs.openhands.dev/sdk/guides/security.md | unreviewed |
| OpenClaw | External content wrapped in boundary markers, chat-template tokens stripped, outbound replies sanitised; guidance to use a read-only reader agent and keep `allowUnsafeExternalContent` off. Cites an unsourced 0.5% to 8.5% attack-success test. Prompt injection alone is closed as no-action. | GA | https://docs.openclaw.ai/gateway/security/prompt-injection, https://docs.openclaw.ai/gateway/security/trust-model | unreviewed / reviewed |
| goose | Tool parameter text matched against threat patterns; optional ML classifier over a configurable HTTP endpoint; score >= 0.8 pauses with a finding id; default off. | unclear; default off | https://goose-docs.ai/docs/guides/security/prompt-injection-detection.md, https://goose-docs.ai/docs/guides/environment-variables.md | reviewed |
| Hermes Agent | Context files, memory entries and cron prompts scanned for injection phrases, hidden HTML comments and exfiltration patterns; commands flagged for invisible or bidi Unicode; MCP results stripped of invisible tag characters. The bundled "tirith" scanner was removed with nothing replacing it. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/security, https://hermes-agent.nousresearch.com/docs/user-guide/features/context-files | unreviewed |
| Zed/ACP | ACP guidance: treat agent strings, URIs, paths as untrusted; sanitise Markdown/HTML; make bidi and zero-width visible; no automatic remote images; ask before sending data off-machine after reading untrusted content. Prompt-injection detection out of scope. | GA (guidance) | https://agentclientprotocol.com/protocol/v1/security.md | unreviewed |
| rizzy-harness (ours) | `web_fetch` fetches a URL only if it appeared in user or tool text (`observed`), so injected pages cannot build exfiltration URLs (on `main` the set includes any tool output and sub-agent task text, AUDIT L11; branch narrows it). Webhook-triggered chats lose memory and web tools and are flagged. No model HTML, no remote images, links show the real host, `ui:` blocks validated in Rust; hidden/bidi characters stripped from memory facts; form fields asking for secrets refused. | works (L11 weak on main) | `crates/web/src/lib.rs:170-205`, `crates/master/src/lib.rs:257-297`, `crates/core/src/ui/mod.rs:56-83`, `crates/memory/src/lib.rs:103-122`, `crates/core/src/ui/secret.rs` | from own inventory |

#### F26. Sub-agent and inter-agent output hygiene

What it is: treating a sub-agent's or peer agent's report as data, not as instructions with user authority.

Variants: only Claude Code documents active scanning and neutralising of instruction-shaped text; ours limits what a sub-agent's reply can render.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | From v2.1.210 each subagent's final report is scanned: `<system-reminder>` and `Human:`/`Assistant:` imitations are neutralised with a backslash, a `[harness: ...]` marker is prepended for tag or permission-config matches, nothing is deleted; reports arrive under a header saying they carry no user authority; in auto mode inter-agent messages are classified before delivery. Docs: the scan does not judge malice. Background task notifications state no human input occurred. | GA | https://code.claude.com/docs/en/sub-agents.md, https://code.claude.com/docs/en/agent-sdk/subagents.md, https://code.claude.com/docs/en/agent-teams.md | unreviewed |
| rizzy-harness (ours) | Sub-agent replies are capped to `display` level (no questions or forms); citations resolve in the parent's numbering. No instruction-pattern scan. | partial | `crates/core/src/ui/mod.rs:132-145`, `crates/agent-native/src/lib.rs:310-314` | from own inventory |

#### F27. SSRF and private-address guards on fetches

What it is: refusing fetches to loopback, private ranges and cloud metadata endpoints, including after redirects and DNS rebinding.

Variants: resolver-level filtering with re-check on every redirect (ours, Hermes), proxy-level refusal after allowlisting (Claude Code sandbox proxy, Codex network proxy best effort), and explicit documentation that SSRF guards do not stop exfiltration to public hosts (DeepSeek).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Network proxy blocks local and private destinations unless exact literals allowed; DNS check is best effort against rebinding; 0.163.0 checks permission before DNS lookup and rejects duplicate Host headers. | experimental | https://learn.chatgpt.com/docs/permissions.md, https://learn.chatgpt.com/docs/changelog | reviewed |
| Claude Code | Sandbox proxy refuses names resolving only to local addresses (loopback, link-local incl. 169.254.169.254); private 10/8 allowed. Hosted marketplace downloads refuse redirects to loopback/link-local/metadata. Gateway SSRF guard pins resolved IPs. | GA | https://code.claude.com/docs/en/sandboxing.md, https://code.claude.com/docs/en/plugins/host-marketplace.md, https://code.claude.com/docs/en/claude-apps-gateway-deploy.md | reviewed (sandbox), unreviewed (others) |
| DeepSeek harness | `web_fetch` SSRF checks block private addresses (pinned addresses noted in review) but not exfiltration to a public URL. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/web | reviewed |
| Hermes Agent | SSRF protection on by default (RFC 1918, loopback, link-local incl. metadata, CGNAT, DNS failures; redirects re-validated per hop); `allow_private_urls` default false, metadata always blocked; `website_blocklist`. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/security | unreviewed |
| rizzy-harness (ours) | Custom DNS resolver drops non-public addresses on every redirect, env proxies ignored, IP-literal and `localhost` URLs checked; `is_public` misses 6to4, Teredo, IPv4-compatible IPv6 on `main` (AUDIT L12; fixed on branch). Chat runtime only. | works (gap L12) | `crates/web/src/lib.rs:39-90`, `:125-165` | from own inventory |

#### F28. Project and workspace trust prompts

What it is: before loading configuration, hooks, MCP servers or skills from a folder, the user confirms they trust it.

Variants: what trust gates (hooks, allow rules, MCP servers in Claude Code; project config layers, hooks and rules in Codex; all project-local resources in Pi; skills and instructions in Zed); what headless mode does (Claude Code `-p` treats any folder as trusted and runs repo hooks, `env`, `apiKeyHelper` and `.mcp.json` servers; Pi `ask` skips in print mode); defaults (Cursor Workspace Trust off). Pi says trust is not a full boundary (`AGENTS.md` loads regardless).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Untrusted projects skip project `.codex/` layers (config, hooks, rules); `[projects."<path>"] trust_level = "untrusted"` keeps the stricter behaviour. | GA | https://learn.chatgpt.com/docs/agent-approvals-security.md, https://learn.chatgpt.com/docs/config-file/config-basic.md | reviewed |
| Claude Code | Interactive trust dialog lists what it would activate; repo `permissions.allow`, `additionalDirectories`, hooks, status line, MCP approvals wait for it, deny/ask apply at once; keyed on repo root. `-p`/SDK never show it: repo allow rules are ignored but hooks, `env`, `apiKeyHelper`, skill `allowed-tools` and `.mcp.json` servers run without asking. `/cd` re-prompts. | GA | https://code.claude.com/docs/en/permissions.md, https://code.claude.com/docs/en/hooks.md, https://code.claude.com/docs/en/mcp.md | unreviewed (trust dialog), reviewed (working dirs) |
| Oh My Pi | No trust prompt; narrowing flags only. | GA (flags) | https://github.com/can1357/oh-my-pi | unreviewed |
| Pi | Trust decision gates `.pi` settings, MCP config, extensions, skills, prompts, themes, system-prompt files; resolution CLI override, extensions, saved `trust.json`, then `defaultProjectTrust` (default `ask`, settable only in agent-dir settings). Not a full boundary: `AGENTS.md`/`CLAUDE.md` load regardless and trust does not limit tool calls. | GA | https://pi.dev/docs/latest/security | reviewed |
| Cursor | Workspace Trust off by default; restricted mode breaks AI features. | GA | https://cursor.com/docs/agent/security | reviewed |
| Antigravity | CLI `trustedWorkspaces` lists folders the user chose to trust (added automatically, default empty); what trust unlocks is not stated. | GA | https://antigravity.google/docs/settings | corrected |
| Zed/ACP | Project-local skills and instructions load only from trusted worktrees. | GA | https://zed.dev/docs/ai/skills.md, https://zed.dev/docs/ai/privacy-and-security.md | unreviewed |
| rizzy-harness (ours) | No trust prompt; not needed today because no repo-sourced configuration or instruction file is loaded (F10). Becomes required the day AGENTS.md or repo config is read. | none (n/a) | `own-rizzy-harness.md` section 4 "Project/instruction files" | from own inventory |

#### F29. Trust for extensions, plugins, hooks and marketplaces

What it is: how the harness decides whether to run third-party code it did not write (plugins, MCP servers, hooks, skills).

Variants: trust by content hash so a changed hook needs re-review (Codex); install-time static scans with non-overridable "dangerous" verdicts (Hermes, OpenClaw `--deep`, Claude Code mod scanner refusing unreadable `$` usage); allowlists by exact command (goose); marketplace name tiers with commit pinning (Claude Code); install confirmation showing source (DeepSeek). Almost everyone states plugins run in-process with full user rights and are not sandboxed (OpenCode, Claude Code, Hermes, DeepSeek, Oh My Pi, OpenHands Apps).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Non-managed hooks must be reviewed and are trusted by hash; a changed hook or plugin script needs review again; `--dangerously-bypass-hook-trust`; `allow_managed_hooks_only`. | GA (no label) | https://learn.chatgpt.com/docs/hooks.md | corrected |
| OpenCode | Policies can withhold plugins but do not sandbox plugin code; `` !`cmd` `` in custom commands runs outside the permission flow (docs warn). | GA | https://opencode.ai/docs/policies/, https://opencode.ai/docs/commands/ | reviewed |
| Claude Code | Plugins run arbitrary code as the user; hooks, monitors, MCP, LSP and mods run outside the sandbox and permission rules; repo plugins load only after folder trust. Marketplace tiers by name accepted only from `github.com/anthropics/`; community catalogue pinned to commit SHAs. Dependency installs pinned, `--ignore-scripts`, registry-only. Mods: static scanner must be able to enumerate `$` API use or the mod is refused; built-in guard only with managed settings or Team/Enterprise login. Project `.mcp.json` servers need first-use approval (not in `-p`). | GA | https://code.claude.com/docs/en/plugins/security.md, https://code.claude.com/docs/en/plugins/loading.md, https://code.claude.com/docs/en/plugins/mods/overview.md, https://code.claude.com/docs/en/mcp.md | unreviewed (review-2 confirmed mods guard details) |
| DeepSeek harness | Market install shows a confirmation dialog with the source; markets untrusted, `http:` indexes tamperable; `plugin_manager` tool needs full access or approval; plugins appear to run in-process (survey inference). | preview | https://deepseek-harness.github.io/deepseek-harness/en/guide/plugin-market | reviewed |
| Oh My Pi | MCP, plugins, hooks, skills, custom tools all treated as trusted code; repo content auto-loaded. | GA | https://omp.sh/docs/plugins, https://omp.sh/docs/mcp | unreviewed |
| OpenHands | Canvas Apps have no isolation for app code (local backends only). | beta | https://docs.openhands.dev/openhands/usage/agent-canvas/canvas-extensions.md | reviewed |
| OpenClaw | `openclaw security audit --deep` scans plugin and skill code (500-file cap, skips >1 MiB) plus gateway probes; auto-fix only for filesystem permissions. | GA | https://docs.openclaw.ai/gateway/security/audit-checks | unreviewed |
| goose | Admin extension allowlist (exact command match); malware check on external extensions before activation (mechanism undocumented); `goose://extension` deeplinks only accept `jbang`, `npx`, `uvx`, `goosed`, `docker`. | GA / unclear | https://goose-docs.ai/docs/guides/allowlist.md, https://goose-docs.ai/docs/getting-started/using-extensions.md | reviewed |
| Hermes Agent | Plugins opt-in; declared capabilities consent-recorded, off by default; install-time static scan safe/caution/dangerous (dangerous not overridable); `--ref` pins a 40-char SHA; optional `plugins.isolation: host` separate process. Docs: a malicious in-process plugin can ignore the gates. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/plugins | reviewed |
| Warp | File-based `~/.warp/.mcp.json` servers auto-spawn; project-scoped and third-party servers and config edits need approval. | GA | https://docs.warp.dev/agents/capabilities/mcp/ | unreviewed |
| Cline | SDK plugins run in a worker thread with opt-in sandbox, 3 s timeout, disabled after 5 failures. | GA | https://docs.cline.bot/sdk/guides/permission-handling.md (survey: SDK plugins and hooks) | reviewed |
| rizzy-harness (ours) | No plugins, MCP, skills or hooks; the only external helper is optional `graphify` (install-by-HTTP removed in `1fa8f27`). | none | `crates/tools/src/lib.rs:385-396`, `crates/tools/src/graphify.rs` | from own inventory |

#### F30. Authentication of the harness's own server and web UI

What it is: how the harness's local or self-hosted HTTP/WebSocket server decides who may use it.

Variants: loopback with no auth plus mandatory token for any wider bind (OpenCodex refuses to start; ours needs `--insecure-bind`); launch token exchanged for a cookie with a Host fence (DeepSeek); password/Basic auth plus pairing links (OpenCode); shared secret plus TLS fingerprint pinning (goose); no auth at all (OpenHands "unauthenticated, single-tenant", Cline Kanban and hub, Codex app-server non-loopback listeners "during rollout"). Accounts with roles exist only in ours, OpenClaw (operator scopes) and the Claude apps gateway (OIDC).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | App-server WebSocket auth `capability-token` or `signed-bearer-token`, enforced before `initialize`; requests with `Origin` get 403; non-loopback listeners allow unauthenticated connections by default during rollout. | experimental | https://learn.chatgpt.com/docs/app-server.md | reviewed |
| OpenCodex | Loopback default with no token; non-loopback bind refuses to start without `OPENCODEX_API_AUTH_TOKEN`; separate admin token kept in memory; optional "remember device" (off by default, 30-day record, HttpOnly SameSite=Strict cookie plus device proof) that cannot perform consent actions; Host/Origin checks. | GA | https://opencodex.me/guides/web-dashboard/, https://github.com/lidge-jun/opencodex | corrected |
| OpenCode | Web UI password-protected by default (`OPENCODE_PASSWORD`, Basic auth user `opencode`); `opencode pair` single-use 5-minute link and QR code, 30-day session cookie; `--remote` via end-to-end encrypted OpenTunnel with password still required; localhost default. | GA | https://opencode.ai/docs/cli/web/ | reviewed |
| Claude Code | IDE bridge: loopback `ws://` MCP server with a per-activation token in a lock file. Claude apps gateway: RFC 8628 device grant to the gateway, OIDC with PKCE upstream, HS256 session JWTs (TTL 1 h), CIDR allow/deny, rate limits; no per-session revocation (rotate the secret). | GA | https://code.claude.com/docs/en/vs-code.md, https://code.claude.com/docs/en/claude-apps-gateway-config.md, https://code.claude.com/docs/en/claude-apps-gateway-deploy.md | unreviewed |
| DeepSeek harness | Printed startup URL carries a launch token exchanged for a signed session cookie (`Secure` with TLS); request fence accepts only loopback, the bind address and `--trusted-host` (others 403); Origin checks on every route; `0.0.0.0` rejected; TLS never falls back to HTTP; one credential per process, no accounts. | preview | https://deepseek-harness.github.io/deepseek-harness/en/guide/public-deployments, https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/web-server | reviewed |
| Oh My Pi | `omp auth-broker` loopback-only by default with bearer token, plain HTTP; health endpoint unauthenticated. | GA | https://omp.sh/docs/secrets | corrected |
| OpenHands | Agent Server has no auth by default; `X-Session-API-Key` with rotation; `OH_SECRET_KEY` encrypts stored values; Canvas auto-generates a local key, required with `--public`; loopback default; anyone with the key can request execution. | GA | https://docs.openhands.dev/sdk/arch/agent-server.md, https://docs.openhands.dev/openhands/usage/agent-canvas/setup.md | reviewed |
| OpenClaw | Gateway auth (token/password, Tailscale Serve identity, trusted-proxy headers, `none`); every client signs a `connect.challenge` nonce with a device identity; new devices need approval; loopback auto-approved. Non-loopback binds require auth. | GA | https://docs.openclaw.ai/gateway/pairing, https://docs.openclaw.ai/gateway/remote | corrected |
| goose | `goose serve` shared secret `GOOSE_SERVER__SECRET_KEY` (`X-Secret-Key`), `--dangerously-unauthenticated` for local only; self-signed TLS with printed fingerprint for pinning, else TOFU; secret stored plaintext in the launch plist and Desktop settings; manual rotation. | unclear | https://goose-docs.ai/docs/guides/remote-goose-server.md | reviewed |
| Hermes Agent | Messaging gateway default deny; DM pairing 8-char codes from `secrets.choice`, 1 h expiry, 5 failures give 1 h lockout, files 0600; admin vs regular users. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/security | unreviewed |
| Cline | Hub daemon (127.0.0.1:25463): no authentication documented. Kanban (127.0.0.1:3484): no built-in auth, docs rely on tunnels. Hub dashboard needs `--room-secret` for non-local hosts. | GA / preview | https://docs.cline.bot/sdk/architecture/hub-spoke.md, https://docs.cline.bot/kanban/remote-access.md, https://docs.cline.bot/cli/cli-reference.md | corrected |
| rizzy-harness (ours) | Accounts (Argon2id, admin-created, first admin via claim code), server-side session table (cookie holds random id, SHA-256 stored, 30-day sliding), `HttpOnly; SameSite=Strict`, Host allowlist, Origin check, constant-time admin-equivalent API token, 5 failures per user per 15 min. Default bind `127.0.0.1:7777`; wider bind needs `--insecure-bind` or a WireGuard address; `--tls` self-signed. Pre-auth DoS (S4), sockets outliving logout (S5), no HTTP timeouts (S6) open on `main`, fixed on branch. | works | `crates/master/src/auth.rs:50`, `:445-470`, `:506-580`, `crates/cli/src/main.rs:33-70`, `crates/cli/src/net.rs` | from own inventory |

#### F31. Authentication of remote links (nodes, workers, remote control)

What it is: how a worker, executor or remote-control client proves its identity to the central server, and the reverse.

Variants: outbound-only polling to a vendor control plane with short-lived single-purpose credentials (Claude Code Remote Control and self-hosted runners, Cursor self-hosted workers, Warp self-hosted workers, Hermes Relay, Antigravity reverse tunnel); device keys signing a challenge plus a second approval of declared capabilities (OpenClaw two-layer pairing); encrypted RPC with Ed25519-signed ECDH (OpenCodex Remote Workspace); SSH as the transport (Cline, Claude Desktop, DeepSeek); peer-to-peer QUIC with key exchange (goose roaming). Ours: pinned self-signed TLS plus Ed25519 node key signing a nonce bound to the cert fingerprint, single-use 15-minute join token.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Remote control prints short-lived pairing codes; remote TUI uses `--remote-auth-token-env`, plain ws only on localhost or through SSH; relay architecture and encryption undocumented. | GA (RC), experimental (remote TUI) | https://learn.chatgpt.com/docs/developer-commands.md, https://learn.chatgpt.com/docs/app-server.md | reviewed / unreviewed |
| OpenCodex | Remote hub: single-use 5-minute pairing code via stdin, key rotation with 10-minute overlap, data listener plain HTTP (TLS from operator). Remote Workspace: executor dials out over WebSocket, encrypted RPC v2 (Ed25519-signed ECDH, AES-256-GCM), hub stores only a bearer hash, 10 failed codes in 10 minutes give 429. | GA (hub), experimental (workspace) | https://opencodex.me/guides/remote-hub/, https://opencodex.me/guides/remote-workspace/ | corrected |
| Claude Code | Remote Control: outbound HTTPS only, registers and polls, several short-lived single-purpose credentials; transcript stored server-side while connected; inference-only setup-tokens rejected. Trusted Devices (beta) needs an enrolled device credential plus a sign-in under 18 hours. Self-hosted runners: environment secret (365 days) registers runners; orchestrator mode gives each spawned runner a single-use work-order JWT registering exactly one runner, expiring with the spawn lease (default 120 s); one owner per runner; session JWT (ES256, 4 h default) for internal services, no revocation feed. Gateway TLS leaf pinned per hostname and required to be on private addresses. | GA (RC), beta (runners, Trusted Devices) | https://code.claude.com/docs/en/remote-control.md, https://code.claude.com/docs/en/self-hosted-environments-configuration.md, https://code.claude.com/docs/en/self-hosted-environments-identity.md, https://code.claude.com/docs/en/claude-apps-gateway.md | reviewed / corrected (work order scope) |
| Claude Desktop | Remote Control (Owner toggle for Team/Enterprise; not with API keys, custom base URL or apps gateway). SSH sessions via `~/.ssh/config`, forwarding disabled, admin `sshHostAllowlist`, `disableSshSavedPasswords`. | GA | https://code.claude.com/docs/en/remote-control, https://code.claude.com/docs/en/desktop | corrected |
| DeepSeek harness | SSH backend: auth and host keys from existing OpenSSH config, helper checked against a configured SHA-256 hash, SSH loss is final. | documented (no label) | https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/subsystems/ssh.md | corrected |
| Cursor | Self-hosted workers: one long-lived outbound HTTPS connection, no inbound ports; controller claims requests and the worker connects with a single-claim session token (5-minute claim token); per-claim OIDC identity JWTs; wire protocol and encryption undocumented. | unclear (no label) | https://cursor.com/docs/cloud-agent/self-hosted, https://cursor.com/docs/cloud-agent/self-hosted/pool | reviewed |
| Antigravity | Remote Control reverse tunnel opened by the host; same Google account required; headless daemon as an OS service; relay and encryption undocumented. | GA | https://antigravity.google/docs/remote-control | reviewed |
| OpenClaw | Device pairing (requests expire in 5 min) then node capability approval (pending ones do not expire); optional SSH-verified auto-approval (default on) and trusted-CIDR auto-approval (off); scope or key changes always need explicit approval; node commands run only if declared, approved on the paired record and in the Gateway's allowlist. Cloud workers: closed method allowlist, per-dispatch hashed credential, owner-epoch fencing. `ws://` only for loopback/private, public must be `wss://`, TLS fingerprint pinning. | GA (pairing), beta (cloud workers) | https://docs.openclaw.ai/gateway/pairing, https://docs.openclaw.ai/nodes/command-policy, https://docs.openclaw.ai/gateway/cloud-workers/security-model, https://docs.openclaw.ai/gateway/remote | corrected / reviewed |
| goose | Roaming: iroh/QUIC, long-lived node identity, both sides accept the other's key, all-or-nothing ACP access, revoke closes connections in seconds (spawned processes survive); not in released binaries. | experimental | https://goose-docs.ai/docs/guides/roaming-agents.md | reviewed |
| Hermes Agent | Hermes Relay: gateway dials out over one authenticated WebSocket with a per-gateway secret and delivery key, holds no platform credentials, no inbound port. | experimental | https://hermes-agent.nousresearch.com/docs/user-guide/messaging/relay | reviewed |
| Zed/ACP | ACP remote transport (Streamable HTTP and WebSocket at `/acp`) is an RFD; origin validation and version header hardening still to do; stdio is the only standardised transport. | planned | https://agentclientprotocol.com/rfds/streamable-http-websocket-transport.md | reviewed |
| Warp | Self-hosted workers connect outbound only (Enterprise); five backends. | GA (per review) | https://docs.warp.dev/platform/architecture/ | reviewed |
| Cline | SSH remote: key-based login only, host key must already be trusted, strict host-key checking, provider key sent per session over the tunnel and not written to the host. | GA | https://docs.cline.bot/usage/cline-desktop-ssh.md | reviewed |
| rizzy-harness (ours) | WebSocket over TLS 1.2/1.3 with a pinned self-signed master cert fingerprint; Ed25519 node key signs a nonce bound to the fingerprint; admin-created single-use 15-minute join token stored hashed; revoke drops the link; 10 s handshake steps, 32 concurrent pre-auth handshakes. Master reads node frames into an unbounded channel on `main` (AUDIT L1; bounded on branch). Node signs, master is pinned: not mutual challenge-response (AUDIT L15). | works (L1 open) | `crates/link/src/lib.rs:43-49`, `crates/master/src/nodes.rs:16-80`, `crates/link/src/server.rs:23-24`, `docs/security-node-link.md` | from own inventory |

#### F32. Telemetry defaults and opt-outs

What it is: what the harness sends to its vendor by default, and how to stop it.

Variants: default on with content excluded (Claude Code metrics, Cline, Pi install telemetry, OpenCodex since v2.100.0, Antigravity "interactions collected" for consumer accounts, OpenClaw daily version check that `telemetry off` does not stop); default on with content (DeepSeek session-log upload for official API use); opt-in only (goose, Codex OTel); none stated (Hermes, Oh My Pi, ours).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Built-in Statsig metrics follow `analytics.enabled`; OTel export off by default; project config cannot set `otel`. | GA | https://learn.chatgpt.com/docs/config-file/config-reference.md | unreviewed |
| OpenCodex | Default-on anonymous daily summary after a consent popup (install hash, version, platform, provider kind, counts; no prompts or keys); `ocx telemetry disable`, `OCX_TELEMETRY=0`, `DO_NOT_TRACK=1`. Arrived in v2.100.0 the day of the survey. | GA | https://github.com/lidge-jun/opencodex | unreviewed (release-feed source not re-opened) |
| OpenCode | Telemetry for the local app: nothing found in pages opened. | nothing documented | https://opencode.ai/docs/ | unreviewed |
| Claude Code | Metrics to Anthropic by default on Anthropic API sessions, never code or prompts; error reports only for Pro/Max/Ultra direct; off by default on Bedrock/Vertex/Foundry. `DISABLE_TELEMETRY`, `DO_NOT_TRACK`, `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` (also disable feature flags and so Remote Control). WebFetch hostname preflight to `api.anthropic.com` on every provider unless `skipWebFetchPreflight`. `/feedback` uploads code. | GA | https://code.claude.com/docs/en/data-usage.md, https://code.claude.com/docs/en/env-vars.md | reviewed |
| Claude Desktop | 3P: crash reports and analytics scrubbed and disableable; only `downloads.claude.ai` required egress when other services are disabled. | GA | https://claude.com/docs/third-party/claude-desktop/overview.md | unreviewed |
| DeepSeek harness | DeepSeek session-log upload on by default for official-API use (up to 8 MiB per request), opt out in Settings or config; OTel session telemetry `FEEDBACK_ONLY` default, `FULL` rejected, `DISABLED` turns off; the seam ships no redaction rules. | preview | https://github.com/deepseek-ai/deepseek-harness/blob/master/apps/cli/reference/README.md, https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/session-telemetry | reviewed |
| Oh My Pi | No default telemetry documented (opt-in OTEL, auto-QA reports, update checks). | GA | https://omp.sh/docs/env | unreviewed |
| Pi | `enableInstallTelemetry` default true (content not described), `enableAnalytics` default false, `PI_TELEMETRY` and `PI_SKIP_VERSION_CHECK`. | GA | https://pi.dev/docs/latest/settings | reviewed |
| Antigravity | Interactions collected to improve the product and models, opt-out in App Settings; always off for business accounts; consumer default not stated. | GA | https://antigravity.google/docs/settings | unreviewed |
| OpenClaw | Daily version check (version, OS, Node, arch; approximate location by Cloudflare, kept 3 months), update outcome, install count; opt-in feature stats; `telemetry off` and `DO_NOT_TRACK` do not stop the daily check, `update.checkOnStart: false` does; CI sends nothing. | GA | https://docs.openclaw.ai/gateway/telemetry | reviewed |
| goose | Opt-in anonymous usage data (`GOOSE_TELEMETRY_ENABLED` default false); no conversations, code or tool arguments. | GA | https://goose-docs.ai/docs/guides/usage-data.md | unreviewed |
| Hermes Agent | Docs state API calls go only to the configured provider and data stays in `~/.hermes/`. | GA | https://hermes-agent.nousresearch.com/docs/guides/secure-hermes-on-a-work-machine | unreviewed |
| Zed/ACP | Zed does not retain prompts or code by default; ratings send the full thread; Edit Prediction training data only if opted in; master switch `disable_ai`. | GA | https://zed.dev/docs/ai/privacy-and-security.md | unreviewed |
| Warp | "Help improve Warp" analytics (Rudderstack) and Sentry crash reports, two toggles; published event table; Business/Enterprise under ZDR. | GA | https://docs.warp.dev/support-and-community/privacy-and-security/privacy/ | unreviewed |
| Cline | On by default in all products, even with own keys; excludes code and prompts except Cline/ClinePass provider requests; opt-outs per surface; admins force off in VS Code/JetBrains only. | GA | https://docs.cline.bot/enterprise-solutions/monitoring/telemetry.md | reviewed |
| rizzy-harness (ours) | Not in the inventory (no exporter found; logs local only). | not in the inventory | `own-rizzy-harness.md` section 16 | from own inventory |

#### F33. Data retention and local data at rest

What it is: what session data the harness keeps, where, for how long, encrypted or not, and how to delete it.

Variants: plaintext local transcripts with a sweep (Claude Code 30 days), vendor-side retention by plan (Claude Code, Cursor, Zed's Anthropic carve-out), indefinite (Cursor cloud transcripts, OpenClaw dreaming memories, ours event log), crypto-shredding (ours blobs).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| OpenCode | Docs warn snapshot and heap-dump data may contain secrets. | GA | https://opencode.ai/docs/snapshots/, https://opencode.ai/docs/troubleshooting/ | unreviewed |
| Claude Code | `~/.claude` plaintext transcripts (including system prompt and tool definitions), tool outputs, checkpoints, paste cache; swept after `cleanupPeriodDays` (default 30) only when Claude Code starts; `history.jsonl` kept until deleted; nothing encrypted at rest; `claude purge`. Vendor side: consumer 5 years if training allowed else 30 days; commercial 30 days; ZDR for qualified Enterprise; `/feedback` 5 years. | GA | https://code.claude.com/docs/en/claude-directory.md, https://code.claude.com/docs/en/data-usage.md, https://code.claude.com/docs/en/zero-data-retention.md | reviewed (vendor retention), unreviewed (local) |
| Claude Desktop | Deleted Cowork tasks purged from backend within 30 days; ZDR and HIPAA orgs cannot use cloud sessions. | GA | https://code.claude.com/docs/en/desktop | unreviewed |
| Oh My Pi | `agent.db` unencrypted at rest. | GA | https://omp.sh/docs/secrets | unreviewed |
| Pi | Session exports and debug logs can hold sensitive data (docs warn); encryption at rest not stated. | GA | https://pi.dev/docs/latest/cli | unreviewed |
| Cursor | Cloud conversation state kept indefinitely by default (deletable via API; Enterprise can cap); VM snapshots with cloned code kept 30 days after inactivity, not deletable on demand; privacy page says repository copies deleted after the run (contradiction). Privacy Mode: no training, Enterprise default. | GA | https://cursor.com/docs/cloud-agent/security, https://cursor.com/docs/enterprise/privacy-and-data-governance | reviewed / corrected |
| OpenClaw | "Dreaming" memory promotion default-on with no time-based retention limit (project's own admission). | GA | https://docs.openclaw.ai/start/why-openclaw | reviewed |
| Zed/ACP | No retention by default for Zed-hosted models except Anthropic "Covered Models" retained at least 30 days for safety review (no opt-out, also with own key). | GA | https://zed.dev/docs/ai/privacy-and-security.md | unreviewed |
| Warp | Warp-hosted clones destroyed with the sandbox; prompts, transcripts and artifacts may persist as run data. | GA | https://docs.warp.dev/platform/architecture/ | reviewed |
| rizzy-harness (ours) | Append-only SQLite event log keeps every streaming delta forever (AUDIT D5); chat delete overwrites events (secure delete); incognito chats purged after idle TTL (default 1440 min); blobs sealed with AES-256-GCM with per-blob keys in `master.db` (crypto-shredding, not confidentiality at rest, AUDIT L8). | works (D5 weakness) | `crates/node/src/lib.rs:537-589`, `crates/store/src/lib.rs:485-490`, `crates/master/src/blobs.rs:1-12` | from own inventory |

#### F34. Documented trust model and stated non-boundaries

What it is: the vendor's own written statement of what its controls do not protect against.

Variants: almost every serious harness now states that its permission layer is not a security boundary; the differences are what they recommend instead (container/VM: Pi, Claude Code, DeepSeek, Oh My Pi; separate gateways per trust domain: OpenClaw; MDM: Claude Code) and whether they publish a "will not fix" list (OpenClaw, Pi).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Candid limits: DNS-rebinding protection best effort, deny-read globs are a per-process snapshot (files created later readable), proxy does not cover search, MCP or apps. | GA | https://learn.chatgpt.com/docs/permissions.md | reviewed |
| Claude Code | Bash rules "not a security boundary"; sandbox "not a complete isolation boundary"; auto mode "does not guarantee safety"; server-managed settings not a security boundary; working directory is a prompt, not enforcement; bypass offers no protection against prompt injection. | GA | https://code.claude.com/docs/en/permissions.md, https://code.claude.com/docs/en/sandboxing.md, https://code.claude.com/docs/en/permission-modes.md, https://code.claude.com/docs/en/server-managed-settings.md | reviewed |
| DeepSeek harness | SAFETY.md: sandbox, approvals and permissions reduce risk but do not guarantee isolation; no security audit; `web_fetch` can exfiltrate; proxy passwords visible to commands. | preview | https://github.com/deepseek-ai/deepseek-harness/blob/master/SAFETY.md | unreviewed |
| Oh My Pi | Approvals are "not a sandbox"; subagent isolation "not a security sandbox". | GA | https://omp.sh/docs/approvals, https://omp.sh/docs/subagents | reviewed |
| Pi | Watching the transcript, project trust and reviewing changes are not security boundaries; prompt injection and lack of sandbox out of scope for reports. | GA | https://pi.dev/docs/latest/security | reviewed |
| Cursor | Auto-review is not a security boundary; run-mode guardrails "best-effort". | GA | https://cursor.com/docs/agent/security | reviewed |
| OpenHands | Meant for a single user on a local workstation, no built-in authentication, isolation or scalability; Helm chart "an unauthenticated, single-tenant application"; analyzers "not a sandbox replacement". | GA statement | https://docs.openhands.dev/overview/faqs.md, https://docs.openhands.dev/openhands/usage/agent-canvas/backend-setup/kubernetes.md | corrected |
| OpenClaw | One Gateway equals one trust boundary; not hostile multi-tenant; anyone who can message a tool-enabled agent shares its authority; `sessionKey` is a routing selector, not auth; published no-action triage list. | GA | https://docs.openclaw.ai/gateway/security/trust-model | reviewed |
| Hermes Agent | Deny rules and write guards protect against honest mistakes, not a hostile agent; in-process plugins can ignore gates; Kanban single-host threat model. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/security | reviewed |
| Zed/ACP | ACP does not sandbox agents; permission requests advisory; out of scope: malicious agent binaries, injection detection, OS sandboxing. Zed sandbox lists side channels (proc macros, Makefile run in the normal terminal). | GA (guidance) | https://agentclientprotocol.com/protocol/v1/security.md, https://zed.dev/docs/ai/sandboxing.md | unreviewed / reviewed |
| Warp | Redaction is "a backstop, not a substitute" for narrow scopes. | GA | https://docs.warp.dev/platform/architecture/ | reviewed |
| Cline | Plan mode is not a security boundary (`python -c` can still write); `.clineignore` not a security boundary. | GA | https://docs.cline.bot/features/subagents.md, https://docs.cline.bot/customization/clineignore.md | reviewed |
| rizzy-harness (ours) | README says "The shell is not confined yet" and tells users to keep it on `127.0.0.1` for themselves only; redaction called a courtesy in PLAN T3. | works (as statement) | `README.md:7-13`, PLAN T3 | from own inventory |

## 3. Availability matrix

Legend: `Y` documented and released; `B` beta, experimental, preview or partial; `P` planned or vendor-claim only (for Ours: exists only on an unmerged branch or in PLAN); `-` nothing documented; `?` unverified.

| Feature | Codex | OpenCodex | OpenCode | Claude Code | Claude Desktop | DeepSeek | Oh My Pi | Pi | Cursor | Antigravity | OpenHands | OpenClaw | goose | Hermes | Zed/ACP | Warp | Cline | Ours |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| F1 Approval modes | Y | - | Y | Y | Y | B | Y | - | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y |
| F2 Per-tool/command rules | B | - | Y | Y | Y | - | Y | B | Y | Y | B | Y | Y | Y | Y | Y | Y | B |
| F3 Shell chain/construct parsing | B | - | Y | Y | - | - | Y | - | B | Y | - | Y | - | Y | Y | - | - | - |
| F4 Approval binding and scope | Y | - | Y | Y | - | B | - | - | - | Y | - | Y | Y | Y | Y | - | - | B |
| F5 Model-based approver | Y | - | - | Y | Y | B | - | - | Y | Y | Y | Y | B | Y | - | Y | Y | P |
| F6 Hardcoded floors | - | - | - | Y | - | - | Y | - | Y | - | - | - | - | Y | Y | Y | - | - |
| F7 Protected paths | Y | - | - | Y | - | - | - | - | Y | Y | - | - | - | - | B | - | - | B |
| F8 Workspace boundary (file tools) | Y | - | Y | Y | Y | - | - | - | Y | Y | - | Y | - | - | - | Y | Y | Y |
| F9 Admin/managed policy | Y | - | Y | Y | Y | - | - | - | Y | B | B | Y | Y | Y | - | Y | Y | Y |
| F10 Repo config cannot widen | Y | - | - | Y | - | B | - | Y | - | - | - | - | - | - | - | - | - | Y |
| F11 OS sandbox (shell) | Y | B | - | Y | Y | B | - | - | Y | Y | - | - | ? | - | B | - | ? | P |
| F12 Sandbox-unavailable behaviour | Y | - | - | Y | Y | B | - | - | Y | Y | - | - | - | - | B | - | - | P |
| F13 Sandbox network egress | B | - | - | Y | Y | - | - | - | Y | Y | - | - | - | Y | B | - | - | P |
| F14 Escape hatch / widening | Y | - | - | Y | - | B | - | - | - | Y | - | Y | - | - | B | - | - | - |
| F15 Container/VM isolation | Y | B | - | Y | Y | - | - | Y | Y | - | Y | Y | ? | Y | - | Y | - | - |
| F16 Sub-agent inheritance | Y | - | Y | Y | - | - | Y | - | - | Y | - | - | Y | - | - | - | Y | Y |
| F17 Unattended behaviour | Y | - | - | Y | - | B | Y | - | Y | - | - | Y | - | Y | - | - | Y | Y |
| F18 Remote approval delivery | Y | - | - | Y | Y | - | - | - | - | Y | - | Y | - | Y | - | - | Y | Y |
| F19 Hooks in permission path | Y | - | - | Y | - | - | - | Y | Y | Y | Y | - | - | - | - | - | Y | - |
| F20 Decisions as logged events | Y | - | - | Y | Y | B | - | - | - | - | - | - | ? | - | - | - | - | Y |
| F21 Env scrubbing for children | Y | - | - | Y | - | - | - | - | - | - | - | Y | - | Y | - | - | - | Y |
| F22 Credential placeholder/proxy | Y | Y | - | Y | Y | - | Y | Y | Y | - | - | B | - | Y | - | Y | - | - |
| F23 Secret redaction | Y | - | - | Y | - | - | B | - | - | - | Y | - | - | Y | - | Y | - | B |
| F24 Own credential storage | Y | - | - | Y | Y | B | Y | Y | - | Y | - | - | Y | Y | - | - | Y | Y |
| F25 Prompt-injection defences | Y | Y | - | Y | Y | B | - | - | - | - | Y | Y | ? | Y | Y | - | - | Y |
| F26 Inter-agent output hygiene | - | - | - | Y | - | - | - | - | - | - | - | - | - | - | - | - | - | B |
| F27 SSRF guards | B | - | - | Y | - | B | - | - | - | - | - | - | - | Y | - | - | - | Y |
| F28 Project trust prompt | Y | - | - | Y | - | - | - | Y | Y | Y | - | - | - | - | Y | - | - | - |
| F29 Extension/plugin trust | Y | - | Y | Y | - | B | - | - | - | - | B | Y | Y | Y | - | Y | Y | - |
| F30 Own server/UI auth | B | Y | Y | Y | - | B | Y | - | - | - | Y | Y | ? | Y | - | - | B | Y |
| F31 Remote link auth | Y | B | - | Y | Y | Y | - | - | ? | Y | - | Y | B | B | P | Y | Y | Y |
| F32 Telemetry stated | Y | Y | - | Y | Y | B | Y | Y | - | Y | - | Y | Y | Y | Y | Y | Y | - |
| F33 Retention / data at rest stated | - | - | Y | Y | Y | - | Y | Y | Y | - | - | Y | - | - | Y | Y | - | Y |
| F34 Stated non-boundaries | Y | - | - | Y | - | B | Y | Y | Y | - | Y | Y | - | Y | Y | Y | Y | Y |

Notes on cells: Pi F1 `-` because it documents having no approval system. goose F11 `?` and F15 `?`: macOS sandbox seen only in search snippets; `--container` status unclear. goose F30 `?` because `goose serve` has no maturity label (row present, status unclear); I kept `?` to flag that rather than `Y`. Cline F11 `?`: `CLINE_SANDBOX` meaning unstated. Cursor F31 `?`: no release label for self-hosted workers. Zed/ACP F31 `P`: ACP remote transport is an RFD (Zed's own SSH remote development is not an agent-link auth mechanism here). Ours F11/F12/F13 `P`: Landlock code exists only on `integration/gate3`.

## 4. Common across the ecosystem

"Real harnesses" here are the sixteen excluding OpenCodex (a gateway). Half is eight. Counts use `Y` or `B` in the matrix.

| Feature | Count (of 16) | Comment |
|---|---|---|
| F1 Approval modes | 15 | Everyone but Pi, which documents not having one. Defaults split three ways (F1 Variants); ours is prompt-by-default, the conservative end. |
| F2 Per-tool/command rules | 15 | Universal (several partial). Ours has only whole-tool `denied_tools`. |
| F32 Telemetry stated | 13 | Defaults vary widely: on by default without content (Claude Code, Cline, Pi install telemetry, OpenClaw version check), session-log upload on by default (DeepSeek), opt-in (goose). |
| F34 Stated non-boundaries | 12 | The norm is to say in writing that rule lists and approvals are not a boundary. |
| F9 Admin/managed policy | 12 | Mostly org-level push from a vendor console, MDM or system file. |
| F5 Model-based approver | 12 | The fastest-moving area. Codex, Claude Code, Cursor and OpenClaw ship a separate reviewer; Codex, Claude Code and OpenClaw add a circuit breaker back to a human. |
| F24 Own credential storage | 10 | Keyring with plaintext fallback is the norm. |
| F29 Extension/plugin trust | 10 | Everyone who has plugins says they are unsandboxed. |
| F31 Remote link auth | 10 (Cursor `?`, Zed/ACP `P` not counted) | Dominant pattern: outbound-only connection from worker to control plane with short-lived credentials. |
| F3 Shell parsing | 9 | Every harness that parses converges on the same rule: split plain chains, refuse or exact-match anything with substitution. |
| F8 Workspace boundary | 9 | Usually enforced only for built-in file tools, not the shell. |
| F15 Container/VM isolation | 9 (goose `?` not counted) | Mostly vendor cloud VMs or documented recipes. |
| F22 Credential placeholder/proxy | 9 | The strongest-growing secret pattern; ours has none. |
| F30 Own server/UI auth | 9 (goose `?` not counted) | Ranges from strong (ours, DeepSeek launch token, OpenCode password) to none (OpenHands default, Cline hub and Kanban). |
| F33 Retention stated | 9 | |
| F17 Unattended behaviour | 8 | Deny-and-continue is converging as the safe default; Cline and Cursor cloud are the exceptions. |
| F25 Prompt-injection defences | 8 (goose `?` not counted) | Mostly content marking and pattern scans; nobody claims to solve it. |

Just under half (seven each), listed because they matter for the comparison: F11 OS sandbox for the shell (Codex, Claude Code, Claude Desktop, DeepSeek, Cursor, Antigravity, Zed; goose and Cline unverified), F16 sub-agent inheritance, F18 remote approval delivery, F19 hooks in the permission path. Seatbelt on macOS is used by every harness with a macOS sandbox; Linux splits between bubblewrap (Codex, Claude Code, Zed, Antigravity) and Landlock plus seccomp (Cursor; DeepSeek offers both).

## 5. Only one harness has it

- **Approval binding to exact argv, cwd, env and executable content hash, re-checked before launch (OpenClaw, F4).** Interesting: it closes the "approve `git status`, PATH now points to a different `git`" hole that every prefix-rule design leaves open. Directly copyable for our node, which already resolves paths component by component.
- **Command rules with inline unit tests (`match`/`not_match`) checked at load (Codex, F2).** Interesting: rule lists rot; tests in the rule file make a bad rule fail at startup instead of in production.
- **Per-command network grants reviewed with the command (Claude Code auto mode, F13).** Interesting design (capability scoped to one invocation, adds nothing to session state), but it depends on a classifier and a proxy we do not have.
- **Sentinel credential masking with SigV4 re-signing (Claude Code sandbox, F22).** Interesting but heavy: needs TLS termination (labelled experimental) and has documented failure modes (macOS blocks the file, a non-matching pattern leaves the real file readable). Hermes's iron-proxy is the simpler variant of the same idea.
- **Approval-audit append failure rejects the request (DeepSeek, F20).** Interesting and cheap: "no unlogged decision" matches our one-writer event log exactly.
- **Approval requests that deliberately omit tool arguments (DeepSeek, F4/F20).** Not interesting as UX (the reviewer flags it as weak): an approver who cannot see the arguments is approving blind.
- **Hooks trusted by content hash (Codex, F29).** Interesting for any future hooks: a repo or plugin that changes a reviewed hook is re-prompted.
- **Sub-agent report scanning that neutralises harness-tag imitations (Claude Code, F26).** Interesting, cheap, and fits our sub-agent and cross-node reports.
- **Private-address requirement and TLS pinning for the settings gateway (Claude Code apps gateway, F31).** Interesting principle: a server that can push settings that run commands must itself be pinned. Our node pins the master already.
- **Mined allowlist suggestions that never apply themselves and never propose destructive classes (Hermes, F2).** Interesting UX for reducing prompt fatigue without auto-widening.
- **One-owner-per-runner lock (Claude Code self-hosted, F31).** Interesting for multi-user nodes: a runner that served user A never serves user B without a fresh disk.
- **Editable approval scope in the prompt (Antigravity, F4).** Mildly interesting: lets the user widen a file approval to its directory deliberately instead of clicking "always".
- **Backend-reported `full` vs `partial` sandbox enforcement (DeepSeek, F12).** Interesting and honest: tells the user when Landlock ABI or platform leaves gaps. Our branch's ABI 9 socket gap is exactly such a case.
- **Provenance-gated `web_fetch` (ours, F25).** No other harness documents refusing URLs that did not appear in user or tool text.

## 6. Ours against the field

Ranked by how much it matters for a self-hosted master/node harness that runs shell commands on other people's machines.

1. **No shell sandbox on `main` (F11, F12, F13). This is the single largest gap, and it is worse than it looks.** Seven of sixteen harnesses document an OS sandbox for the shell (two more unverified); Codex, Cursor, Antigravity and Zed confine shell commands by default or in the recommended mode. Ours runs `bash` as the node's OS user, able to read the data directory, with no network control (AUDIT S1). The Landlock sandbox exists only on the unmerged `integration/gate3` branch; until it merges, every compensating control we list (admin-only embedded node, `denied_tools`, file-tool confinement) is a permission-layer control, which every vendor in F34 says is not a boundary. Two blunt facts about the branch design: Codex rejects Landlock for filesystem-restricted policies because it cannot isolate Unix sockets, and our own branch notes the same gap (pathname sockets such as `docker.sock` and `ssh-agent` reachable below ABI 9) plus no UDP/DNS coverage. Its fail-closed `mode = required` default is the right call and is stricter than Claude Code's silent fallback (F12). Merging gate3 is the top priority in these areas; reporting `full`/`partial` enforcement per DeepSeek would make the socket gap visible to the operator.

2. **Every node holds provider keys (F22).** The field is moving to "workers never hold the model key": OpenClaw cloud workers, Oh My Pi auth-gateway, OpenCodex hub, Warp and Claude Code cloud all proxy inference or attach credentials outside the agent's boundary. In our design a compromised node, or an unconfined `bash` on a node, can read the provider key from the node's `config.toml`. The master already exists as a natural inference proxy; routing node inference through the master (or a node-local credential proxy outside the sandbox) would remove secrets from the most exposed machine. This interacts with the "harness stays light" invariant only lightly (it is a forwarder), and with "master never runs tools" not at all.

3. **Approval binding is per tool name per turn (F4).** One click on "allow for this session" allows every later `bash` command of that turn (AUDIT L10), and the label overstates it. The field ranges from prefix rules (Codex, OpenCode) to OpenClaw's argv, cwd, env and executable-hash binding. Gate3 fixes the label and the first-approval semantics; it does not add argument-level binding. Given that we have no sandbox, the binding is currently the only thing between one approval and arbitrary commands.

4. **No per-command rules and no shell parsing (F2, F3).** Fifteen of sixteen have rules; ours has `denied_tools`. Every vendor says Bash rules are UX, not security, so this matters less than items 1-3, but `ask` prompting for every `ls` is the prompt-fatigue failure that drives users to `auto`. If we add rules, the converged design is clear: split plain chains, treat anything with substitution or variables as opaque requiring exact match (Codex, Antigravity), refuse unparseable input (OpenCode), deny beats ask beats allow regardless of specificity (Claude Code). Hermes's "suggest, never apply" mining is a good fit for the event log.

5. **Where we are ahead: node authority and repo config (F9, F10).** "The node decides" is stronger than anything comparable: Claude Code's self-hosted runner and Cursor's workers take policy from the vendor control plane, and Claude Code says its own server-managed settings are a client-side control. OpenClaw's two-layer node pairing (device identity, then declared capability approval) is the closest peer and is worth comparing against our pairing. We also read no repository config at all, so the whole F10/F28 attack class (repo hooks running under `-p`, repo `.mcp.json`, repo settings widening allow rules) does not exist for us yet. The day we read `AGENTS.md` or any repo config, we need a trust prompt and a repo-ignored-keys list from the start.

6. **Where we are ahead: unattended runs (F17) and the approval event trail (F20).** Agentic routines cannot run in `auto` and park for a human for 24 hours; Cline schedules default to yolo, Cursor cloud agents never ask, DeepSeek's SDK pins full access. Every approval decision is a session event under one writer, which matches DeepSeek's best-in-class design; adopting its rule that a failed append rejects the request costs little.

7. **Where we are ahead: prompt-injection containment on the web path (F25, F27).** Provenance-gated `web_fetch`, an SSRF-safe resolver re-checked per redirect, untrusted webhook turns losing memory and web tools, and no model HTML: no other surveyed harness documents URL provenance. Two weaknesses on `main` (provenance set too wide, AUDIT L11; IPv6 transition-address gaps, L12) are fixed only on the branch. Note the asymmetry: these protections cover the chat runtime; agentic sessions reach the network only through unconfined `bash`, which bypasses all of them.

8. **Environment allow-list (F21).** Ours starts children from an empty environment plus an allow-list; Codex passes `*TOKEN*` variables by default and Claude Code's sandbox inherits secrets unless an opt-in scrub is set. This is a real lead, but it only covers the environment: files on disk (provider key in `config.toml`, data dir) are still readable by `bash` on `main` (item 1).

9. **No model-based approver (F5).** Twelve harnesses ship one; ours is docs-only. Do not rush it: every vendor caveats it (Cursor "not a security boundary", Claude Code "does not guarantee safety", goose fails open), and it is only safe behind a sandbox. If built, copy the converged safeguards: strip tool results from the reviewer's input, fail closed on reviewer error, circuit-break back to a human after a few consecutive denials, and drop broad allow rules while it is active.

10. **Secret redaction is a courtesy (F23).** Known-value scrubbing misses split or encoded keys on `main`; gate3's tail-holding stream scrubber helps. The field's stronger answer is item 2 (do not put the secret where the agent can print it).

11. **Things we lack that matter less here:** protected paths for `bash` (only file tools refuse `.git/**`; Codex and Claude Code protect `.git/hooks`, `.git/config` and agent config inside writable roots at the OS layer, which our gate3 sandbox could add cheaply), sub-agent report scanning (F26, cheap to copy), hardcoded dangerous-command floors (F6), container isolation for nodes (deliberately excluded by design), extension trust (no extensions exist), telemetry statement (we send none; say so in the README, since several vendors are judged on it).

## 7. Corrections, contradictions and unverified claims

What the reviews changed:
- Codex: "network off by default for workspace-write" not confirmed on either page and moved to Unverified; "WSL1 dropped at 0.115" removed; permission profiles do not require 0.138.0 (only a mixed-version rollout note); project config blocklist also includes `apps_mcp_product_sku`; hooks page gives no GA label.
- Claude Code (review-1): plan mode also runs classifier-approved commands when auto mode is available; SDK `failIfUnavailable` default true moved to Unverified (CLI docs imply off). Review-1 also flags that two entries disagree on when project `defaultMode: bypassPermissions` started being ignored (v2.1.257 vs Week 36), and that the auto-mode classifier engine entry at line 1062 is stale relative to line 705.
- Claude Code (review-2): mods fail open by default; the built-in guard loads only with managed settings, `managed-mcp.json` or a Team/Enterprise login, so API-key users get no guard and a user mod can approve past deny rules; even with the guard, a mod can read `.env` via `$.fs.read`.
- Claude Code (review-3): sandbox credential masking (sentinel, needs experimental `tlsTerminate`, sandboxed Bash only, macOS blocks masked files) and cloud network secrets (no sentinel, key never in the VM) are different mechanisms, not "the same pattern"; work orders register exactly one runner in orchestrator mode, not one per session; Remote Control has no documented lease; the claude.ai login restriction has a carve-out allowing an unmodified `claude` binary signed in by the end user.
- Claude Desktop: Cowork has both local-VM and cloud modes; the getting-started article's cloud-only statement is stale. Chat egress has a fourth option and non-Team defaults.
- Cursor: hook precedence is deny > ask > allow for permission decisions but last-response-wins for other fields (not "most restrictive wins"); Windows sandboxing not documented; a `bubblewrap` status value unexplained; Legacy Privacy Mode statement is on the cloud security page.
- Antigravity: `toolPermission` not mentioned on `/cli/modes`; "Preview" label for business sandbox not found; bubblewrap confirmed only on `/enterprise`; allow-rule-to-mount wiring documented for the App only; `trustedWorkspaces` semantics partly documented.
- goose: the original "no sandbox documented" was not supported; a macOS Desktop sandbox (`GOOSE_SANDBOX`) appears in search snippets but its page 404s, so it is unverified.
- Hermes: container approval bypass is on the security page, not the tools page; iron-proxy binds the Docker bridge gateway (loopback on Docker Desktop), never 0.0.0.0.
- OpenHands: hooks are not part of the documented security model (only a proposed fix); the FAQ's no-auth statement is general, not "open-source build".
- OpenClaw: cloud workers are beta, not GA, and "outbound-only" is not stated; env binding of approvals is conditional; per-node pairing token removal dated (store Jan 2026, RPC 2026.7).
- OpenCode: a policy `allow` can lift an earlier policy deny; "Console policies persist when unreachable" dropped.
- Warp: the denylist bypass applies only to the explicit "Run until completion" shortcut, not to "Always allow".
- Cline: hub roles are per session participant; Kanban CDK root and token-in-user-data are inferences.
- DeepSeek: `$DSH_HOME/.credentials.yaml` is on the providers guide, not the credentials page; SSH subsystem page exists on GitHub.
- OpenCodex: remember-device is a 30-day record expiry and off by default; pairing code single-use with five-minute lifetime; rate limit is ten failed codes per ten minutes.

Where a vendor's own pages disagree:
- Codex: the retired `untrusted` approval policy "can stop clients starting", yet managed `allowed_approval_policies` "must include `untrusted`".
- OpenHands: default confirmation policy ConfirmRisky (architecture page) vs "not stated" (guide); LLM analyzer inline-only vs also a separate LLM.
- Cursor: cloud repository copies deleted after the run (privacy page) vs VM snapshots with code kept 30 days and not deletable (cloud security page).
- Cline: "every action requires your explicit approval" (overview) vs CLI auto-approve default true, schedules default yolo, SDK default-allow; YOLO enterprise control "all interfaces" vs "no surface"; `--auto-approve` vs `--yolo` naming.
- Antigravity: browser called "sandboxed" in the slash-command catalogue while it drives the user's real Chrome profile.
- Claude Code: auto mode "available to all users on every provider" vs model- and provider-gated.
- DeepSeek: session telemetry default `FEEDBACK_ONLY` (CLI README) vs no default stated (subsystem page); maturity labels for Auto review inconsistent across pages.
- Hermes: plugin page says 27 lifecycle hooks but lists 38.
- OpenClaw: `telemetry off` and `DO_NOT_TRACK` leave the daily check on (documented but easy to misread); prompt-injection page cites an unsourced attack-success test.
- Zed: sandboxing has no maturity label and appears in release notes only as a bug fix.

What nobody could confirm:
- Claude Code: how the server-side tool-result probe works; the exact variable list `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB` removes; the full built-in classifier rules and read-only Bash list (printable only by running the CLI); cloud VM isolation technology; mods' isolation level ("worker thread").
- Codex: relay architecture and encryption for remote control; per-task isolation model of Codex Cloud.
- Antigravity: Auto evaluator mechanism; `sandboxProxy` details; Remote Control relay and encryption.
- DeepSeek: Auto review reviewer model, rules and prompt; default sandbox mode.
- goose: Smart Approval criteria; malware check data source; macOS sandbox existence.
- Cursor: worker wire protocol and encryption ("not documented" per the docs).
- OpenCode: telemetry of the local app; any OS sandbox.
- Pi: what install telemetry sends; whether `auth.json` and sessions are encrypted at rest.
- Oh My Pi: `eval` kernel isolation and tool-callback bridge security.
- Cline: hub authentication; meaning of `CLINE_SANDBOX`.

## 8. Questions for Round 2 (source code only can settle these)

Codex (CLI is open source):
1. Execpolicy: what exactly the tree-sitter splitter treats as "plain linear", and how an opaque invocation is matched (exact string, argv?). How `match`/`not_match` tests are executed at load.
2. Linux sandbox: the bubblewrap argument list and seccomp filter per mode; whether network is actually off by default under `workspace-write`; how the Unix-socket isolation that disqualified Landlock is done.
3. Auto-review: where the "fail closed" paths are and what transcript the reviewer receives; how the circuit-breaker counters persist across turns.
4. `shell_environment_policy`: the real default exclude list and whether `ignore_default_excludes = true` is the shipped default in code.
5. Hook trust by hash: what is hashed (file bytes, resolved command, plugin manifest) and where approvals are stored.

OpenCode:
6. The shell scanner: which parser per shell, how `cd` targets are inferred, and how "always" derives the saved patterns from a command.
7. Whether any OS sandbox or process isolation exists in V2 code despite no docs.
8. What policy applies when the Console is unreachable (cached, fail closed, fail open).

DeepSeek harness:
9. Default sandbox mode and preset at startup; how `partial` enforcement is decided per Landlock ABI.
10. Auto review integration: prompt, model and failure behaviour.
11. Whether the approval UI shows tool arguments anywhere, given requests omit them.

Oh My Pi:
12. `bash.patterns` segment splitting and the exact critical-pattern list; whether yolo really skips them in code.
13. `auth-gateway`: does any provider key ever reach the client process.

Pi:
14. What `enableInstallTelemetry` sends; whether a failing extension `tool_call` handler blocks in every mode.

OpenHands:
15. Whether `LLMSecurityAnalyzer` makes a separate LLM call; the real default confirmation policy when none is set.
16. Per-conversation Docker hardening flags as actually passed, and whether client-supplied limits are capped anywhere.

OpenClaw:
17. Exec approval binding: how the executable content hash and resolved-path re-check are implemented, and the TOCTOU window between check and launch.
18. Auto-review prompt and the three-denial escalation counter; sandbox mode defaults per backend.
19. Node pairing: what the device token is, how SSH-verified auto-approval compares keys.

goose:
20. Does `GOOSE_SANDBOX` exist in current source; Seatbelt profile and egress proxy if so.
21. Smart Approval risk criteria; adversary-mode prompt; malware-check data source.

Hermes Agent:
22. Hardline matcher implementation and what "fails closed on unparseable quoting" means in code; how deny globs "see through" `sh -c`.
23. iron-proxy integration: how tokens map to keys, and what happens to Hermes's own LLM calls.
24. `smart` approver prompt and the 300 s timeout path.

Zed (open source):
25. Terminal chain splitting: which shells are parsed; bubblewrap profile; when Linux falls back to unsandboxed.
26. Where `sandbox_permissions` grants scoped "always" are stored and whether a repo can ship them.

Warp (client open source):
27. The shipped default denylist, and whether any local-agent sandbox code exists; redaction regex list and where it runs.

Cline:
28. Whether `requires_approval` is enforced anywhere beyond the model's flag (e.g. a static override); what `CLINE_SANDBOX` does; whether the hub has any authentication in code.

OpenCodex:
29. Remote Workspace bubblewrap profile and the "command-sandbox probe" that gates write access.

ACP (open protocol, Rust SDK):
30. Whether the SDKs implement any default client-side policy for `fs/*` and `terminal/*`, or leave it entirely to the client.
