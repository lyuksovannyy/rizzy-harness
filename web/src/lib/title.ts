import { t } from "./i18n";

const MAX = 60;

export function titleFrom(text: string): string {
  const line = text.split("\n")[0]!.replace(/^\s*@[a-z0-9-]+[\s,:]*/, "").trim();
  if (!line) return t("session.untitled");
  return line.length > MAX ? line.slice(0, MAX).trimEnd() + "…" : line;
}
