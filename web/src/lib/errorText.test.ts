// The request id trails an error's text; an error without one shows nothing extra.
import { describe, expect, test } from "vitest";
import { HttpError } from "../api";
import { errText } from "./errorText";

describe("errText", () => {
  test("the request id trails the text; none, nothing extra", () => {
    expect(errText(new HttpError(409, "taken", undefined, "3f2a9c0e11223344"))).toBe("taken (request 3f2a9c0e11223344)");
    expect(errText(new HttpError(409, "taken"))).toBe("taken");
    expect(errText(new Error("plain"))).toBe("plain");
    expect(errText("odd")).toBe("odd");
  });
});
