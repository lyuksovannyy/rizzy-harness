// Settings → Server → Storage (docs/media.md): the limits on stored files
// (one file, each account, the whole server, the free disk kept) and how
// much each account stores in Media and Artifacts. Numbers only: no names
// or contents of anyone's files. Server admins and the API token.
import { useEffect, useState } from "react";
import { bytes } from "../lib/bytes";
import { type AdminStorage, type StorageLimits, storageApi } from "../lib/media";
import type { Me } from "../types";

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

const FIELDS: { key: keyof StorageLimits; label: string; hint: string }[] = [
  { key: "max_file_mb", label: "Largest file (MB)", hint: "One upload, or one file or folder an agent publishes." },
  { key: "account_quota_mb", label: "Per account (MB)", hint: "Media and Artifacts together; 0 means no limit." },
  { key: "server_quota_mb", label: "Whole server (MB)", hint: "Every account's files together." },
  { key: "min_free_mb", label: "Keep free on the disk (MB)", hint: "Uploads stop before the data disk gets this full." },
];

export function StorageSettings(_props: { me: Me }) {
  const [data, setData] = useState<AdminStorage | null>(null);
  const [form, setForm] = useState<StorageLimits | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  useEffect(() => {
    storageApi
      .admin()
      .then((d) => {
        setData(d);
        setForm(d.settings);
      })
      .catch((e) => setError(message(e)));
  }, []);

  if (!data || !form) return error ? <div className="error-text">{error}</div> : <div className="muted">Loading…</div>;
  const save = async () => {
    setError(null);
    setNote(null);
    try {
      const saved = await storageApi.save(form);
      setForm(saved);
      setData({ ...data, settings: saved });
      setNote("Saved.");
    } catch (e) {
      setError(message(e));
    }
  };
  const changed = FIELDS.some((f) => form[f.key] !== data.settings[f.key]);
  const usedShare = data.disk.total ? 1 - data.disk.free / data.disk.total : 0;
  return (
    <div className="stack">
      <form
        className="card stack form"
        onSubmit={(e) => {
          e.preventDefault();
          void save();
        }}
      >
        <h3>Limits</h3>
        <p className="muted small">A file over a limit is refused; nothing stored is ever removed to make room.</p>
        <div className="storage-fields">
          {FIELDS.map((f) => (
            <label key={f.key}>
              {f.label}
              <input
                type="number"
                min={0}
                step={1}
                value={form[f.key]}
                onChange={(e) => setForm({ ...form, [f.key]: Math.max(0, Math.floor(Number(e.target.value) || 0)) })}
              />
              <span className="field-hint">{f.hint}</span>
            </label>
          ))}
        </div>
        {error && (
          <div className="error-text" role="alert">
            {error}
          </div>
        )}
        <div className="row end">
          {note && (
            <span className="muted" role="status">
              {note}
            </span>
          )}
          <button className="primary" disabled={!changed}>
            Save
          </button>
        </div>
      </form>
      <div className="card stack">
        <h3>Usage</h3>
        <p className="small">
          {bytes(data.total)} stored in all.{" "}
          {data.disk.total > 0 && (
            <span className="muted">
              The data disk has {bytes(data.disk.free)} free of {bytes(data.disk.total)} ({Math.round(usedShare * 100)}% used).
            </span>
          )}
        </p>
        {data.usage.length === 0 ? (
          <p className="muted small">Nobody stores files yet.</p>
        ) : (
          <table className="users storage-usage">
            <thead>
              <tr>
                <th>Account</th>
                <th>Stored</th>
                <th>Media items</th>
                <th>Artifact versions</th>
              </tr>
            </thead>
            <tbody>
              {data.usage.map((u) => (
                <tr key={u.user_id}>
                  <td>{u.username}</td>
                  <td>{bytes(u.used)}</td>
                  <td>{u.media_items}</td>
                  <td>{u.artifact_versions}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}
