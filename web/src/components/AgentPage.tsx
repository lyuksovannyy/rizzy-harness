// One agent's page (docs/agent-pages.md): its face, name and handle, the
// summary, its instructions and what it can do, read and changed in one
// scope: the server's, or one workspace's (X.33). An agent made in that
// scope is edited whole. One it did not make (built in, a file, and in a
// workspace the server's) keeps its name, handle, instructions and
// features: only its summary and additional instructions (added after its
// own, and in a workspace after the server's) change, and Duplicate makes
// an editable copy. Looks are server-wide: only the server's scope changes
// them. Without permission the page is read-only.
import { Sparkles, Trash2 } from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";
import { type Scope } from "../api";
import { pageApi } from "../apiPages";
import { NEW_AGENT } from "../lib/agents";
import { rawModel } from "../lib/models";
import type { Agent, AgentDetail, AgentFeatures, NewAgent } from "../types";
import { AgentAvatar, AgentMark, agentAlert } from "./AgentAvatar";
import { AvatarEditor } from "./AvatarEditor";

/** What the list says after the name: where the agent comes from (built in, the server's, the workspace's), and where it can't be used. */
export function agentMarkers(a: Agent): string[] {
  const out: string[] = [];
  if (a.scope === "workspace") out.push("workspace");
  else if (a.origin === "file") out.push("server file");
  else if (a.origin === "custom") out.push("server");
  else out.push("built-in");
  if (a.features?.as_agent === false) out.push("sub-agent only");
  return out;
}

/** Asks first; chats that used the agent need another one chosen before their next message. */
export async function confirmDelete(a: Agent, scope: Scope): Promise<boolean> {
  if (!confirm(`Delete ${a.name}? Chats that use @${a.id} need another agent chosen before their next message.`)) return false;
  await pageApi.deleteAgent(a.id, scope);
  return true;
}

interface Feature {
  /** null: planned, shown switched off. */
  key: keyof AgentFeatures | null;
  label: string;
  hint: string;
}

/** The feature grid: one group per row of the diagram. */
export const FEATURE_GROUPS: { title: string; items: Feature[] }[] = [
  {
    title: "Memory",
    items: [
      { key: "memory", label: "Own memory", hint: "Your facts from Settings → Memory: recalled into its answers, new ones proposed" },
      { key: null, label: "Workspace memory", hint: "Facts the whole workspace shares" },
      { key: "project_memory", label: "Project memory", hint: "In a project's chats: the project's facts recalled, new ones proposed" },
      { key: "recall", label: "Earlier chats and memory search", hint: "Searches your own earlier chats in the workspace and your memory when you refer to past work" },
    ],
  },
  {
    title: "Personalisation",
    items: [
      {
        key: "personalisation",
        label: "Personalisation",
        hint: "Each person's reply style and personal instructions from Settings → Personalisation, in their own chats",
      },
    ],
  },
  {
    title: "Web",
    items: [{ key: "web", label: "Web search", hint: "Searches the web and reads pages" }],
  },
  {
    title: "Available as",
    items: [
      { key: "as_agent", label: "Agent", hint: "In the agent picker and @mentions" },
      { key: "as_subagent", label: "Sub-agent", hint: "Agents that launch sub-agents may hand it tasks" },
    ],
  },
  {
    title: "Tools",
    items: [
      { key: "read_files", label: "Read files", hint: "Reads and searches files on a node" },
      { key: "edit_files", label: "Change files", hint: "Writes and edits files" },
      { key: "commands", label: "Run commands", hint: "Runs shell commands on a node" },
      {
        key: "read_only_commands",
        label: "Read-only commands",
        hint: "Runs commands that can only look (git log, grep, tests) in a read-only shell without network; only for an agent that changes nothing else",
      },
      {
        key: "skills",
        label: "Skills",
        hint: "Loads the skills it may use in this workspace (Plugins); a node's skill folders only with file or command access",
      },
      {
        key: "mcp",
        label: "Connectors",
        hint: "Uses the tools of the connectors (MCP servers) the node runs for the chat that it may use in this workspace (Plugins)",
      },
      {
        key: "board",
        label: "Board",
        hint: "In a project's chats: lists the task board; also adds cards, moves them and appends notes (never deletes or rewrites) where the chat's owner may manage projects, except for a read-only agent, which only lists",
      },
      { key: "delegate", label: "Launch sub-agents", hint: "Hands sub-tasks to the agents available as sub-agents" },
      {
        key: "background_subagents",
        label: "Background sub-agents",
        hint: "With Launch sub-agents: may start one that works on while it goes on or ends its turn, and is called back with its result",
      },
      { key: null, label: "Manage workspace", hint: "Changes workspace settings and members" },
    ],
  },
];

/** A new agent's switches: a helper that remembers and searches the web. */
export const NEW_FEATURES: AgentFeatures = {
  memory: true,
  project_memory: true,
  personalisation: true,
  recall: false,
  board: false,
  web: true,
  read_files: false,
  edit_files: false,
  commands: false,
  read_only_commands: false,
  mcp: false,
  skills: true,
  delegate: false,
  background_subagents: false,
  as_agent: true,
  as_subagent: true,
};

/** A handle from a name: `Code reviewer` → `code-reviewer`. */
export function handleFrom(name: string): string {
  return name
    .normalize("NFKD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 32)
    .replace(/-+$/, "");
}

/** Why `id` can't be a new agent's handle, or null. */
export function handleProblem(id: string, agents: Pick<Agent, "id">[]): string | null {
  if (!id) return "Give it a handle.";
  if (!/^[a-z0-9-]{1,32}$/.test(id)) return "Handles are 1–32 characters: a–z, 0–9 and -.";
  if (id === NEW_AGENT || agents.some((a) => a.id === id)) return `@${id} is taken.`;
  return null;
}

/** A switch that does nothing with the others as they are: read-only commands next to a switch that changes things. */
export function moot(f: AgentFeatures, key: keyof AgentFeatures): boolean {
  return key === "read_only_commands" && (f.edit_files || f.commands || f.mcp);
}

/** Turning a feature off would leave the agent nowhere to be used. */
export function lastPlace(f: AgentFeatures, key: keyof AgentFeatures): boolean {
  return (key === "as_agent" && f[key] && !f.as_subagent) || (key === "as_subagent" && f[key] && !f.as_agent);
}

function FeatureGrid(props: { features: AgentFeatures; disabled: boolean; onChange: (f: AgentFeatures) => void }) {
  return (
    <div className="feature-grid">
      {FEATURE_GROUPS.map((g) => (
        <fieldset key={g.title} className="feature-group">
          <legend>{g.title}</legend>
          {g.items.map((it) => {
            const on = it.key ? props.features[it.key] : false;
            const off = !it.key || props.disabled || lastPlace(props.features, it.key) || moot(props.features, it.key);
            return (
              <label key={it.label} className={`feature ${it.key ? "" : "planned"}`}>
                <input
                  type="checkbox"
                  role="switch"
                  className="switch"
                  checked={on}
                  disabled={off}
                  onChange={(e) => it.key && props.onChange({ ...props.features, [it.key]: e.target.checked })}
                />
                <span className="feature-text">
                  <span className="feature-label">
                    {it.label}
                    {!it.key && <span className="tag">planned</span>}
                  </span>
                  <span className="feature-hint">{it.hint}</span>
                </span>
              </label>
            );
          })}
        </fieldset>
      ))}
    </div>
  );
}

/** What the page edits, as one draft. */
interface Draft {
  id: string;
  name: string;
  summary: string;
  prompt: string;
  instructions: string;
  features: AgentFeatures;
}

function draftOf(d: AgentDetail): Draft {
  return { id: d.id, name: d.name, summary: d.summary, prompt: d.prompt, instructions: d.instructions, features: d.features };
}

const same = (a: Draft, b: Draft) => JSON.stringify(a) === JSON.stringify(b);

/** The models the agent runs on, in order, the ones its provider dropped in red. */
export function AgentModels({ agent: a }: { agent: Agent }) {
  const gone = a.unavailable;
  const main = a.main_model;
  // With no chain of its own it runs on the server default, and its
  // fallbacks are General's: show them after the model they back up.
  const head = a.default_model ?? (a.fallbacks.length ? main : null);
  const chain = head ? [head, ...a.fallbacks] : [];
  return (
    <span className="small agent-models">
      {chain.length === 0 ? (
        <span className={main && gone.includes(main) ? "gone" : "muted"}>
          server default model{main && gone.includes(main) ? ` (${rawModel(main)}, unavailable)` : ""}
        </span>
      ) : (
        chain.map((m, i) => (
          <span key={m} className={gone.includes(m) ? "gone" : "muted"} title={gone.includes(m) ? "No longer available" : undefined}>
            {i > 0 ? " → " : ""}
            {rawModel(m)}
            {gone.includes(m) ? " (unavailable)" : ""}
          </span>
        ))
      )}
    </span>
  );
}

export function AgentPage(props: {
  id: string;
  agents: Agent[];
  editable: boolean;
  /** A new agent's starting point (Duplicate), when `id` is NEW_AGENT. */
  seed: NewAgent | null;
  onOpen: (id: string | null) => void;
  onDuplicate: (d: NewAgent) => void;
  onChanged: () => void;
  /** Where the page reads and, when `editable`, writes: a workspace's agents or the server's ("server"). */
  scope?: Scope | null;
}) {
  const creating = props.id === NEW_AGENT;
  const [detail, setDetail] = useState<AgentDetail | null>(null);
  const [saved, setSaved] = useState<Draft | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  // Until the handle is typed, it follows the name.
  const [handleTyped, setHandleTyped] = useState(false);
  const [looks, setLooks] = useState(false);
  // The look editor closed: focus goes back to the face that opened it.
  const face = useRef<HTMLButtonElement>(null);
  const back = useRef(false);
  const closeLooks = () => {
    back.current = true;
    setLooks(false);
  };
  useEffect(() => {
    if (looks || !back.current) return;
    back.current = false;
    face.current?.focus();
  }, [looks]);
  const [busy, setBusy] = useState<"save" | "summary" | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const ids = useId();

  useEffect(() => {
    if (creating) {
      const s = props.seed;
      const d: Draft = {
        id: s?.id ?? "",
        name: s?.name ?? "",
        summary: s?.summary ?? "",
        prompt: s?.prompt ?? "",
        instructions: "",
        features: s?.features ?? NEW_FEATURES,
      };
      setDraft(d);
      setSaved(d);
      setHandleTyped(!!s);
      return;
    }
    let live = true;
    pageApi
      .agent(props.id, props.scope)
      .then((d) => {
        if (!live) return;
        setDetail(d);
        setDraft(draftOf(d));
        setSaved(draftOf(d));
      })
      .catch((x) => live && setErr(x instanceof Error ? x.message : String(x)));
    return () => {
      live = false;
    };
  }, [creating, props.id, props.seed, props.scope]);

  // The listing has the current look and model; the detail what was loaded.
  const listed = props.agents.find((a) => a.id === props.id) ?? detail;
  if (!draft || !saved) {
    return (
      <section className="agents-page">
        <div className="column wide">
          <BackLink onOpen={props.onOpen} />
          {err ? <div className="error-text">{err}</div> : <div className="muted">Loading…</div>}
        </div>
      </section>
    );
  }
  const locked = !creating && !(detail?.own ?? detail?.origin === "custom");
  // A write always names its scope; without one the page only reads.
  const writeScope = props.scope ?? null;
  const canEdit = props.editable && writeScope !== null;
  const inWorkspace = !!writeScope && writeScope !== "server";
  const dirty = !same(draft, saved);
  const set = (p: Partial<Draft>) => {
    setMsg(null);
    setDraft({ ...draft, ...p });
  };
  const handleError = creating ? handleProblem(draft.id, props.agents) : null;
  const nameError = canEdit && !locked && !draft.name.trim() ? "Give it a name." : null;
  const promptError = canEdit && !locked && !draft.prompt.trim() ? "Write its instructions." : null;

  const save = async () => {
    setBusy("save");
    setErr(null);
    setMsg(null);
    try {
      if (creating) {
        const d = await pageApi.createAgent(
          {
            id: draft.id,
            name: draft.name,
            summary: draft.summary,
            prompt: draft.prompt,
            features: draft.features,
          },
          writeScope,
        );
        props.onChanged();
        props.onOpen(d.id);
        return;
      }
      const d = locked
        ? await pageApi.updateAgent(props.id, { summary: draft.summary, instructions: draft.instructions }, writeScope)
        : await pageApi.updateAgent(
            props.id,
            {
              name: draft.name,
              summary: draft.summary,
              prompt: draft.prompt,
              features: draft.features,
            },
            writeScope,
          );
      setDetail(d);
      setDraft(draftOf(d));
      setSaved(draftOf(d));
      setMsg("Saved.");
      props.onChanged();
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    } finally {
      setBusy(null);
    }
  };

  const writeSummary = async () => {
    setBusy("summary");
    setErr(null);
    try {
      const extra = [detail?.server_instructions ?? "", locked ? draft.instructions : ""].map((t) => t.trim()).filter(Boolean);
      const prompt = [draft.prompt, ...extra].join("\n\n");
      if (!writeScope) return;
      const r = await pageApi.agentSummary({ name: draft.name, prompt }, writeScope);
      set({ summary: r.summary });
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    } finally {
      setBusy(null);
    }
  };

  const remove = async () => {
    if (!listed || !writeScope) return;
    setErr(null);
    try {
      if (await confirmDelete(listed, writeScope)) {
        props.onChanged();
        props.onOpen(null);
      }
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    }
  };

  const duplicate = () => {
    const id = handleFrom(`${draft.id}-copy`);
    props.onDuplicate({
      id: handleProblem(id, props.agents) ? "" : id,
      name: `${draft.name} (copy)`,
      summary: draft.summary || detail?.description_default || "",
      prompt: [draft.prompt, detail?.server_instructions ?? "", draft.instructions].map((t) => t.trim()).filter(Boolean).join("\n\n"),
      features: draft.features,
    });
  };

  return (
    <section className="agents-page">
      <div className="column wide agent-page">
        <BackLink onOpen={props.onOpen} />
        <header className="agent-head">
          {listed && !creating ? (
            // Looks are server-wide (config.toml): changed at the server's scope only.
            canEdit && !inWorkspace ? (
              <button
                ref={face}
                type="button"
                className="agent-face"
                aria-expanded={looks}
                aria-label={`Change how ${listed.name} looks`}
                title="Change the look"
                onClick={() => setLooks((v) => !v)}
              >
                <AgentMark agent={listed} size={48} />
              </button>
            ) : (
              <AgentMark agent={listed} size={48} />
            )
          ) : (
            <span className="agent-face" title="Pick a look after saving">
              <AgentAvatar avatar={{ shape: "circle", eyes: "round", accent: "graphite" }} size={48} />
            </span>
          )}
          <div className="agent-title">
            {canEdit && !locked ? (
              <input
                className="agent-name-input"
                aria-label="Name"
                placeholder="Name"
                maxLength={64}
                value={draft.name}
                onChange={(e) => {
                  const name = e.target.value;
                  set(creating && !handleTyped ? { name, id: handleFrom(name) } : { name });
                }}
              />
            ) : (
              <h1>{draft.name}</h1>
            )}
            {creating ? (
              <label className="agent-handle">
                <span aria-hidden="true">@</span>
                <input
                  aria-label="Handle"
                  placeholder="handle"
                  maxLength={32}
                  value={draft.id}
                  onChange={(e) => {
                    setHandleTyped(true);
                    set({ id: e.target.value.toLowerCase() });
                  }}
                />
              </label>
            ) : (
              <span className="mono muted">
                @{draft.id}
                {listed &&
                  agentMarkers(listed).map((m) => (
                    <span key={m} className="small">
                      {" "}
                      · {m}
                    </span>
                  ))}
              </span>
            )}
          </div>
          {canEdit && locked && (
            <button type="button" onClick={duplicate} title="An editable copy with the same instructions and features">
              Duplicate
            </button>
          )}
          {canEdit && !creating && !locked && (
            <button type="button" className="icon danger-icon" aria-label={`Delete ${draft.name}`} title="Delete" onClick={() => void remove()}>
              <Trash2 size={18} />
            </button>
          )}
        </header>
        {listed?.scope === "workspace" && !creating && (
          <p className="field-hint">Made by this workspace's admins, not the server. Its switches below say what it can do, including whether it searches your earlier chats.</p>
        )}
        {looks && listed && (
          <AvatarEditor agent={listed} onSaved={props.onChanged} onClose={closeLooks} />
        )}
        {creating && handleError && draft.name && <div className="field-hint error-text">{handleError}</div>}
        {nameError && !creating && <div className="field-hint error-text">{nameError}</div>}

        <form
          className="form stack agent-form"
          onSubmit={(e) => {
            e.preventDefault();
            void save();
          }}
        >
          <div className="field">
            <div className="field-row">
              <label htmlFor={`${ids}-summary`} className="field-label">
                Summary
              </label>
              {canEdit && (
                <button
                  type="button"
                  className="link small"
                  disabled={busy !== null || !draft.prompt.trim()}
                  title="The server's default model writes one from the instructions; nothing is saved until you save"
                  onClick={() => void writeSummary()}
                >
                  <Sparkles size={14} /> {busy === "summary" ? "Writing…" : "Write from instructions"}
                </button>
              )}
            </div>
            <textarea
              id={`${ids}-summary`}
              rows={2}
              maxLength={300}
              readOnly={!canEdit}
              placeholder={detail?.description_default || "What this agent is for, in a sentence or two"}
              value={draft.summary}
              onChange={(e) => set({ summary: e.target.value.replace(/\n/g, " ") })}
            />
            {locked && canEdit && <p className="field-hint">Leave it empty for the built-in summary.</p>}
          </div>

          {locked ? (
            <>
              <div className="field">
                <span className="field-label">
                  Instructions{detail?.origin === "file" ? " (from its file)" : detail?.origin === "custom" ? " (the server's)" : " (built in)"}
                </span>
                <pre className="agent-prompt">{draft.prompt}</pre>
              </div>
              {inWorkspace && detail?.server_instructions && (
                <div className="field">
                  <span className="field-label">Additional instructions (the server's)</span>
                  <pre className="agent-prompt">{detail.server_instructions}</pre>
                </div>
              )}
              <div className="field">
                <label htmlFor={`${ids}-extra`} className="field-label">
                  {inWorkspace ? "Additional instructions for this workspace" : "Additional instructions"}
                </label>
                <textarea
                  id={`${ids}-extra`}
                  rows={6}
                  maxLength={8000}
                  readOnly={!canEdit}
                  placeholder={canEdit ? "Added after the instructions above, e.g. “Use pnpm, not npm.”" : "None."}
                  value={draft.instructions}
                  onChange={(e) => set({ instructions: e.target.value })}
                />
              </div>
            </>
          ) : (
            <div className="field">
              <label htmlFor={`${ids}-prompt`} className="field-label">
                Instructions
              </label>
              <textarea
                id={`${ids}-prompt`}
                className="agent-prompt-input"
                rows={14}
                maxLength={20000}
                readOnly={!canEdit}
                placeholder="You are… Tell the agent what it is for, how to work and what to avoid."
                value={draft.prompt}
                onChange={(e) => set({ prompt: e.target.value })}
              />
              {promptError && draft.prompt !== saved.prompt && <p className="field-hint error-text">{promptError}</p>}
            </div>
          )}

          <div className="field">
            <span className="field-label">Features{locked && " (fixed for this agent)"}</span>
            <FeatureGrid features={draft.features} disabled={!canEdit || locked} onChange={(features) => set({ features })} />
            {locked && draft.features.read_only_commands && (
              <p className="field-hint">No tool of this agent changes files, so its commands run in a read-only shell, without network.</p>
            )}
          </div>
          {listed && !creating && (
            <div className="field">
              <span className="field-label">Models</span>
              <AgentModels agent={listed} />
              {agentAlert(listed) && <span className="small error-text">{agentAlert(listed)}</span>}
              <p className="field-hint">Chosen per agent in Settings → AI provider.</p>
            </div>
          )}

          {err && <div className="error-text">{err}</div>}
          {canEdit && (
            <div className="row end agent-save">
              {msg && <span className="muted small">{msg}</span>}
              {(dirty || creating) && (
                <button type="button" onClick={() => (creating ? props.onOpen(null) : (setDraft(saved), setErr(null)))}>
                  {creating ? "Cancel" : "Revert"}
                </button>
              )}
              <button
                type="submit"
                className="primary"
                disabled={busy !== null || (!dirty && !creating) || !!handleError || !!nameError || !!promptError}
              >
                {creating ? "Create agent" : "Save"}
              </button>
            </div>
          )}
        </form>
      </div>
    </section>
  );
}

function BackLink(props: { onOpen: (id: string | null) => void }) {
  return (
    <button type="button" className="link small agent-back" onClick={() => props.onOpen(null)}>
      ← All agents
    </button>
  );
}
