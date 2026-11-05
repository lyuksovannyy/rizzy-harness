// Settings → Token savings (admin): every method can be switched off.
import { useEffect, useState } from "react";
import { api } from "../api";
import type { GraphifyStatus, Style, TokenSavings } from "../types";

type BoolKey = {
  [K in keyof TokenSavings]: TokenSavings[K] extends boolean ? K : never;
}[keyof TokenSavings];

const METHODS: { key: BoolKey; title: string; detail: string; mode: string }[] = [
  {
    key: "prompt_cache",
    title: "Prompt caching",
    detail:
      "Marks the unchanging start of each request (instructions, tools, older turns) so the provider bills re-reads at a fraction. Anthropic needs the markers; OpenAI caches by itself.",
    mode: "chat + agentic",
  },
  {
    key: "mask_old_outputs",
    title: "Hide old tool outputs",
    detail:
      "Replaces tool outputs older than the newest few with a one-line note (the model can run the tool again). Hidden in batches so the cache stays valid.",
    mode: "agentic",
  },
  {
    key: "filter_shell_output",
    title: "Filter command output",
    detail:
      "Strips colours and progress bars, folds repeated lines, shows test failures instead of every passing test. The full output stays on the node (tool_output).",
    mode: "agentic",
  },
  {
    key: "bounded_reads",
    title: "Bounded file reads and searches",
    detail: "Reads come in 400-line windows with a 'more lines' note; grep stops at 60 matches, grouped by file.",
    mode: "agentic",
  },
  {
    key: "outline_tool",
    title: "Outline tool",
    detail: "Lets the model list a file's functions and types with line numbers before reading it (Rust, Python, JS, TS, Go).",
    mode: "agentic",
  },
  {
    key: "repo_map",
    title: "Repository map",
    detail:
      "Adds a ranked map of the repository's main definitions to the instructions. Costs tokens up front, saves exploring; best on medium and large repositories.",
    mode: "agentic",
  },
  {
    key: "web_readability",
    title: "Readable web pages",
    detail: "web_fetch keeps the page's main article as Markdown instead of all its text (menus, footers).",
    mode: "chat + agentic",
  },
];

const STYLES: { v: Style; label: string }[] = [
  { v: "off", label: "Off" },
  { v: "lite", label: "Lite: answer first, no filler" },
  { v: "full", label: "Full: terse fragments" },
  { v: "ultra", label: "Ultra: tersest" },
];

function GraphifyCard({ enabled, onToggle }: { enabled: boolean; onToggle: (v: boolean) => void }) {
  const [g, setG] = useState<GraphifyStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => {
    api.graphify().then(setG).catch((x) => setErr(String(x)));
  }, []);
  const act = async (p: Promise<GraphifyStatus>) => {
    setBusy(true);
    setErr(null);
    try {
      setG(await p);
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="card stack" data-testid="graphify">
      <h3>graphify (optional)</h3>
      <p className="muted small">
        A third-party knowledge graph of the repository (Graphify-Labs/graphify, Apache-2.0, Python) that agents can
        query instead of reading many files. Off by default: it runs another program on the node. Its graph is kept
        in harness's data folder, never in your repository, and it gets no API keys.
      </p>
      {g && (
        <div className="small">
          This machine: {g.installed ? <b>{g.installed}</b> : "not installed"} (harness installs {g.pinned_version}) ·{" "}
          {g.local_node ? (g.allowed ? "allowed for the built-in node" : "not allowed for the built-in node") : "no built-in node"}
        </div>
      )}
      {g?.local_node && (
        <div className="row">
          <button type="button" disabled={busy} onClick={() => void act(api.installGraphify())}>
            {busy ? "Working…" : g.installed ? "Reinstall" : "Install graphify"}
          </button>
          <label className="check">
            <input
              type="checkbox"
              checked={g.allowed}
              disabled={busy || !g.installed}
              onChange={(e) => void act(api.allowGraphify(e.target.checked))}
            />
            Allow on the built-in node (writes node.toml)
          </label>
        </div>
      )}
      <label className="check">
        <input type="checkbox" checked={enabled} onChange={(e) => onToggle(e.target.checked)} />
        Offer graph_query to agents (on nodes that allow it and have it installed)
      </label>
      {g?.install_log && (
        <details open={g.install_ok === false}>
          <summary className="small">{g.install_ok ? "Installed" : "Install failed"}: log</summary>
          <pre className="mono join-cmd">{g.install_log}</pre>
        </details>
      )}
      <details>
        <summary className="small">Other nodes</summary>
        <p className="small muted">Each node decides for itself. On the node:</p>
        <pre className="mono join-cmd">{g?.remote_setup.join("\n")}</pre>
      </details>
      {err && <div className="error-text">{err}</div>}
    </div>
  );
}

export function SavingsSettings() {
  const [s, setS] = useState<TokenSavings | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => {
    api.savings().then(setS).catch((x) => setErr(String(x)));
  }, []);
  if (!s) return err ? <div className="error-text">{err}</div> : null;
  const save = async (n: TokenSavings) => {
    setS(n);
    setErr(null);
    try {
      setS(await api.saveSavings(n));
      setMsg("Saved. Applies from the next message.");
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    }
  };
  return (
    <div className="stack">
      <div className="card stack">
        <h3>Token savings</h3>
        <p className="muted small">
          Ways to send and receive fewer tokens. Each one can be switched off. Agentic-only methods never apply to chats.
        </p>
        <ul className="savings">
          {METHODS.map((m) => (
            <li key={m.key}>
              <label className="check">
                <input
                  type="checkbox"
                  aria-label={m.title}
                  checked={s[m.key]}
                  onChange={(e) => void save({ ...s, [m.key]: e.target.checked })}
                />
                <b>{m.title}</b> <span className="muted small">({m.mode})</span>
              </label>
              <div className="muted small">{m.detail}</div>
              {m.key === "mask_old_outputs" && s.mask_old_outputs && (
                <label className="small">
                  Keep at least the newest{" "}
                  <input
                    type="number"
                    min={1}
                    max={50}
                    value={s.keep_tool_outputs}
                    onChange={(e) => void save({ ...s, keep_tool_outputs: Math.max(1, Number(e.target.value) || 1) })}
                  />{" "}
                  outputs
                </label>
              )}
              {m.key === "repo_map" && s.repo_map && (
                <label className="small">
                  Size about{" "}
                  <input
                    type="number"
                    min={256}
                    max={8192}
                    step={256}
                    value={s.repo_map_tokens}
                    onChange={(e) => void save({ ...s, repo_map_tokens: Number(e.target.value) || 1024 })}
                  />{" "}
                  tokens
                </label>
              )}
            </li>
          ))}
        </ul>
        <div className="row">
          <label>
            Answer style in chats
            <select
              aria-label="Chat style"
              value={s.chat_style}
              onChange={(e) => void save({ ...s, chat_style: e.target.value as Style })}
            >
              {STYLES.filter((x) => x.v === "off" || x.v === "lite").map((x) => (
                <option key={x.v} value={x.v}>
                  {x.label}
                </option>
              ))}
            </select>
          </label>
          <label>
            Answer style in agentic sessions
            <select
              aria-label="Agentic style"
              value={s.agentic_style}
              onChange={(e) => void save({ ...s, agentic_style: e.target.value as Style })}
            >
              {STYLES.map((x) => (
                <option key={x.v} value={x.v}>
                  {x.label}
                </option>
              ))}
            </select>
          </label>
        </div>
        <p className="muted small">
          Chats get at most Lite: people read those answers, and the terser styles trade clarity for tokens. Full and Ultra
          are adapted from the caveman rules (Apache-2.0); warnings, irreversible steps and questions to you stay in full
          sentences.
        </p>
        {err && <div className="error-text">{err}</div>}
        {msg && <div className="muted small">{msg}</div>}
      </div>
      <GraphifyCard enabled={s.graphify} onToggle={(v) => void save({ ...s, graphify: v })} />
    </div>
  );
}
