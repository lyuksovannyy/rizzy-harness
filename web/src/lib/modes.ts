import { t } from "./i18n";

export type Mode = "search" | "ask" | "agentic";
export const MODES: Mode[] = ["search", "ask", "agentic"];

/** Why agentic is unavailable, or null. */
export function agenticBlockReason(canAgentic: boolean, workersOnline: number): string | null {
  if (!canAgentic) return t("agentic.noGrant");
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
