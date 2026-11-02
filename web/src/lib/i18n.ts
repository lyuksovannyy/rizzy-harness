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
  "nav.search": "Search chats",
  "nav.recents": "Recents",
  "nav.settings": "Settings",
  "nav.logout": "Log out",
  "nav.collapse": "Collapse sidebar",
  "nav.expand": "Open sidebar",
  "nav.noChats": "No chats yet.",
  "nav.noMatches": "No matching chats.",
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
  "agentic.noGrant": "Agentic sessions are not enabled for your account. Ask an admin.",
  "agentic.noWorker": "No worker node is online.",
  "effort.default": "Effort: default",
  "settings.title": "Settings",
  "settings.profile": "Profile",
  "settings.preferences": "Preferences",
  "settings.provider": "AI provider",
  "settings.password": "Password",
  "settings.users": "Users",
} as const;

type Catalog = typeof en;
export type Key = { [K in keyof Catalog]: Catalog[K] extends string ? K : never }[keyof Catalog];
export type ListKey = { [K in keyof Catalog]: Catalog[K] extends readonly string[] ? K : never }[keyof Catalog];

function fill(s: string, vars?: Record<string, string>): string {
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
