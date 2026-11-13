// What the chat header and the mode choice say about execution modes
// (N.10a). Only lazy chunks import it; the first load carries
// `execModes.ts` alone.
import type { ExecMode } from "../types";

export const MODE_LABEL: Record<ExecMode, string> = {
  sandbox: "sandbox",
  host: "host shell",
};

const NETWORK_GAP = "the network is off, but without a network namespace: UDP and DNS still go out";

/** The gaps of a chat in `mode` on a node that announced `gaps`, in plain
 * words (review T8B-3). A `host` chat is never contained. */
export function modeGaps(mode: ExecMode | undefined, gaps: string[] | undefined): string[] {
  const out: string[] = [];
  if (mode === "host") {
    out.push("not contained: its commands run as the node's own user, without the sandbox");
    if (gaps?.includes("gap:host:netns")) out.push(NETWORK_GAP);
  }
  if (mode === "sandbox" && gaps?.includes("gap:sandbox:netns")) out.push(NETWORK_GAP);
  return out;
}
