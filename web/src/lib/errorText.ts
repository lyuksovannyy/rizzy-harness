// What a person reads for a failed request: the server's own text, then the
// request id the server sent, so it can be copied to whoever runs the server.
// Lazy by construction: only pages that already load i18nPages use it.
import "./i18nPages";
import { HttpError } from "../api";

export function errText(x: unknown): string {
  if (!(x instanceof Error)) return String(x);
  if (!(x instanceof HttpError)) return x.message;
  return x.id ? `${x.message} (request ${x.id})` : x.message;
}
