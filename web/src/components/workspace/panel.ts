import type { Agent, Me, ModelInfo, WorkspaceSummary } from "../../types";

/** What Settings gives every Workspace and Workspace admin panel. */
export interface WorkspacePanelProps {
  me: Me;
  /** The active workspace (from /api/me). */
  workspace: WorkspaceSummary;
  /** Listed in this workspace's scope: its default agent and chains applied. */
  agents: Agent[];
  models: ModelInfo[];
  /** The default agent's id in this scope: the workspace's, else the server's. */
  defaultAgent: string;
  /** Something changed here (name, defaults, members): reloads who the
   * viewer is there, the workspace list and the models. */
  onChanged: () => void;
  /** The viewer left or deleted it: the app goes home, in Personal. */
  onLeft: () => void;
}

/** An error as the panels show it. */
export const message = (x: unknown) => (x instanceof Error ? x.message : String(x));
