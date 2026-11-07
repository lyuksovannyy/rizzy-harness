import { Info, Lightbulb, MessageSquareWarning, OctagonAlert, TriangleAlert } from "lucide-react";
import type { ComponentProps } from "react";
import type { ExtraProps } from "react-markdown";
import type { Tone } from "./remark-callouts";
import { rt } from "./strings";

const ICONS = {
  note: Info,
  tip: Lightbulb,
  important: MessageSquareWarning,
  warning: TriangleAlert,
  caution: OctagonAlert,
} as const;

type Props = ComponentProps<"aside"> & ExtraProps & { "data-tone"?: string; "data-title"?: string };

/** The `aside` remark-callouts makes. The label is ours; the title is the
 * model's, as plain text. */
export function Callout(props: Props) {
  const tone = props["data-tone"] as Tone | undefined;
  if (!tone || !(tone in ICONS)) return <aside>{props.children}</aside>;
  const Icon = ICONS[tone];
  const title = props["data-title"];
  return (
    <aside className={`callout callout-${tone}`} role="note">
      <div className="callout-head">
        <Icon size={16} aria-hidden />
        <span className="callout-label">{rt(`callout.${tone}`)}</span>
        {title && <span className="callout-title">{title}</span>}
      </div>
      {props.children}
    </aside>
  );
}
