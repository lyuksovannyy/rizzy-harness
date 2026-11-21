// @vitest-environment happy-dom
// The mode classifier loads as its own chunk after the first paint. Until it
// is in, nothing is guessed (null: the bar keeps its mode); then the same
// answers as lib/classifier.ts give, the saved weights are applied, and what
// is learned is saved (weights only) a second later.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, test, vi } from "vitest";
import { decide, emptyState } from "./classifier";
import { useClassifier } from "./useClassifier";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

type Api = ReturnType<typeof useClassifier>;

function mount(save: (s: unknown) => void) {
  const out: { api?: Api } = {};
  function Probe() {
    out.api = useClassifier(true, save);
    return null;
  }
  const host = document.createElement("div");
  const root = createRoot(host);
  act(() => root.render(<Probe />));
  return { out, stop: () => act(() => root.unmount()) };
}

test("no guess until the code is in, then the rules' answer", async () => {
  const m = mount(() => {});
  const text = "run cargo test and fix what fails in src/main.rs";
  expect(m.out.api!.predict(text)).toBeNull();
  const before = m.out.api!.predict;
  await act(async () => vi.dynamicImportSettled());
  // A new function, so the composer's effect asks again.
  expect(m.out.api!.predict).not.toBe(before);
  expect(m.out.api!.predict(text)).toBe(decide(emptyState(), text, true).mode);
  m.stop();
});

test("what the server sent first is applied once the code is in; learning is saved without the text", async () => {
  vi.useFakeTimers();
  const save = vi.fn();
  const m = mount(save);
  m.out.api!.load(emptyState());
  m.out.api!.learn("early", "ask"); // before the code: nothing recorded
  await act(async () => vi.dynamicImportSettled());
  expect(save).not.toHaveBeenCalled();
  act(() => m.out.api!.learn("explain rust lifetimes", "ask"));
  await act(async () => vi.advanceTimersByTimeAsync(1100));
  expect(save).toHaveBeenCalledTimes(1);
  const sent = save.mock.calls[0]![0] as { labels: number };
  expect(sent.labels).toBe(1);
  expect(JSON.stringify(sent)).not.toContain("lifetimes");
  m.stop();
  vi.useRealTimers();
});

test("learning still waiting for its second is saved when the hook goes (a workspace switch remounts the shell)", async () => {
  vi.useFakeTimers();
  const save = vi.fn();
  const m = mount(save);
  await act(async () => vi.dynamicImportSettled());
  act(() => m.out.api!.learn("explain rust lifetimes", "ask"));
  expect(save).not.toHaveBeenCalled();
  m.stop();
  expect(save).toHaveBeenCalledTimes(1);
  expect((save.mock.calls[0]![0] as { labels: number }).labels).toBe(1);
  // Nothing pending: nothing saved at the end.
  save.mockClear();
  const idle = mount(save);
  await act(async () => vi.dynamicImportSettled());
  idle.stop();
  expect(save).not.toHaveBeenCalled();
  vi.useRealTimers();
});
