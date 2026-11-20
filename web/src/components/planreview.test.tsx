// @vitest-environment happy-dom
// The plan-review pane (X.7): notes on blocks, the one feedback message, the
// carry-out guard, a changed plan, hostile plan text, notes kept per chat.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import type { Plan } from "../lib/planReview";
import PlanReview from "./PlanReview";

// The code blocks' highlighter is a Web Worker: not in happy-dom.
vi.mock("../md/highlighter", async (orig) => ({ ...(await orig<object>()), highlightOffThread: async () => null }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const P1: Plan = { id: "m1", text: "# Plan\n\nIntroduce a cache.\n\n- Add cache\n  - Use an LRU of 128 entries\n\n```sh\ncargo test\n```" };
const P2: Plan = { id: "m2", text: "# Plan\n\nIntroduce a cache.\n\n- Add cache\n  - Use an LRU of 256 entries\n\n```sh\ncargo test\n```\n\nThen measure." };

interface Rig {
  host: HTMLElement;
  root: Root;
  onSend: ReturnType<typeof vi.fn>;
  onCarry: ReturnType<typeof vi.fn>;
  onClose: ReturnType<typeof vi.fn>;
  show: (o?: { plans?: Plan[]; running?: boolean; session?: string }) => void;
}
let chats = 0;
const open: Root[] = [];
afterEach(() => {
  for (const r of open.splice(0)) act(() => r.unmount());
  document.body.innerHTML = "";
  vi.restoreAllMocks();
});

function mount(o: { plans?: Plan[]; running?: boolean; session?: string } = {}): Rig {
  o = { ...o, session: o.session ?? `chat-${++chats}` };
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  open.push(root);
  const onSend = vi.fn(async () => {});
  const onCarry = vi.fn(async () => {});
  const onClose = vi.fn();
  const show = (n: { plans?: Plan[]; running?: boolean; session?: string } = {}) =>
    act(() =>
      root.render(
        <PlanReview
          sessionId={n.session ?? o.session!}
          plans={n.plans ?? o.plans ?? [P1]}
          running={n.running ?? o.running ?? false}
          max="auto"
          onCarry={onCarry}
          onSend={onSend}
          onClose={onClose}
        />,
      ),
    );
  show();
  return { host, root, onSend, onCarry, onClose, show };
}

const btn = (host: HTMLElement, name: string | RegExp) =>
  [...host.querySelectorAll<HTMLButtonElement>("button")].find((b) => (typeof name === "string" ? b.textContent === name : name.test(b.textContent ?? "")))!;
const blockEl = (host: HTMLElement, text: string) => [...host.querySelectorAll<HTMLElement>("li.pr-block")].find((l) => l.textContent?.includes(text))!;

function type(el: HTMLTextAreaElement, value: string) {
  const set = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!;
  act(() => {
    set.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
const click = (el: HTMLElement) => act(() => el.click());

/** Add a note to the block holding `where`; `select` fakes a selection inside it. */
function addNote(r: Rig, where: string, kind: "Comment" | "Replace" | "Remove", text: string, select?: string) {
  const li = blockEl(r.host, where);
  if (select)
    vi.spyOn(window, "getSelection").mockReturnValue({ isCollapsed: false, rangeCount: 1, getRangeAt: () => ({ commonAncestorContainer: li }), toString: () => select } as unknown as Selection);
  click(li.querySelector<HTMLButtonElement>(".pr-tools button")!);
  vi.restoreAllMocks();
  const radio = [...r.host.querySelectorAll("label")].find((l) => l.textContent?.trim() === kind)!.querySelector("input")!;
  click(radio);
  if (kind !== "Remove" || text) type(r.host.querySelector<HTMLTextAreaElement>(".pr-form textarea")!, text);
  click(btn(r.host, "Save note"));
}

test("the plan is drawn as blocks, one tab stop, focus moves into the pane", () => {
  const r = mount();
  expect(r.host.querySelectorAll("li.pr-block").length).toBe(5);
  expect(document.activeElement?.id).toBe("plan-review-title");
  expect(r.host.querySelectorAll("li.pr-block[tabindex='0']").length).toBe(1);
});

test("three kinds of note, listed beside the plan, editable and deletable", () => {
  const r = mount();
  addNote(r, "Introduce a cache", "Comment", "Say why.");
  addNote(r, "Use an LRU", "Replace", "a 256-entry LRU", "LRU of 128 entries");
  addNote(r, "cargo test", "Remove", "");
  const notes = r.host.querySelector(".pr-notes")!;
  expect(notes.querySelectorAll("li.pr-note").length).toBe(3);
  expect([...notes.querySelectorAll(".pr-kind")].map((k) => k.textContent)).toEqual(["Comment", "Replace", "Remove"]);
  // The selection, not the whole item, is what the replacement quotes.
  expect(notes.querySelector(".pr-note-replace .pr-quote")!.textContent).toBe("LRU of 128 entries");
  // Edit the comment.
  click(btn(notes as HTMLElement, "Edit"));
  type(r.host.querySelector<HTMLTextAreaElement>(".pr-form textarea")!, "Say why, with numbers.");
  click(btn(r.host, "Save note"));
  expect(notes.textContent).toContain("Say why, with numbers.");
  expect(notes.querySelectorAll("li.pr-note").length).toBe(3);
  // Delete one.
  click(btn(notes as HTMLElement, "Delete"));
  expect(notes.querySelectorAll("li.pr-note").length).toBe(2);
});

test("a comment without text is not saved", () => {
  const r = mount();
  addNote(r, "Introduce a cache", "Comment", "  ");
  expect(r.host.querySelectorAll("li.pr-note").length).toBe(0);
  expect(r.host.querySelector(".pr-form")).not.toBeNull();
});

test("Send feedback shows the message first, sends ONE message through onSend and nothing else", async () => {
  const r = mount();
  addNote(r, "Introduce a cache", "Comment", "Say why.");
  expect(btn(r.host, /^Send feedback/).disabled).toBe(false);
  click(btn(r.host, /^Send feedback/));
  const box = r.host.querySelector<HTMLTextAreaElement>("textarea.pr-message")!;
  expect(r.onSend).not.toHaveBeenCalled();
  expect(box.value).toBe(
    "Feedback on your plan. Revise it to address each note below, then end your turn with the full revised plan. Do not carry anything out yet.\n\n**Note 1 of 1 (comment)**\n\n> Introduce a cache.\n\nComment: Say why.",
  );
  // The person may edit it.
  type(box, `${box.value}\n\nAlso keep it short.`);
  await act(async () => btn(r.host, "Send").click());
  expect(r.onSend).toHaveBeenCalledTimes(1);
  expect(r.onSend.mock.calls[0]).toEqual([expect.stringContaining("Also keep it short.")]);
  expect(r.onCarry).not.toHaveBeenCalled();
  expect(r.onClose).toHaveBeenCalled();
});

test("a failed send says why, keeps the notes and the pane", async () => {
  const r = mount();
  r.onSend.mockRejectedValueOnce(new Error("offline"));
  addNote(r, "Introduce a cache", "Comment", "Say why.");
  click(btn(r.host, /^Send feedback/));
  await act(async () => btn(r.host, "Send").click());
  expect(r.host.querySelector("[role=alert]")!.textContent).toContain("offline");
  expect(r.onClose).not.toHaveBeenCalled();
  click(btn(r.host, "Back"));
  expect(r.host.querySelectorAll("li.pr-note").length).toBe(1);
});

test("Carry out is off while notes exist, on after discarding them, and sends the bar's own message", async () => {
  const r = mount();
  const carry = () => [...r.host.querySelectorAll<HTMLButtonElement>(".pr-carry button")];
  expect(carry().map((b) => b.disabled)).toEqual([false, false, false]);
  addNote(r, "Introduce a cache", "Comment", "Say why.");
  expect(carry().every((b) => b.disabled)).toBe(true);
  click(btn(r.host, "Discard my notes"));
  expect(r.host.querySelectorAll("li.pr-note").length).toBe(0);
  await act(async () => carry()[0]!.click());
  expect(r.onCarry).toHaveBeenCalledWith("auto", "Carry out the plan.");
});

test("a newer plan while the pane is open: said, Carry out off until it was looked at; the old notes are orphaned, not moved", () => {
  const r = mount({ plans: [P1] });
  addNote(r, "Use an LRU", "Comment", "Too small.");
  click(btn(r.host, "Discard my notes")); // so only the guard blocks Carry out
  addNote(r, "Use an LRU", "Comment", "Too small.");
  r.show({ plans: [P1, P2] });
  const carry = () => [...r.host.querySelectorAll<HTMLButtonElement>(".pr-carry button")];
  expect(r.host.querySelector(".pr-banner")!.textContent).toContain("newer plan");
  expect(carry().every((b) => b.disabled)).toBe(true);
  expect(btn(r.host, /^Send feedback/).disabled).toBe(true);
  // Still the old plan on screen.
  expect(r.host.textContent).toContain("128 entries");
  click(btn(r.host, "Show the newer plan"));
  expect(r.host.textContent).toContain("256 entries");
  expect(r.host.querySelector(".pr-banner")).toBeNull();
  // The note stays with the plan it was written for.
  expect(r.host.textContent).toContain("Notes on earlier plans");
  expect(r.host.querySelector(".pr-notes")!.textContent).toContain("Written for an earlier plan");
  expect(blockEl(r.host, "256 entries").querySelector(".pr-note")).toBeNull();
  expect(r.host.querySelector(".pr-notes h3")!.textContent).toBe("Your notes (0)");
  // Nothing blocks Carry out on the plan now shown.
  expect(carry().every((b) => b.disabled)).toBe(false);
});

test("a revised plan is shown against the one before it", () => {
  const r = mount({ plans: [P1, P2] });
  expect(r.host.querySelector("[role=status]")!.textContent).toBe("Against the plan before it: 1 added, 1 changed, 0 removed.");
  const changed = blockEl(r.host, "256 entries");
  expect(changed.querySelector(".pr-flag")!.textContent).toBe("Changed");
  expect(changed.querySelector(".pr-was")!.textContent).toContain("128 entries");
  expect(blockEl(r.host, "Then measure").querySelector(".pr-flag")!.textContent).toBe("Added");
});

test("while the agent works nothing is sent or carried out", () => {
  const r = mount({ running: true });
  addNote(r, "Introduce a cache", "Comment", "Say why.");
  expect(btn(r.host, /^Send feedback/).disabled).toBe(true);
  expect([...r.host.querySelectorAll<HTMLButtonElement>(".pr-carry button")].every((b) => b.disabled)).toBe(true);
});

test("hostile plan text is drawn by the chat's renderer: no HTML, no remote image, no javascript: link", () => {
  const evil = '<img src=x onerror="alert(1)">\n\n[click](javascript:alert(1))\n\n![pixel](https://evil.example/p.png)\n\n<script>alert(2)</script>';
  const r = mount({ plans: [{ id: "h", text: evil }] });
  expect(r.host.querySelector("img")).toBeNull();
  expect(r.host.querySelector("script")).toBeNull();
  expect(r.host.querySelector("[onerror]")).toBeNull();
  // The remote image is a chip that names its host, not an image.
  expect(r.host.querySelector(".img-chip")!.textContent).toContain("evil.example");
  expect(r.host.querySelector("a[href^='javascript']")).toBeNull();
  for (const a of r.host.querySelectorAll("a")) expect(a.getAttribute("href") ?? "").not.toMatch(/^javascript/i);
  // The raw text is still there, as text.
  expect(r.host.textContent).toContain("<img src=x");
});

test("closing and reopening keeps the notes; another chat starts empty; another plan keeps them off its blocks", () => {
  const r = mount({ session: "chat-keep" });
  addNote(r, "Introduce a cache", "Comment", "Say why.");
  act(() => r.root.unmount());
  const again = mount({ session: "chat-keep" });
  expect(again.host.querySelector(".pr-notes")!.textContent).toContain("Say why.");
  expect(again.host.querySelector(".pr-notes h3")!.textContent).toBe("Your notes (1)");
  const other = mount({ session: "chat-other" });
  expect(other.host.querySelectorAll("li.pr-note").length).toBe(0);
  const plan2 = mount({ session: "chat-keep", plans: [P1, P2] });
  expect(plan2.host.querySelector(".pr-notes h3")!.textContent).toBe("Your notes (0)");
  expect(plan2.host.querySelector(".pr-notes")!.textContent).toContain("Written for an earlier plan");
});

test("keyboard: arrows move between blocks, N opens a note on the focused one, Escape closes the form and then the pane", () => {
  const r = mount();
  const items = [...r.host.querySelectorAll<HTMLElement>("li.pr-block")];
  act(() => items[0]!.focus());
  act(() => void items[0]!.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true })));
  expect(document.activeElement).toBe(items[1]);
  act(() => void items[1]!.dispatchEvent(new KeyboardEvent("keydown", { key: "n", bubbles: true })));
  const form = r.host.querySelector(".pr-form")!;
  expect(items[1]!.contains(form)).toBe(true);
  expect(document.activeElement).toBe(form.querySelector("textarea"));
  const dialog = r.host.querySelector("[role=dialog]")!;
  act(() => void document.activeElement!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
  expect(r.host.querySelector(".pr-form")).toBeNull();
  expect(r.onClose).not.toHaveBeenCalled();
  act(() => void dialog.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
  expect(r.onClose).toHaveBeenCalledTimes(1);
  expect(dialog.getAttribute("aria-modal")).toBe("true");
});

test("no plan yet: says so", () => {
  const r = mount({ plans: [] });
  expect(r.host.textContent).toContain("has not proposed a plan");
});
