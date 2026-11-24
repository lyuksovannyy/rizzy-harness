// What the open preview panel does so its unsaved changes are not lost without
// a question (X.60): every way of leaving asks the app's own dialog, once —
// closing the panel, another sheet, another file's Preview (`gate` in
// docPreview.ts), another chat or page (the hash changing: every sidebar row,
// link, New chat and the browser's back and forward end there). Closing or
// reloading the tab keeps the browser's own warning (a page cannot draw a
// dialog there). Part of the panel's lazy chunk.
import { gate } from "./docPreview";

const sentence = (n: number) => `Discard ${n} change${n === 1 ? "" : "s"} that ${n === 1 ? "has" : "have"} not been saved?`;

let asking: Promise<boolean> | null = null;

/**
 * The dialog: "Keep editing" has the first focus; Escape, the backdrop and
 * "Keep editing" answer false, "Discard changes" true; focus goes back to
 * where it was. One at a time (a second ask gets the first's answer).
 */
export function askDiscard(n: number): Promise<boolean> {
  if (asking) return asking;
  const from = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  const d = document.createElement("dialog");
  d.className = "host-warning";
  d.setAttribute("aria-labelledby", "unsaved-title");
  d.setAttribute("aria-describedby", "unsaved-text");
  const box = document.createElement("div");
  box.className = "stack";
  const h = document.createElement("h2");
  h.id = "unsaved-title";
  h.textContent = sentence(n);
  const p = document.createElement("p");
  p.id = "unsaved-text";
  p.textContent = "Your edits to the preview are lost unless you save a copy first.";
  const row = document.createElement("div");
  row.className = "row end";
  const keep = document.createElement("button");
  keep.type = "button";
  keep.textContent = "Keep editing";
  const drop = document.createElement("button");
  drop.type = "button";
  drop.className = "danger-fill";
  drop.textContent = "Discard changes";
  row.append(keep, drop);
  box.append(h, p, row);
  d.append(box);
  document.body.append(d);
  const done = new Promise<boolean>((resolve) => {
    const end = (v: boolean) => {
      d.close?.();
      d.remove();
      asking = null;
      if (from?.isConnected) from.focus();
      resolve(v);
    };
    keep.onclick = () => end(false);
    drop.onclick = () => end(true);
    d.addEventListener("cancel", (e) => {
      e.preventDefault();
      end(false);
    });
    d.addEventListener("click", (e) => e.target === d && end(false));
  });
  if (typeof d.showModal === "function") d.showModal();
  keep.focus();
  asking = done;
  return done;
}

/**
 * Guard the panel's changes until the returned function is called (the panel
 * does while it has nothing unsaved). `unsaved` counts them, `discard` throws
 * them away once the person agrees.
 */
export function watch(unsaved: () => number, discard: () => void): () => void {
  const ask = async () => {
    const n = unsaved();
    if (n === 0) return true;
    if (!(await askDiscard(n))) return false;
    discard();
    return true;
  };
  // Capture phase, so it runs before the router's own listener, which never
  // hears of a change held here. Keep editing puts the address back
  // (replaceState fires no event); Discard replays the change to the router.
  const hash = (e: HashChangeEvent) => {
    if (unsaved() === 0) return;
    e.stopImmediatePropagation();
    void ask().then((ok) => {
      if (ok) {
        window.dispatchEvent(new HashChangeEvent("hashchange", { oldURL: e.oldURL, newURL: e.newURL }));
      } else if (e.oldURL) {
        history.replaceState(history.state, "", e.oldURL);
      }
    });
  };
  const unload = (e: BeforeUnloadEvent) => {
    if (unsaved() > 0) e.preventDefault();
  };
  gate.ask = ask;
  window.addEventListener("hashchange", hash, true);
  window.addEventListener("beforeunload", unload);
  return () => {
    if (gate.ask === ask) gate.ask = null;
    window.removeEventListener("hashchange", hash, true);
    window.removeEventListener("beforeunload", unload);
  };
}
