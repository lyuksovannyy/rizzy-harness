import { describe, expect, it } from "vitest";
import type { WorkspaceSummary } from "../types";
import { accountLine, afterChange, can, followRoute, keepsRoute, switchRoute, workspaceName } from "./workspaces";

describe("workspaceName", () => {
  it("names Personal from the catalog, whatever is stored", () => {
    expect(workspaceName({ name: "personal", personal: true })).toBe("Personal");
    expect(workspaceName({ name: "Acme", personal: false })).toBe("Acme");
  });

  it("reads permissions, none without a workspace", () => {
    expect(can({ permissions: ["routines", "members"] }, "members")).toBe(true);
    expect(can({ permissions: ["routines"] }, "settings")).toBe(false);
    expect(can(null, "routines")).toBe(false);
  });
});

describe("followRoute", () => {
  const base = { id: "s1", active: "w1", listed: false as boolean | null, memberOf: ["w1", "w2"] as string[] | null };

  it("does nothing off a chat, for the API token, or while this workspace's chats load", () => {
    expect(followRoute({ ...base, id: null })).toEqual({ do: "nothing" });
    expect(followRoute({ ...base, active: null })).toEqual({ do: "nothing" });
    expect(followRoute({ ...base, listed: null })).toEqual({ do: "nothing" });
    expect(followRoute({ ...base, listed: true })).toEqual({ do: "nothing" });
  });

  it("looks a chat up that this workspace does not list", () => {
    expect(followRoute(base)).toEqual({ do: "lookup", id: "s1" });
  });

  it("switches to the chat's workspace when the user is in it", () => {
    expect(followRoute({ ...base, found: "w2" })).toEqual({ do: "switch", to: "w2" });
    // Known only once the list is: until then, wait.
    expect(followRoute({ ...base, found: "w2", memberOf: null })).toEqual({ do: "nothing" });
  });

  it("stays put when there is nowhere to go", () => {
    expect(followRoute({ ...base, found: "w1" })).toEqual({ do: "nothing" });
    expect(followRoute({ ...base, found: "w3" })).toEqual({ do: "nothing" });
    // Not found, or a chat without an owner (the API token's).
    expect(followRoute({ ...base, found: null })).toEqual({ do: "nothing" });
  });
});

describe("keepsRoute", () => {
  it("a switch stays on the open chat only when it belongs to the target", () => {
    expect(keepsRoute({ workspace_id: "w2" }, "w2")).toBe(true);
    expect(keepsRoute({ workspace_id: "w1" }, "w2")).toBe(false);
    expect(keepsRoute({ workspace_id: null }, "w2")).toBe(false);
    expect(keepsRoute(null, "w2")).toBe(false);
  });
});

describe("afterChange", () => {
  const ws = (id: string, more: Partial<WorkspaceSummary> = {}): WorkspaceSummary => ({
    workspace_id: id,
    name: id,
    personal: false,
    role: "member",
    permissions: ["routines"],
    members: 2,
    exec_level: "host",
    ...more,
  });
  const personal = ws("p", { personal: true, role: "owner", members: 1 });

  it("stays in this tab's workspace, refreshed, whatever another tab made active", () => {
    // Promoted, renamed and joined by someone since this tab loaded it.
    const fresh = ws("acme", { name: "Acme Labs", role: "admin", permissions: ["settings", "members", "routines"], members: 3, exec_level: "host" as const });
    expect(afterChange(ws("acme"), [personal, fresh, ws("beta")], null)).toEqual({ keep: fresh });
    // The server's active workspace is not an input: two tabs cannot pull each other back and forth.
    expect(afterChange(personal, [personal, fresh], null)).toEqual({ keep: personal });
  });

  it("falls back once the user is out, saying where they were", () => {
    expect(afterChange(ws("acme", { name: "Acme" }), [personal], null)).toEqual({ fallBack: true, gone: "Acme" });
  });

  it("says nothing when the user left or deleted it themselves", () => {
    expect(afterChange(ws("acme"), [personal], "acme")).toEqual({ fallBack: true, gone: null });
    // Leaving another one does not hide losing this one.
    expect(afterChange(ws("acme", { name: "Acme" }), [personal], "beta")).toEqual({ fallBack: true, gone: "Acme" });
  });
});

describe("accountLine", () => {
  const ws = (personal: boolean, role: WorkspaceSummary["role"]) => ({ name: "Acme", personal, role });

  it("names the workspace and the role in a team", () => {
    expect(accountLine({ role: "member" }, ws(false, "guest"))).toBe("Acme · Guest");
    expect(accountLine({ role: "admin" }, ws(false, "owner"))).toBe("Acme · Owner");
  });

  it("says the server role in Personal, where the user is always owner", () => {
    expect(accountLine({ role: "admin" }, ws(true, "owner"))).toBe("Personal · Server admin");
    expect(accountLine({ role: "member" }, ws(true, "owner"))).toBe("Personal · User");
  });

  it("has only the server role without a workspace (the API token)", () => {
    expect(accountLine({ role: "admin" }, null)).toBe("Server admin");
  });
});

describe("switchRoute", () => {
  it("takes an area's page to its list in the new workspace", () => {
    expect(switchRoute({ kind: "agents", id: "writer" })).toEqual({ kind: "agents", id: null });
    expect(switchRoute({ kind: "routines", id: "a".repeat(32), tab: "runs" })).toEqual({ kind: "routines", id: null });
    expect(switchRoute({ kind: "media", filter: "images", id: "b".repeat(32) })).toEqual({ kind: "media", filter: "images", id: null });
    expect(switchRoute({ kind: "artifacts", id: "c".repeat(32) })).toEqual({ kind: "artifacts", id: null });
  });

  it("leaves chats, projects and the rest to keepsRoute", () => {
    expect(switchRoute({ kind: "session", id: "s" })).toBeNull();
    expect(switchRoute({ kind: "project", id: "p" })).toBeNull();
    expect(switchRoute({ kind: "home" })).toBeNull();
    expect(switchRoute({ kind: "settings" })).toBeNull();
  });
});
