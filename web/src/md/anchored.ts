import { useLayoutEffect, type RefObject } from "react";

/**
 * Show a `popover="manual"` element in the top layer, under its anchor (or
 * above it when there is no room below), and keep it there on scroll and
 * resize. In place, a scrolling table or any other overflow box would clip
 * it.
 */
export function useAnchored(box: RefObject<HTMLElement | null>, anchor: RefObject<HTMLElement | null>) {
  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    el.showPopover?.();
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
    // Capture: a scroll of any box (the message list, a table) moves the anchor.
    window.addEventListener("scroll", place, true);
    window.addEventListener("resize", place);
    return () => {
      window.removeEventListener("scroll", place, true);
      window.removeEventListener("resize", place);
    };
  }, [box, anchor]);
}
