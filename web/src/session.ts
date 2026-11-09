// Keeps the login alive. The server's access token (an HttpOnly cookie)
// lasts minutes; the refresh token (another HttpOnly cookie, sent only to
// /api/auth/refresh) gets a new one. Nothing here can read either token.
//
// * Every same-origin /api request that answers 401 is refreshed once and
//   retried, so raw fetch() calls (uploads, dictation) recover too.
// * Refreshes are single-flight: the server treats a refresh token used
//   twice outside a short grace period as stolen and ends the login.
// * While a tab is open the access token is renewed before it runs out
//   (WebSocket reconnects and <img> loads need a valid one).

const REFRESH = "/api/auth/refresh";
/** Paths whose 401 means "wrong credentials", not "token expired". */
const NO_RETRY = [REFRESH, "/api/login", "/api/logout", "/api/auth/logout", "/api/setup", "/api/me/password"];
const RENEW_EVERY_MS = 5 * 60 * 1000;

let inflight: Promise<boolean> | null = null;

/** Ask for new tokens; true when the login is still good. */
export function refreshSession(f: typeof fetch = originalFetch): Promise<boolean> {
  inflight ??= f(REFRESH, { method: "POST", credentials: "same-origin" })
    .then((r) => r.ok)
    .catch(() => false)
    .finally(() => {
      inflight = null;
    });
  return inflight;
}

let originalFetch: typeof fetch = (...a) => fetch(...a);

function apiPath(input: RequestInfo | URL): string | null {
  try {
    const u = new URL(input instanceof Request ? input.url : String(input), location.href);
    return u.origin === location.origin && u.pathname.startsWith("/api/") ? u.pathname : null;
  } catch {
    return null;
  }
}

/** Install once at start-up. */
export function installSessionKeeper(): void {
  if ((window as { __sessionKeeper?: boolean }).__sessionKeeper) return;
  (window as { __sessionKeeper?: boolean }).__sessionKeeper = true;
  const base = window.fetch.bind(window);
  originalFetch = base;
  window.fetch = async (input, init) => {
    const path = apiPath(input);
    // A Request's body can be read once: keep a copy for the retry.
    const retryInput = input instanceof Request ? input.clone() : input;
    const res = await base(input, init);
    if (res.status !== 401 || !path || NO_RETRY.includes(path)) return res;
    if (!(await refreshSession(base))) return res;
    return base(retryInput, init);
  };
  let last = Date.now();
  const renew = () => {
    last = Date.now();
    void refreshSession(base);
  };
  setInterval(() => {
    if (document.visibilityState === "visible") renew();
  }, RENEW_EVERY_MS);
  // Back from sleep or another tab: the token may have run out meanwhile.
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible" && Date.now() - last > RENEW_EVERY_MS) renew();
  });
}
