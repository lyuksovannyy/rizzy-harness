import { ChevronDown, ChevronRight, Globe, Search } from "lucide-react";
import { useState } from "react";
import { api } from "../api";
import type { WebGroup } from "../lib/rich";
import { showHidden, stripHidden } from "../md/hidden";
import { hostOf, parseUrl } from "../md/links";
import { rt } from "../md/strings";
import type { Decision, Item, Source } from "../types";
import { MemoryNotice } from "./MemoryNotice";

type Tool = Extract<Item, { type: "tool" }>;
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

function summary(t: Tool): string {
  const i = t.input ?? {};
  switch (t.tool) {
    case "bash":
      return i.command ?? "";
    case "read":
      return `${i.path ?? ""}${i.offset ? `:${i.offset}` : ""}`;
    case "write":
    case "edit":
      return i.path ?? "";
    case "delegate":
      return `@${i.agent ?? "?"}: ${i.task ?? ""}`;
    case "grep":
      return `/${i.pattern ?? ""}/ ${i.path ?? ""}${i.glob ? ` (${i.glob})` : ""}`;
    case "glob":
      return `${i.pattern ?? ""} ${i.path ?? ""}`;
    default:
      return JSON.stringify(i);
  }
}

function EditDiff({ input }: { input: { old_string?: string; new_string?: string } }) {
  const old = (input.old_string ?? "").split("\n");
  const neu = (input.new_string ?? "").split("\n");
  return (
    <pre className="diff">
      {old.map((l, i) => (
        <div key={`o${i}`} className="del">
          - {l}
        </div>
      ))}
      {neu.map((l, i) => (
        <div key={`n${i}`} className="add">
          + {l}
        </div>
      ))}
    </pre>
  );
}

/** `blocked`: the viewer may not drive this chat here (its workspace role,
 * the account's grant), so an approval offers Deny only; the server refuses
 * the rest anyway. */
export function ToolCard({ sessionId, item, blocked }: { sessionId: string; item: Tool; blocked?: boolean }) {
  const pending = item.approval && item.approval.decision === null;
  const [open, setOpen] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const status = statusOf(item);
  const web = item.tool === "web_search" || item.tool === "web_fetch";
  // Open while it matters; collapse finished successful calls by default. A
  // web call says what it does in its head, so it opens only on trouble.
  const expanded =
    open ?? (web ? status === "failed" || status === "denied" : status !== "ok" || item.tool === "edit");

  const decide = async (d: Decision) => {
    if (!item.approval) return;
    setBusy(true);
    setErr(null);
    try {
      await api.approve(sessionId, item.approval.approval_id, d);
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    } finally {
      setBusy(false);
    }
  };

  // A proposal the tool accepted: the notice says where it waits. A
  // rejected one stays an ordinary card with the tool's error.
  if (item.tool === "remember" && status === "ok") return <MemoryNotice text={asText(item.input?.text)} />;

  const output = item.result ? item.result.output : item.live_output;
  // A delegated sub-task runs in its own session: link to it.
  const child = item.tool === "delegate" ? /session: ([0-9a-f-]{36})/.exec(output)?.[1] : undefined;

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
          <span className="tool-name">{item.tool}</span>
          <span className="tool-summary mono ellipsis">{summary(item)}</span>
          {item.result?.exit_code !== null && item.result?.exit_code !== undefined && (
            <span className="muted mono">exit {item.result.exit_code}</span>
          )}
          <span className="chev">{expanded ? <ChevronDown size={16} /> : <ChevronRight size={16} />}</span>
        </button>
      )}
      {child && (
        <a className="child-link small" href={`#/s/${child}`}>
          Open @{item.input?.agent} session{status === "running" ? " (approvals for its tools appear there)" : ""}
        </a>
      )}
      {pending && item.approval && (
        <div className="approval">
          {web && <WebInput item={item} />}
          <div className="approval-reason">{item.approval.reason}</div>
          <div className="row">
            {!blocked && (
              <>
                <button className="primary" disabled={busy} onClick={() => decide("allow")}>
                  Allow
                </button>
                <button disabled={busy} onClick={() => decide("allow_session")}>
                  Allow {item.tool} for this session
                </button>
              </>
            )}
            <button className="danger" disabled={busy} onClick={() => decide("deny")}>
              Deny
            </button>
          </div>
          {err && <div className="error-text">{err}</div>}
        </div>
      )}
      {item.approval && item.approval.decision !== null && item.approval.by !== "user" && (
        <div className="approval-note muted">
          {item.approval.decision === "deny" ? "Denied" : "Allowed"} ({item.approval.by})
        </div>
      )}
      {expanded && web && (
        <div className="tool-body">
          {!pending && <WebInput item={item} />}
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
          {item.tool === "edit" && <EditDiff input={item.input} />}
          {item.tool === "write" && (
            <pre className="term">{String(item.input?.content ?? "").slice(0, 4000)}</pre>
          )}
          {item.tool === "bash" && <pre className="term cmd">$ {item.input?.command}</pre>}
          {output && <pre className="term">{output}</pre>}
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
}: {
  sessionId: string;
  group: WebGroup;
  tools: Tool[];
  blocked?: boolean;
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
          {tools.map((t) => (
            <ToolCard key={t.call_id} sessionId={sessionId} item={t} blocked={blocked} />
          ))}
        </div>
      )}
    </div>
  );
}
