// Links and images in model output. The real host is always visible, a URL
// the model made up that could carry data out asks first, and images are
// never fetched: they become chips that link to the image. Hosts render in
// an isolated left-to-right <bdi>, so no text around them can reorder them.
import { Image as ImageIcon } from "lucide-react";
import {
  useCallback,
  useContext,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ComponentProps,
  type ReactNode,
  type RefObject,
} from "react";
import type { ExtraProps } from "react-markdown";
import { MessageCtx } from "./context";
import { useCopy } from "./copy";
import { reactText } from "./hast";
import { hostOf, needsConfirm, parseUrl, textShowsTarget, urlKey } from "./links";
import { rt } from "./strings";

type AProps = ComponentProps<"a"> & ExtraProps;

export function Link({ href = "", children, id, "aria-describedby": describedBy, "aria-label": label }: AProps) {
  // GFM footnotes: in-page links. The app routes on the URL hash, so scroll
  // to the target instead of navigating.
  if (href.startsWith("#")) {
    return (
      <a
        href={href}
        id={id}
        aria-describedby={describedBy}
        aria-label={label}
        onClick={(e) => {
          e.preventDefault();
          const target = decodeSafe(href.slice(1));
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
  const close = useCallback(() => setOpen(false), []);
  const source = sources.has(urlKey(u));
  const confirm = !source && needsConfirm(u, provenance);
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
      {props.showHost && (
        <bdi dir="ltr" className="link-host">
          {hostOf(u)}
        </bdi>
      )}
      {open && <LinkConfirm url={u.href} anchor={anchor} onClose={close} />}
    </span>
  );
}

/** The confirm is a popover in the top layer, placed under (or above) its
 * link: a scrolling table or any other overflow box would clip it in place. */
function LinkConfirm({ url, anchor, onClose }: { url: string; anchor: RefObject<HTMLAnchorElement | null>; onClose: () => void }) {
  const box = useRef<HTMLSpanElement>(null);
  const [copied, copy] = useCopy();
  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    el.showPopover();
    const place = () => {
      const a = anchor.current?.getBoundingClientRect();
      if (!a) return;
      const gap = 4;
      const margin = 8;
      const left = Math.max(margin, Math.min(a.left, window.innerWidth - el.offsetWidth - margin));
      const below = a.bottom + gap;
      const above = a.top - gap - el.offsetHeight;
      const fits = below + el.offsetHeight <= window.innerHeight - margin || above < margin;
      el.style.left = `${left}px`;
      el.style.top = `${fits ? below : above}px`;
    };
    place();
    // Capture: a scroll of any box (the message list, a table) moves the link.
    window.addEventListener("scroll", place, true);
    window.addEventListener("resize", place);
    return () => {
      window.removeEventListener("scroll", place, true);
      window.removeEventListener("resize", place);
    };
  }, [anchor]);
  useEffect(() => {
    box.current?.querySelector("button")?.focus();
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    const onDown = (e: MouseEvent) => {
      if (!box.current?.contains(e.target as Node)) onClose();
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
      <span className="link-confirm-note">{rt("linkCheck")}</span>
      <span className="link-confirm-url">{url}</span>
      <span className="link-confirm-actions">
        <button
          type="button"
          className="primary"
          onClick={() => {
            window.open(url, "_blank", "noopener,noreferrer");
            onClose();
          }}
        >
          {rt("open")}
        </button>
        <button type="button" onClick={() => copy(url)}>
          {copied ? rt("copied") : rt("copy")}
        </button>
        <button type="button" onClick={onClose}>
          {rt("cancel")}
        </button>
      </span>
    </span>
  );
}

function decodeSafe(s: string): string {
  try {
    return decodeURIComponent(s);
  } catch {
    return s;
  }
}
