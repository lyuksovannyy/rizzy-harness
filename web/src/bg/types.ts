// What Backdrop.tsx hands every mode's background.
import type { Zones } from "./geom";
import type { Motion } from "./hooks";

export type LayerState = "enter" | "idle" | "exit";

export type BgProps = {
  /** enter: play the entrance; exit: play the exit (Backdrop removes the layer after it). */
  state: LayerState;
  motion: Motion;
  /** The tab is hidden: loops and timers wait. */
  paused: boolean;
  zones: Zones;
  seed: number;
  /** Wait this long (ms) before entering, so the previous layer can start leaving. */
  delay: number;
  /** The entrance has finished. */
  onEntered(): void;
};
