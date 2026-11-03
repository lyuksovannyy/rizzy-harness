// First-run setup, loaded lazily. Admins also connect the model provider;
// everyone sets how they are greeted, their time zone and search engine.
import { useEffect, useState } from "react";
import { api } from "../api";
import { browserTimeZone } from "../lib/greeting";
import type { Me, Prefs } from "../types";
import { chooseDemoProvider, ProfileFields, ProviderForm, SearchEngineFields } from "./forms";
import { MemoryCfgForm } from "./Memory";

type Step = "provider" | "memory" | "profile" | "search";

export default function Onboarding(props: { me: Me; prefs: Prefs; onDone: (p: Prefs) => void }) {
  const [steps, setSteps] = useState<Step[] | null>(null);
  const [i, setI] = useState(0);
  const [prefs, setPrefs] = useState<Prefs>(() => ({
    ...props.prefs,
    timezone: props.prefs.timezone ?? browserTimeZone(),
  }));
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    // Admins configure the provider unless one is already set up (e.g. via
    // harness.toml on a headless install).
    if (props.me.role !== "admin") {
      setSteps(["profile", "search"]);
      return;
    }
    api
      .provider()
      .then((p) =>
        setSteps(p.configured ? ["profile", "search"] : ["provider", "memory", "profile", "search"]),
      )
      .catch(() => setSteps(["profile", "search"]));
  }, [props.me.role]);

  if (!steps) return <div className="splash" />;
  const step = steps[i]!;
  const next = () => setI((x) => Math.min(x + 1, steps.length - 1));
  const finish = async () => {
    setErr(null);
    try {
      props.onDone(await api.saveSettings({ ...prefs, onboarded: true }));
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    }
  };

  return (
    <div className="onboarding">
      <div className="card onboard-card">
        <div className="steps muted small">
          Step {i + 1} of {steps.length}
        </div>
        {step === "provider" && (
          <>
            <h2>Connect an AI provider</h2>
            <p className="muted">
              Chats run on the model you connect here, for every account on this server. You can change
              it later in Settings.
            </p>
            <ProviderForm onSaved={next} saveLabel="Save and continue" />
            <button
              type="button"
              className="link"
              onClick={async () => {
                try {
                  await chooseDemoProvider();
                  next();
                } catch (x) {
                  setErr(String(x));
                }
              }}
            >
              Skip: use the built-in demo (no real AI) for now
            </button>
          </>
        )}
        {step === "memory" && (
          <>
            <h2>Memory</h2>
            <p className="muted">
              The assistant can remember facts you confirm. Recall works by keywords out of the box; an
              embeddings model (for example in Ollama, on this machine or your network) lets it match by
              meaning. You can change this later in Settings → Memory.
            </p>
            <MemoryCfgForm onSaved={next} saveLabel="Save and continue" />
            <button type="button" className="link" onClick={next}>
              Skip: keywords only
            </button>
          </>
        )}
        {step === "profile" && (
          <>
            <h2>About you</h2>
            <ProfileFields prefs={prefs} onChange={setPrefs} username={props.me.username} />
            <div className="row end">
              {i > 0 && (
                <button type="button" onClick={() => setI(i - 1)}>
                  Back
                </button>
              )}
              <button className="primary" type="button" onClick={next}>
                Continue
              </button>
            </div>
          </>
        )}
        {step === "search" && (
          <>
            <h2>Search engine</h2>
            <p className="muted">Search mode in the bar sends you here, without involving the AI.</p>
            <SearchEngineFields prefs={prefs} onChange={setPrefs} />
            <div className="row end">
              <button type="button" onClick={() => setI(i - 1)}>
                Back
              </button>
              <button className="primary" type="button" onClick={finish}>
                Finish
              </button>
            </div>
          </>
        )}
        {err && <div className="error-text">{err}</div>}
      </div>
    </div>
  );
}
