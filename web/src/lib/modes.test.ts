import { describe, expect, test } from "vitest";
import type { WorkspaceSummary } from "../types";
import { hostRightOf } from "./modes";

const ws = (exec_level: "none" | "container" | "sandbox" | "host") => ({ exec_level }) as Pick<WorkspaceSummary, "exec_level">;

describe("hostRightOf: one rule for the start screen and the project page", () => {
  test("no workspace (the API token): the account's ceiling", () => {
    expect(hostRightOf({ exec_ceiling: "host" })).toEqual({ level: "host" });
    expect(hostRightOf({ exec_ceiling: "sandbox", workspace: null })).toEqual({ level: "sandbox" });
  });
  test("a workspace's level, when it is lower than the ceiling", () => {
    expect(hostRightOf({ exec_ceiling: "host", workspace: ws("sandbox") })).toEqual({ level: "sandbox" });
  });
  test("a workspace at host under a host ceiling", () => {
    expect(hostRightOf({ exec_ceiling: "host", workspace: ws("host") })).toEqual({ level: "host" });
  });
});

import { agenticCapable } from "./agentDefaults";
test("the coding-chat default lists only agents that can change files or run commands and may answer", () => {
  const a = (id: string, f: object) => ({ id, features: { as_agent: true, edit_files: false, commands: false, ...f } }) as never;
  const list = [a("writer", {}), a("coder", { edit_files: true }), a("ops", { commands: true }), a("tag", { commands: true, as_agent: false })];
  expect(agenticCapable(list).map((x: { id: string }) => x.id)).toEqual(["coder", "ops"]);
});
