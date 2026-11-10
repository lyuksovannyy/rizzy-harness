## 9. Review (reviewer 3: Areas 16-23 and sections 3-7, lines 3507-5340)

Date: 2026-11-10. Reviewer worked from public docs at https://code.claude.com/docs/en/ (fetched live). `claude-code.md` was NOT edited. Apply the lists below by hand.

Checked: 62 items (about 52 feature entries plus every item in "Distinctive" (13) and "Relevance" (10) that I could trace; several share an entry). Entries in range: roughly 300 of the 805 in the file.

Counts: confirmed 46, confirmed-with-correction 9, not-supported 0, wrong 1 (one sentence in a Distinctive bullet), status-wrong 0, unverified (moved) 3.

### Verdict table

| Entry (heading) | Line | Verdict | Reason |
|---|---|---|---|
| Claude in Chrome integration (mechanism) | 3515 | confirmed | v1.0.36, native-messaging manifest name, bridge.claudeusercontent.com, 10 MB, v2.1.293 credential-path refusal, WSL unsupported all on chrome.md |
| Chrome enablement and context cost | 3522 | confirmed | install prompt once per session; Reconnect extension v2.1.290 |
| CLI computer use (macOS) | 3527 | confirmed | research preview, Pro/Max/Ultra only, no -p, 3456x2234 to ~1372x887, one-session lock, Esc consumed |
| Claude in Chrome and computer use | 3544 | confirmed (partly) | 2.1.319 (Nov 6, 2026) removed shortcuts_list/shortcuts_execute confirmed. "GA in 2.1.198 (Jul 1, 2026)" and 2.1.282 NOT verifiable (changelog fetch truncated). Moved to Unverified |
| Non-interactive / print mode flags | 3551 | confirmed (partly) | headless.md confirms the -p, json/stream-json, --forward-subagent-text, --bare points; the rest of the flag list not re-checked |
| Cloud, remote and self-hosted runner CLI | 3557 | confirmed (partly) | `claude gateway --config` row confirmed on cli-reference.md; other flags not re-checked |
| Cloud session and Remote Control security | 3575 | confirmed | security.md "Cloud execution security" matches bullet for bullet |
| Long-lived OAuth token (setup-token) | 3581 | confirmed | "one-year", model requests only, bare mode ignores it (authentication.md) |
| Ultrareview | 3616 | confirmed | 3 free runs, $5-25, 5-15 min, 500 files/8,000 lines, v2.1.227 --post, exit codes 0/1/130 |
| Cloud scheduled tasks | 3604 | confirmed (partly) | research preview, UTC cron, CRON_TZ, /fire token shown once, beta header `experimental-cc-routine-2026-04-01`, fire-payload wrapping all confirmed. "skipped up to 72 hours then turns off" not found in the fetched text |
| Projects | 3610 | confirmed | public beta Pro/Max/Ultra, 500 threads/day, 16,000 chars, 1,000 files/2 GB/480 MB/10 folders, sandbox pause then fresh clone |
| Headless basics and bare mode | 3622 | confirmed | 10MB stdin; -p runs project hooks/.mcp.json untrusted; --bare "will become the default for -p in a future release" |
| Headless output formats | 3628 | confirmed | api_retry fields, mcp_server_errors, MCP_TIMEOUT 30 s (v2.1.221), 30 s drain |
| Headless lifecycle | 3634 | confirmed | 10 min idle cap, CLAUDE_CODE_PRINT_BG_WAIT_CEILING_MS, SIGTERM exit 143, --resume by .jsonl path |
| Remote Control relay | 3640 | confirmed | execution stays local |
| Remote Control transport | 3646 | confirmed | outbound HTTPS only, registers and polls, transcript stored server-side while connected |
| RC invocation / server mode | 3652 | confirmed | --capacity default 32 |
| RC resume, reconnect | 3658 | confirmed | ~4 hours, ~10 min server mode, ~30 min heartbeat |
| Cloud environments / setup scripts / VM limits | 3700-3716 | confirmed | Ubuntu 24.04 x86_64, 4 vCPU/16 GB/30 GB "may change", setup ~5 min, snapshot ~7 days |
| Self-hosted architecture | 3718 | confirmed | Anthropic never connects in; control plane stays hosted; public beta Team/Enterprise; no ZDR/HIPAA |
| Self-hosted lifecycle (claim, lease) | 3724 | confirmed | lease lapses ~60 s; 10 s poll timeout; backoff doubles to 20 s |
| Self-hosted network paths | 3730 | confirmed (partly) | outbound-only confirmed; SSE no-buffering confirmed; host list not re-checked |
| Deployment recipes and sizing | 3736 | confirmed | 3 replicas, capacity 4, 4 GiB req=limit, 2/4 CPU; docs say recipes exceed own capacity-1 advice |
| Self-hosted environments (runner) | 3812 | confirmed | "public beta" |
| Remote Control (changelog) | 3822 | confirmed | Week 34 (Aug 17-21, 2026): RC "out of research preview", device card |
| HIPAA configuration | 3862 | confirmed | v2.1.285, hourly policy fetch, WebFetch off, --cloud/teleport/RC refused, credentials stripped, `HIPAA configured` tag |
| Restricting login org / providers | 3868 | confirmed | setup-token and /install-github-app enforce only forceLoginMethod |
| GitHub App permission set | 3960 | confirmed | the 14-row table matches exactly |
| Claude Tag session model | 3771 | confirmed | ~1 h channel session, ~100 messages, !restart, 7-day follow (how-it-works.md) |
| Managed settings delivery / ranking | 4013, 4025, 4091 | confirmed | server polled hourly, MDM every 30 min, paths per OS, first-wins default, merge from v2.1.242 |
| Server-managed settings / client-side control | 4031, 4948 | confirmed | hourly, 5 s wait, `~/.claude/remote-settings.json`, "client-side control, not a security boundary"; apiKeyHelper/WIF do not trigger fetch |
| Precedence order | 4553 | confirmed | managed > --settings > local > project > user; lists merge |
| Which settings reach a cloud session | 3777 | confirmed | settings.md "Settings in cloud sessions" |
| /usage session cost | 4293 | confirmed | computed locally at list price; estimate; 1.1x residency; resets on /clear (v2.1.211) |
| Org cost controls | 4311 | confirmed | $13/dev/day, $150-250/month, 90% under $30 are in costs.md (still vendor claims) |
| Data training and retention | 4703 | confirmed | 5 y / 30 d consumer; 30 d commercial; /feedback 5 y |
| Telemetry defaults | 4714 | confirmed | error reports only Pro/Max/Ultra v2.1.198+ direct; Bedrock/Vertex/Foundry default off |
| WebFetch preflight | 4720 | confirmed | hostname only, 5 min cache |
| Credential storage | 4692, 4764 | confirmed | Keychain; 0600 file on Linux; Keychain-reject fallback |
| Network secrets (egress proxy) | 4857 | confirmed | key never in VM; never attached to GitHub, api.anthropic.com, registries, setup script, telemetry; not in self-hosted or CMEK |
| One-owner-per-runner lock | 4863 | confirmed | first claimed session locks owner; drain-grace 0 exits |
| Self-hosted hardening checklist | 4869 | confirmed | "any member ... can dispatch a session to any of its environments"; no per-environment ACL |
| Environment secret, work orders | 4881 | confirmed-with-correction | work order is single-use and registers ONE RUNNER (orchestrator mode only); expires with spawn lease, default 120 s (`--expected-spawn-seconds`). Add this |
| Git config hardening in hooks | 4893 | confirmed | applies only to hook git, not the session's git; v2.1.280 |
| Per-session config assembly | 4541 | confirmed | control plane supplies hooks (python3 needed), tool list, permission rules; runner image managed-settings also applies |
| Terms for embedding Claude Code | 5063 | confirmed-with-correction | omits the carve-out sentence (see Corrections 1) |
| Authentication and claude.ai login restriction | 5036 | confirmed-with-correction | wrong third evidence URL; exact sentence below (Corrections 2) |
| Distinctive: credential masking | 5210 | confirmed-with-correction | see Corrections 3 |
| Distinctive: pull-only execution with leases | 5214 | wrong (one sentence) | "Each session gets a single-use work order" is false: work order = one runner, orchestrator mode only (Corrections 4) |
| Relevance bullet 1 (runner is closest design) | 5274 | confirmed-with-correction | "single-use work orders" same issue; control-plane-pushes-policy claim CONFIRMED (Corrections 5) |
| Relevance bullet "Legal constraint" | 5283 | confirmed-with-correction | omits end-user-own-subscription carve-out (Corrections 1) |
| Architecture: self-hosted runner bullet | 5255 | confirmed-with-correction | stdin/fd 3 is documented on the configuration page, not the reference page (Corrections 6) |
| Architecture: Remote Control bullet | 5251 | confirmed | up to 32 sessions |
| Trusted Devices for RC | 4851 | confirmed (partly) | 18-hour sign-in confirmed; "beta" label not re-checked |
| GitHub Action trigger checks | 4905 | confirmed | write-access check, allowed_bots, schedule attributed to last cron editor |

### Corrections to apply (entry heading, line, what is wrong, corrected text, URL)

1. **"Terms for embedding Claude Code and credential-use restrictions"** (line 5063) and **Relevance bullet "Legal constraint"** (line 5283). The summary is right but one-sided. The page's actual sentence: "Anthropic does not permit third-party developers to offer Claude.ai login into their own applications, or to route requests through Free, Pro, Max, or Ultra plan credentials on behalf of their users." Immediately after, the same page says the restriction does not stop "an end user from signing in to the unmodified Claude Code binary with their own Claude subscription, including where a platform hosts Claude Code". Add: embedding is allowed under Commercial Terms if the binary is unmodified, no auth method is removed or restricted, and each end user authenticates with their own API key, subscription or 3P credential. Also the page says OAuth is for "Free, Pro, Max, Ultra, Team, and Enterprise" purchasers. Rizzy-harness cannot itself offer claude.ai login, but it can drive an unmodified `claude` binary that the user signs in. URL: https://code.claude.com/docs/en/legal-and-compliance.md (Can customers offer Claude Code in their products?; Authentication and credential use).
2. **"Authentication and claude.ai login restriction"** (line 5036). Evidence line cites `agent-sdk/session-storage.md (Dual-write architecture)`, which has nothing to do with this claim; drop it. The supporting sentence is on https://code.claude.com/docs/en/agent-sdk/overview.md (Get started note): "Unless previously approved, Anthropic does not allow third party developers to offer claude.ai login or rate limits for their products, including agents built on the Claude Agent SDK." Note "rate limits" and "unless previously approved" are in this version; the longer sentence is on legal-and-compliance.md.
3. **Distinctive "Credential masking at the sandbox proxy"** (line 5210-5212). Add what the page states: masking needs the experimental `sandbox.network.tlsTerminate` (without it the sentinel reaches the server unchanged and auth fails); `sandbox.credentials` is honoured only from user, managed or `--settings`, never repo settings; it covers sandboxed Bash only (`!` commands and other subprocesses need `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB`); substitution only to hosts in `network.allowedDomains`; on macOS, masked FILES are unreadable (same as `deny`), sentinel copies only on Linux/WSL2; SigV4 re-signing needs `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY` masked together; a pattern that matches nothing leaves the real file readable (default `onExtractNoMatch: warn`). Also fix "Cloud sessions and Claude Tag's Agent Proxy apply the same pattern": cloud network secrets use no sentinel; the key is simply never in the VM and the agent proxy attaches it on matching hosts. Similar goal, different mechanism. URLs: https://code.claude.com/docs/en/sandboxing.md (Mask credentials); https://code.claude.com/docs/en/cloud-environments.md (Add network secrets).
4. **Distinctive "Pull-only execution with leases"** (line 5216): replace "Each session gets a single-use work order and a child process" with "In orchestrator (on-demand) mode each spawned runner gets a single-use work order that registers exactly one runner and expires with the spawn lease (default 120 s); each session runs as a child process of its runner". Also Remote Control is outbound-polling but no lease is documented for it; do not group it under "leases". The poll refreshing the lease is documented only for self-hosted runners. "Outbound HTTPS only" is also slightly short: runner/session also use SSE and, with the Anthropic git proxy, a WebSocket to api.anthropic.com (still outbound only). URL: https://code.claude.com/docs/en/self-hosted-environments.md (Session lifecycle; Network paths); https://code.claude.com/docs/en/self-hosted-environments-configuration.md (On-demand runners).
5. **Relevance bullet 1** (line 5274). The claim "Anthropic's control plane pushes policy to the runner with each work payload" is CONFIRMED but incomplete: configuration.md says "Anthropic's control plane sends each session's tool list and permission rules with the work payload", the control plane also sends each session's model ("The runner removes `ANTHROPIC_MODEL` and `ANTHROPIC_DEFAULT_MODEL` from the environment it passes to sessions"), delivers claude.ai connectors as MCP config, and supplies lifecycle hooks (need `python3` in the image, disabled by `disableAllHooks`, not counted as managed hooks). Host-side: sessions also read `managed-settings.json` from the runner image, and the runner seeds `~/.claude` from a startup snapshot. On Bedrock/Agent Platform runners, server-managed settings and org policies do NOT reach sessions, so the image's managed file is the only enforcement. Same "single-use work orders" fix as 4. URL: https://code.claude.com/docs/en/self-hosted-environments-configuration.md.
6. **Architecture: self-hosted runner bullet** (line 5258-5259): stdin carries "token rotations and session-end signals" and fd 3 carries "the child's activity signals to drive idle and startup timeouts" (the page says "Keep stdin and file descriptor 3 attached"); cite https://code.claude.com/docs/en/self-hosted-environments-configuration.md, not the reference page. "Per-session policy (tools, rules, MCP, hooks, model) arrives with the work payload": the page says tool list and permission rules arrive "with the work payload"; model, MCP connectors, hooks and API endpoint are also control-plane delivered per session, but the page does not call all of them "the work payload". Reword to "from the control plane".
7. **"Cloud scheduled tasks (formerly routines)"** (line 3604-3608): the entry reports "runs are skipped for up to 72 hours, after which the task turns off"; I could not find this in the fetched page. Soften or move that clause to Unverified. Everything else checked is correct. Also naming: the entry says "formerly routines" while line 4846 ("Routine prompt trust model") still uses "routine"; same feature, same status (research preview).
8. **"Claude in Chrome and computer use"** (line 3544-3547): the digest/changelog lines for "GA in 2.1.198 (Jul 1, 2026)" and 2.1.282 could not be confirmed; only 2.1.319 was visible. See Move to Unverified.
9. **Self-hosted work order entry** (line 4882): add default spawn lease of 120 s (`--expected-spawn-seconds`; all orchestrator replicas must use the same value) and that the work order "expires with" it. URL: https://code.claude.com/docs/en/self-hosted-environments-configuration.md (On-demand runners).

### Move to Unverified

- "Claude in Chrome became GA in 2.1.198 (Jul 1, 2026)" and `allowClaudeInChromeWithManagedMcp` (2.1.282), line 3545. Looked for: changelog.md entries 2.1.198 and 2.1.282; the docs changelog page was too large to read past 2.1.311. Settle by reading https://github.com/anthropics/claude-code/blob/main/CHANGELOG.md or whats-new/2026-w27.md.
- "self-hosted-runner public beta from 2.1.224 (Aug 7, 2026)", line 3813. Looked for 2.1.224 in the changelog (not reachable) and did not fetch whats-new/2026-w32.md. The public-beta status itself is confirmed.
- Routine "skipped up to 72 hours then turns off", line 3608. Not in the fetched routines.md text.

### Duplicate entries that disagree (in my range)

- Line 5216 ("Each session gets a single-use work order") contradicts line 4882 (work order registers exactly one runner in orchestrator mode). Line 4882 is correct.
- Line 5212 says cloud sessions "apply the same pattern" as sentinel substitution; line 4858 (network secrets) correctly describes attach-at-proxy with no sentinel. Line 4858 is correct.
- "Routines" vs "scheduled tasks": lines 3604/3606 and 4846-4849 name the same feature differently. Not a factual conflict.
- Self-hosted runner appears at 3718, 3812 (beta vs "public beta"), consistent. Remote Control at 3640-3680 and 3822 consistent (GA, out of research preview in Week 34). Managed-settings delivery at 4013, 4025, 4091 and login enforcement at 3868, 4067, 4097 are consistent duplicates; they could be merged.

### Borrowed-feature findings

None found. The one imprecision is the "same pattern" equivalence between sandbox credential masking (sentinel substitution, needs TLS termination) and cloud network secrets (key never in VM, attached by proxy); see Correction 3. The Relevance legal bullet also treats the restriction as stricter than the page, which is a mis-statement of scope rather than a borrowed feature.

### Notable omissions (documented, not in my range of the survey)

1. Sandbox environments comparison page: https://code.claude.com/docs/en/sandbox-environments.md
2. Claude Security: https://code.claude.com/docs/en/claude-security.md
3. Code Review (managed PR review; only costs/analytics covered): https://code.claude.com/docs/en/code-review.md
4. Cross-session messaging: https://code.claude.com/docs/en/cross-session-messaging.md
5. Managed Agents with self-hosted sandboxes (API-side counterpart to self-hosted runners): https://platform.claude.com/docs/en/managed-agents/self-hosted-sandboxes
6. Self-hosted session identity JWT page is covered; the quickstart/testing pages are covered; but the SCM connector flags in the reference (https://code.claude.com/docs/en/self-hosted-environments-reference.md) are not.
7. Claude Platform on AWS: https://code.claude.com/docs/en/claude-platform-on-aws.md
8. Security guidance plugin: https://code.claude.com/docs/en/security-guidance.md
9. Goal / `/goal` and background-work check-ins: https://code.claude.com/docs/en/goal.md
10. Desktop iOS simulator pane: https://code.claude.com/docs/en/desktop-ios-simulator.md

### Overall judgement

Reliable for Areas 16-23. Every one of about 50 entries that I opened against the cited primary page matched in substance, including numbers (60 s lease, 10 s poll, 20 s backoff, 4 vCPU/16 GB/30 GB, ~4 h / 10 min / 30 min RC limits, 18 h trusted device, 32 capacity, 3 free ultrareviews, 500 threads/day, 1-year setup-token), defaults and status labels. Errors are in the synthesis sections: Distinctive/Relevance over-generalise "single-use work order" to every session, group Remote Control with leases, and state the legal restriction without its carve-out and without the exact sentence. The credential-masking claim omits its experimental TLS-termination dependency and its macOS limitation. Trust the unchecked remainder of the entries (cited per page, written from page text) at roughly the same level, but treat any Distinctive or Relevance sentence that generalises across entries as needing a check. All version-and-date claims that depend on the changelog (2.1.x dates) are the weakest and should be treated as unverified until the GitHub changelog is read. Vendor claims are correctly labelled where I checked ($13/day, ultrareview "reproduced", computer-use "flags prompt injection").
