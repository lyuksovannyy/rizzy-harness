// Citation pills and the Sources footer. A label resolves only through
// MessageCtx.cites, the sources our web tools recorded before the message:
// the model chooses which label to write, never the URL behind it. A label
// with no source stays muted text.
import { useContext, useEffect, useId, useRef, useState, type ComponentProps, type RefObject } from "react";
import type { ExtraProps } from "react-markdown";
import type { Source } from "../types";
import { useAnchored } from "./anchored";
import { DoneCtx, MessageCtx } from "./context";
import { stripHidden } from "./hidden";
import { hostOf, parseUrl } from "./links";
import { rt } from "./strings";

type Props = ComponentProps<"span"> & ExtraProps & { "data-labels"?: string };

interface Resolved {
  source: Source;
  url: URL;
  host: string;
  title: string;
}

/** The source behind a label, if it is one with a web URL. */
function resolve(cites: ReadonlyMap<string, Source>, label: string): Resolved | null {
  const source = cites.get(label);
  const url = source ? parseUrl(source.url) : null;
  if (!source || !url || (url.protocol !== "https:" && url.protocol !== "http:")) return null;
  const host = hostOf(url);
  return { source, url, host, title: stripHidden(source.title) || host };
}

const labelsOf = (p: Props) => (p["data-labels"] ?? "").split(" ").filter(Boolean);

const EXTERNAL = { target: "_blank", rel: "noopener noreferrer nofollow", referrerPolicy: "no-referrer" } as const;

/** A run of adjacent labels: one pill for the sources it names (`host +k`)
 * and the unknown labels as muted text. */
export function CiteRef(props: Props) {
  const { cites } = useContext(MessageCtx);
  const found: Resolved[] = [];
  const missing: string[] = [];
  for (const label of labelsOf(props)) {
    const r = resolve(cites, label);
    if (!r) missing.push(label);
    else if (!found.some((f) => f.source.id === r.source.id)) found.push(r);
  }
  return (
    <>
      {found.length > 0 && <Pill found={found} />}
      {missing.map((label) => (
        <span key={label} className="cite-missing" title={rt("citeMissing")}>
          [{label}]
        </span>
      ))}
    </>
  );
}

function Pill({ found }: { found: Resolved[] }) {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLAnchorElement>(null);
  const timer = useRef<number | undefined>(undefined);
  const cardId = useId();
  useEffect(() => () => window.clearTimeout(timer.current), []);
  const show = () => {
    window.clearTimeout(timer.current);
    setOpen(true);
  };
  // A short delay, so the pointer can cross the gap into the card.
  const hide = () => {
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setOpen(false), 150);
  };
  const first = found[0]!;
  return (
    <span
      className="cite-wrap"
      onMouseEnter={show}
      onMouseLeave={hide}
      onFocus={show}
      onBlur={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node | null)) hide();
      }}
      onKeyDown={(e) => {
        if (e.key !== "Escape") return;
        // Focus on a link in the card would go with the card, to the top
        // of the page; it goes back to the pill instead. (The pill's focus
        // reopens the card; the close below comes after it.)
        if (document.activeElement !== anchor.current && e.currentTarget.contains(document.activeElement)) {
          anchor.current?.focus();
        }
        setOpen(false);
      }}
    >
      <a
        ref={anchor}
        className="cite"
        href={first.url.href}
        {...EXTERNAL}
        aria-label={rt("citeLabel", { label: first.source.id, title: first.title, host: first.host })}
        aria-describedby={open ? cardId : undefined}
      >
        <bdi dir="ltr">{first.host}</bdi>
        {found.length > 1 && ` +${found.length - 1}`}
      </a>
      {open && <Card id={cardId} found={found} anchor={anchor} />}
    </span>
  );
}

function Card({ id, found, anchor }: { id: string; found: Resolved[]; anchor: RefObject<HTMLAnchorElement | null> }) {
  const box = useRef<HTMLSpanElement>(null);
  useAnchored(box, anchor);
  return (
    <span className="cite-card" popover="manual" id={id} ref={box}>
      {found.map((r) => (
        <a key={r.source.id} className="cite-card-item" href={r.url.href} {...EXTERNAL}>
          <span className="cite-card-title">{r.title}</span>
          <span className="cite-card-url">{r.url.href}</span>
        </a>
      ))}
    </span>
  );
}

/** Under a final message that cites: the sources it named, in the order it
 * first named them. */
export function SourcesFooter(props: Props) {
  const done = useContext(DoneCtx);
  const { cites } = useContext(MessageCtx);
  if (!done) return null;
  const list: Resolved[] = [];
  for (const label of labelsOf(props)) {
    const r = resolve(cites, label);
    if (r && !list.some((f) => f.source.id === r.source.id)) list.push(r);
  }
  if (!list.length) return null;
  return (
    <details className="sources">
      <summary>{rt("sources", { n: String(list.length) })}</summary>
      <ol>
        {list.map((r) => (
          <li key={r.source.id}>
            <a href={r.url.href} {...EXTERNAL}>
              {r.title}
            </a>{" "}
            <bdi dir="ltr" className="link-host">
              {r.host}
            </bdi>
          </li>
        ))}
      </ol>
    </details>
  );
}
