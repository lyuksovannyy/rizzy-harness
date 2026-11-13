// Execution modes (N.10a, docs/execution-modes.md): what the UI offers and
// says. The node decides what runs; the master decides who may ask for
// `host`. This only mirrors both so the choice is not offered where it
// would be refused.
import type { NodeInfo } from "../types";

/** Who is asking: a server admin, or the active workspace's owner. */
export interface HostRight {
  admin: boolean;
  owner: boolean;
}

/** Whether `who` may pick `host` for a new chat on `node`: the node offers
 * it, and they are a server admin, or (on a node that is not the server's
 * own) the workspace's owner. Mirrors `workspaces::exec_mode_for`. */
export function mayPickHost(node: NodeInfo | undefined, who: HostRight | undefined): boolean {
  if (!node || !who || !node.exec_modes?.includes("host")) return false;
  return who.admin || (!node.embedded && who.owner);
}
