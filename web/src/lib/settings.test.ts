import { describe, expect, it } from "vitest";
import type { Me, Perm, WorkspaceRole, WorkspaceSummary } from "../types";
import { pickTab, visibleSections } from "./settings";

const ws = (role: WorkspaceRole, permissions: Perm[], personal = false): WorkspaceSummary => ({
  workspace_id: personal ? "w-personal" : "w-team",
  name: personal ? "Personal" : "Acme",
  personal,
  role,
  permissions,
  members: personal ? 1 : 3,
});
const ALL: Perm[] = ["agentic", "settings", "members"];
const viewer = (over: Partial<Me>): Pick<Me, "user_id" | "role" | "workspace"> => ({
  user_id: "u1",
  role: "member",
  workspace: ws("member", ["agentic"]),
  ...over,
});
const shape = (v: Pick<Me, "user_id" | "role" | "workspace">) =>
  Object.fromEntries(visibleSections(v).map((s) => [s.id, s.tabs]));

describe("visibleSections", () => {
  it("a member: their account and the workspace, read-only", () => {
    expect(shape(viewer({}))).toEqual({
      account: ["profile", "preferences", "memory", "password"],
      workspace: ["overview"],
    });
    // A guest sees the same; what a role may do only shows inside.
    expect(shape(viewer({ workspace: ws("guest", []) }))).toEqual(shape(viewer({})));
  });

  it("a workspace admin also gets Workspace admin, without Delete", () => {
    expect(shape(viewer({ workspace: ws("admin", ALL) }))["workspace-admin"]).toEqual(["general", "models", "members"]);
    // Delete is the owner's.
    expect(shape(viewer({ workspace: ws("owner", ALL) }))["workspace-admin"]).toEqual([
      "general",
      "models",
      "members",
      "delete",
    ]);
    // Either permission opens the section, with only its own tabs.
    expect(shape(viewer({ workspace: ws("admin", ["members"]) }))["workspace-admin"]).toEqual(["members"]);
    expect(shape(viewer({ workspace: ws("admin", ["settings"]) }))["workspace-admin"]).toEqual(["general", "models"]);
  });

  it("Personal never has Workspace admin, though its owner holds everything", () => {
    expect(Object.keys(shape(viewer({ workspace: ws("owner", ALL, true) })))).toEqual(["account", "workspace"]);
  });

  it("a harness admin gets Server, and no workspace powers from it", () => {
    const s = shape(viewer({ role: "admin" }));
    expect(Object.keys(s)).toEqual(["account", "workspace", "server"]);
    expect(s.server).toEqual(["providers", "agents", "recall", "savings", "nodes", "users", "workspaces"]);
  });

  it("the API token: no password, no workspace", () => {
    expect(shape(viewer({ user_id: null, role: "admin", workspace: null }))).toEqual({
      account: ["profile", "preferences", "memory"],
      server: ["providers", "agents", "recall", "savings", "nodes", "users", "workspaces"],
    });
  });
});

describe("pickTab", () => {
  const member = visibleSections(viewer({}));
  const admin = visibleSections(viewer({ role: "admin", workspace: ws("owner", ALL) }));

  it("opens what the route names when the viewer sees it", () => {
    expect(pickTab(admin, { section: "server", tab: "users" })).toEqual({ section: "server", tab: "users" });
    expect(pickTab(admin, { section: "workspace-admin", tab: "delete" })).toEqual({
      section: "workspace-admin",
      tab: "delete",
    });
  });

  it("falls back to the section's first tab, then to Account", () => {
    expect(pickTab(admin, { section: "server" })).toEqual({ section: "server", tab: "providers" });
    expect(pickTab(admin, { section: "server", tab: "nope" })).toEqual({ section: "server", tab: "providers" });
    expect(pickTab(member, {})).toEqual({ section: "account", tab: "profile" });
    // A member cannot open an admin panel by its address.
    expect(pickTab(member, { section: "server", tab: "users" })).toEqual({ section: "account", tab: "profile" });
    expect(pickTab(member, { section: "workspace-admin", tab: "members" })).toEqual({
      section: "account",
      tab: "profile",
    });
    // A tab of another section does not open under this one.
    expect(pickTab(admin, { section: "account", tab: "users" })).toEqual({ section: "account", tab: "profile" });
  });
});
