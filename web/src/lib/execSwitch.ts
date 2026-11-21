// When a chat's commands may be switched between the sandbox and the host
// shell (N.10b): only while nothing of the chat runs. The node refuses
// otherwise; this says why before the click, from what the page already
// folds, in the words of what to stop.
import type { SessionView } from "../types";

/** Why the chat's mode cannot be switched now, or null. `running`: a turn
 * runs or is being stopped; `compacting`: a /compact runs; `waiting`: an
 * approval card waits. */
export function whyNotSwitchable(
  view: Pick<SessionView, "background"> | null | undefined,
  state: { running: boolean; compacting: boolean; waiting: boolean },
): string | null {
  if (state.running) return "Stop the agent or wait for it to finish, then switch";
  if (state.waiting) return "Answer the approval first, then switch";
  if (state.compacting) return "The chat is being summarised: switch when that is done";
  const open = Object.values(view?.background ?? {}).filter((b) => !b.ended).length;
  if (open > 0) return `${open === 1 ? "A background command or sub-agent is" : "Background commands or sub-agents are"} still running: stop ${open === 1 ? "it" : "them"} first`;
  return null;
}
