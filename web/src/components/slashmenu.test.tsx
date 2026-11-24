// @vitest-environment happy-dom
// The composer's `/` menu (X.65): opens on `/` at the start of a message in a
// chat, lists the built-in commands and the skills the node offers, filters,
// completes without sending, and shows repository text as inert text. The
// only request it makes is the one list of skills.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { forgetSkills, readSkills, rank, complete, BUILTINS } from "../lib/slashMenu";
import { slashQuery } from "../lib/compactCommand";

const call = vi.fn<(...a: unknown[]) => Promise<unknown>>();
vi.mock("../api", async (orig) => ({ ...(await orig<typeof import("../api")>()), call: (...a: unknown[]) => call(...a) }));
const { Composer } = await import("./Composer");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const SKILLS = {
  skills: [
    { name: "deploy", description: "Ship it" },
    { name: "release", description: "Cut a release, then deploy it" },
    { name: "notes", description: "Write notes" },
  ],
};

let host: HTMLDivElement;
let root: Root;
let sent: string[];
let compacted: string[];
beforeEach(async () => {
  // The menu is a lazy chunk: loaded before the test types, so a loaded machine cannot outrun it.
  await import("./SlashMenu");
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  call.mockReset();
  call.mockResolvedValue(SKILLS);
  forgetSkills();
  sent = [];
  compacted = [];
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const flush = () => act(async () => {});
async function mount(over: { chatId?: string; where?: "session" | "home" } = {}) {
  await act(async () =>
    root.render(
      <Composer
        where={over.where ?? "session"}
        agents={[]}
        models={[]}
        workers={[]}
        agentId=""
        onAgentChange={() => {}}
        agenticBlocked={null}
        onSubmit={async (s) => void sent.push(s.text)}
        compact={async (f) => void compacted.push(f)}
        chatId={"chatId" in over ? over.chatId : "s1"}
      />,
    ),
  );
  return host.querySelector("textarea")!;
}
async function type(ta: HTMLTextAreaElement, value: string, caret = value.length) {
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(ta, value);
    ta.setSelectionRange(caret, caret);
    ta.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await flush();
  await act(async () => void (await new Promise((r) => setTimeout(r, 0))));
}
const key = (ta: HTMLTextAreaElement, k: string, extra: KeyboardEventInit = {}) =>
  act(async () => void ta.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true, ...extra })));
const names = () => [...host.querySelectorAll(".slash-menu li b")].map((b) => b.textContent);
const menu = () => host.querySelector<HTMLUListElement>(".slash-menu");
const active = () => host.querySelector(".slash-menu li.active b")?.textContent;

describe("the pure parts", () => {
  test("the menu's word is the first word, only while the caret is in it", () => {
    expect(slashQuery("/", 1)).toBe("");
    expect(slashQuery("/dep", 4)).toBe("dep");
    expect(slashQuery("/dep", 2)).toBe("dep");
    expect(slashQuery("/dep", 0)).toBeNull();
    expect(slashQuery("/dep ", 5)).toBeNull();
    expect(slashQuery("/dep rest", 9)).toBeNull();
    expect(slashQuery("/dep rest", 3)).toBe("dep");
    expect(slashQuery("hello /dep", 10)).toBeNull();
    expect(slashQuery("/usr/bin", 8)).toBeNull();
    expect(slashQuery("//", 2)).toBeNull();
    expect(slashQuery("/a.b_c-d", 8)).toBe("a.b_c-d");
    expect(slashQuery("/<script>", 9)).toBeNull();
  });

  test("a list from the server keeps only what has the shape", () => {
    const many = Array.from({ length: 400 }, (_, i) => ({ name: `s${i}`, description: "d" }));
    expect(readSkills({ skills: many }).length).toBe(150);
    const got = readSkills({
      skills: [
        { name: "ok", description: "é".repeat(500) },
        { name: "ok", description: "again" },
        { name: "a b", description: "space" },
        { name: "a/b", description: "slash" },
        { name: "x‮y", description: "bidi" },
        { name: "<img src=x onerror=1>", description: "markup" },
        { name: "compact", description: "shadows" },
        { name: 7, description: "number" },
        null,
        "string",
        { name: "nodesc" },
      ],
    });
    expect(got.map((g) => g.name)).toEqual(["ok", "nodesc"]);
    expect(Array.from(got[0]!.description).length).toBe(200);
    expect(readSkills(null)).toEqual([]);
    expect(readSkills({ skills: "no" })).toEqual([]);
  });

  test("names that start with the word come before names that hold it", () => {
    const e = (name: string) => ({ name, description: "", builtin: false });
    expect(rank([e("redeploy"), e("deploy"), e("x"), e("Dep")], "dep").map((x) => x.name)).toEqual(["deploy", "Dep", "redeploy"]);
    expect(rank([e("a"), e("b")], "").map((x) => x.name)).toEqual(["a", "b"]);
  });

  test("completing makes the first word /name and a space, the rest kept", () => {
    expect(complete("/de", "deploy")).toEqual({ text: "/deploy ", caret: 8 });
    expect(complete("/de rest", "deploy")).toEqual({ text: "/deploy rest", caret: 8 });
    expect(complete("/", "compact")).toEqual({ text: "/compact ", caret: 9 });
    expect(complete("/de\nrest", "deploy")).toEqual({ text: "/deploy \nrest", caret: 8 });
  });
});

describe("in a chat's composer", () => {
  test("`/` lists the commands and the skills, read once, as a listbox", async () => {
    const ta = await mount();
    await type(ta, "/");
    expect(menu()?.getAttribute("role")).toBe("listbox");
    expect(names()).toEqual(["/compact", "/deploy", "/release", "/notes"]);
    expect(call).toHaveBeenCalledTimes(1);
    expect(call.mock.calls[0]!.slice(0, 2)).toEqual(["GET", "/api/sessions/s1/skills-menu"]);
    expect(host.querySelector(".slash-menu li")?.getAttribute("role")).toBe("option");
    expect(host.querySelector('.slash-menu [aria-selected="true"]')?.textContent).toContain("/compact");
    expect(host.querySelector('[role="status"].sr-only')?.textContent).toBe(`/compact [focus], ${BUILTINS[0]!.description}, 1 of 4`);
    // Typing on does not ask again; neither does a second opening soon after.
    await type(ta, "/d");
    await type(ta, "");
    await type(ta, "/");
    expect(call).toHaveBeenCalledTimes(1);
  });

  test("typing filters: the prefix first, then the rest", async () => {
    const ta = await mount();
    await type(ta, "/ea");
    expect(names()).toEqual(["/release"]);
    await type(ta, "/dep");
    expect(names()).toEqual(["/deploy"]);
    await type(ta, "/zzz");
    expect(menu()).toBeNull();
  });

  test("Down and Enter complete to /name and a space, caret after, nothing sent", async () => {
    const ta = await mount();
    await type(ta, "/");
    await key(ta, "ArrowDown");
    expect(active()).toBe("/deploy");
    await key(ta, "ArrowUp");
    await key(ta, "ArrowUp");
    expect(active()).toBe("/notes");
    await key(ta, "ArrowDown");
    await key(ta, "ArrowDown");
    expect(active()).toBe("/deploy");
    await key(ta, "Enter");
    expect(ta.value).toBe("/deploy ");
    expect(ta.selectionStart).toBe(8);
    expect(sent).toEqual([]);
    expect(compacted).toEqual([]);
    expect(menu()).toBeNull();
    expect(call).toHaveBeenCalledTimes(1);
    // Now Enter sends the message, as before.
    await type(ta, "/deploy staging");
    await key(ta, "Enter");
    expect(sent).toEqual(["/deploy staging"]);
  });

  test("Tab completes too; the mouse completes; Escape closes and keeps the text", async () => {
    const ta = await mount();
    await type(ta, "/no");
    await key(ta, "Tab");
    expect(ta.value).toBe("/notes ");
    await type(ta, "/re");
    await act(async () => void host.querySelector(".slash-menu li")!.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, cancelable: true })));
    expect(ta.value).toBe("/release ");
    expect(sent).toEqual([]);
    await type(ta, "/de");
    await key(ta, "Escape");
    expect(menu()).toBeNull();
    expect(ta.value).toBe("/de");
    // Typing again opens it again.
    await type(ta, "/dep");
    expect(menu()).not.toBeNull();
  });

  test("`/compact` typed in full and Enter is still the command", async () => {
    const ta = await mount();
    await type(ta, "/compact");
    expect(names()).toEqual(["/compact"]);
    await key(ta, "Enter");
    expect(compacted).toEqual([""]);
    expect(sent).toEqual([]);
    await type(ta, "/compact keep the tests");
    expect(menu()).toBeNull();
    await key(ta, "Enter");
    expect(compacted).toEqual(["", "keep the tests"]);
    // A skill typed in full is sent, as before.
    await type(ta, "/deploy");
    await key(ta, "Enter");
    expect(sent).toEqual(["/deploy"]);
  });

  test("a path, a later slash and a caret in later text open nothing", async () => {
    const ta = await mount();
    for (const v of ["/usr/bin", "see /de", "//", "/de rest"]) {
      await type(ta, v);
      expect(menu(), v).toBeNull();
    }
    await type(ta, "/dep rest", 4);
    expect(menu()).not.toBeNull();
    await type(ta, "/dep rests", 10);
    expect(menu()).toBeNull();
    expect(call).toHaveBeenCalledTimes(1);
  });

  test("an IME composition is not completed, Shift+Enter and chords are left alone", async () => {
    const ta = await mount();
    await type(ta, "/");
    await key(ta, "Enter", { isComposing: true });
    expect(ta.value).toBe("/");
    await key(ta, "Enter", { shiftKey: true });
    expect(ta.value).toBe("/");
    await key(ta, "Tab", { ctrlKey: true });
    expect(ta.value).toBe("/");
  });

  test("a failed read, an empty list and a hostile one leave the built-ins and inert text", async () => {
    call.mockRejectedValue(new Error("503"));
    let ta = await mount();
    await type(ta, "/");
    expect(names()).toEqual(["/compact"]);
    forgetSkills();
    call.mockResolvedValue({
      skills: [
        { name: "compact", description: "shadow" },
        { name: "ok", description: "<img src=x onerror=alert(1)> ‮ hidden <a href='http://x'>l</a>" },
        { name: "<b>", description: "markup" },
      ],
    });
    act(() => root.unmount());
    host.remove();
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
    ta = await mount();
    await type(ta, "/");
    expect(names()).toEqual(["/compact", "/ok"]);
    expect(host.querySelector(".slash-menu img, .slash-menu a")).toBeNull();
    const text = host.querySelector(".slash-menu")!.textContent!;
    expect(text).toContain("<img src=x");
    expect(text).toContain("<U+202E>");
  });

  test("no menu in the new-chat composer or a chat without commands", async () => {
    let ta = await mount({ where: "home" });
    await type(ta, "/");
    expect(menu()).toBeNull();
    ta = await mount({ chatId: undefined });
    await type(ta, "/");
    expect(menu()).toBeNull();
    expect(call).not.toHaveBeenCalled();
  });

  test("choosing sends no request beyond the one list", async () => {
    const ta = await mount();
    await type(ta, "/");
    await key(ta, "ArrowDown");
    await key(ta, "Enter");
    expect(call).toHaveBeenCalledTimes(1);
    expect(call.mock.calls.every((c) => String(c[1]).endsWith("/skills-menu"))).toBe(true);
  });
});
