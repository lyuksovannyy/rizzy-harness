// The undone flags after every event of a chain of undos and put-backs
// (X.42): the same sequences as crates/core/tests/restore_flags.rs, which
// writes the fixture. Restoring a saved id returns the folder to what it was
// just before the restore that saved it, flags included.
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { apply } from "./fold";
import { emptyView } from "./emptyView";
import type { RawEvent } from "./types";

const CASES = JSON.parse(readFileSync(new URL("./__fixtures__/restore_flags.json", import.meta.url), "utf8")) as { name: string; events: RawEvent[]; after: Record<string, string>[] }[];

describe("undone flags", () => {
  for (const c of CASES) {
    it(c.name, () => {
      const v = emptyView();
      c.events.forEach((e, i) => {
        apply(v, e);
        const got = Object.fromEntries(Object.entries(v.checkpoints).filter(([, s]) => s.restored).map(([k, s]) => [k, s.saved ?? ""]));
        expect(got, `after event ${i}`).toEqual(c.after[i]);
      });
    });
  }

  it("remembers only the last hundred saved ids and survives more", () => {
    const v = emptyView();
    let seq = 0;
    const cp = (phase: string, turn: string) => apply(v, { kind: "checkpoint", payload: { turn_id: turn, phase, git_ref: `r${seq}`, files: ["a"] }, seq: ++seq, session_id: "s", ts_ms: 0 } as unknown as RawEvent);
    cp("before", "t1");
    cp("after", "t1");
    cp("before_restore", "R1");
    cp("after", "R1");
    cp("restored", "t1");
    for (let n = 2; n <= 110; n++) {
      cp("before_restore", `R${n}`);
      cp("after", `R${n}`);
      cp("restored", `R${n - 1}`);
    }
    expect(v.checkpoints["t1"]!.restored).toBe(false);
    cp("before_restore", "R111");
    cp("after", "R111");
    cp("restored", "R1"); // no longer remembered: no flag but its own moves
    expect(v.checkpoints["t1"]!.restored).toBe(false);
  });
});
