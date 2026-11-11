// @vitest-environment happy-dom
// The project icon set is a lazy chunk (the first-load budget): until it has
// loaded the sidebar draws the default folder, the same size, then the icon.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, test } from "vitest";
import { ProjectIcon } from "./projectIcons";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

test("a project's own icon follows the folder once the set has loaded", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  act(() => root.render(<ProjectIcon icon="rocket" accent="blue" size={14} />));
  const first = host.querySelector("svg")!;
  expect(first.getAttribute("class")).toContain("lucide-folder");
  expect(first.getAttribute("width")).toBe("14");
  await act(async () => {
    await import("./projectIconSet");
  });
  const then = host.querySelector("svg")!;
  expect(then.getAttribute("class")).toContain("lucide-rocket");
  expect(then.getAttribute("class")).toContain("project-icon");
  expect(then.getAttribute("width")).toBe("14");
  act(() => root.unmount());
  host.remove();
});

test("the default folder needs no chunk", () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  act(() => root.render(<ProjectIcon icon="folder" accent="" />));
  expect(host.querySelector("svg")!.getAttribute("class")).toContain("lucide-folder");
  act(() => root.unmount());
  host.remove();
});
