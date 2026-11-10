# Round 1 survey brief (shared by every per-harness agent)

Research date: 2026-11-10. You survey ONE harness (named in your task) for a
competitive study of AI coding harnesses. The reader is building their own
self-hosted harness (Rust, a web dashboard, a central "master" server plus
worker "nodes" that run the agent loop and tools) and will decide what to
copy, skip or do differently. **Accuracy beats volume: a wrong claim is worse
than a missing one.**

## Sources

- Start from your harness's section in `research/round1/00-targets.md` (same
  folder as this file): it gives the identity, surfaces and primary URLs.
- Round 1 uses the PUBLIC SURFACE only: official site and docs, README files,
  changelogs, release notes, official blog and announcement posts, public
  repository descriptions. Do NOT read source code in this round (README,
  docs folders, CHANGELOG and release notes inside a repository are fine).
- Prefer primary sources. A third-party article may be used only to find a
  primary source, or, if nothing primary exists, cited and labelled
  `third-party`.
- Go beyond the listed URLs: follow the docs navigation and the changelog
  until you have covered the whole documented surface, not just the
  headline features.

## What to collect

Build as complete an inventory as you reasonably can of what the harness
offers TODAY. Use these areas as a starting taxonomy; add areas when
something fits none, and write "nothing documented" for an area you checked
and found empty (that is a finding):

1. Agent loop and execution model
2. Built-in tools (list each tool)
3. Code editing and file operations
4. Terminal and shell integration
5. Permissions, approvals and sandboxing
6. Context management and compaction
7. Memory and persistent instructions (instruction files, memories)
8. Planning, tasks and modes
9. Sub-agents and multi-agent orchestration
10. Background, long-running and scheduled work
11. Sessions: persistence, resume, fork, checkpoints, undo
12. Git and version control (worktrees, commits, pull requests, review)
13. Extensibility: MCP, plugins, skills, hooks, custom commands, SDKs, protocols
14. Models, providers and inference configuration
15. UI surfaces (TUI, IDE, desktop, web, mobile, voice)
16. Browser automation and computer use
17. Remote, cloud, headless, CI and distributed execution
18. Collaboration, teams and enterprise administration
19. Observability: usage and cost, logs, tracing, debugging
20. Configuration and customisation (files, formats, precedence)
21. Security and privacy (data handling, telemetry, authentication)
22. Licence, pricing and distribution (brief)
23. Anything else or unusual

For each feature write ONE entry:

```
### <short feature name>
- What: one to three sentences, paraphrased, saying what it does and, where
  the source says so, how it works (mechanism, limits, defaults).
- Status: GA | beta | experimental | preview | deprecated | planned | unclear
- Evidence: <full URL you opened> (page or section title) [; more URLs]
- Notes: version or date the source refers to; `vendor-claim` if it is a
  marketing statement with no technical description; `unverified` if you
  could not open a primary source; anything where sources disagree.
```

Rules for entries:
- No URL you actually opened = do not write the claim. If you believe
  something exists but could not confirm it, put it in the "Unverified"
  section instead.
- Do not copy feature descriptions from memory or from another harness:
  similarly named features often work differently, so record the mechanism
  this vendor documents.
- Distinguish implemented, experimental, deprecated and planned. A roadmap
  item is `planned`, never GA.
- Paraphrase; never quote more than a short phrase.
- Record the product version or the page's date where visible.

## File to write

Write `research/round1/<slug>.md` (the slug is given in your task; the path
is relative to the repository root `/run/media/lebron/PROJ/Rust/rizzy-harness`).
Write no other file and modify nothing else. Structure:

1. **Identity**: name, vendor, surfaces, open or closed, licence, version and
   date surveyed (take it from `00-targets.md`, correct it if you find it
   wrong and say so).
2. **Features by area**: the 23 areas above, entries as specified.
3. **Distinctive or unusual**: features you have not seen described for
   other harnesses, or implemented in an unusual way, with one line each on
   why it is interesting.
4. **Architecture and UX notes**: only what public sources state (process
   model, client/server split, protocols, where the loop runs).
5. **Relevance to a self-hosted master/node harness**: five to ten bullets,
   each tied to an entry above.
6. **Overhyped, stale or contradictory**: claims that look like marketing,
   docs that lag releases, pages that contradict each other.
7. **Unverified**: things you looked for and could not confirm, and what
   would settle each.
8. **Sources**: every URL you opened, with its page title and what you used
   it for.

## Report back

Under 200 words: how many feature entries you wrote, the three most
distinctive things, the biggest gaps in your coverage, and anything in
`00-targets.md` that turned out to be wrong.
