// A project's icon and accent. The icon set itself (projectIconSet.ts) is a
// lazy chunk, so the first load carries only the folder: a project whose
// icon is another one shows the folder, the same size, for the moment the
// chunk takes; the accent is one of the app's accent tokens, so it follows
// the theme.
import { Folder, type LucideIcon } from "lucide-react";
import { useEffect, useState } from "react";

let loaded: Record<string, LucideIcon> | null = null;

/** The CSS colour of an accent name (unknown names fall back to the app accent). */
export function accentColor(accent: string): string {
  return /^[a-z]+$/.test(accent) ? `var(--accent-${accent}, var(--accent))` : "var(--accent)";
}

export function ProjectIcon({ icon, accent, size = 16 }: { icon: string; accent: string; size?: number }) {
  const [set, setSet] = useState(loaded);
  useEffect(() => {
    if (set || icon === "folder") return;
    let live = true;
    void import("./projectIconSet").then((m) => {
      loaded = m.PROJECT_ICONS;
      if (live) setSet(loaded);
    });
    return () => {
      live = false;
    };
  }, [set, icon]);
  const Icon = set?.[icon] ?? Folder;
  return <Icon size={size} className="project-icon" style={{ color: accentColor(accent) }} aria-hidden />;
}
