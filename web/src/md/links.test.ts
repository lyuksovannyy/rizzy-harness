import { describe, expect, test } from "vitest";
import { hostOf, leavesApp, linkKey, needsConfirm, parseUrl, textShowsTarget, urlKey, urlKeysIn, type Provenance } from "./links";

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
  test("a mail domain shows as the punycode host, as for http links", () => {
    // [Google support](mailto:support@gооgle.com), with Cyrillic о.
    expect(hostOf(u("mailto:support@g%D0%BE%D0%BEgle.com"))).toBe("support@xn--ggle-55da.com");
    expect(hostOf(u("https://g%D0%BE%D0%BEgle.com/"))).toBe("xn--ggle-55da.com");
    expect(hostOf(u("mailto:J%C3%BCrgen@B%C3%BCcher.Example"))).toBe("Jürgen@xn--bcher-kva.example");
    // Not a plain host: shown as written, encoded.
    expect(hostOf(u("mailto:a@evil.example/paypal.com"))).toBe("a@evil.example/paypal.com");
    expect(hostOf(u("mailto:a@b%2Fc.example"))).toBe("a@b%2Fc.example");
  });
});

describe("urlKey", () => {
  test("matches the server's url_key", () => {
    expect(urlKey(u("https://www.example.com/a/?q=1#frag"))).toBe("example.com/a/?q=1");
    expect(urlKey(u("http://example.com/a/"))).toBe("example.com/a");
    expect(urlKey(u("https://example.com"))).toBe("example.com");
  });
});

describe("linkKey", () => {
  test("is urlKey plus the fragment", () => {
    expect(linkKey(u("https://www.example.com/a/?q=1#frag"))).toBe("example.com/a/?q=1#frag");
    expect(linkKey(u("https://example.com/a/"))).toBe("example.com/a");
    expect(linkKey(u("https://example.com/a#"))).toBe("example.com/a");
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
  test("a number is not an address (1.5 would parse as 1.0.0.5); a full one is", () => {
    expect(urlKeysIn("people: 1.5, budget 3.75 or 0.0000001, v2.5/x and 93.184.216.34/page")).toEqual([
      "93.184.216.34/page",
    ]);
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
      // a look-alike or punycode mail domain, or one that is not a plain host
      "mailto:support@g%D0%BE%D0%BEgle.com",
      "mailto:support@xn--ggle-55da.com",
      "mailto:a@evil.example/paypal.com",
      "mailto:nobody",
    ]) {
      expect(needsConfirm(u(s), none), s).toBe(true);
    }
  });
  test("a URL the user or a tool wrote needs no confirm", () => {
    const prov = of("look at https://www.example.com/search?q=rust please");
    expect(needsConfirm(u("https://example.com/search?q=rust"), prov)).toBe(false);
    expect(needsConfirm(u("https://example.com/search?q=secret"), prov)).toBe(true);
  });
  test("a fragment counts only as it was seen: the page's scripts can read it", () => {
    const prov = of("see https://example.com/docs#install and https://example.com/a");
    expect(needsConfirm(u("https://example.com/docs#install"), prov)).toBe(false);
    expect(needsConfirm(u("https://example.com/docs"), prov)).toBe(false);
    expect(needsConfirm(u("https://example.com/docs#c2VjcmV0LXRva2Vu"), prov)).toBe(true);
    expect(needsConfirm(u("https://example.com/a#top"), prov)).toBe(true);
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

describe("W-2: another port of the app's own host", () => {
  const here = { protocol: "http:", hostname: "localhost", port: "7777" };

  test("the host shows its port when it is not the scheme's default", () => {
    expect(hostOf(u("http://localhost:8000/"))).toBe("localhost:8000");
    expect(hostOf(u("https://www.example.com:8443/a"))).toBe("example.com:8443");
    expect(hostOf(u("https://example.com:443/a"))).toBe("example.com");
    expect(hostOf(u("http://[::1]:3000/"))).toBe("[::1]:3000");
  });

  test("a link to another port, scheme or loopback name of this machine leaves the app", () => {
    expect(leavesApp(u("http://localhost:8000/"), here)).toBe(true);
    expect(leavesApp(u("https://localhost:7777/"), here)).toBe(true);
    expect(leavesApp(u("http://127.0.0.1:7777/"), here)).toBe(true);
    expect(leavesApp(u("http://[::1]:5173/"), here)).toBe(true);
    expect(leavesApp(u("http://app.localhost/"), here)).toBe(true);
    // The app itself, and other hosts, do not.
    expect(leavesApp(u("http://localhost:7777/#/s/1"), here)).toBe(false);
    expect(leavesApp(u("https://example.com/"), here)).toBe(false);
    expect(leavesApp(u("mailto:a@localhost"), here)).toBe(false);
    const lan = { protocol: "http:", hostname: "192.168.1.5", port: "7777" };
    expect(leavesApp(u("http://192.168.1.5:8080/"), lan)).toBe(true);
    expect(leavesApp(u("http://192.168.1.6:8080/"), lan)).toBe(false);
  });
});
