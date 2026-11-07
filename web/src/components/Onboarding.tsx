// First-run setup, loaded lazily. Admins also connect the model provider;
// everyone sets how they are greeted, their time zone and search engine.
// A step once shown stays mounted (hidden while another is current), so
// going Back finds every field, selection and search as it was left.
import { useEffect, useState } from "react";
import { api } from "../api";
import { browserTimeZone } from "../lib/greeting";
import type { Me, Prefs } from "../types";
import { ProfileFields, SearchEngineFields } from "./forms";
import { chooseDemoProvider, ProviderForm, useProviders } from "./ProviderForm";
import { MemoryCfgForm } from "./Memory";

type Step = "provider" | "memory" | "profile" | "search";

export default function Onboarding(props: { me: Me; prefs: Prefs; onDone: (p: Prefs) => void }) {
  const admin = props.me.role === "admin";
  const [steps, setSteps] = useState<Step[] | null>(null);
  const [i, setI] = useState(0);
  const [prefs, setPrefs] = useState<Prefs>(() => ({
    ...props.prefs,
    timezone: props.prefs.timezone ?? browserTimeZone(),
  }));
  const [err, setErr] = useState<string | null>(null);
  // Steps saved in this run: coming Back to one, the admin can go on
  // without saving (and testing) it again.
  const [saved, setSaved] = useState<Step[]>([]);
  const [seen, setSeen] = useState<Step[]>([]);
  // One providers state for both steps that change it.
  const { state: providers, setState: setProviders, agents, err: providersErr } = useProviders(admin);

  useEffect(() => {
    if (steps) return;
    // Admins configure the provider unless one is already set up (e.g. via
    // harness.toml on a headless install).
    if (!admin || providersErr) setSteps(["profile", "search"]);
    else if (providers) setSteps(providers.configured ? ["profile", "search"] : ["provider", "memory", "profile", "search"]);
  }, [admin, providers, providersErr, steps]);

  const step = steps?.[i];
  useEffect(() => {
    if (step) setSeen((v) => (v.includes(step) ? v : [...v, step]));
  }, [step]);

  if (!steps || !step) return <div className="splash" />;
  const next = () => setI((x) => Math.min(x + 1, steps.length - 1));
  const back = () => setI((x) => Math.max(x - 1, 0));
  const savedNext = (s: Step) => {
    setSaved((d) => (d.includes(s) ? d : [...d, s]));
    next();
  };
  const finish = async () => {
    setErr(null);
    try {
      props.onDone(await api.saveSettings({ ...prefs, onboarded: true }));
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    }
  };
  const mounted = (s: Step) => steps.includes(s) && (s === step || seen.includes(s));

  return (
    <div className="onboarding">
      <div className="card onboard-card">
        <div className="steps muted small">
          Step {i + 1} of {steps.length}
        </div>
        {mounted("provider") && (
          <div className="onboard-step" hidden={step !== "provider"}>
            <h2>Connect an AI provider</h2>
            <p className="muted">
              Chats run on the models you connect here, for every account on this server. You can add
              more providers and change this later in Settings.
            </p>
            {providers ? (
              <ProviderForm
                state={providers}
                agents={agents}
                onSaved={(s) => {
                  setProviders(s);
                  savedNext("provider");
                }}
                onState={setProviders}
                saveLabel="Save and continue"
              />
            ) : (
              <div className="muted">Loading…</div>
            )}
            {saved.includes("provider") ? (
              <div className="row end">
                <button type="button" onClick={next}>
                  Continue without saving
                </button>
              </div>
            ) : (
              <button
                type="button"
                className="link"
                onClick={async () => {
                  try {
                    setProviders(await chooseDemoProvider());
                    savedNext("provider");
                  } catch (x) {
                    setErr(String(x));
                  }
                }}
              >
                Skip: use the built-in demo (no real AI) for now
              </button>
            )}
          </div>
        )}
        {mounted("memory") && (
          <div className="onboard-step" hidden={step !== "memory"}>
            <h2>Memory</h2>
            <p className="muted">
              The assistant can remember facts you confirm. Recall works by keywords out of the box; an
              embeddings model, from your AI provider, Ollama or another endpoint, lets it match by
              meaning. You can change this later in Settings → Memory.
            </p>
            <MemoryCfgForm
              providers={providers}
              onProviders={setProviders}
              onSaved={() => savedNext("memory")}
              saveLabel="Save and continue"
              actions={
                <>
                  <button type="button" onClick={back}>
                    Back
                  </button>
                  {saved.includes("memory") && (
                    <button type="button" onClick={next}>
                      Continue without saving
                    </button>
                  )}
                </>
              }
            />
          </div>
        )}
        {mounted("profile") && (
          <div className="onboard-step" hidden={step !== "profile"}>
            <h2>About you</h2>
            <p className="muted">
              These details are optional. They let the assistant greet you by name, know your local
              time, and look for local things like transit, shops and news. You can change them later in
              Settings.
            </p>
            <ProfileFields prefs={prefs} onChange={setPrefs} username={props.me.username} />
            <div className="row end">
              {i > 0 && (
                <button type="button" onClick={back}>
                  Back
                </button>
              )}
              <button className="primary" type="button" onClick={next}>
                Continue
              </button>
            </div>
          </div>
        )}
        {mounted("search") && (
          <div className="onboard-step" hidden={step !== "search"}>
            <h2>Search engine</h2>
            <p className="muted">Search mode in the bar sends you here, without involving the AI.</p>
            <SearchEngineFields prefs={prefs} onChange={setPrefs} />
            <div className="row end">
              <button type="button" onClick={back}>
                Back
              </button>
              <button className="primary" type="button" onClick={finish}>
                Finish
              </button>
            </div>
          </div>
        )}
        {err && <div className="error-text">{err}</div>}
      </div>
    </div>
  );
}
