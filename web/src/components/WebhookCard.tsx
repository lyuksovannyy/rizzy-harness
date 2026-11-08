// A webhook routine's "When" card (docs/routines.md): the URL to call, the
// secret (shown once, after a save or a rotation), a curl line, Rotate, and
// the "Sample event (JSON)" box whose paths feed the preview and the variable
// picker. Part of the Routines chunk. What a call sends is untrusted data, and
// the card says so.
import { useState } from "react";
import { curlExample, localOnly, parseSample, type RoutineTrigger, routinesApi } from "../lib/routines";

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

export function WebhookCard(props: {
  routineId: string | null;
  hook: RoutineTrigger | undefined;
  /** The secret from the last save, when there is one to show. */
  secret: string | null;
  sample: string;
  onSample: (text: string) => void;
}) {
  const { hook } = props;
  const [rotated, setRotated] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const secret = rotated ?? props.secret;
  const url = hook?.url_path ? `${location.origin}${hook.url_path}` : null;
  const parsed = parseSample(props.sample);
  const body = parsed && "value" in parsed ? parsed.value : undefined;

  const rotate = async () => {
    if (!props.routineId || !hook || !confirm("Make a new secret? The current one stops working at once.")) return;
    setError(null);
    try {
      setRotated((await routinesApi.rotate(props.routineId, hook.trigger_id)).secret);
    } catch (e) {
      setError(message(e));
    }
  };

  return (
    <div className="stack routine-hook">
      <p className="muted small">
        Each call of its URL with the secret starts a run. What the call sends fills the prompt's webhook variables; that data is
        untrusted, so memory and web tools stay off in those chats.
      </p>
      {url ? (
        <>
          <div className="field">
            URL
            <code className="routine-code">{url}</code>
            {localOnly(location.hostname) && (
              <span className="field-hint">
                This address works only on this computer. To call it from elsewhere, reach the harness over WireGuard or put a TLS
                proxy in front of it.
              </span>
            )}
          </div>
          <div className="field">
            <span className="row">
              <span className="grow">Secret</span>
              <button type="button" className="link small" onClick={() => void rotate()}>
                Rotate
              </button>
            </span>
            {secret ? (
              <>
                <code className="routine-code">{secret}</code>
                <span className="field-hint">Copy it now: it isn't shown again. Send it as an Authorization: Bearer header.</span>
              </>
            ) : (
              <span className="field-hint">A secret is set. Rotate it to see a new one.</span>
            )}
          </div>
          <div className="field">
            Try it
            <pre className="routine-code">{curlExample(url, secret, body)}</pre>
          </div>
        </>
      ) : (
        <p className="muted small">The URL and its secret appear once you save.</p>
      )}
      {error && (
        <div className="error-text small" role="alert">
          {error}
        </div>
      )}
      <label>
        Sample event (JSON)
        <textarea
          className="routine-prompt"
          rows={4}
          value={props.sample}
          placeholder={'{"issue": {"title": "Login fails", "number": 42}}'}
          onChange={(e) => props.onSample(e.target.value)}
        />
        <span className={`field-hint${parsed && "error" in parsed ? " error-text" : ""}`}>
          {parsed && "error" in parsed
            ? parsed.error
            : "A body like the one the caller sends. Its fields appear in the variables and the preview; Run now sends it as the event."}
        </span>
      </label>
    </div>
  );
}
