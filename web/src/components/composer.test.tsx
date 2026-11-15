// @vitest-environment happy-dom
// Who changed the bar's mode: the home background follows the user at once
// and the classifier only once it has settled, so each change must say
// which it was.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, test, vi } from "vitest";
import type { Mode } from "../lib/modes";
import { Composer, type Submission } from "./Composer";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function mount() {
  const changes: [Mode, "user" | "auto"][] = [];
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  act(() =>
    root.render(
      <Composer
        where="home"
        agents={[]}
        models={[]}
        workers={[]}
        agentId=""
        onAgentChange={() => {}}
        agenticBlocked={null}
        onSubmit={async () => {}}
        predict={(text) => (text.startsWith("http") ? "search" : "ask")}
        onModeChange={(mode, by) => changes.push([mode, by])}
      />,
    ),
  );
  const textarea = host.querySelector("textarea")!;
  const type = (value: string) =>
    act(() => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(textarea, value);
      textarea.dispatchEvent(new Event("input", { bubbles: true }));
    });
  const click = (label: string) =>
    act(() => [...host.querySelectorAll<HTMLButtonElement>(".mode-switch button")].find((b) => b.textContent === label)!.click());
  const tab = () => act(() => textarea.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", bubbles: true })));
  return { changes, type, click, tab, host, unmount: () => act(() => root.unmount()) };
}

describe("mode changes", () => {
  test("the classifier's picks are auto, clicks and Tab are the user's", () => {
    const c = mount();
    expect(c.changes).toEqual([["ask", "auto"]]);
    c.type("https://example.com");
    expect(c.changes.at(-1)).toEqual(["search", "auto"]);
    c.click("Agentic");
    expect(c.changes.at(-1)).toEqual(["agentic", "user"]);
    // Picking the mode it already has changes nothing and reports nothing.
    const n = c.changes.length;
    c.click("Agentic");
    expect(c.changes.length).toBe(n);
    c.tab();
    expect(c.changes.at(-1)).toEqual(["search", "user"]);
    c.unmount();
  });

  test("the switch has a thumb that screen readers skip", () => {
    const c = mount();
    const thumb = c.host.querySelector(".mode-switch > .mode-thumb");
    expect(thumb?.getAttribute("aria-hidden")).toBe("true");
    expect(c.host.querySelectorAll('.mode-switch [role="radio"]').length).toBe(3);
    c.unmount();
  });
});

describe("model for a mention", () => {
  const info = (id: string) => ({
    id,
    raw: id.split("::")[1]!,
    provider: id.split("::")[0]!,
    provider_id: id.split("::")[0]!,
    name: null,
    context: null,
    max_output: null,
    effort: false,
    vision: false,
    tools: false,
    default: id === "openai::gpt-4o",
    unavailable: false,
  });
  test("a leading @agent sends with that agent's default", () => {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    act(() =>
      root.render(
        <Composer
          where="home"
          agents={[
            { id: "general", name: "General", description: "", builtin: true, default_model: null, main_model: "openai::gpt-4o", fallbacks: [], unavailable: [], avatar: { shape: "circle", eyes: "round", accent: "blue" } },
            { id: "coder", name: "Coder", description: "", builtin: true, default_model: "anthropic::claude-x", main_model: "anthropic::claude-x", fallbacks: [], unavailable: [], avatar: { shape: "circle", eyes: "round", accent: "blue" } },
          ]}
          models={[info("openai::gpt-4o"), info("anthropic::claude-x")]}
          workers={[]}
          agentId="general"
          onAgentChange={() => {}}
          agenticBlocked={null}
          onSubmit={async () => {}}
          predict={() => "ask"}
        />,
      ),
    );
    const textarea = host.querySelector("textarea")!;
    const select = () => host.querySelector<HTMLSelectElement>('select[aria-label="model"]')!;
    expect(select().value).toBe("openai::gpt-4o");
    act(() => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(textarea, "@coder fix this");
      textarea.dispatchEvent(new Event("input", { bubbles: true }));
    });
    expect(select().value).toBe("anthropic::claude-x");
    act(() => root.unmount());
    host.remove();
  });
});

describe("models the provider dropped", () => {
  const info = (id: string, more: { default?: boolean; unavailable?: boolean } = {}) => ({
    id,
    raw: id.split("::")[1]!,
    provider: "OpenAI",
    provider_id: "openai",
    name: null,
    context: null,
    max_output: null,
    effort: false,
    vision: false,
    tools: false,
    default: false,
    unavailable: false,
    ...more,
  });
  const mount = (models: ReturnType<typeof info>[], defaultModel?: string) => {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    act(() =>
      root.render(
        <Composer
          where="home"
          agents={[]}
          models={models}
          defaultModel={defaultModel}
          workers={[]}
          agentId=""
          onAgentChange={() => {}}
          agenticBlocked={null}
          onSubmit={async () => {}}
          predict={() => "ask"}
        />,
      ),
    );
    const options = (label: string) =>
      [...host.querySelectorAll<HTMLOptionElement>(`select[aria-label="${label}"] option`)].map((o) => [o.value, o.disabled, o.textContent]);
    const value = () => host.querySelector<HTMLSelectElement>('select[aria-label="model"]')!.value;
    return { options, value, done: () => (act(() => root.unmount()), host.remove()) };
  };

  test("are not offered, and never picked by default", () => {
    const c = mount([info("openai::gpt-4", { default: true, unavailable: true }), info("openai::gpt-4o"), info("openai::gpt-4o-mini")]);
    expect(c.value()).toBe("openai::gpt-4o");
    expect(c.options("model")).toEqual([
      ["openai::gpt-4o", false, "gpt-4o"],
      ["openai::gpt-4o-mini", false, "gpt-4o-mini"],
    ]);
    expect(c.options("compare with").map((o) => o[0])).toEqual(["", "openai::gpt-4o-mini"]);
    c.done();
  });

  test("a default naming one still shows it, marked and not to be picked again", () => {
    const c = mount([info("openai::gpt-4", { unavailable: true }), info("openai::gpt-4o")], "openai::gpt-4");
    expect(c.value()).toBe("openai::gpt-4");
    expect(c.options("model")).toEqual([
      ["openai::gpt-4", true, "gpt-4 (unavailable)"],
      ["openai::gpt-4o", false, "gpt-4o"],
    ]);
    expect(c.options("compare with").map((o) => o[0])).toEqual(["", "openai::gpt-4o"]);
    c.done();
  });
});

describe("a chat the viewer cannot drive here", () => {
  test("the reply box is read-only and says why; Stop still works", () => {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    let stopped = false;
    act(() =>
      root.render(
        <Composer
          where="session"
          agents={[]}
          models={[]}
          workers={[]}
          agentId="general"
          onAgentChange={() => {}}
          agenticBlocked={null}
          readOnly="your role in Acme cannot run agentic sessions"
          running
          onStop={() => {
            stopped = true;
          }}
          onSubmit={async () => {}}
        />,
      ),
    );
    expect(host.querySelector("textarea")!.disabled).toBe(true);
    expect(host.textContent).toContain("This chat is read-only: your role in Acme cannot run agentic sessions.");
    act(() => host.querySelector<HTMLButtonElement>("button.stop")!.click());
    expect(stopped).toBe(true);
    act(() => root.unmount());
  });
});

describe("writing while the agent works", () => {
  test("the box stays open: Enter and Send send, Stop stays", async () => {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    const sent: string[] = [];
    act(() =>
      root.render(
        <Composer
          where="session"
          agents={[]}
          models={[]}
          workers={[]}
          agentId="general"
          onAgentChange={() => {}}
          agenticBlocked={null}
          running
          onStop={() => {}}
          onSubmit={async (s) => {
            sent.push(s.text);
          }}
        />,
      ),
    );
    const textarea = host.querySelector("textarea")!;
    expect(textarea.disabled).toBe(false);
    expect(textarea.placeholder).toContain("write to add to the task");
    // Said in words too, not only in the placeholder (review ST-10).
    expect(host.querySelector(".composer-hint")!.textContent).toBe("Read after the current step. Stop interrupts.");
    // Nothing typed: Stop alone.
    expect(host.querySelector("button.stop")).not.toBeNull();
    expect(host.querySelector('button.send:not(.stop)')).toBeNull();
    const type = (value: string) =>
      act(() => {
        Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(textarea, value);
        textarea.dispatchEvent(new Event("input", { bubbles: true }));
      });
    type("also update the README");
    expect(host.querySelector("button.stop")).not.toBeNull();
    expect(host.querySelector<HTMLButtonElement>('button.send:not(.stop)')!.disabled).toBe(false);
    await act(async () => textarea.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })));
    expect(sent).toEqual(["also update the README"]);
    type("and the changelog");
    await act(async () => host.querySelector<HTMLButtonElement>('button.send:not(.stop)')!.click());
    expect(sent).toEqual(["also update the README", "and the changelog"]);
    act(() => root.unmount());
  });
});

describe("@ file mentions (T63)", () => {
  const agent = { id: "coder", name: "Coder", description: "codes", builtin: true, default_model: null, main_model: "m", fallbacks: [], unavailable: [], avatar: { shape: "circle" as const, eyes: "round" as const, accent: "blue" as const } };
  async function mountSession(filesOf: string | undefined, paths: string[]) {
    const { pageApi } = await import("../apiPages");
    await import("../lib/fileMentions");
    const asked: string[] = [];
    vi.spyOn(pageApi, "files").mockImplementation(async (_id: string, q: string) => {
      asked.push(q);
      return { paths: paths.filter((p) => p.includes(q.replace(/^\.\//, ""))) };
    });
    const sent: Submission[] = [];
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    act(() =>
      root.render(
        <Composer
          where="session"
          agents={[agent]}
          models={[]}
          workers={[]}
          agentId="coder"
          onAgentChange={() => {}}
          agenticBlocked={null}
          onSubmit={async (s) => {
            sent.push(s);
          }}
          filesOf={filesOf}
        />,
      ),
    );
    const textarea = host.querySelector("textarea")!;
    const type = async (value: string) => {
      act(() => {
        Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(textarea, value);
        textarea.setSelectionRange(value.length, value.length);
        textarea.dispatchEvent(new Event("input", { bubbles: true }));
      });
      await act(async () => {
        vi.advanceTimersByTime(400);
        await Promise.resolve();
        await Promise.resolve();
      });
    };
    const key = (k: string) => act(() => textarea.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true })));
    return { host, textarea, type, key, sent, asked, unmount: () => act(() => root.unmount()) };
  }

  test("a path token lists the chat's files under Files, picks one and sends it", async () => {
    vi.useFakeTimers();
    const c = await mountSession("s1", ["src/main.rs", "src/lib.rs"]);
    await c.type("fix @src/ma");
    const popup = c.host.querySelector(".mention-popup")!;
    expect(popup.textContent).toContain("Files");
    expect(popup.textContent).toContain("@src/main.rs");
    expect(c.asked[0]).toBe("src/ma");
    c.key("Enter");
    expect(c.textarea.value).toBe("fix @src/main.rs ");
    // The person goes on typing after the mention (happy-dom reports no
    // caret move of its own).
    await c.type("fix @src/main.rs now");
    expect(c.host.querySelector(".mention-popup")).toBeNull();
    // Shown before sending: the path that goes with the message.
    expect(c.host.querySelector(".composer .chips")?.textContent).toContain("src/main.rs");
    await act(async () => {
      c.key("Enter");
      await vi.dynamicImportSettled();
      await Promise.resolve();
    });
    expect(c.sent[0]!.files).toEqual(["src/main.rs"]);
    c.unmount();
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  test("only paths the folder holds are attached; one taken off is not sent", async () => {
    vi.useFakeTimers();
    const c = await mountSession("s1", ["src/main.rs", "src/lib.rs"]);
    await c.type("bump @types/node and fix @src/main.rs and @src/lib.rs now");
    const chips = () => [...c.host.querySelectorAll(".composer .chips li")].map((x) => x.textContent?.trim());
    expect(chips()).toEqual(["src/main.rs", "src/lib.rs"]);
    act(() => c.host.querySelector<HTMLButtonElement>('.composer .chips button[aria-label$="src/lib.rs"]')!.click());
    expect(chips()).toEqual(["src/main.rs"]);
    await act(async () => {
      c.key("Enter");
      await vi.dynamicImportSettled();
      for (let i = 0; i < 6; i++) await Promise.resolve();
    });
    expect(c.sent[0]!.files).toEqual(["src/main.rs"]);
    c.unmount();
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  test("a bare word is an agent, even where a file has that name", async () => {
    vi.useFakeTimers();
    const c = await mountSession("s1", ["coder", "coder/x.rs"]);
    await c.type("@cod");
    const popup = c.host.querySelector(".mention-popup")!;
    expect(popup.textContent).toContain("@coder");
    expect(popup.textContent).not.toContain("Files");
    expect(c.asked).toEqual([]);
    await c.type("@coder/");
    expect(c.host.querySelector(".mention-popup")!.textContent).toContain("@coder/x.rs");
    c.unmount();
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  test("no match says so; a chat without a folder offers no files", async () => {
    vi.useFakeTimers();
    const c = await mountSession("s1", []);
    await c.type("see @nope.rs");
    expect(c.host.querySelector(".mention-none")?.textContent).toContain("No file or folder");
    c.unmount();
    const d = await mountSession(undefined, ["src/main.rs"]);
    await d.type("see @src/ma");
    expect(d.host.querySelector(".mention-popup")).toBeNull();
    expect(d.asked).toEqual([]);
    await act(async () => {
      d.key("Enter");
      await Promise.resolve();
    });
    expect(d.sent[0]!.files).toBeUndefined();
    d.unmount();
    vi.useRealTimers();
    vi.restoreAllMocks();
  });
});
