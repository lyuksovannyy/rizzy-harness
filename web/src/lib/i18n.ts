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
  "nav.logout": "Log out",
  "nav.collapse": "Collapse sidebar",
  "nav.expand": "Open sidebar",
  "nav.noChats": "No chats yet.",
  "workspace.personal": "Personal",
  "workspace.new": "New workspace…",
  "workspace.newName": "Workspace name",
  "workspace.create": "Create",
  "workspace.cancel": "Cancel",
  "workspace.settings": "Workspace settings",
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
  "conv.incognito": "Incognito chat. It is not in your history, search or memory and is deleted after it has been idle for a day. The AI provider still receives what you send.",
  "conv.deleteNow": "Delete now",
  "conv.delete": "Delete chat",
  "conv.deleteConfirm": "Delete this chat for good?",
  "conv.summary": "Earlier messages were summarised for the model",
  "conv.tokens": "tokens",
  "conv.context": "context",
  "conv.readOnly": "This chat is read-only: {reason}.",
  "composer.readOnly": "Read-only",
  "agentic.noGrant": "Agentic sessions are not enabled for your account. Ask a server admin.",
  "agentic.noRole": "Your role in {workspace} can't start agentic sessions.",
  "agentic.noWorker": "No worker node is online.",
  "effort.default": "Effort: default",
  "settings.title": "Settings",
  "settings.section.account": "Account",
  "settings.section.workspace": "Workspace",
  "settings.section.workspace-admin": "Workspace admin",
  "settings.section.server": "Server",
  "settings.profile": "Profile",
  "settings.preferences": "Preferences",
  "settings.memory": "Memory",
  "settings.password": "Password",
  "settings.overview": "Overview",
  "settings.general": "General",
  "settings.models": "Models",
  "settings.members": "Members",
  "settings.delete": "Delete",
  "settings.providers": "AI providers",
  "settings.recall": "Memory recall",
  "settings.savings": "Token savings",
  "settings.nodes": "Nodes",
  "settings.users": "Users",
  "settings.workspaces": "Workspaces",
  "settings.storage": "Storage",
  "settings.workspaceDefault": "Workspace default",
  "settings.defaultsHint": "Applies in every workspace; leave it on 'Workspace default' to follow each workspace.",
  "settings.appearance": "Appearance",
  "settings.theme": "Theme",
  "settings.accent": "Accent",
  "theme.system": "System",
  "theme.light": "Light",
  "theme.dark": "Dark",
  "settings.backdrop": "Home background",
  "backdrop.animated": "Animated",
  "backdrop.still": "Still",
  "backdrop.off": "Off",
  "settings.backdropHint":
    "Each mode has its own background on the home screen. Still keeps them without motion; your system's reduce-motion setting does the same.",
  "settings.backdropChats": "Also in chats",
  "settings.backdropChatsHint": "A quiet, still version beside chats on wide screens.",
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

type Catalog = typeof en;
export type Key = { [K in keyof Catalog]: Catalog[K] extends string ? K : never }[keyof Catalog];
export type ListKey = { [K in keyof Catalog]: Catalog[K] extends readonly string[] ? K : never }[keyof Catalog];

export function fill(s: string, vars?: Record<string, string>): string {
  return vars ? s.replace(/\{(\w+)\}/g, (m, k: string) => vars[k] ?? m) : s;
}

export function t(key: Key, vars?: Record<string, string>): string {
  return fill(en[key] as string, vars);
}

/** Pick variant `index` (wrapped) from a list entry. */
export function tPick(key: ListKey, index: number, vars?: Record<string, string>): string {
  const list = en[key] as readonly string[];
  return fill(list[((index % list.length) + list.length) % list.length]!, vars);
}
