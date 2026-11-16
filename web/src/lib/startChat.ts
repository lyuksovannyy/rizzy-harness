// A new chat from the home screen (or a project's): the session first, then
// its first message. Loaded on the first Send, outside the first load: the
// box above shows the message being sent meanwhile (Composer), and the chat
// shows it from the moment it opens (lib/outbox) until the log has it.
//
// `api` and `keep` (writeDraft) come from the caller: imported here, the
// bundler moves them out of the entry chunk into one of their own, which
// costs the first load more than this module saves it.
import type { api as Api } from "../api";
import type { Submission } from "../components/Composer";
import { outbox } from "./outbox";
import { titleFrom } from "./title";

export async function startChat(
  s: Submission,
  where: { workspace: string | null; project?: string },
  open: (id: string) => void,
  api: typeof Api,
  keep: (chat: string, text: string) => void,
): Promise<void> {
  const agentic = s.mode === "agentic";
  const { session_id } = await api.createSession({
    profile: agentic ? "agentic" : "chat",
    title: titleFrom(s.text),
    agent_id: s.agentId,
    model: s.model || undefined,
    node_id: agentic ? s.nodeId : undefined,
    cwd: agentic ? s.cwd : undefined,
    ...(s.incognito ? { incognito: true } : {}),
    ...(where.workspace ? { workspace_id: where.workspace } : {}),
    ...(where.project ? { project_id: where.project } : {}),
    ...(agentic && s.execMode ? { exec_mode: s.execMode } : {}),
    ...(agentic && s.approvalMode ? { approval_mode: s.approvalMode } : {}),
  });
  const o = outbox.add(session_id, s.text, 0, s);
  open(session_id);
  try {
    await api.prompt(session_id, s.text, {
      ...(s.effort ? { effort: s.effort } : {}),
      ...(s.attachments.length ? { attachments: s.attachments } : {}),
      ...(s.compareWith ? { compare: [s.model, s.compareWith] } : {}),
      ...(s.images?.length ? { images: s.images } : {}),
    });
    outbox.accept(o);
  } catch (x) {
    // The box it was typed in is gone with the home screen: the new chat
    // keeps the message, marked not sent with why, with Retry and Edit, and
    // puts it (with its attachments and images) into its own reply box. The
    // draft first, so a box that reads it then has it; an incognito chat
    // keeps no draft, only the bubble and the box. Thrown on, so the home
    // box learns nothing from a message that did not go.
    if (!s.incognito) keep(session_id, s.text);
    outbox.drop(session_id, o, x instanceof Error ? x.message : String(x));
    throw x;
  }
}
