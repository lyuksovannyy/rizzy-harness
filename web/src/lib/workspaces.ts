// Workspaces in the browser: how one is named, what a role may do, when
// opening a chat should switch to the workspace it is in, and where a tab
// goes when the workspaces change.
import type { Perm, WorkspaceSummary } from "../types";
import { t } from "./i18n";

/** A Personal workspace's stored name is not shown: the catalog names it. */
export function workspaceName(w: Pick<WorkspaceSummary, "name" | "personal">): string {
  return w.personal ? t("workspace.personal") : w.name;
}

/** Whether the caller's role there holds `p` (no workspace: no). */
export function can(w: Pick<WorkspaceSummary, "permissions"> | null | undefined, p: Perm): boolean {
  return !!w && w.permissions.includes(p);
}

export type FollowStep = { do: "nothing" } | { do: "lookup"; id: string } | { do: "switch"; to: string };

const NOTHING: FollowStep = { do: "nothing" };

/** What to do about the chat a route opens (`#/s/<id>`) so that it shows in
 * its own workspace: a notification, Attention, a sub-task's parent link or
 * back/forward can open a chat from another workspace. Nothing while this
 * workspace's chats are loading or include it; otherwise look it up, and
 * once looked up, switch when it is in another workspace the user is in. */
export function followRoute(p: {
  /** The route's chat, when the route is one. */
  id: string | null;
  /** The active workspace; null for the API token, which has none to switch. */
  active: string | null;
  /** Whether this workspace's chats include it; null until they are loaded. */
  listed: boolean | null;
  /** The lookup's answer: the chat's workspace (null: none or unknown); undefined before asking. */
  found?: string | null;
  /** The user's workspaces; null until loaded. */
  memberOf: string[] | null;
}): FollowStep {
  if (!p.id || !p.active || p.listed !== false) return NOTHING;
  if (p.found === undefined) return { do: "lookup", id: p.id };
  if (!p.found || p.found === p.active || !p.memberOf?.includes(p.found)) return NOTHING;
  return { do: "switch", to: p.found };
}

export type AfterChange = { keep: WorkspaceSummary } | { fallBack: true; gone: string | null };

/** Where an open tab goes after the workspaces changed (or its socket came
 * back). It stays in its own workspace while the user is still a member,
 * with the list's fresh summary (role, permissions, name, member count),
 * and never follows the server's active one: another tab or device picked
 * that, and following it would move this tab under the user, then bounce
 * two tabs between two workspaces as each route-following switch moves the
 * other. Only once the user is out does it fall back to the server's
 * choice, saying which workspace they lost, unless they left or deleted it
 * themselves (`left`: its id). */
export function afterChange(here: WorkspaceSummary, list: WorkspaceSummary[], left: string | null): AfterChange {
  const fresh = list.find((w) => w.workspace_id === here.workspace_id);
  if (fresh) return { keep: fresh };
  return { fallBack: true, gone: left === here.workspace_id ? null : workspaceName(here) };
}

/** A switch from the switcher stays on the open chat only when that chat is
 * in the target workspace; otherwise it goes home. */
export function keepsRoute(open: { workspace_id?: string | null } | null | undefined, target: string): boolean {
  return !!open?.workspace_id && open.workspace_id === target;
}
