// File names that look like host names (.rs, .md, .py, .sh, .zip and .cfg-like
// ones are real top-level domains) must stay plain text in a coding chat: no
// link, no "real host" beside them. Only a real link keeps its link and host.
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test } from "vitest";
import { Markdown } from "../components/Markdown";
import { MessageCtx, NO_CITES, NONE } from "./context";

const md = (text: string) =>
  renderToStaticMarkup(
    <MessageCtx.Provider value={{ provenance: NONE, sources: NONE, cites: NO_CITES }}>
      <Markdown text={text} done />
    </MessageCtx.Provider>,
  );

const NAMES = ["main.rs", "lib.rs", "README.md", "index.ts", "Cargo.toml", "a.py", "foo.sh", "x.zip", "setup.cfg", "build.gradle", "mod.go", "run.pl", "app.io"];

describe("file names are not links", () => {
  for (const n of NAMES) {
    test(n, () => {
      const cases = [
        `Open ${n} now.`,
        `Open \`${n}\` now.`,
        `- ${n}\n- other ${n}`,
        `see ${n}.`,
        `see ${n}, then ${n}: ok (${n})`,
        `look at src/${n} and ./${n}:42 and ../x/${n}`,
        `**${n}** and _${n}_ and "${n}" and <${n}>`,
        `| file |\n|---|\n| ${n} |`,
      ];
      for (const c of cases) {
        const out = md(c);
        expect(out, c).not.toContain("<a ");
        expect(out, c).not.toContain("link-host");
        expect(out, c).toContain(n);
      }
    });
  }

  test("an address in plain text with a file-like domain is still only what GFM makes of an email", () => {
    // Not a file name: documented, so a change is seen.
    expect(md("mail me@example.com")).toContain("<a ");
  });

  test("a real link keeps its link and shows its real host", () => {
    for (const n of NAMES) {
      const out = md(`[${n}](https://example.org/x)`);
      expect(out, n).toContain('href="https://example.org/x"');
      expect(out, n).toContain("link-host");
      expect(out, n).toContain("example.org");
    }
  });

  test("a scheme or www. still makes a link", () => {
    expect(md("go to https://example.org/main.rs now")).toContain("<a ");
    expect(md("go to www.example.org now")).toContain("<a ");
  });
});
