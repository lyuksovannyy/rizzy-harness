// @vitest-environment happy-dom
// Settings → Profile and Personalisation, and Workspace settings → Agents
// (PLAN.md T74): the profile's fields as the sketch lays them out, one Save,
// the server's message under the field it names, the description's limit in
// characters; the avatar and banner picked (made small first), replaced and
// removed; Personalisation's five choices, its counter and limit, its own
// save; the workspace's agents as the Agents page lists them, with edit
// actions only for those who may edit; markup in a name stays text.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { Agent, Me, Prefs, WorkspaceSummary } from "../types";

const pageApi = {
  saveSettings: vi.fn(),
  savePersonalisation: vi.fn(),
  profileImages: vi.fn(),
  setProfileImage: vi.fn(),
  removeProfileImage: vi.fn(),
  deleteAgent: vi.fn(),
  agent: vi.fn(),
};
const api = { agents: vi.fn() };
vi.mock("../apiPages", () => ({ pageApi }));
vi.mock("../api", () => ({ api }));
const shrinkPicture = vi.fn();
vi.mock("./profile/pictures", async (orig) => ({ ...(await orig<typeof import("./profile/pictures")>()), shrinkPicture }));
vi.mock("../lib/greeting", () => ({ browserTimeZone: () => "Europe/Riga" }));

const { default: ProfileTab } = await import("./profile/ProfileTab");
const { default: PersonalisationTab } = await import("./profile/PersonalisationTab");
const { default: WorkspaceAgents } = await import("./workspace/WorkspaceAgents");
const { frame } = await import("./profile/pictures");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const me: Me = { user_id: "u1", username: "alex", role: "member", exec_ceiling: "none", must_change_password: false };
const prefs: Prefs = {
  search_url: "https://e.com/?q={q}",
  default_agent: null,
  default_model: null,
  display_name: "Alex",
  country: "Latvia",
  city: "Riga",
  timezone: "Europe/Riga",
  description: null,
  personalisation: {},
};

let host: HTMLElement;
let root: Root;
beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  for (const f of [...Object.values(pageApi), ...Object.values(api), shrinkPicture]) f.mockReset();
  pageApi.profileImages.mockResolvedValue({ avatar: null, banner: null });
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
});

function type(el: HTMLInputElement | HTMLTextAreaElement, value: string) {
  const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  act(() => {
    Object.getOwnPropertyDescriptor(proto, "value")!.set!.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
const labels = () => [...host.querySelectorAll<HTMLLabelElement>("label.field-label")].map((l) => l.textContent?.replace("(optional)", "").trim());
const field = (label: string) =>
  document.getElementById([...host.querySelectorAll<HTMLLabelElement>("label.field-label")].find((l) => l.textContent?.startsWith(label))!.htmlFor) as HTMLInputElement &
    HTMLTextAreaElement;
const button = (name: string) =>
  [...host.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent?.trim() === name || b.getAttribute("aria-label") === name);

async function profile(p: Prefs = prefs) {
  const onSaved = vi.fn();
  await act(async () => root.render(<ProfileTab me={me} prefs={p} models={[]} onSaved={onSaved} />));
  return onSaved;
}

describe("Profile", () => {
  test("banner, avatar, then Name, Country, City and Time zone, About you, and one Save", async () => {
    await profile();
    expect(button("Add banner")).toBeDefined();
    expect(button("Add avatar")!.textContent).toBe("A");
    expect(labels()).toEqual(["Name", "Country", "City", "Time zone", "About you"]);
    // Country, City and Time zone share one row.
    expect(field("Country").closest(".profile-place")).toBe(field("Time zone").closest(".profile-place"));
    expect(host.querySelectorAll("button.primary")).toHaveLength(1);
  });

  test("Save sends the fields together; a server message lands under its field", async () => {
    const onSaved = await profile();
    type(field("About you"), "  Gardener in Riga  ");
    pageApi.saveSettings.mockResolvedValue({ ...prefs, description: "Gardener in Riga" });
    await act(async () => button("Save")!.click());
    expect(pageApi.saveSettings).toHaveBeenCalledWith(expect.objectContaining({ display_name: "Alex", city: "Riga", description: "Gardener in Riga" }));
    expect(onSaved).toHaveBeenCalled();
    pageApi.saveSettings.mockRejectedValue(new Error("display_name must be 1-64 printable characters"));
    await act(async () => button("Save")!.click());
    const hint = field("Name").parentElement!.querySelector(".error-text")!;
    expect(hint.textContent).toBe("Must be 1-64 printable characters");
  });

  test("the description counts characters, takes no line break and stops Save past 280", async () => {
    await profile();
    type(field("About you"), "line\none");
    expect(field("About you").value).toBe("line one");
    type(field("About you"), "😀".repeat(280));
    expect(host.textContent).toContain("280 / 280");
    type(field("About you"), "😀".repeat(281));
    await act(async () => button("Save")!.click());
    expect(pageApi.saveSettings).not.toHaveBeenCalled();
    expect(field("About you").parentElement!.querySelector(".error-text")!.textContent).toContain("At most 280 characters (281 now)");
  });

  test("the avatar and banner are made small, saved at once, replaced and removed", async () => {
    await profile();
    const small = new Blob(["x"], { type: "image/webp" });
    shrinkPicture.mockResolvedValue(small);
    pageApi.setProfileImage.mockResolvedValue({ avatar: "v1" });
    const input = host.querySelector<HTMLInputElement>('input[aria-label="Choose avatar"]')!;
    const file = new File(["raw"], "me.png", { type: "image/png" });
    Object.defineProperty(input, "files", { value: [file], configurable: true });
    await act(async () => input.dispatchEvent(new Event("change", { bubbles: true })));
    expect(shrinkPicture).toHaveBeenCalledWith("avatar", file);
    expect(pageApi.setProfileImage).toHaveBeenCalledWith("avatar", small);
    expect(button("Change avatar")!.querySelector("img")!.getAttribute("src")).toBe("/api/users/u1/avatar?v=v1&link=t");
    // A refused picture says why.
    shrinkPicture.mockRejectedValue(new Error("Pick a PNG, JPEG or WebP picture."));
    const banner = host.querySelector<HTMLInputElement>('input[aria-label="Choose banner"]')!;
    Object.defineProperty(banner, "files", { value: [new File(["<svg/>"], "x.svg", { type: "image/svg+xml" })], configurable: true });
    await act(async () => banner.dispatchEvent(new Event("change", { bubbles: true })));
    expect(host.querySelector('[role="alert"]')!.textContent).toBe("Banner: Pick a PNG, JPEG or WebP picture.");
    // Saved at once, and the page says so.
    expect(host.querySelector(".profile-pictures-hint")!.textContent).toContain("saved as soon as you pick it");
    // Remove asks first; No keeps it.
    pageApi.removeProfileImage.mockResolvedValue({ avatar: null });
    vi.stubGlobal("confirm", () => false);
    await act(async () => button("Remove avatar")!.click());
    expect(pageApi.removeProfileImage).not.toHaveBeenCalled();
    vi.stubGlobal("confirm", () => true);
    await act(async () => button("Remove avatar")!.click());
    expect(pageApi.removeProfileImage).toHaveBeenCalledWith("avatar");
    expect(button("Add avatar")).toBeDefined();
  });

  test("the crop: a centred square for the avatar, 3:1 for the banner, never larger than the caps", () => {
    expect(frame("avatar", 1000, 500)).toEqual({ sx: 250, sy: 0, sw: 500, sh: 500, dw: 256, dh: 256 });
    expect(frame("banner", 3000, 3000)).toEqual({ sx: 0, sy: 1000, sw: 3000, sh: 1000, dw: 1500, dh: 500 });
    // A small one is not blown up.
    expect(frame("avatar", 100, 100)).toMatchObject({ dw: 100, dh: 100 });
  });

  test("a picture that fails to load falls back to the initial", async () => {
    pageApi.profileImages.mockResolvedValue({ avatar: "gone", banner: null });
    await profile();
    const img = button("Change avatar")!.querySelector("img")!;
    await act(async () => img.dispatchEvent(new Event("error")));
    expect(button("Change avatar")!.querySelector("img")).toBeNull();
    expect(button("Change avatar")!.textContent).toBe("A");
  });

  test("a name with markup is text", async () => {
    await profile({ ...prefs, display_name: "<b>x</b>" });
    expect(host.querySelector("b")).toBeNull();
    expect(button("Add avatar")!.textContent).toBe("<");
  });
});

describe("Personalisation", () => {
  async function show(p: Prefs = prefs) {
    const onSaved = vi.fn();
    await act(async () => root.render(<PersonalisationTab prefs={p} onSaved={onSaved} />));
    return onSaved;
  }
  const selects = () => [...host.querySelectorAll<HTMLSelectElement>("select")];

  test("five choices, each Default first; a Memory row links to the memory page", async () => {
    await show();
    expect(selects().map((s) => [...s.options].map((o) => o.value))).toEqual([
      ["default", "friendly", "professional", "direct", "concise"],
      ["default", "more", "less"],
      ["default", "more", "less"],
      ["default", "more", "less"],
      ["default", "more", "less"],
    ]);
    expect(selects().every((s) => s.value === "default")).toBe(true);
    expect([...host.querySelectorAll(".setting-title")].map((t) => t.textContent)).toEqual([
      "Your memory",
      "Base style and tone",
      "Warmth",
      "Enthusiasm",
      "Emoji",
      "Headings and lists",
    ]);
    expect(host.querySelector<HTMLAnchorElement>("a.button-like")!.getAttribute("href")).toBe("#/settings/account/memory");
  });

  test("saves the choices and the instructions with its own call", async () => {
    const onSaved = await show();
    act(() => {
      selects()[0]!.value = "concise";
      selects()[0]!.dispatchEvent(new Event("change", { bubbles: true }));
      selects()[3]!.value = "less";
      selects()[3]!.dispatchEvent(new Event("change", { bubbles: true }));
    });
    type(host.querySelector("textarea")!, "  Metric units, please.  ");
    pageApi.savePersonalisation.mockResolvedValue(prefs);
    await act(async () => button("Save")!.click());
    expect(pageApi.savePersonalisation).toHaveBeenCalledWith({ style: "concise", emoji: "less", instructions: "Metric units, please." });
    expect(pageApi.saveSettings).not.toHaveBeenCalled();
    expect(onSaved).toHaveBeenCalled();
    expect(host.textContent).toContain("applies from your next message");
  });

  test("the counter counts characters; past 1,024 Save is off", async () => {
    await show();
    const t = host.querySelector("textarea")!;
    type(t, "😀".repeat(1024));
    expect(host.textContent).toContain("1024 / 1024");
    expect(button("Save")!.disabled).toBe(false);
    type(t, "😀".repeat(1025));
    expect(host.textContent).toContain("1025 / 1024: too long");
    expect(button("Save")!.disabled).toBe(true);
    expect(t.getAttribute("aria-invalid")).toBe("true");
  });
});

describe("Workspace settings → Agents", () => {
  const ws: WorkspaceSummary = { workspace_id: "w1", name: "Acme", personal: false, role: "member", permissions: [], members: 3, exec_level: "host" };
  const agent = (id: string, more: Partial<Agent> = {}): Agent => ({
    id,
    name: id,
    description: `${id} things.`,
    builtin: true,
    origin: "builtin",
    default_model: null,
    main_model: "m",
    fallbacks: [],
    unavailable: [],
    avatar: { shape: "circle", eyes: "round", accent: "blue" },
    ...more,
  });
  async function show(viewer: Me) {
    await act(async () =>
      root.render(<WorkspaceAgents me={viewer} workspace={ws} agents={[]} models={[]} defaultAgent="general" onChanged={() => {}} onLeft={() => {}} />),
    );
  }

  test("lists the workspace's agents as the Agents page does, read in its scope", async () => {
    api.agents.mockResolvedValue({ agents: [agent("coder"), agent("<i>x</i>", { origin: "custom", builtin: false })], default: "general" });
    await show(me);
    expect(api.agents).toHaveBeenCalledWith({ workspace: "w1" });
    const names = [...host.querySelectorAll(".agent-name")].map((n) => n.textContent);
    expect(names[0]).toContain("coder");
    expect(names[1]).toContain("<i>x</i>");
    expect(host.querySelector("i")).toBeNull();
  });

  test("a member sees no edit or delete; a settings holder edits, and deletes the workspace's own", async () => {
    const mine = agent("mine", { origin: "custom", builtin: false, scope: "workspace" });
    const server = agent("lead", { origin: "custom", builtin: false });
    api.agents.mockResolvedValue({ agents: [agent("coder"), server, mine], default: "general", may_edit: false });
    // A server admin who is no admin here gets no more than any member.
    await show({ ...me, role: "admin" });
    expect(host.querySelector(".agent-actions")).toBeNull();
    expect(button("New agent")).toBeUndefined();
    expect([...host.querySelectorAll(".agent-name")].map((n) => n.textContent)).toEqual([
      "coder @coder · built-in",
      "lead @lead · server",
      "mine @mine · workspace",
    ]);
    api.agents.mockResolvedValue({ agents: [agent("coder"), server, mine], default: "general", may_edit: true });
    // A fresh panel: the list is read when it opens.
    act(() => root.unmount());
    root = createRoot(host);
    await show(me);
    expect(button("Edit coder")).toBeDefined();
    expect(button("Edit lead")).toBeDefined();
    expect(button("Delete coder")).toBeUndefined();
    expect(button("Delete lead")).toBeUndefined();
    expect(button("New agent")).toBeDefined();
    vi.stubGlobal("confirm", () => true);
    pageApi.deleteAgent.mockResolvedValue({ ok: true });
    api.agents.mockResolvedValue({ agents: [agent("coder")], default: "general", may_edit: true });
    await act(async () => button("Delete mine")!.click());
    expect(pageApi.deleteAgent).toHaveBeenCalledWith("mine", { workspace: "w1" });
    expect(button("Delete mine")).toBeUndefined();
    // A row opens the agent's page here, in this workspace's scope.
    const f = { memory: true, project_memory: true, personalisation: true, recall: false, board: false, web: true, read_files: false, edit_files: false, commands: false, read_only_commands: false, mcp: false, delegate: false, background_subagents: false, as_agent: true, as_subagent: true };
    pageApi.agent.mockResolvedValue({ ...agent("coder"), origin: "builtin", features: f, prompt: "P", summary: "", instructions: "", description_default: "" });
    await act(async () => host.querySelector<HTMLButtonElement>(".agent-open")!.click());
    expect(pageApi.agent).toHaveBeenCalledWith("coder", { workspace: "w1" });
  });
  test("a workspace agent a server agent hides is named, and may be deleted", async () => {
    api.agents.mockResolvedValue({ agents: [agent("planner")], default: "general", may_edit: true, hidden: [{ id: "planner", name: "Mine" }] });
    await show({ ...me });
    expect(host.textContent).toContain("a server agent has the same handle");
    vi.stubGlobal("confirm", () => true);
    pageApi.deleteAgent.mockResolvedValue({ ok: true });
    api.agents.mockResolvedValue({ agents: [agent("planner")], default: "general", may_edit: true, hidden: [] });
    await act(async () => [...host.querySelectorAll("button")].find((b) => b.textContent === "Delete")!.click());
    expect(pageApi.deleteAgent).toHaveBeenCalledWith("planner", { workspace: "w1" });
    expect(host.textContent).not.toContain("a server agent has the same handle");
  });
});
