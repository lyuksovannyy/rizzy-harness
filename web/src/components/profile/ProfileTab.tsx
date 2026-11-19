// Settings → Profile (README "Profile"): a banner and an avatar, each
// changed by clicking it (made small here, saved at once, removable), then
// the name, the place and time zone, and a short line about the person,
// saved together with Save. The server checks every field; its message goes
// under the field it names.
import { ImagePlus, Trash2 } from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";
import "./profile.css";
import { pageApi } from "../../apiPages";
import { savablePrefs } from "../../lib/modelPlan";
import type { Me, ModelInfo, Prefs, ProfileImages } from "../../types";
import { chars, MAX_DESCRIPTION, type ProfileField, ProfileFields } from "../forms";
import { LinkedImg } from "../Linked";
import { type Slot, shrinkPicture } from "./pictures";
import { avatarUrl } from "./UserAvatar";

const FIELDS: ProfileField[] = ["display_name", "timezone", "country", "city", "description"];

/** The field a server message is about: it starts with the field's name. */
export function fieldOf(message: string): ProfileField | null {
  return FIELDS.find((f) => message.startsWith(`${f} `)) ?? null;
}

/** What the browser can say before sending. */
function localErrors(p: Prefs): Partial<Record<ProfileField, string>> {
  const out: Partial<Record<ProfileField, string>> = {};
  const about = (p.description ?? "").trim();
  if (chars(about) > MAX_DESCRIPTION) out.description = `At most ${MAX_DESCRIPTION} characters (${chars(about)} now).`;
  for (const k of ["display_name", "country", "city"] as const) {
    if (chars((p[k] ?? "").trim()) > 64) out[k] = "At most 64 characters.";
  }
  return out;
}

/** What an agent's commits carry until an e-mail is set (README "What an agent's commits are signed with"). */
const NOREPLY_DOMAIN = "users.noreply.rizzy.invalid";

const message = (x: unknown) => (x instanceof Error ? x.message : String(x));

function Picture(props: {
  slot: Slot;
  me: Me;
  version: string | null;
  initial: string;
  onChanged: (v: ProfileImages) => void;
  onError: (e: string | null) => void;
}) {
  const input = useRef<HTMLInputElement>(null);
  const [busy, setBusy] = useState(false);
  const label = props.slot === "avatar" ? "avatar" : "banner";
  const shown =
    props.version && props.me.user_id
      ? props.slot === "avatar"
        ? avatarUrl(props.me.user_id, props.version)
        : `/api/users/${encodeURIComponent(props.me.user_id)}/banner?v=${encodeURIComponent(props.version)}`
      : null;
  // One that fails to load shows what an empty slot shows.
  const [failed, setFailed] = useState<string | null>(null);
  const url = shown && failed !== shown ? shown : null;
  const pick = async (file: File | undefined) => {
    if (!file) return;
    setBusy(true);
    props.onError(null);
    try {
      props.onChanged(await pageApi.setProfileImage(props.slot, await shrinkPicture(props.slot, file)));
    } catch (x) {
      props.onError(`${props.slot === "avatar" ? "Avatar" : "Banner"}: ${message(x)}`);
    } finally {
      setBusy(false);
      if (input.current) input.current.value = "";
    }
  };
  const remove = async () => {
    if (!confirm(`Remove your ${label}? It is deleted now.`)) return;
    props.onError(null);
    try {
      props.onChanged(await pageApi.removeProfileImage(props.slot));
    } catch (x) {
      props.onError(message(x));
    }
  };
  return (
    <div className={`profile-${props.slot}`}>
      <button
        type="button"
        className={`profile-${props.slot}-pick`}
        aria-label={props.version ? `Change ${label}` : `Add ${label}`}
        title={props.version ? `Change ${label}` : `Add ${label}`}
        disabled={busy || !props.me.user_id}
        onClick={() => input.current?.click()}
      >
        {url ? (
          <LinkedImg url={url} alt="" onError={() => setFailed(url)} />
        ) : props.slot === "avatar" ? (
          <span className="profile-initial">{props.initial}</span>
        ) : (
          <span className="profile-banner-empty muted small">
            <ImagePlus size={16} aria-hidden="true" /> Add a banner
          </span>
        )}
      </button>
      <input
        ref={input}
        type="file"
        hidden
        accept="image/png,image/jpeg,image/webp"
        aria-label={`Choose ${label}`}
        onChange={(e) => void pick(e.target.files?.[0])}
      />
      {props.version && (
        <button type="button" className={`icon profile-remove profile-remove-${props.slot}`} aria-label={`Remove ${label}`} title={`Remove ${label}`} onClick={() => void remove()}>
          <Trash2 size={14} />
        </button>
      )}
    </div>
  );
}

export default function ProfileTab(props: { me: Me; prefs: Prefs; models: ModelInfo[]; onSaved: (p: Prefs) => void }) {
  const [p, setP] = useState<Prefs>(props.prefs);
  const [errors, setErrors] = useState<Partial<Record<ProfileField, string>>>({});
  const [err, setErr] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [emailErr, setEmailErr] = useState<string | null>(null);
  const emailId = useId();
  const [images, setImages] = useState<ProfileImages>({ avatar: null, banner: null });
  const [pictureErr, setPictureErr] = useState<string | null>(null);
  useEffect(() => setP(props.prefs), [props.prefs]);
  useEffect(() => {
    if (!props.me.user_id) return;
    pageApi.profileImages().then(setImages, () => {});
  }, [props.me.user_id]);
  const name = (p.display_name || props.me.username).trim();
  const save = async () => {
    setMsg(null);
    setErr(null);
    const local = localErrors(p);
    setErrors(local);
    setEmailErr(null);
    if (Object.keys(local).length > 0) return;
    setBusy(true);
    try {
      const body = { ...savablePrefs(p, props.models), description: (p.description ?? "").trim() || null, commit_email: (p.commit_email ?? "").trim() || null };
      const saved = await pageApi.saveSettings(body);
      props.onSaved(saved);
      setMsg("Saved.");
    } catch (x) {
      const m = message(x);
      const f = fieldOf(m);
      if (m.startsWith("commit_email ")) setEmailErr(m.slice("commit_email ".length).replace(/^must/, "Must"));
      else if (f) setErrors({ [f]: m.slice(f.length + 1).replace(/^must/, "Must") });
      else setErr(m);
    } finally {
      setBusy(false);
    }
  };
  const changed = (v: ProfileImages) => setImages((old) => ({ ...old, ...v }));
  return (
    <form
      className="card stack profile-card"
      onSubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      <div className="profile-head">
        <Picture slot="banner" me={props.me} version={images.banner} initial="" onChanged={changed} onError={setPictureErr} />
        <Picture slot="avatar" me={props.me} version={images.avatar} initial={name.slice(0, 1).toUpperCase()} onChanged={changed} onError={setPictureErr} />
      </div>
      <p className="muted small profile-pictures-hint">Click the banner or the avatar to choose a picture; it is saved as soon as you pick it.</p>
      {pictureErr && (
        <div className="error-text" role="alert">
          {pictureErr}
        </div>
      )}
      {!props.me.user_id && <div className="muted small">The API token has no pictures.</div>}
      <ProfileFields
        prefs={p}
        onChange={(n) => {
          setP(n);
          setMsg(null);
        }}
        username={props.me.username}
        withDescription
        errors={errors}
      />
      {props.me.user_id && (
        <div className="field">
          <label className="field-label" htmlFor={`${emailId}-email`}>
            E-mail for commits <span className="optional">(optional)</span>
          </label>
          <input
            id={`${emailId}-email`}
            type="email"
            value={p.commit_email ?? ""}
            placeholder={`${props.me.username}@${NOREPLY_DOMAIN}`}
            maxLength={254}
            autoComplete="off"
            aria-invalid={!!emailErr}
            onChange={(e) => {
              setP({ ...p, commit_email: e.target.value.trim() ? e.target.value : null });
              setMsg(null);
            }}
          />
          {emailErr ? (
            <span className="field-hint error-text">{emailErr}</span>
          ) : (
            <span className="field-hint">The address your agents' commits carry, with your name. Until you set one they carry {props.me.username}@{NOREPLY_DOMAIN}, which is not a mailbox. Only you can see it; the shell of your agents can.</span>
          )}
        </div>
      )}
      {err && <div className="error-text">{err}</div>}
      <div className="row end">
        {msg && <span className="muted">{msg}</span>}
        <button className="primary" disabled={busy}>
          Save
        </button>
      </div>
    </form>
  );
}
