import { useCallback, useEffect, useRef, useState } from "react";

/** Copy text to the clipboard; `copied` is true for a moment afterwards. */
export function useCopy(): [boolean, (text: string) => void] {
  const [copied, setCopied] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(timer.current), []);
  const copy = useCallback((text: string) => {
    void navigator.clipboard?.writeText(text).then(
      () => {
        setCopied(true);
        window.clearTimeout(timer.current);
        timer.current = window.setTimeout(() => setCopied(false), 1200);
      },
      () => {},
    );
  }, []);
  return [copied, copy];
}
