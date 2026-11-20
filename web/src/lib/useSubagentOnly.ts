// Whether a nested chat's agent runs only as a sub-agent, as the chat's OWN
// workspace lists it (X.41). The page's agent list follows the workspace
// that is current, which can be another one than the chat's (a link to a
// chat of another workspace), so it is read afresh for the chat. Until it
// is known, or when it cannot be read or does not name the agent, the answer
// is false: a reply box shown where the server then refuses is acceptable;
// a person locked out of a chat they may write to is not.
import { useEffect, useState } from "react";
import { api } from "../api";
import { takesNoChat } from "./agents";

export function useSubagentOnly(nested: boolean, workspaceId: string | null, agentId: string): boolean {
  const [found, setFound] = useState<{ key: string; only: boolean } | null>(null);
  const key = `${workspaceId ?? ""}/${agentId}`;
  useEffect(() => {
    if (!nested) return;
    let live = true;
    api
      .agents(workspaceId ? { workspace: workspaceId } : null)
      .then((r) => live && setFound({ key, only: takesNoChat(r.agents.find((a) => a.id === agentId)) }))
      .catch(() => live && setFound(null));
    return () => {
      live = false;
    };
  }, [nested, workspaceId, agentId, key]);
  return nested && found?.key === key && found.only;
}
