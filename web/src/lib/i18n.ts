import type { pages } from "./i18nPages";
// English catalog behind a small `t()`. A real i18n library comes with the
// second language; until then this keeps the new-tab bundle small.
const en = {
  "greeting.morning": ["Good morning, {name}", "Morning, {name}", "Rise and shine, {name}"],
  "greeting.afternoon": ["Good afternoon, {name}", "Howdy, {name}", "Afternoon, {name}"],
  "greeting.evening": ["Good evening, {name}", "Evening, {name}", "Welcome back, {name}"],
  "greeting.night": ["Up late, {name}?", "Burning the midnight oil, {name}?", "Still here, {name}?"],
  "home.subtitle": "How can I help?",
  "session.untitled": "New chat",
  "nav.newChat": "New chat",
  "nav.search": "Search",
  "nav.recents": "Recent",
  "nav.agents": "Agents",
  "nav.collapse": "Collapse sidebar",
  "nav.expand": "Open sidebar",
  "nav.noChats": "No chats yet.",
  "workspace.personal": "Personal",
  "workspace.gone": "You are no longer in {name}; showing {here}.",
  "workspace.dismiss": "Dismiss",
  "role.owner": "Owner",
  "role.admin": "Admin",
  "role.member": "Member",
  "role.guest": "Guest",
  "nav.projects": "Projects",
  "nav.newProject": "New project",
  "mode.search": "Search",
  "mode.ask": "Ask",
  "mode.agentic": "Agentic",
  "mode.hint": "Tab switches mode",
  "composer.placeholder.ask": "Ask anything. Type @ to pick an agent.",
  "composer.placeholder.search": "Search the web",
  "composer.placeholder.agentic": "Describe a task for the agent",
  "composer.placeholder.session": "Reply… (@ to pick an agent)",
  "composer.running": "Working… press Stop to interrupt",
  "composer.send": "Send",
  "composer.stop": "Stop",
  "composer.node": "Node",
  "composer.folder": "Folder on that node",
  "composer.forMessage": "for this message",
  "composer.attach": "Attach files",
  "composer.attachHint": "Attach documents (text, code, Markdown, CSV, JSON, PDF; up to 5)",
  "composer.attachments": "Attachments",
  "composer.chars": "characters",
  "composer.truncated": "cut to fit",
  "composer.remove": "Remove",
  "composer.incognito": "Incognito",
  "composer.incognitoHint": "Temporary chat: not in history, search or memory, deleted after a day. The AI provider still receives it.",
  "conv.readOnly": "This chat is read-only: {reason}.",
  "composer.readOnly": "Read-only",
  "agentic.noGrant": "Agentic sessions are not enabled for your account. Ask a server admin.",
  "agentic.noRole": "Your role in {workspace} can't start agentic sessions.",
  "agentic.noWorker": "No worker node is online.",
  "effort.default": "Effort: default",
  "settings.section.workspace-admin": "Workspace admin",
  "home.toReview": "{n} new memories to review",
  "home.toReviewOne": "1 new memory to review",
  // Two blocks, one per owner, so parallel additions merge cleanly: each
  // adds lines only inside its own block.
  // ── shell (I3) ──
  "nav.searchShortcut": "Search (Ctrl K)",
  "nav.expandRail": "Expand sidebar",
  "nav.routines": "Routines",
  "nav.media": "Media",
  "nav.artifacts": "Artifacts",
  "nav.connections": "Connections",
  "nav.later": "Later",
  "nav.comingLater": "Coming later",
  "nav.newResult": "{name}, 1 new result",
  "nav.newResults": "{name}, {n} new results",
  "nav.noProjects": "No projects yet",
  "nav.showMore": "Show {n} more",
  "nav.showFewer": "Show fewer",
  "nav.account": "{name} (@{username}) — {tier}",
  "nav.railAccount": "{name} — {tier}",
  "account.serverAdmin": "Server admin",
  "account.user": "User",
  // ── end shell ──

  // ── feature pages (I4) ──
  // ── end feature pages ──
} as const;

type Catalog = typeof en & typeof pages;

// Strings the lazy pages add when they load (i18nPages.ts).
const extra: Record<string, string> = {};
export const extend = (more: Record<string, string>) => void Object.assign(extra, more);
export type Key = { [K in keyof Catalog]: Catalog[K] extends string ? K : never }[keyof Catalog];
export type ListKey = { [K in keyof Catalog]: Catalog[K] extends readonly string[] ? K : never }[keyof Catalog];

export function fill(s: string, vars?: Record<string, string>): string {
  return vars ? s.replace(/\{(\w+)\}/g, (m, k: string) => vars[k] ?? m) : s;
}

export function t(key: Key, vars?: Record<string, string>): string {
  return fill(((en as Record<string, unknown>)[key] ?? extra[key]) as string, vars);
}

/** Pick variant `index` (wrapped) from a list entry. */
export function tPick(key: ListKey, index: number, vars?: Record<string, string>): string {
  const list = en[key] as readonly string[];
  return fill(list[((index % list.length) + list.length) % list.length]!, vars);
}
