// Wire types. Mirrors crates/core (event.rs, view.rs). Keep in step.

export interface RawEvent {
  session_id: string;
  seq: number;
  ts_ms: number;
  kind: string;
  payload: any; // eslint-disable-line @typescript-eslint/no-explicit-any
}

export type ApprovalMode = "ask_all" | "ask" | "auto";
export type Decision = "allow" | "deny" | "allow_session";
export type ResolvedBy = "user" | "policy" | "cancelled" | "interrupted";
export type TurnEndReason = "completed" | "cancelled" | "error" | "interrupted" | "stopped";

export type Profile = "chat" | "agentic";

/** How an agentic chat's commands run (N.10a, docs/execution-modes.md):
 * chosen when it is created, never changed. */
export type ExecMode = "sandbox" | "host";
/** What a person may run (N.10a part 2), lowest first: an account's ceiling
 * and a member's level in a workspace; the lower of the two counts. */
export type ExecLevel = "none" | "container" | "sandbox" | "host";

export interface SessionCreated {
  title: string;
  cwd: string;
  model: string;
  backend: string;
  node_id: string;
  approval_mode: ApprovalMode;
  profile: Profile;
  agent_id?: string;
  incognito?: boolean;
  /** Set on a sub-agent's session: the session that delegated to it. */
  parent_session?: string;
  /** Never resumed by a background command's end (a routine's chat). */
  no_callbacks?: boolean;
  /** How its commands run; absent for a chat (no commands). */
  exec_mode?: ExecMode;
}

export interface AgentRef {
  agent_id: string;
  name: string;
  prompt_hash: string;
}

export interface Variant {
  group: string;
  index: number;
  model?: string;
}

/** A page behind a citation label (`S4`), written by the web tools only. */
export interface Source {
  id: string;
  url: string;
  title: string;
}

export type UiKind = "choices" | "form" | "progress" | "chart";

/** One option of a question or of a form's choice field. */
export interface Choice {
  id: string;
  label: string;
  description?: string;
}

export interface ChoicesSpec {
  question: string;
  options: Choice[];
  multi: boolean;
  other: boolean;
  min?: number;
  max?: number;
}

export type FieldType = "text" | "number" | "choice" | "toggle" | "date";

/** A form field, flat: which keys a type has is checked in Rust. */
export interface Field {
  id: string;
  label: string;
  type: FieldType;
  /** Asks for a secret: shown as a warning, never as an input. */
  refused?: boolean;
  required?: boolean;
  help?: string;
  multiline?: boolean;
  max_length?: number;
  placeholder?: string;
  /** Numbers for number and choice fields, `YYYY-MM-DD` for dates. */
  min?: number | string;
  max?: number | string;
  step?: number;
  default?: string | number | boolean | string[];
  unit?: string;
  options?: Choice[];
  multi?: boolean;
}

export interface FormSpec {
  title: string;
  description?: string;
  fields: Field[];
}

export type StepStatus = "pending" | "active" | "done" | "failed" | "skipped";

export interface ProgressSpec {
  title: string;
  status?: "running" | "done" | "failed";
  value?: number;
  steps: { label: string; status: StepStatus; detail?: string }[];
}

export interface ChartSpec {
  kind: "bar" | "line";
  title?: string;
  x: { label?: string; values: string[] | number[] };
  y?: { label?: string; unit?: string };
  series: { name: string; values: (number | null)[] }[];
  stacked?: boolean;
}

interface UiBlockBase {
  ordinal: number;
  hash: string;
  id: string;
  /** Why the block is not shown (harness text); set when spec is not. */
  error?: string;
}

/** A `ui:` block of an assistant message, validated and normalized in Rust
 * (crates/core/src/ui). The browser renders only these specs. */
export type UiBlock = UiBlockBase &
  (
    | { type: "choices"; spec?: ChoicesSpec }
    | { type: "form"; spec?: FormSpec }
    | { type: "progress"; spec?: ProgressSpec }
    | { type: "chart"; spec?: ChartSpec }
  );

/** One answer value: text, a number, a toggle, an option id or several. */
export type FormValue = string | number | boolean | string[];

/** The user's answer to a choices or form block (user_message.reply).
 * Choices: `{selected: [option ids], other?: text}`; a form: field id to
 * value, unanswered optional fields left out. */
export interface FormReply {
  message_id: string;
  block_id: string;
  values: Record<string, FormValue>;
}

export interface ToolResult {
  ok: boolean;
  output: string;
  exit_code: number | null;
  sources?: Source[];
}

export interface ApprovalState {
  approval_id: string;
  reason: string;
  /** "This exact call for this turn" may be offered; present only when
   * true, as the Rust view serialises it. */
  turn_allow?: true;
  decision: Decision | null;
  by: ResolvedBy | null;
}

/** An image a user message carries (`user_message.images`); its bytes stay in Media. */
export interface ImageRef {
  media_id: string;
  sha256: string;
  mime: string;
  name: string;
  width: number;
  height: number;
}

/** What a node reports about a published file: display only, never trusted
 * for anything else (docs/artifacts.md). */
export interface ArtifactMeta {
  toolchain?: string;
  package?: { name: string; version: string };
  git?: { commit: string; branch: string; dirty: boolean };
  target?: string;
  entries?: { path: string; size: number }[];
  entries_total?: number;
  skipped?: Record<string, number>;
  source_path?: string;
}

/** A file staged for Artifacts (the `artifact_staged` event): on its tool
 * card when an agent published it, a standalone item when the user saved it. */
export interface StagedArtifact {
  stage_id: string;
  call_id?: string | null;
  by: "agent" | "user";
  name: string;
  filename: string;
  packed: "tar.gz" | null;
  size: number;
  sha256: string;
  kind_hint?: string | null;
  source_path: string;
  notes?: string | null;
  meta: ArtifactMeta;
}

export type Item =
  | {
      type: "user";
      turn_id: string;
      text: string;
      attachments?: string[];
      reply?: FormReply;
      images?: ImageRef[];
      /** Sent while a turn ran (X.5): its seq, and whether the agent has read it. */
      queued?: { seq: number; read: boolean };
    }
  | {
      type: "assistant";
      message_id: string;
      text: string;
      done: boolean;
      author: string | null;
      variant: Variant | null;
      blocks?: UiBlock[];
    }
  | {
      type: "tool";
      call_id: string;
      author: string | null;
      tool: string;
      input: any; // eslint-disable-line @typescript-eslint/no-explicit-any
      live_output: string;
      result: ToolResult | null;
      approval: ApprovalState | null;
      /** A `publish` call's file, once the node staged it. */
      staged?: StagedArtifact;
    }
  | { type: "error"; message: string }
  | { type: "turn_end"; turn_id: string; reason: TurnEndReason }
  | { type: "agent_changed"; agent: AgentRef }
  | { type: "summary"; through_seq: number; text: string }
  /** The turn's main model failed or is unavailable: `model` (prefixed) answered instead of `from`. */
  | { type: "fallback"; turn_id: string; model: string; from: string; reason?: string | null }
  /** A file the user saved as an artifact ("Save as artifact"). */
  | { type: "artifact"; staged: StagedArtifact }
  /** Saving a file as an artifact failed on the node (`artifact_stage_failed`). */
  | { type: "artifact_failed"; path: string; name: string | null; error: string }
  /** A background command ended (`background_ended`); `tail` is program output, shown as text. */
  | {
      type: "background_ended";
      bg_id: string;
      reason: string;
      exit_code: number | null;
      ran_ms: number;
      tail: string;
    }
  /** A turn the harness started because these background commands ended while the chat was idle. */
  | { type: "callback"; turn_id: string; bg_ids: string[] }
  /** The node did not resume the agent for these ends (`callback_refused`). */
  | { type: "callback_refused"; bg_ids: string[]; reason: string };

export type UserItem = Extract<Item, { type: "user" }>;
export type ToolItem = Extract<Item, { type: "tool" }>;

export interface SessionView {
  session_id: string | null;
  head_seq: number;
  meta: SessionCreated | null;
  items: Item[];
  running_turn: string | null;
  pending_approvals: string[];
  unknown_events: number;
  active_agent: string | null;
  running_agent: AgentRef | null;
  variant_selection: Record<string, string>;
  usage: Usage;
  /** Context size of the latest turn (its last model call's input). */
  context_tokens: number;
  checkpoints: Record<string, CheckpointState>;
  /** Background commands by id; absent when there are none (as serde writes it). */
  background?: Record<string, BackgroundState>;
  /** The agent's task list (the latest `todo_updated`); absent when empty. */
  todo?: TodoItem[];
  /** A model call waits to be sent again (`model_retry`); absent otherwise. */
  retry?: ModelRetry;
  /** The latest turn's limits (`turn_ended.usage`): the window of the model it
   * started on, and the history size at which the next summary is written
   * (0: never). Absent when not known. */
  context_window?: number;
  summary_at?: number;
}

export type RetryReason = "rate_limit" | "overloaded" | "server_error" | "network";

export interface ModelRetry {
  turn_id: string;
  model: string;
  /** This retry's number, from 1, of `of`. */
  attempt: number;
  of: number;
  /** When it is sent again (ms since the epoch, on the node's clock)... */
  at_ms: number;
  /** ...this long after the event: the most a skewed browser clock shows. */
  wait_ms: number;
  reason: RetryReason;
}

/** One item of the agent's task list: the model's text, one line. */
export interface TodoItem {
  content: string;
  status: "pending" | "in_progress" | "done";
}

export interface BackgroundState {
  call_id: string;
  /** When its timeout ends it at the latest, on the node's clock. */
  ends_by_ms: number;
  /** The timeout it runs under: what is left can never be more. */
  timeout_s: number;
  ended: boolean;
}

export interface CheckpointState {
  before: string | null;
  after: string | null;
  files: string[];
  restored: boolean;
}

export interface Usage {
  input_tokens: number;
  output_tokens: number;
  context_tokens: number;
  /** Of input_tokens, read from / written to the provider's prompt cache. */
  cache_read_tokens?: number;
  cache_write_tokens?: number;
  /** A turn's own limits (`turn_ended` only, never in the totals). */
  window?: number;
  summary_at?: number;
}

export interface UploadedAttachment {
  attachment_id: string;
  name: string;
  mime: string;
  chars: number;
  truncated: boolean;
}

export type Effort = "low" | "medium" | "high" | "max";

export interface Agent {
  id: string;
  name: string;
  description: string;
  builtin: boolean;
  /** The admin's default model for this agent (prefixed); null when it has no chain of its own. */
  default_model: string | null;
  /** The model it runs on (prefixed): its own, else the server default. */
  main_model: string | null;
  /** Tried in order when the default fails or is unavailable (prefixed). */
  fallbacks: string[];
  /** Its default and fallbacks that the provider no longer lists. */
  unavailable: string[];
  avatar: AgentAvatar;
  /** Where its definition comes from (absent in older fixtures: built in). */
  origin?: AgentOrigin;
  /** What it can do; for built-in and file agents read off the definition. */
  features?: AgentFeatures;
}

/** Built into the server, a file on the server (`agents/<id>.md`), or made on the Agents page. */
export type AgentOrigin = "builtin" | "file" | "custom";

/** The Agents page's switches (see `harness_agents::Features`). */
export interface AgentFeatures {
  /** Your memory: facts recalled into its turns, and proposing new ones. */
  memory: boolean;
  /** In a project's chats, the project's facts the same way (docs/projects.md). */
  project_memory: boolean;
  web: boolean;
  read_files: boolean;
  edit_files: boolean;
  commands: boolean;
  /** Hands sub-tasks to the agents available as sub-agents. */
  delegate: boolean;
  /** In the agent picker and @mentions. */
  as_agent: boolean;
  /** May be handed sub-tasks. */
  as_subagent: boolean;
}

/** One agent as its page shows it (`GET /api/agents/{id}`). */
export interface AgentDetail extends Agent {
  origin: AgentOrigin;
  features: AgentFeatures;
  /** Its instructions as written: a built-in's own prompt, without the additional instructions. */
  prompt: string;
  /** The saved summary; empty means `description_default` (built-in and file agents). */
  summary: string;
  /** Built-in and file agents: text added after their prompt. */
  instructions: string;
  description_default: string;
}

/** A new agent made on the Agents page. */
export interface NewAgent {
  id: string;
  name: string;
  summary: string;
  prompt: string;
  features: AgentFeatures;
}

export type AvatarShape = "circle" | "triangle" | "square" | "hexagon" | "diamond" | "drop";
export type AvatarEyes = "round" | "long" | "short" | "wide" | "sleepy";
export type AvatarAccent = "blue" | "purple" | "pink" | "red" | "orange" | "yellow" | "green" | "graphite";

/** How an agent looks: a shape with eyes, in one of the accents. */
export interface AgentAvatar {
  shape: AvatarShape;
  eyes: AvatarEyes;
  accent: AvatarAccent;
}

/** What an avatar's face shows; the browser derives it from the session. */
export type AvatarState = "idle" | "thinking" | "working" | "unsure" | "done" | "error";

/** A problem that lasts until fixed: a model in use that the provider dropped. */
export interface Notice {
  id: string;
  kind: "model_unavailable";
  /** Prefixed. */
  model: string;
  /** The provider's display name. */
  provider: string;
  /** Agents using it as their main model or a fallback. */
  agents: string[];
  /** It is the viewer's own default model. */
  your_default: boolean;
  /** It is the server default and applies where the notice was computed (no workspace default replaces it). */
  global_default: boolean;
  /** It is the workspace's own default model (its General chain), in place of the server default. */
  workspace_default: boolean;
  /** The viewer is a server admin and can fix it in Settings. */
  can_fix: boolean;
  /** The use comes from the workspace's own settings (its default or an agent's chain there). */
  workspace: boolean;
  /** The viewer may change the workspace's settings, where it can be fixed. */
  can_fix_workspace: boolean;
}

/** A model offered to chats. `id` is `<provider_id>::<raw>`. */
export interface ModelInfo {
  id: string;
  raw: string;
  /** The provider's display name. */
  provider: string;
  provider_id: string;
  name: string | null;
  context: number | null;
  max_output: number | null;
  effort: boolean;
  vision: boolean;
  tools: boolean;
  default: boolean;
  /** The provider no longer lists it: kept so sessions and agents naming it
   * still resolve, but not offered for new picks. */
  unavailable: boolean;
}

export type BackdropPref = "animated" | "still" | "off";

export interface Prefs {
  search_url: string;
  image_search_url?: string;
  custom_engines?: { name: string; text_url?: string | null; image_url?: string | null }[];
  default_agent: string | null;
  /** The person's default agent for Agentic chats. */
  default_agentic_agent?: string | null;
  default_model: string | null;
  display_name?: string | null;
  timezone?: string | null;
  country?: string | null;
  city?: string | null;
  accent?: string | null;
  /** A forced theme; null follows the OS. */
  theme?: "light" | "dark" | null;
  /** How the home screen's per-mode backgrounds move; null is animated. */
  backdrop?: BackdropPref | null;
  /** A still, fainter background beside chats on wide screens. */
  backdrop_chats?: boolean;
  onboarded?: boolean;
}

export type ProviderKind = "openai" | "anthropic" | "mock";

export interface ProviderWarning {
  kind: "trusted_source" | "training_data" | "data_collection";
  text: string;
  /** Save unlocks this many seconds after a successful test. */
  wait_secs: number;
}

export interface ProviderPreset {
  id: string;
  label: string;
  kind: ProviderKind;
  /** null: the user enters it. */
  base_url: string | null;
  url_editable: boolean;
  needs_key: boolean;
  warning: ProviderWarning | null;
  /** The user names it and picks the prefix. */
  custom: boolean;
  /** "own": an endpoint the admin runs or names; "known": a hosted service. */
  group: "own" | "known";
}

/** A model as the provider reports it; null: not reported. */
export interface ProviderModel {
  /** The raw id, no prefix. */
  id: string;
  name: string | null;
  context: number | null;
  max_output: number | null;
  effort: boolean | null;
  vision: boolean | null;
  tools: boolean | null;
  /** Added by the admin with parameters set by hand; never unavailable. */
  custom: boolean;
  /** Saved, but missing from the provider's latest listing. */
  unavailable: boolean;
  /** When it went missing (ms). */
  missing_since: number | null;
}

/** The server's last listing of a provider; fetched_ms null: never listed. */
export interface ModelListing {
  models: ProviderModel[];
  fetched_ms: number | null;
}

/** An agent's model and, in order, its fallbacks (prefixed ids). */
export interface AgentChain {
  model: string;
  fallbacks: string[];
}

/** What a one-line test request to a model gave. */
export type ModelTestResult = { ok: true; reply: string; ms: number } | { ok: false; error: string };

export interface ProviderState {
  /** The prefix of its model ids, unique. */
  id: string;
  name: string;
  preset: string;
  kind: ProviderKind;
  base_url: string;
  /** `stored`, `none`, or `env:NAME` — never the key itself. */
  key: string;
  models: ProviderModel[];
}

export interface ProvidersState {
  providers: ProviderState[];
  presets: ProviderPreset[];
  /** Prefixed. */
  default_model: string | null;
  /** Agent id to its model and fallbacks. */
  agent_models: Record<string, AgentChain>;
  /** At least one non-mock provider. */
  configured: boolean;
}

export type SearchKind = "chats" | "projects" | "media" | "artifacts";

/** One row of the search panel (`GET /api/search`). Title, preview and
 * snippet mark matches with \u0002 … \u0003. */
export interface SearchResult {
  kind: "chat" | "project" | "media" | "artifact";
  /** A chat's 36-character id; 32 hex characters for the rest. */
  id: string;
  title: string;
  /** Localized by the client. */
  type:
    | "chat"
    | "agentic"
    | "routine"
    | "project"
    | "image"
    | "video"
    | "audio"
    | "media_other"
    | "binary"
    | "package"
    | "archive"
    | "document"
    | "other";
  /** Line 2 of the row; "" when there is none. */
  preview: string;
  /** Line 3, only when title and preview do not show every term. */
  snippet: string | null;
  time_ms: number;
  project_id: string | null;
  running?: boolean;
  /** Same-origin path only. */
  thumb?: string | null;
}

export interface SearchResponse {
  results: SearchResult[];
  next: string | null;
  /** The tabs this viewer has, in order. */
  kinds: SearchKind[];
}

export interface SessionRow {
  session_id: string;
  node_id: string;
  title: string;
  cwd: string;
  model: string;
  created_ms: number;
  updated_ms: number;
  head_seq: number;
  running: boolean;
  owner_id: string | null;
  agent_id: string | null;
  profile: Profile | null;
  incognito: boolean;
  /** Set on a sub-agent's session. */
  parent_id: string | null;
  /** The workspace it is in; null only for a chat without an owner (API token). */
  workspace_id?: string | null;
  /** Agentic chats only: why the viewer cannot drive it here (deny and stop still work). */
  blocked?: string | null;
  /** The project the chat is in (docs/projects.md). */
  project_id?: string | null;
  /** Set when a routine run started the chat. */
  routine?: RoutineMark | null;
  /** Background commands running in it, as far as the server knows. */
  background?: number;
  /** Its last turn finished (or failed) after the viewer last looked at it. */
  unread?: boolean;
}

/** The routine run behind a chat (`SessionRow.routine`). */
export interface RoutineMark {
  routine_id: string;
  name: string;
  run_id: string;
  status: string;
  seen: boolean;
  promoted: boolean;
  /** The chat holds data from an external event. */
  untrusted: boolean;
}

export interface RemoteNode {
  node_id: string;
  hostname: string;
  key_fingerprint: string;
  created_ms: number;
  revoked: boolean;
  connected: boolean;
}

export interface NodeInfo {
  node_id: string;
  hostname: string;
  version: string;
  connected: boolean;
  embedded: boolean;
  profile: Profile;
  /** The execution modes the node announced it offers, most confined first. */
  exec_modes?: ExecMode[];
  /** Their gaps (`gap:sandbox:netns`, `gap:host:netns`), for the chat to say. */
  exec_gaps?: string[];
}

/** A server-wide account role (Settings → Users), not a workspace role. */
export type Role = "admin" | "member";

export interface Me {
  user_id: string | null;
  username: string;
  role: Role;
  /** The account's ceiling as it counts (a server admin's is `host`). */
  exec_ceiling: ExecLevel;
  must_change_password: boolean;
  /** The active workspace; null for the API token. */
  workspace?: WorkspaceSummary | null;
}

/** What a workspace role allows, in this order on the wire. */
export type Perm = "settings" | "members" | "routines";
export type WorkspaceRole = "owner" | "admin" | "member" | "guest";

export interface WorkspaceSummary {
  workspace_id: string;
  /** The stored name; a Personal workspace is shown as t("workspace.personal"). */
  name: string;
  personal: boolean;
  role: WorkspaceRole;
  permissions: Perm[];
  members: number;
  /** The caller's effective level here: the lower of their ceiling and
   * their level in it (in a Personal workspace, the ceiling). */
  exec_level: ExecLevel;
}

export interface WorkspaceList {
  /** Personal first, then by name. */
  workspaces: WorkspaceSummary[];
  active: string | null;
  can_create: boolean;
}

/** A team workspace's own defaults; `agent_models.general` is its default model. */
export interface WorkspaceSettings {
  default_agent: string | null;
  agent_models: Record<string, AgentChain>;
}

export interface WorkspaceDetail extends WorkspaceSummary {
  /** Unknown agents and models already dropped, so saving what was read validates. */
  settings: WorkspaceSettings;
  /** The caller's own "use my memory here" switch (always true for Personal). */
  use_memory: boolean;
  created_ms: number;
}

export interface WorkspaceMember {
  user_id: string;
  username: string;
  display_name: string | null;
  role: WorkspaceRole;
  joined_ms: number;
  /** Their level here as set (an owner's is always `host`). Only to those
   * who manage members here, and on one's own row. */
  exec_level?: ExecLevel;
  /** As it counts: the lower of that and their account's level. */
  effective_level?: ExecLevel;
}

/** Settings → Server → Workspaces: one row per workspace. */
export interface AdminWorkspace {
  workspace_id: string;
  name: string;
  personal: boolean;
  /** Usernames. */
  owners: string[];
  members: number;
  created_ms: number;
}

export interface AdminWorkspaceMember {
  user_id: string;
  username: string;
  role: WorkspaceRole;
  disabled: boolean;
}

export interface User {
  user_id: string;
  username: string;
  role: Role;
  disabled: boolean;
  /** As it counts: a server admin's is `host`. */
  exec_ceiling: ExecLevel;
  /** As stored: what applies once they are no longer a server admin. */
  exec_ceiling_stored: ExecLevel;
  must_change_password: boolean;
  created_ms: number;
}

export type FactKind = "fact" | "preference" | "location";

export interface MemoryFact {
  id: string;
  owner_id: string | null;
  text: string;
  kind: FactKind;
  status: "proposed" | "active";
  source_session: string | null;
  created_ms: number;
  updated_ms: number;
}

export type Preset = "fast" | "balanced" | "best";

export type MemorySource = "provider" | "ollama";

/** A setting the server's environment (`.env`) fixes. */
export interface EnvLock {
  key: string;
  var: string;
}

export interface MemoryCfg {
  backend: "builtin" | "mem0" | "qdrant" | "off";
  recall: "keywords" | "meaning";
  source: MemorySource;
  provider_id: string | null;
  embeddings_url: string;
  embeddings_model: string;
  preset: Preset;
  /** Effective. */
  model: string;
  active_model: string | null;
  presets: { id: Preset; model: string; size: string }[];
  /** Settings `.env` fixes; the form is read-only while there are any. */
  locked?: EnvLock[];
}

export interface AttentionItem {
  session_id: string;
  title: string;
  /** `resumed`/`not_resumed`: what a background command's end did while the owner was away. */
  reason: "approval" | "question" | "resumed" | "not_resumed";
  /** For `resumed`/`not_resumed`: what happened, in words. */
  detail?: string;
  seq: number;
  parent_id: string | null;
  updated_ms: number;
  /** Attention lists the caller's chats in every workspace. */
  workspace_id?: string | null;
}

/** A routine's results waiting for its owner (`GET /api/attention`). */
export interface RoutineAttention {
  kind: "result" | "failed" | "paused";
  routine_id: string;
  name: string;
  workspace_id: string;
  run_id: string | null;
  session_id: string | null;
  status: string;
  reason: string | null;
  /** Finished runs not seen yet. */
  unseen: number;
  notify: "always" | "failures" | "never";
  ts_ms: number;
}

export interface AttentionResponse {
  items: AttentionItem[];
  routines?: RoutineAttention[];
}

/** What the server has, and what the viewer may use in one workspace
 * (`GET /api/features?workspace=`): the sidebar shows an area only when its
 * `show` is true; `why` explains an empty page. */
export interface Features {
  voice: boolean;
  memory: boolean;
  routines: { enabled: boolean; show: boolean; webhooks: boolean; why: string | null };
  media: { enabled: boolean; show: boolean; max_file: number; vision_images: boolean };
  artifacts: { enabled: boolean; show: boolean; why: string | null };
  search: { kinds: SearchKind[] };
}

export type Style = "off" | "lite" | "full" | "ultra";

export interface TokenSavings {
  prompt_cache: boolean;
  mask_old_outputs: boolean;
  keep_tool_outputs: number;
  filter_shell_output: boolean;
  bounded_reads: boolean;
  web_readability: boolean;
  chat_style: Style;
  agentic_style: Style;
  repo_map: boolean;
  repo_map_tokens: number;
  outline_tool: boolean;
  graphify: boolean;
}

/** `GET /api/admin/graphify`: read-only. node.toml decides; no HTTP call
 * installs graphify or edits node.toml. */
export interface GraphifyStatus {
  local_node: boolean;
  allowed: boolean;
  /** The built-in node's node.toml, when there is a built-in node. */
  node_toml: string | null;
  installed: string | null;
  pinned_version: string;
  /** What to do on a node to turn it on. */
  steps: string[];
}
