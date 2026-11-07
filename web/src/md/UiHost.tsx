// The `ui-block` element remark-ui makes for a `ui:` fence. While the
// message streams it is a typed skeleton, never the raw JSON. Once the
// message is final it shows the block Rust validated and stored for this
// fence (assistant_message.blocks), found by ordinal, hash and type; with
// no such block the fence is ordinary code, and a block Rust rejected is
// the code plus the reason. The cards themselves load lazily.
import { lazy, Suspense, useContext, type ComponentProps } from "react";
import type { ExtraProps } from "react-markdown";
import type { UiKind } from "../types";
import { CodeBlock } from "./Code";
import { DoneCtx, UiCtx } from "./context";
import { hastText, type HastNode } from "./hast";
import { rt } from "./strings";

const UiBlock = lazy(() => import("../ui/UiBlock"));

type Props = ComponentProps<"div"> &
  ExtraProps & { "data-type"?: UiKind; "data-ordinal"?: number | string; "data-hash"?: string };

function Skeleton({ type }: { type: UiKind }) {
  return (
    <div className="ui-skeleton" role="status">
      {rt(`ui.preparing.${type}`)}
    </div>
  );
}

export function UiHost(props: Props) {
  const done = useContext(DoneCtx);
  const { blocks, from } = useContext(UiCtx);
  const type = props["data-type"] ?? "progress";
  if (!done) return <Skeleton type={type} />;
  const ordinal = Number(props["data-ordinal"]);
  const block = blocks.find((b) => b.ordinal === ordinal && b.hash === props["data-hash"] && b.type === type);
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
  return (
    <section className={`ui-card ui-card-${type}`}>
      <div className="ui-from">{from ? rt("ui.from", { name: from }) : rt("ui.fromAssistant")}</div>
      <Suspense fallback={<Skeleton type={type} />}>
        <UiBlock block={block} />
      </Suspense>
    </section>
  );
}
