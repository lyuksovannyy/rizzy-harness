// Execution modes (N.10a, docs/execution-modes.md): what the UI offers and
// says. The node decides what runs; the master decides who may ask for
// which mode, by their level. This only mirrors both so the choice is not
// offered where it would be refused.
import type { ExecLevel, NodeInfo } from "../types";

/** Who is asking: their effective level in the workspace the chat goes in. */
export interface HostRight {
  level: ExecLevel;
}

/** Whether `who` may pick `host` for a new chat on `node`: the node offers
 * it, and their effective level there is `host` (N.10a part 2). Mirrors
 * `levels::create_mode`. */
export function mayPickHost(node: NodeInfo | undefined, who: HostRight | undefined): boolean {
  return !!node?.exec_modes?.includes("host") && who?.level === "host";
}
