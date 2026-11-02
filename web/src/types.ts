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

export interface SessionCreated {
  title: string;
  cwd: string;
  model: string;
  backend: string;
  node_id: string;
  approval_mode: ApprovalMode;
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
  | { type: "assistant"; message_id: string; text: string; done: boolean }
  | {
      type: "tool";
      call_id: string;
      tool: string;
      input: any; // eslint-disable-line @typescript-eslint/no-explicit-any
      live_output: string;
      result: ToolResult | null;
      approval: ApprovalState | null;
    }
  | { type: "error"; message: string }
  | { type: "turn_end"; turn_id: string; reason: TurnEndReason };

export interface SessionView {
  session_id: string | null;
  head_seq: number;
  meta: SessionCreated | null;
  items: Item[];
  running_turn: string | null;
  pending_approvals: string[];
  unknown_events: number;
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
}
