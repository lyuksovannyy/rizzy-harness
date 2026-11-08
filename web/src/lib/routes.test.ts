import { describe, expect, it } from "vitest";
import {
  artifactHash,
  MEMORY_HASH,
  mediaHash,
  NODES_HASH,
  parseRoute,
  projectHash,
  routeHash,
  routineHash,
  settingsHash,
} from "./routes";

const ID = "0195f3a2-7c1e-7b40-9a3e-1f2d3c4b5a69";

describe("parseRoute", () => {
  it("reads home, a chat and settings", () => {
    expect(parseRoute("")).toEqual({ kind: "home" });
    expect(parseRoute("#/")).toEqual({ kind: "home" });
    expect(parseRoute(`#/s/${ID}`)).toEqual({ kind: "session", id: ID });
    expect(parseRoute("#/s/not-an-id")).toEqual({ kind: "home" });
    expect(parseRoute("#/settings")).toEqual({ kind: "settings" });
    expect(parseRoute("#/settings/server")).toEqual({ kind: "settings", section: "server" });
    expect(parseRoute("#/settings/workspace-admin/members")).toEqual({
      kind: "settings",
      section: "workspace-admin",
      tab: "members",
    });
  });

  it("reads a project, its tab, or the create form; nothing else under #/p/", () => {
    const id = "0193f1c2a4b57d6e8f9012345678abcd";
    expect(parseRoute(`#/p/${id}`)).toEqual({ kind: "project", id });
    expect(parseRoute("#/p/new")).toEqual({ kind: "project", id: "new" });
    expect(parseRoute(`#/p/${id}/memory`)).toEqual({ kind: "project", id, tab: "memory" });
    for (const h of ["#/p/NEW", `#/p/${id}/files`, "#/p/new/board", `#/p/${ID}`]) {
      expect(parseRoute(h)).toEqual({ kind: "home" });
    }
    expect(routeHash(parseRoute(`#/p/${id}/board`))).toBe(`#/p/${id}/board`);
    expect(projectHash(id, "memory")).toBe(`#/p/${id}/memory`);
  });

  it("keeps accepting the paths from before the sections", () => {
    expect(parseRoute("#/settings/memory")).toEqual({ kind: "settings", section: "account", tab: "memory" });
    expect(parseRoute("#/settings/preferences")).toEqual({ kind: "settings", section: "account", tab: "preferences" });
    expect(parseRoute("#/settings/provider")).toEqual({ kind: "settings", section: "server", tab: "providers" });
    expect(parseRoute("#/settings/nodes")).toEqual({ kind: "settings", section: "server", tab: "nodes" });
  });

  it("reads the agents list, a new agent and an agent", () => {
    expect(parseRoute("#/agents")).toEqual({ kind: "agents", id: null });
    expect(parseRoute("#/agents/new")).toEqual({ kind: "agents", id: "new" });
    expect(parseRoute("#/agents/code-reviewer")).toEqual({ kind: "agents", id: "code-reviewer" });
    expect(parseRoute("#/agents/Bad")).toEqual({ kind: "home" });
    // The old Server → Agents tab is the sidebar's page now.
    expect(parseRoute("#/settings/server/agents")).toEqual({ kind: "agents", id: null });
  });

  it("reads routines: the list, a new one, one, its runs", () => {
    const id = "0193f1c2a4b57d6e8f9012345678abcd";
    expect(parseRoute("#/routines")).toEqual({ kind: "routines", id: null });
    expect(parseRoute("#/routines/new")).toEqual({ kind: "routines", id: "new" });
    expect(parseRoute(`#/routines/${id}`)).toEqual({ kind: "routines", id });
    expect(parseRoute(`#/routines/${id}/runs`)).toEqual({ kind: "routines", id, tab: "runs" });
    // A routine not saved yet has no runs; anything else malformed goes home.
    for (const h of ["#/routines/new/runs", `#/routines/${id}/edit`, "#/routines/", `#/routines/${ID}`, "#/routines/ABC"]) {
      expect(parseRoute(h)).toEqual({ kind: "home" });
    }
  });

  it("reads media: the grid, a filter, a preview over either", () => {
    const id = "0193f1c2a4b57d6e8f9012345678abcd";
    expect(parseRoute("#/media")).toEqual({ kind: "media", filter: null, id: null });
    expect(parseRoute("#/media/videos")).toEqual({ kind: "media", filter: "videos", id: null });
    expect(parseRoute(`#/media/${id}`)).toEqual({ kind: "media", filter: null, id });
    expect(parseRoute(`#/media/other/${id}`)).toEqual({ kind: "media", filter: "other", id });
    for (const h of ["#/media/photos", `#/media/${id}/images`, "#/media/images/", `#/media/${ID}`]) {
      expect(parseRoute(h)).toEqual({ kind: "home" });
    }
  });

  it("reads artifacts: the list and one", () => {
    const id = "0193f1c2a4b57d6e8f9012345678abcd";
    expect(parseRoute("#/artifacts")).toEqual({ kind: "artifacts", id: null });
    expect(parseRoute(`#/artifacts/${id}`)).toEqual({ kind: "artifacts", id });
    for (const h of ["#/artifacts/new", `#/artifacts/${id}/versions`, "#/search", "#/search/x"]) {
      expect(parseRoute(h)).toEqual({ kind: "home" });
    }
  });

  it("leaves checking a deep link to the registry", () => {
    // Unknown names parse; Settings falls back to what the viewer sees.
    expect(parseRoute("#/settings/server/nope")).toEqual({ kind: "settings", section: "server", tab: "nope" });
    expect(parseRoute("#/settings/Server/users")).toEqual({ kind: "home" });
    expect(parseRoute("#/settings/server/users/more")).toEqual({ kind: "home" });
  });
});

describe("routeHash", () => {
  it("writes what parseRoute reads", () => {
    for (const h of [
      "#/",
      `#/s/${ID}`,
      "#/settings",
      "#/settings/account",
      "#/settings/server/users",
      "#/agents",
      "#/agents/coder",
      "#/routines",
      "#/routines/new",
      "#/routines/0193f1c2a4b57d6e8f9012345678abcd",
      "#/routines/0193f1c2a4b57d6e8f9012345678abcd/runs",
      "#/media",
      "#/media/audio",
      "#/media/0193f1c2a4b57d6e8f9012345678abcd",
      "#/media/images/0193f1c2a4b57d6e8f9012345678abcd",
      "#/artifacts",
      "#/artifacts/0193f1c2a4b57d6e8f9012345678abcd",
    ]) {
      expect(routeHash(parseRoute(h))).toBe(h);
    }
    // A tab without a section has nowhere to go.
    expect(routeHash({ kind: "settings", tab: "users" })).toBe("#/settings");
  });

  it("writes the area links", () => {
    const id = "0193f1c2a4b57d6e8f9012345678abcd";
    expect(routineHash(null)).toBe("#/routines");
    expect(routineHash(id, "runs")).toBe(`#/routines/${id}/runs`);
    // Never a runs tab for a routine not saved yet.
    expect(routineHash("new", "runs")).toBe("#/routines/new");
    expect(mediaHash(null)).toBe("#/media");
    expect(mediaHash("images")).toBe("#/media/images");
    expect(mediaHash("videos", id)).toBe(`#/media/videos/${id}`);
    expect(mediaHash(null, id)).toBe(`#/media/${id}`);
    expect(artifactHash(null)).toBe("#/artifacts");
    expect(artifactHash(id)).toBe(`#/artifacts/${id}`);
  });

  it("gives the links other screens use", () => {
    expect(settingsHash("server", "workspaces")).toBe("#/settings/server/workspaces");
    expect([MEMORY_HASH, NODES_HASH]).toEqual(["#/settings/account/memory", "#/settings/server/nodes"]);
  });
});
