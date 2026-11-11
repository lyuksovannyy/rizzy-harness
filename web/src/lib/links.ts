// Links for what the browser fetches by itself (Gate 3 P-1): an <img>, a
// <video>, an <audio> or an <a download> cannot send the login key, and the
// cookie alone is refused. So each such URL is swapped for a link the
// server mints for this login and this exact URL (POST /api/links), good
// for 30 minutes and only together with the cookie. Requests made in one
// tick go in one call; a link is reused until shortly before it expires.
// Only lazy chunks import this, so the first load does not carry it.
import { useEffect, useState } from "react";
import { call, loginKey } from "../api";

/** The server's links last 30 min; a cached one is reused for 25. */
const FRESH_MS = 25 * 60_000;
/** The server mints at most 200 per call. */
const PER_CALL = 200;

type Waiter = { url: string; done: (link: string) => void; fail: (e: unknown) => void };

const cache = new Map<string, { link: Promise<string>; at: number }>();
let waiting: Waiter[] | null = null;

function flush() {
  const all = waiting ?? [];
  waiting = null;
  for (let i = 0; i < all.length; i += PER_CALL) {
    const part = all.slice(i, i + PER_CALL);
    const urls = [...new Set(part.map((w) => w.url))];
    call<{ links: Record<string, string> }>("POST", "/api/links", { urls }).then(
      (r) =>
        part.forEach((w) => {
          const l = r.links[w.url];
          if (l) w.done(l);
          else w.fail(new Error("no link"));
        }),
      (e) => part.forEach((w) => w.fail(e)),
    );
  }
}

/** `url` (a path on this server) as a link the browser may load by itself. */
export function link(url: string): Promise<string> {
  // A link is for one login: a new login (or none) never reuses another's.
  const key = `${loginKey()}|${url}`;
  const hit = cache.get(key);
  if (hit && Date.now() - hit.at < FRESH_MS) return hit.link;
  const p = new Promise<string>((done, fail) => {
    if (!waiting) {
      waiting = [];
      queueMicrotask(flush);
    }
    waiting.push({ url, done, fail });
  });
  cache.set(key, { link: p, at: Date.now() });
  p.catch(() => cache.delete(key));
  return p;
}

/** `url`'s link once it is minted (undefined until then, or for no url).
 * `renew`: mint a fresh one before it expires, for a download button that
 * stays on screen; an <img> or <video> keeps the one it loaded with. */
export function useLink(url: string | null | undefined, renew = false): string | undefined {
  const [got, setGot] = useState<{ url: string; link: string } | null>(null);
  const [round, setRound] = useState(0);
  useEffect(() => {
    if (!url) return;
    let live = true;
    link(url).then(
      (l) => live && setGot({ url, link: l }),
      () => {},
    );
    const t = renew ? setTimeout(() => setRound((n) => n + 1), FRESH_MS) : undefined;
    return () => {
      live = false;
      clearTimeout(t);
    };
  }, [url, renew, round]);
  return got && got.url === url ? got.link : undefined;
}
