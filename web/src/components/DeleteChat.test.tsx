// @vitest-environment happy-dom
// The delete dialog: a delete the server refused stays on the chat and says
// why; only a delete that worked goes home.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, expect, test, vi } from "vitest";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const deleteSession = vi.fn();
vi.mock("../apiPages", () => ({ pageApi: { deleteSession: (...a: unknown[]) => deleteSession(...a) } }));
vi.mock("../lib/media", () => ({ mediaApi: { list: () => Promise.resolve({ items: [], next: null }) } }));

import { DeleteChat } from "./DeleteChat";

beforeEach(() => {
  deleteSession.mockReset();
  location.hash = "#/s/abc";
});

async function open() {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const onClose = vi.fn();
  await act(async () => root.render(<DeleteChat sessionId="abc" workspaceId={null} incognito onClose={onClose} />));
  const press = async () => {
    const b = [...host.querySelectorAll("button")].find((x) => x.textContent === "Delete")!;
    await act(async () => b.click());
  };
  return { host, press };
}

test("a refused delete keeps the dialog and the chat, and shows the server's message", async () => {
  deleteSession.mockRejectedValue(new Error("the node that holds this chat is away"));
  const { host, press } = await open();
  await press();
  expect(host.textContent).toContain("the node that holds this chat is away");
  expect(location.hash).toBe("#/s/abc");
  expect(host.querySelector("dialog")).not.toBeNull();
  // Not stuck: it can be tried again.
  expect([...host.querySelectorAll("button")].find((x) => x.textContent === "Delete")!.disabled).toBe(false);
});

test("a delete that worked goes home", async () => {
  deleteSession.mockResolvedValue({});
  const { press } = await open();
  await press();
  expect(location.hash).toBe("#/");
});
