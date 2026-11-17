// The folder name a node makes for a new project (ADOPTION_PLAN X.26,
// PLAN.md T77), as a preview only: the node makes the real one
// (crates/node/src/projects.rs `slug`), with four random characters after
// it. Same rule: ASCII letters (lower-cased) and digits kept, every other
// run one "-", none at either end, at most 48 characters, "project" when
// nothing is left.
export const SLUG_MAX = 48;

export function projectSlug(name: string): string {
  let out = "";
  let gap = false;
  for (const c of name) {
    if (/^[A-Za-z0-9]$/.test(c)) {
      const dash = gap && out.length > 0;
      if (out.length + (dash ? 1 : 0) + 1 > SLUG_MAX) break;
      if (dash) out += "-";
      gap = false;
      out += c.toLowerCase();
    } else {
      gap = true;
    }
  }
  return out || "project";
}
