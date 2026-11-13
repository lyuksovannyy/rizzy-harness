// The new chat's execution mode (N.10a): offered only where the node offers
// `host` and the person may pick it; `host` is kept only once its warning
// was confirmed. Cancelling leaves the most confined mode, and nothing is
// created until the message is sent.
import { lazy, Suspense, useState } from "react";
import { MODE_LABEL } from "../lib/execWords";
import type { ExecMode } from "../types";

const HostWarning = lazy(() => import("./HostWarning"));

export function ExecChoice(props: {
  node: string;
  /** What the node offers, most confined first. */
  offered: ExecMode[];
  /** `host` only once confirmed; otherwise `undefined`. */
  value: ExecMode | undefined;
  onChange: (m: ExecMode | undefined) => void;
}) {
  const [asking, setAsking] = useState(false);
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
