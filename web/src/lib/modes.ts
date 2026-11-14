import type { ExecLevel, WorkspaceSummary } from "../types";
import { type Key, t } from "./i18n";
import { workspaceName } from "./workspaces";

export type Mode = "search" | "ask" | "agentic";
export const MODES: Mode[] = ["search", "ask", "agentic"];

/** Whether a level runs any agent chat: `container` runs none until a node
 * offers containers (N.4). */
export const runsChats = (l: ExecLevel | undefined): boolean => l === "sandbox" || l === "host";

/** Why agentic is unavailable, or null (N.10a part 2): the account's ceiling
 * first (server admins set it), then the effective level in the workspace
 * (its admins set the member's part; no workspace, as for the API token,
 * adds none), then a worker to run on. */
export function agenticBlockReason(
  ceiling: ExecLevel,
  workspace: Pick<WorkspaceSummary, "name" | "personal" | "exec_level"> | null | undefined,
  workersOnline: number,
): string | null {
  const why = (key: Key, level: ExecLevel) =>
    t(key, { level, workspace: workspace ? workspaceName(workspace) : "" }) +
    (level === "container" ? ` ${t("agentic.noContainers")}` : "");
  if (!runsChats(ceiling)) return why("agentic.noGrant", ceiling);
  if (workspace && !runsChats(workspace.exec_level)) return why("agentic.noRole", workspace.exec_level);
  if (workersOnline === 0) return t("agentic.noWorker");
  return null;
}

export function enabledModes(agenticBlocked: string | null): Mode[] {
  return MODES.filter((m) => m !== "agentic" || agenticBlocked === null);
}

export function cycleMode(current: Mode, enabled: Mode[], dir: 1 | -1): Mode {
  const i = enabled.indexOf(current);
  if (i < 0) return enabled[0] ?? "ask";
  return enabled[(i + dir + enabled.length) % enabled.length]!;
}
