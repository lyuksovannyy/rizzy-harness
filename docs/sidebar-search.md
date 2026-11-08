# Sidebar, search panel and account menu: design note

Status: Phase A of the sidebar areas (the integrated contract, sections 3 and 4), built on branch
`areas/i3-shell` on top of the Phase 0 scaffold. Routines, Media and Artifacts show in the sidebar
and in search once their backends answer `show: true` in `/api/features`; until they merge, their
`features()` stubs say false and their search providers return nothing.

## The sidebar

`<aside class="sidebar" aria-label="Sidebar">`, top to bottom:

1. **Header**: the app name, the connection icon (only when something is wrong), **Search**
   (`Search` icon, name "Search", title "Search (Ctrl K)", `aria-keyshortcuts="Control+K Meta+K"`)
   and **Collapse**. The workspace switcher left the header; workspaces live in the account menu.
2. **`<nav aria-label="Main">`**: New chat, Agents, then Routines, Media and Artifacts, each only
   while `features.<area>.show` is true for this viewer in this workspace, then **Connections**, a
   `<button aria-disabled="true">` with a "Later" tag and the title "Coming later" (no route, no
   chunk, no code). Routines carries a dot while routine results wait unseen in this workspace;
   its accessible name then says how many ("Routines, 2 new results"). Shell sums the `unseen` of
   this workspace's `result` and `failed` rows from `GET /api/attention` (`routines`).
3. **One scroll region** with two disclosure sections, open or closed as the user left them
   (localStorage `harness.sidebarSections`, read and written inside try/catch):
   - **Projects**: the five most recently changed, then "Show N more"; "+" when the viewer may
     create one; "No projects yet" when there are none.
   - **Recent**: the workspace's chats as before (running dot, title, project mark, node badge for
     agentic chats); "No chats yet." when empty. Routine run chats arrive filtered by the server.
4. **Footer**: the account button.

Every row is an `<a href>`: a plain click calls Shell's `navigate`, a modified or middle click and
"copy link" behave as on any link (`NavLink.tsx`). The row of what is on screen has
`aria-current="page"`. The props are `route` and `onNavigate` instead of one callback per page.

A workspace switch made on an area's page (agents, routines, media, artifacts) lands on that
area's list in the new workspace (`switchRoute` in `lib/workspaces.ts`); on a chat of the target
workspace it stays; anywhere else it goes home.

### Collapsed rail and the phone drawer

Collapsed on a desktop, the same component renders as a 56px rail (`rail`): Expand, New chat,
Search, Agents, the areas that show (Routines with its dot), Connections, a spacer, and the avatar.
Each icon has `aria-label` and `title`. The avatar's label is "<name> — <workspace> · <role>", and
in a team workspace the workspace's badge letter sits at its lower right, so the workspace stays
visible. The desktop topbar's Menu and New chat buttons are gone.

At 760px and below, Shell reads `matchMedia("(max-width: 760px)")` through
`useSyncExternalStore` and always puts the whole sidebar in the drawer, whatever the desktop
choice was. Opening the drawer focuses its first link and makes `.main` inert; Esc closes it and
returns focus to Menu (after the page is no longer inert). Search from the drawer closes the
drawer, then opens the panel full-screen.

## The account menu

The footer button shows the initial on the accent, the display name (or username) and the **tier
line** (`accountLine` in `lib/workspaces.ts`): "Acme · Admin" in a team workspace,
"Personal · Server admin" or "Personal · User" in Personal, where the user is always the owner.
Its accessible name and title are "<name> (@<username>) — <tier line>".

The menu is a lazy chunk (`AccountMenu`), imported on `pointerenter`, `focus` and `touchstart` of
the button, so the first click never waits. It is a top-layer popover (`md/anchored.ts`, which
gained `side: "right"` for the rail avatar and the submenu, and re-places itself when its content
changes size):

1. a header row (`menuitem`, `aria-haspopup="menu"`): avatar, name, tier line and a chevron; its
   accessible name carries `@username`. It opens **Workspaces**. There is no separate identity
   line;
2. Profile (`#/settings/account/profile`), Settings (`#/settings/account/preferences`);
3. Log out.

**Workspaces**: the Personal workspace first, labelled with the display name and hinted
"Personal", then team workspaces A–Z with their role and badge letter, each a `menuitemradio`
with the active one checked; then "+ New workspace…" when the server lets the user create one,
and Workspace settings. A failed switch shows an inline `role="alert"` row and the menu stays
open. A new workspace is named in a small native `<dialog>` (a text field may not live inside a
menu); once created, Shell switches to it.

Keys follow the APG menu button: Enter, Space or ↓ opens on the first item; ↑/↓/Home/End move
and wrap; → on the header row opens Workspaces on the checked workspace, ← or Esc comes back; Esc
or Tab closes the menu and refocuses the button. On a fine pointer, hovering the header row opens
Workspaces as a flyout after 120 ms. On a narrow screen or a coarse pointer it drills in place,
with "Back" first. "+ Add account" is not built (one HttpOnly cookie per origin).

## The search panel

A lazy chunk (`SearchPanel`), imported on pointer or focus on the Search button and on the first
shortcut; its Suspense fallback is nothing. It is a native `<dialog>` opened with `showModal()`
(focus trap, inert page, Esc, focus return), centred near the top at 640px wide and at most 70vh,
full-screen with a Close button at 760px and below. A click on the backdrop closes it.

- **Opening**: the header or rail button, **Ctrl/Cmd+K anywhere** (the composer too; pressed
  again while open, it selects the input) and "/" outside inputs, textareas, selects and
  editable content (`opensSearch` in Shell).
- **Layout**: an input with `role="combobox"`, `aria-controls` and `aria-activedescendant`; a
  tablist All | Chats | Media | Artifacts | Projects with a roving tabindex and ←/→/Home/End
  (automatic activation), showing only the kinds of the answer's `kinds`; a listbox; an
  `aria-live="polite"` line saying how many results show.
- **Behaviour**: it opens on the last tab used (localStorage `harness.searchTab`, All at first,
  All again when the viewer no longer has that tab) and always sends `in`. Nothing typed lists
  the newest items. Typing waits 150 ms; each request has its own AbortController, aborted by the
  next. A tab loads its next page when an IntersectionObserver sees the end of the list; All has
  no pages. ↑/↓/Home/End move the active row while focus stays in the input; Enter or a click
  opens it, closing the panel and the drawer. Everything but the tab resets on close.
- **Rows**, at most three lines: a leading icon (a same-origin 32px thumbnail for media and image
  artifacts, until it fails to load, say the file was deleted since; the project's icon for a project or a chat in one, CalendarClock for a routine run,
  Terminal for agentic, MessageSquare for a chat, the kind's icon for the rest); line 1 the title
  and the type, with `when(time_ms)` on the right; line 2 the preview; line 3 the excerpt, only
  when the server sent one. Lines 1–3 mark the matched words with `Highlight`.
- **Where a result opens**: a chat `#/s/<id>`, a project `#/p/<id>`, a media item
  `#/media/<id>`, an artifact `#/artifacts/<id>`.

The panel's and the menu's own words live in their chunks rather than in `lib/i18n.ts`, which is
part of the first load.

## The search API

`GET /api/search?q=&in=all|chats|projects|media|artifacts&limit=1..50&cursor=&workspace=&suggest=`

- `in` defaults to `chats`; anything else is 400. A kind the viewer does not have (its `show` is
  false) answers no results.
- `q` is cut to 500 characters and 16 words; words of one letter and stopwords do not count, so
  such a query lists the newest items.
- `cursor` is the previous page's `next` (a decimal offset); anything else is 400.
- `suggest=1` (or `true`) is the bar's "You may have asked this before": the strict path as
  before, chats only, in the same row shape.
- The answer is `{ results, next, kinds }`; `kinds` is chats and projects, plus media and
  artifacts while their areas show (the same flags `/api/features` sends as `search.kinds`).

One row:

```ts
interface SearchResult {
  kind: "chat" | "project" | "media" | "artifact";
  id: string;               // chat: its 36-character session id; others: 32 hex
  title: string;            // \u0002…\u0003 around matched words
  type: "chat" | "agentic" | "routine" | "project" | "image" | "video" | "audio"
      | "media_other" | "binary" | "package" | "archive" | "document" | "other";
  preview: string;          // line 2, marked; "" when none
  snippet: string | null;   // line 3: the FTS excerpt, only when title + preview miss a word
  time_ms: number;
  project_id: string | null;
  running?: boolean;        // chats
  thumb?: string | null;    // same-origin path only
}
```

`SearchHit` and its `score` are gone; Similar reads `id`, `time_ms` and `snippet ?? preview`, and
shows its age with `when`.

### Providers (`crates/master/src/search.rs`)

`search::run` gets a `Request` (the capped text, the kinds, the caller's owner and workspace, a
page size and offset) inside one `mirror.call`. A tab asks its one provider for a page plus one
row (to know whether there is a next page). All asks each provider the viewer has for its top 10
and merges them by tier, then newest first, capped at 30 (`merge`); with nothing typed it lists
the newest 20 across kinds. Tiers: 0 every word in the title or name, 1 some, 2 only in the body,
caption or metadata.

Every provider has the `Provider` signature of the Phase 0 contract, queries its own index
through `fts_query` (every word quoted, so FTS syntax in the input stays inert), applies its list
endpoint's visibility, never returns incognito content and gives `thumb` only as a same-origin
path. `search::run` holds every provider to that last rule (`checked`): a `thumb` that is a URL,
or a path a browser reads as one (`//host`, `/\host`), is dropped from the row. Titles and
previews have their control characters removed before `mark` wraps whole words
(case-insensitive, the last word also as a prefix, the same words `coverage` counts).

- **Chats** (`search::chats`): with words, bm25 over titles and messages through
  `Store::search`, every word required, paged through the deduped best-first window of 500 index
  rows; routine run chats are included (`type: "routine"`). Without words, the caller's newest
  visible chats in the workspace, as the chat list shows them, less incognito chats, sub-agent
  chats and routine run chats (`Store::routine_marks`). The preview is the first sentence of the
  latest message with text (`Store::latest_message_texts`, `first_sentence(…, 140)`), the project
  is the chat's effective project, and `type` is `routine`, `agentic` or `chat`.
- **Projects** (`search::projects`): `Store::projects(workspace)`, as the project list shows
  them (every member, guests too); with words, every word must be in the name or description.
  The preview is the description.
- **Media** and **Artifacts**: `media::search` and `artifacts::search`, owned by the files backend
  (their own FTS tables and visibility).

`Store::search` no longer returns sub-agent sessions (`parent_id IS NULL`), which also affects
suggestions; their parent chat is the hit.

### Privacy

Search is always in the Shell's workspace (`?workspace=` → `workspaces::scope`; a non-member gets
404, server admins included). Chats are the caller's own for everyone, admins included (T10); only
the API token without a workspace searches every chat. Previews and excerpts are plain text and
render as text. There is no cross-workspace search and no `#/search` route.

## Bundle

Phase B, measured 2026-11-08: the login form became its own lazy chunk (`App.tsx`, as
ChangePassword already was; signed-in visits never draw it), taking the first load from 103.7 KB
(the merged Phase A) to 102.1 KB. The split costs 1.1 KB of total: Login is 2.0 KB, and the eye
icon and the copy helper it shared with lazy pages became chunks of their own. `lazyChunk` keeps
it out of the first load.

Phase A, measured 2026-11-08: SearchPanel 2.6 KB and AccountMenu 2.3 KB gzip, lazy (checked by
`lazyChunk` in `web/scripts/size.mjs`), plus 1.1 KB of chunks they share with others; the first
load grew by 2.0 KB (the sidebar's areas, sections, rail and account button, the shortcuts and
drawer handling, and `when` through Similar, less the switcher and the inline search), to
102.8 KB against the 103 KB program ceiling.

## Tests

- Rust: `search.rs` units (`mark`, tiers and the All merge, chat rows and their types and
  excerpts, the empty search's visibility); store `search_never_finds_sub_agent_or_incognito_chats`;
  `crates/master/tests/search_http.rs` (each `in`, paging and the cursor, excerpts only when
  needed, newest first, FTS syntax inert, T10 with the server admin, non-members 404, a guest
  finding projects, the API token, `kinds` and features per viewer).
- Rust, with the other areas merged: a routine run chat found by a query as `type: "routine"`
  before and after its result is seen, never in the empty search
  (`routine_run_chats_are_found_but_never_listed_empty`); Routines shown to a guest who still owns
  a routine there, with why, until it is deleted (`routines_show_while_orphaned_ones_remain`);
  Media rows in the Media tab and All, with the thumbnail path the row loads, which only the
  owner can (`media_rows_carry_thumbnails_their_owner_can_load`); `checked` against artifact and
  media rows with foreign thumbnails (`file_rows_keep_only_same_origin_thumbnails`).
- Web: `sidebar.test.tsx`, `accountmenu.test.tsx`, `searchpanel.test.tsx` (including thumbnails,
  same-origin only, falling back to the icon), `shell.test.tsx` (shortcuts, the narrow drawer,
  the rail and the unseen count) and `lib/workspaces.test.ts` (`accountLine`, `switchRoute`).
- End to end: `web/e2e/phase4.cjs` (Ctrl+K from the composer, rows, the kept tab, Enter, "/" and
  Esc, the rail), `web/e2e/phase6.cjs` (an incognito chat is not found through the panel);
  `phase3.cjs` and `phase5.cjs` reach Settings through the account menu.

Still waiting on Artifacts (files backend, Phase B): the features case with a visible shared
artifact, and artifact rows from the real `artifacts::search`.
