// The preview of a Word or Excel file in the side panel (PLAN.md T99): what
// the panel is asked to show, and the shape of what the server answers. The
// panel itself is a lazy chunk (components/DocPreview.tsx); this file is all
// the first load carries of it. The state is one open target at a time,
// outside React, so no screen has to hold it.
import { useSyncExternalStore } from "react";

/** A file of a chat without a folder: the id its message or its call names. */
export interface PreviewTarget {
  session: string;
  fileId: string;
  name: string;
}

/** A stretch of text with its own formatting. */
export interface PRun {
  t: string;
  b?: boolean;
  i?: boolean;
  s?: boolean;
}
export type PBlock = { k: "h"; l: number; r: PRun[] } | { k: "p"; r: PRun[] } | { k: "li"; r: PRun[] } | { k: "t"; rows: string[][] };
export interface PCell {
  /** `B7`. */
  a: string;
  /** The shown value; none for a formula (nothing is calculated). */
  v?: string;
  /** The formula, as written, with its `=`. */
  f?: string;
}
export interface PRow {
  n: number;
  c: PCell[];
}
export type PreviewData =
  | { kind: "doc"; blocks: PBlock[]; more: boolean; notes: string[] }
  | { kind: "sheet"; sheets: string[]; /** The place of the shown sheet among `sheets`. */ at: number; rows: PRow[]; more: boolean; notes: string[] };
export interface PreviewAnswer {
  name: string;
  preview: PreviewData;
}

/** Only these names are offered a preview (the server refuses the others). */
export const previewable = (name: string) => /\.(docx|docm|xlsx|xlsm)$/i.test(name);

let current: PreviewTarget | null = null;
let opener: HTMLElement | null = null;
const listeners = new Set<() => void>();
const emit = () => listeners.forEach((l) => l());

/** Open the file in the side panel; the control that asked gets focus back on close. */
export function openPreview(t: PreviewTarget, from?: HTMLElement | null) {
  opener = from ?? (document.activeElement instanceof HTMLElement ? document.activeElement : null);
  current = t;
  emit();
}

export function closePreview() {
  if (!current) return;
  current = null;
  emit();
  const o = opener;
  opener = null;
  if (o?.isConnected) o.focus();
}

/** The file the panel shows now, if any. */
export function usePreview(): PreviewTarget | null {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => current,
    () => null,
  );
}
