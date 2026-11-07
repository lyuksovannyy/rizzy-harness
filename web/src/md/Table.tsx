// GFM tables: a scrolling wrapper, sortable header cells and Copy CSV. The
// rows keep react-markdown's rendering (links, code); only their order and
// the header buttons are ours.
import { Children, cloneElement, isValidElement, useMemo, useState, type ComponentProps, type ReactElement, type ReactNode } from "react";
import type { ExtraProps } from "react-markdown";
import { useCopy } from "./copy";
import { hastText, type HastNode } from "./hast";
import { rt } from "./strings";
import { sortOrder, toCsv } from "./table";

type Props = ComponentProps<"table"> & ExtraProps;
type Sort = { col: number; dir: 1 | -1 } | null;
type El = ReactElement<{ children?: ReactNode }>;

const elements = (n: HastNode | undefined, tag: string) =>
  (n?.children ?? []).filter((c) => c.type === "element" && c.tagName === tag);

/** Header and body cell texts, read from the hast table. */
function tableData(node: HastNode | undefined): { head: string[]; body: string[][] } {
  const rowCells = (tr: HastNode) => (tr.children ?? []).filter((c) => c.type === "element").map(hastText);
  const headRow = elements(elements(node, "thead")[0], "tr")[0];
  return {
    head: headRow ? rowCells(headRow) : [],
    body: elements(elements(node, "tbody")[0], "tr").map(rowCells),
  };
}

const kids = (el: El) => Children.toArray(el.props.children).filter(isValidElement) as El[];

export function Table({ node, children }: Props) {
  const data = useMemo(() => tableData(node as HastNode | undefined), [node]);
  const [sort, setSort] = useState<Sort>(null);
  const [copied, copy] = useCopy();
  const order = useMemo(() => (sort ? sortOrder(data.body, sort.col, sort.dir) : null), [data, sort]);
  const next = (col: number) =>
    setSort((s) => (s?.col !== col ? { col, dir: 1 } : s.dir === 1 ? { col, dir: -1 } : null));

  const parts = Children.toArray(children).map((part) => {
    if (!isValidElement(part)) return part;
    const el = part as El;
    if (el.type === "thead") {
      return cloneElement(
        el,
        undefined,
        kids(el).map((tr) =>
          cloneElement(
            tr,
            undefined,
            kids(tr).map((th, col) =>
              cloneElement(
                th as ReactElement<{ children?: ReactNode; "aria-sort"?: string }>,
                { "aria-sort": sort?.col === col ? (sort.dir === 1 ? "ascending" : "descending") : "none" },
                <button
                  type="button"
                  className="th-sort"
                  title={rt("sortBy", { col: data.head[col] ?? "" })}
                  onClick={() => next(col)}
                >
                  {th.props.children}
                </button>,
              ),
            ),
          ),
        ),
      );
    }
    if (el.type === "tbody" && order) {
      const rows = kids(el);
      return cloneElement(el, undefined, order.map((i) => rows[i]));
    }
    return el;
  });

  return (
    <div className="table-wrap">
      <div className="table-scroll">
        <table>{parts}</table>
      </div>
      <div className="table-tools">
        <button
          type="button"
          className="link small"
          onClick={() => copy(toCsv([data.head, ...(order ? order.map((i) => data.body[i]!) : data.body)]))}
        >
          {copied ? rt("copied") : rt("copyCsv")}
        </button>
      </div>
    </div>
  );
}
