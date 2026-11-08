// Browser notifications for routine results (docs/routines.md): a new
// finished, failed or paused row in attention notifies as the routine's
// `notify` says, and a run's chat on screen counts as seen. Shell calls it on
// every render; it does nothing until routines land.
import type { Route } from "./routes";
import type { RoutineAttention } from "../types";

export function useRoutineResults(_opts: { route: Route; routines: RoutineAttention[]; workspaceId: string | null }): void {}
