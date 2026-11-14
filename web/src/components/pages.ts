// What Shell hands every area page (Routines, Media, Artifacts), so each
// page is built in its own files without touching Shell again.
import type { Live } from "../live";
import type { AgentDefaults } from "../lib/agentDefaults";
import type { ProjectSummary } from "../lib/projects";
import type { MediaFilter, Route } from "../lib/routes";
import type { Agent, Features, Me, NodeInfo } from "../types";

export interface FeaturePageProps {
  me: Me;
  /** The Shell's workspace (`?workspace=` on every list); null for the API token. */
  scope: string | null;
  navigate: (r: Route) => void;
  /** `live.onTopic(...)` refreshes the page's list. */
  live: Live;
  /** What the viewer may use here; refetched on every workspace switch. */
  features: Features;
}

export type RoutinesPageProps = FeaturePageProps & {
  /** null: the list; "new": the editor, empty; else that routine. */
  id: string | null;
  tab?: "runs";
  agents: Agent[];
  nodes: NodeInfo[];
  projects: ProjectSummary[];
  /** A new routine's agent per mode until one is picked. */
  agentDefaults: AgentDefaults;
};

export type MediaPageProps = FeaturePageProps & {
  filter: MediaFilter | null;
  /** An item whose preview opens over the grid. */
  id: string | null;
};

export type ArtifactsPageProps = FeaturePageProps & {
  id: string | null;
  projects: ProjectSummary[];
};
