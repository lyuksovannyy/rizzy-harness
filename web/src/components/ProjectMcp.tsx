// "MCP servers from the repository" on a project's Settings tab
// (ADOPTION_PLAN X.36, docs/projects.md, PLAN.md T82): the servers the
// project folder's `.mcp.json` declares, as the project's node reads them
// for you, each with what it runs in full, as text: the command, then each
// argument on its own line, then each `NAME=value` of its environment
// (repository content, part of what runs), nothing joined, quoted or cut
// (the node makes an entry it could not show so unsupported), and where
// it stands for you. Approve and
// Remove are yours alone: an approval covers your own chats in this
// folder, and the page says where the node runs them (inside the sandbox,
// or as the node's own user in a host chat, which the node offers or not),
// once, above the list. Reload asks the node to read the
// file again; nothing else does. Part of the project page's lazy chunk.
import { own } from "../emptyView";
import { useCallback, useEffect, useState } from "react";
import { call, HttpError } from "../api";
import type { ExecMode } from "../types";
import "./ProjectMcp.css";

export type McpState = "approved" | "changed" | "new" | "removed" | "unsupported" | "hidden";

export interface McpEnv {
  name: string;
  value: string;
}

export interface McpEntry {
  name: string;
  state: McpState;
  command: string;
  args: string[];
  env: McpEnv[];
  digest?: string;
  reason?: string;
  approved_command?: string;
  approved_args?: string[];
  approved_env?: McpEnv[];
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

/** Where an approved server runs, by the execution modes the node offers.
 * Not known (no list yet, the node not in it, modes blanked): says so, never
 * the sandbox, next to an Approve button. */
export function whereItRuns(modes?: ExecMode[]): string {
  if (!modes || modes.length === 0) {
    return "Where an approved server runs is not known yet: this node's modes have not loaded.";
  }
  if (!modes.includes("host")) return "An approved server runs in your chats in this folder, inside the sandbox.";
  if (!modes.includes("sandbox")) {
    return "An approved server runs in your chats in this folder as the node's own user, without the sandbox: every chat on this node is a host chat.";
  }
  return "An approved server runs in your chats in this folder: inside the sandbox in a sandboxed chat, as the node's own user without the sandbox in a host chat.";
}

const HOST_ENV = "In a host chat this entry is not started while it has environment values: the chat is told so.";

const WHAT: Partial<Record<McpState, string>> = {
  changed: "Your approved copy keeps running until you approve the file's new entry.",
  new: "Does not run until you approve it.",
  removed: "Not started; remove your approval to forget it.",
};

/** What an entry runs, one line each, exactly as the node sent it. */
function Runs(props: { command: string; args: string[]; env: McpEnv[]; label: string }) {
  return (
    <dl className="mcp-runs" aria-label={props.label}>
      <dt>Command</dt>
      <dd>
        <code className="mcp-line">{props.command}</code>
      </dd>
      {props.args.length > 0 && <dt>Arguments</dt>}
      {props.args.map((a, i) => (
        <dd key={`a${i}`}>
          <code className="mcp-line mcp-arg">{a}</code>
        </dd>
      ))}
      {props.env.length > 0 && <dt>Environment</dt>}
      {props.env.map((e, i) => (
        <dd key={`e${i}`}>
          <code className="mcp-line mcp-env">{`${e.name}=${e.value}`}</code>
        </dd>
      ))}
    </dl>
  );
}

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** The node's answer when its node.toml keeps project servers off: a 409
 * naming the setting (`crates/node/src/project_mcp.rs` OFF). */
const isOff = (e: unknown) => e instanceof HttpError && e.status === 409 && e.message.includes("project_servers");

export default function ProjectMcp(props: { projectId: string; hasFolder: boolean; modes?: ExecMode[] }) {
  const host = !!props.modes?.includes("host");
  const [list, setList] = useState<McpList | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [off, setOff] = useState(false);
  const [busy, setBusy] = useState(false);
  const base = `/api/projects/${encodeURIComponent(props.projectId)}/mcp`;
  const run = useCallback(async (p: Promise<McpList>) => {
    setBusy(true);
    setError(null);
    setOff(false);
    try {
      setList(await p);
    } catch (e) {
      if (isOff(e)) setOff(true);
      else setError(message(e));
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
        only in your own chats in this folder, exactly as you approved it: a later change to the file runs only once
        you approve it again. Approving pins the command, its arguments and its environment as shown here, not the
        files it runs.
      </p>
      {!off && <p className="field-hint">{whereItRuns(props.modes)}</p>}
      {!props.hasFolder && <p className="muted">This project has no folder on a node, so it has no .mcp.json.</p>}
      {off && (
        <p className="muted mcp-off">
          Project MCP servers are off on this node. Setting <code>project_servers = "ask"</code> under <code>[mcp]</code> in its
          node.toml turns them on (the default is "off").
        </p>
      )}
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
                <span className="tag">{own(LABEL, e.state) ?? e.state}</span>
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
              {own(WHAT, e.state) && <p className="muted small">{own(WHAT, e.state)}</p>}
              {host && (e.env.length > 0 || (e.approved_env?.length ?? 0) > 0) && ["new", "approved", "changed"].includes(e.state) && (
                <p className="muted small">{HOST_ENV}</p>
              )}
              {e.command && <Runs label="In the file" command={e.command} args={e.args} env={e.env} />}
              {e.state === "changed" && e.approved_command && (
                <>
                  <p className="muted small">What runs now (your approved copy):</p>
                  <Runs
                    label="Your approved copy"
                    command={e.approved_command}
                    args={e.approved_args ?? []}
                    env={e.approved_env ?? []}
                  />
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
