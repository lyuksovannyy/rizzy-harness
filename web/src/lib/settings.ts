// The settings registry: four sections, one per tier (the account, the
// active workspace for every member, its admins, the server's admins), each
// with its tabs and who sees them. Settings.tsx maps tab ids to panels; a
// route names a section and a tab, checked here against what the viewer may
// see, so a URL cannot open a panel the tabs do not offer.
import type { Me } from "../types";
import { can } from "./workspaces";

export type SectionId = "account" | "workspace" | "workspace-admin" | "server";
export type TabId =
  | "profile"
  | "preferences"
  | "memory"
  | "password"
  | "overview"
  | "general"
  | "models"
  | "members"
  | "delete"
  | "providers"
  | "agents"
  | "recall"
  | "savings"
  | "nodes"
  | "users"
  | "workspaces";

type Viewer = Pick<Me, "user_id" | "role" | "workspace">;
type Show = (v: Viewer) => boolean;

const always: Show = () => true;
const settingsPerm: Show = (v) => can(v.workspace, "settings");
const membersPerm: Show = (v) => can(v.workspace, "members");

const SECTIONS: { id: SectionId; show: Show; tabs: { id: TabId; show: Show }[] }[] = [
  {
    id: "account",
    show: always,
    tabs: [
      { id: "profile", show: always },
      { id: "preferences", show: always },
      { id: "memory", show: always },
      // The API token has no password to change.
      { id: "password", show: (v) => !!v.user_id },
    ],
  },
  {
    id: "workspace",
    show: (v) => !!v.workspace,
    tabs: [{ id: "overview", show: always }],
  },
  {
    // Personal has no settings of its own (Preferences covers it) and no other members.
    id: "workspace-admin",
    show: (v) => !!v.workspace && !v.workspace.personal && (settingsPerm(v) || membersPerm(v)),
    tabs: [
      { id: "general", show: settingsPerm },
      { id: "models", show: settingsPerm },
      { id: "members", show: membersPerm },
      // Deleting is the one owner-only action here.
      { id: "delete", show: (v) => v.workspace?.role === "owner" },
    ],
  },
  {
    id: "server",
    show: (v) => v.role === "admin",
    tabs: ["providers", "agents", "recall", "savings", "nodes", "users", "workspaces"].map((id) => ({
      id: id as TabId,
      show: always,
    })),
  },
];

export interface VisibleSection {
  id: SectionId;
  tabs: TabId[];
}

/** The sections and tabs this viewer sees, in order; Account always. */
export function visibleSections(v: Viewer): VisibleSection[] {
  return SECTIONS.filter((s) => s.show(v))
    .map((s) => ({ id: s.id, tabs: s.tabs.filter((x) => x.show(v)).map((x) => x.id) }))
    .filter((s) => s.tabs.length > 0);
}

/** The panel a route asks for, if the viewer sees it; else the section's
 * first tab, else Account's first. */
export function pickTab(
  sections: VisibleSection[],
  want: { section?: string; tab?: string },
): { section: SectionId; tab: TabId } {
  const s = sections.find((x) => x.id === want.section) ?? sections[0]!;
  const tab = s.tabs.find((x) => x === want.tab) ?? s.tabs[0]!;
  return { section: s.id, tab };
}
