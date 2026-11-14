// @vitest-environment happy-dom
// Settings, Preferences: the default agent lists each agent with its face,
// or the red alert when its model is gone; the default model offers only
// listed models, keeping a gone current one, marked, so it can be changed.
// And the route: a tab the viewer does not see falls back to one they do,
// and an old path is put right in the address.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { Agent, Me, ModelInfo, Prefs } from "../types";

const saveSettings = vi.fn();
vi.mock("../apiPages", () => ({ pageApi: { saveSettings } }));

const { default: Settings } = await import("./Settings");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLElement;
let root: Root;
beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const model = (raw: string, unavailable = false): ModelInfo => ({
  id: `openai::${raw}`,
  raw,
  provider: "OpenAI",
  provider_id: "openai",
  name: null,
  context: null,
  max_output: null,
  effort: false,
  vision: false,
  tools: false,
  default: false,
  unavailable,
});
const agent = (id: string, name: string, more: Partial<Agent> = {}): Agent => ({
  id,
  name,
  description: "",
  builtin: true,
  default_model: null,
  main_model: null,
  fallbacks: [],
  unavailable: [],
  avatar: { shape: "circle", eyes: "round", accent: "blue" },
  ...more,
});

const member: Me = { user_id: "u", username: "alex", role: "member", exec_ceiling: "none" as const, must_change_password: false };

function render(prefs: Partial<Prefs>, route: { section?: string; tab?: string } = { section: "account", tab: "preferences" }, me = member) {
  act(() =>
    root.render(
      <Settings
        me={me}
        agents={[agent("general", "General"), agent("coder", "Coder", { main_model: "openai::gpt-4", unavailable: ["openai::gpt-4"] })]}
        models={[model("gpt-4", true), model("gpt-4o")]}
        prefs={{ search_url: "", default_agent: null, default_model: null, ...prefs } as Prefs}
        onPrefs={() => {}}
        onProviderChanged={() => {}}
        route={route}
        memoryVersion={0}
        nodes={[]}
      />,
    ),
  );
}
const modelOptions = () =>
  [...[...host.querySelectorAll("label")].find((l) => l.textContent!.startsWith("Default model"))!.querySelectorAll("option")].map(
    (o) => [o.value, o.disabled, o.textContent],
  );

describe("Preferences", () => {
  test("the default model offers listed models only", () => {
    render({});
    expect(modelOptions()).toEqual([
      ["", false, "Workspace default"],
      ["openai::gpt-4o", false, "gpt-4o"],
    ]);
  });

  test("a gone default stays shown, marked, until it is changed", () => {
    render({ default_model: "openai::gpt-4" });
    expect(modelOptions()).toEqual([
      ["", false, "Workspace default"],
      ["openai::gpt-4", true, "gpt-4 (unavailable)"],
      ["openai::gpt-4o", false, "gpt-4o"],
    ]);
  });

  test("the default agent list marks an agent whose model is gone", () => {
    render({ default_agent: "coder" });
    const button = host.querySelector<HTMLButtonElement>('button[aria-label^="Default agent"]')!;
    expect(button.querySelector(".agent-alert")).not.toBeNull();
    act(() => button.click());
    const options = [...host.querySelectorAll('[role="option"]')];
    expect(options.map((o) => o.querySelector(".option-label")!.textContent)).toEqual(["Workspace default", "General", "Coder"]);
    expect(options[1]!.querySelector("svg.agent-avatar")).not.toBeNull();
    expect(options[2]!.querySelector(".agent-alert")).not.toBeNull();
    expect(options[2]!.textContent).toContain("model unavailable");
  });
});

describe("the route", () => {
  const shown = () => ({
    sections: [...host.querySelectorAll(".settings-sections button")].map((b) => b.textContent),
    tab: host.querySelector('[role="tab"][aria-selected="true"]')?.textContent,
  });

  test("the old paths still open their tab", async () => {
    const { parseRoute } = await import("../lib/routes");
    const r = parseRoute("#/settings/preferences");
    render({}, r.kind === "settings" ? r : {});
    expect(shown().tab).toBe("Preferences");
  });

  test("an old path is put right in the address, also while Settings shows its tab", async () => {
    const { parseRoute } = await import("../lib/routes");
    const go = (hash: string) => {
      history.replaceState(null, "", hash);
      const r = parseRoute(hash);
      render({}, r.kind === "settings" ? r : {});
    };
    go("#/settings/account/profile");
    // From another tab: the tab changes, and so does the address.
    go("#/settings/preferences");
    expect(location.hash).toBe("#/settings/account/preferences");
    expect(shown().tab).toBe("Preferences");
    // To the tab already shown (an old link followed again): no tab changes,
    // yet the address is put right as on a fresh load.
    go("#/settings/preferences");
    expect(location.hash).toBe("#/settings/account/preferences");
    expect(shown().tab).toBe("Preferences");
  });

  test("a member cannot open a server panel by its address", () => {
    render({}, { section: "server", tab: "users" });
    expect(shown().tab).toBe("Profile");
    expect(host.textContent).not.toContain("Users");
  });

  test("a section without a tab opens its first", () => {
    const personal = {
      workspace_id: "w1",
      name: "Personal",
      personal: true,
      role: "owner" as const,
      permissions: ["settings" as const, "members" as const],
      members: 1,
      exec_level: "host" as const,
    };
    render({}, { section: "account" }, { ...member, workspace: personal });
    // Personal has no Workspace admin section, whatever the role holds.
    expect(shown()).toEqual({ sections: ["Account", "Workspace"], tab: "Profile" });
  });
});
