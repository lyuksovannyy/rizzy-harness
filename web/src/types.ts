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
}

export interface AgentRef {
  agent_id: string;
  name: string;
  prompt_hash: string;
}

export interface Variant {
  group: string;
  index: number;
}

export interface ToolResult {
  ok: boolean;
  output: string;
  exit_code: number | null;
}

export interface ApprovalState {
  approval_id: string;
  reason: string;
  decision: Decision | null;
  by: ResolvedBy | null;
}

export type Item =
  | { type: "user"; turn_id: string; text: string }
  | {
      type: "assistant";
      message_id: string;
      text: string;
      done: boolean;
      author: string | null;
      variant: Variant | null;
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
  | { type: "agent_changed"; agent: AgentRef };

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
