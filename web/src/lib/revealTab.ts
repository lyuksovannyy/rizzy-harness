import { useEffect, useRef } from "react";

/** For a tab row that scrolls sideways (a phone): the chosen tab (the one
 * with `aria-selected`) is brought into view when `chosen` changes, so a tab
 * past the edge is not left half hidden. Put the ref on the row. */
export function useRevealTab(chosen: unknown) {
  const row = useRef<HTMLDivElement>(null);
  useEffect(() => {
    row.current?.querySelector<HTMLElement>('[aria-selected="true"]')?.scrollIntoView?.({ inline: "nearest", block: "nearest" });
  }, [chosen]);
  return row;
}
