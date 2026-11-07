// Problems that last until someone fixes them, under the home bar: a model
// in use that its provider no longer lists. Not dismissible; each goes away
// when the server stops reporting it. The server picks who sees which: an
// admin every one, with a way to fix it; anyone else those that affect them.
import { AlertTriangle } from "lucide-react";
import { rawModel } from "../lib/models";
import type { Agent, Notice } from "../types";

/** Settings sections a notice links to (Shell.tsx parses them). */
export const FIX_PROVIDERS = "#/settings/provider";
export const FIX_DEFAULT = "#/settings/preferences";

/** "a", "a and b", "a, b and c". */
function list(xs: string[]): string {
  return xs.length < 2 ? (xs[0] ?? "") : `${xs.slice(0, -1).join(", ")} and ${xs.at(-1)}`;
}

/** What a notice says, and where it sends the viewer to fix it, if anywhere. */
export function noticeText(
  n: Notice,
  agents: Pick<Agent, "id" | "name">[],
): { text: string; hint: string; link: { href: string; label: string } | null } {
  const names = n.agents.map((id) => agents.find((a) => a.id === id)?.name ?? id);
  const roles = [n.global_default && "the default model for everyone", n.your_default && "your default model"].filter(
    (r): r is string => !!r,
  );
  const text =
    `Model ${rawModel(n.model)} (${n.provider}) is no longer available` +
    (names.length ? ` — used by ${names.join(", ")}.` : ".") +
    (roles.length ? ` It is ${list(roles)}.` : "");
  if (n.can_fix) {
    return {
      text,
      hint: "Pick another model in its place, or remove it if it is gone for good.",
      link: { href: FIX_PROVIDERS, label: "Fix in Settings" },
    };
  }
  if (n.your_default) {
    return {
      text,
      hint: "Pick another default so new chats do not start on it.",
      link: { href: FIX_DEFAULT, label: "Change your default" },
    };
  }
  return {
    text,
    hint: "An admin needs to replace it; until then chats on it may answer with another model, or fail.",
    link: null,
  };
}

export function Notices(props: { notices: Notice[]; agents: Agent[] }) {
  if (props.notices.length === 0) return null;
  return (
    <section className="notices" aria-label="problems">
      <ul>
        {props.notices.map((n) => {
          const { text, hint, link } = noticeText(n, props.agents);
          return (
            <li key={n.id} className="notice">
              <AlertTriangle size={16} aria-hidden="true" />
              <div className="notice-body">
                <span className="notice-text">{text}</span>
                <span className="notice-hint">
                  {hint}
                  {link && (
                    <>
                      {" "}
                      <a className="link" href={link.href}>
                        {link.label}
                      </a>
                    </>
                  )}
                </span>
              </div>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
