# W.7

## Covers

The browser scripts pass again

## Change

The first run in a real browser (2026-11-12, `main` at `4961065`, Chromium 158): 4 of 15 pass (`attention`, `backdrop`, `phase7`, `savings`); `artifacts`, `container`, `media`, `phase3` to `phase6`, `phase8`, `phase9`, `rich` and `routines` fail, most on a selector that never appears, `phase4` on "focus is in the search input", `rich` on "the footnote keeps its id".
Each failure is either a script that fell behind the UI (fixed in the script) or a fault in the product (fixed with a test, or listed for the owner).
The login key and the cookie's reach to other ports (R.2) get a script of their own: they have never run in a browser.

## Done when, and size

Every script passes three times in a row, or its failure is a named, filed fault.
The README says how to run them.
**M**

