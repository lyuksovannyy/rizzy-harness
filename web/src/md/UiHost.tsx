// The `ui-block` element remark-ui makes for a `ui:` fence. While the
// message streams it is a typed skeleton, never the raw JSON. Once the
// message is final it shows the block Rust validated and stored for this
// fence (assistant_message.blocks), found by ordinal, hash and type; with
// no such block the fence is ordinary code, and a block Rust rejected is
// the code plus the reason. A fence that never finished (the message was
// cut off mid-stream, or the text never closes it) is half a JSON body, so
// it stays out of sight behind a harness note instead. The cards load
// lazily, each in a "from <agent>" frame: the frame is what tells model
// content from harness chrome, so a progress snapshot line keeps a compact
// one.
import { lazy, Suspense, useContext, type ComponentProps } from "react";
import type { ExtraProps } from "react-markdown";
import type { UiKind } from "../types";
import { CodeBlock } from "./Code";
import { slotKey } from "../ui/blocks";
import { DoneCtx, ProgressCtx, UiCtx } from "./context";
import { hastText, type HastNode } from "./hast";
import { rt } from "./strings";

const UiBlock = lazy(() => import("../ui/UiBlock"));

type Props = ComponentProps<"div"> &
  ExtraProps & {
    "data-type"?: UiKind;
    "data-ordinal"?: number | string;
    "data-hash"?: string;
    /** Set when the text never closes the fence. */
    "data-open"?: string;
  };

function Skeleton({ type }: { type: UiKind }) {
  return (
    <div className="ui-skeleton" role="status">
      {rt(`ui.preparing.${type}`)}
    </div>
  );
}

export function UiHost(props: Props) {
  const done = useContext(DoneCtx);
  const { messageId, blocks, from, cut } = useContext(UiCtx);
  const slots = useContext(ProgressCtx);
  const type = props["data-type"] ?? "progress";
  if (!done) return <Skeleton type={type} />;
  const ordinal = Number(props["data-ordinal"]);
  const block = blocks.find((b) => b.ordinal === ordinal && b.hash === props["data-hash"] && b.type === type);
  if (!block && (cut || props["data-open"] !== undefined)) {
    return (
      <div className="ui-cut" role="note">
        {rt("ui.cut")}
      </div>
    );
  }
  const code = <CodeBlock code={hastText(props.node as HastNode | undefined)} lang={`ui:${type}`} />;
  if (!block) return code;
  if (!block.spec) {
    return (
      <>
        {code}
        <div className="ui-error" role="note">
          {rt("ui.error", { what: rt(`ui.what.${type}`), error: block.error ?? "" })}
        </div>
      </>
    );
  }
  const slot = block.type === "progress" && messageId ? slots.get(slotKey(messageId, ordinal)) : undefined;
  const snapshot = slot?.mode === "snapshot";
  return (
    <section className={snapshot ? "ui-card-snapshot" : `ui-card ui-card-${type}`}>
      <div className="ui-from">{from ? rt("ui.from", { name: from }) : rt("ui.fromAssistant")}</div>
      <Suspense fallback={<Skeleton type={type} />}>
        <UiBlock block={block} slot={slot} />
      </Suspense>
    </section>
  );
}
