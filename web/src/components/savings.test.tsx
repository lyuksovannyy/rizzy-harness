// @vitest-environment happy-dom
// Settings → Token savings, graphify card: read-only. Whether graphify may
// run is the node's call (node.toml, read at start), so the card shows the
// two things to do on the node and offers no install button or allow switch.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, test, vi } from "vitest";
import type { GraphifyStatus, TokenSavings } from "../types";

const status: GraphifyStatus = {
  local_node: true,
  allowed: false,
  node_toml: "/srv/harness/node.toml",
  installed: null,
  pinned_version: "0.9.102",
  steps: ["rizzy-harness graphify install", "graphify = true    # add to node.toml, then restart the node"],
};
const savings = {
  prompt_cache: true,
  mask_old_outputs: true,
  keep_tool_outputs: 3,
  filter_shell_output: true,
  bounded_reads: true,
  web_readability: true,
  chat_style: "lite",
  agentic_style: "off",
  repo_map: false,
  repo_map_tokens: 1024,
  outline_tool: false,
  graphify: false,
  handoff_notes: "small_model",
  small_subagents: false,
} satisfies TokenSavings;

const api = {
  savings: vi.fn(() => Promise.resolve(savings)),
  saveSavings: vi.fn((s: TokenSavings) => Promise.resolve(s)),
  graphify: vi.fn(() => Promise.resolve(status)),
};
vi.mock("../api", () => ({ api, HttpError: class extends Error {} }));
vi.mock("../apiPages", () => ({ pageApi: api }));
const { SavingsSettings } = await import("./Savings");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const models = [
  { id: "p::big", name: "big", provider_id: "p", default: true },
  { id: "p::small", name: "small", provider_id: "p", default: false },
] as unknown as import("../types").ModelInfo[];

async function mount() {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(<SavingsSettings models={models} />));
  await act(async () => {});
  return host;
}

describe("graphify card", () => {
  test("shows the node steps, with no install button or allow switch", async () => {
    const host = await mount();
    const card = host.querySelector("[data-testid=graphify]")!;
    expect(card).not.toBeNull();
    expect(card.querySelectorAll("button")).toHaveLength(0);
    expect(card.textContent).not.toMatch(/Allow on the built-in node|Install graphify|Reinstall/);
    // The only switch left is the admin's offer to agents (config.toml).
    const boxes = [...card.querySelectorAll("input[type=checkbox]")];
    expect(boxes).toHaveLength(1);
    expect(boxes[0]!.closest("label")!.textContent).toContain("Offer graph_query");
    const steps = card.querySelector("[data-testid=graphify-steps]") as HTMLDetailsElement;
    expect(steps.open).toBe(true);
    expect(steps.querySelector("pre")!.textContent).toBe(status.steps.join("\n"));
    expect(steps.textContent).toContain("/srv/harness/node.toml");
  });
});

describe("small model card", () => {
  test("one small model, a note writer and the sub-agent switch, each saved", async () => {
    const host = await mount();
    const card = host.querySelector("[data-testid=small-model]")!;
    expect(card).not.toBeNull();
    expect(card.querySelector("[data-testid=small-model-where]")!.textContent).toContain(
      "sent to the small model's provider, which may not be the provider the chat uses",
    );
    const pick = card.querySelector("select[aria-label='Small model']") as HTMLSelectElement;
    expect([...pick.options].map((o) => o.value)).toEqual(["", "p::big", "p::small"]);
    await act(async () => {
      pick.value = "p::small";
      pick.dispatchEvent(new Event("change", { bubbles: true }));
    });
    expect(api.saveSavings).toHaveBeenLastCalledWith(expect.objectContaining({ small_model: "p::small" }));
    const writer = card.querySelector("select[aria-label='Hand-off notes']") as HTMLSelectElement;
    expect(writer.value).toBe("small_model");
    await act(async () => {
      writer.value = "off";
      writer.dispatchEvent(new Event("change", { bubbles: true }));
    });
    expect(api.saveSavings).toHaveBeenLastCalledWith(expect.objectContaining({ handoff_notes: "off" }));
    const sub = card.querySelector("input[type=checkbox]") as HTMLInputElement;
    expect(sub.checked).toBe(false);
    await act(async () => sub.click());
    expect(api.saveSavings).toHaveBeenLastCalledWith(expect.objectContaining({ small_subagents: true }));
  });
});
