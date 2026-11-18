// @vitest-environment happy-dom
// Forge accounts on the Plugins screen (X.27 step 1) against a mocked API:
// one item per allowed host, Connect with a password field (the POST it
// sends), the account as @login with what the forge said shown as text
// only, Replace token and Disconnect, and the note that nothing is cloned
// or pushed yet.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";

const call = vi.fn();
vi.mock("../api", () => ({ call }));
const { default: Connections } = await import("./Connections");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const hosts = [
  { host: "github.com", kind: "github", token_page: "https://github.com/settings/tokens", scopes_needed: "the repo scope" },
  { host: "gitlab.com", kind: "gitlab", token_page: "https://gitlab.com/-/user_settings/personal_access_tokens", scopes_needed: "read_api" },
];
const ada = {
  id: "c1",
  host: "github.com",
  allowed: true,
  login: "ada",
  display_name: '<img src="https://x.example/a.png"> Ada',
  commit_email: "ada@example.com",
  scopes: ["repo", "admin:org"],
  scopes_more: 3,
};

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

async function show(connections: unknown[]) {
  call.mockImplementation((method: string) =>
    Promise.resolve(method === "GET" ? { hosts, connections, available: true, max: 20 } : {}),
  );
  await act(async () => {
    root.render(<Connections />);
  });
}
const button = (text: string) => [...host.querySelectorAll("button")].find((b) => b.textContent === text);
const input = (el: HTMLInputElement, value: string) => {
  const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
  set.call(el, value);
  el.dispatchEvent(new Event("input", { bubbles: true }));
};

describe("connections", () => {
  test("lists hosts and the account as text, says what is not done yet", async () => {
    await show([ada]);
    expect(call).toHaveBeenCalledWith("GET", "/api/connections");
    expect(host.textContent).toContain("GitHub");
    expect(host.textContent).toContain("@ada");
    expect(host.textContent).toContain('<img src="https://x.example/a.png"> Ada');
    expect(host.querySelector("img")).toBeNull();
    expect(host.querySelector("a")).toBeNull();
    expect(host.textContent).toContain("Token scopes: repo, admin:org and 3 more");
    expect(host.textContent).toContain("cloning and pushing come next");
    expect(button("Replace token")).toBeTruthy();
    expect(button("Disconnect")).toBeTruthy();
  });

  test("connect sends the pasted token from a password field once", async () => {
    await show([]);
    await act(async () => button("Connect")!.click());
    const field = host.querySelector<HTMLInputElement>('input[type="password"]')!;
    expect(field).toBeTruthy();
    expect(field.getAttribute("autocomplete")).toBe("new-password");
    expect(field.hasAttribute("data-1p-ignore")).toBe(true);
    expect(field.getAttribute("data-lpignore")).toBe("true");
    expect(host.textContent).toContain("https://github.com/settings/tokens");
    await act(async () => input(field, "  tok_123  "));
    await act(async () => button("Save")!.click());
    expect(call).toHaveBeenCalledWith("POST", "/api/connections", { host: "github.com", token: "  tok_123  " });
    expect(host.querySelector('input[type="password"]')).toBeNull();
  });

  test("replace and disconnect", async () => {
    await show([ada]);
    await act(async () => button("Replace token")!.click());
    await act(async () => input(host.querySelector<HTMLInputElement>('input[type="password"]')!, "tok_new"));
    await act(async () => button("Save")!.click());
    expect(call).toHaveBeenCalledWith("PUT", "/api/connections/c1/token", { token: "tok_new" });
    window.confirm = () => true;
    await act(async () => button("Disconnect")!.click());
    expect(call).toHaveBeenCalledWith("DELETE", "/api/connections/c1");
  });

  test("an error is shown and the field stays", async () => {
    call.mockImplementation((method: string) =>
      method === "GET" ? Promise.resolve({ hosts, connections: [], available: true }) : Promise.reject(new Error("github.com refused this token (HTTP 401)")),
    );
    await act(async () => {
      root.render(<Connections />);
    });
    await act(async () => button("Connect")!.click());
    await act(async () => input(host.querySelector<HTMLInputElement>('input[type="password"]')!, "bad"));
    await act(async () => button("Save")!.click());
    expect(host.textContent).toContain("refused this token");
    // The field stays for another try, emptied: the token is not kept.
    expect(host.querySelector<HTMLInputElement>('input[type="password"]')!.value).toBe("");
  });
});
