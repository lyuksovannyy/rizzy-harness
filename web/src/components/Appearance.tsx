// Settings, Preferences, Appearance: theme and accent. A pick shows at once
// as a preview; Save keeps it, and leaving the tab without saving goes back
// to what is saved. Every control is type="button": they sit in the form.
import { Monitor, Moon, Sun } from "lucide-react";
import { useEffect, useRef } from "react";
import { applyAppearance, withThemeTransition } from "../lib/appearance";
import { t } from "../lib/i18n";
import type { Prefs } from "../types";
import { ACCENTS, THEMES } from "./forms";

const ICONS = { system: Monitor, light: Sun, dark: Moon } as const;

export function AppearanceFields(props: { prefs: Prefs; saved: Prefs; onChange: (p: Prefs) => void }) {
  const p = props.prefs;
  const saved = useRef(props.saved);
  saved.current = props.saved;
  // A cross-fade runs its change a frame later: by then the tab may be gone,
  // and the preview must not come back over the saved look.
  const shown = useRef(true);
  useEffect(() => {
    shown.current = true;
    return () => {
      shown.current = false;
      applyAppearance(saved.current.theme, saved.current.accent, false);
    };
  }, []);

  const preview = (next: Prefs) => {
    const apply = () => shown.current && applyAppearance(next.theme, next.accent, false);
    if ((next.theme ?? null) !== (p.theme ?? null)) withThemeTransition(apply);
    else apply();
    props.onChange(next);
  };
  const theme = p.theme ?? "system";
  const accent = p.accent ?? "blue";

  return (
    <>
      <h3>{t("settings.appearance")}</h3>
      <div className="form stack">
        <div className="field">
          {t("settings.theme")}
          <div className="seg" role="radiogroup" aria-label="theme">
            {THEMES.map((x) => {
              const Icon = ICONS[x];
              return (
                <button
                  key={x}
                  type="button"
                  role="radio"
                  aria-checked={theme === x}
                  className={theme === x ? "on" : ""}
                  onClick={() => preview({ ...p, theme: x === "system" ? null : x })}
                >
                  <Icon size={14} /> {t(`theme.${x}`)}
                </button>
              );
            })}
          </div>
        </div>
        <div className="field">
          {t("settings.accent")}
          <div className="row swatches" role="radiogroup" aria-label="accent">
            {ACCENTS.map((a) => (
              <button
                key={a}
                type="button"
                role="radio"
                aria-checked={accent === a}
                aria-label={a}
                className={`swatch sw-${a} ${accent === a ? "on" : ""}`}
                onClick={() => preview({ ...p, accent: a === "blue" ? null : a })}
              />
            ))}
          </div>
        </div>
      </div>
    </>
  );
}
