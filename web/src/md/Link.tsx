// Links and images in model output. The real host is always visible, a URL
// the model made up that could carry data out asks first, and images are
// never fetched: they become chips that link to the image. Hosts render in
// an isolated left-to-right <bdi>, so no text around them can reorder them.
import { Image as ImageIcon } from "lucide-react";
import {
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  type ComponentProps,
  type ReactNode,
  type RefObject,
} from "react";
import type { ExtraProps } from "react-markdown";
import { useAnchored } from "./anchored";
import { MessageCtx, UiCtx } from "./context";
import { useCopy } from "./copy";
import { reactText } from "./hast";
import { hostOf, leavesApp, linkKey, needsConfirm, parseUrl, textShowsTarget } from "./links";
import { rt } from "./strings";

type AProps = ComponentProps<"a"> & ExtraProps;

export function Link({ href = "", children, id, "aria-describedby": describedBy, "aria-label": label }: AProps) {
  const { fn } = useContext(UiCtx);
  // In-page links work only between this message's GFM footnotes and their
  // references, whose ids the renderer gave this message's prefix (AUDIT
  // L14). Any other fragment (an app route, a button's or field's id,
  // another message's footnote) is text. The app routes on the URL hash, so
  // a footnote link scrolls instead of navigating.
  if (href.startsWith("#")) {
    const target = href.slice(1);
    if (!fn || !target.startsWith(fn) || !/^fn(ref)?-/.test(target.slice(fn.length)))
      return <span className="link-off">{children}</span>;
    return (
      <a
        href={href}
        id={id}
        aria-describedby={describedBy && fn + describedBy}
        aria-label={label}
        onClick={(e) => {
          e.preventDefault();
          e.currentTarget
            .closest(".md")
            ?.querySelector(`[id="${CSS.escape(target)}"]`)
            ?.scrollIntoView({ block: "center" });
        }}
      >
        {children}
      </a>
    );
  }
  const u = href ? parseUrl(href) : null;
  // A URL the allowlist removed (javascript:, relative, …) stays as text.
  if (!u) return <span className="link-off">{children}</span>;
  return (
    <External u={u} showHost={!textShowsTarget(reactText(children), u.href, hostOf(u))}>
      {children}
    </External>
  );
}

type ImgProps = ComponentProps<"img"> & ExtraProps;

export function ImageChip({ src = "", alt = "" }: ImgProps) {
  if (src === "data:") {
    return (
      <span className="img-chip off" title={rt("imageHint")}>
        <ImageIcon size={14} aria-hidden /> {rt("embedded")}
      </span>
    );
  }
  const u = src ? parseUrl(src) : null;
  const label = (
    <>
      <ImageIcon size={14} aria-hidden /> {rt("image")}
      {alt ? `: ${alt}` : ""}
      {u && (
        <>
          {" · "}
          <bdi dir="ltr" className="img-host">
            {hostOf(u)}
          </bdi>
        </>
      )}
    </>
  );
  if (!u) return <span className="img-chip off">{label}</span>;
  return (
    <External u={u} className="img-chip" title={rt("imageHint")}>
      {label}
    </External>
  );
}

function External(props: { u: URL; children: ReactNode; showHost?: boolean; className?: string; title?: string }) {
  const { u } = props;
  const { provenance, sources } = useContext(MessageCtx);
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLAnchorElement>(null);
  // The confirm holds focus while open; closing it from the keyboard or
  // its own buttons hands focus back to the link (it would drop to the top
  // of the page with the removed button). A click elsewhere keeps its own.
  const close = useCallback((refocus: boolean) => {
    setOpen(false);
    if (refocus) anchor.current?.focus();
  }, []);
  // With a fragment only when it is the source's own (see linkKey).
  const source = sources.has(linkKey(u));
  // Another port of this app's host is another server: its host is always
  // shown, with the port and a word that it is not this app, and it asks.
  const away = leavesApp(u);
  const confirm = away || (!source && needsConfirm(u, provenance));
  const guard = (e: React.MouseEvent) => {
    if (!confirm) return;
    e.preventDefault();
    setOpen(true);
  };
  const cls = [props.className, source ? "link-source" : ""].filter(Boolean).join(" ");
  return (
    <span className="link-wrap">
      <a
        ref={anchor}
        href={u.href}
        target="_blank"
        rel="noopener noreferrer nofollow"
        referrerPolicy="no-referrer"
        className={cls || undefined}
        title={props.title}
        onClick={guard}
        onAuxClick={guard}
      >
        {props.children}
      </a>
      {(props.showHost || away) && (
        <bdi dir="ltr" className={away ? "link-host link-away" : "link-host"}>
          {hostOf(u)}
          {away && ` · ${rt("linkAway")}`}
        </bdi>
      )}
      {open && <LinkConfirm url={u.href} away={away} anchor={anchor} onClose={close} />}
    </span>
  );
}

/** The confirm is a popover in the top layer, placed under (or above) its
 * link (see useAnchored). */
function LinkConfirm(props: {
  url: string;
  away?: boolean;
  anchor: RefObject<HTMLAnchorElement | null>;
  onClose: (refocus: boolean) => void;
}) {
  const { url, anchor, onClose } = props;
  const box = useRef<HTMLSpanElement>(null);
  const [copied, copy] = useCopy();
  useAnchored(box, anchor);
  useEffect(() => {
    box.current?.querySelector("button")?.focus();
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose(true);
    const onDown = (e: MouseEvent) => {
      if (!box.current?.contains(e.target as Node)) onClose(false);
    };
    document.addEventListener("keydown", onKey);
    document.addEventListener("mousedown", onDown);
    return () => {
      document.removeEventListener("keydown", onKey);
      document.removeEventListener("mousedown", onDown);
    };
  }, [onClose]);
  return (
    <span className="link-confirm" popover="manual" role="dialog" aria-label={rt("linkTitle")} ref={box}>
      <span className="link-confirm-note">
        {rt("linkCheck")}
        {props.away && ` (${rt("linkAway")})`}
      </span>
      <span className="link-confirm-url">{url}</span>
      <span className="link-confirm-actions">
        <button
          type="button"
          className="primary"
          onClick={() => {
            window.open(url, "_blank", "noopener,noreferrer");
            onClose(true);
          }}
        >
          {rt("open")}
        </button>
        <button type="button" onClick={() => copy(url)}>
          {copied ? rt("copied") : rt("copy")}
        </button>
        <button type="button" onClick={() => onClose(true)}>
          {rt("cancel")}
        </button>
      </span>
    </span>
  );
}
