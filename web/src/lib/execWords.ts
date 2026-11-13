// What the chat header and the mode choice say about execution modes
// (N.10a). Only lazy chunks import it; the first load carries
// `execModes.ts` alone.
import type { ExecMode } from "../types";

export const MODE_LABEL: Record<ExecMode, string> = {
  sandbox: "sandbox",
  host: "host shell",
};

const NETWORK_GAP = "the network is off, but without a network namespace: UDP and DNS still go out";

/** Each gap a node announces, in the words the chat shows (reviews T8B-3,
 * L-d, L-e), by the mode it belongs to. */
const GAP_WORDS: [ExecMode, string, string][] = [
  ["sandbox", "gap:sandbox:netns", NETWORK_GAP],
  [
    "sandbox",
    "gap:sandbox:sockets",
    "on this kernel (below Linux 7.1) a command can ask a service of the node's user, over a socket in its folders, to start a process outside the sandbox",
  ],
  ["sandbox", "gap:sandbox:proc", "the sandbox has no /proc of its own here: commands can see this machine's processes"],
  ["host", "gap:host:netns", NETWORK_GAP],
  ["host", "gap:host:net-open", "network = off is not applied to this chat: everything goes out"],
];

/** The gaps of a chat in `mode` on a node that announced `gaps`, in plain
 * words. A `host` chat is never contained. What the node said at its last
 * hello: a gap it closes later still shows until it connects again. */
export function modeGaps(mode: ExecMode | undefined, gaps: string[] | undefined): string[] {
  const out: string[] = [];
  if (mode === "host") out.push("not contained: its commands run as the node's own user, without the sandbox");
  for (const [m, gap, words] of GAP_WORDS) if (m === mode && gaps?.includes(gap)) out.push(words);
  return out;
}
