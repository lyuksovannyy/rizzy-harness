// Settings, Preferences, Appearance: theme, accent and the home backgrounds.
// A theme or accent pick shows at once as a preview; Save keeps it, and
// leaving the tab without saving goes back to what is saved. Every control
// is type="button": they sit in the form. Each group is one Tab stop, and
// the arrow keys move along it (lib/radios.ts).
import { Monitor, Moon, Sun } from "lucide-react";
import { useEffect, useRef } from "react";
import { applyAppearance, withThemeTransition } from "../lib/appearance";
import { t } from "../lib/i18n";
import { radioKeys, radioTab } from "../lib/radios";
import type { Prefs } from "../types";
import { ACCENTS, BACKDROPS, THEMES } from "./forms";
import { Seg } from "./Seg";

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
  const backdrop = p.backdrop ?? "animated";

  return (
    <>
      <h3>{t("settings.appearance")}</h3>
      <div className="form stack">
        <div className="field">
          {t("settings.theme")}
          <Seg
            label="theme"
            value={theme}
            options={THEMES.map((x) => {
              const Icon = ICONS[x];
              return { value: x, label: <><Icon size={14} /> {t(`theme.${x}`)}</> };
            })}
            onChange={(x) => preview({ ...p, theme: x === "system" ? null : x })}
          />
        </div>
        <div className="field">
          {t("settings.accent")}
          <div className="row swatches" role="radiogroup" aria-label="accent" onKeyDown={radioKeys}>
            {ACCENTS.map((a) => (
              <button
                key={a}
                type="button"
                role="radio"
                aria-checked={accent === a}
                tabIndex={radioTab(accent === a)}
                aria-label={a}
                className={`swatch sw-${a} ${accent === a ? "on" : ""}`}
                onClick={() => preview({ ...p, accent: a === "blue" ? null : a })}
              />
            ))}
          </div>
        </div>
        <div className="field">
          {t("settings.backdrop")}
          <Seg
            label="backgrounds"
            value={backdrop}
            options={BACKDROPS.map((x) => ({ value: x, label: t(`backdrop.${x}`) }))}
            onChange={(x) => props.onChange({ ...p, backdrop: x === "animated" ? null : x })}
          />
          <span className="field-hint">{t("settings.backdropHint")}</span>
        </div>
        <div className="field">
          <label className="check">
            <input
              type="checkbox"
              checked={!!p.backdrop_chats}
              disabled={backdrop === "off"}
              onChange={(e) => props.onChange({ ...p, backdrop_chats: e.target.checked })}
            />
            {t("settings.backdropChats")}
          </label>
          <span className="field-hint">{t("settings.backdropChatsHint")}</span>
        </div>
      </div>
    </>
  );
}
