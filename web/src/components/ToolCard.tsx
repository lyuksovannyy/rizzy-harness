import { own } from "../emptyView";
import { ChevronDown, ChevronRight, Globe, Search } from "lucide-react";
import { useContext, useState } from "react";
import type { WebGroup } from "../lib/rich";
import { markedText } from "../lib/callText";
import { showHidden, shownName, stripHidden } from "../md/hidden";
import { hostOf, parseUrl } from "../md/links";
import { rt } from "../md/strings";
import type { Item, Source } from "../types";
import { ApprovalChoice, Held, useShown } from "./ApprovalChoice";
import { CallInput, useOpenFields } from "./CallInput";
import { MadeChip, PublishCard, SaveAsArtifact } from "./ChatArtifacts";
import { ChildAnswer, ChildModel } from "./DelegateResult";
import { MemoryNotice } from "./MemoryNotice";

type Tool = Extract<Item, { type: "tool" }>;
/** Cards that show a file (or ran a command that made one): "Save as artifact" sits on them. */
const SAVEABLE = new Set(["write", "edit", "read", "bash"]);
type Status = "waiting" | "ok" | "failed" | "denied" | "running";

function statusOf(item: Tool): Status {
  if (item.approval && item.approval.decision === null) return "waiting";
  if (item.result) return item.result.ok ? "ok" : "failed";
  return item.approval?.decision === "deny" ? "denied" : "running";
}

const EXTERNAL = { target: "_blank", rel: "noopener noreferrer nofollow", referrerPolicy: "no-referrer" } as const;

/** The model's query as one short line of plain text. */
function queryText(q: unknown): string {
  const s = stripHidden(typeof q === "string" ? q : "").replace(/\s+/g, " ").trim();
  return s.length > 100 ? `${s.slice(0, 99)}…` : s;
}

/** The host a fetch reads: the page it ended on once done, else the URL asked for. */
function fetchHost(item: Tool): string {
  const u = parseUrl(item.result?.sources?.[0]?.url ?? String(item.input?.url ?? ""));
  return u && (u.protocol === "https:" || u.protocol === "http:") ? hostOf(u) : rt("web.page");
}

/** The head of a web tool's card, in the harness's words. */
function webStatus(item: Tool, status: Status): string {
  if (item.tool === "web_search") {
    const q = queryText(item.input?.query);
    if (status === "waiting") return rt("web.wantsSearch", { q });
    if (status === "running") return rt("web.searching", { q });
    if (status === "ok") {
      const n = item.result?.sources?.length ?? 0;
      return n === 1 ? rt("web.searched1", { q }) : rt("web.searched", { q, n: String(n) });
    }
    return rt("web.searchFailed", { q });
  }
  const host = fetchHost(item);
  if (status === "waiting") return rt("web.wantsRead", { host });
  if (status === "running") return rt("web.reading", { host });
  return rt(status === "ok" ? "web.read" : "web.readFailed", { host });
}

const asText = (v: unknown) => (typeof v === "string" ? v : (JSON.stringify(v) ?? ""));

/** What a web call sends, in full: the address a fetch reads, or the query
 * a search hands to the engine. Data the model wants out would ride in it,
 * so nothing is cut and hidden characters show as codes. A web address is
 * shown parsed (punycode host, percent-encoded rest), as it is requested. */
export function webInput(item: Tool): string {
  if (item.tool === "web_search") return showHidden(asText(item.input?.query));
  const raw = asText(item.input?.url);
  const u = parseUrl(raw);
  return u && (u.protocol === "https:" || u.protocol === "http:") ? u.href : showHidden(raw);
}

function WebInput({ item }: { item: Tool }) {
  return (
    <div className="web-input">
      <span className="web-input-label">{rt(item.tool === "web_search" ? "web.query" : "web.address")}</span>
      <bdi dir="ltr" className="web-input-value mono">
        {webInput(item)}
      </bdi>
    </div>
  );
}

/** host · title per page a web tool returned. */
function SourceRows({ sources }: { sources: Source[] }) {
  return (
    <ul className="web-results">
      {sources.map((s) => {
        const u = parseUrl(s.url);
        if (!u || (u.protocol !== "https:" && u.protocol !== "http:")) return null;
        const host = hostOf(u);
        return (
          <li key={s.id}>
            <a href={u.href} {...EXTERNAL}>
              <bdi dir="ltr" className="web-host">
                {host}
              </bdi>
              {" · "}
              <span className="web-title">{stripHidden(s.title) || host}</span>
            </a>
          </li>
        );
      })}
    </ul>
  );
}

export type ChildStatus = "done" | "partial" | "failed";

const CHILD_STATUS_CLASS: Record<ChildStatus, string> = { done: "ok", partial: "waiting", failed: "failed" };

/** The status a `delegate` result names on its second line, the harness's
 * (`harness_core::subagent::render`); every line of the sub-agent's own
 * report is behind "| ", so a status line it wrote is never read here. */
export function childStatus(output: string): ChildStatus | undefined {
  const line = output.split("\n")[1] ?? "";
  const m = /^Sub-agent @\S+ finished: (done|partial|failed)[. ]/.exec(line);
  return m ? (m[1] as ChildStatus) : undefined;
}

/** The one-line head (CSS cuts it): a summary that the body spells out in
 * full. Hidden characters and runs of blanks are marked here too. */
function summary(tool: string, input: Tool["input"]): string {
  return markedText(summaryRaw(tool, input).slice(0, 500), "cmd");
}

function summaryRaw(tool: string, input: Tool["input"]): string {
  const i = input ?? {};
  switch (tool) {
    case "bash":
      return asText(i.command ?? "");
    case "read":
      return `${asText(i.path ?? "")}${i.offset ? `:${asText(i.offset)}` : ""}`;
    case "write":
    case "edit":
      return asText(i.path ?? "");
    case "delegate":
      return `@${asText(i.agent ?? "?")}: ${asText(i.task ?? "")}`;
    case "grep":
      return `/${asText(i.pattern ?? "")}/ ${asText(i.path ?? "")}${i.glob ? ` (${asText(i.glob)})` : ""}`;
    case "glob":
      return `${asText(i.pattern ?? "")} ${asText(i.path ?? "")}`;
    case "skill":
      return `${asText(i.name ?? "")}${i.file ? ` · ${asText(i.file)}` : ""}`;
    case "todo": {
      // The list is pinned above the composer; the card names the change.
      const items: { content?: unknown; status?: unknown }[] = Array.isArray(i.items) ? i.items : [];
      const now = items.find((x) => x?.status === "in_progress");
      const done = items.filter((x) => x?.status === "done").length;
      return `task list: ${done}/${items.length} done${now ? ` · ${asText(now.content ?? "")}` : ""}`;
    }
    default:
      return JSON.stringify(i);
  }
}

/** `blocked`: the viewer may not drive this chat here (its workspace role,
 * the account's grant), so an approval offers Deny only; the server refuses
 * the rest anyway. `projectId`: the chat's project (a `remember` for it
 * shows in the same memory block as any other). `cwd`: the chat's folder,
 * so a path outside it is named. */
export function ToolCard({
  sessionId,
  item,
  blocked,
  cwd,
  inherited,
}: {
  sessionId: string;
  item: Tool;
  blocked?: boolean;
  /** A copy of the parent's event (a fork): the grant it shows does not hold here. */
  inherited?: boolean;
  projectId?: string | null;
  cwd?: string;
}) {
  const pending = item.approval && item.approval.decision === null;
  const [open, setOpen] = useState<boolean | null>(null);
  const [openFields, toggleField] = useOpenFields();
  const status = statusOf(item);
  // The input exactly as the event carried it: the fold keeps the object it
  // parsed, nothing in between trims, reorders or re-encodes it. The node
  // sends the approval the same value it sent with tool_call_started
  // (agent-native: both are `call.input`).
  const input = item.input;
  const shown = useShown(item);
  const held = useContext(Held);
  const web = item.tool === "web_search" || item.tool === "web_fetch";
  // Open while it matters; collapse finished successful calls by default. A
  // web call says what it does in its head, so it opens only on trouble.
  const expanded =
    open ?? (web ? status === "failed" || status === "denied" : status !== "ok" || item.tool === "edit");

  // A proposal the tool accepted: the notice says where it waits. A
  // rejected one stays an ordinary card with the tool's error.
  // A memory block, not a tool card, from the moment the call is made:
  // the server says what it did (saved, waiting, refused) and offers Undo.
  if (item.tool === "remember" && (status === "ok" || status === "running"))
    return (
      <MemoryNotice
        text={asText(item.input?.text ?? "")}
        sessionId={sessionId}
        callId={item.call_id}
        forget={!!item.input?.forget && item.input?.text == null}
        done={status === "ok"}
      />
    );
  // A file the agent published: its card says where the upload stands. A
  // refused or failed call stays an ordinary card with the tool's error.
  if (item.tool === "publish" && status === "ok") return <PublishCard item={item} />;

  const output = item.result ? item.result.output : item.live_output;
  // A delegated sub-task runs in its own session: link to it.
  const child = item.tool === "delegate" ? /^session: ([0-9a-f-]{36})(?:\n|$)/.exec(output)?.[1] : undefined;
  const reported = item.tool === "delegate" && item.result ? childStatus(item.result.output) : undefined;

  const Icon = item.tool === "web_search" ? Search : Globe;
  return (
    <div className={`tool tool-${status}${web ? " tool-web" : ""}`}>
      {web ? (
        <button className="tool-head" aria-expanded={expanded} onClick={() => setOpen(!expanded)}>
          <Icon size={14} aria-hidden className={`s-${status}`} />
          <span className={`web-status ellipsis${status === "running" ? " shimmer" : ""}`}>{webStatus(item, status)}</span>
          <span className="chev">{expanded ? <ChevronDown size={16} /> : <ChevronRight size={16} />}</span>
        </button>
      ) : (
        <button className="tool-head" aria-expanded={expanded} onClick={() => setOpen(!expanded)}>
          <span className={`tool-status s-${status}`}>{status}</span>
          <span className="tool-name">{shownName(item.tool)}</span>
          <span className="tool-summary mono ellipsis">{summary(item.tool, input)}</span>
          {item.result?.exit_code !== null && item.result?.exit_code !== undefined && (
            <span className="muted mono">exit {item.result.exit_code}</span>
          )}
          <span className="chev">{expanded ? <ChevronDown size={16} /> : <ChevronRight size={16} />}</span>
        </button>
      )}
      {child && (
        <div className="child-row small">
          <a className="child-link" href={`#/s/${child}`}>
            Open @{shownName(asText(item.input?.agent))} session{status === "running" ? " (approvals for its tools appear there)" : ""}
          </a>
          <ChildModel sessionId={child} running={status === "running"} done={!!item.result} />
          {status === "running" && <span className="muted small">· its progress is in the panel on the right</span>}
          {reported && (
            <span
              className={`tool-status child-status s-${own(CHILD_STATUS_CLASS, reported) ?? "waiting"}`}
              title="What the sub-agent reported; the harness did not check it"
            >
              {reported}
            </span>
          )}
        </div>
      )}
      {status === "ok" && item.result?.made && <MadeChip sessionId={sessionId} made={item.result.made} />}
      {child && item.result && <ChildAnswer output={item.result.output} agent={shownName(asText(item.input?.agent))} />}
      {pending &&
        item.approval &&
        (held !== null ? (
          <div className="approval-held muted small" role="note">
            {held === item.approval.approval_id ? "Waiting for your answer below." : "Asked after the one below."}
          </div>
        ) : (
          <ApprovalChoice sessionId={sessionId} item={item} blocked={blocked} cwd={cwd} lead={web ? <WebInput item={item} /> : undefined} />
        ))}
      {item.approval?.decision === "deny" && item.approval.note && (
        <div className="approval-note muted">
          Denied with your note: <span className="deny-said">{showHidden(item.approval.note)}</span>
        </div>
      )}
      {item.approval?.decision === "allow_chat" && (
        <div className="approval-note muted">{inherited ? "Allowed in the chat this was forked from" : "Allowed for this chat"}</div>
      )}
      {item.approval && item.approval.decision !== null && item.approval.by !== "user" && (
        <div className="approval-note muted">
          {item.approval.decision === "deny" ? "Denied" : "Allowed"} ({item.approval.by})
        </div>
      )}
      {expanded && web && (
        <div className="tool-body">
          {!pending && <WebInput item={item} />}
          {!pending && <CallInput fields={shown} cwd={cwd} open={openFields} onToggle={toggleField} />}
          {!!item.result?.sources?.length && <SourceRows sources={item.result.sources} />}
          {/* What the model read, as text: where injected instructions would show. */}
          {output &&
            (status === "ok" ? (
              <details className="web-output">
                <summary>{rt("web.output")}</summary>
                <pre className="term">{showHidden(output)}</pre>
              </details>
            ) : (
              <pre className="term web-output">{showHidden(output)}</pre>
            ))}
        </div>
      )}
      {expanded && !web && (
        <div className="tool-body">
          {!pending && <CallInput fields={shown} cwd={cwd} open={openFields} onToggle={toggleField} />}
          {output && <pre className="term">{showHidden(output)}</pre>}
          {status === "ok" && SAVEABLE.has(item.tool) && (
            <SaveAsArtifact path={item.tool === "bash" ? undefined : asText(item.input?.path) || undefined} />
          )}
        </div>
      )}
    </div>
  );
}

/** Several web calls in a row: one "Researched · 2 searches, 3 pages" row
 * that opens to their cards. Open while one of them runs or failed. */
export function WebGroupCard({
  sessionId,
  group,
  tools,
  blocked,
  cwd,
  inherited,
}: {
  sessionId: string;
  group: WebGroup;
  tools: Tool[];
  blocked?: boolean;
  cwd?: string;
  /** Per tool, in order: copied from the chat this one was forked from. */
  inherited?: boolean[];
}) {
  const [open, setOpen] = useState<boolean | null>(null);
  const statuses = tools.map(statusOf);
  const busy = statuses.some((s) => s === "running" || s === "waiting");
  const trouble = statuses.some((s) => s === "failed" || s === "denied");
  const expanded = open ?? (busy || trouble);
  const count = (n: number, one: "web.searches1" | "web.pages1", many: "web.searches" | "web.pages") =>
    n === 1 ? rt(one) : rt(many, { n: String(n) });
  const parts = [
    group.searches ? count(group.searches, "web.searches1", "web.searches") : "",
    group.pages ? count(group.pages, "web.pages1", "web.pages") : "",
  ]
    .filter(Boolean)
    .join(", ");
  return (
    <div className="tool-group">
      <button type="button" className="tool-group-head" aria-expanded={expanded} onClick={() => setOpen(!expanded)}>
        <Globe size={14} aria-hidden />
        <span className={`web-status${busy ? " shimmer" : ""}`}>
          {rt(busy ? "web.researching" : "web.researched", { parts })}
        </span>
        <span className="chev">{expanded ? <ChevronDown size={16} /> : <ChevronRight size={16} />}</span>
      </button>
      {expanded && (
        <div className="tool-group-body">
          {tools.map((t, k) => (
            <ToolCard key={t.call_id} sessionId={sessionId} item={t} blocked={blocked} cwd={cwd} inherited={inherited?.[k]} />
          ))}
        </div>
      )}
    </div>
  );
}
