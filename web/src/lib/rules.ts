// Hand-written mode rules: the classifier's cold start and its baseline.
import type { Mode } from "./modes";

const QUESTION_WORDS = new Set([
  "what", "why", "how", "when", "where", "who", "which", "whats", "can", "could", "should",
  "would", "is", "are", "does", "do", "did", "explain", "compare", "summarize", "summarise",
  "tell", "write", "draft", "translate", "help",
]);
/** Short, but conversational: never a web search. */
const CHAT_WORDS = new Set([
  "hi", "hello", "hey", "yo", "sup", "howdy", "morning", "evening", "thanks", "thank", "thx", "ty",
  "ok", "okay", "cool", "nice", "great", "yes", "no", "yep", "nope", "bye", "goodbye", "continue",
  "go", "again", "more", "why", "what", "sorry", "please",
]);
const DEV_VERBS = new Set([
  "fix", "refactor", "implement", "run", "build", "test", "deploy", "debug", "install", "migrate",
  "rename", "add", "remove", "update", "upgrade", "lint", "format", "commit", "review", "scaffold",
  "bump", "patch", "optimize", "optimise", "setup",
]);
const TLD = /\.(com|org|net|io|dev|app|ai|co|gov|edu|de|uk|fr|ru|ua|lv|me|tv|gg|xyz)(\/|$)/;
const PATH = /(^|\s)(\.{0,2}\/[\w./-]+|[\w-]+\/[\w./-]+|[\w-]+\.(rs|ts|tsx|js|py|go|java|toml|json|ya?ml|md|sh|c|cpp|h|css|html|sql))(\s|$|[:,.])/;

export interface Signals {
  words: string[];
  url: boolean;
  path: boolean;
  code: boolean;
  dev: boolean;
  question: boolean;
  mention: boolean;
  chatty: boolean;
}

export function signals(text: string): Signals {
  const t = text.trim();
  const lower = t.toLowerCase();
  const words = lower.split(/\s+/).filter(Boolean);
  const first = (words[0] ?? "").replace(/^@[\w-]+$/, "") || (words[1] ?? "");
  return {
    words,
    url: /^https?:\/\//.test(lower) || (words.length === 1 && TLD.test(lower)),
    path: PATH.test(t),
    code: t.includes("```") || /[{};]\s*$/.test(t) || /\b(fn|def|class|const|let)\s+\w+/.test(t),
    dev: DEV_VERBS.has(first),
    question: t.endsWith("?") || QUESTION_WORDS.has(first),
    mention: /^@[\w-]+/.test(t),
    chatty: CHAT_WORDS.has(first.replace(/[^\p{L}]/gu, "")),
  };
}

export function ruleMode(text: string, agenticAllowed: boolean): Mode {
  const s = signals(text);
  if (s.words.length === 0) return "ask";
  if (s.url) return "search";
  if (agenticAllowed && (s.path || s.code || (s.dev && s.words.length >= 3))) return "agentic";
  if (s.question || s.mention || s.chatty) return "ask";
  if (s.words.length <= 2) return "search";
  return "ask";
}
