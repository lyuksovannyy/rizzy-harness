import { describe, expect, it } from "vitest";
import { countries, countryCode, fold, isTimeZone, rank, suggestCities, suggestCountries, suggestTimeZones, utcOffset } from "./geo";

describe("geo suggestions", () => {
  it("lists real countries only", () => {
    const names = countries().map((c) => c.name);
    expect(names).toContain("Latvia");
    expect(names).toContain("Ukraine");
    for (const bad of ["European Union", "United Nations", "Pseudo-Accents", "Unknown Region"]) {
      expect(names).not.toContain(bad);
    }
    expect(new Set(names).size).toBe(names.length);
    expect(countryCode("latvia")).toBe("LV");
    expect(countryCode("Atlantis")).toBeNull();
  });

  it("ranks prefix, then word start, then anywhere, ignoring accents", () => {
    expect(fold("Zürich")).toBe("zurich");
    const r = rank(["Northern Kyiv", "Kyiv", "Bakyiv"], "kyiv", (x) => x);
    expect(r).toEqual(["Kyiv", "Northern Kyiv", "Bakyiv"]);
    expect(suggestCountries("unit").map((s) => s.value).slice(0, 3)).toEqual(
      expect.arrayContaining(["United Kingdom", "United States"]),
    );
  });

  it("finds time zones by city with spaces, and shows the offset", () => {
    const z = suggestTimeZones("new york").map((s) => s.value);
    expect(z[0]).toBe("America/New_York");
    expect(suggestTimeZones("riga")[0]?.value).toBe("Europe/Riga");
    expect(utcOffset("UTC")).toBe("UTC");
    expect(utcOffset("Asia/Kolkata", new Date("2026-01-01T00:00:00Z"))).toBe("UTC+05:30");
    expect(isTimeZone("Europe/Riga")).toBe(true);
    expect(isTimeZone("Mars/Base")).toBe(false);
  });

  it("filters cities by the chosen country", () => {
    const all: [string, string][] = [
      ["Riga", "LV"],
      ["Rīga Port", "LV"],
      ["Rio de Janeiro", "BR"],
    ];
    expect(suggestCities(all, "ri", "LV").map((s) => s.value)).toEqual(["Riga", "Rīga Port"]);
    const any = suggestCities(all, "ri", null);
    expect(any.map((s) => s.value)).toEqual(["Riga", "Rīga Port", "Rio de Janeiro"]);
    expect(any[2]?.hint).toBe("Brazil");
  });
});
