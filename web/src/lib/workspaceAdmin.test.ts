// The workspace panels' own rules: what each role may give and to whom (the
// server's guards, mirrored to turn choices off), and a workspace's chains:
// the plan read from its settings, the document saved, what was dropped.
import { describe, expect, test } from "vitest";
import type { WorkspaceRole, WorkspaceSettings } from "../types";
import { canGive, canManage, chainsOf, dropped, ROLES, rolePerms, workspacePlan } from "./workspaceAdmin";

describe("roles", () => {
  test("permissions are fixed, and an unknown role has none", () => {
    expect(ROLES.map(rolePerms)).toEqual([["agentic", "settings", "members"], ["agentic", "settings", "members"], ["agentic"], []]);
    expect(rolePerms("boss" as WorkspaceRole)).toEqual([]);
  });

  test("only owners give the owner role; nobody gives more than they hold", () => {
    const gives = (actor: WorkspaceRole) => ROLES.filter((r) => canGive(actor, r));
    expect(gives("owner")).toEqual(["owner", "admin", "member", "guest"]);
    expect(gives("admin")).toEqual(["admin", "member", "guest"]);
    // Without the members permission, nothing.
    expect(gives("member")).toEqual([]);
    expect(gives("guest")).toEqual([]);
  });

  test("an admin manages up to admins, never owners; nobody their own row", () => {
    const manages = (actor: WorkspaceRole) => ROLES.filter((r) => canManage(actor, r, false));
    expect(manages("owner")).toEqual(["owner", "admin", "member", "guest"]);
    expect(manages("admin")).toEqual(["admin", "member", "guest"]);
    expect(manages("member")).toEqual([]);
    expect(canManage("owner", "member", true)).toBe(false);
    expect(canManage("boss" as WorkspaceRole, "guest", false)).toBe(false);
  });
});

describe("workspace chains", () => {
  const settings: WorkspaceSettings = {
    default_agent: "coder",
    agent_models: {
      general: { model: "a::x", fallbacks: ["a::gone", "a::y"] },
      coder: { model: "a::gone", fallbacks: ["a::x"] },
      ghost: { model: "a::x", fallbacks: [] },
    },
  };
  const knownAgent = (id: string) => id !== "ghost";
  const knownModel = (id: string) => id !== "a::gone";

  test("the plan keeps listed agents and models; a chain whose main model is gone goes", () => {
    expect(workspacePlan(settings, knownAgent, knownModel)).toEqual({ general: { model: "a::x", fallbacks: ["a::y"] } });
  });

  test("agents without a main model are left out of what is saved", () => {
    expect(chainsOf({ general: { model: "a::x", fallbacks: ["a::y"] }, coder: { model: null, fallbacks: [] } })).toEqual({
      general: { model: "a::x", fallbacks: ["a::y"] },
    });
  });

  test("what was dropped, named", () => {
    const got: WorkspaceSettings = { default_agent: null, agent_models: chainsOf(workspacePlan(settings, knownAgent, knownModel)) };
    expect(dropped(settings, got, { agent: (a) => a.toUpperCase(), model: (m) => m.slice(3) })).toEqual([
      "the default agent CODER",
      "gone",
      "CODER's chain",
      "GHOST's chain",
    ]);
    expect(dropped(settings, settings)).toEqual([]);
  });
});
