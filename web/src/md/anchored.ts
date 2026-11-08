import { useLayoutEffect, type RefObject } from "react";

/** Where a popover sits: under its anchor (above it when there is no room
 * below), or to its right, tops aligned as far as the window lets (a
 * submenu beside its item, a menu from the rail's avatar). */
export type Side = "below" | "right";

/**
 * Show a `popover="manual"` element in the top layer next to its anchor,
 * and keep it there on scroll and resize. In place, a scrolling table or
 * any other overflow box would clip it.
 */
export function useAnchored(box: RefObject<HTMLElement | null>, anchor: RefObject<HTMLElement | null>, side: Side = "below") {
  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    el.showPopover?.();
    const place = () => {
      const a = anchor.current?.getBoundingClientRect();
      if (!a) return;
      const gap = 4;
      const margin = 8;
      const fit = (v: number, size: number, room: number) => Math.max(margin, Math.min(v, room - size - margin));
      if (side === "right") {
        el.style.left = `${fit(a.right + gap, el.offsetWidth, window.innerWidth)}px`;
        el.style.top = `${fit(a.top, el.offsetHeight, window.innerHeight)}px`;
        return;
      }
      const below = a.bottom + gap;
      const above = a.top - gap - el.offsetHeight;
      const fits = below + el.offsetHeight <= window.innerHeight - margin || above < margin;
      el.style.left = `${fit(a.left, el.offsetWidth, window.innerWidth)}px`;
      el.style.top = `${fits ? below : above}px`;
    };
    place();
    // Capture: a scroll of any box (the message list, a table) moves the anchor.
    window.addEventListener("scroll", place, true);
    window.addEventListener("resize", place);
    // Content that grows (a menu drilling into a longer list) moves it too.
    const grows = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(place);
    grows?.observe(el);
    return () => {
      window.removeEventListener("scroll", place, true);
      window.removeEventListener("resize", place);
      grows?.disconnect();
    };
  }, [box, anchor, side]);
}
