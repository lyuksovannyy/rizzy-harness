// The preview of a Word or Excel file in the side panel (PLAN.md T99): what
// the panel is asked to show, and the shape of what the server answers. The
// panel itself is a lazy chunk (components/DocPreview.tsx); this file is all
// the first load carries of it. The state is one open target at a time,
// outside React, so no screen has to hold it.
import { useSyncExternalStore } from "react";

/**
 * The file to show: of a chat without a folder, the id its message or its call
 * names (the server keeps a copy); of a chat with a folder, its path in the
 * folder (the node reads it). Exactly one of the two.
 */
export type PreviewTarget = { session: string; name: string; /** A sentence the panel opens with (a copy was just saved). */ note?: string } & ({ fileId: string; path?: undefined } | { path: string; fileId?: undefined });

/** What tells one target from another (the panel restarts on a change). */
export const targetKey = (t: PreviewTarget) => `${t.session}:${t.fileId ?? `path:${t.path}`}`;

/** The route that answers for a target, `sheet` being the place among the visible sheets. */
export function previewUrl(t: PreviewTarget, sheet?: number): string {
  const s = encodeURIComponent(t.session);
  if (t.fileId !== undefined) return `/api/sessions/${s}/files/${encodeURIComponent(t.fileId)}/preview${sheet === undefined ? "" : `?sheet=${sheet}`}`;
  return `/api/sessions/${s}/folder-preview?path=${encodeURIComponent(t.path)}${sheet === undefined ? "" : `&sheet=${sheet}`}`;
}

/** A stretch of text with its own formatting. */
export interface PRun {
  t: string;
  b?: boolean;
  i?: boolean;
  s?: boolean;
}
/** `at`: the place the edit tools take (`p2#00a1`); none: not changeable from the page. */
export type PBlock = { k: "h"; l: number; r: PRun[]; at?: string } | { k: "p"; r: PRun[]; at?: string } | { k: "li"; r: PRun[]; at?: string } | { k: "t"; rows: string[][] };
export interface PCell {
  /** `B7`. */
  a: string;
  /** The shown value; none for a formula (nothing is calculated). */
  v?: string;
  /** The formula, as written, with its `=`. */
  f?: string;
  /** Cut or a stand-in: not changeable from the page. */
  x?: boolean;
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

/** The open panel's question before it is replaced (lib/unsavedGuard.ts sets it while it has unsaved changes). */
export const gate: { ask: (() => Promise<boolean>) | null } = { ask: null };

/** Open the file in the side panel; the control that asked gets focus back on close. */
export function openPreview(t: PreviewTarget, from?: HTMLElement | null) {
  const opened = from ?? (document.activeElement instanceof HTMLElement ? document.activeElement : null);
  const apply = () => {
    opener = opened;
    current = t;
    emit();
  };
  // Changes wait in the panel: ask first, open on a yes.
  if (current && gate.ask && targetKey(t) !== targetKey(current)) void gate.ask().then((ok) => ok && apply());
  else apply();
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
