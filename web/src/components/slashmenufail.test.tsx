// @vitest-environment happy-dom
// A `/` menu chunk that fails to load must not replace the chat screen: no
// menu, and the box keeps working (Enter sends).
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, test, vi } from "vitest";

vi.mock("./SlashMenu", () => {
  throw new Error("chunk failed to load");
});
vi.mock("../api", async (orig) => ({ ...(await orig<typeof import("../api")>()), call: vi.fn() }));
const { Composer } = await import("./Composer");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

test("a failed menu chunk leaves the composer working", async () => {
  const sent: string[] = [];
  const errors: unknown[] = [];
  const onError = (e: ErrorEvent) => errors.push(e.error);
  window.addEventListener("error", onError);
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host, { onUncaughtError: (e) => errors.push(e), onCaughtError: (e) => errors.push(e) });
  await act(async () =>
    root.render(
      <Composer
        where="session"
        agents={[]}
        models={[]}
        workers={[]}
        agentId=""
        onAgentChange={() => {}}
        agenticBlocked={null}
        onSubmit={async (s) => void sent.push(s.text)}
        compact={async () => {}}
        chatId="s1"
      />,
    ),
  );
  const ta = host.querySelector("textarea")!;
  const type = async (v: string) => {
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(ta, v);
      ta.setSelectionRange(v.length, v.length);
      ta.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () => void (await new Promise((r) => setTimeout(r, 20))));
  };
  await type("/");
  expect(host.querySelector("textarea")).not.toBeNull();
  expect(host.querySelector(".slash-menu")).toBeNull();
  await type("/dep");
  await act(async () => void ta.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true })));
  expect(sent).toEqual(["/dep"]);
  expect(errors).toEqual([]);
  window.removeEventListener("error", onError);
  act(() => root.unmount());
});
