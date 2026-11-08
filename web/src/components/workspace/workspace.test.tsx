// @vitest-environment happy-dom
// The workspace settings panels against a mocked API: Overview's memory
// switch and leaving, Members' guard-disabled choices and adding someone,
// General's and Models' saves (each reads the settings again and changes
// only its own field; General's chain is the workspace default), Delete's
// typed confirmation, and Server → Workspaces' policy switch and owner
// recovery.
import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type {
  AdminWorkspace,
  Agent,
  Me,
  ModelInfo,
  WorkspaceDetail,
  WorkspaceMember,
  WorkspaceRole,
  WorkspaceSummary,
} from "../../types";
import type { WorkspacePanelProps } from "./panel";

const api = {
  workspace: vi.fn(),
  workspaceMembers: vi.fn(),
  setWorkspaceMemory: vi.fn(),
  removeWorkspaceMember: vi.fn(),
  addWorkspaceMember: vi.fn(),
  setWorkspaceMemberRole: vi.fn(),
  updateWorkspace: vi.fn(),
  deleteWorkspace: vi.fn(),
  agents: vi.fn(),
  adminWorkspaces: vi.fn(),
  adminWorkspaceMembers: vi.fn(),
  saveWorkspacePolicy: vi.fn(),
  setWorkspaceOwner: vi.fn(),
  adminDeleteWorkspace: vi.fn(),
};
vi.mock("../../api", () => ({ api }));

const { WorkspaceOverview } = await import("./WorkspaceOverview");
const { WorkspaceGeneral } = await import("./WorkspaceGeneral");
const { WorkspaceMembers } = await import("./WorkspaceMembers");
const { WorkspaceModels } = await import("./WorkspaceModels");
const { WorkspaceDelete } = await import("./WorkspaceDelete");
const { ServerWorkspaces } = await import("./ServerWorkspaces");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const summary = (role: WorkspaceRole, more: Partial<WorkspaceSummary> = {}): WorkspaceSummary => ({
  workspace_id: "w-acme",
  name: "Acme",
  personal: false,
  role,
  permissions: role === "owner" || role === "admin" ? ["agentic", "settings", "members"] : role === "member" ? ["agentic"] : [],
  members: 4,
  ...more,
});
const detail = (more: Partial<WorkspaceDetail> = {}): WorkspaceDetail => ({
  ...summary("admin"),
  settings: { default_agent: null, agent_models: {} },
  use_memory: true,
  created_ms: 1,
  ...more,
});
const member = (id: string, role: WorkspaceRole, display_name: string | null = null): WorkspaceMember => ({
  user_id: `u-${id}`,
  username: id,
  display_name,
  role,
  joined_ms: 1,
});
const agent = (id: string, name: string): Agent => ({
  id,
  name,
  description: "",
  builtin: true,
  default_model: null,
  main_model: null,
  fallbacks: [],
  unavailable: [],
  avatar: { shape: "circle", eyes: "round", accent: "blue" },
});
const model = (raw: string, more: Partial<ModelInfo> = {}): ModelInfo => ({
  id: `openai::${raw}`,
  raw,
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
const me = (role: WorkspaceRole = "admin"): Me => ({
  user_id: "u-alex",
  username: "alex",
  role: "member",
  can_agentic: true,
  must_change_password: false,
  workspace: summary(role),
});

let host: HTMLElement;
let root: Root;
beforeEach(() => {
  for (const f of Object.values(api)) f.mockReset();
  vi.stubGlobal("confirm", () => true);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
});

const render = (el: ReactNode) => act(async () => root.render(el));
const props = (role: WorkspaceRole = "admin", more: Partial<WorkspacePanelProps> = {}): WorkspacePanelProps => ({
  me: me(role),
  workspace: summary(role),
  agents: [agent("general", "General"), agent("coder", "Coder")],
  models: [model("gpt-4o", { default: true }), model("gpt-4o-mini")],
  defaultAgent: "general",
  onChanged: vi.fn(),
  onLeft: vi.fn(),
  ...more,
});
const button = (label: string) =>
  [...host.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent!.trim() === label || b.getAttribute("aria-label") === label)!;
const click = (el: HTMLElement) => act(async () => el.click());
const type = (input: HTMLInputElement, value: string) =>
  act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
const choose = (select: HTMLSelectElement, value: string) =>
  act(async () => {
    select.value = value;
    select.dispatchEvent(new Event("change", { bubbles: true }));
  });

describe("Overview", () => {
  test("the role, the members, the defaults; the memory switch is the member's own", async () => {
    api.workspace.mockResolvedValue(detail({ settings: { default_agent: "coder", agent_models: {} } }));
    api.workspaceMembers.mockResolvedValue({ members: [member("sam", "owner"), member("alex", "admin", "Alex")] });
    await render(<WorkspaceOverview {...props("admin", { defaultAgent: "coder" })} />);
    expect(host.querySelector(".ws-role-name")!.textContent).toBe("Admin");
    expect([...host.querySelectorAll(".ws-members li")].map((li) => li.textContent)).toEqual(["samOwner", "Alex @alex (you)Admin"]);
    const facts = host.querySelector(".ws-facts")!.textContent;
    expect(facts).toContain("Coder set for this workspace");
    expect(facts).toContain("gpt-4o the server's");

    api.setWorkspaceMemory.mockResolvedValue(detail({ use_memory: false }));
    const memory = [...host.querySelectorAll("label")].find((l) => l.textContent === "Use my memory in this workspace")!.querySelector("input")!;
    expect(memory.checked).toBe(true);
    await click(memory);
    expect(api.setWorkspaceMemory).toHaveBeenCalledWith("w-acme", false);
    expect(memory.checked).toBe(false);
  });

  test("leaving asks first, then goes to Personal; a refusal is shown", async () => {
    api.workspace.mockResolvedValue(detail());
    api.workspaceMembers.mockResolvedValue({ members: [] });
    const p = props("owner");
    await render(<WorkspaceOverview {...p} />);
    api.removeWorkspaceMember.mockRejectedValueOnce(new Error("Acme needs another owner first"));
    await click(button("Leave Acme"));
    expect(api.removeWorkspaceMember).toHaveBeenCalledWith("w-acme", "u-alex");
    expect(host.textContent).toContain("Acme needs another owner first");
    expect(p.onLeft).not.toHaveBeenCalled();

    api.removeWorkspaceMember.mockResolvedValue({ ok: true });
    await click(button("Leave Acme"));
    expect(p.onLeft).toHaveBeenCalledTimes(1);

    vi.stubGlobal("confirm", () => false);
    await click(button("Leave Acme"));
    expect(api.removeWorkspaceMember).toHaveBeenCalledTimes(2);
  });

  test("leaving as the only member says it deletes the workspace", async () => {
    const asked: string[] = [];
    vi.stubGlobal("confirm", (m: string) => (asked.push(m), false));
    api.workspace.mockResolvedValue(detail());
    api.workspaceMembers.mockResolvedValue({ members: [member("sam", "owner"), member("alex", "admin")] });
    await render(<WorkspaceOverview {...props("admin")} />);
    expect(host.textContent).not.toContain("deletes the workspace");
    await click(button("Leave Acme"));
    expect(asked.at(-1)).toBe("Leave Acme? Your chats here go back to your Personal workspace.");

    api.workspaceMembers.mockResolvedValue({ members: [member("alex", "owner")] });
    await render(<WorkspaceOverview {...props("owner", { workspace: summary("owner", { members: 1 }) })} />);
    expect(host.textContent).toContain("You are its only member: leaving deletes the workspace");
    await click(button("Leave Acme"));
    expect(asked.at(-1)).toMatch(/^Leave Acme\? You are its only member, so leaving deletes it/);
    expect(api.removeWorkspaceMember).not.toHaveBeenCalled();
  });

  test("Personal: named from the catalog, no members, memory switch or leaving", async () => {
    api.workspace.mockResolvedValue(detail({ personal: true, name: "stored" }));
    const personal = summary("owner", { personal: true, name: "stored", members: 1 });
    await render(<WorkspaceOverview {...props("owner", { workspace: personal })} />);
    expect(host.querySelector("h3")!.textContent).toBe("Personal");
    expect(api.workspaceMembers).not.toHaveBeenCalled();
    expect(host.textContent).not.toContain("Use my memory");
    expect(button("Leave stored")).toBeUndefined();
  });
});

describe("Members", () => {
  const MEMBERS = [member("alex", "admin"), member("sam", "owner"), member("kim", "member", "Kim"), member("lee", "guest")];
  const row = (username: string) => [...host.querySelectorAll("tbody tr")].find((tr) => tr.textContent!.includes(username))!;
  // Named after the member as the row shows them.
  const roleOf = (name: string) => host.querySelector<HTMLSelectElement>(`select[aria-label="Role of ${name}"]`)!;
  const off = (s: HTMLSelectElement) => [...s.options].filter((o) => o.disabled).map((o) => o.value);

  test("an admin cannot touch owners or their own row, nor make owners", async () => {
    api.workspaceMembers.mockResolvedValue({ members: MEMBERS });
    await render(<WorkspaceMembers {...props("admin")} />);
    expect(roleOf("alex").disabled).toBe(true);
    expect(roleOf("sam").disabled).toBe(true);
    expect(row("sam").querySelector("button")).toBeNull();
    expect(row("alex").querySelector("button")).toBeNull();
    expect(roleOf("Kim").disabled).toBe(false);
    expect(off(roleOf("Kim"))).toEqual(["owner"]);
    expect(row("kim").querySelector("button")!.textContent).toBe("Remove");
    expect(row("kim").querySelector("button")!.getAttribute("aria-label")).toBe("Remove Kim");
    // The add form offers the same roles, member first.
    const pick = host.querySelector<HTMLSelectElement>('select[aria-label="Role of the new member"]')!;
    expect(pick.value).toBe("member");
    expect(off(pick)).toEqual(["owner"]);
    // What each role may do.
    expect([...host.querySelectorAll(".ws-roles tbody tr")].map((tr) => tr.querySelectorAll("svg").length)).toEqual([3, 3, 1, 0]);
  });

  test("an owner may make owners and manage other owners", async () => {
    api.workspaceMembers.mockResolvedValue({ members: MEMBERS });
    await render(<WorkspaceMembers {...props("owner")} />);
    expect(roleOf("sam").disabled).toBe(false);
    expect(off(roleOf("Kim"))).toEqual([]);
    expect(roleOf("alex").disabled).toBe(true);
  });

  test("adding by username with the chosen role; changing a role; refusals shown", async () => {
    api.workspaceMembers.mockResolvedValue({ members: MEMBERS });
    const p = props("admin");
    await render(<WorkspaceMembers {...p} />);
    const name = host.querySelector<HTMLInputElement>('input[aria-label="Username"]')!;
    expect(button("Add").disabled).toBe(true);

    api.addWorkspaceMember.mockRejectedValueOnce(new Error("no such user"));
    await type(name, " nobody ");
    await click(button("Add"));
    expect(api.addWorkspaceMember).toHaveBeenCalledWith("w-acme", "nobody", "member");
    expect(host.textContent).toContain("no such user");
    expect(name.value).toBe(" nobody ");

    api.addWorkspaceMember.mockResolvedValue(member("jo", "guest"));
    await type(name, "jo");
    await choose(host.querySelector<HTMLSelectElement>('select[aria-label="Role of the new member"]')!, "guest");
    await click(button("Add"));
    expect(api.addWorkspaceMember).toHaveBeenLastCalledWith("w-acme", "jo", "guest");
    expect(name.value).toBe("");
    expect(host.textContent).not.toContain("no such user");
    expect(p.onChanged).toHaveBeenCalled();
    expect(api.workspaceMembers.mock.calls.length).toBeGreaterThan(1);

    api.setWorkspaceMemberRole.mockResolvedValue(member("kim", "guest"));
    await choose(roleOf("Kim"), "guest");
    expect(api.setWorkspaceMemberRole).toHaveBeenCalledWith("w-acme", "u-kim", "guest");

    api.removeWorkspaceMember.mockResolvedValue({ ok: true });
    await click(row("lee").querySelector("button")!);
    expect(api.removeWorkspaceMember).toHaveBeenCalledWith("w-acme", "u-lee");
  });
});

describe("Models", () => {
  const box = (raw: string) =>
    [...host.querySelectorAll<HTMLLIElement>(".model-pick > ul > li")]
      .find((li) => li.querySelector(".mono")!.textContent === `openai::${raw}`)!
      .querySelector<HTMLInputElement>('input[type="checkbox"]')!;
  const item = (label: string) => host.querySelector<HTMLButtonElement>(`.agent-menu button[aria-label="${label}"]`)!;
  const save = () => [...host.querySelectorAll<HTMLButtonElement>("button.primary")].at(-1)!;

  beforeEach(() => {
    api.agents.mockResolvedValue({
      agents: [
        { ...agent("general", "General"), main_model: "openai::gpt-4o" },
        { ...agent("coder", "Coder"), main_model: "openai::gpt-4o", fallbacks: ["openai::gpt-4o-mini"] },
      ],
      default: "general",
    });
  });

  test("General's chain is the workspace default; Use server chain drops an agent's; Save changes only the chains", async () => {
    api.workspace.mockResolvedValue(
      detail({ settings: { default_agent: "coder", agent_models: { coder: { model: "openai::gpt-4o", fallbacks: [] } } } }),
    );
    api.updateWorkspace.mockImplementation((_id: string, b: { settings: WorkspaceDetail["settings"] }) =>
      Promise.resolve(detail({ settings: b.settings })),
    );
    const p = props("admin");
    await render(<WorkspaceModels {...p} />);
    expect(api.agents).toHaveBeenCalledWith("server");
    const chains = () => [...host.querySelectorAll(".agent-chains > li")].map((li) => li.textContent);
    expect(chains()[0]).toBe("GeneralServer chain: gpt-4o");
    expect(chains()[1]).toContain("Use server chain");

    // A model ticked waits for an agent; given to General, it is the default.
    await click(box("gpt-4o-mini"));
    expect(save().disabled).toBe(true);
    await click(button("Assign to agents… (openai::gpt-4o-mini)"));
    await click(item("General: main model"));
    expect(save().disabled).toBe(false);
    await click(button("Coder: Use server chain"));
    expect(chains()[1]).toBe("CoderServer chain: gpt-4o → gpt-4o-mini");
    expect(box("gpt-4o").checked).toBe(false);

    // Another admin set the default agent since this panel loaded: the save
    // reads the settings again and changes only the chains.
    api.workspace.mockResolvedValueOnce(
      detail({ settings: { default_agent: "general", agent_models: { coder: { model: "openai::gpt-4o", fallbacks: [] } } } }),
    );
    await click(save());
    expect(api.workspace).toHaveBeenCalledTimes(2);
    expect(api.updateWorkspace).toHaveBeenCalledWith("w-acme", {
      settings: { default_agent: "general", agent_models: { general: { model: "openai::gpt-4o-mini", fallbacks: [] } } },
    });
    expect(p.onChanged).toHaveBeenCalled();
    expect(host.textContent).toContain("Saved.");
    expect(host.textContent).not.toContain("No longer known");
  });

  test("what the server dropped as no longer known is said", async () => {
    api.workspace.mockResolvedValue(
      detail({ settings: { default_agent: null, agent_models: { general: { model: "openai::gpt-4o", fallbacks: ["openai::gpt-4o-mini"] } } } }),
    );
    // Removed from its provider between the read and the save.
    api.updateWorkspace.mockResolvedValue(
      detail({ settings: { default_agent: null, agent_models: { general: { model: "openai::gpt-4o", fallbacks: [] } } } }),
    );
    await render(<WorkspaceModels {...props("admin")} />);
    await click(save());
    expect(host.textContent).toContain("No longer known, so dropped: gpt-4o-mini.");
  });
});

describe("General", () => {
  test("sends only what changed: the name, then the settings as the server has them with the new default agent", async () => {
    const settings = { default_agent: null, agent_models: { coder: { model: "openai::gpt-4o", fallbacks: [] } } };
    api.workspace.mockResolvedValue(detail({ settings }));
    api.updateWorkspace.mockImplementation((_id: string, b: { name?: string; settings?: WorkspaceDetail["settings"] }) =>
      Promise.resolve(detail({ name: b.name ?? "Acme", settings: b.settings ?? settings })),
    );
    const p = props("admin");
    await render(<WorkspaceGeneral {...p} />);
    const save = button("Save");
    expect(save.disabled).toBe(true);
    await type(host.querySelector<HTMLInputElement>("input")!, "Acme Labs");
    await click(save);
    expect(api.updateWorkspace).toHaveBeenLastCalledWith("w-acme", { name: "Acme Labs" });
    expect(p.onChanged).toHaveBeenCalledTimes(1);
    // A rename alone has no settings to read again.
    expect(api.workspace).toHaveBeenCalledTimes(1);

    // Another admin saved a chain in Models since this panel loaded: it stays.
    const newer = { default_agent: null, agent_models: { ...settings.agent_models, general: { model: "openai::gpt-4o-mini", fallbacks: [] } } };
    api.workspace.mockResolvedValueOnce(detail({ settings: newer }));
    await click(host.querySelector<HTMLButtonElement>(".select-button")!);
    await click([...host.querySelectorAll<HTMLElement>('[role="option"]')].find((o) => o.textContent === "Coder")!);
    await click(save);
    expect(api.workspace).toHaveBeenCalledTimes(2);
    expect(api.updateWorkspace).toHaveBeenLastCalledWith("w-acme", { settings: { ...newer, default_agent: "coder" } });
  });
});

describe("Delete", () => {
  test("typing the name unlocks it; then the app goes to Personal", async () => {
    const p = props("owner");
    await render(<WorkspaceDelete {...p} />);
    expect(host.textContent).toContain("you and 3 other members");
    const go = button("Delete workspace");
    expect(go.disabled).toBe(true);
    const input = host.querySelector<HTMLInputElement>("input")!;
    await type(input, "acme");
    expect(go.disabled).toBe(true);
    await type(input, "Acme");
    expect(go.disabled).toBe(false);

    api.deleteWorkspace.mockRejectedValueOnce(new Error("server says no"));
    await click(go);
    expect(host.textContent).toContain("server says no");
    expect(p.onLeft).not.toHaveBeenCalled();

    api.deleteWorkspace.mockResolvedValue({ ok: true });
    await click(go);
    expect(api.deleteWorkspace).toHaveBeenLastCalledWith("w-acme");
    expect(p.onLeft).toHaveBeenCalledTimes(1);
  });
});

describe("Server → Workspaces", () => {
  const ws = (id: string, more: Partial<AdminWorkspace> = {}): AdminWorkspace => ({
    workspace_id: id,
    name: id,
    personal: false,
    owners: ["sam"],
    members: 3,
    created_ms: 1,
    ...more,
  });
  const LIST = [ws("Acme"), ws("w-p", { personal: true, name: "stored", owners: ["alex"], members: 1 })];
  const admin: Me = { user_id: "u-alex", username: "alex", role: "admin", can_agentic: true, must_change_password: false };

  test("the policy switch saves what the server keeps", async () => {
    api.adminWorkspaces.mockResolvedValue({ workspaces: LIST, members_can_create: true });
    const onChanged = vi.fn();
    await render(<ServerWorkspaces me={admin} onChanged={onChanged} />);
    const rows = [...host.querySelectorAll("tbody tr")].map((tr) => tr.querySelector("td")!.textContent);
    expect(rows).toEqual(["Acme", "Personal"]);
    // A Personal workspace has nothing to recover or delete.
    expect([...host.querySelectorAll("tbody tr")][1]!.querySelector("button")).toBeNull();

    api.saveWorkspacePolicy.mockResolvedValue({ members_can_create: false });
    const policy = [...host.querySelectorAll("label")].find((l) => l.textContent === "Members can create workspaces")!.querySelector("input")!;
    expect(policy.checked).toBe(true);
    await click(policy);
    expect(api.saveWorkspacePolicy).toHaveBeenCalledWith(false);
    expect(policy.checked).toBe(false);
    expect(onChanged).toHaveBeenCalled();
  });

  test("members open under a team workspace; make owner among enabled members, never yourself", async () => {
    api.adminWorkspaces.mockResolvedValue({ workspaces: LIST, members_can_create: true });
    api.adminWorkspaceMembers.mockResolvedValue({
      members: [
        { user_id: "u-sam", username: "sam", role: "owner", disabled: false },
        { user_id: "u-kim", username: "kim", role: "member", disabled: false },
        { user_id: "u-lee", username: "lee", role: "guest", disabled: true },
        { user_id: "u-alex", username: "alex", role: "admin", disabled: false },
      ],
    });
    api.setWorkspaceOwner.mockResolvedValue(ws("Acme", { owners: ["sam", "kim"] }));
    await render(<ServerWorkspaces me={admin} />);
    const toggle = button("Members of Acme");
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    await click(toggle);
    expect(api.adminWorkspaceMembers).toHaveBeenCalledWith("Acme");
    const make = (username: string) =>
      [...host.querySelectorAll(".ws-admin-members li")].find((li) => li.textContent!.startsWith(username))!.querySelector("button");
    expect(make("sam")).toBeNull();
    expect(make("lee")!.disabled).toBe(true);
    expect(make("alex")!.disabled).toBe(true);
    await click(make("kim")!);
    expect(api.setWorkspaceOwner).toHaveBeenCalledWith("Acme", "u-kim");

    api.adminDeleteWorkspace.mockResolvedValue({ ok: true });
    await click(button("Delete"));
    expect(api.adminDeleteWorkspace).toHaveBeenCalledWith("Acme");
  });
});
