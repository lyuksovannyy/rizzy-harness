// @vitest-environment happy-dom
// "You may have asked this before" loads as its own chunk right after the
// home screen's first paint (it shows nothing until three words are typed).
// Here the real component sits behind the lazy import: typed text still finds
// earlier chats, and the home screen renders without them until then.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, test, vi } from "vitest";

const search = vi.fn((..._a: unknown[]) =>
  Promise.resolve({ results: [{ id: "s1", title: "Rebase notes", snippet: "how to rebase", preview: "", time_ms: Date.now() }] }),
);
vi.mock("../api", () => ({
  api: { features: () => Promise.resolve({ media: { vision_images: false } }), search: (...a: unknown[]) => search(...a) },
  call: () => Promise.resolve({ ok: true }),
  HttpError: class extends Error {},
}));
vi.mock("./Composer", async () => {
  const { useEffect } = await import("react");
  return {
    Composer: (p: { onTextChange: (t: string, m: string) => void }) => {
      const { onTextChange } = p;
      useEffect(() => onTextChange("how do I rebase", "ask"), [onTextChange]);
      return <div className="composer" />;
    },
  };
});
vi.mock("../bg/Backdrop", () => ({ default: () => null }));
const { Home } = await import("./Home");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

test("the lazy similar-chats list still finds earlier chats for typed text", async () => {
  vi.useFakeTimers();
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  act(() =>
    root.render(
      <Home
        username="alex"
        agents={[]}
        agentDefaults={{ chat: "general", agentic: null }}
        models={[]}
        workers={[]}
        prefs={null}
        agenticBlocked={null}
        workspace="w1"
        onSubmit={async () => {}}
        predict={() => null}
        onLearn={() => {}}
        onOpen={() => {}}
        seed={1}
        history={{ titles: [], banned: [] }}
      />,
    ),
  );
  // The bar is there at once, the list is not.
  expect(host.querySelector(".composer")).not.toBeNull();
  expect(host.querySelector(".similar")).toBeNull();
  await act(async () => vi.dynamicImportSettled());
  await act(async () => {
    await vi.advanceTimersByTimeAsync(300);
  });
  expect(search).toHaveBeenCalled();
  expect(host.querySelector(".similar-title")?.textContent).toBe("Rebase notes");
  act(() => root.unmount());
  vi.useRealTimers();
});
