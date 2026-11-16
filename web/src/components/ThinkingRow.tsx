import { useState } from "react";
import { t } from "../lib/i18n";
import type { Item } from "../types";

/** What the row shows at first: the rest waits for "Show all". */
export const SHOWN_CHARS = 64 * 1024;

/** One step's thinking (X.25), collapsed: its readable text as text (the
 * model's words, never markup), cut to {@link SHOWN_CHARS} until asked, and
 * one line for the redacted parts. Nothing when nothing is readable. */
export function ThinkingRow({ it }: { it: Extract<Item, { type: "thinking" }> }) {
  const [all, setAll] = useState(false);
  const blocks = Array.isArray(it.blocks) ? it.blocks : [];
  const text = blocks
    .flatMap((b) => (b && b.type !== "redacted" && typeof b.text === "string" && /[^\p{White_Space}]/u.test(b.text) ? [b.text] : []))
    .join("\n\n");
  const redacted = blocks.filter((b) => b?.type === "redacted").length;
  if (!text && !redacted) return null;
  const cut = !all && text.length > SHOWN_CHARS;
  return (
    <details className="divider summary" data-testid="thinking">
      <summary>{t("conv.thinking")}</summary>
      {text && <p>{cut ? text.slice(0, SHOWN_CHARS) : text}</p>}
      {cut && (
        <button type="button" className="link" onClick={() => setAll(true)}>
          {t("conv.thinkingAll")}
        </button>
      )}
      {!!redacted && <p>{t("conv.thinkingRedacted", { n: String(redacted) })}</p>}
    </details>
  );
}
