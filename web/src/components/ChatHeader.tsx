// The chat's header: one line. The title (a click renames it: ChatTitle),
// then a muted suffix, "project · node · how commands run", and at the right
// Export and Delete, with a slot for one more small button. The agent and the
// model are in the composer's strip, the figures too; the folder is the
// suffix's hover text. The project in the suffix is the control the header's
// chip was (ProjectChip), the node and the execution state are the badges'
// words (NodeBadge, ExecBadge). Banners and the plan bar stay in the chat.
import { Download, Trash2 } from "lucide-react";
import { Fragment, type ReactNode } from "react";
import "../lib/i18nPages";
import { t } from "../lib/i18n";
import type { ProjectSummary } from "../lib/projects";
import { shownName } from "../md/hidden";
import type { ExecMode, NodeInfo, SessionRow } from "../types";
import { ChatTitle } from "./ChatTitle";
import { ExecBadge } from "./ExecBadge";
import { GitChip } from "./GitChip";
import { NodeBadge, nodeLabel } from "./NodeBadge";
import { ProjectChip, showsProjectChip } from "./ProjectChip";

export interface ChatHeaderProps {
  sessionId: string;
  title: string;
  canRename: boolean;
  row: SessionRow | null;
  projects: ProjectSummary[];
  /** A chat with a folder and a node shell: the node and the execution state apply. */
  agentic: boolean;
  node?: NodeInfo;
  nodeId: string;
  execMode?: ExecMode;
  /** The log names a mode this page does not read: the chat runs nothing. */
  execUnknown?: boolean;
  cwd?: string;
  /** A turn is running: the git chip is read again when it ends. */
  running?: boolean;
  exportDisabled: boolean;
  onExport: () => void;
  onDelete: () => void;
  /** One more small button, drawn before Export (another feature's). */
  extra?: ReactNode;
}

export function ChatHeader(p: ChatHeaderProps) {
  const project = p.row && showsProjectChip(p.row, p.projects) ? p.row : null;
  const current = project ? (p.projects.find((x) => x.project_id === project.project_id) ?? null) : null;
  const shell = p.agentic && (p.execMode || p.execUnknown);
  const cwd = p.cwd ? shownName(p.cwd) : "";
  // The parts, in order; a part that does not apply leaves no separator.
  const parts: [string, ReactNode][] = [];
  if (project) parts.push(["project", <ProjectChip key="project" row={project} projects={p.projects} />]);
  if (p.agentic) parts.push(["node", <NodeBadge key="node" node={p.node} nodeId={p.nodeId} />]);
  if (shell && p.execUnknown && !p.execMode)
    parts.push([
      "exec",
      <span key="exec" className="badge exec-mode badge-warn" title="This chat's execution mode is not known here, so it runs nothing">
        mode unknown
      </span>,
    ]);
  else if (shell) parts.push(["exec", <ExecBadge key="exec" mode={p.execMode} node={p.node} inline />]);
  // The same, in words, for a screen reader.
  const words = [
    project && (current ? t("head.inProject", { name: shownName(current.name) }) : t("head.noProject")),
    p.agentic && t("head.onNode", { name: nodeLabel(p.node, p.nodeId) }),
    shell && (p.execMode === "host" ? t("head.hostShell") : p.execMode === "sandbox" ? t("head.sandboxed") : "commands: mode unknown, runs nothing"),
    p.agentic && cwd && t("head.folder", { path: cwd }),
  ].filter((x): x is string => !!x);
  const sentence = words.join(", ").replace(/^./, (c) => c.toUpperCase());
  return (
    <header className="conv-head">
      <div className="conv-title">
        {/* The row's title: a rename changes only the row (the log keeps the first). */}
        <ChatTitle sessionId={p.sessionId} title={p.title} canRename={p.canRename} />
        {parts.length > 0 && (
          <span className="chat-suffix muted" title={p.agentic && cwd ? cwd : undefined}>
            {parts.map(([k, el], i) => (
              <Fragment key={k}>
                {i > 0 && (
                  <span className="chat-sep" aria-hidden="true">
                    ·
                  </span>
                )}
                <span className="chat-part" aria-hidden={k === "project" ? undefined : true}>
                  {el}
                </span>
              </Fragment>
            ))}
            {p.agentic && <GitChip key={p.sessionId} sessionId={p.sessionId} running={!!p.running} />}
            <span className="sr-only">{sentence}</span>
          </span>
        )}
      </div>
      <div className="conv-actions">
        {p.extra}
        <button
          type="button"
          className="icon"
          aria-label={t("conv.export")}
          title={t("conv.export")}
          disabled={p.exportDisabled}
          onClick={p.onExport}
        >
          <Download size={18} />
        </button>
        <button type="button" className="icon" aria-label={t("conv.delete")} title={t("conv.delete")} onClick={p.onDelete}>
          <Trash2 size={18} />
        </button>
      </div>
    </header>
  );
}
