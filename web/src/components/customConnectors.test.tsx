// @vitest-environment happy-dom
// Custom connectors on the Plugins screen (X.35 step 1) against a mocked
// API: the workspace's connectors with what a check found, as text only;
// Connect from a password field; Check; Edit that warns before an address
// change disconnects everyone; editors alone add, edit and delete; the
// notes that agents do not use them yet and that browser sign-in is not
// built.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";

const call = vi.fn();
vi.mock("../api", () => ({ call }));
const { default: CustomConnectors } = await import("./CustomConnectors");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const linear = {
  id: "k1",
  name: "linear",
  url: "https://mcp.example.com/mcp",
  host: "mcp.example.com",
  auth: "token",
  header: "authorization",
  clash: false,
  stamp: "s1",
  connected: true,
  offer: {
    server_name: "Linear <b>MCP</b>",
    server_version: "1.0",
    instructions: '<img src="https://x.example/a.png">',
    tools: [{ name: "list_issues", agent_name: "mcp__linear__list_issues", description: '<a href="https://evil.example">x</a>' }],
    more: true,
  },
};
const open = { ...linear, id: "k2", name: "docs", auth: "none", connected: null, offer: null };

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  call.mockReset();
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

let topics: Record<string, () => void> = {};
const live = {
  onTopic: (t: string, fn: () => void) => {
    topics[t] = fn;
    return () => delete topics[t];
  },
} as unknown as import("../live").Live;

async function show(connectors: unknown[], can_edit: boolean, adding = false) {
  let list = connectors;
  call.mockImplementation((method: string) =>
    Promise.resolve(method === "GET" ? { connectors: list, can_edit, person: true, available: true, max: 50 } : {}),
  );
  await act(async () => {
    root.render(<CustomConnectors scope="ws1" live={live} adding={adding} onAdded={() => undefined} />);
  });
  return (next: unknown[]) => {
    list = next;
  };
}
const button = (text: string) => [...host.querySelectorAll("button")].find((b) => b.textContent === text);
const input = (el: HTMLInputElement, value: string) => {
  const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
  set.call(el, value);
  el.dispatchEvent(new Event("input", { bubbles: true }));
};

describe("custom connectors", () => {
  test("lists connectors and what a check found as text, says whose account agents use", async () => {
    await show([{ ...linear, agent_tools: 3 }, open], false);
    expect(call).toHaveBeenCalledWith("GET", "/api/workspaces/ws1/connectors");
    expect(host.textContent).toContain("use a connector with your own account");
    expect(host.textContent).toContain("Your agents can use 3 of its tools now.");
    expect(host.textContent).toContain("Your agents get its tools once it has been checked");
    expect(host.textContent).toContain("Linear <b>MCP</b> 1.0 offers 1 or more tool");
    expect(host.textContent).toContain('<a href="https://evil.example">x</a>');
    expect(host.querySelector("img")).toBeNull();
    expect(host.querySelector("a")).toBeNull();
    expect(host.querySelector("b")).toBeNull();
    expect(host.textContent).toContain("Your account is connected.");
    expect(button("Replace token")).toBeTruthy();
    expect(button("Disconnect")).toBeTruthy();
    expect(button("Edit")).toBeUndefined();
    expect(button("Delete")).toBeUndefined();
  });

  test("connect sends the pasted token from a password field once", async () => {
    await show([{ ...linear, connected: false, offer: null }], false);
    await act(async () => button("Connect")!.click());
    const field = host.querySelector<HTMLInputElement>('input[type="password"]')!;
    expect(field.getAttribute("autocomplete")).toBe("new-password");
    await act(async () => input(field, " tok-123 "));
    await act(async () => host.querySelector("form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })));
    expect(call).toHaveBeenCalledWith("PUT", "/api/workspaces/ws1/connectors/k1/account", { token: " tok-123 ", seen: "s1" });
  });

  test("check posts; a connector without sign-in has no Connect", async () => {
    await show([open], false);
    expect(button("Connect")).toBeUndefined();
    await act(async () => button("Check")!.click());
    expect(call).toHaveBeenCalledWith("POST", "/api/workspaces/ws1/connectors/k2/check", { seen: "s1" });
  });

  test("editors add with the browser sign-in note, and an address change asks first", async () => {
    await show([linear], true, true);
    expect(host.textContent).toContain("Add custom connector");
    expect(host.textContent).toContain("(OAuth) is not built yet");
    await act(async () => button("Edit")!.click());
    const edit = host.querySelector<HTMLElement>('[aria-label="Edit linear"]')!;
    const url = [...edit.querySelectorAll("input")].find((i) => i.value.startsWith("https://"))!;
    await act(async () => input(url, "https://other.example.com/mcp"));
    const confirm = vi.fn(() => false);
    vi.stubGlobal("confirm", confirm);
    await act(async () => [...edit.querySelectorAll("button")].find((b) => b.textContent === "Save")!.click());
    expect(String((confirm.mock.calls[0] as unknown[])[0])).toContain("disconnects everyone's accounts");
    expect(call).not.toHaveBeenCalledWith("PUT", "/api/workspaces/ws1/connectors/k1", expect.anything());
    confirm.mockReturnValue(true);
    await act(async () => [...edit.querySelectorAll("button")].find((b) => b.textContent === "Save")!.click());
    expect(call).toHaveBeenCalledWith("PUT", "/api/workspaces/ws1/connectors/k1", { url: "https://other.example.com/mcp", auth: "token" });
    vi.unstubAllGlobals();
  });

  test("a change event reloads the list, and an open token form on a changed connector closes", async () => {
    const next = await show([{ ...linear, connected: false, offer: null }], false);
    await act(async () => button("Connect")!.click());
    expect(host.querySelector('input[type="password"]')).toBeTruthy();
    next([{ ...linear, url: "https://elsewhere.example.com/mcp", stamp: "s2", connected: false, offer: null }]);
    await act(async () => topics["connectors"]!());
    expect(call.mock.calls.filter((c) => c[0] === "GET").length).toBe(2);
    expect(host.textContent).toContain("https://elsewhere.example.com/mcp");
    expect(host.querySelector('input[type="password"]')).toBeNull();
  });

  test("a header change asks before it disconnects everyone", async () => {
    await show([linear], true);
    await act(async () => button("Edit")!.click());
    const edit = host.querySelector<HTMLElement>('[aria-label="Edit linear"]')!;
    const header = [...edit.querySelectorAll("input")].find((i) => i.placeholder === "X-API-Key")!;
    await act(async () => input(header, "X-API-Key"));
    const confirm = vi.fn(() => false);
    vi.stubGlobal("confirm", confirm);
    await act(async () => [...edit.querySelectorAll("button")].find((b) => b.textContent === "Save")!.click());
    expect(String((confirm.mock.calls[0] as unknown[])[0])).toContain("header disconnects everyone's accounts");
    expect(call).not.toHaveBeenCalledWith("PUT", "/api/workspaces/ws1/connectors/k1", expect.anything());
    vi.unstubAllGlobals();
  });
});
