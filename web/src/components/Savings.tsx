// Settings → Token savings (admin): every method can be switched off.
import { useEffect, useState } from "react";
import { pageApi } from "../apiPages";
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

/** Read-only: whether graphify may run is the node's call (node.toml, read at
 * start), so the page says what to do on the node instead of doing it. */
function GraphifyCard({ enabled, onToggle }: { enabled: boolean; onToggle: (v: boolean) => void }) {
  const [g, setG] = useState<GraphifyStatus | null>(null);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => {
    pageApi.graphify().then(setG).catch((x) => setErr(String(x)));
  }, []);
  const ready = !!g?.local_node && g.allowed && !!g.installed;
  return (
    <div className="card stack" data-testid="graphify">
      <h3>graphify (optional)</h3>
      <p className="muted small">
        A third-party knowledge graph of the repository (Graphify-Labs/graphify, Apache-2.0, Python) that agents can
        query instead of reading many files. Off by default: it runs another program on the node. Its graph is kept
        in the rizzy-harness data folder, never in your repository, and it gets no API keys.
      </p>
      {g && (
        <div className="small" data-testid="graphify-status">
          This machine: {g.installed ? <b>{g.installed}</b> : "not installed"} (the install command pins{" "}
          {g.pinned_version}) ·{" "}
          {g.local_node ? (g.allowed ? "allowed for the built-in node" : "not allowed for the built-in node") : "no built-in node"}
        </div>
      )}
      <label className="check">
        <input type="checkbox" checked={enabled} onChange={(e) => onToggle(e.target.checked)} />
        Offer graph_query to agents (on nodes that allow it and have it installed)
      </label>
      {g && (
        <details open={!!g.local_node && !ready} data-testid="graphify-steps">
          <summary className="small">{g.local_node ? "Turn it on for a node" : "Turn it on for each node"}</summary>
          <p className="small muted">
            Each node decides for itself; nothing here installs it or changes node.toml. On the node, run the
            install, add the line to {g.node_toml ? <code>{g.node_toml}</code> : "its node.toml"}, then restart the
            node:
          </p>
          <pre className="mono join-cmd">{g.steps.join("\n")}</pre>
        </details>
      )}
      {err && <div className="error-text">{err}</div>}
    </div>
  );
}

export function SavingsSettings() {
  const [s, setS] = useState<TokenSavings | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => {
    pageApi.savings().then(setS).catch((x) => setErr(String(x)));
  }, []);
  if (!s) return err ? <div className="error-text">{err}</div> : null;
  const save = async (n: TokenSavings) => {
    setS(n);
    setErr(null);
    try {
      setS(await pageApi.saveSavings(n));
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
