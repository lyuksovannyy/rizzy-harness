// The fold guard's table (fold.ts, GUARD) against the event schema as serde
// reads it (__fixtures__/schema.json, written by
// crates/core/tests/browser_schema.rs, which fails when it is stale). A kind,
// a field or an enum value added in Rust fails here until the table knows
// it; a field the fold reads cannot drop out of the table unnoticed.
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { GUARD } from "./fold";

interface RustField {
  path: string;
  type: "string" | "number" | "boolean" | "array" | "object" | "map" | "any";
  required: boolean;
  restricted?: boolean;
  values?: string[];
}
const SCHEMA = JSON.parse(readFileSync(new URL("./__fixtures__/schema.json", import.meta.url), "utf8")).kinds as Record<string, RustField[]>;

/** Optional fields the guard leaves out, with why. Nothing below one is checked. */
const NOT_READ: Record<string, string> = {
  "turn_started.environment": "the node's own line for the model; never shown",
  "turn_started.retry.error": "for the model's retry opening; the chat shows the failed turn's own error",
  "user_message.attachments[].truncated": "the chip shows the name only",
  "user_message.files[].text": "the file's content went to the model; the chip shows the path",
  "user_message.files[].lines": "not shown",
  "user_message.files[].shown": "not shown",
  "user_message.files[].bytes": "not shown",
  "user_message.files[].note": "not shown",
  "user_message.files[].seen": "the node's read-first guard",
  "assistant_message.tool_calls": "each call has its own tool_call_started",
  "assistant_message.thinking.blocks[].signature": "removed by for_browser",
  "assistant_message.thinking.blocks[].data": "removed by for_browser",
  "assistant_message.thinking.prefix": "removed by for_browser",
  "assistant_message.thinking.unsent": "for the provider, not shown",
  "tool_call_finished.truncated": "the output says so itself",
  "tool_call_finished.note": "for the model",
  "tool_call_finished.seen": "the node's read-first guard",
  "tool_call_finished.image": "the image row reads Media, not the event",
  "checkpoint.excluded": "the UI ignores it for now (PLAN.md)",
  "checkpoint.excluded_more": "the UI ignores it for now (PLAN.md)",
  "error.turn_id": "an error shows where it is in the log",
  "context_shortened.folded.todo": "the task list shows from todo_updated",
  "background_ended.signal": "the end shows its reason and exit code",
  "background_ended.tail_cut": "not shown",
  "background_ended.delivered": "decides callbacks on the node",
  "background_ended.child.ok": "the card's state comes from the reason",
  "chat_allow_added.key": "the node's match key",
  "context_sent.sections": "what the model was sent; never shown",
  "images_withheld.reason": "not shown",
  "thinking_withheld.reason": "not shown",
};
/** Rust structs the guard keeps as sent (`o`), with why. */
const OPAQUE: Record<string, string> = {
  "artifact_staged.meta": "display only; the artifact rows read what they show with their own checks",
};

type Flat = { type: RustField["type"] | "opaque"; required: boolean; values?: string[] };
/** The table as paths, in the schema's notation (`a[]` an element, `a{}` a map's value). */
function flatten(spec: string, prefix: string, out: Map<string, Flat>) {
  for (const w of spec.split(" ").filter(Boolean)) {
    const opt = w[0] === "?";
    const [name, type = "s"] = (opt ? w.slice(1) : w).split(":");
    leaf(type, prefix ? `${prefix}.${name}` : name!, !opt, out);
  }
}
function leaf(t: string, path: string, required: boolean, out: Map<string, Flat>) {
  const rest = t.slice(1);
  const put = (type: Flat["type"], values?: string[]) => out.set(path, { type, required, values });
  switch (t[0]) {
    case "s":
      return put("string");
    case "n":
      return put("number");
    case "b":
      return put("boolean");
    case "o":
      return put("opaque");
    case "j":
      return put("any");
    case "=":
      return put("string", GUARD.ENUMS[rest]!.split(" "));
    case "[":
      put("array");
      return leaf(rest, `${path}[]`, true, out);
    case "{":
      put("map");
      return leaf(rest, `${path}{}`, true, out);
    default:
      put("object");
      return flatten(GUARD.STRUCTS[rest]!, path, out);
  }
}
/** `a.b` → `a`, `a[]` → `a`, `a{}` → `a`, a top-level field → "". */
const parent = (p: string) => (/(\[\]|\{\}|\.[^.[{]+)$/.test(p) ? p.replace(/(\[\]|\{\}|\.[^.[{]+)$/, "") : "");

describe("the fold guard's table is the Rust schema", () => {
  it("has every kind serde knows, and no other", () => {
    expect(Object.keys(GUARD.KINDS).sort()).toEqual(Object.keys(SCHEMA).sort());
  });

  for (const [kind, fields] of Object.entries(SCHEMA)) {
    it(`${kind}: every field the table lists is serde's, every required one is listed, every optional one is listed or named`, () => {
      const table = new Map<string, Flat>();
      flatten(GUARD.KINDS[kind]!, "", table);
      const rust = new Map(fields.map((f) => [f.path, f]));
      const problems: string[] = [];
      for (const [path, t] of table) {
        const r = rust.get(path);
        if (!r) {
          problems.push(`${path}: not in Rust`);
          continue;
        }
        if (t.required !== r.required) problems.push(`${path}: required ${t.required}, Rust ${r.required}`);
        const want = t.type === "opaque" ? "object" : t.type;
        if (want !== r.type) problems.push(`${path}: ${t.type}, Rust ${r.type}`);
        if (t.type === "opaque" && !OPAQUE[`${kind}.${path}`]) problems.push(`${path}: kept as sent, not in OPAQUE`);
        if (r.values && JSON.stringify([...(t.values ?? [])].sort()) !== JSON.stringify([...r.values].sort()))
          problems.push(`${path}: values ${t.values ?? "any string"}, Rust ${r.values}`);
      }
      for (const r of fields) {
        if (table.has(r.path)) continue;
        const up = parent(r.path);
        const p = up === "" ? { type: "object" } : table.get(up);
        // Below a field the table leaves out, keeps as sent or names as not read.
        if (!p || p.type === "opaque" || p.type === "any") continue;
        if (r.required) problems.push(`${r.path}: serde requires it, the table does not list it`);
        else if (!NOT_READ[`${kind}.${r.path}`]) problems.push(`${r.path}: optional, neither in the table nor in NOT_READ`);
      }
      for (const k of [...Object.keys(NOT_READ), ...Object.keys(OPAQUE)].filter((k) => k.startsWith(`${kind}.`))) {
        if (!rust.has(k.slice(kind.length + 1))) problems.push(`${k}: named here, not in Rust`);
        if (NOT_READ[k] && table.has(k.slice(kind.length + 1))) problems.push(`${k}: in NOT_READ and in the table`);
      }
      expect(problems).toEqual([]);
    });
  }
});
