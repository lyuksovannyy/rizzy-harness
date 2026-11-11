import { lazy, Suspense, useCallback, useEffect, useState } from "react";
import { api, forgetLogin, HttpError, login } from "./api";
import { clearDrafts, setDraftOwner } from "./lib/drafts";
import type { Me, WorkspaceSummary } from "./types";
import { Shell } from "./components/Shell";

// Signed-in visits, nearly all of them, never draw these: each is its own
// chunk, so the first load carries only the shell.
const Login = lazy(() => import("./components/Login").then((m) => ({ default: m.Login })));
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
    // Any 401, from any request: the login is gone (or this tab has no key).
    login.ended = logout;
    void load(true);
  }, [load, logout]);
  // Signed out, however it happened (logout, a 401, a fresh load without a
  // login): no one's unsent drafts stay in this browser (AUDIT L14), nor the
  // login key (P-1). Signed in: the drafts read and written are this
  // user's. Set while rendering, since the reply box reads its draft in its
  // first render.
  useEffect(() => {
    if (me === null) {
      clearDrafts();
      forgetLogin();
    }
  }, [me]);
  if (me) setDraftOwner(me.user_id);
  if (me === undefined) return <div className="splash" />;
  if (me === null)
    return (
      <Suspense fallback={<div className="splash" />}>
        <Login onDone={signedIn} />
      </Suspense>
    );
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
