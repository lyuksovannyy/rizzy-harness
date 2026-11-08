import { lazy, Suspense, useCallback, useEffect, useState } from "react";
import { api, HttpError } from "./api";
import type { Me, WorkspaceSummary } from "./types";
import { Login } from "./components/Login";
import { Shell } from "./components/Shell";

const ChangePassword = lazy(() =>
  import("./components/ChangePassword").then((m) => ({ default: m.ChangePassword })),
);

export function App() {
  // undefined = loading, null = logged out
  const [me, setMe] = useState<Me | null | undefined>(undefined);
  // The workspace the user was taken out of (removed, or it was deleted):
  // kept here, since Shell remounts in the workspace that replaces it.
  const [gone, setGone] = useState<string | null>(null);
  // `keep`: the workspace this tab shows, refreshed; without it, the server's
  // active one (a fresh load, a switch, or the tab's workspace is gone).
  const load = useCallback(
    (first: boolean, keep?: WorkspaceSummary) =>
      api
        .me()
        .then((r) => setMe({ ...r.user, workspace: keep ?? r.workspace }))
        .catch((e) => {
          // At first anything that fails shows the login; later (a switch, a
          // workspace change) only a lost session does.
          if (first || (e instanceof HttpError && e.status === 401)) setMe(null);
        }),
    [],
  );
  const reload = useCallback((keep?: WorkspaceSummary) => load(false, keep), [load]);
  // Stable: Shell's data effects depend on it, and App re-renders on every
  // reload of `me` (switches, workspace changes). Whoever signs in next must
  // not read the last account's "no longer in" line.
  const logout = useCallback(() => {
    setGone(null);
    setMe(null);
  }, []);
  const signedIn = useCallback(() => {
    setGone(null);
    void load(true);
  }, [load]);
  useEffect(() => {
    void load(true);
  }, [load]);
  if (me === undefined) return <div className="splash" />;
  if (me === null) return <Login onDone={signedIn} />;
  if (me.must_change_password)
    return (
      <Suspense fallback={<div className="splash" />}>
        <ChangePassword forced onDone={() => void load(true)} />
      </Suspense>
    );
  // One Shell per workspace: switching remounts it, so every list, default
  // and the live connection start over in the new one.
  return (
    <Shell
      key={me.workspace?.workspace_id ?? ""}
      me={me}
      onLogout={logout}
      onMeChanged={reload}
      gone={gone}
      onGone={setGone}
    />
  );
}
