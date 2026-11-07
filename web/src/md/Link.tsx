// Links and images in model output. The real host is always visible, a URL
// the model made up that could carry data out asks first, and images are
// never fetched: they become chips that link to the image.
import { Image as ImageIcon } from "lucide-react";
import { useCallback, useContext, useEffect, useRef, useState, type ComponentProps, type ReactNode } from "react";
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
      {u ? ` · ${hostOf(u)}` : ""}
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
      {props.showHost && <span className="link-host">{hostOf(u)}</span>}
      {open && <LinkConfirm url={u.href} onClose={close} />}
    </span>
  );
}

function LinkConfirm({ url, onClose }: { url: string; onClose: () => void }) {
  const box = useRef<HTMLSpanElement>(null);
  const [copied, copy] = useCopy();
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
    <span className="link-confirm" role="dialog" aria-label={rt("linkTitle")} ref={box}>
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
