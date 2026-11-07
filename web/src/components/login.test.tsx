// @vitest-environment happy-dom
// First-run setup: from the new password, Tab and Enter go on to its
// confirmation, past the show-password button.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, test, vi } from "vitest";

vi.mock("../api", () => ({ api: { setupStatus: vi.fn().mockResolvedValue({ claimed: false }) } }));
const { Login } = await import("./Login");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

async function mount() {
  const host = document.createElement("div");
  document.body.append(host);
  await act(async () => createRoot(host).render(<Login onDone={() => {}} />));
  const code = host.querySelector<HTMLInputElement>('input[aria-label="claim code"]')!;
  act(() => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(code, "ABCDE-FGHJK-MNPQR-STUVW");
    code.dispatchEvent(new Event("input", { bubbles: true }));
  });
  const password = host.querySelector<HTMLInputElement>('input[autocomplete="new-password"]')!;
  const confirm = host.querySelector<HTMLInputElement>('input[placeholder="confirm password"]')!;
  return { password, confirm };
}

const press = (el: HTMLElement, key: string, shiftKey = false) => {
  const e = new KeyboardEvent("keydown", { key, shiftKey, bubbles: true, cancelable: true });
  act(() => void el.dispatchEvent(e));
  return e;
};

describe("setup password keys", () => {
  test.each(["Tab", "Enter"])("%s moves to the confirmation", async (key) => {
    const { password, confirm } = await mount();
    password.focus();
    const e = press(password, key);
    expect(e.defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(confirm);
  });

  test("Shift+Tab is left alone", async () => {
    const { password } = await mount();
    password.focus();
    expect(press(password, "Tab", true).defaultPrevented).toBe(false);
  });
});
