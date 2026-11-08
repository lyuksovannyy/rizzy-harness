// A sidebar row that is a real link: middle-click, a modified click and
// "copy link" work as on any link, and a plain click navigates in place.
import type { MouseEvent, ReactNode } from "react";
import { type Route, routeHash } from "../lib/routes";

/** Whether a click on a link should navigate in place: a plain, primary one. */
export function plainClick(e: MouseEvent): boolean {
  return e.button === 0 && !e.metaKey && !e.ctrlKey && !e.shiftKey && !e.altKey;
}

export function NavLink(props: {
  to: Route;
  /** Marked `aria-current="page"` (the row of what is on screen). */
  current?: boolean;
  onNavigate: (r: Route) => void;
  className?: string;
  /** An accessible name other than the text (an icon-only row, a count). */
  label?: string;
  title?: string;
  role?: string;
  children: ReactNode;
}) {
  return (
    <a
      href={routeHash(props.to)}
      className={`${props.className ?? ""}${props.current ? " active" : ""}`}
      aria-current={props.current ? "page" : undefined}
      aria-label={props.label}
      title={props.title}
      role={props.role}
      onClick={(e) => {
        if (!plainClick(e)) return;
        e.preventDefault();
        props.onNavigate(props.to);
      }}
    >
      {props.children}
    </a>
  );
}
