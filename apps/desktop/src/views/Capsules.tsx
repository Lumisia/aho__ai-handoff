import { useEffect, useState } from "react";
import {
  ChevronDown,
  Copy,
  Edit3,
  ExternalLink,
  FolderOpen,
  MoreHorizontal,
  Trash2,
} from "lucide-react";
import { readCapsule, setCapsuleField, setCapsuleState } from "../api";
import type { CapsuleList, CapsuleSummary, ReadTextResult } from "../types";
import type { Translator } from "../i18n";

const fields = [
  { id: "goal", labelKey: "goal" },
  { id: "next_prompt", labelKey: "nextPrompt" },
  { id: "remaining", labelKey: "remaining" },
  { id: "done", labelKey: "done" },
  { id: "risks", labelKey: "risks" },
];

export const capsuleStates = [
  "pending",
  "in_progress",
  "blocked",
  "needs_review",
  "consumed",
  "archived",
];

const stateLabelKeys: Record<string, string> = {
  pending: "statePending",
  in_progress: "stateInProgress",
  blocked: "stateBlocked",
  needs_review: "stateNeedsReview",
  consumed: "stateConsumed",
  archived: "stateArchived",
};

export function stateLabel(t: Translator, state: string) {
  return t(stateLabelKeys[state] ?? state);
}

export default function Capsules({
  initial,
  selectedPath,
  onSelectedPathChange,
  onChanged,
  onDeleteCapsule,
  onCopyPath,
  onOpenFolder,
  onOpenWith,
  t,
}: {
  initial: CapsuleList;
  selectedPath?: string | null;
  onSelectedPathChange: (path: string | null) => void;
  onChanged: () => void | Promise<void>;
  onDeleteCapsule: (item: CapsuleSummary) => void;
  onCopyPath: (item: CapsuleSummary) => void;
  onOpenFolder: (item: CapsuleSummary) => void;
  onOpenWith: (item: CapsuleSummary) => void;
  t: Translator;
}) {
  const [raw, setRaw] = useState<ReadTextResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [field, setField] = useState(fields[0].id);
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const [editing, setEditing] = useState(false);
  const [stateOpen, setStateOpen] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const selected =
    initial.items.find((item) => item.path === selectedPath) ?? initial.items[0] ?? null;

  useEffect(() => {
    if (!selectedPath && initial.items[0]) onSelectedPathChange(initial.items[0].path);
    if (selectedPath && !initial.items.some((item) => item.path === selectedPath)) {
      onSelectedPathChange(initial.items[0]?.path ?? null);
    }
  }, [initial, onSelectedPathChange, selectedPath]);

  useEffect(() => {
    setEditing(false);
    setStateOpen(false);
    setMenuOpen(false);
    setValue("");
    if (!selected) {
      setRaw(null);
      return;
    }
    let cancelled = false;
    setError(null);
    readCapsule(selected.path)
      .then((result) => {
        if (!cancelled) setRaw(result);
      })
      .catch((err) => {
        if (!cancelled) {
          setRaw(null);
          setError(err instanceof Error ? err.message : String(err));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [selected?.path]);

  async function runAction(action: (selected: CapsuleSummary) => Promise<void>) {
    if (!selected) return;
    setBusy(true);
    setError(null);
    try {
      await action(selected);
      await onChanged();
      setRaw(await readCapsule(selected.path));
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }

  function runMenuAction(action: (item: CapsuleSummary) => void) {
    if (!selected) return;
    setMenuOpen(false);
    action(selected);
  }

  return (
    <div className="capsule-detail-layout view">
      {!selected && (
        <section className="panel capsule-detail-panel">
          <div className="empty">{t("selectCapsule")}</div>
        </section>
      )}
      {selected && (
        <>
          <div className="capsule-detail-topline">
            <div className="capsule-status-wrap">
              <span className="capsule-status-label">{t("statusLabel")} :</span>
              <button
                className="status-chip"
                disabled={busy}
                title={t("changeState")}
                onClick={() => {
                  setMenuOpen(false);
                  setStateOpen((open) => !open);
                }}
              >
                <span>{stateLabel(t, selected.state)}</span>
                <ChevronDown size={14} />
              </button>
              {stateOpen && (
                <div className="capsule-state-menu">
                  {capsuleStates.map((item) => (
                    <button
                      key={item}
                      className={item === selected.state ? "active" : undefined}
                      disabled={busy}
                      onClick={() => {
                        setStateOpen(false);
                        void runAction(async (capsule) => {
                          await setCapsuleState(capsule.path, item);
                        });
                      }}
                    >
                      {stateLabel(t, item)}
                    </button>
                  ))}
                </div>
              )}
            </div>
            <div className="capsule-detail-actions">
              <button className="ghost" disabled={busy} onClick={() => setEditing((open) => !open)}>
                <Edit3 size={15} />
                <span>{t("editCapsule")}</span>
              </button>
              <div className="capsule-more-wrap">
                <button
                  className="icon-button"
                  title={t("moreActions")}
                  disabled={busy}
                  onClick={() => {
                    setStateOpen(false);
                    setMenuOpen((open) => !open);
                  }}
                >
                  <MoreHorizontal size={18} />
                </button>
                {menuOpen && (
                  <div className="capsule-detail-menu">
                    <button className="danger-item" onClick={() => runMenuAction(onDeleteCapsule)}>
                      <Trash2 size={15} />
                      <span>{t("deleteCapsule")}</span>
                    </button>
                    <button onClick={() => runMenuAction(onCopyPath)}>
                      <Copy size={15} />
                      <span>{t("copyPath")}</span>
                    </button>
                    <button onClick={() => runMenuAction(onOpenFolder)}>
                      <FolderOpen size={15} />
                      <span>{t("openFolder")}</span>
                    </button>
                    <button onClick={() => runMenuAction(onOpenWith)}>
                      <ExternalLink size={15} />
                      <span>{t("openWith")}</span>
                    </button>
                  </div>
                )}
              </div>
            </div>
          </div>
          <section className="panel capsule-detail-panel">
            <div className="capsule-title-stack">
              <h3>{selected.summary_preview || selected.capsule_id}</h3>
              <p>
                {selected.source_agent} -&gt; {selected.target_agent}
              </p>
            </div>
            {error && <p className="inline-error">{error}</p>}
            {editing && (
              <div className="editor-row">
                <select value={field} onChange={(event) => setField(event.target.value)}>
                  {fields.map((item) => (
                    <option value={item.id} key={item.id}>
                      {t(item.labelKey)}
                    </option>
                  ))}
                </select>
                <input
                  value={value}
                  onChange={(event) => setValue(event.target.value)}
                  placeholder={t("fieldPlaceholder")}
                />
                <button
                  disabled={busy || !value.trim()}
                  onClick={() =>
                    void runAction(async (item) => {
                      await setCapsuleField(item.path, field, value);
                      setValue("");
                    })
                  }
                >
                  {t("saveField")}
                </button>
              </div>
            )}
            <pre>{raw?.text ?? t("rawPlaceholder")}</pre>
          </section>
        </>
      )}
    </div>
  );
}
