// @vitest-environment happy-dom
// Answering a card from the keyboard: its controls are disabled while it
// sends and gone once it is answered, so focus moves to the card itself
// (named by its question or title) instead of dropping to the top of the
// page.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, test, vi } from "vitest";
import type { ChoicesSpec, FormSpec, FormValue } from "../types";
import type { InputState } from "./blocks";
import Choices from "./Choices";
import Form from "./Form";
import type { InputProps } from "./Input";

vi.mock("../api", () => ({ api: { prompt: vi.fn(async () => ({})) } }));

const colours: ChoicesSpec = {
  question: "Which colour?",
  options: [
    { id: "blue", label: "Blue" },
    { id: "red", label: "Red" },
  ],
  multi: false,
  other: false,
};
const note: FormSpec = { title: "Trip", fields: [{ id: "notes", label: "Notes", type: "text", required: false }] };
const props = (state: InputState): InputProps => ({ sessionId: "s", messageId: "m", blockId: "b", info: { state, notice: [] } });

let host: HTMLElement | null = null;
let root: Root | null = null;
afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
});

async function mount(card: (state: InputState) => React.ReactElement) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(card({ kind: "open" })));
  // The answer arrives as the next user message; the log closes the card.
  const answered = (values: Record<string, FormValue>) => act(async () => root!.render(card({ kind: "answered", values })));
  return { el: host, answered };
}

describe("focus after answering", () => {
  test("a choice answered from the keyboard leaves focus on its question", async () => {
    const { el, answered } = await mount((s) => <Choices spec={colours} input={props(s)} />);
    const option = el.querySelector<HTMLButtonElement>(".ui-option")!;
    option.focus();
    await act(async () => option.click());
    const frame = el.querySelector(".ui-choices")!;
    expect(document.activeElement).toBe(frame);
    await answered({ selected: ["blue"] });
    expect(document.activeElement).toBe(frame);
    expect(frame.getAttribute("role")).toBe("group");
    expect(document.getElementById(frame.getAttribute("aria-labelledby")!)?.textContent).toBe("Which colour?");
  });

  test("a form sent from the keyboard leaves focus on the form once Send is gone", async () => {
    const { el, answered } = await mount((s) => <Form spec={note} input={props(s)} />);
    const send = el.querySelector<HTMLButtonElement>(".ui-send")!;
    send.focus();
    await act(async () => send.click());
    await answered({});
    expect(el.querySelector(".ui-send")).toBeNull();
    const frame = el.querySelector(".ui-form")!;
    expect(document.activeElement).toBe(frame);
    expect(document.getElementById(frame.getAttribute("aria-labelledby")!)?.textContent).toBe("Trip");
  });

  test("a click with focus elsewhere does not move it", async () => {
    const { el } = await mount((s) => <Choices spec={colours} input={props(s)} />);
    const other = document.createElement("input");
    document.body.append(other);
    other.focus();
    await act(async () => el.querySelector<HTMLButtonElement>(".ui-option")!.click());
    expect(document.activeElement).toBe(other);
    other.remove();
  });
});
