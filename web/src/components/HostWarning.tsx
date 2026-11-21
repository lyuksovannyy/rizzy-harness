// The warning shown when someone picks the host shell for a new chat, and
// when someone grants the `host` level (an account's ceiling or a member's
// level in a workspace), before anything is saved (N.10a; the owner,
// 2026-11-12: "display warning when selected and say the risks"). The same
// risks in both. Its own chunk: only those who may pick or grant `host` ever
// load it.
import { useEffect, useRef } from "react";

/** Whom `host` would be granted to, and where ("in Acme", "for every
 * workspace they are in"). */
export interface HostGrant {
  who: string;
  where: string;
}

export function HostWarning(props: {
  /** The node of a new chat; not used for a grant. */
  node?: string;
  grant?: HostGrant;
  /** An existing chat is being switched to the host shell (N.10b): `approval` is its approval mode, which stays. */
  switching?: { approval: string };
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const g = props.grant;
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const d = dialog.current;
    if (d && typeof d.showModal === "function" && !d.open) d.showModal();
  }, []);
  const cancel = () => {
    dialog.current?.close?.();
    props.onCancel();
  };
  const confirm = () => {
    dialog.current?.close?.();
    props.onConfirm();
  };
  return (
    <dialog
      ref={dialog}
      className="host-warning"
      aria-labelledby="host-warning-title"
      // Esc and the backdrop cancel; nothing is chosen.
      onCancel={(e) => {
        e.preventDefault();
        cancel();
      }}
      onClick={(e) => e.target === dialog.current && cancel()}
    >
      <div className="stack">
        <h2 id="host-warning-title">
          {g
            ? `Let ${g.who} run commands without the sandbox?`
            : props.switching
              ? "Switch this chat's commands to the host shell?"
              : "Run this chat's commands without the sandbox?"}
        </h2>
        <p>
          {g
            ? `With the host level ${g.where}, ${g.who} can start chats in the host shell of any node that offers it. In such a chat:`
            : `In the host shell on ${props.node}:`}
        </p>
        <ul className="host-risks">
          <li>the agent's commands run as the node's own user, with nothing between them and the machine;</li>
          <li>they can read and change every file that user can, its keys and other projects included;</li>
          <li>what they start can keep running after the chat ends;</li>
          <li>one web page or file the agent reads can turn all of that against you;</li>
          <li>
            {g
              ? "what a sandbox chat wrote in a folder (a script, a Makefile, git hooks) runs with all of that too in a host chat there."
              : "what a sandbox chat wrote in this folder (a script, a Makefile, git hooks) runs here with all of that too."}
          </li>
        </ul>
        {g ? (
          <p>
            The approval card is then the only check, and only in Ask: in Auto (the default for new chats) every command
            runs as the node's user with no question asked. Lowering the level later stops their host chats at once.
          </p>
        ) : props.switching ? (
          <>
            <p>
              The approval card is then the only check, and only in Ask.
              {props.switching.approval === "auto" || props.switching.approval === "accept_edits"
                ? props.switching.approval === "auto"
                  ? " This chat is in Auto: from its next message every command runs in the host shell with no question asked."
                  : " This chat is in Accept edits: its edits go through unasked, and its commands ask."
                : ""}{" "}
              The approval mode stays as it is; change it first if you want questions.
            </p>
            <p>
              What you allowed for this chat in the sandbox ("Allow for this chat") does not carry over: those calls are asked
              again. You can switch back to the sandbox whenever nothing of the chat runs.
            </p>
          </>
        ) : (
          <p>
            The approval card is then the only check, and only in Ask: in Auto (the default for new chats) every command
            runs as the node's user with no question asked. Choose Ask beside the folder, or switch the chat to it at any
            time. You can switch the chat back to the sandbox while nothing of it runs.
          </p>
        )}
        <div className="row end">
          <button type="button" autoFocus onClick={cancel}>
            Cancel
          </button>
          <button type="button" className="danger-fill" onClick={confirm}>
            {g ? "Grant host" : props.switching ? "Switch to the host shell" : "Use the host shell"}
          </button>
        </div>
      </div>
    </dialog>
  );
}

export default HostWarning;
