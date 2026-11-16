import { useEffect, useRef, useState } from "react";
import { deleteCapsuleProject, getCapsuleProject } from "../api";
import type { Translator } from "../i18n";
import type { CapsuleProjectInfo } from "../types";

export default function ProjectDeleteDialog({ projectId, projectLabel, onClose, onDeleted, t }: {
  projectId: string;
  projectLabel: string;
  onClose: () => void;
  onDeleted: () => void;
  t: Translator;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [info, setInfo] = useState<CapsuleProjectInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const deleting = useRef(false);

  useEffect(() => {
    const element = dialog.current;
    element?.showModal();
    return () => element?.close();
  }, []);

  useEffect(() => {
    let cancelled = false;
    getCapsuleProject(projectId).then(
      (result) => { if (!cancelled) setInfo(result); },
      (err) => { if (!cancelled) setError(String(err)); },
    );
    return () => { cancelled = true; };
  }, [projectId]);

  async function confirmDelete() {
    if (!info || error || deleting.current) return;
    deleting.current = true;
    setBusy(true);
    try {
      await deleteCapsuleProject(projectId, info.revision);
    } catch (err) {
      setError(String(err));
      setBusy(false);
      deleting.current = false;
      return;
    }
    onDeleted();
  }

  return (
    <dialog
      ref={dialog}
      className="confirm-modal project-delete-dialog"
      role="alertdialog"
      aria-labelledby="project-delete-title"
      aria-describedby="project-delete-count"
      onCancel={(event) => {
        event.preventDefault();
        if (!busy) onClose();
      }}
    >
      <h3 id="project-delete-title">{t("confirmDeleteProject")}</h3>
      <p className="project-delete-name">{projectLabel}</p>
      <p id="project-delete-count" aria-live="polite">
        {info ? t("confirmDeleteProjectCount").replace("{count}", String(info.capsule_count)) : error ? null : t("loadingProjectCapsules")}
      </p>
      {error && <p className="inline-error" role="alert">{error}</p>}
      <div className="confirm-modal-actions">
        <button onClick={onClose} disabled={busy}>{t("cancel")}</button>
        <button className="update-danger-button" onClick={() => void confirmDelete()} disabled={busy || !info || !!error}>
          {t("deleteProject")}
        </button>
      </div>
    </dialog>
  );
}
