// A chat's approval mode (T50, README "Approval modes"): how much runs
// without a click. One compact control in the composer: the current mode,
// and the choices with one line each saying what runs unasked. A choice the
// node does not allow is shown disabled with the reason. The node decides:
// it clamps whatever is sent, and the chat shows what it recorded.
import { Select } from "./Select";
import type { ApprovalMode } from "../types";

const RANK: Record<ApprovalMode, number> = { plan: -1, ask_all: 0, ask: 1, accept_edits: 2, auto: 3 };

export const MODE_WORDS: Record<ApprovalMode, { label: string; hint: string }> = {
  plan: { label: "Plan", hint: "Looks and plans; changes nothing (the shell is read-only). Only you leave it" },
  ask_all: { label: "Ask every call", hint: "Asks before every tool call, reads included" },
  ask: { label: "Ask", hint: "Asks before every edit and every command" },
  accept_edits: { label: "Accept edits", hint: "Edits files in this folder without asking (undo per turn); commands still ask" },
  auto: { label: "Auto", hint: "Runs edits and commands without asking (inside the sandbox)" },
};

/** Auto in a host chat: said plainly (the owner's decision, 2026-11-14). */
const HOST_AUTO = "Runs every command as the node's own user, no sandbox, with no question asked";

/** Why `mode` cannot be chosen here, or null: only the node's ceiling. */
export function whyNot(mode: ApprovalMode, max: ApprovalMode): string | null {
  return RANK[mode] > RANK[max] ? "Off on this node (max_approval_mode in its node.toml)" : null;
}

/** The loosest of `mode` and what the node allows (what it will give). */
export function clampMode(mode: ApprovalMode, max: ApprovalMode): ApprovalMode {
  return RANK[mode] > RANK[max] ? max : mode;
}

export function ApprovalModePick(props: {
  value: ApprovalMode;
  /** The node's ceiling (`approval-max`). */
  max: ApprovalMode;
  /** A host chat: Auto's line says what it means there. */
  host: boolean;
  onChange: (m: ApprovalMode) => void;
}) {
  const value = clampMode(props.value, props.max);
  const hint = (m: ApprovalMode) => (props.host && m === "auto" ? HOST_AUTO : MODE_WORDS[m].hint);
  const shown: ApprovalMode[] = ["plan", "ask", "accept_edits", "auto"];
  if (value === "ask_all") shown.unshift("ask_all");
  return (
    <span className="approval-pick" title={hint(value)}>
      <Select
        compact
        stacked
        label="Approval mode"
        value={value}
        options={shown.map((m) => {
          const why = whyNot(m, props.max);
          return { value: m, label: MODE_WORDS[m].label, hint: why ?? hint(m), disabled: !!why };
        })}
        onChange={(v) => props.onChange(v as ApprovalMode)}
      />
    </span>
  );
}

export default ApprovalModePick;
