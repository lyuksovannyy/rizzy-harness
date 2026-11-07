// Model output rendered as GitHub-flavoured Markdown. Raw HTML stays escaped
// (no rehype-raw), URLs pass an allowlist, images never load, and syntax
// highlighting and math render lazily once the message is final (`done`).
// Conversation provides MessageCtx (provenance and sources for links).
import { memo, useMemo } from "react";
import ReactMarkdown, { type Components, type Options } from "react-markdown";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import { Callout } from "../md/Callout";
import { InlineCode, Pre } from "../md/Code";
import { DoneCtx } from "../md/context";
import { ImageChip, Link } from "../md/Link";
import { prepare, urlTransform } from "../md/pipeline";
import remarkCallouts from "../md/remark-callouts";
import rehypeStripHidden from "../md/hidden";
import { Table } from "../md/Table";

// Single-dollar math is off: "$5 and $10" and $HOME are text. prepare()
// turns \( \) and \[ \] into the $$ forms.
const remarkPlugins: Options["remarkPlugins"] = [remarkGfm, [remarkMath, { singleDollarTextMath: false }], remarkCallouts];
// prepare() strips hidden characters from the raw text; this strips the ones
// that character references (&#x202E;) decode to.
const rehypePlugins: Options["rehypePlugins"] = [rehypeStripHidden];

const components: Components = {
  a: Link,
  img: ImageChip,
  pre: Pre,
  code: InlineCode,
  table: Table,
  aside: Callout,
};

export const Markdown = memo(function Markdown({ text, done }: { text: string; done: boolean }) {
  const md = useMemo(() => prepare(text), [text]);
  return (
    <DoneCtx.Provider value={done}>
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
    </DoneCtx.Provider>
  );
});
