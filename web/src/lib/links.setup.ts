// Every test file but links.test.ts: what the browser loads by itself goes
// through a link (lib/links, Gate 3 P-1); here a link is the URL with
// `link=t` added, minted at once, so a test sees which URLs are linked.
import { vi } from "vitest";

export const linked = (url: string) => `${url}${url.includes("?") ? "&" : "?"}link=t`;

vi.mock("./links", () => ({
  link: async (url: string) => linked(url),
  useLink: (url: string | null | undefined) => (url ? linked(url) : undefined),
}));
