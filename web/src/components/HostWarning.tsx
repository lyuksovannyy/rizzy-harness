// The warning shown when someone picks the host shell for a new chat
// (N.10a; the owner, 2026-11-12: "display warning when selected and say the
// risks"). Its own chunk: only those who may pick `host` ever load it.
import { useEffect, useRef } from "react";

export function HostWarning(props: { node: string; onConfirm: () => void; onCancel: () => void }) {
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
        <h2 id="host-warning-title">Run this chat's commands without the sandbox?</h2>
        <p>In the host shell on {props.node}:</p>
        <ul>
          <li>the agent's commands run as the node's own user, with nothing between them and the machine;</li>
          <li>they can read and change every file that user can, its keys and other projects included;</li>
          <li>what they start can keep running after the chat ends;</li>
          <li>one web page or file the agent reads can turn all of that against you;</li>
          <li>what a sandbox chat wrote in this folder (a script, a Makefile, git hooks) runs here with all of that too.</li>
        </ul>
        <p>
          The approval card is then the only check, so this chat always asks before a command that can change things: it
          never runs in auto. The mode cannot be changed once the chat exists.
        </p>
        <div className="row end">
          <button type="button" autoFocus onClick={cancel}>
            Cancel
          </button>
          <button type="button" className="danger" onClick={confirm}>
            Use the host shell
          </button>
        </div>
      </div>
    </dialog>
  );
}

export default HostWarning;
