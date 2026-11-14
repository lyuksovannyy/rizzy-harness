// @vitest-environment happy-dom
// The Routines page against a mocked API: the list (trigger line, mode,
// next run, last status, the unseen dot, Not running with delete only, Run
// now, the switch, Open latest result), the empty page and its templates,
// the editor (what it saves, the schedule/webhook choice, the variable
// picker, the preview's chats-per-day note, the project, the webhook card
// with its sample event) and the Runs tab's columns.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { Live } from "../live";
import type { ProjectSummary } from "../lib/projects";
import type { Preview, Routine, RoutineList, Run, RunDetail } from "../lib/routines";
import type { Route } from "../lib/routes";
import type { Agent, Features, Me, NodeInfo } from "../types";

const routinesApi = {
  list: vi.fn<(ws: string | null) => Promise<RoutineList>>(),
  get: vi.fn<(id: string) => Promise<Routine>>(),
  create: vi.fn(),
  update: vi.fn(),
  remove: vi.fn(),
  run: vi.fn(),
  runs: vi.fn<(id: string, before?: string | null) => Promise<{ runs: Run[]; next: string | null }>>(),
  run_detail: vi.fn<(id: string, run: string) => Promise<RunDetail>>(),
  preview: vi.fn<(b: unknown) => Promise<Preview>>(),
  rotate: vi.fn<(id: string, tid: string) => Promise<{ secret: string }>>(),
};
vi.mock("../lib/routines", async (original) => ({ ...(await original<typeof import("../lib/routines")>()), routinesApi }));
const call = vi.fn((..._a: unknown[]) => Promise.resolve({ ok: true }));
vi.mock("../api", () => ({
  api: { settings: () => Promise.resolve({ timezone: "Europe/Riga" }), models: () => Promise.resolve({ models: [] }) },
  call: (...a: unknown[]) => call(...a),
  HttpError: class extends Error {
    constructor(
      public status: number,
      m: string,
    ) {
      super(m);
    }
  },
}));
const { default: Routines } = await import("./Routines");
const { localOnly } = await import("../lib/routines");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const TZ = "Europe/Riga";
const agent = (id: string): Agent => ({
  id,
  name: id[0]!.toUpperCase() + id.slice(1),
  description: "",
  builtin: true,
  default_model: null,
  main_model: "m",
  fallbacks: [],
  unavailable: [],
  avatar: { shape: "circle", eyes: "round", accent: "blue" },
});
const AGENTS = [agent("general"), agent("coder")];
const NODES: NodeInfo[] = [{ node_id: "n1", hostname: "box", version: "1", connected: true, embedded: false, profile: "agentic" }];
const me: Me = { user_id: "u1", username: "alex", role: "member", can_agentic: true, must_change_password: false };
const features = (webhooks = false): Features => ({
  voice: false,
  memory: false,
  routines: { enabled: true, show: true, webhooks, why: null },
  media: { enabled: false, show: false, max_file: 0, vision_images: false },
  artifacts: { enabled: false, show: false, why: null },
  search: { kinds: ["chats", "projects"] },
});

const run = (over: Partial<Run> = {}): Run => ({
  run_id: "run1",
  routine_id: "r1",
  trigger_kind: "schedule",
  scheduled_ms: null,
  started_ms: Date.now() - 3_600_000,
  ended_ms: Date.now() - 3_540_000,
  status: "completed",
  reason: null,
  session_id: "11111111-1111-1111-1111-111111111111",
  chat_deleted: false,
  usage: { input_tokens: 1000, output_tokens: 234 },
  warnings: [],
  untrusted: false,
  attempt: 1,
  repeats: 1,
  seen: true,
  answer_preview: "All quiet today.",
  ...over,
});
const routine = (over: Partial<Routine> = {}): Routine => ({
  routine_id: "r1",
  workspace_id: "w1",
  name: "Morning briefing",
  enabled: true,
  paused_reason: null,
  spec: {
    profile: "chat",
    agent_id: "general",
    model: null,
    effort: null,
    prompt: "Plan my {{now.weekday}}",
    project_id: null,
    catch_up: "skip",
    on_missing: "empty",
    notify: "failures",
    max_runtime_min: 120,
  },
  triggers: [{ trigger_id: "t1", kind: "schedule", config: { type: "cron", expr: "30 8 * * 1-5", tz: TZ }, next_run_ms: null, last_fired_ms: null }],
  next_run_ms: Date.now() + 5 * 60_000,
  last_run: run(),
  unseen: 0,
  blocked: null,
  event_paths: [],
  created_ms: 0,
  updated_ms: 0,
  ...over,
});
const LIST = (over: Partial<RoutineList> = {}): RoutineList => ({
  routines: [],
  workspace_id: "w1",
  can_create: true,
  can_agentic: true,
  why: null,
  limits: { max_triggers: 8 },
  variables: [
    { namespace: "now", vars: [{ path: "now.date", example: "2026-11-08" }, { path: "now.weekday", example: "Sunday" }] },
    { namespace: "user", vars: [{ path: "user.name", example: "alex" }] },
  ],
  sources: [{ kind: "webhook", vars: [{ path: "event.text", example: "" }], free_prefix: "event.body" }],
  orphaned: false,
  ...over,
});
const PREVIEW: Preview = { next_runs: [], chats_per_day: null, rendered: null, untrusted: false, warnings: [], errors: [], event_paths: [] };

let host: HTMLElement;
let root: Root;
const navigate = vi.fn<(r: Route) => void>();
const live = { onTopic: vi.fn(() => () => {}) } as unknown as Live;
beforeEach(() => {
  for (const f of Object.values(routinesApi)) f.mockReset();
  routinesApi.preview.mockResolvedValue(PREVIEW);
  navigate.mockReset();
  call.mockClear();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
});

async function show(
  id: string | null,
  opts: { tab?: "runs"; webhooks?: boolean; projects?: ProjectSummary[]; agentDefaults?: { chat: string; agentic: string | null } } = {},
) {
  await act(async () =>
    root.render(
      <Routines
        me={me}
        scope="w1"
        navigate={navigate}
        live={live}
        features={features(opts.webhooks)}
        id={id}
        tab={opts.tab}
        agents={AGENTS}
        nodes={NODES}
        projects={opts.projects ?? []}
        agentDefaults={opts.agentDefaults ?? { chat: "general", agentic: "coder" }}
      />,
    ),
  );
}
const settle = () => act(() => new Promise((r) => setTimeout(r, 450)));
const buttons = () => [...host.querySelectorAll<HTMLButtonElement>("button")];
const button = (text: string) => buttons().find((b) => b.textContent?.trim() === text || b.getAttribute("aria-label") === text);
function type(el: HTMLInputElement | HTMLTextAreaElement, value: string) {
  const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  act(() => {
    Object.getOwnPropertyDescriptor(proto, "value")!.set!.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
function choose(el: HTMLSelectElement, value: string) {
  act(() => {
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")!.set!.call(el, value);
    el.dispatchEvent(new Event("change", { bubbles: true }));
  });
}
const radio = (group: string, label: string) =>
  [...host.querySelector(`[role="radiogroup"][aria-label="${group}"]`)!.querySelectorAll<HTMLButtonElement>('[role="radio"]')].find(
    (b) => b.textContent === label,
  )!;

describe("list", () => {
  test("each row: trigger line, mode, next run, last status, unseen dot; Not running apart", async () => {
    routinesApi.list.mockResolvedValue(
      LIST({
        routines: [
          routine({ unseen: 2 }),
          routine({
            routine_id: "r2",
            name: "Repo check",
            spec: { ...routine().spec, profile: "agentic", node_id: "n1", cwd: "/src", approval_mode: "ask" },
            last_run: run({ status: "error", reason: "node offline" }),
          }),
          routine({ routine_id: "r3", name: "Paused one", paused_reason: "failures" }),
          routine({ routine_id: "r4", name: "Guest left", blocked: "Guests can't run routines in Acme" }),
        ],
      }),
    );
    await show(null);
    const rows = [...host.querySelectorAll<HTMLLIElement>(".routine-item")];
    expect(rows.map((r) => r.querySelector(".agent-name")!.textContent)).toEqual(["Morning briefing", "Repo check", "Paused one", "Guest left"]);
    const meta = rows.map((r) => r.querySelector(".routine-meta")!.textContent);
    expect(meta[0]).toMatch(/^Weekdays at 08:30ChatNext in \d+mLast: Finished 1h ago$/);
    expect(meta[1]).toContain("Agentic");
    expect(meta[1]).toContain("box");
    expect(rows[1]!.querySelector(".routine-meta .routine-bad")!.textContent).toMatch(/^Last: Failed/);
    expect(rows[0]!.querySelector(".dot.unseen")!.getAttribute("aria-label")).toBe("2 new results");
    // Not running: its own section, with the reason; a blocked one can only be deleted.
    expect(host.querySelector(".routine-section")!.textContent).toBe("Not running");
    expect(rows[2]!.textContent).toContain("Paused after failing several runs in a row.");
    expect(rows[2]!.querySelector("button.agent-open")).not.toBeNull();
    expect(rows[3]!.textContent).toContain("Guests can't run routines in Acme");
    expect(rows[3]!.querySelector("button.agent-open")).toBeNull();
    expect([...rows[3]!.querySelectorAll("button")].map((b) => b.textContent)).toEqual(["Delete"]);
  });

  test("Run now, the switch and Open latest result", async () => {
    routinesApi.list.mockResolvedValue(LIST({ routines: [routine(), routine({ routine_id: "r2", name: "B", last_run: run({ session_id: null }) })] }));
    routinesApi.run.mockResolvedValue({ run_id: "new" });
    routinesApi.update.mockResolvedValue({ routine: routine({ enabled: false }), secrets: {} });
    await show(null);
    const [a, b] = [...host.querySelectorAll<HTMLLIElement>(".routine-item")];
    await act(async () => [...a!.querySelectorAll("button")].find((x) => x.textContent === "Run now")!.click());
    expect(routinesApi.run).toHaveBeenCalledWith("r1");
    expect(a!.querySelector('[role="status"]')!.textContent).toBe("Started. Its chat appears in Recent.");
    await act(async () => a!.querySelector<HTMLInputElement>("input.switch")!.click());
    expect(routinesApi.update).toHaveBeenCalledWith("r1", { enabled: false });
    act(() => [...a!.querySelectorAll("button")].find((x) => x.textContent === "Open latest result")!.click());
    expect(navigate).toHaveBeenLastCalledWith({ kind: "session", id: "11111111-1111-1111-1111-111111111111" });
    // A run without a chat: its Runs tab.
    act(() => [...b!.querySelectorAll("button")].find((x) => x.textContent === "Open latest result")!.click());
    expect(navigate).toHaveBeenLastCalledWith({ kind: "routines", id: "r2", tab: "runs" });
    // The row itself opens the editor.
    act(() => a!.querySelector<HTMLButtonElement>("button.agent-open")!.click());
    expect(navigate).toHaveBeenLastCalledWith({ kind: "routines", id: "r1" });
  });

  test("empty: three templates that open the editor filled in; a guest reads why instead", async () => {
    routinesApi.list.mockResolvedValue(LIST());
    await show(null);
    expect(host.textContent).toContain("No routines yet.");
    const templates = [...host.querySelectorAll<HTMLButtonElement>(".routine-templates button")];
    expect(templates.map((t) => t.querySelector(".routine-name")!.textContent)).toEqual(["Morning briefing", "Weekly review", "Monthly check-in"]);
    act(() => templates[1]!.click());
    expect(navigate).toHaveBeenLastCalledWith({ kind: "routines", id: "new" });
    await show("new");
    expect(host.querySelector<HTMLInputElement>("form input")!.value).toBe("Weekly review");
    expect(host.querySelector<HTMLSelectElement>('select[aria-label="Repeats"]')!.value).toBe("weekly");

    routinesApi.list.mockResolvedValue(LIST({ can_create: false, why: "Guests can't run routines in Acme" }));
    act(() => root.unmount());
    root = createRoot(host);
    await show(null);
    expect(host.querySelector(".routine-empty")!.textContent).toBe("Guests can't run routines in Acme");
    expect(button("New routine")).toBeUndefined();
  });
});

describe("editor", () => {
  test("a new chat routine saves its name, spec and a weekday schedule in the user's zone", async () => {
    routinesApi.list.mockResolvedValue(LIST());
    routinesApi.create.mockResolvedValue({ routine: routine({ routine_id: "r9" }), secrets: {} });
    await show("new");
    type(host.querySelector<HTMLInputElement>("form input")!, "Digest");
    type(host.querySelector<HTMLTextAreaElement>("textarea")!, "Summarise the news");
    await act(async () => button("Create routine")!.click());
    const body = routinesApi.create.mock.calls[0]![0];
    expect(body).toMatchObject({
      workspace_id: "w1",
      name: "Digest",
      enabled: true,
      spec: { profile: "chat", agent_id: "general", model: null, prompt: "Summarise the news", notify: "failures" },
      triggers: [{ kind: "schedule", config: { type: "cron", expr: "30 8 * * 1-5", tz: TZ } }],
    });
    expect(body.spec).not.toHaveProperty("node_id");
    expect(navigate).toHaveBeenLastCalledWith({ kind: "routines", id: "r9" });
  });

  test("agentic needs the permission; with it, the node, folder and approvals are saved", async () => {
    routinesApi.list.mockResolvedValue(LIST({ can_agentic: false }));
    await show("new");
    expect(radio("Runs as", "Agentic").disabled).toBe(true);
    expect(host.textContent).toContain("You can't run agentic chats in this workspace.");

    routinesApi.list.mockResolvedValue(LIST());
    routinesApi.create.mockResolvedValue({ routine: routine(), secrets: {} });
    act(() => root.unmount());
    root = createRoot(host);
    await show("new");
    act(() => radio("Runs as", "Agentic").click());
    expect(host.textContent).toContain("A run waiting for you is cancelled after 24 hours.");
    type(host.querySelector<HTMLInputElement>("form input")!, "Check");
    await act(async () => button("Create routine")!.click());
    expect(host.querySelector('[role="alert"]')!.textContent).toBe("Pick a node and a folder for an agentic routine.");
    type(host.querySelector<HTMLInputElement>('input[placeholder="/home/me/project"]')!, "/src/app");
    act(() => radio("Approvals", "Ask before every tool").click());
    await act(async () => button("Create routine")!.click());
    expect(routinesApi.create.mock.calls[0]![0].spec).toMatchObject({ profile: "agentic", node_id: "n1", cwd: "/src/app", approval_mode: "ask_all" });
  });

  // TD-1: a blank draft follows the mode's default agent until one is picked.
  test("switched to Agentic, a blank draft takes the Agentic default; a picked agent stays", async () => {
    routinesApi.list.mockResolvedValue(LIST());
    routinesApi.create.mockResolvedValue({ routine: routine(), secrets: {} });
    await show("new");
    const agentButton = () => host.querySelector<HTMLButtonElement>('button[aria-label^="Agent: "]')!;
    expect(agentButton().getAttribute("aria-label")).toBe("Agent: General");
    act(() => radio("Runs as", "Agentic").click());
    expect(agentButton().getAttribute("aria-label")).toBe("Agent: Coder");
    act(() => radio("Runs as", "Chat").click());
    expect(agentButton().getAttribute("aria-label")).toBe("Agent: General");
    act(() => radio("Runs as", "Agentic").click());
    type(host.querySelector<HTMLInputElement>("form input")!, "Check");
    type(host.querySelector<HTMLInputElement>('input[placeholder="/home/me/project"]')!, "/src/app");
    await act(async () => button("Create routine")!.click());
    expect(routinesApi.create.mock.calls[0]![0].spec).toMatchObject({ profile: "agentic", agent_id: "coder" });

    // Picked first: switching the mode leaves the pick alone.
    act(() => root.unmount());
    root = createRoot(host);
    await show("new");
    act(() => agentButton().click());
    act(() => [...host.querySelectorAll<HTMLElement>('[role="option"]')].find((o) => o.textContent?.includes("General"))!.click());
    act(() => radio("Runs as", "Agentic").click());
    expect(agentButton().getAttribute("aria-label")).toBe("Agent: General");
  });

  test("the webhook choice shows only where the server takes webhooks, and saves one webhook", async () => {
    routinesApi.list.mockResolvedValue(LIST());
    await show("new");
    expect(host.querySelector('[aria-label="When it runs"]')).toBeNull();
    routinesApi.create.mockResolvedValue({ routine: routine(), secrets: {} });
    await show("new", { webhooks: true });
    act(() => radio("When it runs", "When a webhook is called").click());
    expect(host.querySelector(".schedule-builder")).toBeNull();
    expect(host.textContent).toContain("The URL and its secret appear once you save.");
    type(host.querySelector<HTMLInputElement>("form input")!, "Hook");
    await act(async () => button("Create routine")!.click());
    expect(routinesApi.create.mock.calls[0]![0].triggers).toEqual([{ kind: "webhook", config: {} }]);
  });

  test("a saved webhook shows its URL, and its secret once", async () => {
    const hooked = routine({
      triggers: [{ trigger_id: "h1", kind: "webhook", config: {}, next_run_ms: null, last_fired_ms: null, url_path: "/api/hooks/h1", secret_set: true }],
      event_paths: ["user.name"],
    });
    routinesApi.list.mockResolvedValue(LIST({ routines: [hooked] }));
    routinesApi.update.mockResolvedValue({ routine: hooked, secrets: { h1: "s3cret" } });
    await show("r1", { webhooks: true });
    expect(host.querySelector(".routine-hook code")!.textContent).toBe(`${location.origin}/api/hooks/h1`);
    expect(host.textContent).not.toContain("s3cret");
    await act(async () => button("Save")!.click());
    expect(routinesApi.update.mock.calls[0]![1].triggers).toEqual([{ trigger_id: "h1", kind: "webhook", config: {} }]);
    expect(host.textContent).toContain("s3cret");
  });

  test("the webhook card: a curl line, Rotate shows a new secret once, a warning on localhost", async () => {
    const hooked = routine({
      triggers: [{ trigger_id: "h1", kind: "webhook", config: {}, next_run_ms: null, last_fired_ms: null, url_path: "/api/hooks/h1", secret_set: true }],
    });
    routinesApi.list.mockResolvedValue(LIST({ routines: [hooked] }));
    routinesApi.rotate.mockResolvedValue({ secret: "n3w-secret" });
    vi.stubGlobal("confirm", () => true);
    await show("r1", { webhooks: true });
    const card = host.querySelector(".routine-hook")!;
    expect(card.querySelector("pre")!.textContent).toContain(`curl -X POST '${location.origin}/api/hooks/h1'`);
    expect(card.querySelector("pre")!.textContent).toContain('"Authorization: Bearer $HOOK_SECRET"');
    expect(card.textContent).toContain("A secret is set. Rotate it to see a new one.");
    expect(card.textContent!.includes("This address works only on this computer.")).toBe(localOnly(location.hostname));
    await act(async () => button("Rotate")!.click());
    expect(routinesApi.rotate).toHaveBeenCalledWith("r1", "h1");
    expect(card.textContent).toContain("n3w-secret");
    expect(card.querySelector("pre")!.textContent).toContain("'Authorization: Bearer n3w-secret'");
  });

  test("the sample event feeds the preview, the picker and Run now", async () => {
    const hooked = routine({
      triggers: [{ trigger_id: "h1", kind: "webhook", config: {}, next_run_ms: null, last_fired_ms: null, url_path: "/api/hooks/h1", secret_set: true }],
      event_paths: ["event.body.repo", "issue.title"],
    });
    routinesApi.list.mockResolvedValue(LIST({ routines: [hooked] }));
    routinesApi.preview.mockResolvedValue({ ...PREVIEW, event_paths: ["event.body.issue.title", "event.body.issue.number"], untrusted: true });
    routinesApi.run.mockResolvedValue({ run_id: "x" });
    await show("r1", { webhooks: true });
    const box = host.querySelector<HTMLTextAreaElement>(".routine-hook textarea")!;
    type(box, "{oops");
    expect(host.querySelector(".routine-hook")!.textContent).toContain("Not valid JSON yet.");
    type(box, '{"issue": {"title": "Login fails", "number": 42}}');
    await settle();
    expect(routinesApi.preview.mock.calls.at(-1)![0]).toMatchObject({ routine_id: "r1", sample_event: { issue: { title: "Login fails", number: 42 } } });
    expect(host.textContent).toContain("With webhook data, the run is untrusted");
    expect(host.querySelector(".routine-hook pre")!.textContent).toContain(`-d '{"issue":{"title":"Login fails","number":42}}'`);
    // The sample's paths first, then the last call's, each once.
    act(() => button("Insert a variable")!.click());
    const hooks = [...host.querySelectorAll('[role="group"][aria-label="From the webhook"] [role="menuitem"] .mono')].map((e) => e.textContent);
    expect(hooks).toEqual(["{{ event.text }}", "{{ event.body.issue.title }}", "{{ event.body.issue.number }}", "{{ event.body.repo }}"]);
    await act(async () => button("Run now")!.click());
    expect(routinesApi.run).toHaveBeenCalledWith("r1", { issue: { title: "Login fails", number: 42 } });
  });

  test("the sample event stays in this browser across visits; emptied, it is forgotten", async () => {
    const hooked = routine({
      triggers: [{ trigger_id: "h1", kind: "webhook", config: {}, next_run_ms: null, last_fired_ms: null, url_path: "/api/hooks/h1", secret_set: true }],
    });
    routinesApi.list.mockResolvedValue(LIST({ routines: [hooked] }));
    routinesApi.preview.mockResolvedValue(PREVIEW);
    localStorage.removeItem("routine-sample:r1");
    await show("r1", { webhooks: true });
    const sample = '{"issue": {"number": 7}}';
    type(host.querySelector<HTMLTextAreaElement>(".routine-hook textarea")!, sample);
    expect(localStorage.getItem("routine-sample:r1")).toBe(sample);
    await show(null);
    await show("r1", { webhooks: true });
    const box = host.querySelector<HTMLTextAreaElement>(".routine-hook textarea")!;
    expect(box.value).toBe(sample);
    type(box, "");
    expect(localStorage.getItem("routine-sample:r1")).toBeNull();
  });

  test("a run's chat can land in a project; preview errors not about the prompt carry no offset", async () => {
    const project = { project_id: "p".repeat(32), workspace_id: "w1", name: "Website", description: "", icon: "", accent: "", chats: 0, updated_ms: 0 };
    routinesApi.list.mockResolvedValue(LIST());
    routinesApi.create.mockResolvedValue({ routine: routine(), secrets: {} });
    routinesApi.preview.mockResolvedValue({ ...PREVIEW, errors: [{ offset: null, message: "unknown time zone Mars/Base" }] });
    await show("new");
    expect(host.textContent).not.toContain("No project");
    await show("new", { projects: [project] });
    await settle();
    expect(host.textContent).toContain("unknown time zone Mars/Base");
    expect(host.textContent).not.toContain("At character");
    const select = [...host.querySelectorAll<HTMLSelectElement>("select")].find((s) => s.querySelector('option[value=""]')?.textContent === "No project")!;
    choose(select, project.project_id);
    type(host.querySelector<HTMLInputElement>("form input")!, "Site check");
    await act(async () => button("Create routine")!.click());
    expect(routinesApi.create.mock.calls[0]![0].spec.project_id).toBe(project.project_id);
  });

  test("the picker lists this routine's variables and inserts one at the cursor", async () => {
    routinesApi.list.mockResolvedValue(LIST());
    await show("new", { webhooks: true });
    const ta = host.querySelector<HTMLTextAreaElement>("textarea")!;
    type(ta, "Hello , today");
    ta.setSelectionRange(6, 6);
    act(() => button("Insert a variable")!.click());
    const groups = () => [...host.querySelectorAll('[role="menu"] [role="group"]')].map((g) => g.getAttribute("aria-label"));
    // A schedule routine has no webhook data.
    expect(groups()).toEqual(["Time", "You"]);
    act(() => [...host.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')].find((b) => b.textContent!.startsWith("{{ user.name }}"))!.click());
    expect(ta.value).toBe("Hello {{ user.name }}, today");
    expect(host.querySelector('[role="menu"]')).toBeNull();
    // A webhook routine adds what the webhook sends, and the last body's paths.
    act(() => radio("When it runs", "When a webhook is called").click());
    act(() => button("Insert a variable")!.click());
    expect(groups()).toEqual(["Time", "You", "From the webhook"]);
    // Esc closes it and gives the prompt back its focus.
    act(() => host.querySelector('[role="menu"]')!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
    expect(host.querySelector('[role="menu"]')).toBeNull();
    expect(document.activeElement).toBe(ta);
  });

  test("the server's preview: next runs, chats per day, template errors", async () => {
    routinesApi.list.mockResolvedValue(LIST());
    routinesApi.preview.mockResolvedValue({
      ...PREVIEW,
      next_runs: [Date.UTC(2026, 10, 9, 6, 30)],
      chats_per_day: 2.6,
      errors: [{ offset: 4, message: "unknown namespace 'foo'" }],
      rendered: "Plan Monday",
    });
    await show("new");
    await settle();
    expect(routinesApi.preview.mock.calls.at(-1)![0]).toMatchObject({ workspace_id: "w1", triggers: [{ kind: "schedule" }] });
    expect(host.querySelector(".schedule-chats")!.textContent).toBe("Creates about 3 chats per day.");
    expect(host.querySelector(".schedule-next")!.textContent).toBe("Next runs: Mon 9 Nov, 08:30");
    expect(host.textContent).toContain("At character 5: unknown namespace 'foo'");
    expect(host.querySelector(".routine-rendered pre")!.textContent).toBe("Plan Monday");
  });

  test("the schedule builder switches shapes and adds schedules", async () => {
    routinesApi.list.mockResolvedValue(LIST());
    await show("new");
    const repeats = () => [...host.querySelectorAll<HTMLSelectElement>('select[aria-label="Repeats"]')];
    choose(repeats()[0]!, "every");
    expect(host.querySelector<HTMLInputElement>('input[aria-label="Every"]')!.value).toBe("30");
    act(() => button("Add a schedule")!.click());
    expect(repeats()).toHaveLength(2);
    choose(repeats()[1]!, "weekly");
    const days = [...host.querySelectorAll<HTMLButtonElement>('[aria-label="Days"] button')];
    expect(days.filter((d) => d.getAttribute("aria-pressed") === "true").map((d) => d.textContent)).toEqual(["Mon"]);
    act(() => days[2]!.click());
    await settle();
    expect(routinesApi.preview.mock.calls.at(-1)![0]).toMatchObject({
      triggers: [
        { kind: "schedule", config: { type: "interval", every_minutes: 30 } },
        { kind: "schedule", config: { type: "cron", expr: "0 9 * * 1,3", tz: TZ } },
      ],
    });
    act(() => button("Remove schedule 1")!.click());
    expect(repeats()).toHaveLength(1);
  });
});

describe("runs", () => {
  test("columns: started (and when due), trigger, status ×N, duration, tokens, result, chat", async () => {
    const due = Date.UTC(2026, 10, 8, 6, 30);
    routinesApi.list.mockResolvedValue(LIST({ routines: [routine()] }));
    routinesApi.runs.mockResolvedValue({
      runs: [
        run({ run_id: "a", seen: false, trigger_kind: "manual" }),
        run({ run_id: "b", trigger_kind: "schedule", scheduled_ms: due, started_ms: due + 5 * 60_000, ended_ms: due + 5 * 60_000 + 45_000 }),
        run({ run_id: "c", status: "skipped", repeats: 3, session_id: null, usage: null, answer_preview: null, ended_ms: null }),
        run({ run_id: "d", chat_deleted: true, session_id: null, trigger_kind: "webhook" }),
      ],
      next: "cursor",
    });
    routinesApi.run_detail.mockResolvedValue({ ...run({ run_id: "a" }), prompt: "Plan Monday", event: { body: { x: 1 } } });
    await show("r1", { tab: "runs" });
    expect([...host.querySelectorAll(".runs-table th")].map((t) => t.textContent)).toEqual([
      "Started",
      "Trigger",
      "Status",
      "Duration",
      "Tokens",
      "Result",
      "Chat",
    ]);
    const rows = [...host.querySelectorAll<HTMLTableRowElement>(".runs-table tbody tr")];
    const cell = (r: number, label: string) => rows[r]!.querySelector(`td[data-label="${label}"]`)!.textContent;
    expect(rows.map((r) => r.className)).toEqual(["unseen", "", "", ""]);
    expect(cell(0, "Trigger")).toBe("Run now");
    expect(cell(0, "Tokens")).toBe("1.2k");
    expect(cell(0, "Result")).toBe("All quiet today.");
    expect(cell(1, "Started")).toContain("scheduled 08:30");
    expect(cell(1, "Duration")).toBe("45s");
    expect(cell(2, "Status")).toBe("Skipped ×3");
    expect(cell(2, "Duration")).toBe("—");
    expect(cell(3, "Chat")).toBe("chat deleted");
    expect(cell(3, "Trigger")).toBe("Webhook");
    // Opening an unseen run's chat counts as seeing it.
    act(() => rows[0]!.querySelector<HTMLButtonElement>('td[data-label="Chat"] button')!.click());
    expect(call).toHaveBeenCalledWith("POST", "/api/routines/r1/runs/a/seen");
    expect(navigate).toHaveBeenLastCalledWith({ kind: "session", id: "11111111-1111-1111-1111-111111111111" });
    // A row opens to its prompt and event, as plain text.
    await act(async () => rows[0]!.querySelector<HTMLButtonElement>('td[data-label="Started"] button')!.click());
    const detail = host.querySelector(".run-detail")!;
    expect(detail.textContent).toContain("Plan Monday");
    expect(detail.textContent).toContain("Event, from an external event");
    expect(detail.querySelectorAll("pre")[1]!.textContent).toBe(JSON.stringify({ body: { x: 1 } }, null, 2));
    // More by cursor.
    routinesApi.runs.mockResolvedValue({ runs: [run({ run_id: "e" })], next: null });
    await act(async () => button("Load more")!.click());
    expect(routinesApi.runs).toHaveBeenLastCalledWith("r1", "cursor");
    expect(host.querySelectorAll(".runs-table tbody tr:not(.run-detail)")).toHaveLength(5);
  });
});
