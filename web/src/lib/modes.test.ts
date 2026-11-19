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
