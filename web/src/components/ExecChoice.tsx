// The new chat's execution mode (N.10a): always shown (the owner,
// 2026-11-17, X.26), a choice only where the node offers `host` and the
// person may pick it, a plain label otherwise; `host` is kept only once its
// warning was confirmed. Cancelling leaves the most confined mode, and
// nothing is created until the message is sent.
import { lazy, Suspense, useState } from "react";
import { MODE_LABEL } from "../lib/execWords";
import type { ExecMode } from "../types";

const HostWarning = lazy(() => import("./HostWarning"));

export function ExecChoice(props: {
  /** The person may pick `host` on this node (`mayPickHost`); else a label. */
  pickable?: boolean;
  node: string;
  /** What the node offers, most confined first. */
  offered: ExecMode[];
  /** `host` only once confirmed; otherwise `undefined`. */
  value: ExecMode | undefined;
  onChange: (m: ExecMode | undefined) => void;
}) {
  const [asking, setAsking] = useState(false);
  if (props.pickable === false)
    return (
      <label>
        Commands
        <span className="pill exec-only" aria-label="How this chat's commands run">
          {props.offered.includes("sandbox") ? MODE_LABEL.sandbox : "no sandbox on this node"}
        </span>
      </label>
    );
  return (
    <>
      <label>
        Commands
        <select
          className="pill"
          aria-label="How this chat's commands run"
          value={props.value ?? "sandbox"}
          onChange={(e) => {
            if (e.target.value === "host") setAsking(true);
            else props.onChange(undefined);
          }}
        >
          <option value="sandbox" disabled={!props.offered.includes("sandbox")}>
            {props.offered.includes("sandbox") ? MODE_LABEL.sandbox : `${MODE_LABEL.sandbox} (not on this node)`}
          </option>
          <option value="host">{MODE_LABEL.host} (no sandbox)</option>
        </select>
      </label>
      {asking && (
        <Suspense fallback={null}>
          <HostWarning
            node={props.node}
            onConfirm={() => {
              setAsking(false);
              props.onChange("host");
            }}
            onCancel={() => {
              setAsking(false);
              props.onChange(undefined);
            }}
          />
        </Suspense>
      )}
    </>
  );
}

export default ExecChoice;
