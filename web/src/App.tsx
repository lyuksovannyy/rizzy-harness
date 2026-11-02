import { lazy, Suspense, useCallback, useEffect, useState } from "react";
import { api } from "./api";
import type { Me } from "./types";
import { Login } from "./components/Login";
import { Shell } from "./components/Shell";

const ChangePassword = lazy(() =>
  import("./components/ChangePassword").then((m) => ({ default: m.ChangePassword })),
);

export function App() {
  // undefined = loading, null = logged out
  const [me, setMe] = useState<Me | null | undefined>(undefined);
  const refresh = useCallback(() => {
    api
      .me()
      .then((r) => setMe(r.user))
      .catch(() => setMe(null));
  }, []);
  useEffect(refresh, [refresh]);
  if (me === undefined) return <div className="splash" />;
  if (me === null) return <Login onDone={refresh} />;
  if (me.must_change_password)
    return (
      <Suspense fallback={<div className="splash" />}>
        <ChangePassword forced onDone={refresh} />
      </Suspense>
    );
  return <Shell me={me} onLogout={() => setMe(null)} />;
}
