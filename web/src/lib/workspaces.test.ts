import { describe, expect, it } from "vitest";
import type { WorkspaceSummary } from "../types";
import { afterChange, can, followRoute, keepsRoute, workspaceName } from "./workspaces";

describe("workspaceName", () => {
  it("names Personal from the catalog, whatever is stored", () => {
    expect(workspaceName({ name: "personal", personal: true })).toBe("Personal");
    expect(workspaceName({ name: "Acme", personal: false })).toBe("Acme");
  });

  it("reads permissions, none without a workspace", () => {
    expect(can({ permissions: ["agentic", "members"] }, "members")).toBe(true);
    expect(can({ permissions: ["agentic"] }, "settings")).toBe(false);
    expect(can(null, "agentic")).toBe(false);
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
    permissions: ["agentic"],
    members: 2,
    ...more,
  });
  const personal = ws("p", { personal: true, role: "owner", members: 1 });

  it("stays in this tab's workspace, refreshed, whatever another tab made active", () => {
    // Promoted, renamed and joined by someone since this tab loaded it.
    const fresh = ws("acme", { name: "Acme Labs", role: "admin", permissions: ["agentic", "settings", "members"], members: 3 });
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
