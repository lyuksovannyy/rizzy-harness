// What a person reads for a failed request. The server's own text stays as
// it is when it has one (the master writes them as sentences); a bare status
// line ("403 Forbidden": the guard's plain-text refusals, a proxy's answer)
// becomes a plain sentence, a 429 names its wait, and the request id the
// server sent trails the text so it can be copied to whoever runs the server.
// Lazy by construction: the strings live in i18nPages.
import "./i18nPages";
import { HttpError } from "../api";
import { t } from "./i18n";

const BARE = /^(403|422|429|503) /;

export function errText(x: unknown): string {
  if (!(x instanceof Error)) return String(x);
  if (!(x instanceof HttpError)) return x.message;
  let m = x.message;
  const wait = x.retry ? t("error.in", { n: String(x.retry) }) : "";
  if (BARE.test(m)) m = t(`error.${x.status}` as "error.403", { in: wait || t("error.later") });
  else if (x.status === 429 && wait && !/try again/i.test(m)) m += `. ${wait}`;
  return x.id ? `${m} (request ${x.id})` : m;
}
