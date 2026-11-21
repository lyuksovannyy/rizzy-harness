// Bundle budget (PLAN.md Phase 3 and "Rich messages"): the new-tab home
// screen must stay small, and heavy libraries must stay in the lazy chunks
// that need them. Reads Vite's manifest, prints every emitted file with its
// gzip size, and fails if over budget or if a library lands where it must
// not (see the markers below).
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { gzipSync } from "node:zlib";
import { join } from "node:path";
import { createRequire } from "node:module";

const INITIAL_KB = 110;
// 300 before workspaces: the switcher, the Settings sections and the workspace
// panels brought the total to 304.9 KB, almost all of it in the lazy Settings
// chunk (the initial load stayed at 96.1 KB). 310, the ceiling the workspaces
// design allows, leaves room for its follow-up fixes without a raise each time.
// The Agents page (a lazy chunk of about 5 KB, with Settings → Agents removed)
// then took it to 309.9 KB on 2026-11-08.
// 321 for Projects (docs/projects.md), measured 320.2 KB the same day: 10.3 KB
// on top, 5.5 in its lazy page, 2.2 for the 20 icons the sidebar draws (in the
// first load, which stays under its own budget at 100.1) and the rest in the
// sidebar section, the shell and the chat header's picker.
//
// From the sidebar areas on (routines, media, artifacts, search), the budget
// is the last merged measurement plus each implementer's own growth, one line
// each, so parallel branches merge without touching the same line. Inside a
// phase a branch sets only its own line (its measured growth + at most 0.5,
// with the reason); at each merged phase BASE_KB is re-measured and the three
// lines go back to 0.
//
// The total is a soft limit (since 2026-11-08): it records what each feature
// added and why, and a feature that needs room raises its line with the
// reason. Most of it is lazy chunks a visit loads only when it opens their
// page, all of it is cached for a year once loaded, and the server sends it
// gzipped. The hard limits are the ones a visit waits for: the first-load
// ceiling, the lazy-chunk checks and the library markers below.

// Measured total of the last merged phase, with what it bought.
const BASE_KB = 361.0; // measured 360.9 on 2026-11-09 (initial 102.9) after the fix-plan cuts: the JWT session keeper, the server-setup pages and node-problem notices, and dead code are gone

// I2: its own growth over BASE_KB + ≤ 0.5, with the reason on the line.
const I2_KB = 0;

// I3: its own growth over BASE_KB + ≤ 0.5, with the reason on the line.
const I3_KB = 0;

// I4: its own growth over BASE_KB + ≤ 0.5, with the reason on the line.
const I4_KB = 0;

// Fix plan 3.10 (audit L14), measured 361.9 (initial 103.4): drafts kept per
// user and cleared at sign-out, with the chat and for incognito chats (first
// load, +0.5); the server-made temporary password's dialog and per-message
// footnote ids (lazy, +0.4).
const L14_KB = 0.9;

// Gate 3 P-1, measured 363.0 (initial 103.6): the login key kept and sent
// by the API client and the socket (first load, +0.2); the links that
// what the browser loads by itself (<img>, <video>, downloads) needs, in
// their own chunk that only lazy pages and the composer's image chip
// load (+0.9).
const P1_KB = 1.2;

// Gate 3 fix review 2, W-1 to W-3, measured 366.5 (initial 103.9): the
// approval card shows every field of a call in full, with sizes, head and
// tail, blank-run and hidden-character markers, look-alike and path notes
// (lazy, in the conversation chunk, +3.1); hidden characters in chat
// titles and node names shown as codes on the first screen (+0.3).
const R6_KB = 3.5;

// 2026-11, measured 368.2 (initial 99.4): page-only API calls, strings,
// model helpers and project icons moved out of the first load into the
// chunks that use them. The first load fell by 4.6; the total rose by 1.6,
// which is what five more chunks cost (+1.6). Gate 3 R2-1: the favicon is
// fetched with the login key through the links chunk (+0.1).
const SPLIT_KB = 1.7;

// T.8d step 1, measured 368.4 (initial 99.5): the fold knows the two
// background events and the conversation shows a background command's end
// as a plain line with its tail as text (+0.2).
// T.8d step 2, measured 370.5 (initial 99.5, unchanged): the chat's
// background strip (Stop, Stop all, time left, a node away), its own lazy
// chunk loaded on a chat's first running background command (+1.9, its
// stylesheet 0.4 beside it), and the conversation's lazy import of it and
// the fold's timeout (+0.1); the strip's hidden-character marks on a
// refusal took it from 370.4 to 370.5 (+0.1).
const T8D_KB = 2.3;

// Rizzy Glass stage 1, measured 370.6 (initial 99.7): the accent presets'
// one list, in the first load (+0.1).
const GLASS_KB = 0.1;

// N.10a part 1, measured 373.0 on Rizzy Glass (initial 100.1, from 99.7):
// the chat header's mode and gaps (in the lazy conversation chunk), the
// new chat's mode choice and the host shell's warning, each its own lazy
// chunk (only server admins on a node offering the host shell load them),
// and in the first load the composer's mode state and the rule for who may
// pick it (+2.4; 372.7 before the merge, the rest the review's fixes: the
// gaps' words, the choice bound to its node).
const N10A_KB = 2.4;

// N.10a part 2, measured 373.9 (initial 100.1, unchanged): the level
// select and its host warning's grant variant, in the lazy Settings chunk
// (Users, Workspace admin → Members) and the warning's own lazy chunk; the
// composer's rule now reads the level instead of the admin role (+0.9).
// The review's fixes, measured 374.1 (initial 100.2): the member list's
// levels as they count, the server admin's level select and the demotion's
// question, all in the lazy Settings chunk (+0.2).
const N10A2_KB = 1.1;

// The agent's task list (`todo`), measured 374.1 (initial 100.1, unchanged)
// against 373.0 on main: the pinned panel, its own lazy chunk loaded on a
// chat's first list (0.7, its stylesheet 0.5 beside it) and the lucide
// circle icon it shares (0.2); the conversation's lazy import and the tool
// card's line (+0.2); the fold's case in the first load (+0.1 before
// rounding). Its review's fixes, measured 374.3 (initial 100.2): a new
// chat's agent per mode for the home screen, projects and routines (TD-1,
// first load, +0.1) and the panel folding when no turn works on it (TD-2,
// +0.1).
const TODO_KB = 1.3;

// Sub-agent briefing (pull request 91), measured 375.9 with X.5 against
// 375.7 before it: the delegate card's status (+0.2 as the total rounds).
const SUBAGENT_KB = 0.2;

// X.5, writing while a turn runs, measured 375.7 (initial 100.5) against
// the recorded 375.5 on main: in the first load the composer sends while a
// turn runs (Send beside Stop, the line under the box), the fold reads
// `queued_delivered`, the "sent while working" and "not read yet" marks and
// their strings, and a turn's start after a turn's end for the progress
// cards and the face (+0.2, recorded +0.3: the total rounds up past 375.7).
const X5_KB = 0.3;

// Rename and export a chat (feat/chat-rename-export), measured 378.4
// (initial 100.6) against 375.9 on main (+2.5, not split per chunk): the
// header's title with its Rename field and the Pencil and Download icons in
// the conversation chunk; the export dialog and the Markdown builder in
// their own lazy chunk, loaded on the first click. After its review and the
// merge of main with pull requests 94 and 95 (the skill tool's card, which
// recorded no line of its own), measured 378.6 (initial 100.6): +0.2.
const EXPORT_KB = 2.7;

// Memory by default (T.3, owner 2026-11-14), measured 377.9 (initial 100.7)
// against 375.9: the chat's memory line reads what was saved and has Undo,
// the Memory page shows each fact's place, moves it and restores retired
// ones, and the join question (a lazy chunk; Shell only decides whether to
// load it, +0.2 initial). Review fixes and the owner's memory block,
// measured 378.6: Keep and Dismiss, the replaced/forgotten/undone wording,
// project facts' wording, the retry while the call is not filed (+0.7).
const MEMDEF_KB = 2.7;

// Workspace memory (T54, owner 2026-11-14), measured 387.8 against the
// recorded 386.5: the workspace's Memory tab (its own chunk with Settings),
// the waiting count on the Overview, the block's wording for the shared and
// Personal places (+1.3). Its review (F2, F4, F5: the two-step proposal,
// who wrote each fact, retire before delete), measured 392.0 (+0.4).
// Merged with the step limit (pull request 106): measured 393.0, a tenth over the
// sum of the two lines (rounding), recorded here (+0.1).
const WSMEM_KB = 1.8;

// What the agent is doing (feat/turn-finished-signal), measured 377.9
// (initial 101.0) against 375.9 on main (+2.0, not split per chunk): in the
// conversation chunk the retry countdown, the context meter against the
// window, the sub-agent card's activity line and their strings; in the
// first load the fold's `model_retry` and limits, the "finished, unread"
// hook with its notification, the sidebar's dot and its strings.
// After its review and the owner's side panel (sub-agents and background
// commands beside the conversation, its own lazy chunk, which replaces the
// background strip's), merged with the export above: measured 382.3
// (initial 101.1), +1.7 over both lines (the total rounds up).
const VISIBILITY_KB = 3.7;

// T50, a chat's approval mode, measured 377.0 (initial 100.8) against 375.9
// (initial 100.5) on main: in the first load the composer's pick for a new
// chat, the session prop, the Select's disabled and stacked options and the
// fold's case (+0.3); the picker in its own lazy chunk, the mode-change line
// in the conversation chunk and its API call in the page-only chunk (+0.8).
// The owner's defaults (2026-11-14), measured 377.2 after merging main: the
// host chat's Auto line, the host warning's words and the notification's
// fixed words (+0.1). After the owner's answer on routines and the merge
// with visibility, measured 386.4: the routine editor's webhook line (+0.1).
const T50_KB = 1.3;

// The loop's step limit (pull request 96) recorded no line of its own: the
// "step limit reached" divider and its string in the conversation chunk.
// Measured with approval modes merged on top: the total passes 386.4 by
// less than a tenth (+0.1).
const LOOP_KB = 0.1;
// X.14 (2026-11-15, the chat's step limit): the lazy "N steps" control and
// its form (StepLimitPick), the fold of `step_limit_changed`, its divider and
// the route call; measured 387.3 (rounded) after it against 386.5 before.
const STEPS_KB = 0.9;

// A turn's changes (X.12, feat/turn-diff), measured 389.6 (initial 101.5,
// unchanged) against 386.4 on main: the diff panel in its own lazy chunk,
// loaded on the first "Show changes" (2.6, its stylesheet 0.9 beside it),
// the shared chunk the panel and the conversation now take the approval
// card's hidden-character marks and the size words from (two more chunks),
// and the row's button, the open panel's state and the lazy import in the
// conversation chunk (+0.3). Its review's fixes, measured 390.2: lines
// mounted only while a file is open with "Show more", code marks that keep
// indentation and say CRLF once, the rename count, and the side panel
// yielding the right side (+0.6).
const DIFF_KB = 3.8;

// T51, a denial's note and "allow for this chat", measured 387.2 (initial
// 101.6) against 386.5 on main: the card's note field, its button and the
// note on a denied card in the conversation chunk, the side panel's
// "Allowed in this chat" rows with Remove in its own chunk, the fold's two
// events and the types in the first load (+0.1 initial). After its review,
// measured 387.4: the card's line on what a chat grant allows and the side
// panel's every-argument text (+0.2); 391.2 after the merge of the diff view (+0.1, rounding).
const T51_KB = 1.0;

// Two agent switches of their own (the owner, 2026-11-14): "MCP servers"
// and "Earlier chats and memory search", their labels and hints in the
// Agents page chunk. Measured 386.54 with both (+0.1); after merging the
// turn diff, 390.3.
const AGENT_SWITCHES_KB = 0.1;

// Rounded to 0.01 first, so a float sum (0.1 + 0.2) cannot round a tenth up.
// Another chat's work before an undo (PLAN.md T57, fix/undo-other-chats),
// measured 390.3 against 390.2: the diff panel's note naming the files
// another chat changed, in its lazy chunk, and the conversation row's
// "Undo anyway" for every refusal that offers it (+0.1). After its review
// (certain and possible files apart in the panel), measured 390.4 against
// a recorded 390.4: no change to the line; 394.1 after the merge of the
// step limit, workspace memory and approval answers (no change).
const T57_KB = 0.1;

// Shortened context inside a turn (ADOPTION_PLAN X.15, feat/turn-compaction),
// measured 394.5 against 394.2: the fold's `context_shortened` item and the
// chat's quiet line for it (with the summary collapsed, as for a history
// summary), in the conversation's chunk and the fold (+0.3). After its
// review, measured 394.6: the "Hide old tool outputs" setting's description
// says it acts only for a model whose window is unknown (+0.1).
const X15_KB = 0.4;

// Hand-off notes (ADOPTION_PLAN X.16, feat/handoff-note), measured 395.3
// against 394.6 (+0.7, not split by part): the fold's `handoff` item, the
// chat's quiet line for the latest note, and the "Small model and hand-off
// notes" card on the lazy Token savings page with its model picker. After
// its review, measured just over 395.4: the card says where the text goes
// and what a note costs (+0.2 recorded, the measure is a few hundredths over
// a tenth).
const X16_KB = 0.9;

// The chat's cost estimate (X.18, feat/cost-estimate), measured 397.6
// (initial 102.0) against 395.4 (initial 101.9) on main: the estimate, its
// rounding and words and the header's cost in the conversation chunk
// (+0.8), the per-model price fields and the currency field in the
// provider form's chunk (Settings and onboarding), the sub-agent line's
// cost in the side panel, and in the first load the fold's per-model sums
// (+0.1). After its review, measured 397.9 (initial 102.3) against 395.4
// (initial 101.9) on main with pull request 118: the fold reads model ids
// by own key only, in byte order, capped where an event is read (first
// load, +0.3), the currency rule and the decimal-only price fields (+0.3).
const COST_KB = 2.5;

// `@path` file mentions (ADOPTION_PLAN X.17, PLAN.md T63, feat/context-inputs),
// measured 399.3 (initial 103.0) against 397.9 (initial 102.3) on main with
// pull requests 118 and 119: in the first load, the composer's path tokens,
// its "Files" group and "no match" line, the debounce, the chips of the
// paths that will go with the message, each removable, and the check of the
// text as sent (+0.7); a lazy chunk with the paths a message mentions, the
// existence check (one exact question a path, answers kept 10 s) and the
// folder's completion call, which only a chat with a folder loads (+0.4,
// most of it the chunk's own overhead); the chip in the conversation's
// chunk and the fold (+0.2).
const X17_KB = 1.4; // the measure is a few hundredths over 399.3

// Rounded to 0.01 first, so a float sum (0.1 + 0.2) cannot round a tenth up.
// Parallel reads and /compact (ADOPTION_PLAN X.19, X.20,
// feat/parallel-reads-and-compact): measured just over 398.5 with and without
// it (origin/main at 5832dbb already measured 398.5 against its recorded
// 398): the composer's command, the fold's manual summary and failure, the
// working line and Stop while it runs add under a twentieth. The line (+0.6)
// records the measure.
const X19_X20_KB = 0.6;

// Plan mode (ADOPTION_PLAN X.7, feat/plan-mode): measured 400.1 against
// origin/main's 399.0 the same day (486bd15, built side by side): its bar
// is a lazy chunk loaded on the first chat in plan mode, plus the picker's
// fifth choice and the conversation's lines. The first load did not grow
// (103.2).
const X7_KB = 1.1;

// Rounded to 0.01 first, so a float sum (0.1 + 0.2) cannot round a tenth up.
// Retry on a failed turn (X.21, PR 128), all in lazy chunks (the chat page,
// the export): the Retry link and its route call, the retry item of the fold,
// `lib/retry.ts` and the export's line. Measured 399.4 here, 400.3 in the
// review's merge; 0.5 is what the feature adds on top of the line before it.
const X21_KB = 0.5;

// The sidebar to the owner's sketch of 2026-11-16 (PR 132): boxed projects
// with their chats, sub-agent chats as a tree, the node badge, the marks on
// closed groups, and Home's notices as their own chunk. Measured 403.5
// against origin/main's 401.9 the same day (f24aa00), so +1.6; the first
// load is 103.8. The owner approved recording this line on 2026-11-16. What
// main was already over its record (401.9 against 401.6) is not recorded
// here.
const SIDEBAR_KB = 1.6;

// What main was over its record after pull requests 130 (images through
// `read`) and 131 (read-only agents' shell), which recorded no line: 401.9
// measured against 401.6 on 2026-11-16. Recorded under the owner's standing
// yes of 2026-11-16 for soft lines (never the first-load ceiling).
const X23_T70_KB = 0.3;

// Background sub-agents (X.24, PR 134): the side panel's rows, Stop all and
// states (a lazy chunk), the agent page's switch and the fold's two cases.
// Measured 403.9 against origin/main's 403.5 the same day (2e0a60c): +0.4,
// recorded as 0.5 because the measure is a few hundredths over 403.9. The
// first load is 103.9.
const X24_KB = 0.5;

// The agent switch as chat state (X.28, PR 135): the pick's pending state
// and refusal line in the conversation chunk, the notice with its model, and
// the microphone recorder moved out of the first load into `lib/voice.ts`
// (a new lazy chunk). Measured 404.5 against origin/main's 403.9 the same
// day (4d138ed1), so +0.6; the first load is 103.9, a few bytes under main.
const X28_KB = 0.6;

// A sent message shown at once and the waiting line (PR 138): the outbox
// overlay, `TurnStatus`, the home screen's `startChat` flow and the failed
// bubble's Retry and Edit, all in lazy chunks. Measured 406.8 against
// origin/main's 404.5 the same day (b8cc2925), so +2.3; the first load is
// 103.9, a few bytes under main.
const WAITING_KB = 2.3;

// The account menu, Profile, Personalisation and the workspace's Agents tab
// (X.29, PR 136): the Profile and Personalisation tabs, the picture crop,
// the Agents tab and Home's Attention list as its own chunk, all lazy.
// Measured 413.7 against origin/main's 406.8 the same day (58774df2), so
// +6.9; the first load went DOWN to 103.6 (the Attention list left it).
const X29_KB = 6.9;

// The model's thinking kept and shown (X.25, PR 137): the fold's thinking
// item (first load, a few dozen bytes) and the lazy `ThinkingRow`. Measured
// 414.1 against origin/main's 413.7 the same day (a7a17153), so +0.4,
// recorded as 0.5 because the measure is a few hundredths over; the first
// load is 103.7.
const X25_KB = 0.5;

// The Project button and its popup (X.26, PR 143): the lazy `ProjectPicker`
// dialog (projects, New project, Existing folder), the slug preview and the
// always-shown execution mode label. Measured 416.9 against origin/main's
// 414.1 the same day (88b3e575), so +2.8, of which 0.2 was room left under
// the line before; the first load is 103.7, unchanged.
const X26_KB = 2.7;

// The board switch on the agent page and the "by an agent" mark on a card
// (X.32, PR 145), both in lazy chunks. Measured 417.03 against origin/main's
// 416.9 the same day (e509a453), so +0.1; the first load is 103.7, unchanged.
const X32_KB = 0.1;

// Workspace agents (X.33, PR 146): New agent, the agent page in the
// workspace's scope, origin marks, the hidden-agent row and the "made by this
// workspace's admins" line, all in the lazy settings and agent-page chunks.
// Measured 417.73 against origin/main's 417.03 the same day (9b7615bc), so
// +0.7; the first load is 103.7, unchanged.
const X33_KB = 0.7;

// The Plugins screen (X.34 step A, PR 147): a lazy chunk of 2.0 with the
// list, the filter, the search and the Shared / Assigned editor, and about
// 0.2 on the agent page for the Skills and Connectors switches. Measured
// 419.89 against origin/main's 417.73 the same day (9ba1a7cb), so +2.16, of
// which 0.07 was room left under the line before; the first load is 103.7,
// unchanged.
const X34A_KB = 2.1;

// Stored skills on the Plugins screen (X.34 step B, PR 148): the Add menu
// (write a skill, upload a SKILL.md), the skill viewer and editor, origin
// marks and the hidden-skill note, all in the lazy Plugins chunk (2.1 to
// 3.4). Measured 421.20 against origin/main's 419.89 the same day
// (239484b5), so +1.3; the first load is 103.7, unchanged.
const X34B_KB = 1.3;

// A repository's MCP servers on the project page (X.36, PR 150): the list,
// Reload, Approve and Remove, each entry shown string by string, in the lazy
// project chunk. Measured 422.31 against origin/main's 421.20 the same day
// (2026-11-18), so +1.12, recorded as 1.2 because the line is kept to one
// decimal and 1.1 would leave it 0.01 short; the first load is 103.7,
// unchanged.
const X36_KB = 1.2;

// Forge connections on the Plugins screen (X.27 step 1, PR 151): one item
// per allowed host with Connect, Replace token and Disconnect, the scopes
// and the plain-words note, in the lazy Plugins chunk (3.4 to 4.5). Measured
// 423.48 against origin/main's 422.31 the same day (28562202), so +1.17, of
// which 0.09 was room left under the line before; the first load is 103.7,
// unchanged.
const X27A_KB = 1.1;

// The fold guard and the boundaries (A.25, PR 156): the guard's field table
// and enum lists, the own-key lookups, the per-item and per-screen error
// boundaries and the cards' shape checks. The fold itself left the first
// load for a chunk of its own (4.5), so the first load FELL from 103.7 to
// 101.3 while the total grew. Measured 427.30 against origin/main's 423.48
// the same day (abc47904), so +3.8.
const WEBGUARD_KB = 3.8;

// Custom connectors on the Plugins screen (X.35 step 1, PR 155): the list,
// Add / Edit / Delete for editors, each person's Connect, Replace token,
// Disconnect and Check with the offered tools, in the lazy Plugins chunk.
// Measured 429.06 against origin/main's 427.30 the same day (8705c126), so
// +1.8; the first load is 101.3, unchanged.
const X35A_KB = 1.8;
// The composer's agent picker and @mention list mark a workspace's own
// agent (X.33's leftover): `marked` in lib/agents.ts, first-load code.
// Measured 429.11 against origin/main's 429.06 the same day (b9870654), so
// +0.1 as recorded; the first load is 101.4 (was 101.3), under the 104 ceiling.
const PICKMARK_KB = 0.1;
// Agents call a custom connector's tools (X.35 step 2, PR 157): the
// turn's line about connectors, the approval modes' wording in the lazy
// Plugins text and one line on a memory proposal's card that says it comes
// from a chat someone else acted in. Measured 429.26 against the recorded
// 429.2 the same day (a3ef442c merged in), so +0.1; the first load is
// 101.4, unchanged.
const X35B_KB = 0.1;
// "From a repository" in the Project popup (X.27 step 3, PR 164): the
// repository picker as its own lazy chunk (2.6 KB), the popup's and the
// project page's pending and failed states, and in the first load the
// pending-clone store, the sidebar's cloning rows and the dismiss call.
// Measured 432.53 against the recorded 429.3 the same day (3f0859a3 merged
// in), so +3.3; the first load is 101.7 (was 101.4), under the 104 ceiling.
const X27C_KB = 3.3;
// Calling several agents by mentions (X.37 step 1, PR 166): the mention
// scanner as its own lazy chunk (1.2 KB, loaded when the text holds an
// `@`), mention chips in messages, the status line of a running list, and
// in the first load the composer's chips for who will be called.
// Measured 435.26 against the recorded 432.6 the same day (01c5aa39), so
// +2.7; the first load is 102.1 (was 101.7), under the 104 ceiling.
const X37A_KB = 2.7;
// The e-mail for commits in the profile's settings (X.38, PR 167): one
// field, its validation message and the save call, all in the lazy profile
// page. Measured 435.58 against the recorded 435.3 the same day (PR 166's
// content merged in), so +0.3; the first load stays 102.1, under the 104
// ceiling.
const X38_KB = 0.3;
// The trial's second batch of UI faults (PR 170): the first-run steps kept
// across a reload with a validated stored list (the lazy first-run page),
// one "server is not reachable" sentence for requests and uploads that get
// no answer (0.1 in the first load: the string must be there when the
// server is down), and the chat header's meta row. Measured 436.02 against
// the recorded 435.6 the same day (d96f46ec merged in), so +0.5; the first
// load is 102.54 (was 102.41), under the 104 ceiling.
const TRIALUI2_KB = 0.5;
// A turn's usage while it runs (X.39, PR 174): the fold of the `turn_usage`
// event (the settled sums and the running turn's figure) and the check
// that usage numbers are non-negative whole numbers, both in the lazy fold
// chunk. Measured 436.28 against the recorded 436.1 the same day (a5dd27a3
// merged in), so +0.2; the first load stays 102.5, under the 104 ceiling.
const X39_KB = 0.2;
// The header's running sub-agents (X.39 follow-up, PR 176): the rule that
// counts a turn's foreground sub-agents while their call has no result, the
// per-turn floor and the watches on their logs, in the lazy conversation
// chunk. Measured 436.98 against the recorded 436.3 the same day (56809cfd
// merged in), so +0.7; the first load is 102.56 (was 102.54), under the 104
// ceiling.
const X39B_KB = 0.7;
// Sub-agents for coding (X.41, PR 178): the "may delegate to" choice on the
// agent page (lazy Agents chunk), the delegate card's model and open answer
// with its own cut and "Show all", and the line that replaces the composer
// in a sub-agent-only agent's chat (lazy conversation chunk). Measured
// 438.14 against the recorded 437.0 the same day (main with PR 177 in), so
// +1.2; the first load is 102.62 (was 102.56), under the 104 ceiling.
const X41_KB = 1.2;
// Undo and the branch (X.42, PR 180): the sentence on the Undo block and
// the changes panel when HEAD moved while a turn ran, "Put back what this
// undo replaced", and the fold's memory of which turns each restore found
// undone, in the lazy conversation and fold chunks. Measured 439.51 against
// the recorded 438.2 the same day (main at b2493ac8), so +1.4; the first
// load is 102.63 (was 102.62), under the 104 ceiling.
const X42_KB = 1.4;
// The cost hint names which price each unpriced model lacks, and the price
// form warns under an empty cached-input field (PR 182, lazy chunks).
// Measured 439.70 and a few bytes, just over 439.7, against the recorded
// 439.6 (main at 0870b9a3 measured 439.5), so +0.2 to clear it; the first
// load is 102.6, unchanged, under the 104 ceiling.
const PRICEHINT_KB = 0.2;
// The approval card shows the rows a line edit replaces and reads `lines`
// as "rows 12-15 →" (X.44, PR 187), and the background strip says "may
// still be running" for an end nobody confirmed (X.43, PR 186, which
// measured exactly at the old line and recorded nothing). Measured 440.22
// against the recorded 439.8, so +0.5 to clear it; the first load is 102.6,
// unchanged, under the 104 ceiling.
const X44_KB = 0.5;
// The plan-review pane of plan mode (X.7, PR 193): a lazy chunk of its own
// (the pane, its block split, its diff and its styles), and the Markdown
// renderer now sits in a chunk the conversation and the pane share. Measured
// 447.51 against the recorded 440.3, so +7.3 to clear it; the first load is
// 102.7 (+0.1), under the 104 ceiling.
const X7_REVIEW_KB = 7.3;
// Three things the first paint does not need moved out of the first load
// into chunks of their own (PR 195: the similar-chats list of the home
// screen, the mode classifier and its rules, the pending-clone rows), plus
// the cost hint on a sub-agent's row. The first load went from 102.7 to
// 100.3; the total pays for six more chunks: measured 449.62 against the
// recorded 447.6, so +2.1 to clear it.
const DIET_KB = 2.1;
// The project page says where a repository's approved MCP server will run
// (in the sandbox, on the host, or not known yet) and which entries a host
// chat does not start (X.36 in host chats, PR 190, lazy chunk). Measured
// 449.89 against the recorded 449.7, so +0.2; the first load is 100.3,
// unchanged.
const X36_HOST_KB = 0.2;
// A waiting call's choice takes the composer's place (W.6, PR 194): the
// choice component with its key guard, the scroll box with its cue, and the
// flag that silences the page's typing shortcuts, in the conversation's
// chunk. Measured 451.15 against the recorded 449.9, so +1.3; the first load
// is 100.6 (+0.3: the composer hides its field and the shell reads the
// flag), under the 104 ceiling.
const W6_KB = 1.3;
// The agent avatars drawn as flat blobs with pill eyes (X.45, PR 192): 21
// body paths with the place for their eyes, five eye styles, twelve colours
// and the per-colour eye table in the first load (the avatar is in the chat),
// the grid picker in the Agents chunk. Measured 452.76 against the recorded
// 451.2, so +1.6; the first load is 101.7 (+1.1), under the 104 ceiling.
const X45_KB = 1.6;
// Web batch 2 (PR 197): the request id carried on errors and shown on the
// generic error displays, plain sentences for a bare 403, 422, 429 or 503,
// the socket's guarded reconnect after an ended login, aliases in the @
// popup, the second default-agent picker in Settings. Measured 453.36
// against the recorded 452.8, so +0.6; the first load is 102.0 (+0.3: the
// error helper and the socket's retry are in it), under the 104 ceiling.
const BATCH2_KB = 0.6;
// The Usage page for server admins (X.18 / X.39, PR 198): tokens per person
// and model for a period, priced with the chat header's own functions, in a
// chunk of its own under Settings. Measured 455.39 against the recorded
// 453.4, so +2.0; the first load is 102.0, unchanged.
const USAGE_KB = 2.0;
// The fixes after the first browser run of the real binary (2026-11-21): the
// plan review shown when its plan arrives after mount, the calm "off" state
// of project MCP servers, the reveal of a chosen tab on a narrow screen, the
// usage tables' own scroll, the composer following the live connection.
// Measured 455.85 against the recorded 455.4, so +0.5; the first load is
// 102.1, under the 104 ceiling.
const BROWSER_FIXES_KB = 0.5;
// Rewinding a conversation, the fold's half (X.46, T90): rewound items are
// marked through the one range rule (a second implementation of
// `harness_core::rewind`, pinned by the shared fixture and a fuzz fixture),
// the task list goes back with them, and a muted "rewound" rendering until
// the UI for it lands. Measured 456.47 against the recorded 455.9, so +0.6;
// the first load is 102.1, under the 104 ceiling.
const REWIND_FOLD_KB = 0.6;
// The "Chat history" switch on the Agents page (X.47, T91): one more row
// of the per-agent capabilities and its type. Measured 456.52 against the
// recorded 456.5, so +0.1; the first load is 102.1, under the 104 ceiling.
const HISTORY_SWITCH_KB = 0.1;
// The rewind UI (X.46): "Rewind to here" on the person's messages with its
// inline choice, the collapsed "N turns rewound" block with Undo and
// "Bring the files back", and the message put back into the composer; all
// in the chat's lazily loaded code. Measured 458.89 against the recorded
// 456.6, so +2.3; the first load is 102.1, under the 104 ceiling.
const REWIND_UI_KB = 2.3;
// Fork a chat, part 1 (X.48, T92): "Fork from here" on a finished turn's
// end line, the divider and the shared-folder note in the fork, copied
// turns' changes named without an Undo, and the fold's rule for copied
// events (usage, checkpoints, grants). Measured 459.75 against the
// recorded 458.9, so +0.9; the first load is 102.1, under the 104 ceiling.
const FORK_KB = 0.9;
// The fork UI's details after its browser run: a fresh fork's header
// figures, the copied card's own label, the folder note in the flow, the
// side panel staying closed on a narrow screen. Measured 459.92 against
// the recorded 459.8, so +0.2; the first load is 102.1, under the ceiling.
const FORK_DETAILS_KB = 0.2;
// Fork a chat, part 2 (X.48, T92): the inline choice of where a fork's
// files live ("in its own copy of the repository" or "in the same
// folder"), the own-copy wording of the fork's note and of the delete
// dialog. Measured 460.70 against the recorded 460.0, so +0.7; the first
// load is 102.1, under the 104 ceiling.
const FORK_CHECKOUT_KB = 0.7;

const TOTAL_KB = Math.ceil(Math.round((BASE_KB + I2_KB + I3_KB + I4_KB + L14_KB + P1_KB + R6_KB + SPLIT_KB + T8D_KB + GLASS_KB + N10A_KB + N10A2_KB + TODO_KB + X5_KB + SUBAGENT_KB + EXPORT_KB + MEMDEF_KB + VISIBILITY_KB + T50_KB + LOOP_KB + DIFF_KB + T51_KB + WSMEM_KB + STEPS_KB + AGENT_SWITCHES_KB + T57_KB + X15_KB + X16_KB + COST_KB + X17_KB + X19_X20_KB + X21_KB + X7_KB + SIDEBAR_KB + X23_T70_KB + X24_KB + X28_KB + WAITING_KB + X29_KB + X25_KB + X26_KB + X32_KB + X33_KB + X34A_KB + X34B_KB + X36_KB + X27A_KB + WEBGUARD_KB + X35A_KB + PICKMARK_KB + X35B_KB + X27C_KB + X37A_KB + X38_KB + TRIALUI2_KB + X39_KB + X39B_KB + X41_KB + X42_KB + PRICEHINT_KB + X44_KB + X7_REVIEW_KB + DIET_KB + X36_HOST_KB + W6_KB + X45_KB + BATCH2_KB + USAGE_KB + BROWSER_FIXES_KB + REWIND_FOLD_KB + HISTORY_SWITCH_KB + REWIND_UI_KB + FORK_KB + FORK_DETAILS_KB + FORK_CHECKOUT_KB) * 100) / 10) / 10;
// Raised from 103 to 106 once the Phase A pages merged (103.7 measured: the
// new sidebar, account button and routine-result rows are on the first
// screen), then lowered to 104 at Phase B (102.3 measured) once the login
// form moved to its own chunk. The rest is for the Phase C image attach in
// the composer (~0.4). Still under the long-standing 110 KB first-load budget.
// Three security fixes had brought it back to 103.9. On 2026-11-11 what only
// lazy pages use left the first load (the page-only API calls, the model plan
// helpers, the page-only English strings, 19 of the project icons: 4.6 KB), so
// the initial load measured 99.3. The ceiling stays 104: the room is for the
// next fixes, not for features.
const PROGRAM_INITIAL_KB = 104;
const web = new URL("../", import.meta.url).pathname;
const dist = join(web, "dist");
const manifest = JSON.parse(readFileSync(join(dist, ".vite/manifest.json"), "utf8"));

const read = (file) => readFileSync(file.startsWith("/") ? file : join(dist, file));
const gz = (file) => gzipSync(read(file)).length / 1024;
const failures = [];

// Initial = entry chunks plus their static imports (not dynamic ones).
const initial = new Set();
const walk = (key) => {
  const c = manifest[key];
  if (!c || initial.has(c.file)) return;
  initial.add(c.file);
  (c.imports ?? []).forEach(walk);
};
for (const [key, c] of Object.entries(manifest)) if (c.isEntry) walk(key);

// Every emitted file counts. The manifest lists the main build only, not the
// highlighter worker or the grammar chunks it loads.
const assets = readdirSync(join(dist, "assets")).map((f) => `assets/${f}`);
const allJs = assets.filter((f) => f.endsWith(".js"));
const sizes = new Map(allJs.map((f) => [f, gz(f)]));
const initialKb = [...initial].filter((f) => f.endsWith(".js")).reduce((a, f) => a + sizes.get(f), 0);
const totalKb = allJs.reduce((a, f) => a + sizes.get(f), 0);

for (const f of [...allJs].sort((a, b) => sizes.get(b) - sizes.get(a))) {
  console.log(`  ${sizes.get(f).toFixed(1).padStart(6)} KB  ${initial.has(f) ? "initial " : "        "}${f}`);
}
// Stylesheets and fonts come with their chunks but are outside the JS
// budget; they are listed so the whole download is visible. A font is
// compressed already, so its own size is what travels.
for (const f of assets.filter((f) => !f.endsWith(".js")).sort()) {
  const font = /\.(woff2?|ttf|otf)$/.test(f);
  const kb = font ? read(f).length / 1024 : gz(f);
  console.log(`  ${kb.toFixed(1).padStart(6)} KB  ${(font ? "font" : f.split(".").pop()).padEnd(8)}${f}`);
}
console.log(`initial JS: ${initialKb.toFixed(1)} KB gzip (budget ${INITIAL_KB}, ceiling ${PROGRAM_INITIAL_KB})`);
console.log(`total JS:   ${totalKb.toFixed(1)} KB gzip (budget ${TOTAL_KB}, soft), ${allJs.length} chunks`);

/** The emitted chunks a lazy part must have; none means it is not lazy any more. */
const chunk = (re, what) => {
  const found = allJs.filter((f) => re.test(f));
  if (found.length === 0) failures.push(`no ${what} chunk in dist/assets`);
  return found;
};
/** A lazy part's chunks: they must exist and stay out of the initial load. */
const lazyChunk = (re, what) => {
  const found = chunk(re, what);
  for (const f of found) if (initial.has(f)) failures.push(`${f} (${what}) is in the initial load`);
  return found;
};
// The mode backgrounds and their host load only when one shows, and the
// still copy beside chats only when it is turned on.
lazyChunk(/\/Backdrop-/, "background host");
lazyChunk(/\/SearchBg-/, "Search background");
lazyChunk(/\/WallBg-/, "Ask background");
lazyChunk(/\/BinaryBg-/, "Agentic background");
lazyChunk(/\/ChatGutter-/, "chat gutter");
// T.8d step 2: the chat's background strip loads only while a chat has a
// background command running.
lazyChunk(/\/SidePanel-/, "side panel");
// The agent's task list loads only once a chat has one.
lazyChunk(/\/TodoPanel-/, "task list panel");
// A turn's changes load on the first "Show changes".
lazyChunk(/\/DiffPanel-/, "turn diff panel");

// I3: its lazy chunks (the search panel, the account menu, and the login
// form, which signed-in visits never draw).
const lazyI3 = [
  ...lazyChunk(/\/SearchPanel-/, "search panel"),
  ...lazyChunk(/\/AccountMenu-/, "account menu"),
  ...lazyChunk(/\/Login-/, "login form"),
];

// I4: its lazy chunks (the area pages, their previews, and the composer's
// image shrinking, which loads on the first image put in a message).
const lazyI4 = [
  ...lazyChunk(/\/Routines-/, "Routines page"),
  ...lazyChunk(/\/Media-/, "Media page"),
  ...lazyChunk(/\/Artifacts-/, "Artifacts page"),
  ...lazyChunk(/\/MediaPreview-/, "Media preview"),
  ...lazyChunk(/\/downscale-/, "image downscale"),
];

console.log(`lazy chunks checked: ${lazyI3.length} (I3), ${lazyI4.length} (I4)`);
// What the home screen and every chat load: math and highlighting must not.
const eager = [...initial, ...chunk(/\/Conversation-/, "Conversation")];
// An installed package's file, found the way the package that pulls it in
// finds it (pnpm keeps transitive dependencies out of web/node_modules).
const installed = (via, file) => {
  try {
    const from = via.reduce((at, dep) => createRequire(at).resolve(dep), join(web, "package.json"));
    return [createRequire(from).resolve(file)].filter(existsSync);
  } catch {
    return [];
  }
};

// Each library is found by a string only it contains, one that survives
// minification. A marker must also match where the library does live (its
// lazy chunk, or its installed package when nothing may bundle it);
// otherwise a check against it would pass without testing anything.
const markers = [
  // micromark-extension-math's HTML half imports KaTeX; it must tree-shake out.
  { lib: "KaTeX", re: /KaTeX parse error/, banned: allJs, home: installed(["remark-math", "micromark-extension-math"], "katex/dist/katex.mjs") },
  // Our own code names mermaid fences, so the marker is the library's API
  // object. Mermaid is not installed, so there is nothing to hold it to.
  { lib: "mermaid", re: /mermaidAPI/, banned: allJs, home: [] },
  // refractor's build has no `Prism.languages`: its core throws "Unknown
  // language" and its grammars call `languages.extend`.
  { lib: "Temml", re: /temml/i, banned: eager, home: chunk(/\/math-/, "math") },
  { lib: "the highlighter", re: /Unknown language|languages\.extend/, banned: eager, home: chunk(/\/highlight\.worker-/, "highlighter worker") },
];
const has = (file, re) => re.test(read(file).toString("utf8"));
for (const m of markers) {
  for (const f of m.banned) if (has(f, m.re)) failures.push(`${f} contains ${m.lib}`);
  if (m.home.length && !m.home.some((f) => has(f, m.re))) failures.push(`the ${m.lib} marker ${m.re} matches nothing in ${m.home.join(", ")}`);
}
// A worker has no DOM: a browser build that needs one breaks highlighting.
for (const f of allJs.filter((f) => /\/highlight\.worker-/.test(f))) {
  if (has(f, /\b(document|window)\./)) failures.push(`${f} uses the DOM, which a worker does not have`);
}
if (initialKb > INITIAL_KB) failures.push("initial JS is over its budget");
// Soft: reported, never a failure (it failed CI on main while called soft).
if (totalKb > TOTAL_KB) {
  console.warn(`warning: total JS is ${totalKb.toFixed(1)} KB, over the recorded ${TOTAL_KB}: raise your line above to it with the reason`);
}
if (initialKb > PROGRAM_INITIAL_KB) failures.push(`initial JS is over the program ceiling of ${PROGRAM_INITIAL_KB} KB`);
for (const msg of failures) console.error(msg);
if (failures.length) process.exit(1);
