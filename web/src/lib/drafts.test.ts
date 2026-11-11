// @vitest-environment happy-dom
// Unsent drafts (AUDIT L14): kept per user, gone at sign-out, with the chat,
// and never for an incognito chat; storage that fails never breaks typing.
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";

const session = vi.fn();
class HttpError extends Error {
  constructor(public status: number) {
    super(String(status));
  }
}
vi.mock("../api", () => ({ api: { session: (id: string) => session(id) }, HttpError }));
const { clearDrafts, dropDraft, pruneDrafts, readDraft, setDraftOwner, writeDraft } = await import("./drafts");

const keys = (s: Storage) => Array.from({ length: s.length }, (_, i) => s.key(i)!).sort();

beforeEach(() => {
  localStorage.clear();
  sessionStorage.clear();
  session.mockReset();
  setDraftOwner("alex");
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("drafts", () => {
  test("are kept under the user's id and the chat's", () => {
    writeDraft("s1", "half a thought");
    expect(keys(localStorage)).toEqual(["harness.draft.alex.s1"]);
    expect(readDraft("s1")).toBe("half a thought");
    writeDraft("s1", "");
    expect(keys(localStorage)).toEqual([]);
  });

  test("one user's drafts are not the next user's", () => {
    writeDraft("s1", "alex's secret");
    setDraftOwner("bob");
    expect(readDraft("s1")).toBe("");
  });

  test("sign-out clears every draft, form answer and other trace of the user's, not display preferences", () => {
    writeDraft("s1", "alex's draft");
    setDraftOwner("bob");
    writeDraft("s2", "bob's draft");
    sessionStorage.setItem("harness.ui.m1#b1", '{"selected":["yes"]}');
    localStorage.setItem("harness.folders", '["/home/alex/secret-project"]');
    localStorage.setItem("routine-sample:r1", '{"body":"a test event"}');
    sessionStorage.setItem("harness.pendingImages", '["m1"]');
    localStorage.setItem("harness.sidebarCollapsed", "1");
    localStorage.setItem("harness.theme", "dark");
    sessionStorage.setItem("other.tab", "1");
    clearDrafts();
    expect(keys(localStorage)).toEqual(["harness.sidebarCollapsed", "harness.theme"]);
    expect(keys(sessionStorage)).toEqual(["other.tab"]);
  });

  test("dropping one chat's draft leaves the others", () => {
    writeDraft("s1", "a");
    writeDraft("s2", "b");
    dropDraft("s1");
    expect(keys(localStorage)).toEqual(["harness.draft.alex.s2"]);
  });

  test("storage that throws never breaks typing", () => {
    const boom = () => {
      throw new DOMException("quota", "QuotaExceededError");
    };
    const broken = { getItem: boom, setItem: boom, removeItem: boom, key: boom, get length(): number { return boom(); } };
    vi.stubGlobal("localStorage", broken);
    vi.stubGlobal("sessionStorage", broken);
    expect(() => writeDraft("s1", "text")).not.toThrow();
    expect(readDraft("s1")).toBe("");
    expect(() => dropDraft("s1")).not.toThrow();
    expect(() => clearDrafts()).not.toThrow();
    expect(() => pruneDrafts([])).not.toThrow();
  });
});

describe("pruneDrafts", () => {
  const row = (session_id: string, incognito = false) => ({ session_id, incognito });
  const settle = () => new Promise((r) => setTimeout(r, 0));

  test("drops an incognito chat's draft and keeps listed chats' without asking", async () => {
    writeDraft("s1", "kept");
    writeDraft("s2", "incognito");
    pruneDrafts([row("s1"), row("s2", true)]);
    await settle();
    expect(keys(localStorage)).toEqual(["harness.draft.alex.s1"]);
    expect(session).not.toHaveBeenCalled();
  });

  test("a chat deleted elsewhere loses its draft; one in another workspace keeps it", async () => {
    writeDraft("gone", "a");
    writeDraft("elsewhere", "b");
    writeDraft("offline", "c");
    session.mockImplementation((id: string) =>
      id === "gone"
        ? Promise.reject(new HttpError(404))
        : id === "offline"
          ? Promise.reject(new Error("network"))
          : Promise.resolve({ session_id: id, incognito: false }),
    );
    pruneDrafts([]);
    await settle();
    expect(keys(localStorage)).toEqual(["harness.draft.alex.elsewhere", "harness.draft.alex.offline"]);
  });

  test("an unlisted chat that turns out incognito loses its draft", async () => {
    writeDraft("inc", "a");
    session.mockResolvedValue({ session_id: "inc", incognito: true });
    pruneDrafts([]);
    await settle();
    expect(keys(localStorage)).toEqual([]);
  });
});
