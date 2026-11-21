// The bar's mode classifier (lib/classifier.ts and its rules), loaded as its
// own chunk right after the first paint: nothing needs a guess before a
// person has typed, and until the chunk is in, `predict` answers null, which
// the composer already reads as "no guess" (the mode stays where it is). The
// saved weights that arrive first wait for the code.
import { useCallback, useEffect, useRef, useState } from "react";
import type { ClassifierState } from "./classifier";
import type { Mode } from "./modes";

type Code = typeof import("./classifier");

export function useClassifier(agenticAllowed: boolean, save: (st: ClassifierState) => void) {
  const code = useRef<Code | null>(null);
  const state = useRef<ClassifierState | null>(null);
  // What the server sent before the code was in.
  const saved = useRef<unknown>(undefined);
  const [ready, setReady] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  // What is learned and not yet sent: sent when the hook goes (the shell is
  // remounted by a workspace switch), not dropped with its timer.
  const pending = useRef<(() => void) | null>(null);
  useEffect(() => {
    let live = true;
    void import("./classifier").then((m) => {
      if (!live) return;
      code.current = m;
      state.current = saved.current === undefined ? m.emptyState() : m.parseState(saved.current);
      setReady(true);
    });
    return () => {
      live = false;
      if (timer.current) clearTimeout(timer.current);
      pending.current?.();
      pending.current = null;
    };
  }, []);
  /** The saved state from the server. */
  const load = useCallback((v: unknown) => {
    if (code.current) state.current = code.current.parseState(v);
    else saved.current = v;
  }, []);
  const predict = useCallback(
    (text: string): Mode | null =>
      code.current && state.current ? code.current.decide(state.current, text, agenticAllowed).mode : null,
    // `ready` renews it once the code is in, so the composer asks again.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [agenticAllowed, ready],
  );
  const learn = useCallback(
    (text: string, mode: Mode) => {
      const c = code.current;
      const st = state.current;
      if (!c || !st) return;
      c.record(st, text, mode, agenticAllowed);
      // Only weights and accuracy counters are saved, never the text.
      if (timer.current) clearTimeout(timer.current);
      const send = () => {
        pending.current = null;
        save(c.serialize(st));
      };
      pending.current = send;
      timer.current = setTimeout(send, 1000);
    },
    [agenticAllowed, save],
  );
  return { predict, learn, load };
}
