// "MCP servers from the repository" on a project's Settings tab
// (ADOPTION_PLAN X.36, docs/projects.md, PLAN.md T82): the servers the
// project folder's `.mcp.json` declares, as the project's node reads them
// for you, each with what it runs (in full, as text), the names of its
// environment (never a value) and where it stands for you. Approve and
// Remove are yours alone: an approval covers your own sandboxed chats in
// this folder. Reload asks the node to read the file again; nothing else
// does. Part of the project page's lazy chunk.
import { useCallback, useEffect, useState } from "react";
import { call } from "../api";
import "./ProjectMcp.css";

export type McpState = "approved" | "changed" | "new" | "removed" | "unsupported" | "hidden";

export interface McpEntry {
  name: string;
  state: McpState;
  command: string;
  args: string[];
  env_keys: string[];
  digest?: string;
  reason?: string;
  approved_command?: string;
  approved_args?: string[];
  approved_env_keys?: string[];
}

export interface McpList {
  node_id: string;
  unreadable?: string;
  missing?: boolean;
  entries: McpEntry[];
  note?: string;
}

const LABEL: Record<McpState, string> = {
  approved: "Approved",
  changed: "Changed in the file",
  new: "Not approved",
  removed: "Gone from the file",
  unsupported: "Not supported",
  hidden: "Hidden",
};

const WHAT: Partial<Record<McpState, string>> = {
  approved: "Runs in your sandboxed chats in this folder.",
  changed: "Your approved copy keeps running until you approve the file's new entry.",
  new: "Does not run until you approve it.",
  removed: "Not started; remove your approval to forget it.",
};

/** A command line as text: an argument with spaces or quotes is quoted. */
export function commandLine(command: string, args: string[]): string {
  const q = (a: string) => (a === "" || /[\s"'\\$`]/.test(a) ? `'${a.replace(/'/g, `'\\''`)}'` : a);
  return [command, ...args].map(q).join(" ");
}

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

export default function ProjectMcp(props: { projectId: string; hasFolder: boolean }) {
  const [list, setList] = useState<McpList | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const base = `/api/projects/${encodeURIComponent(props.projectId)}/mcp`;
  const run = useCallback(async (p: Promise<McpList>) => {
    setBusy(true);
    setError(null);
    try {
      setList(await p);
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  }, []);
  const load = useCallback(() => run(call<McpList>("GET", base)), [base, run]);
  useEffect(() => {
    if (props.hasFolder) void load();
  }, [props.hasFolder, load]);
  const approve = (e: McpEntry) => run(call<McpList>("POST", `${base}/approve`, { name: e.name, digest: e.digest }));
  const remove = (e: McpEntry) => run(call<McpList>("POST", `${base}/remove`, { name: e.name }));
  return (
    <section className="project-mcp stack" aria-label="MCP servers from the repository">
      <h3>MCP servers from the repository</h3>
      <p className="field-hint">
        The servers this project's folder declares in <code>.mcp.json</code>. One runs only after you approve it,
        only in your own sandboxed chats in this folder (never in a host chat), exactly as you approved it: a later
        change to the file runs only once you approve it again. Approving pins the command, not the files it runs.
      </p>
      {!props.hasFolder && <p className="muted">This project has no folder on a node, so it has no .mcp.json.</p>}
      {error && <p className="error-text">{error}</p>}
      {list?.note && <p className="error-text">{list.note}</p>}
      {list?.unreadable && (
        <p className="muted">
          The file could not be read: {list.unreadable}. Your approved copies still run as you approved them.
        </p>
      )}
      {list?.missing && <p className="muted">The folder has no .mcp.json.</p>}
      {list && list.entries.length === 0 && !list.missing && !list.unreadable && (
        <p className="muted">The file declares no servers.</p>
      )}
      {list && list.entries.length > 0 && (
        <ul className="mcp-list">
          {list.entries.map((e) => (
            <li key={`${e.state}:${e.name}`} className={`mcp-entry state-${e.state}`}>
              <div className="row">
                <strong className="mono">{e.name}</strong>
                <span className="tag">{LABEL[e.state] ?? e.state}</span>
                <span className="grow" />
                {(e.state === "new" || e.state === "changed") && e.digest && (
                  <button type="button" className="primary" disabled={busy} onClick={() => void approve(e)}>
                    {e.state === "changed" ? "Approve the change" : "Approve"}
                  </button>
                )}
                {(e.state === "approved" || e.state === "changed" || e.state === "removed") && (
                  <button type="button" disabled={busy} onClick={() => void remove(e)}>
                    Remove
                  </button>
                )}
              </div>
              {e.reason && <p className="muted small">{e.reason}</p>}
              {WHAT[e.state] && <p className="muted small">{WHAT[e.state]}</p>}
              {e.command && <pre className="mono mcp-cmd">{commandLine(e.command, e.args)}</pre>}
              {e.env_keys.length > 0 && (
                <p className="muted small">Environment: {e.env_keys.join(", ")} (values not shown)</p>
              )}
              {e.state === "changed" && e.approved_command && (
                <>
                  <p className="muted small">What runs now (your approved copy):</p>
                  <pre className="mono mcp-cmd">{commandLine(e.approved_command, e.approved_args ?? [])}</pre>
                  {(e.approved_env_keys ?? []).length > 0 && (
                    <p className="muted small">Environment: {(e.approved_env_keys ?? []).join(", ")}</p>
                  )}
                </>
              )}
            </li>
          ))}
        </ul>
      )}
      {props.hasFolder && (
        <div className="row">
          <button type="button" disabled={busy} onClick={() => void load()}>
            Reload
          </button>
          <span className="field-hint">Reads the file again; nothing else does, and no change to it starts anything.</span>
        </div>
      )}
    </section>
  );
}
