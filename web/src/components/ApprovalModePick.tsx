// A chat's approval mode (T48, README "Approval modes"): how much runs
// without a click. One compact control in the composer: the current mode,
// and the choices with one line each saying what runs unasked. A choice the
// node does not allow is shown disabled with the reason. The node decides:
// it clamps whatever is sent, and the chat shows what it recorded.
import { Select } from "./Select";
import type { ApprovalMode } from "../types";

const RANK: Record<ApprovalMode, number> = { ask_all: 0, ask: 1, accept_edits: 2, auto: 3 };

export const MODE_WORDS: Record<ApprovalMode, { label: string; hint: string }> = {
  ask_all: { label: "Ask every call", hint: "Asks before every tool call, reads included" },
  ask: { label: "Ask", hint: "Asks before every edit and every command" },
  accept_edits: { label: "Accept edits", hint: "Edits files in this folder without asking (undo per turn); commands still ask" },
  auto: { label: "Auto", hint: "Runs edits and commands without asking, inside the sandbox" },
};

/** Why `mode` cannot be chosen here, or null. */
export function whyNot(mode: ApprovalMode, max: ApprovalMode, host: boolean): string | null {
  if (host && mode === "auto") return "Never in a host chat: its commands run without the sandbox";
  if (RANK[mode] > RANK[max]) return "Off on this node (max_approval_mode in its node.toml)";
  return null;
}

/** The loosest of `mode` and what is allowed (what the node will give). */
export function clampMode(mode: ApprovalMode, max: ApprovalMode, host: boolean): ApprovalMode {
  const limit: ApprovalMode = host && RANK[max] > RANK.accept_edits ? "accept_edits" : max;
  return RANK[mode] > RANK[limit] ? limit : mode;
}

export function ApprovalModePick(props: {
  value: ApprovalMode;
  /** The node's ceiling (`approval-max`). */
  max: ApprovalMode;
  /** A host chat: never `auto`. */
  host: boolean;
  onChange: (m: ApprovalMode) => void;
}) {
  const value = clampMode(props.value, props.max, props.host);
  const shown: ApprovalMode[] = ["ask", "accept_edits", "auto"];
  if (value === "ask_all") shown.unshift("ask_all");
  return (
    <span className="approval-pick" title={MODE_WORDS[value].hint}>
      <Select
        compact
        stacked
        label="Approval mode"
        value={value}
        options={shown.map((m) => {
          const why = whyNot(m, props.max, props.host);
          return { value: m, label: MODE_WORDS[m].label, hint: why ?? MODE_WORDS[m].hint, disabled: !!why };
        })}
        onChange={(v) => props.onChange(v as ApprovalMode)}
      />
    </span>
  );
}

export default ApprovalModePick;
