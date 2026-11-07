// Copies public/flags/<cc>.svg (country suggestions in the profile form)
// from flag-icons (MIT), only for the codes geo.ts lists. Static files, so
// they stay out of the JS bundle. Run after updating flag-icons (Node 23.6+,
// which loads geo.ts as is): node scripts/flags.mjs
import { copyFileSync, existsSync, mkdirSync, rmSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { countries } from "../src/lib/geo.ts";

/** Codes flag-icons files under another name. */
const ALIASES = { AC: "sh-ac", TA: "sh-ta" };
const require = createRequire(import.meta.url);
const pkg = dirname(require.resolve("flag-icons/package.json"));
const out = new URL("../public/flags/", import.meta.url).pathname;
rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });
const missing = [];
let n = 0;
for (const { code } of countries()) {
  const src = join(pkg, "flags/4x3", `${ALIASES[code] ?? code.toLowerCase()}.svg`);
  if (existsSync(src)) {
    copyFileSync(src, join(out, `${code.toLowerCase()}.svg`));
    n++;
  } else missing.push(code);
}
copyFileSync(join(pkg, "LICENSE"), join(out, "LICENSE"));
console.log(`${n} flags; no flag (the code shows instead): ${missing.join(" ") || "none"}`);
