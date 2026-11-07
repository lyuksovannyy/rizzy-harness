// The home screen's background layer: it sits behind the column
// (styles.css, "Backdrops") and knows which mode's background to show.
// Each mode's artwork is a lazy chunk that mounts inside it.
import type { Mode } from "../lib/modes";

export function Backdrop(props: { mode: Mode }) {
  return <div className="backdrop" aria-hidden="true" data-mode={props.mode} />;
}
