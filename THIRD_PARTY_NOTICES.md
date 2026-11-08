# Third-party notices

## caveman (adapted text)

`crates/agent-native/src/style.rs` contains answer-style rules adapted from
the "caveman" skill (`skills/caveman/SKILL.md`, `skills/ultracave/SKILL.md`)
by Julius Brussee, https://github.com/JuliusBrussee/caveman, licensed under
the Apache License, Version 2.0 (http://www.apache.org/licenses/LICENSE-2.0).
Changes: shortened, reworded, rules merged and renumbered, harness-specific
parts added. "Caveman" is a trademark of its owner; this project is not
affiliated with or endorsed by it.

## Rich messages: bundled in the web UI

These packages were added for rich messages (PLAN.md, "Rich messages") and
are part of `web/dist`, which the binary embeds. Each is under the MIT
licence printed at the end of this section, with the copyright line given
here. Versions are pinned in `web/package.json` and `web/pnpm-lock.yaml`.

| Package | Copyright | Where it runs |
|---|---|---|
| remark-math 6.0.0 (https://github.com/remarkjs/remark-math) | Copyright (c) Junyoung Choi | Markdown math syntax, Conversation chunk |
| micromark-extension-math 3.1.0 | Copyright (c) 2020 Titus Wormer | the same (its HTML half, which imports KaTeX, is not bundled) |
| mdast-util-math 3.0.0 | Copyright (c) 2020 Titus Wormer | the same |
| Temml 0.14.0 (https://github.com/ronkok/Temml), with `Temml-Local.css` | Copyright (c) 2020 Ron Kok | math rendering, lazy `math` chunk |
| refractor 5.0.0 (https://github.com/wooorm/refractor) | Copyright (c) Titus Wormer | syntax highlighting, highlighter worker |
| Prism grammars, as shipped in refractor (https://github.com/PrismJS/prism) | Copyright (c) Lea Verou | one lazy chunk per language |
| hastscript 9.0.1 | Copyright (c) Titus Wormer | highlighter worker (through refractor) |
| hast-util-parse-selector 4.0.0 | Copyright (c) 2016 Titus Wormer | highlighter worker (through hastscript) |

**Temml.woff2**, the font `Temml-Local.css` loads (for `\mathscr` and
primes), is not covered by Temml's MIT licence. It is a modified copy of
KaTeX_Script-Regular: "Copyright (c) 2009-2010 Design Science, Inc.,
Copyright (c) 2014-2018 Khan Academy, with Reserved Font Name KaTeX_Script",
licensed under the SIL Open Font License, Version 1.1
(https://openfontlicense.org), and renamed Temml-Script with "Copyright (c)
2021 Ron Kok" added. Its licence field, however, reads: "This font is made
with the home edition of FontCreator. You may not use this font for
commercial purposes." The OFL requires a modified font to stay under the
OFL, so the two statements conflict. Until that is settled (PLAN.md, open
risk R10), treat the font as OFL-1.1 with a disputed non-commercial note.

MIT licence (for every package in the table above):

> Permission is hereby granted, free of charge, to any person obtaining a
> copy of this software and associated documentation files (the
> "Software"), to deal in the Software without restriction, including
> without limitation the rights to use, copy, modify, merge, publish,
> distribute, sublicense, and/or sell copies of the Software, and to permit
> persons to whom the Software is furnished to do so, subject to the
> following conditions:
>
> The above copyright notice and this permission notice shall be included
> in all copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
> OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
> MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN
> NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
> DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR
> OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE
> USE OR OTHER DEALINGS IN THE SOFTWARE.

## Rich messages: installed, not shipped

pnpm installs these with the packages above, or for tests. None of them is
in `web/dist`.

- katex 0.16.47 (MIT, Copyright (c) 2013-2020 Khan Academy and other
  contributors) and its CLI's commander 8.3.0 (MIT, Copyright (c) 2011 TJ
  Holowaychuk): dependencies of micromark-extension-math's HTML half, which
  we do not use. `pnpm size` fails if KaTeX reaches any chunk.
- unist-util-remove-position 5.0.0 (MIT, Copyright (c) 2016 Titus Wormer):
  declared by mdast-util-math 3.0.0, which does not import it.
- @types/katex 0.16.8 and @types/prismjs 1.26.6 (MIT, Copyright (c)
  Microsoft Corporation): type definitions.
- Dev only, the DOM for `web/src/md/math.test.ts`: happy-dom 21.0.4 (MIT,
  Copyright (c) 2019 David Ortner), with entities 7.0.1 (BSD-2-Clause,
  Copyright (c) Felix Böhm), whatwg-mimetype 3.0.0 (MIT, Copyright (c)
  Domenic Denicola), ws 8.22.1 (MIT, Copyright (c) 2011 Einar Otto Stangvik,
  Copyright (c) 2013 Arnout Kazemier and contributors), buffer-image-size
  0.6.4 (MIT, Copyright (c) 2017 Aditya Yadav), undici-types 8.9.0 (MIT,
  Copyright (c) Matteo Collina and Undici contributors), and @types/node
  26.6.6, @types/ws 8.18.2, @types/whatwg-mimetype 3.0.2 (MIT, Copyright (c)
  Microsoft Corporation).

Rich messages added no Rust crates: the ui-block scanner, FNV hash, date and
id checks and the secret lexicon are our own code.

## Ideas, no code copied

- Shell output filtering (`crates/tools/src/filter.rs`): ideas from rtk
  (https://github.com/rtk-ai/rtk), whose CLI is Apache-2.0. No code copied.
- Masking only older tool outputs and keeping the cached prefix stable: ideas
  from headroom (https://github.com/headroomlabs-ai/headroom, Apache-2.0) and
  "The Complexity Trap" (JetBrains Research, MIT). No code copied.
- Repository map: the approach of aider's repo map (tree-sitter tags ranked by
  PageRank), reimplemented.
