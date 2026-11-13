# The web UI follows Rizzy Glass

Rizzy Glass is the design system of the owner's rizzy-llmrouter dashboard:
neutral glass surfaces on a flat grey backdrop, one accent from eight
presets, capsule controls, the system font, light and dark. The harness UI
takes its values and rules; it does not take its markup.

## Where the values come from

Every colour, radius, shadow, blur, weight and duration in the first blocks
of `web/src/styles.css` is a value from the system's `tokens.json` (synced
2026-11-12 from the router's `web/src/app.css`), copied exactly. Component
rules (button, field, switch, segmented control, menu, dialog, sidebar)
follow the system's `components/bundle.css`. When the system changes, change
the values there and run `pnpm test`: `theme.test.ts` checks every text
colour against every ground it sits on.

## Names

The harness keeps its own variable names where they mean the same thing
(`--fg`, `--fg-2`, `--bg-2`, `--line`, `--shadow-1`, `--scrim`, ...), so
branches written before this change still resolve. It takes the system's
names for what it lacked (`--fg-3`, `--fill`, `--field`, `--hairline`,
`--glass-*`, `--ok-soft`, `--ok-fill`, ...) and for the accent family, where
the meaning changed:

| Name | Is | Use it for |
|---|---|---|
| `--accent` | the preset's fill | primary buttons, highlighted menu rows, checkboxes |
| `--on-accent` | text on that fill | labels and icons on `--accent` (dark on Orange and Yellow) |
| `--accent-fg` | the preset's text tone | links, accent text, the focus ring |
| `--accent-2` | the preset's graphics tone | sidebar icons, dots, bars |
| `--ac-<preset>`, `-2`, `-fg` | one preset by name | projects, avatars, swatches |

Before this change `--accent` was a text colour and `--accent-fg` was the
text on it: CSS written against the old names reads wrong (blue on blue)
until it moves to the table above.

## Materials and blur

`--bg-2` (cards, tool and approval cards), menus and dialogs are thick
glass; the sidebar is its own material; rows nested in a card are thin
glass. Each edge is the inset highlight and hairline, then the shadow.
Blur (`backdrop-filter`) is only on the sidebar, menus and popovers,
dialogs and the search palette. It is never on the transcript, messages,
tool cards, code blocks or tables: a chat streams for minutes, and every
frame under a backdrop filter is repainted. A card there is the tint
without blur, which over the flat backdrop is one fixed colour.
`theme.test.ts` fails if a blur rule names a transcript class, and
`web/e2e/glass.cjs` checks what the browser computed.

Without `backdrop-filter`, or under `prefers-reduced-transparency: reduce`,
the materials are the system's solids and the dialog backdrop darkens.
Under `prefers-reduced-motion: reduce` everything that moves is instant;
colour fades stay.

## What differs from the system, and why

- **The composer is opaque** (`--solid`, glass's own fallback): the home
  backgrounds move behind it, and text over moving art through glass
  would not keep its contrast.
- **The home backgrounds stay.** The system wants nothing behind glass;
  the home screen's art is a harness feature, behind the opaque composer
  only, and it can be turned off.
- **The narrow layout keeps the drawer** (now from 820px down, the system's
  width): the system's top bar of page capsules has no room for the chat
  list.
- **Reading widths stay:** the chat column is 760px and pages 960px; the
  system's 1240px main column is for dashboards.
- **Message text stays 15px;** the interface is 14px as in the system.
- **Two pairs the system's own values miss** (light theme): secondary text
  on the active sidebar row is 4.47:1, so text there switches to `--fg`;
  the graphics tone as the icon of a hovered or active sidebar row is
  2.8:1 for Blue, so that icon takes the text tone.
- **The dark background art** is capped at 10% ink (it was 12%) to stay
  under 1.35:1 on the lighter backdrop.

## What was not taken

- The `rg-` classes and `bundle.css` itself: the harness keeps its markup.
- The RuneIcons set. `runeicons-react` 0.1.0 has one `RuneIcon({ name })`
  component that looks a name up in a single array of all 357 icons, so one
  icon brings all of them: converting a single sidebar icon took the first
  load from 99.7 KB to 313.5 KB gzip (the ceiling is 104). The harness
  keeps `lucide-react` until a build of RuneIcons with one export per icon
  exists.
- Toasts, tooltips and the command palette's contents: the harness has no
  toasts or custom tooltips yet, and the palette's contents are a later
  stage.
- Sentence-case copy and per-page layout: later stages.
