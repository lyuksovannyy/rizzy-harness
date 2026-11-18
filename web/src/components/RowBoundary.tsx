// One conversation item's error boundary (review of PR 156, MEDIUM 4): a row
// that throws while rendering shows one line in its place, and the rest of
// the chat (the other rows, the composer) stays. A tool call that waits for
// an answer keeps a Deny button: the call could not be shown, so it can be
// refused but not approved blind. The error is logged, never shown.
import { Component, type ReactNode } from "react";
import { pageApi } from "../apiPages";
import { rt } from "../md/strings";
import type { Item } from "../types";

interface Props {
  children: ReactNode;
  item: Item;
  sessionId: string;
  /** False where the viewer may not answer (a read-only chat). */
  canAnswer: boolean;
}

export class RowBoundary extends Component<Props, { failed: boolean }> {
  state = { failed: false };

  static getDerivedStateFromError() {
    return { failed: true };
  }

  componentDidCatch(e: unknown) {
    console.error(e);
  }

  render() {
    return this.state.failed ? <RowFailed {...this.props} /> : this.props.children;
  }
}

/** The line a row that could not be shown leaves. */
export function RowFailed({ item, sessionId, canAnswer }: Omit<Props, "children">) {
  const waiting = item.type === "tool" && item.approval && item.approval.decision === null ? item.approval : null;
  return (
    <div className="ui-error" role="note" data-testid="row-failed">
      {waiting ? rt("ui.unshownApproval") : rt("ui.unshown")}
      {waiting && canAnswer && (
        <>
          {" "}
          <button type="button" className="danger small" onClick={() => void pageApi.approve(sessionId, waiting.approval_id, "deny")}>
            {rt("ui.deny")}
          </button>
        </>
      )}
    </div>
  );
}
