// The sentence a person reads for a failed request: the server's own text
// when it has one, a plain sentence for a bare 403/422/429/503, and the
// request id at the end when the server sent one.
import { describe, expect, test } from "vitest";
import { HttpError } from "../api";
import { errText } from "./errorText";

const e = (status: number, msg: string, id?: string, retry?: number) => new HttpError(status, msg, undefined, id, retry);

describe("errText", () => {
  test("keeps the server's own sentence (real strings from crates/master)", () => {
    expect(errText(e(403, "change your password first"))).toBe("change your password first");
    expect(errText(e(503, "the node mini that holds this chat's folder is away"))).toBe("the node mini that holds this chat's folder is away");
    expect(errText(e(429, "too many unknown usernames; try again in 15 minutes", undefined, 900))).toBe(
      "too many unknown usernames; try again in 15 minutes",
    );
  });
  test("a bare 403 (the guard's plain-text refusal) says who decides", () => {
    expect(errText(e(403, "403 Forbidden"))).toMatch(/not allowed.*whoever runs the server/i);
  });
  test("a bare 422 says the input was not usable", () => {
    expect(errText(e(422, "422 Unprocessable Entity"))).toMatch(/could not use what was sent/i);
  });
  test("a bare 429 names the wait when Retry-After came", () => {
    expect(errText(e(429, "429 Too Many Requests", undefined, 5))).toBe("Too many requests. Try again in 5 seconds.");
    expect(errText(e(429, "429 Too Many Requests"))).toBe("Too many requests. Try again in a little while.");
  });
  test("a terse 429 text gets the wait added, a sentence that has it does not", () => {
    expect(errText(e(429, "too many icon requests", undefined, 5))).toBe("too many icon requests. Try again in 5 seconds.");
  });
  test("a bare 503 says the server or node is away or busy", () => {
    expect(errText(e(503, "503 Service Unavailable"))).toMatch(/server or the node.*away or busy/i);
  });
  test("other statuses are untouched", () => {
    expect(errText(e(502, "502 Bad Gateway"))).toBe("502 Bad Gateway");
  });
  test("the request id trails the text; none, nothing extra", () => {
    expect(errText(e(409, "taken", "3f2a9c0e11223344"))).toBe("taken (request 3f2a9c0e11223344)");
    expect(errText(e(409, "taken"))).toBe("taken");
    expect(errText(new Error("plain"))).toBe("plain");
    expect(errText("odd")).toBe("odd");
  });
});
