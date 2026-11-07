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
export type TurnEndReason = "completed" | "cancelled" | "error" | "interrupted";

export type Profile = "chat" | "agentic";

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
  decision: Decision | null;
  by: ResolvedBy | null;
}

export type Item =
  | { type: "user"; turn_id: string; text: string; attachments?: string[]; reply?: FormReply }
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
    }
  | { type: "error"; message: string }
  | { type: "turn_end"; turn_id: string; reason: TurnEndReason }
  | { type: "agent_changed"; agent: AgentRef }
  | { type: "summary"; through_seq: number; text: string };

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
}

export interface ModelInfo {
  id: string;
  provider: string;
  effort: boolean;
  vision: boolean;
  default: boolean;
}

export interface Prefs {
  search_url: string;
  image_search_url?: string;
  custom_engines?: { name: string; text_url?: string | null; image_url?: string | null }[];
  default_agent: string | null;
  default_model: string | null;
  display_name?: string | null;
  timezone?: string | null;
  country?: string | null;
  city?: string | null;
  accent?: string | null;
  /** A forced theme; null follows the OS. */
  theme?: "light" | "dark" | null;
  onboarded?: boolean;
}

export type ProviderKind = "openai" | "anthropic" | "mock";

export interface ProviderModel {
  id: string;
  effort: boolean;
  vision: boolean;
}

export interface ProviderState {
  kind: ProviderKind;
  base_url: string;
  model: string;
  /** `stored`, `none`, or `env:NAME` — never the key itself. */
  key: string;
  configured: boolean;
  models: ProviderModel[];
}

export interface SearchHit {
  session_id: string;
  title: string;
  /** Matched terms wrapped in \u0002 … \u0003. */
  snippet: string;
  score: number;
  updated_ms: number;
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
}

export type Role = "admin" | "member";

export interface Me {
  user_id: string | null;
  username: string;
  role: Role;
  can_agentic: boolean;
  must_change_password: boolean;
}

export interface User {
  user_id: string;
  username: string;
  role: Role;
  disabled: boolean;
  can_agentic: boolean;
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

export interface MemoryCfg {
  backend: "builtin" | "mem0" | "off";
  embeddings_url: string;
  embeddings_model: string;
  preset: Preset;
  model: string;
  active_model: string | null;
  presets: { id: Preset; model: string; size: string }[];
}

export interface AttentionItem {
  session_id: string;
  title: string;
  reason: "approval" | "question";
  seq: number;
  parent_id: string | null;
  updated_ms: number;
}

export interface SetupStep {
  id: string;
  title: string;
  status: "pending" | "running" | "ok" | "warning" | "failed" | "todo" | "waiting" | "skipped";
  detail: string;
  script?: string;
  root: boolean;
  choices?: { id: string; label: string }[];
}

export type Engine = "podman" | "docker" | "none";

export interface SetupState {
  id: string;
  mode: "auto" | "manual";
  host: string;
  /** The user harness logs in as now. */
  user: string;
  engine: Engine;
  steps: SetupStep[];
  finished: boolean;
  node_id: string | null;
  error: string | null;
}

export interface NodeProblem {
  kind: "ssh" | "firewall" | "reach" | "host_key";
  detail: string;
}

export interface NodeAttention {
  setup_id: string;
  host: string;
  node_id: string | null;
  problems: NodeProblem[];
}

export interface SetupRecord {
  id: string;
  host: string;
  port: number;
  login_user: string;
  fingerprint: string;
  mode: "auto" | "manual";
  engine: Engine;
  ssh_protected: boolean;
  firewall_enabled: boolean;
  watch_ssh: boolean;
  watch_firewall: boolean;
  admin_sudo: boolean;
  node_id: string | null;
  created_ms: number;
  last_check_ms: number | null;
  problems: NodeProblem[];
}

export interface SetupCommands {
  firewall_on: string;
  firewall_off: string;
  ssh_on: string;
  ssh_off: string;
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

export interface GraphifyStatus {
  local_node: boolean;
  allowed: boolean;
  installed: string | null;
  pinned_version: string;
  remote_setup: string[];
  install_ok?: boolean;
  install_log?: string;
}
