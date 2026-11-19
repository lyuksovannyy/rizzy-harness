// @vitest-environment happy-dom
// "From a repository" in the Project popup (ADOPTION_PLAN X.27 step 3, PLAN.md
// T86), against a mocked API: no connection, one, several; the forge's list
// as inert one-line text; a search that waits, cancels the request before and
// treats our own 429 as still loading; GitHub's within-page search said in
// words and paged only on request; errors in place with Retry; Create, which
// answers at once with the project pending, and the pending (cloning, failed)
// project in the popup's and the sidebar's lists: never pickable, a failure's
// reason and Dismiss for its starter only.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeAll, beforeEach, describe, expect, test, vi } from "vitest";
import { type PendingProject, pendingProjects } from "../lib/projects";
import type { NodeInfo } from "../types";
import { Composer } from "./Composer";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

beforeAll(async () => {
  await import("./ProjectPicker");
  await import("./RepoPicker");
  await import("./ExecChoice");
}, 30_000);

const tick = () => act(async () => new Promise((r) => setTimeout(r, 10)));
async function until<T>(q: () => T | null | undefined | false, ms = 4000): Promise<T | null> {
  const by = Date.now() + ms;
  while (!q() && Date.now() < by) await tick();
  return (q() || null) as T | null;
}

const node: NodeInfo = {
  node_id: "node-1",
  hostname: "box",
  version: "0",
  connected: true,
  embedded: false,
  profile: "agentic",
  exec_modes: ["sandbox"],
  exec_gaps: [],
  projects: true,
};

const EVIL = '<img src=x onerror="window.pwned=1">';
const LINK = "[click me](https://evil.example/x) ![i](https://evil.example/i.png)";
const BIDI = "safe‮exe.txt​zw";
const LONG = "x".repeat(5000);

const repo = (n: number, over: Record<string, unknown> = {}) => ({
  id: String(n),
  full_name: `ada/repo-${n}`,
  private: false,
  default_branch: "main",
  description: null,
  updated_ms: Date.now() - n * 3_600_000,
  push: true,
  ...over,
});

type Answer = { status: number; body: unknown } | (() => Promise<{ status: number; body: unknown }>);
let conns: unknown[];
let listings: (path: string) => Answer;
let calls: { method: string; path: string; body: unknown; signal?: AbortSignal | null }[];
let listBody: { projects: unknown[]; pending: PendingProject[]; can_create: boolean };
let cloneAnswer: { status: number; body: unknown };

beforeEach(() => {
  localStorage.clear();
  calls = [];
  conns = [{ id: "c1", host: "github.com", allowed: true, login: "ada" }];
  listings = () => ({ status: 200, body: { repositories: [repo(1), repo(2)], page: 1, has_more: false, left_out: 0, search: "none" } });
  listBody = { projects: [], pending: [], can_create: true };
  cloneAnswer = {
    status: 202,
    body: {
      project_id: "p9",
      workspace_id: "w",
      name: "repo-1",
      node_id: "node-1",
      state: "cloning",
      source: { host: "github.com", full_name: "ada/repo-1" },
      started_ms: 1,
      mine: true,
      reason: null,
    },
  };
  vi.stubGlobal("fetch", async (path: string, init: RequestInit) => {
    const body = init.body ? JSON.parse(init.body as string) : undefined;
    calls.push({ method: init.method ?? "GET", path, body, signal: init.signal });
    const json = (status: number, v: unknown) => new Response(JSON.stringify(v), { status, headers: { "content-type": "application/json" } });
    if (path === "/api/connections") return json(200, { hosts: [], connections: conns, available: true });
    if (path.startsWith("/api/connections/")) {
      let a = listings(path);
      if (typeof a === "function") a = await a();
      if (init.signal?.aborted) throw new DOMException("aborted", "AbortError");
      return json(a.status, a.body);
    }
    if (path.startsWith("/api/projects?") || path === "/api/projects") {
      if (init.method === "POST") return json(cloneAnswer.status, cloneAnswer.body);
      return json(200, { ...listBody, workspace_id: "w", icons: [], accents: [] });
    }
    if (path.endsWith("/clone") && init.method === "DELETE") return json(200, { ok: true });
    return json(404, {});
  });
});
afterEach(() => {
  vi.unstubAllGlobals();
  document.body.innerHTML = "";
  pendingProjects.set([]);
});

const set = (el: HTMLInputElement | HTMLSelectElement, value: string) =>
  act(() => {
    Object.getOwnPropertyDescriptor(Object.getPrototypeOf(el), "value")!.set!.call(el, value);
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  });
const key = (el: HTMLElement, k: string) => act(async () => void el.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true })));
const listCalls = () => calls.filter((c) => c.path.startsWith("/api/connections/"));

async function openRepos() {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  act(() =>
    root.render(
      <Composer
        where="home"
        agents={[]}
        models={[]}
        workers={[node]}
        agentId=""
        onAgentChange={() => {}}
        agenticBlocked={null}
        workspace="w"
        onSubmit={async () => {}}
      />,
    ),
  );
  act(() => [...host.querySelectorAll<HTMLButtonElement>(".mode-switch button")].find((b) => b.textContent === "Agentic")!.click());
  const button = (await until(() => host.querySelector<HTMLButtonElement>(".project-pick")))!;
  await act(async () => button.click());
  const d = (await until(() => document.querySelector<HTMLDialogElement>("dialog.project-picker")))!;
  const section = (await until(() => d.querySelector<HTMLElement>('section[aria-labelledby="pp-repo"]')))!;
  return { host, d, section, unmount: () => act(() => root.unmount()) };
}

describe("From a repository", () => {
  test("no connection: one line and a link to Plugins → Connectors", async () => {
    conns = [{ id: "c0", host: "gone.example", allowed: false, login: "x" }];
    const c = await openRepos();
    const p = (await until(() => (c.section.textContent?.includes("No forge account") ? c.section : null)))!;
    const a = p.querySelector("a")!;
    expect(a.getAttribute("href")).toBe("#/plugins");
    expect(a.textContent).toContain("Plugins → Connectors");
    expect(listCalls()).toEqual([]);
    c.unmount();
  });

  test("one connection is used without asking; several are a select of host and login", async () => {
    let c = await openRepos();
    await until(() => c.section.querySelector('[role="option"]'));
    expect(c.section.querySelector("select")).toBeNull();
    expect(listCalls()[0]!.path).toBe("/api/connections/c1/repositories");
    c.unmount();
    document.body.innerHTML = "";
    calls = [];
    conns = [
      { id: "c1", host: "github.com", allowed: true, login: "ada" },
      { id: "c2", host: "git.example.org", allowed: true, login: "ada2" },
    ];
    c = await openRepos();
    const sel = (await until(() => c.section.querySelector<HTMLSelectElement>('select[aria-label="Forge account"]')))!;
    expect([...sel.options].map((o) => o.textContent)).toEqual(["github.com · @ada", "git.example.org · @ada2"]);
    await until(() => listCalls().length === 1);
    await set(sel, "c2");
    await until(() => listCalls().some((x) => x.path === "/api/connections/c2/repositories"));
    expect(listCalls().at(-1)!.path).toBe("/api/connections/c2/repositories");
    c.unmount();
  });

  test("the forge's text is inert, one line, cut, with hidden characters shown", async () => {
    conns = [{ id: "c1", host: "github.com", allowed: true, login: `${EVIL}${BIDI}` }];
    listings = () => ({
      status: 200,
      body: {
        repositories: [
          repo(1, { full_name: `ada/${EVIL}`, description: `${EVIL}\n${LINK}`, private: true }),
          repo(2, { full_name: `ada/${BIDI}`, description: LONG }),
          repo(3, { full_name: `ada/${LONG}`, description: LINK, push: false }),
        ],
        page: 1,
        has_more: false,
        left_out: 2,
        search: "none",
      },
    });
    const c = await openRepos();
    await until(() => c.section.querySelectorAll('[role="option"]').length === 3);
    const list = c.section.querySelector('[role="listbox"]')!;
    // Nothing made from the forge's text: no image, no link, no element but ours.
    expect(c.d.querySelector("img")).toBeNull();
    expect(list.querySelector("a")).toBeNull();
    expect((window as { pwned?: number }).pwned).toBeUndefined();
    const tags = new Set([...list.querySelectorAll("*")].map((e) => e.tagName.toLowerCase()));
    for (const t of tags) expect(["li", "span", "svg", "path", "rect"]).toContain(t);
    const rows = [...list.querySelectorAll<HTMLElement>('[role="option"]')];
    expect(rows[0]!.querySelector(".rp-name")!.textContent).toContain(EVIL);
    expect(rows[0]!.querySelector('[aria-label="private"]')).not.toBeNull();
    // One line: the line break is gone.
    expect(rows[0]!.querySelector(".rp-desc")!.textContent).not.toContain("\n");
    expect(rows[0]!.querySelector(".rp-desc")!.textContent).toContain("[click me](https://evil.example/x)");
    // Bidi and zero-width characters are shown, not obeyed.
    expect(rows[1]!.textContent).toContain("<U+202E>");
    expect(rows[1]!.textContent).toContain("<U+200B>");
    expect(rows[1]!.textContent).not.toContain("‮");
    // Cut: a 5,000-character description and name stay short; the title keeps the name.
    expect(rows[1]!.querySelector(".rp-desc")!.textContent!.length).toBeLessThanOrEqual(201);
    expect(rows[2]!.querySelector(".rp-name")!.textContent!.length).toBeLessThan(140);
    expect(rows[2]!.title.length).toBeLessThanOrEqual(513);
    expect(rows[2]!.textContent).toContain("This account cannot push to it.");
    expect(rows[0]!.textContent).not.toContain("cannot push");
    expect(c.section.textContent).toContain("Some repositories were left out because their names could not be shown safely.");
    c.unmount();
  });

  test("the search waits for the person to stop typing, cancels the request before and treats our 429 as loading", async () => {
    let gate: (() => void) | null = null;
    let busyOnce = true;
    listings = (path) => {
      if (path.includes("q=abc") && busyOnce) {
        busyOnce = false;
        return { status: 429, body: { error: "your earlier repository listing is still running; try again when it has finished" } };
      }
      if (path.includes("q=slow"))
        return () =>
          new Promise((r) => {
            gate = () => r({ status: 200, body: { repositories: [repo(7)], page: 1, has_more: false, left_out: 0, search: "forge" } });
          });
      return { status: 200, body: { repositories: [repo(1)], page: 1, has_more: false, left_out: 0, search: path.includes("q=") ? "forge" : "none" } };
    };
    const c = await openRepos();
    await until(() => listCalls().length === 1);
    const input = c.section.querySelector<HTMLInputElement>('input[aria-label="Search repositories"]')!;
    await set(input, "a");
    await set(input, "ab");
    await set(input, "abc");
    await new Promise((r) => setTimeout(r, 100));
    expect(listCalls().length).toBe(1);
    // After the pause: one request for the last word; its 429 is retried, never shown.
    await until(() => listCalls().filter((x) => x.path.includes("q=abc")).length === 2, 4000);
    expect(listCalls().filter((x) => x.path.includes("q=a&") || x.path.endsWith("q=a") || x.path.endsWith("q=ab"))).toEqual([]);
    expect(c.section.querySelector('[role="alert"]')).toBeNull();
    // A slow request is cancelled before the next is sent.
    await set(input, "slow");
    await until(() => listCalls().some((x) => x.path.includes("q=slow")));
    const slow = listCalls().find((x) => x.path.includes("q=slow"))!;
    await set(input, "slower2");
    await until(() => listCalls().some((x) => x.path.includes("q=slower2")));
    expect(slow.signal!.aborted).toBe(true);
    (gate as (() => void) | null)?.();
    c.unmount();
  });

  test("GitHub's within-page search is said in words and pages only when asked", async () => {
    listings = (path) => ({
      status: 200,
      body: {
        repositories: [repo(path.includes("page=2") ? 9 : 1)],
        page: path.includes("page=2") ? 2 : 1,
        has_more: true,
        left_out: 0,
        search: path.includes("q=") ? "within_page" : "none",
      },
    });
    const c = await openRepos();
    await until(() => listCalls().length === 1);
    expect(c.section.textContent).toContain("Show more");
    await set(c.section.querySelector<HTMLInputElement>('input[aria-label="Search repositories"]')!, "repo");
    await until(() => c.section.textContent?.includes("Searched the 100 most recently updated repositories"));
    await new Promise((r) => setTimeout(r, 400));
    expect(listCalls().filter((x) => x.path.includes("page=2"))).toEqual([]);
    const next = [...c.section.querySelectorAll("button")].find((b) => b.textContent === "Search the next 100")!;
    await act(async () => next.click());
    await until(() => c.section.textContent?.includes("Searched the 200 most recently updated repositories"));
    expect(listCalls().filter((x) => x.path.includes("page=2")).length).toBe(1);
    expect(c.section.querySelectorAll('[role="option"]').length).toBe(2);
    c.unmount();
  });

  test("an error is the server's sentence, in place, with Retry", async () => {
    let fail = true;
    const said = "github.com refused this connection's token (HTTP 401): it was revoked or has expired; replace the token under Plugins → Connectors";
    listings = () => (fail ? { status: 502, body: { error: said } } : { status: 200, body: { repositories: [repo(1)], page: 1, has_more: false, left_out: 0, search: "none" } });
    const c = await openRepos();
    const alert = (await until(() => c.section.querySelector('[role="alert"]')))!;
    expect(alert.textContent).toContain(said);
    fail = false;
    await act(async () => [...alert.querySelectorAll("button")].find((b) => b.textContent === "Retry")!.click());
    await until(() => c.section.querySelector('[role="option"]'));
    expect(c.section.querySelector('[role="alert"]')).toBeNull();
    c.unmount();
  });

  test("keys: arrows move, Enter picks, Escape leaves the search first", async () => {
    const c = await openRepos();
    await until(() => c.section.querySelectorAll('[role="option"]').length === 2);
    const input = c.section.querySelector<HTMLInputElement>('input[aria-label="Search repositories"]')!;
    const opts = [...c.section.querySelectorAll('[role="option"]')];
    expect(input.getAttribute("aria-activedescendant")).toBe(opts[0]!.id);
    await key(input, "ArrowDown");
    expect(input.getAttribute("aria-activedescendant")).toBe(opts[1]!.id);
    await key(input, "ArrowDown");
    expect(input.getAttribute("aria-activedescendant")).toBe(opts[0]!.id);
    await key(input, "ArrowUp");
    await key(input, "Enter");
    const name = (await until(() => c.section.querySelector<HTMLInputElement>('input[aria-label="Project name for the repository"]')))!;
    expect(name.value).toBe("repo-2");
    await set(input, "zz");
    const esc = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
    await act(async () => void input.dispatchEvent(esc));
    expect(esc.defaultPrevented).toBe(true);
    expect(input.value).toBe("");
    expect(document.querySelector("dialog.project-picker")).not.toBeNull();
    // Every control has a name.
    for (const el of c.section.querySelectorAll("input, select, button, [role=listbox]")) {
      const named = el.getAttribute("aria-label") || el.textContent?.trim();
      expect(named, el.outerHTML.slice(0, 80)).toBeTruthy();
    }
    c.unmount();
  });

  test("Create: the repository's name, the node, the connection; answered pending; shown cloning, not pickable", async () => {
    const c = await openRepos();
    await until(() => c.section.querySelectorAll('[role="option"]').length === 2);
    await act(async () => c.section.querySelector<HTMLElement>('[role="option"]')!.click());
    const name = (await until(() => c.section.querySelector<HTMLInputElement>('input[aria-label="Project name for the repository"]')))!;
    expect(name.value).toBe("repo-1");
    await set(name, "My clone");
    listBody.pending = [cloneAnswer.body as PendingProject];
    await act(async () => c.section.querySelector<HTMLButtonElement>('button[type="submit"]')!.click());
    const status = (await until(() => c.section.querySelector<HTMLElement>('[role="status"]')))!;
    expect(status.textContent).toContain("Cloning github.com/ada/repo-1…");
    expect(document.activeElement).toBe(status);
    const post = calls.find((x) => x.method === "POST")!;
    expect(post.body).toEqual({ name: "My clone", folder_on: "node-1", connection_id: "c1", full_name: "ada/repo-1", workspace_id: "w" });
    // The popup's list: cloning, a row that is no button.
    const row = (await until(() => [...c.d.querySelectorAll(".pp-list li")].find((li) => li.textContent?.includes("Cloning"))))!;
    expect(row.querySelector("button")).toBeNull();
    expect(row.textContent).toContain("Cloning github.com/ada/repo-1…");
    c.unmount();
  });

  test("Create while our own listing still holds the server's slot asks again", async () => {
    const busy = { status: 429, body: { error: "your earlier repository request is still running; try again when it has finished" } };
    const ok = cloneAnswer;
    let first = true;
    const c = await openRepos();
    await until(() => c.section.querySelector('[role="option"]'));
    await act(async () => c.section.querySelector<HTMLElement>('[role="option"]')!.click());
    await until(() => c.section.querySelector('button[type="submit"]'));
    cloneAnswer = busy;
    const real = globalThis.fetch;
    vi.stubGlobal("fetch", async (path: string, init: RequestInit) => {
      if (init.method === "POST" && first) {
        first = false;
        calls.push({ method: "POST", path, body: JSON.parse(init.body as string) });
        return new Response(JSON.stringify(busy.body), { status: 429, headers: { "content-type": "application/json" } });
      }
      cloneAnswer = ok;
      return real(path, init);
    });
    await act(async () => c.section.querySelector<HTMLButtonElement>('button[type="submit"]')!.click());
    const status = (await until(() => c.section.querySelector('[role="status"]')))!;
    expect(status.textContent).toContain("Cloning");
    expect(calls.filter((x) => x.method === "POST").length).toBe(2);
    expect(c.section.querySelector('[role="alert"]')).toBeNull();
    c.unmount();
  });

  test("a refused Create says the server's sentence", async () => {
    cloneAnswer = { status: 409, body: { error: "github.com/ada/repo-1 is being cloned onto this node for you already" } };
    const c = await openRepos();
    await until(() => c.section.querySelector('[role="option"]'));
    await act(async () => c.section.querySelector<HTMLElement>('[role="option"]')!.click());
    await until(() => c.section.querySelector('button[type="submit"]'));
    await act(async () => c.section.querySelector<HTMLButtonElement>('button[type="submit"]')!.click());
    const alert = (await until(() => [...c.section.querySelectorAll('[role="alert"]')].find((a) => a.textContent?.includes("already"))))!;
    expect(alert.textContent).toContain("is being cloned onto this node for you already");
    c.unmount();
  });

  test("a failed clone shows its reason, with Dismiss for its starter only", async () => {
    const base = cloneAnswer.body as PendingProject;
    listBody.pending = [
      { ...base, project_id: "f1", state: "failed", reason: `this node does not clone from that host ${EVIL}` },
      { ...base, project_id: "f2", name: "Theirs", mine: false },
    ];
    const c = await openRepos();
    const rows = (await until(() => {
      const r = [...c.d.querySelectorAll(".pp-list li")];
      return r.length === 2 ? r : null;
    }))!;
    expect(rows[0]!.textContent).toContain("Clone failed: this node does not clone from that host");
    expect(c.d.querySelector("img")).toBeNull();
    expect(rows[1]!.textContent).toContain("Cloning");
    expect(rows[1]!.querySelector("button")).toBeNull();
    await act(async () => [...rows[0]!.querySelectorAll("button")].find((b) => b.textContent === "Dismiss")!.click());
    await until(() => calls.some((x) => x.method === "DELETE"));
    expect(calls.find((x) => x.method === "DELETE")!.path).toBe("/api/projects/f1/clone");
    c.unmount();
  });
});

describe("the sidebar's projects", () => {
  test("a pending project is drawn as cloning, never a link; a failed one of mine has Dismiss", async () => {
    const { SidebarLists } = await import("./ProjectNav");
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    act(() =>
      root.render(
        <SidebarLists route={{ kind: "home" }} onNavigate={() => {}} sessions={[]} nodes={[]} projects={[]} canCreateProject />,
      ),
    );
    const base = cloneAnswer.body as PendingProject;
    act(() =>
      pendingProjects.set([
        { ...base, name: EVIL },
        { ...base, project_id: "f1", state: "failed", reason: "the clone took longer than this node allows" },
      ]),
    );
    const rows = [...host.querySelectorAll<HTMLElement>(".pending-project")];
    expect(rows.length).toBe(2);
    expect(rows[0]!.textContent).toContain("cloning github.com/ada/repo-1…");
    expect(rows[0]!.querySelector("a, img")).toBeNull();
    expect(rows[0]!.querySelector("button")).toBeNull();
    expect(rows[1]!.textContent).toContain("clone failed: the clone took longer");
    expect(host.textContent).not.toContain("No projects");
    await act(async () => rows[1]!.querySelector("button")!.click());
    await until(() => calls.some((x) => x.method === "DELETE"));
    expect(calls.find((x) => x.method === "DELETE")!.path).toBe("/api/projects/f1/clone");
    act(() => root.unmount());
  });
});
