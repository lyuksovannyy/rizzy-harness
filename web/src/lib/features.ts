// What the server offers the viewer in a workspace (`/api/features`), for
// the screens Shell does not hand its copy to: the home bar and a chat's
// reply box (images in chats) and a chat's "Save as artifact". Asked on
// mount, so a setting changed meanwhile shows on the next visit.
import { useEffect, useState } from "react";
import { api } from "../api";
import type { Features } from "../types";

/** null until it answers, when `ask` is false, or when the call failed. */
export function useFeatures(workspace: string | null | undefined, ask = true): Features | null {
  const [got, setGot] = useState<Features | null>(null);
  useEffect(() => {
    if (!ask) return;
    let live = true;
    api
      .features(workspace ? { workspace } : null)
      .then((f) => live && setGot(f))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [ask, workspace]);
  return ask ? got : null;
}
