// Keyboard for radiogroups made of buttons, as the WAI-ARIA radio group
// pattern has it: the group is one Tab stop (the checked radio), and the
// arrow keys move to the next enabled radio and pick it, wrapping around.
import type { KeyboardEvent } from "react";

const STEP: Record<string, number> = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 };

/** The group's onKeyDown. */
export function radioKeys(e: KeyboardEvent<HTMLElement>): void {
  const step = STEP[e.key];
  if (!step) return;
  const radios = [...e.currentTarget.querySelectorAll<HTMLElement>('[role="radio"]:not(:disabled)')];
  const at = radios.indexOf(e.target as HTMLElement);
  if (at < 0) return;
  e.preventDefault();
  const next = radios[(at + step + radios.length) % radios.length]!;
  // Click first: a pick may move focus elsewhere (the mode switch hands it
  // to the composer), and the arrow keys must stay on the group.
  next.click();
  next.focus();
}

/** A radio's tabIndex: only the checked one is a Tab stop. */
export const radioTab = (checked: boolean) => (checked ? 0 : -1);
