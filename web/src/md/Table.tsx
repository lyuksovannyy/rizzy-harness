// GFM tables: a scrolling wrapper, sortable header cells and Copy CSV. The
// rows keep react-markdown's rendering (links, code); only their order and
// the header buttons are ours.
import { ArrowDown, ArrowUp, ArrowUpDown } from "lucide-react";
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

// What renders as a link: links and footnote references, image chips and
// citation pills.
const INTERACTIVE = new Set(["a", "img", "cite-ref"]);
const interactive = (n: HastNode): boolean =>
  n.type === "element" && (INTERACTIVE.has(n.tagName ?? "") || (n.children ?? []).some(interactive));

/** Header and body cell texts, read from the hast table, and which header
 * cells hold a link. */
function tableData(node: HastNode | undefined): { head: string[]; links: boolean[]; body: string[][] } {
  const cells = (tr: HastNode) => (tr.children ?? []).filter((c) => c.type === "element");
  const headRow = elements(elements(node, "thead")[0], "tr")[0];
  const head = headRow ? cells(headRow) : [];
  return {
    head: head.map(hastText),
    links: head.map(interactive),
    body: elements(elements(node, "tbody")[0], "tr").map((tr) => cells(tr).map(hastText)),
  };
}

const kids = (el: El) => Children.toArray(el.props.children).filter(isValidElement) as El[];

/** A header cell's content as the sort button. A link cannot sit inside a
 * button (neither would work as itself), so a cell with one keeps its
 * content as it is, followed by a small sort button of its own. */
function HeadCell(props: { label: string; link: boolean; dir: 1 | -1 | null; onSort: () => void; children: ReactNode }) {
  const title = rt("sortBy", { col: props.label });
  if (!props.link) {
    return (
      <button type="button" className="th-sort" title={title} onClick={props.onSort}>
        {props.children}
      </button>
    );
  }
  const Icon = props.dir === 1 ? ArrowUp : props.dir === -1 ? ArrowDown : ArrowUpDown;
  return (
    <span className="th-cell">
      <span className="th-label">{props.children}</span>
      <button type="button" className="th-sort-icon" title={title} aria-label={title} onClick={props.onSort}>
        <Icon size={12} aria-hidden="true" />
      </button>
    </span>
  );
}

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
            kids(tr).map((th, col) => {
              const dir = sort?.col === col ? sort.dir : null;
              return cloneElement(
                th as ReactElement<{ children?: ReactNode; "aria-sort"?: string }>,
                { "aria-sort": dir === 1 ? "ascending" : dir === -1 ? "descending" : "none" },
                <HeadCell
                  label={data.head[col]?.trim() || rt("column", { n: String(col + 1) })}
                  link={!!data.links[col]}
                  dir={dir}
                  onSort={() => next(col)}
                >
                  {th.props.children}
                </HeadCell>,
              );
            }),
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
