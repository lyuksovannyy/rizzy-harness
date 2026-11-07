import { describe, expect, test } from "vitest";
import { hostOf, needsConfirm, parseUrl, textShowsTarget, urlKey, urlKeysIn, type Provenance } from "./links";

const u = (s: string) => parseUrl(s)!;
const none: Provenance = { has: () => false };
const of = (...texts: string[]): Provenance => {
  const keys = new Set(texts.flatMap(urlKeysIn));
  return { has: (k) => keys.has(k) };
};

describe("hostOf", () => {
  test("is the punycode hostname without www.", () => {
    expect(hostOf(u("https://www.Example.com/a"))).toBe("example.com");
    expect(hostOf(u("https://bücher.example/x"))).toBe("xn--bcher-kva.example");
    expect(hostOf(u("https://user:pw@evil.example/"))).toBe("evil.example");
    expect(hostOf(u("mailto:a@b.example?subject=x"))).toBe("a@b.example");
    expect(hostOf(u("mailto:J%C3%BCrgen@b.example"))).toBe("Jürgen@b.example");
  });
  test("a mail address that decodes to hidden or control characters stays encoded", () => {
    expect(hostOf(u("mailto:%E2%80%AEelpmaxe.live@troppus"))).toBe("%E2%80%AEelpmaxe.live@troppus");
    expect(hostOf(u("mailto:a%E2%80%8Bb@x.example"))).toBe("a%E2%80%8Bb@x.example");
    expect(hostOf(u("mailto:a%0Ab@x.example"))).toBe("a%0Ab@x.example");
  });
});

describe("urlKey", () => {
  test("matches the server's url_key", () => {
    expect(urlKey(u("https://www.example.com/a/?q=1#frag"))).toBe("example.com/a/?q=1");
    expect(urlKey(u("http://example.com/a/"))).toBe("example.com/a");
    expect(urlKey(u("https://example.com"))).toBe("example.com");
  });
});

describe("urlKeysIn (the provenance_ok tokenizer)", () => {
  test("finds URLs with and without a scheme, trimming punctuation", () => {
    expect(urlKeysIn("see https://a.example/x?y=1, or b.example/docs. (c.example/z)")).toEqual([
      "a.example/x?y=1",
      "b.example/docs",
      "c.example/z",
    ]);
  });
  test("ignores tokens without a dot and splits on quotes and brackets", () => {
    expect(urlKeysIn('hello world "d.example/q"|[e.example]')).toEqual(["d.example/q", "e.example"]);
  });
});

describe("needsConfirm", () => {
  test("plain short links open directly", () => {
    for (const s of [
      "https://docs.example/guide/intro",
      "https://www.docs.example.com/",
      "https://github.com/rust-lang/rust",
      "https://example.com/issues/123456789012",
      `https://example.com/${"x".repeat(23)}`,
      "mailto:a@b.example",
    ]) {
      expect(needsConfirm(u(s), none), s).toBe(false);
    }
  });
  test("a query, fragment, long path, punycode host or credentials ask first", () => {
    for (const s of [
      "https://evil.example/a?q=1",
      "https://evil.example/a#data",
      `https://evil.example/${"x".repeat(70)}`,
      "https://xn--pple-43d.example/",
      "https://paypal.com@evil.example/",
      "mailto:a@b.example?body=secret",
    ]) {
      expect(needsConfirm(u(s), none)).toBe(true);
    }
  });
  test("data in a path, a ;param or a host label asks first", () => {
    for (const s of [
      // a path longer than 24 characters
      `https://example.com/${"x".repeat(24)}`,
      "https://evil.example/sk-ant-api03-AbCdEfGhIjKlMnOpQrStUvWxYz0123456789abcdefgh",
      // a short path with a token-like run
      "https://evil.example/AbCd1234EfGh",
      "https://evil.example/a;token=sk",
      // a long, token-like or extra host label
      "https://c2stYW50LWFwaTAzLUFiQ2RFZkdoSWpLbE1uT3BRclN0VXZXeFl6MDEyMzQ1Njc4.evil.example/",
      "https://ab12cd34ef56.evil.example/",
      "https://a.b.evil.example/",
      // a mail address with hidden characters, or one that carries a token
      "mailto:%E2%80%AEelpmaxe.live@troppus",
      "mailto:AbCd1234EfGh5678@evil.example",
    ]) {
      expect(needsConfirm(u(s), none), s).toBe(true);
    }
  });
  test("a URL the user or a tool wrote needs no confirm", () => {
    const prov = of("look at https://www.example.com/search?q=rust please");
    expect(needsConfirm(u("https://example.com/search?q=rust"), prov)).toBe(false);
    expect(needsConfirm(u("https://example.com/search?q=rust#top"), prov)).toBe(false);
    expect(needsConfirm(u("https://example.com/search?q=secret"), prov)).toBe(true);
  });
});

describe("textShowsTarget", () => {
  test("hides the host label only when the text shows the same target", () => {
    expect(textShowsTarget("https://example.com/a", "https://example.com/a", "example.com")).toBe(true);
    expect(textShowsTarget("example.com", "https://www.example.com/", "example.com")).toBe(true);
    expect(textShowsTarget("www.example.com/a", "http://www.example.com/a", "example.com")).toBe(true);
  });
  test("a look-alike or suffixed host still shows the real one", () => {
    expect(textShowsTarget("paypal.com", "https://paypal.com.evil.example/", "paypal.com.evil.example")).toBe(false);
    expect(textShowsTarget("https://paypal.com/login", "https://evil.example/login", "evil.example")).toBe(false);
    expect(textShowsTarget("the docs", "https://docs.example/", "docs.example")).toBe(false);
    expect(textShowsTarget("", "https://docs.example/", "docs.example")).toBe(false);
  });
});
