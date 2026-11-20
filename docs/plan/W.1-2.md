# W.1 (done)

## Covers

Browser tests runnable here (the owner, 2026-11-12: through `uv`, in the project)

## Change

Node 22 and pnpm were installed on 2026-11-12.
Playwright: `uv venv .venv`, `uv pip install --python .venv/bin/python playwright`, `PLAYWRIGHT_BROWSERS_PATH=$PWD/.venv/ms-playwright .venv/bin/playwright install chromium`. The scripts are Node, and the Python package carries the same Node driver, so `.venv/node_modules/playwright` is a link to `.venv/lib/python*/site-packages/playwright/driver/package`; run a script with `NODE_PATH=$PWD/.venv/node_modules PLAYWRIGHT_BROWSERS_PATH=$PWD/.venv/ms-playwright node web/e2e/<name>.cjs target/debug/rizzy-harness`. Nothing is added to `web/package.json`.

## Done when, and size

Chromium 158 starts and the scripts run.
Still to do: these steps in the README's development section (with W.7).

