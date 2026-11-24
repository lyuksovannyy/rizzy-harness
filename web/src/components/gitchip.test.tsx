// @vitest-environment happy-dom
// The header's git chip (X.64): the branch and the count of changed files of a
// coding chat's folder. The branch is text from the repository: cut, drawn as
// text, hidden characters as codes, never markup. Nothing for a folder that is
// no repository; read when the chat opens, again when a turn ends and on a
// click, never polled; a failed read keeps what is shown.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { readGit } from "../lib/gitChip";

const call = vi.fn<(...a: unknown[]) => Promise<unknown>>();
vi.mock("../api", () => ({ call: (...a: unknown[]) => call(...a) }));
const { GitChip } = await import("./GitChip");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  call.mockReset();
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const flush = () => act(async () => {});
let opened = 0;
/** A chip newly opened (its own mount) on a chat whose route answers `answer`. */
async function draw(answer: unknown, running = false, session = "s 1") {
  call.mockImplementation(() => (answer instanceof Error ? Promise.reject(answer) : Promise.resolve(answer)));
  await act(async () => root.render(<GitChip key={opened++} sessionId={session} running={running} />));
  await flush();
}
const chip = () => host.querySelector<HTMLButtonElement>(".git-chip");

test("the branch and the count, with the whole sentence as its hover and screen-reader text", async () => {
  await draw({ branch: "feat/chip", commit: "abcdef0", changed: 3 });
  expect(call).toHaveBeenCalledWith("GET", "/api/sessions/s%201/git", undefined, expect.anything());
  expect(chip()!.querySelector(".git-name")!.textContent).toBe("feat/chip");
  expect(chip()!.querySelector(".git-count")!.textContent).toBe("3 changed");
  const words = "Branch feat/chip; 3 changed files since the last commit, new files included. Click to refresh.";
  expect(chip()!.title).toBe(words);
  expect(chip()!.getAttribute("aria-label")).toBe(words);
});

test("a clean folder shows the branch alone; one changed file is singular; 1000 is 999+", async () => {
  await draw({ branch: "main", commit: "abcdef0", changed: 0 });
  expect(chip()!.querySelector(".git-count")).toBeNull();
  expect(chip()!.title).toContain("no changed files");
  await draw({ branch: "main", commit: "abcdef0", changed: 1 });
  expect(chip()!.title).toContain("1 changed file since");
  await draw({ branch: "main", commit: "abcdef0", changed: 1000 });
  expect(chip()!.querySelector(".git-count")!.textContent).toBe("999+ changed");
  expect(chip()!.title).toContain("999 or more changed files");
  // A count the node could not tell is not zero.
  await draw({ branch: "main", commit: "abcdef0" });
  expect(chip()!.querySelector(".git-count")).toBeNull();
  expect(chip()!.title).toContain("not known");
});

test("detached, and a branch with no commit yet", async () => {
  await draw({ commit: "abcdef0", changed: 2 });
  expect(chip()!.querySelector(".git-name")!.textContent).toBe("detached at abcdef0");
  expect(chip()!.title).toMatch(/^HEAD is detached at abcdef0/);
  await draw({ branch: "trunk" });
  expect(chip()!.title).toMatch(/^Branch trunk, no commit yet/);
});

test("nothing for a folder that is no repository", async () => {
  await draw(null);
  expect(chip()).toBeNull();
  expect(host.textContent).toBe("");
});

test("a hostile branch is inert text: markup, direction overrides and control characters shown as codes, cut at 80", async () => {
  await draw({ branch: "<script>alert(1)</script><img src=x onerror=alert(2)>", commit: "abcdef0" });
  expect(host.querySelector("script, img")).toBeNull();
  expect(chip()!.querySelector(".git-name")!.textContent).toBe("<script>alert(1)</script><img src=x onerror=alert(2)>");
  await draw({ branch: "a\u202Eb\u001b[31mc\u2028d\u200Be", commit: "abcdef0" });
  const text = chip()!.querySelector(".git-name")!.textContent!;
  expect(text).not.toMatch(/[\u202E\u001b\u2028\u200B]/u);
  expect(text).toMatch(/U\+202E/);
  expect(chip()!.title).not.toMatch(/[\u202E\u001b\u2028\u200B]/u);
  await draw({ branch: "x".repeat(500), commit: "abcdef0" });
  expect(chip()!.querySelector(".git-name")!.textContent).toBe(`${"x".repeat(80)}…`);
  // A branch of emoji is cut on characters, not halves of one.
  await draw({ branch: "😀".repeat(200), commit: "abcdef0" });
  expect(chip()!.querySelector(".git-name")!.textContent).toBe(`${"😀".repeat(80)}…`);
});

test("it is read when the chat opens, when a turn ends and on a click, not otherwise", async () => {
  await draw({ branch: "main", commit: "abcdef0", changed: 0 }, true);
  const key = opened - 1;
  expect(call).toHaveBeenCalledTimes(1);
  // The turn goes on: nothing is asked.
  await act(async () => root.render(<GitChip key={key} sessionId="s 1" running={true} />));
  await flush();
  expect(call).toHaveBeenCalledTimes(1);
  // It ends: the folder is read again, and the count is the new one.
  call.mockImplementation(() => Promise.resolve({ branch: "main", commit: "abcdef0", changed: 4 }));
  await act(async () => root.render(<GitChip key={key} sessionId="s 1" running={false} />));
  await flush();
  expect(call).toHaveBeenCalledTimes(2);
  expect(chip()!.querySelector(".git-count")!.textContent).toBe("4 changed");
  // A click asks again.
  call.mockImplementation(() => Promise.resolve({ branch: "other", commit: "abcdef0", changed: 0 }));
  await act(async () => chip()!.click());
  await flush();
  expect(call).toHaveBeenCalledTimes(3);
  expect(chip()!.querySelector(".git-name")!.textContent).toBe("other");
});

test("a failed or refused read keeps what is shown; a later answer of none clears it", async () => {
  await draw({ branch: "main", commit: "abcdef0", changed: 2 });
  call.mockImplementation(() => Promise.reject(new Error("the server is busy")));
  await act(async () => chip()!.click());
  await flush();
  expect(chip()!.querySelector(".git-name")!.textContent).toBe("main");
  expect(chip()!.getAttribute("aria-busy")).toBe("false");
  call.mockImplementation(() => Promise.resolve(null));
  await act(async () => chip()!.click());
  await flush();
  expect(chip()).toBeNull();
});

test("an answer that is not the shape is nothing to show", () => {
  for (const bad of [undefined, 3, "main", [], {}, { commit: "<img>" }, { branch: "" }, { changed: 4 }]) expect(readGit(bad)).toBeNull();
  expect(readGit({ branch: "m", commit: "ABCDEF0", changed: -1 })).toEqual({ branch: "m", commit: undefined, changed: undefined });
  expect(readGit({ branch: "m", changed: 99999.7 })!.changed).toBe(1000);
});
