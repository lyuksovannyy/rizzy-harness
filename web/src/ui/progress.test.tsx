// The progress card and snapshot line as static markup.
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test } from "vitest";
import type { ProgressSpec } from "../types";
import Progress from "./Progress";
import type { ProgressSlot } from "./blocks";

const job: ProgressSpec = {
  title: "Migrating config",
  steps: [
    { label: "Read config", status: "done" },
    { label: "Write schema", status: "active", detail: "3 of 7 tables" },
    { label: "Run tests", status: "pending" },
    { label: "Lint", status: "skipped" },
  ],
};
const html = (spec: ProgressSpec, slot?: ProgressSlot) => renderToStaticMarkup(<Progress spec={spec} slot={slot} />);
const text = (h: string) => h.replace(/<[^>]+>/g, "");

describe("Progress", () => {
  test("a live card: title, bar from the finished steps, a spinner on the active step", () => {
    const h = html(job);
    expect(h).toContain('class="ui-progress ui-progress-running"');
    expect(h).toContain('aria-valuenow="50"');
    expect(h).toContain("width:50%");
    expect(h).toContain('class="ui-step ui-step-active"');
    expect(h.match(/class="[^"]*\bspin\b/g)).toHaveLength(1);
    expect(text(h)).toContain("Migrating configIn progress");
    expect(text(h)).toContain("Write schema3 of 7 tables");
    for (const label of ["Done", "In progress", "Pending", "Skipped"]) expect(h).toContain(`aria-label="${label}"`);
  });

  test("after the turn ended, the active step has stopped and nothing spins", () => {
    const h = html(job, { mode: "full", ended: true, reason: "cancelled" });
    expect(h).toContain('class="ui-progress ui-progress-stopped"');
    expect(h).toContain('class="ui-step ui-step-stopped"');
    expect(h).not.toMatch(/\bspin\b/);
    expect(text(h)).toContain("Stopped · turn cancelled");
  });

  test("a turn the node's stop ended says so (A.19)", () => {
    const h = html(job, { mode: "full", ended: true, reason: "stopped" });
    expect(h).toContain('class="ui-progress ui-progress-stopped"');
    expect(text(h)).toContain("Stopped · node stopped");
  });

  test("a block marked done or failed leaves a forgotten active step still, even while the turn runs", () => {
    for (const status of ["done", "failed"] as const) {
      const h = html({ ...job, status });
      expect(h).toContain(`class="ui-progress ui-progress-${status}"`);
      expect(h).toContain('class="ui-step ui-step-stopped"');
      expect(h).not.toMatch(/\bspin\b/);
    }
    expect(html({ ...job, status: "running" })).toMatch(/\bspin\b/);
  });

  test("a turn that simply finished is not named", () => {
    const done: ProgressSpec = { ...job, steps: job.steps.map((s) => ({ ...s, status: "done" })) };
    const h = html(done, { mode: "full", ended: true, reason: "completed" });
    expect(text(h)).toContain("Migrating configDone");
    expect(text(h)).not.toContain("turn");
    expect(h).toContain('aria-valuenow="100"');
  });

  test("the model's value wins over the steps", () => {
    expect(html({ ...job, value: 0.25 })).toContain('aria-valuenow="25"');
  });

  test("a snapshot is one line: icon, title, finished of total, current step", () => {
    const h = html(job, { mode: "snapshot", ended: false });
    expect(h.startsWith('<div class="ui-progress-snapshot ui-progress-running">')).toBe(true);
    expect(text(h)).toBe("Migrating config · 2/4 · Write schema");
    expect(h).not.toMatch(/\bspin\b|ui-steps|ui-bar/);
  });

  test("model text stays text", () => {
    const h = html({ title: "<b>x</b>", steps: [{ label: "<img src=x>", status: "done" }] });
    expect(h).not.toContain("<b>");
    expect(h).not.toContain("<img");
  });
});
