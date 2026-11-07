import { describe, expect, it } from "vitest";
import { passwordStrength } from "./password";

describe("passwordStrength", () => {
  it("is too short under the server's floor", () => {
    expect(passwordStrength("").score).toBe(0);
    expect(passwordStrength("Ab1!Ab1!x").score).toBe(0);
  });

  it("rates a plain 10-character password weak", () => {
    expect(passwordStrength("lowercases").score).toBe(1);
  });

  it("rewards length and mixed characters", () => {
    expect(passwordStrength("correct horse battery staple").score).toBeGreaterThanOrEqual(3);
    expect(passwordStrength("Tr0ub4dor&3-Kettle-Moss").score).toBe(4);
  });

  it("marks down repeats and runs", () => {
    expect(passwordStrength("aaaaaaaaaaaaaaaa").score).toBe(1);
    expect(passwordStrength("Password1234").score).toBeLessThan(passwordStrength("Pass-word9x7q").score);
  });
});
