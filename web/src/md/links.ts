// Link rules for model-written URLs. The confirm they drive is UX only; the
// server's web_fetch provenance check is separate and unchanged.

/** Answers "did this URL key appear in text the user or a tool produced?". */
export interface Provenance {
  has(key: string): boolean;
}

export function parseUrl(href: string): URL | null {
  try {
    return new URL(href);
  } catch {
    return null;
  }
}

/** The host the browser will contact (punycode ASCII), without a leading
 * www.; for mailto it is the address. */
export function hostOf(u: URL): string {
  if (u.protocol === "mailto:") return decodeSafe(u.pathname);
  return u.hostname.replace(/^www\./, "");
}

function decodeSafe(s: string): string {
  try {
    return decodeURIComponent(s);
  } catch {
    return s;
  }
}

/** The same key the server's url_key builds (crates/web): no fragment, no
 * http(s) scheme, no trailing slash, no leading www. */
export function urlKey(u: URL): string {
  let s = u.href;
  const hash = s.indexOf("#");
  if (hash >= 0) s = s.slice(0, hash);
  s = s.replace(/^https?:\/\//, "").replace(/\/+$/, "");
  return s.startsWith("www.") ? s.slice(4) : s;
}

// Same tokenizer as the server's provenance_ok.
const SPLIT = /[\s<>"'`()[\]{}|]+/;

/** URL keys of every URL-ish token in a text (anything with a dot). */
export function urlKeysIn(text: string): string[] {
  const keys: string[] = [];
  for (const raw of text.split(SPLIT)) {
    const tok = raw.replace(/[.,;:!?]+$/, "");
    if (!tok.includes(".")) continue;
    const u = parseUrl(tok.startsWith("http://") || tok.startsWith("https://") ? tok : `https://${tok}`);
    const key = u ? urlKey(u) : "";
    if (key) keys.push(key);
  }
  return keys;
}

/**
 * Should a click show the full URL first? Only for URLs that came from
 * nowhere but the model and that could carry data out (a query, a fragment,
 * a long path) or mislead (a punycode host, credentials before the host).
 */
export function needsConfirm(u: URL, provenance: Provenance): boolean {
  if (provenance.has(urlKey(u))) return false;
  return (
    u.search !== "" ||
    u.hash !== "" ||
    u.pathname.length > 64 ||
    u.hostname.includes("xn--") ||
    u.username !== "" ||
    u.password !== ""
  );
}

const bare = (s: string) =>
  s
    .trim()
    .toLowerCase()
    .replace(/^[a-z][a-z0-9+.-]*:(\/\/)?/, "")
    .replace(/^www\./, "")
    .replace(/\/+$/, "");

/** Does the link text already show where it goes (the URL or the host)?
 * Otherwise the real host is shown next to it. */
export function textShowsTarget(text: string, href: string, host: string): boolean {
  const t = bare(text);
  return t !== "" && (t === bare(href) || t === host.toLowerCase());
}
