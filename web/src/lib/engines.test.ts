import { describe, expect, it } from "vitest";
import { abilities, allEngines, BUILTIN_ENGINES, engineByUrl, engineMention, engineOrigin, supports, urlOrigin } from "./engines";

describe("search engines", () => {
  it("built-ins carry real templates and their abilities", () => {
    for (const e of BUILTIN_ENGINES) {
      for (const u of [e.text, e.image].filter(Boolean) as string[]) {
        expect(u.startsWith("https://") && u.includes("{q}")).toBe(true);
      }
    }
    const g = BUILTIN_ENGINES.find((e) => e.id === "google")!;
    expect(abilities(g)).toBe("text & image");
    expect(abilities(BUILTIN_ENGINES.find((e) => e.id === "wikipedia")!)).toBe("text");
    expect(abilities(BUILTIN_ENGINES.find((e) => e.id === "unsplash")!)).toBe("image");
  });

  it("custom engines get a handle that never shadows a built-in", () => {
    const all = allEngines([
      { name: "Google", text_url: "https://g.example/?q={q}" },
      { name: "My Pics!", image_url: "https://p.example/?q={q}" },
    ]);
    const mine = all.filter((e) => e.custom);
    expect(mine.map((e) => e.id)).toEqual(["google2", "mypics"]);
    expect(supports(mine[1]!, "image") && !supports(mine[1]!, "text")).toBe(true);
    expect(engineByUrl(all, "https://p.example/?q={q}", "image")?.name).toBe("My Pics!");
  });

  it("a leading @mention picks the engine for one search", () => {
    const all = allEngines([]);
    expect(engineMention("@yandex  red panda", all)).toEqual({ engine: all.find((e) => e.id === "yandex"), query: "red panda" });
    expect(engineMention("@nope cats", all)).toBeNull();
    expect(engineMention("cats @google", all)).toBeNull();
  });

  it("an engine's favicon comes from the origin of its text URL, else its image URL", () => {
    const all = allEngines([
      { name: "Pics", image_url: "https://img.example:8443/s?q={q}" },
      { name: "Half typed", text_url: "https://" },
      { name: "Plain", text_url: "http://search.example/?q={q}" },
    ]);
    const by = (id: string) => all.find((e) => e.id === id)!;
    expect(engineOrigin(by("google"))).toBe("https://www.google.com");
    expect(engineOrigin(by("unsplash"))).toBe("https://unsplash.com");
    expect(engineOrigin(by("pics"))).toBe("https://img.example:8443");
    expect(engineOrigin(by("halftyped"))).toBeNull();
    expect(engineOrigin(by("plain"))).toBeNull();
    expect(urlOrigin("not a url")).toBeNull();
    expect(urlOrigin(null)).toBeNull();
  });
});
