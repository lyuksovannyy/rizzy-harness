// English strings for rendering messages. They live here, in the lazy
// Conversation chunk, rather than in lib/i18n.ts so that the new-tab bundle
// does not grow; they move with the catalog when a second language arrives.
import { fill } from "../lib/i18n";

const en = {
  copy: "Copy",
  copied: "Copied",
  copyCsv: "Copy CSV",
  copyMessage: "Copy",
  copyMessageHint: "Copy this message as Markdown",
  sortBy: "Sort by {col}",
  showAll: "Show all {n} lines",
  showLess: "Show fewer lines",
  diagram: "diagram source",
  open: "Open",
  cancel: "Cancel",
  linkTitle: "Check this link",
  linkCheck: "The assistant wrote this link. Check the full address before you open it.",
  image: "Image",
  imageHint: "Images in replies are not loaded",
  embedded: "embedded image (not shown)",
  "callout.note": "Note",
  "callout.tip": "Tip",
  "callout.important": "Important",
  "callout.warning": "Warning",
  "callout.caution": "Caution",
  citeMissing: "No such source",
  citeLabel: "Source {label}: {title} ({host})",
  sources: "Sources ({n})",
  "web.wantsSearch": "Wants to search the web for “{q}”",
  "web.searching": "Searching the web for “{q}”",
  "web.searched": "Searched “{q}” · {n} results",
  "web.searched1": "Searched “{q}” · 1 result",
  "web.searchFailed": "Could not search the web for “{q}”",
  "web.wantsRead": "Wants to read {host}",
  "web.reading": "Reading {host}…",
  "web.read": "Read {host}",
  "web.readFailed": "Could not read {host}",
  "web.page": "a page",
  "web.address": "Address",
  "web.query": "Query",
  "web.output": "Show what the model read",
  "web.researching": "Researching · {parts}",
  "web.researched": "Researched · {parts}",
  "web.searches": "{n} searches",
  "web.searches1": "1 search",
  "web.pages": "{n} pages",
  "web.pages1": "1 page",
} as const;

export type RichKey = keyof typeof en;

export function rt(key: RichKey, vars?: Record<string, string>): string {
  return fill(en[key], vars);
}
