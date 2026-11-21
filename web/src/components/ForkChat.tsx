// Forking a chat (ADOPTION_PLAN C.2, README "Fork a chat"): the link on a
// finished turn's end line, the divider where a fork's own conversation
// begins, the note that a fork shares its parent's folder, and what an
// inherited turn's Undo block says. Lazily loaded with the chat.
import { useEffect, useState } from "react";
import { api, HttpError } from "../api";
import { pageApi } from "../apiPages";
import { errText } from "../lib/errorText";
import { routeHash } from "../lib/routes";
import { ForkChoice } from "./ForkChoice";

/** "Fork from here" on a turn's end line. Where the node can give the fork a
 * folder of its own (`own`) the link opens a small choice of where its files
 * live; otherwise it forks at once into the chat's folder. */
export function ForkChat(props: { sessionId: string; turnId: string; own?: boolean }) {
  return props.own ? <ForkChoice sessionId={props.sessionId} turnId={props.turnId} /> : <ForkNow sessionId={props.sessionId} turnId={props.turnId} />;
}

/** A new chat that begins with this conversation up to here and goes its
 * own way; the parent is not changed. The new chat opens at once. The route
 * refuses what this misses. */
function ForkNow(props: { sessionId: string; turnId: string }) {
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const run = async () => {
    setBusy(true);
    setErr(null);
    try {
      const { session_id } = await pageApi.forkChat(props.sessionId, props.turnId);
      location.hash = routeHash({ kind: "session", id: session_id });
    } catch (x) {
      setErr(errText(x));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className={`fork-line small${err || busy ? " shown" : ""}`}>
      <button
        type="button"
        className="link"
        disabled={busy}
        title="Start a new chat with this conversation up to here. This chat stays as it is."
        onClick={() => void run()}
      >
        Fork from here
      </button>
      {err && <div className="error-text">{err}</div>}
    </div>
  );
}

/** What the chat the fork came from is, as far as this page can tell:
 * its title, or that it is gone (deleted, or never the viewer's). */
function useParent(id: string): { title: string } | "gone" | null {
  const [p, setP] = useState<{ title: string } | "gone" | null>(null);
  useEffect(() => {
    let on = true;
    setP(null);
    api.session(id).then(
      (r) => on && setP({ title: r.title }),
      (e) => on && setP(e instanceof HttpError && e.status === 404 ? "gone" : null),
    );
    return () => {
      on = false;
    };
  }, [id]);
  return p;
}

/** The divider where the copied conversation ends and the fork's own begins. */
export function ForkDivider(props: { parent: string }) {
  const p = useParent(props.parent);
  return (
    <div className="divider fork-divider" data-testid="fork-divider">
      {p === "gone" ? (
        "Forked from a chat that no longer exists"
      ) : p ? (
        <>
          Forked from{" "}
          <a className="link" href={routeHash({ kind: "session", id: props.parent })}>
            {p.title}
          </a>{" "}
          here
        </>
      ) : (
        "Forked from another chat here"
      )}
    </div>
  );
}

/** The first line of a fork's conversation, in the normal flow (a banner
 * over the timeline hid the first message), for a fork that works in a
 * folder: the parent's, so each chat's changes are in the other's files; or
 * its own copy of the repository (`own`), which has what the parent had at
 * the fork and nothing git ignores. */
export function ForkNote(props: { parent: string; own?: boolean; cwd?: string }) {
  const p = useParent(props.parent);
  if (props.own) {
    return (
      <div className="fork-note small muted" role="note" data-testid="fork-note">
        <span>
          This chat works in its own copy of the repository{props.cwd ? <>, <span className="mono">{props.cwd}</span></> : null}, on a branch of its own. Files git ignores (build output, <span className="mono">.env</span>) were not copied. Deleting the chat keeps the folder.
        </span>
      </div>
    );
  }
  return (
    <div className="fork-note small muted" role="note" data-testid="fork-note">
      <span>
        This chat shares its folder with{" "}
        {p && p !== "gone" ? (
          <a className="link" href={routeHash({ kind: "session", id: props.parent })}>
            {p.title}
          </a>
        ) : (
          "the chat it was forked from"
        )}
        : what either does there is in the other's files.
      </span>
    </div>
  );
}

/** An inherited turn's changes, where the Undo block would be: they were
 * made in the parent's chat, so this chat cannot put them back. */
export function InheritedChanges(props: { files: string[] }) {
  const n = props.files.length;
  return (
    <div className="undo inherited-changes small muted" data-testid="inherited-changes">
      changed {n} {n === 1 ? "file" : "files"} (before the fork)
    </div>
  );
}
