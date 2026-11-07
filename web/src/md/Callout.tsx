import { Info, Lightbulb, MessageSquareWarning, OctagonAlert, TriangleAlert } from "lucide-react";
import { Children, isValidElement, type ComponentProps } from "react";
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

type Props = ComponentProps<"aside"> & ExtraProps & { "data-tone"?: string };

/** The model's title: its text, with citation pills where it cites. */
export function CalloutTitle({ children }: ComponentProps<"span"> & ExtraProps) {
  return <span className="callout-title">{children}</span>;
}

/** The `aside` remark-callouts makes. The label is ours; the title (the
 * `callout-title` child, moved into the head) is the model's. */
export function Callout(props: Props) {
  const tone = props["data-tone"] as Tone | undefined;
  if (!tone || !(tone in ICONS)) return <aside>{props.children}</aside>;
  const Icon = ICONS[tone];
  const parts = Children.toArray(props.children);
  const at = parts.findIndex((c) => isValidElement(c) && c.type === CalloutTitle);
  const title = at >= 0 ? parts.splice(at, 1)[0] : null;
  return (
    <aside className={`callout callout-${tone}`} role="note">
      <div className="callout-head">
        <Icon size={16} aria-hidden />
        <span className="callout-label">{rt(`callout.${tone}`)}</span>
        {title}
      </div>
      {parts}
    </aside>
  );
}
