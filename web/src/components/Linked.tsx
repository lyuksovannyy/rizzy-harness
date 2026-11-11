// An <img> whose bytes come from this server: it loads through a link
// (lib/links, Gate 3 P-1), since an <img> cannot send the login key.
import type { AnchorHTMLAttributes, ImgHTMLAttributes } from "react";
import { useLink } from "../lib/links";

export function LinkedImg({ url, ...img }: { url: string } & ImgHTMLAttributes<HTMLImageElement>) {
  // Nothing is fetched until the link is there.
  return <img {...img} src={useLink(url)} />;
}

/** A download button: its link is renewed while it stays on screen. */
export function LinkedA({ url, ...a }: { url: string } & AnchorHTMLAttributes<HTMLAnchorElement>) {
  return <a {...a} href={useLink(url, true)} />;
}

export default LinkedImg;
