import { useState } from "react";
import { api } from "../api";
import type { Decision, Item } from "../types";

type Tool = Extract<Item, { type: "tool" }>;

function summary(t: Tool): string {
  const i = t.input ?? {};
  switch (t.tool) {
    case "bash":
      return i.command ?? "";
    case "read":
      return `${i.path ?? ""}${i.offset ? `:${i.offset}` : ""}`;
    case "write":
    case "edit":
      return i.path ?? "";
    case "grep":
      return `/${i.pattern ?? ""}/ ${i.path ?? ""}${i.glob ? ` (${i.glob})` : ""}`;
    case "glob":
      return `${i.pattern ?? ""} ${i.path ?? ""}`;
    default:
      return JSON.stringify(i);
  }
}

function EditDiff({ input }: { input: { old_string?: string; new_string?: string } }) {
  const old = (input.old_string ?? "").split("\n");
  const neu = (input.new_string ?? "").split("\n");
  return (
    <pre className="diff">
      {old.map((l, i) => (
        <div key={`o${i}`} className="del">
          - {l}
        </div>
      ))}
      {neu.map((l, i) => (
        <div key={`n${i}`} className="add">
          + {l}
        </div>
      ))}
    </pre>
  );
}

export function ToolCard({ sessionId, item }: { sessionId: string; item: Tool }) {
  const pending = item.approval && item.approval.decision === null;
  const [open, setOpen] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const status = pending
    ? "waiting"
    : item.result
      ? item.result.ok
        ? "ok"
        : "failed"
      : item.approval?.decision === "deny"
        ? "denied"
        : "running";
  // Open while it matters; collapse finished successful calls by default.
  const expanded = open ?? (status !== "ok" || item.tool === "edit");

  const decide = async (d: Decision) => {
    if (!item.approval) return;
    setBusy(true);
    setErr(null);
    try {
      await api.approve(sessionId, item.approval.approval_id, d);
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    } finally {
      setBusy(false);
    }
  };

  const output = item.result ? item.result.output : item.live_output;

  return (
    <div className={`tool tool-${status}`}>
      <button className="tool-head" onClick={() => setOpen(!expanded)}>
        <span className={`tool-status s-${status}`}>{status}</span>
        <span className="tool-name">{item.tool}</span>
        <span className="tool-summary mono ellipsis">{summary(item)}</span>
        {item.result?.exit_code !== null && item.result?.exit_code !== undefined && (
          <span className="muted mono">exit {item.result.exit_code}</span>
        )}
        <span className="chev">{expanded ? "▾" : "▸"}</span>
      </button>
      {pending && item.approval && (
        <div className="approval">
          <div className="approval-reason">{item.approval.reason}</div>
          <div className="row">
            <button className="primary" disabled={busy} onClick={() => decide("allow")}>
              Allow
            </button>
            <button disabled={busy} onClick={() => decide("allow_session")}>
              Allow {item.tool} for this session
            </button>
            <button className="danger" disabled={busy} onClick={() => decide("deny")}>
              Deny
            </button>
          </div>
          {err && <div className="error-text">{err}</div>}
        </div>
      )}
      {item.approval && item.approval.decision !== null && item.approval.by !== "user" && (
        <div className="approval-note muted">
          {item.approval.decision === "deny" ? "Denied" : "Allowed"} ({item.approval.by})
        </div>
      )}
      {expanded && (
        <div className="tool-body">
          {item.tool === "edit" && <EditDiff input={item.input} />}
          {item.tool === "write" && (
            <pre className="term">{String(item.input?.content ?? "").slice(0, 4000)}</pre>
          )}
          {item.tool === "bash" && <pre className="term cmd">$ {item.input?.command}</pre>}
          {output && <pre className="term">{output}</pre>}
        </div>
      )}
    </div>
  );
}
