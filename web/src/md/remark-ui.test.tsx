// ui fences through the whole rendering path, as static markup, against the
// texts and blocks Rust wrote to ui.json and the counts each case records by
// hand: how many fences the browser marks and how many cards and error
// notes it shows. Where micromark and Rust disagree about a fence, nothing
// may become a card.
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test } from "vitest";
import fixture from "../__fixtures__/ui.json";
import { Markdown } from "../components/Markdown";
import type { UiBlock } from "../types";
import { slotKey, type ProgressSlot } from "../ui/blocks";
import { ProgressCtx } from "./context";
import { fnv1a } from "./fnv";
import { uiOpener } from "./remark-ui";

const md = (text: string, done: boolean, blocks: readonly UiBlock[] = [], from: string | null = null) =>
  renderToStaticMarkup(<Markdown text={text} done={done} blocks={blocks} from={from} />);

const count = (html: string, cls: string) => html.split(`class="${cls}`).length - 1;
const unescape = (html: string) =>
  html.replace(/&quot;/g, '"').replace(/&#x27;/g, "'").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&");

const cases = fixture.texts.map((t) => ({ ...t, blocks: t.blocks as unknown as UiBlock[] }));

describe("ui.json parity", () => {
  test.each(cases.map((c) => [c.name, c] as const))("%s", (_, c) => {
    // While streaming, every marked fence is a skeleton: as many as the
    // browser is expected to mark.
    expect(count(md(c.text, false), "ui-skeleton")).toBe(c.web_marks);
    // Once final, each stored block the browser finds shows: a card for a
    // spec, the code and the reason for an error. A marked fence with no
    // matching block stays code.
    const html = unescape(md(c.text, true, c.blocks));
    expect(count(html, "ui-card")).toBe(c.cards);
    expect(count(html, "ui-error")).toBe(c.errors);
    for (const b of c.blocks) if (b.error) expect(html).toContain(`: ${b.error}</div>`);
    expect(count(html, "codeblock")).toBeGreaterThanOrEqual(c.web_marks - c.cards);
  });

  test("the browser hashes what Rust hashed", () => {
    for (const c of cases) {
      for (const b of c.blocks) expect(b.hash).toMatch(/^[0-9a-f]{8}$/);
    }
    const every = cases.find((c) => c.name === "every type")!;
    const pretty = every.blocks[3]!;
    expect(pretty.hash).toBe(fnv1a('{\n  "id": "p",\n\n  "title": "Pretty",\n\t"steps": [{"label": "a", "status": "done"}]\n}'));
  });

  test("a block shows only where ordinal, hash and type all match", () => {
    const c = cases.find((c) => c.name === "every type")!;
    const shown = (blocks: UiBlock[]) => count(md(c.text, true, blocks), "ui-card");
    expect(shown(c.blocks)).toBe(4);
    const first = c.blocks[0]!;
    expect(shown([{ ...first, hash: "00000000" }])).toBe(0);
    expect(shown([{ ...first, ordinal: 1 }])).toBe(0);
    expect(shown([{ ...first, type: "form" } as UiBlock])).toBe(0);
    expect(shown([first])).toBe(1);
  });
});

describe("remark-ui", () => {
  test("only a root fence at column 1 with exactly ui:<type> is marked", () => {
    const skeletons = (text: string) => count(md(text, false), "ui-skeleton");
    expect(skeletons("```ui:choices\n{}\n```")).toBe(1);
    expect(skeletons("~~~ ui:form \t\n{}\n~~~")).toBe(1);
    for (const text of [
      " ```ui:choices\n{}\n```",
      "```ui:choices x\n{}\n```",
      "```ui:widget\n{}\n```",
      "```ui:Choices\n{}\n```",
      "```ui&#58;choices\n{}\n```",
      "```ui\\:choices\n{}\n```",
      "- a\n\n  ```ui:choices\n  {}\n  ```",
      "> ```ui:choices\n> {}\n> ```",
      "Inline ```ui:choices``` here.",
    ]) {
      expect(skeletons(text), text).toBe(0);
    }
  });

  test("while streaming a typed skeleton, never the raw JSON", () => {
    const out = md('Here:\n\n```ui:form\n{"id": "trip", "title": "Tr', false);
    expect(out).toContain('<div class="ui-skeleton" role="status">Preparing form…</div>');
    expect(out).not.toContain("trip");
    expect(md("```ui:choices\n{}\n```", false)).toContain("Preparing question…");
  });

  test("the card frame names the agent", () => {
    const c = cases.find((c) => c.name === "form with a refused field")!;
    expect(md(c.text, true, c.blocks, "Coder")).toContain('<div class="ui-from">from Coder</div>');
    expect(md(c.text, true, c.blocks)).toContain('<div class="ui-from">from the assistant</div>');
  });

  test("a rejected block is its code plus the reason", () => {
    const c = cases.find((c) => c.name === "display level")!;
    const html = md(c.text, true, c.blocks);
    expect(html).toContain('<span class="lang">ui:choices</span>');
    expect(html).toContain("Could not show this question: interactive blocks are off for this agent");
  });

  test("a progress block a later one replaced loses the card frame", () => {
    const c = cases.find((c) => c.name === "every type")!;
    const progress = c.blocks.find((b) => b.type === "progress")!;
    const render = (slot: ProgressSlot) =>
      renderToStaticMarkup(
        <ProgressCtx.Provider value={new Map([[slotKey("m", progress.ordinal), slot]])}>
          <Markdown text={c.text} done blocks={c.blocks} messageId="m" />
        </ProgressCtx.Provider>,
      );
    // The case has two progress blocks; only the first has a slot here.
    expect(count(render({ mode: "full", ended: false }), "ui-card ui-card-progress")).toBe(2);
    expect(count(render({ mode: "snapshot", ended: false }), "ui-card ui-card-progress")).toBe(1);
    expect(count(render({ mode: "snapshot", ended: false }), "ui-card")).toBe(c.cards - 1);
  });

  test("opener lines", () => {
    expect(uiOpener("```ui:chart")).toBe("chart");
    expect(uiOpener("~~~~  ui:progress  ")).toBe("progress");
    for (const line of ["``ui:chart", " ```ui:chart", "```ui:chart x", "```ui:pie", "```ui:chart`"]) {
      expect(uiOpener(line), line).toBeNull();
    }
  });
});
