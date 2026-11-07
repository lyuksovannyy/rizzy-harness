// Question and form cards, and the reply bubble, as static markup: the
// harness chrome, locked states, refused fields, answered values.
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, test, vi } from "vitest";
import { ReplyBubble } from "../components/ReplyBubble";
import type { InputInfo } from "../md/context";
import type { ChoicesSpec, FormSpec, UiBlock } from "../types";
import type { InputState } from "./blocks";
import Choices from "./Choices";
import Form from "./Form";
import type { InputProps } from "./Input";

const colours: ChoicesSpec = {
  question: "Which colour?",
  options: [
    { id: "blue", label: "Blue", description: "Calm" },
    { id: "red", label: "Red" },
  ],
  multi: false,
  other: false,
};
const trip: FormSpec = {
  title: "Trip",
  fields: [
    { id: "people", label: "Travellers", type: "number", required: true, min: 1, max: 12, step: 1, unit: "people" },
    { id: "dest", label: "Destination", type: "choice", required: false, multi: false, options: [{ id: "lis", label: "Lisbon" }, { id: "rom", label: "Rome" }] },
    { id: "flex", label: "Flexible", type: "toggle", default: true },
    { id: "key", label: "API key", type: "text", refused: true },
    { id: "notes", label: "Notes", type: "text", required: false, multiline: true, max_length: 200 },
  ],
};
const props = (state: InputState = { kind: "open" }, notice: string[] = []): InputProps => ({
  sessionId: "s",
  messageId: "m",
  blockId: "b",
  info: { state, notice } satisfies InputInfo,
});
const choices = (spec: ChoicesSpec, p = props()) => renderToStaticMarkup(<Choices spec={spec} input={p} />);
const form = (p = props()) => renderToStaticMarkup(<Form spec={trip} input={p} />);
const text = (h: string) => h.replace(/<[^>]+>/g, "").replaceAll("&#x27;", "'");

/** Render as if the user had typed `draft` into the card (props()'s ids). */
function withDraft(draft: unknown) {
  const kept = new Map([["harness.ui.m#b", JSON.stringify(draft)]]);
  vi.stubGlobal("sessionStorage", {
    getItem: (k: string) => kept.get(k) ?? null,
    setItem: () => {},
    removeItem: () => {},
  });
}
afterEach(() => void vi.unstubAllGlobals());

describe("harness chrome", () => {
  test("the fixed footer, and where the agent had been reading", () => {
    for (const h of [choices(colours), form()]) {
      expect(text(h)).toContain("Sends your answers as a message. Never type passwords or keys here.");
      expect(h).not.toContain("ui-notice");
    }
    const h = choices(colours, props({ kind: "open" }, ["a.example", "b.example", "c.example", "d.example"]));
    expect(text(h)).toContain("Asked after reading: a.example, b.example, c.example and 1 more. Check it before you answer.");
  });

  test("the Send label is the harness's", () => {
    for (const h of [form(), choices({ ...colours, multi: true, min: 1, max: 2 })]) {
      expect(h.match(/<button[^>]*class="primary ui-send"[^>]*>([^<]*)</)![1]).toBe("Send");
    }
  });

  test("a locked card says why and disables everything", () => {
    for (const [state, why] of [
      [{ kind: "not_answered" }, "Not answered: the conversation went on without it."],
      [{ kind: "waiting" }, "Waiting for the current turn to finish."],
      [{ kind: "choose_variant" }, "Continue with this answer to reply here."],
      [{ kind: "sub_task" }, "A sub-task's questions are for the agent that started it."],
    ] as [InputState, string][]) {
      const h = choices(colours, props(state));
      expect(text(h)).toContain(why);
      expect(h.match(/<button[^>]*>/g)!.every((b) => b.includes("disabled"))).toBe(true);
      expect(form(props(state)).match(/<(input|textarea|button)[^>]*>/g)!.every((b) => b.includes("disabled"))).toBe(true);
    }
  });

  test("a preview outside a conversation cannot send", () => {
    const h = choices(colours, { ...props(), sessionId: null });
    expect(h.match(/<button[^>]*>/g)!.every((b) => b.includes("disabled"))).toBe(true);
  });
});

describe("Choices", () => {
  test("a single choice is one button per option, labels as text", () => {
    const h = choices(colours);
    expect(h.match(/class="ui-option"/g)).toHaveLength(2);
    expect(text(h)).toContain("BlueCalm");
    expect(h).not.toContain("ui-send");
  });

  test("Other and multiple choices get a Send button and a hint", () => {
    expect(choices({ ...colours, other: true })).toContain('placeholder="Other…"');
    const h = choices({ ...colours, multi: true, min: 2, max: 2 });
    expect(h.match(/type="checkbox"/g)).toHaveLength(2);
    expect(text(h)).toContain("Pick 2");
    expect(h).toMatch(/class="primary ui-send" disabled/);
  });

  test("a typed Other is one of the picks that Pick N allows", () => {
    const spec: ChoicesSpec = { ...colours, options: [...colours.options, { id: "green", label: "Green" }], multi: true, other: true, min: 2, max: 2 };
    const boxes = (h: string) => h.match(/<input type="checkbox"[^>]*>/g)!.map((b) => b.includes("disabled"));
    const other = (h: string) => h.match(/<input type="text"[^>]*>/)![0];
    // Two boxes ticked: Other cannot be started.
    withDraft({ selected: ["blue", "red"], other: "" });
    let h = choices(spec);
    expect(boxes(h)).toEqual([false, false, true]);
    expect(other(h)).toContain("disabled");
    expect(h).not.toMatch(/class="primary ui-send" disabled/);
    // One box and Other: the last box waits, Other can still be cleared.
    withDraft({ selected: ["blue"], other: "Teal" });
    h = choices(spec);
    expect(boxes(h)).toEqual([false, true, true]);
    expect(other(h)).not.toContain("disabled");
    expect(h).not.toMatch(/class="primary ui-send" disabled/);
    // A draft from before the limit counted Other: too many, Send is off.
    withDraft({ selected: ["blue", "red"], other: "Teal" });
    h = choices(spec);
    expect(other(h)).not.toContain("disabled");
    expect(h).toMatch(/class="primary ui-send" disabled/);
  });

  test("an answered question marks what was picked", () => {
    const h = choices({ ...colours, other: true }, props({ kind: "answered", values: { selected: [], other: "Teal" } }));
    expect(text(h)).toContain("Answered");
    expect(text(h)).toContain("Other: Teal");
    const picked = choices(colours, props({ kind: "answered", values: { selected: ["red"] } }));
    expect(picked).toMatch(/class="ui-option ui-option-chosen" aria-pressed="true" disabled[^>]*><span class="ui-option-text"><span class="ui-option-label">Red/);
  });
});

describe("Form", () => {
  test("fields as inputs; a refused one only as the warning", () => {
    const h = form();
    expect(h).toContain('type="number"');
    expect(h).toContain('type="range"');
    expect(h.match(/type="radio"/g)).toHaveLength(2);
    expect(h).toContain('role="switch" checked=""');
    expect(h).toContain("<textarea");
    expect(text(h)).toContain("API key: This asks for a secret.");
    expect(h).not.toContain('type="password"');
    // Every input belongs to a shown field: none for the key.
    expect(h.match(/<(input|textarea|select)/g)).toHaveLength(6);
    // Travellers is required and empty: Send is off, with no error yet.
    expect(h).toMatch(/class="primary ui-send" disabled/);
    expect(h).not.toContain("ui-field-error");
  });

  test("answers that are each fine but too large together say so", () => {
    const long: FormSpec = {
      title: "Essay",
      fields: Array.from({ length: 9 }, (_, i) => ({ id: `p${i}`, label: `Part ${i}`, type: "text" as const, multiline: true, max_length: 4000 })),
    };
    const draft = (n: number) => Object.fromEntries(long.fields.map((f) => [f.id, "x".repeat(n)]));
    withDraft(draft(3500));
    let h = renderToStaticMarkup(<Form spec={long} input={props()} />);
    expect(h).not.toContain("ui-field-error");
    expect(h).not.toMatch(/class="primary ui-send" disabled/);
    withDraft(draft(3700));
    h = renderToStaticMarkup(<Form spec={long} input={props()} />);
    expect(text(h)).toContain("The answer is larger than 32 KiB");
    expect(h).toMatch(/class="primary ui-send" disabled/);
  });

  test("an answered form shows its values read-only", () => {
    const h = form(props({ kind: "answered", values: { people: 3, dest: "lis", flex: false } }));
    expect(h).not.toMatch(/<(input|textarea|select|button)/);
    expect(text(h)).toContain("Travellers3 people");
    expect(text(h)).toContain("DestinationLisbon");
    expect(text(h)).toContain("FlexibleNo");
  });
});

describe("ReplyBubble", () => {
  const block: UiBlock = { ordinal: 0, hash: "00000000", type: "form", id: "trip", spec: trip };
  const reply = { message_id: "m", block_id: "trip", values: { people: 2, notes: "Aisle\nplease" } };

  test("the answer in the block's words, then the note", () => {
    const h = renderToStaticMarkup(
      <ReplyBubble text={'Form answer · trip\n- people: 2\n- notes: "Aisle\\nplease"\n\nThanks'} reply={reply} block={block} />,
    );
    expect(h).toContain('class="bubble reply"');
    expect(text(h)).toContain("Answered “Trip”");
    expect(text(h)).toContain("Travellers: 2 people");
    expect(text(h)).toContain("Notes: Aisle\nplease");
    expect(h).toContain('<p class="reply-note">Thanks</p>');
  });

  test("without its block, the message text", () => {
    const h = renderToStaticMarkup(<ReplyBubble text="Form answer · trip" reply={reply} />);
    expect(h).toBe('<div class="bubble">Form answer · trip</div>');
  });
});
