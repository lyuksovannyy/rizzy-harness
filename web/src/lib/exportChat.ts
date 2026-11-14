// A chat as a Markdown file, built in the browser from the fold the chat
// already shows: the person's messages, the answers, and each tool call as
// one short line (its output only when the person asks for it). Nothing is
// fetched for it, so nothing can be in the file that the view did not hold,
// and nothing here is rendered: the text goes into a downloaded file.
import { markedText } from "./callText";
import type { Item, SessionView } from "../types";

export interface ExportOptions {
  /** Each tool call's output under its line, in a code fence. */
  outputs: boolean;
  /** Names for agent ids (the header shows the same). */
  agentName?: (id: string | null | undefined) => string;
  /** When it was exported (the file's first line says so). */
  now?: Date;
}

type Tool = Extract<Item, { type: "tool" }>;
const str = (v: unknown) => (typeof v === "string" ? v : "");

/** One line, hidden characters marked as the tool card marks them. */
function oneLine(s: string, max = 120): string {
  const line = markedText(s.trim(), "cmd").replace(/\n/g, " ");
  return line.length > max ? `${line.slice(0, max - 1)}…` : line;
}

/** What a call works on, in a few words. */
export function toolSummary(it: Tool): string {
  const i = (it.input ?? {}) as Record<string, unknown>;
  switch (it.tool) {
    case "bash":
      return oneLine(str(i.command));
    case "read":
    case "write":
    case "edit":
      return oneLine(str(i.path));
    case "grep":
    case "glob":
      return oneLine(`${str(i.pattern)} ${str(i.path)}`);
    case "web_search":
      return oneLine(str(i.query));
    case "web_fetch":
      return oneLine(str(i.url));
    case "delegate":
      return oneLine(`@${str(i.agent)}: ${str(i.task)}`);
    default:
      return "";
  }
}

function status(it: Tool): string {
  if (it.approval?.decision === "deny") return "denied";
  if (!it.result) return "did not finish";
  const code = it.result.exit_code;
  return `${it.result.ok ? "ok" : "failed"}${code !== null && code !== undefined ? `, exit ${code}` : ""}`;
}

/** A fence longer than any run of backticks in `text`, so the text cannot end it. */
export function fence(text: string, lang = "text"): string {
  const longest = Math.max(0, ...[...text.matchAll(/`+/g)].map((m) => m[0].length));
  const f = "`".repeat(Math.max(3, longest + 1));
  return `${f}${lang}\n${text.replace(/\n$/, "")}\n${f}`;
}

/** `text` with a code fence it leaves open closed at its end, so a cut-off
 * message (a cancelled answer, pasted text) cannot swallow what follows. */
export function closeFences(text: string): string {
  let open: string | null = null;
  for (const line of text.split("\n")) {
    const m = /^ {0,3}(`{3,}|~{3,})/.exec(line);
    if (!m) continue;
    const f = m[1]!;
    if (open === null) open = f;
    // A closing fence: the same character, at least as long, nothing after.
    else if (f[0] === open[0] && f.length >= open.length && line.trim() === f) open = null;
  }
  return open === null ? text : `${text.replace(/\n$/, "")}\n${open}`;
}

/** A file name from the title: letters, digits, dashes; never empty. */
export function fileName(title: string): string {
  const base = title
    .normalize("NFKD")
    .replace(/[^\p{L}\p{N}]+/gu, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 60)
    .toLowerCase();
  return `${base || "chat"}.md`;
}

export function chatMarkdown(view: SessionView, title: string, opts: ExportOptions): string {
  const name = (id: string | null | undefined) => (opts.agentName ? opts.agentName(id) : (id ?? "")) || "Assistant";
  const out: string[] = [`# ${oneLine(title, 200) || "Chat"}`, ""];
  const meta = view.meta;
  const when = (opts.now ?? new Date()).toISOString().slice(0, 16).replace("T", " ");
  out.push(`Exported ${when} UTC${meta?.model ? ` · model ${oneLine(meta.model)}` : ""}`, "");
  for (const it of view.items) {
    switch (it.type) {
      case "user":
        out.push("", "## You", "", closeFences(it.text), "");
        if (it.attachments?.length) out.push(`Attached: ${it.attachments.map((a) => oneLine(a)).join(", ")}`, "");
        break;
      case "assistant": {
        const variant = it.variant?.model ? ` (${oneLine(it.variant.model)})` : "";
        out.push("", `## ${oneLine(name(it.author))}${variant}`, "", closeFences(it.text), "");
        break;
      }
      case "tool": {
        const what = toolSummary(it);
        out.push(`- Tool \`${oneLine(it.tool, 40)}\`${what ? `: ${what}` : ""} (${status(it)})`);
        const output = it.result ? it.result.output : it.live_output;
        if (opts.outputs && output) out.push("", fence(output), "");
        break;
      }
      case "summary":
        out.push("_Earlier messages were summarised for the model here._", "");
        break;
      case "error":
        out.push(`> Error: ${oneLine(it.message, 500)}`, "");
        break;
      case "turn_end":
        if (it.reason !== "completed") out.push(`_Turn ${it.reason}._`, "");
        break;
      case "fallback":
        out.push(`_${oneLine(it.model)} answered instead of ${oneLine(it.from)}._`, "");
        break;
      case "background_ended":
        out.push(`- Background command ${oneLine(it.bg_id)} ended (${it.reason}${it.exit_code !== null ? `, exit ${it.exit_code}` : ""})`);
        break;
      case "callback":
        out.push(`_The agent was resumed after ${it.bg_ids.map((b) => oneLine(b)).join(", ")} ended._`, "");
        break;
      default:
        break;
    }
  }
  return `${out.join("\n").replace(/\n{3,}/g, "\n\n").trimEnd()}\n`;
}

/** Hand the file to the browser as a download. */
export function download(text: string, name: string) {
  const url = URL.createObjectURL(new Blob([text], { type: "text/markdown;charset=utf-8" }));
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
