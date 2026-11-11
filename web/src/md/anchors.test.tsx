// @vitest-environment happy-dom
// In-page `#` links in model output (AUDIT L14): only a footnote of the
// same message, as the renderer made it, is a working link. Any other
// fragment (an app route, the id of a button or field, another message's
// footnote) is plain text that scrolls, focuses and opens nothing. The
// scheme allow-list and the real-host rule are unchanged.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { Markdown } from "../components/Markdown";
import { MessageCtx, NO_CITES, NONE } from "./context";

const roots: Root[] = [];
let app: HTMLElement;
const scrolled: Element[] = [];
beforeEach(() => {
  document.body.innerHTML = "";
  history.replaceState(null, "", "#/s/chat");
  // Stand-ins for the app around the messages.
  app = document.createElement("div");
  app.innerHTML = '<button id="delete-everything">Delete</button><input id="api-key"><div id="user-content-fn-1">elsewhere</div>';
  document.body.append(app);
  scrolled.length = 0;
  vi.spyOn(Element.prototype, "scrollIntoView").mockImplementation(function (this: Element) {
    scrolled.push(this);
  });
});
afterEach(() => {
  act(() => roots.forEach((r) => r.unmount()));
  roots.length = 0;
  vi.restoreAllMocks();
});

/** One message, mounted the way Conversation mounts each. */
async function message(text: string): Promise<HTMLElement> {
  const host = document.createElement("article");
  document.body.append(host);
  const root = createRoot(host);
  roots.push(root);
  await act(async () =>
    root.render(
      <MessageCtx.Provider value={{ provenance: NONE, sources: NONE, cites: NO_CITES }}>
        <Markdown text={text} done />
      </MessageCtx.Provider>,
    ),
  );
  return host;
}

const click = (el: Element) => {
  const e = new MouseEvent("click", { bubbles: true, cancelable: true });
  act(() => void el.dispatchEvent(e));
  return e;
};
const inPageLinks = (el: Element) => [...el.querySelectorAll<HTMLAnchorElement>('a[href^="#"]')];

describe("footnotes", () => {
  test("a reference scrolls to its note and the note's back link to the reference", async () => {
    const m = await message("Claim[^1].\n\n[^1]: The note.");
    const ref = m.querySelector<HTMLAnchorElement>("sup a")!;
    const note = m.querySelector(`[id="${ref.getAttribute("href")!.slice(1)}"]`)!;
    expect(note.tagName).toBe("LI");
    expect(m.contains(note)).toBe(true);
    expect(click(ref).defaultPrevented).toBe(true);
    expect(scrolled).toEqual([note]);
    const back = m.querySelector<HTMLAnchorElement>('a[aria-label^="Back to reference"]')!;
    click(back);
    expect(scrolled.at(-1)).toBe(ref);
    expect(location.hash).toBe("#/s/chat");
  });

  test("a label outside ASCII still finds its note", async () => {
    const m = await message("Claim[^ü].\n\n[^ü]: The note.");
    click(m.querySelector("sup a")!);
    expect(scrolled.map((e) => e.tagName)).toEqual(["LI"]);
  });

  test("ids do not collide between messages, nor with the app's", async () => {
    const a = await message("One[^1].\n\n[^1]: First.");
    const b = await message("Two[^1].\n\n[^1]: Second.");
    const ids = [...document.querySelectorAll("[id]")].map((e) => e.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const m of [a, b]) {
      for (const link of inPageLinks(m)) {
        const target = document.getElementById(link.getAttribute("href")!.slice(1))!;
        expect(m.contains(target), link.outerHTML).toBe(true);
      }
    }
    click(b.querySelector("sup a")!);
    expect(b.contains(scrolled[0]!)).toBe(true);
    // Each reference is described by its own message's "Footnotes" heading.
    for (const m of [a, b]) {
      const by = m.querySelector("sup a")!.getAttribute("aria-describedby")!;
      expect(m.contains(document.getElementById(by)), by).toBe(true);
    }
  });

  test("a message cannot link to another message's footnote", async () => {
    const a = await message("One[^1].\n\n[^1]: First.");
    const theirs = a.querySelector("sup a")!.getAttribute("href")!;
    const b = await message(`[jump](${theirs}) and [back](#${a.querySelector("sup a")!.id})`);
    expect(inPageLinks(b)).toEqual([]);
    expect(b.textContent).toBe("jump and back");
  });
});

describe("any other fragment is inert text", () => {
  test.each([
    ["an app button", "[press](#delete-everything)"],
    ["a form field", "[type here](#api-key)"],
    ["an app route", "[settings](#/settings/server/users)"],
    ["the default footnote id", "[note](#user-content-fn-1)"],
    ["an encoded id", "[x](#delete%2Deverything)"],
    ["an empty fragment", "[top](#)"],
  ])("%s", async (_, src) => {
    const m = await message(src);
    expect(inPageLinks(m)).toEqual([]);
    expect(m.querySelector("a")).toBeNull();
    click(m.querySelector(".md")!.firstElementChild!.firstElementChild ?? m);
    expect(scrolled).toEqual([]);
    expect(document.activeElement).toBe(document.body);
    expect(location.hash).toBe("#/s/chat");
  });
});

describe("other links are unchanged", () => {
  test.each([
    "[x](javascript:alert(1))",
    "[x](JaVaScRiPt:alert(1))",
    "[x](%20javascript:alert(1))",
    "[x](data:text/html,<script>alert(1)</script>)",
    "[x](DATA:text/html;base64,PHNjcmlwdD4=)",
    "[x](//evil.example/path)",
    "[x](vbscript:msgbox)",
    "<javascript:alert(1)>",
  ])("%s renders no link", async (src) => {
    const m = await message(src);
    expect(m.querySelector("a")).toBeNull();
  });

  test("an https link still opens in a new tab and shows its real host", async () => {
    const m = await message("[docs](https://evil.example/x)");
    const a = m.querySelector("a")!;
    expect(a.getAttribute("href")).toBe("https://evil.example/x");
    expect(a.getAttribute("target")).toBe("_blank");
    expect(m.querySelector(".link-host")!.textContent).toBe("evil.example");
  });

  test("mixed-case https and mailto still work", async () => {
    const m = await message("[a](HTTPS://a.example/) [b](MailTo:x@b.example)");
    expect([...m.querySelectorAll("a")].map((a) => a.getAttribute("href"))).toEqual(["https://a.example/", "mailto:x@b.example"]);
  });
});
