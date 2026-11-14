// "Export as Markdown": a small dialog, loaded on the first click, that
// builds the file from the chat's fold in the browser (lib/exportChat.ts).
import "../lib/i18nPages";
import { useEffect, useRef, useState } from "react";
import { chatMarkdown, download, fileName } from "../lib/exportChat";
import { t } from "../lib/i18n";
import type { SessionView } from "../types";

export default function ExportChat(props: {
  view: SessionView;
  title: string;
  agentName: (id: string | null | undefined) => string;
  onClose: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [outputs, setOutputs] = useState(false);
  useEffect(() => {
    const d = dialog.current;
    if (d && typeof d.showModal === "function" && !d.open) d.showModal();
  }, []);
  const close = () => {
    dialog.current?.close?.();
    props.onClose();
  };
  const save = () => {
    download(chatMarkdown(props.view, props.title, { outputs, agentName: props.agentName }), fileName(props.title));
    close();
  };
  return (
    <dialog
      ref={dialog}
      className="export-chat"
      aria-label={t("conv.export")}
      onClose={props.onClose}
      onClick={(e) => e.target === dialog.current && close()}
    >
      <div className="media-confirm stack">
        <p>{t("conv.exportWhat")}</p>
        <label className="row">
          <input type="checkbox" checked={outputs} onChange={(e) => setOutputs(e.target.checked)} />
          <span>{t("conv.exportOutputs")}</span>
        </label>
        <div className="row end">
          <button type="button" onClick={close}>
            {t("workspace.cancel")}
          </button>
          <button type="button" className="primary" autoFocus onClick={save}>
            {t("conv.exportSave")}
          </button>
        </div>
      </div>
    </dialog>
  );
}
