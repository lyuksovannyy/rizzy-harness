// One screen's error boundary: a lazy chunk that fails to load (an update
// removed it while this tab held the old index, or the network dropped it)
// or a screen that throws while rendering shows a short message and a
// Reload button in its place, not a blank app. The rest of the page (the
// sidebar) stays as it was. It never reloads by itself: an update that is
// still rolling out would reload in a loop. The error's own text is logged,
// never shown (it can name files and hold a node's text).
import { Component, Suspense, type ReactNode } from "react";
import { t } from "../lib/i18n";

// What the browsers say when an `import()` fails (Chromium, Firefox,
// Safari) and what Vite's preload helper says for a chunk's stylesheet.
const LOAD = /dynamically imported module|module script failed|unable to preload/i;

interface Props {
  children: ReactNode;
  /** A new value (the route) clears the message: another screen gets its chance. */
  reset?: unknown;
  /** Injected by tests; the page's own reload otherwise. */
  reload?: () => void;
}

export class ScreenBoundary extends Component<Props, { failed: "" | "load" | "render" }> {
  state = { failed: "" as "" | "load" | "render" };

  static getDerivedStateFromError(e: unknown) {
    return { failed: LOAD.test(String((e as Error)?.message)) ? "load" : "render" };
  }

  componentDidCatch(e: unknown) {
    console.error(e);
  }

  componentDidUpdate(prev: Props) {
    if (this.state.failed && prev.reset !== this.props.reset) this.setState({ failed: "" });
  }

  render() {
    const { failed } = this.state;
    if (!failed) return this.props.children;
    return (
      <div className="pad" role="alert">
        <p>{t(failed === "load" ? "boundary.load" : "boundary.render")}</p>
        <button type="button" onClick={() => (this.props.reload ?? (() => location.reload()))()}>
          {t("boundary.reload")}
        </button>
      </div>
    );
  }
}

/** The shell's screen: "Loading…" while its chunk loads, the boundary around it. */
export function Screen(props: { children: ReactNode; reset: unknown }) {
  return (
    <ScreenBoundary reset={props.reset}>
      <Suspense fallback={<div className="muted pad">Loading…</div>}>{props.children}</Suspense>
    </ScreenBoundary>
  );
}
