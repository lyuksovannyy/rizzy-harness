// Problems that last until someone fixes them, under the home bar: a model
// in use that its provider no longer lists. Not dismissible; each goes away
// when the server stops reporting it. The server picks who sees which: a
// server admin every one, with a way to fix the server's uses; a workspace
// admin those from the workspace's settings, with theirs; anyone else those
// that affect them.
import { AlertTriangle } from "lucide-react";
import { rawModel } from "../lib/models";
import { settingsHash } from "../lib/routes";
import type { Agent, Notice } from "../types";

/** Settings tabs a notice links to. */
export const FIX_PROVIDERS = settingsHash("server", "providers");
export const FIX_DEFAULT = settingsHash("account", "preferences");
export const FIX_WORKSPACE = settingsHash("workspace-admin", "models");

/** "a", "a and b", "a, b and c". */
function list(xs: string[]): string {
  return xs.length < 2 ? (xs[0] ?? "") : `${xs.slice(0, -1).join(", ")} and ${xs.at(-1)}`;
}

type Link = { href: string; label: string };

/** What a notice says, and where it sends the viewer to fix it, if anywhere. */
export function noticeText(n: Notice, agents: Pick<Agent, "id" | "name">[]): { text: string; hint: string; links: Link[] } {
  const names = n.agents.map((id) => agents.find((a) => a.id === id)?.name ?? id);
  const roles = [
    n.workspace_default && "this workspace's default model",
    n.global_default && "the server default model",
    n.your_default && "your default model",
  ].filter((r): r is string => !!r);
  const text =
    `Model ${rawModel(n.model)} (${n.provider}) is no longer available` +
    (names.length ? ` — used by ${names.join(", ")}.` : ".") +
    (roles.length ? ` It is ${list(roles)}.` : "");
  // A use the workspace set is fixed there; one from the server, in its
  // providers. The server default is the server's even when a workspace
  // chain names the model too; any other use counts as the workspace's
  // once its settings name the model (the notice does not say per agent).
  const here = n.workspace && n.can_fix_workspace;
  const server = n.can_fix && (n.global_default || !n.workspace);
  const links: Link[] = [
    ...(here ? [{ href: FIX_WORKSPACE, label: "Fix in Workspace admin" }] : []),
    ...(server ? [{ href: FIX_PROVIDERS, label: "Fix in Server settings" }] : []),
  ];
  if (here && server)
    return { text, hint: "Pick another model in its place, in this workspace's models and in the server's providers.", links };
  if (here) {
    const also = n.global_default ? " As the server default, it also needs a server admin." : "";
    return { text, hint: `Pick another model in its place in this workspace's models.${also}`, links };
  }
  if (server) return { text, hint: "Pick another model in its place, or remove it if it is gone for good.", links };
  if (n.your_default) {
    return {
      text,
      hint: "Pick another default so new chats do not start on it.",
      links: [{ href: FIX_DEFAULT, label: "Change your default" }],
    };
  }
  const who = !n.workspace
    ? "A server admin needs"
    : n.global_default
      ? "A workspace admin and a server admin need"
      : "A workspace admin needs";
  return { text, hint: `${who} to replace it; until then chats on it may answer with another model, or fail.`, links };
}

export function Notices(props: { notices: Notice[]; agents: Agent[] }) {
  if (props.notices.length === 0) return null;
  return (
    <section className="notices" aria-label="problems">
      <ul>
        {props.notices.map((n) => {
          const { text, hint, links } = noticeText(n, props.agents);
          return (
            <li key={n.id} className="notice">
              <AlertTriangle size={16} aria-hidden="true" />
              <div className="notice-body">
                <span className="notice-text">{text}</span>
                <span className="notice-hint">
                  {hint}
                  {links.map((l) => (
                    <span key={l.href}>
                      {" "}
                      <a className="link" href={l.href}>
                        {l.label}
                      </a>
                    </span>
                  ))}
                </span>
              </div>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
