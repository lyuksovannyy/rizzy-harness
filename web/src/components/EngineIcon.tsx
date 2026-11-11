// A search engine's favicon, fetched by the server from the engine's site
// (GET /api/favicon); a globe when it has none. An <img> cannot send the
// login key, so it loads through a link (lib/links, Gate 3 R2-1), in the
// lazy chunk the other linked images use: the first load does not grow.
import { Globe } from "lucide-react";
import { lazy, Suspense, useEffect, useState } from "react";

const LinkedImg = lazy(() => import("./Linked"));

/** The server fetches only a couple of new icons per user at a time and
 * answers the rest "try again" (an <img> cannot tell that from "none"),
 * so a list of engines retries a few times before settling on the globe. */
const RETRIES = [800, 2000, 5000];

export function EngineIcon(props: { origin: string | null }) {
  const [tries, setTries] = useState<{ origin: string | null; n: number; waiting: boolean }>({
    origin: props.origin,
    n: 0,
    waiting: false,
  });
  const cur = tries.origin === props.origin ? tries : { origin: props.origin, n: 0, waiting: false };
  useEffect(() => {
    if (!cur.waiting) return;
    const t = window.setTimeout(() => setTries({ ...cur, waiting: false }), RETRIES[cur.n - 1]);
    return () => window.clearTimeout(t);
  }, [cur]);
  if (!props.origin || cur.n > RETRIES.length) return <Globe size={16} className="engine-icon muted" aria-hidden="true" />;
  // Hold the old attempt's slot empty while waiting, so the layout keeps still.
  const blank = <span className="engine-icon" aria-hidden="true" />;
  if (cur.waiting) return blank;
  return (
    <Suspense fallback={blank}>
      <LinkedImg
        className="engine-icon"
        url={`/api/favicon?url=${encodeURIComponent(props.origin)}${cur.n ? `&try=${cur.n}` : ""}`}
        width={16}
        height={16}
        alt=""
        loading="lazy"
        onError={() => setTries({ origin: props.origin, n: cur.n + 1, waiting: cur.n < RETRIES.length })}
      />
    </Suspense>
  );
}
