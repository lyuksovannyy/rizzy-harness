import { useState } from "react";
import { type PendingProject, projectsApi } from "../lib/projects";
import { shownName } from "../md/hiddenChars";

/** Projects whose folder is being cloned (X.27): shown, never a link (no
 * chat can start in one); a failed one, its starter's only, with the reason
 * and Dismiss. */
export default function PendingProjects({ list }: { list: PendingProject[] }) {
  // What the server said when a Dismiss was refused (by project).
  const [refused, setRefused] = useState<Record<string, string>>({});
  const dismiss = (id: string) =>
    projectsApi.dismissClone(id).then(
      () => setRefused(({ [id]: _, ...rest }) => rest),
      (e: unknown) => setRefused((r) => ({ ...r, [id]: e instanceof Error ? e.message : String(e) })),
    );
  return list.map((p) => {
    const what = shownName(`${p.source.host}/${p.source.full_name}`);
    const why = p.reason ? shownName(p.reason) : "";
    return (
      <li key={p.project_id} className="pending-project muted small" title={why || what}>
        <span className="ellipsis grow">
          {shownName(p.name)} · {why ? `clone failed: ${why}` : `cloning ${what}…`}
        </span>
        {why && p.mine && (
          <button type="button" className="link" onClick={() => void dismiss(p.project_id)}>
            Dismiss
          </button>
        )}
        {refused[p.project_id] && (
          <span className="error-text" role="alert">
            {shownName(refused[p.project_id]!)}
          </span>
        )}
      </li>
    );
  });
}
