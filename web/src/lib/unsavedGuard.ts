// What the open preview panel does so its unsaved changes are not lost without
// a question (X.60): another file's Preview (docPreview.ts's `gate`), another
// chat or page (the hash changing: every sidebar row, link, New chat and the
// browser's back and forward end there), and closing the tab. Part of the
// panel's lazy chunk; the first load carries only the gate in docPreview.ts.
import { gate } from "./docPreview";

/** The one question every way of leaving asks. */
export const discardQuestion = (n: number) => `Discard ${n} change${n === 1 ? "" : "s"} that ${n === 1 ? "has" : "have"} not been saved?`;

/**
 * Guard the panel's changes until the returned function is called (call it
 * while there is nothing unsaved: the panel does). `unsaved` counts them,
 * `discard` throws them away once the person agrees.
 */
export function watch(unsaved: () => number, discard: () => void): () => void {
  const ask = () => {
    const n = unsaved();
    if (n === 0) return true;
    if (!window.confirm(discardQuestion(n))) return false;
    discard();
    return true;
  };
  // Capture phase, so it runs before the router's own listener. A "no" puts the
  // address back (replaceState fires no event) and the router never hears of it.
  const hash = (e: HashChangeEvent) => {
    if (ask()) return;
    e.stopImmediatePropagation();
    if (e.oldURL) history.replaceState(history.state, "", e.oldURL);
  };
  const unload = (e: BeforeUnloadEvent) => {
    if (unsaved() > 0) e.preventDefault();
  };
  gate.ask = ask;
  window.addEventListener("hashchange", hash, true);
  window.addEventListener("beforeunload", unload);
  return () => {
    if (gate.ask === ask) gate.ask = () => true;
    window.removeEventListener("hashchange", hash, true);
    window.removeEventListener("beforeunload", unload);
  };
}
