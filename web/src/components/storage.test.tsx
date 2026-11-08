// @vitest-environment happy-dom
// Settings → Server → Storage: the four limits, saved as a whole, and each
// account's usage as numbers (never names or contents of files).
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { AdminStorage, StorageLimits } from "../lib/media";
import type { Me } from "../types";

vi.mock("../api", () => ({ call: vi.fn(), HttpError: class extends Error {} }));
const storageApi = {
  mine: vi.fn(),
  admin: vi.fn<() => Promise<AdminStorage>>(),
  save: vi.fn<(s: StorageLimits) => Promise<{ settings: StorageLimits }>>(),
};
vi.mock("../lib/media", async (original) => ({ ...(await original<typeof import("../lib/media")>()), storageApi }));
const { StorageSettings } = await import("./StorageSettings");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const admin: Me = { user_id: "u1", username: "root", role: "admin", can_agentic: true, must_change_password: false };
const SETTINGS: StorageLimits = { max_file_mb: 200, account_quota_mb: 4096, server_quota_mb: 20480, min_free_mb: 1024 };
const GB = 1024 ** 3;

let host: HTMLElement;
let root: Root;
beforeEach(() => {
  storageApi.admin.mockResolvedValue({
    settings: SETTINGS,
    usage: [
      { user_id: "u1", username: "alex", used: 1.5 * GB, media_items: 12, artifact_versions: 3 },
      { user_id: "u2", username: "bob", used: 2048, media_items: 1, artifact_versions: 0 },
    ],
    total: 1.5 * GB + 2048,
    disk: { free: 50 * GB, total: 200 * GB },
  });
  storageApi.save.mockImplementation((s) => Promise.resolve({ settings: s }));
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

describe("storage settings", () => {
  test("usage per account, and the limits saved together", async () => {
    await act(async () => root.render(<StorageSettings me={admin} />));
    const rows = [...host.querySelectorAll(".storage-usage tbody tr")].map((r) => [...r.querySelectorAll("td")].map((d) => d.textContent));
    expect(rows).toEqual([
      ["alex", "1.5 GB", "12", "3"],
      ["bob", "2.0 KB", "1", "0"],
    ]);
    expect(host.textContent).toContain("The data disk has 50 GB free of 200 GB (75% used).");
    const save = [...host.querySelectorAll("button")].find((b) => b.textContent === "Save")!;
    expect(save.disabled).toBe(true);
    const quota = [...host.querySelectorAll("label")].find((l) => l.textContent!.startsWith("Per account"))!.querySelector("input")!;
    act(() => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(quota, "8192");
      quota.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () => save.click());
    expect(storageApi.save).toHaveBeenCalledWith({ ...SETTINGS, account_quota_mb: 8192 });
    expect(host.querySelector('[role="status"]')!.textContent).toBe("Saved.");
  });

  test("a refusal shows", async () => {
    storageApi.admin.mockRejectedValue(new Error("not an admin"));
    await act(async () => root.render(<StorageSettings me={admin} />));
    expect(host.querySelector(".error-text")!.textContent).toBe("not an admin");
  });
});
