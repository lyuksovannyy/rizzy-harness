// Link rules for model-written URLs. The confirm they drive is UX only; the
// server's web_fetch provenance check is separate and unchanged.
import { hasHidden } from "./hidden";

/** Answers "did this URL key appear in the user's text or a web result?". */
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
 * www.; for mailto it is the address, left percent-encoded when decoding it
 * would give hidden or control characters (they could make it read as
 * another address). */
export function hostOf(u: URL): string {
  if (u.protocol === "mailto:") {
    const addr = decodeSafe(u.pathname);
    return oddChars(addr) ? u.pathname : addr;
  }
  return u.hostname.replace(/^www\./, "");
}

const CONTROL = /[\x00-\x1f\x7f-\x9f]/;
const oddChars = (s: string) => hasHidden(s) || CONTROL.test(s);

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

/** What a link is checked against: urlKey plus the fragment. The fragment
 * never reaches the server, but scripts on the page can read it, so a link
 * only counts as seen (or as a source) with exactly the fragment it was
 * seen with, or with none. */
export function linkKey(u: URL): string {
  return urlKey(u) + u.hash;
}

/** The keys a seen URL vouches for: itself without its fragment, and with
 * it when it has one. */
export function seenKeys(u: URL): string[] {
  const key = urlKey(u);
  if (!key) return [];
  return u.hash ? [key, key + u.hash] : [key];
}

// Same tokenizer as the server's provenance_ok.
const SPLIT = /[\s<>"'`()[\]{}|]+/;

/** Link keys of every URL-ish token in a text (anything with a dot). */
export function urlKeysIn(text: string): string[] {
  const keys: string[] = [];
  for (const raw of text.split(SPLIT)) {
    const tok = raw.replace(/[.,;:!?]+$/, "");
    if (!tok.includes(".")) continue;
    const u = parseUrl(tok.startsWith("http://") || tok.startsWith("https://") ? tok : `https://${tok}`);
    if (u && !shorthandAddress(u, tok)) keys.push(...seenKeys(u));
  }
  return keys;
}

const IPV4 = /^\d+\.\d+\.\d+\.\d+$/;

/** Is `tok` an IPv4 address in a shorthand the URL parser expands (`1.5`
 * is 1.0.0.5)? Such a token is a number, not an address someone gave: a
 * form answer carries numbers the model chose. Same rule as the server. */
function shorthandAddress(u: URL, tok: string): boolean {
  if (!IPV4.test(u.hostname)) return false;
  const rest = tok.includes("://") ? tok.slice(tok.indexOf("://") + 3) : tok;
  const authority = rest.split(/[/?#]/)[0] ?? "";
  const host = authority.slice(authority.lastIndexOf("@") + 1).split(":")[0] ?? "";
  return host !== u.hostname;
}

// Data rides in a path as easily as in a query, so all but short paths ask.
const MAX_PATH = 24;
const MAX_LABEL = 20;
const MAX_LABELS = 3;
const ALNUM_RUN = /[A-Za-z0-9]{12,}/g;

/** A run of 12 or more letters and digits, mixed: an id, key or encoded
 * value rather than a word. */
function tokenLike(s: string): boolean {
  return (s.match(ALNUM_RUN) ?? []).some((r) => /\d/.test(r) && /[A-Za-z]/.test(r));
}

/**
 * Should a click show the full URL first? Only for URLs that came from
 * nowhere but the model and that could carry data out (a query, a fragment,
 * a path that is long, holds a token-like run or a `;param`, a long or
 * token-like host label, many labels) or mislead (a punycode host,
 * credentials before the host, hidden characters in a mail address).
 */
export function needsConfirm(u: URL, provenance: Provenance): boolean {
  if (provenance.has(linkKey(u))) return false;
  if (u.search !== "" || u.hash !== "") return true;
  if (u.protocol === "mailto:") {
    const addr = decodeSafe(u.pathname);
    return oddChars(addr) || addr.length > 64 || tokenLike(addr);
  }
  const labels = u.hostname.replace(/^www\./, "").split(".");
  return (
    u.username !== "" ||
    u.password !== "" ||
    u.pathname.length > MAX_PATH ||
    u.pathname.includes(";") ||
    tokenLike(u.pathname) ||
    u.hostname.includes("xn--") ||
    labels.length > MAX_LABELS ||
    labels.some((l) => l.length > MAX_LABEL || tokenLike(l))
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
