// The sidebar's workspace switcher: the active workspace, the others to
// switch to (Personal first, as the server lists them), a new one when the
// server lets this user create it, and the workspace's settings. Part of the
// first load, so it reuses Select and stays small.
import { type FormEvent, useState } from "react";
import { api } from "../api";
import { t } from "../lib/i18n";
import { workspaceName } from "../lib/workspaces";
import type { WorkspaceList, WorkspaceSummary } from "../types";
import { Select, type SelectOption } from "./Select";

// Workspace ids are uuids, so these cannot collide with one.
const NEW = ":new";
const SETTINGS = ":settings";

const message = (x: unknown) => (x instanceof Error ? x.message : String(x));

function Badge({ w }: { w: Pick<WorkspaceSummary, "name" | "personal"> }) {
  return (
    <span className="workspace-badge" aria-hidden="true">
      {workspaceName(w).slice(0, 1).toUpperCase()}
    </span>
  );
}

export function WorkspaceSwitcher(props: {
  /** The active workspace (from /api/me). */
  active: WorkspaceSummary;
  /** null until loaded: the active workspace stands alone until then. */
  list: WorkspaceList | null;
  /** Switches; rejects with what went wrong. */
  onSwitch: (id: string) => Promise<void>;
  onSettings: () => void;
}) {
  const [naming, setNaming] = useState(false);
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const all = props.list?.workspaces ?? [props.active];
  const options: SelectOption[] = [
    ...all.map((w) => ({
      value: w.workspace_id,
      label: workspaceName(w),
      // Personal is always the user's own: a role there says nothing.
      ...(w.personal ? {} : { hint: t(`role.${w.role}`) }),
      icon: <Badge w={w} />,
    })),
    ...(props.list?.can_create ? [{ value: NEW, label: t("workspace.new") }] : []),
    { value: SETTINGS, label: t("workspace.settings") },
  ];

  const pick = (v: string) => {
    setErr(null);
    if (v === NEW) setNaming(true);
    else if (v === SETTINGS) props.onSettings();
    else if (v !== props.active.workspace_id) props.onSwitch(v).catch((x) => setErr(message(x)));
  };
  const cancel = () => {
    setNaming(false);
    setName("");
    setErr(null);
  };
  const create = async (e: FormEvent) => {
    e.preventDefault();
    const n = name.trim();
    if (!n) return;
    setBusy(true);
    setErr(null);
    try {
      const w = await api.createWorkspace(n);
      setNaming(false);
      setName("");
      await props.onSwitch(w.workspace_id);
    } catch (x) {
      setErr(message(x));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="workspace-switch">
      <Select label={t("workspace.label")} value={props.active.workspace_id} options={options} onChange={pick} />
      {naming && (
        <form className="workspace-new" onSubmit={(e) => void create(e)}>
          <input
            autoFocus
            maxLength={64}
            placeholder={t("workspace.newName")}
            aria-label={t("workspace.newName")}
            value={name}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => e.key === "Escape" && cancel()}
          />
          <div className="row">
            <button type="submit" className="primary" disabled={busy || !name.trim()}>
              {t("workspace.create")}
            </button>
            <button type="button" onClick={cancel}>
              {t("workspace.cancel")}
            </button>
          </div>
        </form>
      )}
      {err && <div className="error-text small">{err}</div>}
    </div>
  );
}
