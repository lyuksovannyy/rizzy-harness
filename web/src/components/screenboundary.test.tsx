// @vitest-environment happy-dom
// A lazy screen whose chunk fails to load (an update removed it, a flaky
// network) or that throws while rendering shows a short message with a
// Reload button in its place; the rest of the page stays as it was. Nothing
// reloads by itself, and the error's own text never reaches the page.
import { act, lazy, Suspense, useState, type ComponentType } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { ScreenBoundary } from "./ScreenBoundary";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLElement;
let root: Root;
beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  // React and the boundary log what they caught; the tests read it below.
  vi.spyOn(console, "error").mockImplementation(() => {});
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});

const goneChunk = () =>
  lazy<ComponentType>(() => Promise.reject(new TypeError("Failed to fetch dynamically imported module: https://h/assets/Settings-SECRET.js")));
const throwing = () =>
  lazy<ComponentType>(() =>
    Promise.resolve({
      default: () => {
        throw new Error("render broke: SECRET_DETAIL");
      },
    }),
  );
const fine = lazy<ComponentType>(() => Promise.resolve({ default: () => <p>the screen</p> }));

/** A sidebar beside the boundary, with state of its own that must survive. */
function Page(props: { Screen: ComponentType; reset?: string; reload: () => void }) {
  const [typed, setTyped] = useState("");
  return (
    <div>
      <aside>
        <input aria-label="draft" value={typed} onChange={(e) => setTyped(e.target.value)} />
        <span data-testid="typed">{typed}</span>
      </aside>
      <ScreenBoundary reset={props.reset} reload={props.reload}>
        <Suspense fallback={<p>Loading…</p>}>
          <props.Screen />
        </Suspense>
      </ScreenBoundary>
    </div>
  );
}

async function draw(Screen: ComponentType, reload: () => void, reset = "a") {
  await act(async () => root.render(<Page Screen={Screen} reset={reset} reload={reload} />));
  await act(async () => {});
}

describe("ScreenBoundary", () => {
  test("a chunk that is gone says the app was updated, with Reload; the rest stays mounted", async () => {
    const reload = vi.fn();
    await draw(fine, reload);
    const aside = host.querySelector("aside")!;
    const input = host.querySelector("input")!;
    // State in the sidebar before the screen fails.
    await act(async () => {
      const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
      set.call(input, "half a message");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await draw(goneChunk(), reload, "b");
    const alert = host.querySelector("[role=alert]")!;
    expect(alert.textContent).toContain("The app may have been updated");
    // The sidebar is the same element, with its state.
    expect(host.querySelector("aside")).toBe(aside);
    expect(host.querySelector("[data-testid=typed]")!.textContent).toBe("half a message");
    // The error's text is logged, never shown.
    expect(host.textContent).not.toContain("SECRET");
    expect(JSON.stringify((console.error as unknown as { mock: { calls: unknown[] } }).mock.calls.map(String))).toContain("SECRET");
    // Nothing reloads by itself; the button does, once per click.
    expect(reload).not.toHaveBeenCalled();
    await act(async () => alert.querySelector("button")!.click());
    expect(reload).toHaveBeenCalledTimes(1);
  });

  test("a screen that throws while rendering is caught, with its own words", async () => {
    const reload = vi.fn();
    await draw(throwing(), reload);
    const alert = host.querySelector("[role=alert]")!;
    expect(alert.textContent).toContain("could not be shown");
    expect(alert.textContent).not.toContain("updated");
    expect(host.textContent).not.toContain("SECRET_DETAIL");
    expect(host.querySelector("aside")).not.toBeNull();
  });

  test("going to another screen clears the message", async () => {
    const reload = vi.fn();
    await draw(goneChunk(), reload, "a");
    expect(host.querySelector("[role=alert]")).not.toBeNull();
    // The same screen again (a re-render) keeps the message.
    await draw(fine, reload, "a");
    expect(host.querySelector("[role=alert]")).not.toBeNull();
    await draw(fine, reload, "b");
    expect(host.querySelector("[role=alert]")).toBeNull();
    expect(host.textContent).toContain("the screen");
  });

  test("never reloads in a loop: a screen that keeps failing only shows the message", async () => {
    const reload = vi.fn();
    const gone = goneChunk();
    for (const reset of ["a", "b", "c", "d"]) await draw(gone, reload, reset);
    expect(host.querySelector("[role=alert]")).not.toBeNull();
    expect(reload).not.toHaveBeenCalled();
  });

  test("without an injected reload it reloads the page (the default)", async () => {
    const reload = vi.fn();
    vi.stubGlobal("location", { ...location, reload });
    await act(async () =>
      root.render(
        <ScreenBoundary>
          <Suspense fallback={null}>{(() => { const G = goneChunk(); return <G />; })()}</Suspense>
        </ScreenBoundary>,
      ),
    );
    await act(async () => {});
    await act(async () => host.querySelector<HTMLButtonElement>("[role=alert] button")!.click());
    expect(reload).toHaveBeenCalledTimes(1);
    vi.unstubAllGlobals();
  });
});
