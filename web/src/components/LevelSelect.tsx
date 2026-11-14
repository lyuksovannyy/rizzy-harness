// Sets an execution level (N.10a part 2): an account's ceiling (Settings →
// Users) or a member's level in a workspace (Workspace admin → Members).
// Levels above what the person setting it may grant are off; the server
// still checks every one. Choosing `host` shows the host warning first, and
// nothing is saved unless it is confirmed: cancelling leaves the level as it
// was. Loaded only with those settings pages.
import { lazy, Suspense, useState } from "react";
import type { ExecLevel } from "../types";
import type { HostGrant } from "./HostWarning";

const HostWarning = lazy(() => import("./HostWarning"));

export const LEVELS: ExecLevel[] = ["none", "container", "sandbox", "host"];

export const LEVEL_LABEL: Record<ExecLevel, string> = {
  none: "None",
  container: "Container (not offered yet)",
  sandbox: "Sandbox",
  host: "Host (no sandbox)",
};

/** Whether `a` is at most `b`. */
export const atMost = (a: ExecLevel, b: ExecLevel): boolean => LEVELS.indexOf(a) <= LEVELS.indexOf(b);

export function LevelSelect(props: {
  /** The select's accessible name. */
  label: string;
  value: ExecLevel;
  /** The highest level the person setting it may grant. */
  max: ExecLevel;
  disabled?: boolean;
  /** For the host warning. */
  grant: HostGrant;
  onSave: (level: ExecLevel) => void;
}) {
  const [asking, setAsking] = useState(false);
  return (
    <>
      <select
        aria-label={props.label}
        value={props.value}
        disabled={props.disabled}
        onChange={(e) => {
          const l = e.target.value as ExecLevel;
          if (l === "host") setAsking(true);
          else props.onSave(l);
        }}
      >
        {LEVELS.map((l) => (
          <option key={l} value={l} disabled={!atMost(l, props.max)}>
            {LEVEL_LABEL[l]}
          </option>
        ))}
      </select>
      {asking && (
        <Suspense fallback={null}>
          <HostWarning
            grant={props.grant}
            onConfirm={() => {
              setAsking(false);
              props.onSave("host");
            }}
            onCancel={() => setAsking(false)}
          />
        </Suspense>
      )}
    </>
  );
}

export default LevelSelect;
