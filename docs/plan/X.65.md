# X.65 **sec** (T46)

## Adds

A `/` menu in the composer of every chat (the owner's choice, 2026-11-24; Claude Code, Codex and OpenCode all open a command menu at a leading `/`, and here a person could not see which skills a chat had without knowing their names).
Typing `/` as the first character of a message opens a listbox above the box with the built-in commands (today `/compact`, its description and its `[focus]` hint) and the skills the chat can ask for, each with a description; typing narrows it (names that start with the text first, then names that hold it); Nothing is highlighted when it opens: Tab completes the first entry (or the highlighted one) to `/name ` and leaves the caret after the space, Up and Down highlight an entry, and Enter completes only an entry the person highlighted that way; with nothing highlighted Enter is the Enter it always was (it sends the text, or runs `/compact`), so a typed `/act` is sent as `/act` and never becomes `/compact`; Escape closes it and keeps the text, a click completes; a screen reader gets a listbox and a polite status line naming the active option and its place.
Choosing only edits the text: nothing is sent, run or read.
Names that start with the typed text are listed before names that hold it; Shift+Enter, Ctrl, Alt and Meta chords, an input-method composition, a path (`/usr/bin`), a `/` after other text and a caret past the first word do not open or use it.
The same menu in every chat kind; the new-chat box has none (the one built-in command needs a chat; there is no session to ask, and no unauthenticated route was added for it).
**Where the list comes from.** The node is the authority.
`session.skills_menu` (`req_id`, `session_id`, `agent`) carries a slim copy of the chat's own agent (its tools, deny lists and stored skills; no prompt, sub-agents or connector tools), resolved by the master as a prompt resolves it (the deny lists, the stored skills it may load), and is answered by one `session.skills_menu_result` (the skills, or `busy`), only to a node that announced `skills-menu` (any node that offers the `skill` tool); additive, no event kind, no schema change, no turn, no event appended, never acked.
The node builds the list with the very pieces a turn uses (`session_skills.rs`: the same `skill_on` check, the same roots, `discover`, `withhold`, `merge_stored`), so it neither offers what the turn would not nor hides what it would: `repository = false`, `denied_tools`, an agent without the tool, a skill the agent may not use and the 150-skill and 16 KiB-of-front-matter bounds all hold; a plain chat has no folder and lists the stored skills only, as its turns do.
`user-invocable: false` (read like the other front matter keys, YAML or line by line; a value that is no boolean is as if the key were absent, so it cannot change how the file is read for the model) hides a skill from the menu only: the list the model gets is unchanged, and a typed `/name` still asks for it, exactly as today (`Catalogue::not_invocable`; invocation is not changed by this row).
A skill whose name is a built-in's (`compact`) is not listed: `/name` never invokes it (T46), so listing it would offer a completion that does nothing; neither is one whose name `/name` would read as another (a trailing `.`, `:` or `,` is trimmed from the typed word), by the one rule (`typed_name`) both use.
A `disable-model-invocation` skill is listed: a person may ask for it.
Untrusted text: names are held to the skill-name rule by the node, again by the master and again by the page; descriptions are one clean line of at most 200 characters (a final ellipsis when cut) at the node and again at the master, and the page draws them as text through the hidden-character treatment, never as HTML or a link.
`GET /api/sessions/{id}/skills-menu` (`crates/master/src/http/skills_menu.rs`, waiting in `crates/master/src/skills_menu.rs`, 12 s, one place per account and 32 in all, held until the node answers even when the page aborted): owner only through `visible_session`; anyone else, admins and workspace admins included, gets the 404 of a chat that does not exist and the node is not asked.
The request passes the send-time filters a prompt passes: a node without the Plugins deny-list capability gets the `skill` tool withheld and one without `stored-skills` gets no stored skill names.
The node's two places are held by the work, not the wait: a read that outlives its 5 s bound keeps its place until it ends.
The answer is `{"skills": []}` for a node that is away, older or slow and for an agent that offers none: the built-in commands are always there, so none of these is an error the page must word; only "too many asks" is a 503, and the page keeps the menu it has.
The page asks when the menu first opens in a chat and keeps the list for 30 seconds (a list fetched while a turn runs is not kept); it never polls.
The menu is a lazy chunk (1.4 KB gzipped); the composer's first-load growth is 0.15 KB.
Not covered: command files of other harnesses (`.claude/commands`, `.codex/prompts`; the owner has not decided to read them), nested skill folders, argument hints for skills, and a refresh of the kept list the moment a turn ends (the 30 seconds bound it).

## Safeguards and abuse tests

See [T46](../threats/T46.md).

## Podman / Node

No / Yes (the node reads; the master only relays)

## Depends

X.2, X.20

## Size

**M**
