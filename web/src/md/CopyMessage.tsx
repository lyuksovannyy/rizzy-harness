import { Check, Copy } from "lucide-react";
import { useCopy } from "./copy";
import { prepare } from "./pipeline";
import { rt } from "./strings";

/** The hover action under a final assistant message: copies the Markdown as
 * rendered (hidden characters stripped, math normalised). */
export function CopyMessage({ text }: { text: string }) {
  const [copied, copy] = useCopy();
  return (
    <div className="msg-actions">
      <button type="button" className="link small" title={rt("copyMessageHint")} onClick={() => copy(prepare(text))}>
        {copied ? <Check size={14} aria-hidden /> : <Copy size={14} aria-hidden />} {copied ? rt("copied") : rt("copyMessage")}
      </button>
    </div>
  );
}
