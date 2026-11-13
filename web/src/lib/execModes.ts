// Execution modes (N.10a, docs/execution-modes.md): what the UI offers and
// says. The node decides what runs; the master decides who may ask for
// `host`. This only mirrors both so the choice is not offered where it
// would be refused.
import type { NodeInfo } from "../types";

/** Who is asking: whether they are a server admin. */
export interface HostRight {
  admin: boolean;
}

/** Whether `who` may pick `host` for a new chat on `node`: the node offers
 * it, and they are a server admin (until per-member levels exist; review
 * H1: every account owns a workspace, so ownership is no reason). Mirrors
 * `workspaces::exec_mode_for`. */
export function mayPickHost(node: NodeInfo | undefined, who: HostRight | undefined): boolean {
  return !!node?.exec_modes?.includes("host") && !!who?.admin;
}
