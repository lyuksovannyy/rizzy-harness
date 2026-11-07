import { describe, expect, it } from "vitest";
import fixture from "../__fixtures__/ui.json";
import type { ChoicesSpec, FormSpec } from "../types";
import { checkReply, fieldError, formValues, initialDraft, isBlank, isDate, num, TOO_LARGE, type Reply, type Values } from "./form";

interface ReplyCase {
  type: "choices" | "form";
  spec: ChoicesSpec | FormSpec;
  reply: Reply;
  ok: boolean;
  error: string | null;
  canonical: string | null;
}

const replies = (fixture as unknown as { replies: ReplyCase[] }).replies;
const reply = (values: Values): Reply => ({ message_id: "m1", block_id: "trip", values });

describe("checkReply", () => {
  it("agrees with Rust on every reply in ui.json", () => {
    expect(replies.length).toBeGreaterThan(30);
    for (const r of replies) {
      // The values name the case when it fails (long ones by their size).
      const values = JSON.stringify(r.reply.values).slice(0, 200);
      expect({ values, error: checkReply(r.type, r.spec, r.reply) }).toEqual({ values, error: r.error });
      expect(r.ok).toBe(r.error === null);
    }
    // The cases these rules were wrong for once.
    const errors = replies.map((r) => r.error);
    expect(errors).toContain(TOO_LARGE);
    expect(errors).toContain("n: must be at most 1000000000000000000000");
  });

  it("treats undefined as absent and null as a wrong value", () => {
    const spec = replies.find((r) => r.type === "form")!.spec as FormSpec;
    expect(checkReply("form", spec, reply({ people: 2, notes: undefined }))).toBeNull();
    expect(checkReply("form", spec, reply({ people: 2, notes: null }))).toBe("notes: expected text");
  });
});

const trip = replies.find((r) => r.type === "form")!.spec as FormSpec;
const field = (id: string) => trip.fields.find((f) => f.id === id)!;

describe("form inputs", () => {
  it("start from the defaults and send only what is filled in", () => {
    const draft = initialDraft(trip);
    expect(draft).toMatchObject({ people: "", flex: false, extras: [], notes: "" });
    expect("key" in draft).toBe(false);
    expect(formValues(trip, draft)).toEqual({ flex: false });
    const filled = { ...draft, people: "3", dest: "lis", extras: ["o1"], notes: "  ", name: "Ana", budget: "abc" };
    expect(formValues(trip, filled)).toEqual({ people: 3, dest: "lis", extras: ["o1"], flex: false, name: "Ana", budget: "abc" });
    // A value for a refused field never leaves the browser.
    expect(formValues(trip, { ...draft, key: "sk-1" })).not.toHaveProperty("key");
  });

  it("name the problem with one field", () => {
    expect(fieldError(field("people"), 13)).toBe("must be at most 12");
    expect(fieldError(field("notes"), "x".repeat(21))).toBe("longer than 20 characters");
    expect(fieldError(field("extras"), ["o1", "o1"])).toBe("an option is picked twice");
    expect(fieldError(field("people"), 3)).toBeNull();
  });

  it("helpers match their Rust twins", () => {
    expect(["2024-02-29", "0001-01-01", "2026-12-31"].every(isDate)).toBe(true);
    expect(["2023-02-29", "0000-01-01", "2026-13-01", "2026-1-01", "\uff12\uff10\uff12\uff16-01-01"].some(isDate)).toBe(false);
    expect([num(3), num(12.0), num(-0), num(0.5)]).toEqual(["3", "12", "0", "0.5"]);
    // f64 Display never writes an exponent.
    expect([num(1e21), num(-1.5e22), num(1e-7), num(-2.5e-8), num(1e16)]).toEqual([
      "1000000000000000000000",
      "-15000000000000000000000",
      "0.0000001",
      "-0.000000025",
      "10000000000000000",
    ]);
    expect(isBlank(" \u3000\u0085")).toBe(true);
    expect(isBlank("\ufeff")).toBe(false);
  });
});
