// The chat's git branch and count of changed files, in the header (X.64): a
// small chip after "project · node · how commands run" for a coding chat whose
// folder is in a git repository; nothing for any other. It is read when the
// chat is opened and again each time a turn ends, and a click reads it again;
// it is never polled. A failed or refused read keeps what is shown; a folder
// that is no repository (or a node that is away or too old) shows nothing.
import { GitBranch } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { call } from "../api";
import { chipName, chipTitle, countText, type GitView, gitUrl, readGit } from "../lib/gitChip";
import "./gitChip.css";

export function GitChip(props: { sessionId: string; running: boolean }) {
  const { sessionId, running } = props;
  const [git, setGit] = useState<GitView | null>(null);
  const [busy, setBusy] = useState(false);
  const latest = useRef<AbortController | null>(null);
  const refresh = useCallback(() => {
    latest.current?.abort();
    const ctl = new AbortController();
    latest.current = ctl;
    setBusy(true);
    call<unknown>("GET", gitUrl(sessionId), undefined, ctl.signal)
      .then((v) => setGit(readGit(v)))
      .catch(() => {})
      .finally(() => {
        if (!ctl.signal.aborted) setBusy(false);
      });
  }, [sessionId]);
  useEffect(() => {
    refresh();
    return () => latest.current?.abort();
  }, [refresh]);
  // The end of a turn: its changes are in the folder now.
  const was = useRef(running);
  useEffect(() => {
    if (was.current && !running) refresh();
    was.current = running;
  }, [running, refresh]);
  if (!git) return null;
  const count = countText(git);
  const title = chipTitle(git);
  return (
    <>
      <span className="chat-sep" aria-hidden="true">
        ·
      </span>
      <button type="button" className="git-chip" title={title} aria-label={title} aria-busy={busy} onClick={refresh}>
        <GitBranch size={13} aria-hidden="true" />
        <span className="git-name">{chipName(git)}</span>
        {count && <span className="git-count">{count}</span>}
      </button>
    </>
  );
}
