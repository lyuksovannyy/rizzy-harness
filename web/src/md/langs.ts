// Fence languages we highlight, and their aliases. The grammars themselves
// load lazily (highlight.ts); this map stays in the Conversation chunk so an
// unknown language never pulls in the highlighter.

export const GRAMMARS = [
  "bash",
  "c",
  "cpp",
  "css",
  "diff",
  "docker",
  "go",
  "java",
  "javascript",
  "json",
  "jsx",
  "markup",
  "python",
  "rust",
  "sql",
  "toml",
  "tsx",
  "typescript",
  "yaml",
] as const;

export type Grammar = (typeof GRAMMARS)[number];

const ALIASES: Record<string, Grammar> = {
  sh: "bash",
  shell: "bash",
  zsh: "bash",
  console: "bash",
  "c++": "cpp",
  patch: "diff",
  dockerfile: "docker",
  golang: "go",
  js: "javascript",
  jsonc: "json",
  html: "markup",
  xml: "markup",
  svg: "markup",
  py: "python",
  rs: "rust",
  ts: "typescript",
  yml: "yaml",
};

const known = new Set<string>(GRAMMARS);

export function grammarFor(lang: string): Grammar | null {
  const l = lang.toLowerCase();
  const g = ALIASES[l] ?? l;
  return known.has(g) ? (g as Grammar) : null;
}
