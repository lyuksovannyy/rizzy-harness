import type { WorkspaceSummary } from "../types";
import { t } from "./i18n";
import { workspaceName } from "./workspaces";

export type Mode = "search" | "ask" | "agentic";
export const MODES: Mode[] = ["search", "ask", "agentic"];

/** Why agentic is unavailable, or null. The account's server grant comes
 * first (the security boundary), then the workspace role's `agentic`
 * permission (the team's rule; no workspace, as for the API token, adds
 * none), then a worker to run on. */
export function agenticBlockReason(
  canAgentic: boolean,
  workspace: Pick<WorkspaceSummary, "name" | "personal" | "permissions"> | null | undefined,
  workersOnline: number,
): string | null {
  if (!canAgentic) return t("agentic.noGrant");
  if (workspace && !workspace.permissions.includes("agentic")) {
    return t("agentic.noRole", { workspace: workspaceName(workspace) });
  }
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
