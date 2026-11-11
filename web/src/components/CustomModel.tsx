// A model the admin adds by hand: one a provider serves but does not list
// (many /models leave out embeddings models), with the parameters it has.
// A test sends it one short request before it is added.
import { CircleCheck, Loader2 } from "lucide-react";
import { useState } from "react";
import { pageApi } from "../apiPages";
import type { ProviderModel } from "../types";

/** What a model test needs besides the model: the provider it goes to. */
export type TestTarget = Omit<Parameters<typeof pageApi.testModel>[0], "model">;

/** "" is not reported; anything else must be a whole number above 0. */
function tokens(s: string): number | null | undefined {
  const t = s.trim();
  if (!t) return null;
  const n = Number(t);
  return Number.isInteger(n) && n > 0 ? n : undefined;
}

type Tested = { ok: true; text: string } | { ok: false; text: string };

export function CustomModelForm(props: {
  /** The model edited; null adds one. */
  initial: ProviderModel | null;
  /** An id already in the list (other than the one edited). */
  taken: (id: string) => boolean;
  /** null: the provider cannot be reached yet (no successful test). */
  test: (() => TestTarget) | null;
  /** Embeddings models have no context, output or abilities to set. */
  embeddings?: boolean;
  onSave: (m: ProviderModel) => void;
  onCancel: () => void;
}) {
  const m = props.initial;
  const [id, setId] = useState(m?.id ?? "");
  const [name, setName] = useState(m?.name ?? "");
  const [context, setContext] = useState(m?.context ? String(m.context) : "");
  const [output, setOutput] = useState(m?.max_output ? String(m.max_output) : "");
  const [effort, setEffort] = useState(!!m?.effort);
  const [vision, setVision] = useState(!!m?.vision);
  const [tools, setTools] = useState(!!m?.tools);
  const [tested, setTested] = useState<{ id: string; r: Tested } | null>(null);
  const [testing, setTesting] = useState(false);

  const raw = id.trim();
  const ctx = tokens(context);
  const out = tokens(output);
  let invalid: string | null = null;
  if (!raw) invalid = "Enter the id the provider uses for it.";
  else if (raw.includes("::")) invalid = "A model id cannot contain ::.";
  else if (props.taken(raw)) invalid = `${raw} is already in the list.`;
  else if (ctx === undefined || out === undefined) invalid = "Context and output are whole numbers of tokens.";

  const run = async () => {
    if (!props.test || !raw) return;
    setTesting(true);
    try {
      const r = await pageApi.testModel({ ...props.test(), model: raw });
      setTested({
        id: raw,
        r: r.ok ? { ok: true, text: `Replied “${r.reply}” in ${r.ms} ms.` } : { ok: false, text: r.error || "The model did not answer." },
      });
    } catch (x) {
      setTested({ id: raw, r: { ok: false, text: x instanceof Error ? x.message : String(x) } });
    } finally {
      setTesting(false);
    }
  };

  const save = () => {
    if (invalid) return;
    props.onSave({
      id: raw,
      name: name.trim() || null,
      context: props.embeddings ? null : (ctx ?? null),
      max_output: props.embeddings ? null : (out ?? null),
      effort: props.embeddings ? null : effort,
      vision: props.embeddings ? null : vision,
      tools: props.embeddings ? null : tools,
      custom: true,
      unavailable: false,
      missing_since: null,
    });
  };

  // A field here never submits the form the picker sits in; Enter on a
  // button still presses it.
  const noSubmit = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && e.target instanceof HTMLInputElement) e.preventDefault();
  };
  const result = tested?.id === raw ? tested.r : null;

  return (
    <div className="custom-model form stack" role="group" aria-label={m ? `Edit ${m.id}` : "Add custom model"} onKeyDown={noSubmit}>
      <div className="row provider-names">
        <label className="grow">
          Model id
          <input className="mono" value={id} autoFocus={!m} autoComplete="off" placeholder={props.embeddings ? "text-embedding-3-small" : "provider/model-name"} onChange={(e) => setId(e.target.value)} />
        </label>
        <label className="grow">
          <span>
            Name <span className="muted optional">(optional)</span>
          </span>
          <input value={name} autoFocus={!!m} autoComplete="off" onChange={(e) => setName(e.target.value)} />
        </label>
      </div>
      {!props.embeddings && (
        <>
          <div className="row provider-names">
            <label className="grow">
              Context window
              <input inputMode="numeric" value={context} placeholder="tokens, e.g. 128000" onChange={(e) => setContext(e.target.value)} />
            </label>
            <label className="grow">
              Max output
              <input inputMode="numeric" value={output} placeholder="tokens, e.g. 16384" onChange={(e) => setOutput(e.target.value)} />
            </label>
          </div>
          <div className="row">
            <label className="check">
              <input type="checkbox" checked={effort} onChange={(e) => setEffort(e.target.checked)} />
              Effort
            </label>
            <label className="check">
              <input type="checkbox" checked={vision} onChange={(e) => setVision(e.target.checked)} />
              Vision
            </label>
            <label className="check">
              <input type="checkbox" checked={tools} onChange={(e) => setTools(e.target.checked)} />
              Tools
            </label>
          </div>
        </>
      )}
      {invalid && (raw || id) && <div className="error-text">{invalid}</div>}
      <div className="row" aria-live="polite">
        <button type="button" onClick={() => void run()} disabled={!props.test || !raw || testing}>
          {testing && <Loader2 size={14} className="spin" />}
          Test
        </button>
        {!props.test && <span className="muted small">Test the connection first.</span>}
        {result?.ok && (
          <span className="small tested-ok">
            <CircleCheck size={14} /> {result.text}
          </span>
        )}
        {result && !result.ok && <span className="error-text small">{result.text}</span>}
        <span className="spacer" />
        <button type="button" onClick={props.onCancel}>
          Cancel
        </button>
        <button type="button" className="primary" onClick={save} disabled={!!invalid}>
          {m ? "Save model" : "Add model"}
        </button>
      </div>
    </div>
  );
}
