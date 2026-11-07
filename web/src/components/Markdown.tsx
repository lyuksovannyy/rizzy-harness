// Model output rendered as GitHub-flavoured Markdown. Raw HTML stays escaped
// (no rehype-raw), URLs pass an allowlist, images never load, and syntax
// highlighting and math render lazily once the message is final (`done`).
// Conversation provides MessageCtx (provenance and sources for links, and
// the sources behind citation labels); `blocks` are the message's ui blocks
// as Rust validated them, shown in place of their fences.
import { memo, useMemo } from "react";
import ReactMarkdown, { type Components, type Options } from "react-markdown";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import { Callout, CalloutTitle } from "../md/Callout";
import { CiteRef, SourcesFooter } from "../md/Cite";
import { InlineCode, Pre } from "../md/Code";
import { DoneCtx, NO_BLOCKS, UiCtx } from "../md/context";
import { ImageChip, Link } from "../md/Link";
import { prepare, urlTransform } from "../md/pipeline";
import remarkCallouts from "../md/remark-callouts";
import remarkCite, { rehypeSourcesLast } from "../md/remark-cite";
import remarkUi from "../md/remark-ui";
import rehypeStripHidden from "../md/hidden";
import { Table } from "../md/Table";
import { UiHost } from "../md/UiHost";
import type { UiBlock } from "../types";

// Single-dollar math is off: "$5 and $10" and $HOME are text. prepare()
// turns \( \) and \[ \] into the $$ forms.
const remarkPlugins: Options["remarkPlugins"] = [
  remarkGfm,
  [remarkMath, { singleDollarTextMath: false }],
  remarkCallouts,
  remarkCite,
  remarkUi,
];
// prepare() strips hidden characters from the raw text; this strips the ones
// that character references (&#x202E;) decode to.
const rehypePlugins: Options["rehypePlugins"] = [rehypeStripHidden, rehypeSourcesLast];

// The custom elements remark-callouts, remark-cite and remark-ui make are
// not HTML tags, hence the cast.
const components = {
  a: Link,
  img: ImageChip,
  pre: Pre,
  code: InlineCode,
  table: Table,
  aside: Callout,
  "callout-title": CalloutTitle,
  "cite-ref": CiteRef,
  "cite-sources": SourcesFooter,
  "ui-block": UiHost,
} as Components;

export const Markdown = memo(function Markdown(props: {
  text: string;
  done: boolean;
  blocks?: readonly UiBlock[];
  /** The agent's name for the "from" line of block cards. */
  from?: string | null;
  /** The message's id, where its progress slots are looked up. */
  messageId?: string;
}) {
  const { text, done } = props;
  const md = useMemo(() => prepare(text), [text]);
  const blocks = props.blocks ?? NO_BLOCKS;
  const from = props.from ?? null;
  const messageId = props.messageId ?? null;
  const ui = useMemo(() => ({ messageId, blocks, from }), [messageId, blocks, from]);
  return (
    <DoneCtx.Provider value={done}>
      <UiCtx.Provider value={ui}>
        <div className="md">
          <ReactMarkdown
            remarkPlugins={remarkPlugins}
            rehypePlugins={rehypePlugins}
            components={components}
            urlTransform={urlTransform}
          >
            {md}
          </ReactMarkdown>
        </div>
      </UiCtx.Provider>
    </DoneCtx.Provider>
  );
});
