import { useEffect, useRef, useState } from "react";
import { ChevronDown, Copy, Edit3, ExternalLink, FolderOpen, MoreHorizontal, Trash2 } from "lucide-react";
import { ClaudeIcon, OpenAIIcon } from "../components/BrandIcons";
import { readCapsule, readCapsuleEditor, saveCapsule, setCapsuleState } from "../api";
import type { CapsuleList, CapsuleSummary, ReadTextResult } from "../types";
import type { Translator } from "../i18n";

type Agent = "codex" | "claude-code";
type Route = { source?: Agent; target?: Agent };
type OpenMenu = "state" | "more" | "source" | "target" | null;

export const capsuleStates = ["pending", "in_progress", "blocked", "needs_review", "consumed", "archived"];
const stateLabelKeys: Record<string, string> = {
  pending: "statePending", in_progress: "stateInProgress", blocked: "stateBlocked",
  needs_review: "stateNeedsReview", consumed: "stateConsumed", archived: "stateArchived",
};
export function stateLabel(t: Translator, state: string) { return t(stateLabelKeys[state] ?? state); }

function AgentLabel({ agent, t }: { agent: string; t: Translator }) {
  if (agent === "codex") return <><span className="agent-logo codex" aria-hidden="true"><OpenAIIcon size={16} /></span><span>{t("codex")}</span></>;
  if (agent === "claude" || agent === "claude-code") return <><span className="agent-logo claude" aria-hidden="true"><ClaudeIcon size={16} /></span><span>{t("claude")}</span></>;
  return <span>{agent}</span>;
}
function draftAgent(draft: string, field: "source_agent" | "target_agent", fallback: string) {
  try {
    const value: unknown = JSON.parse(draft)?.[field];
    return value === null ? "any" : typeof value === "string" ? value : fallback;
  } catch { return fallback; }
}

export default function Capsules({
  initial, selectedPath, onSelectedPathChange, onChanged, onDeleteCapsule, onCopyPath,
  onOpenFolder, onOpenWith, t,
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
  const [draft, setDraft] = useState("");
  const [route, setRoute] = useState<Route>({});
  const [busy, setBusy] = useState(false);
  const [editing, setEditing] = useState(false);
  const [openMenu, setOpenMenu] = useState<OpenMenu>(null);
  const stateWrap = useRef<HTMLDivElement>(null);
  const moreWrap = useRef<HTMLDivElement>(null);
  const sourceWrap = useRef<HTMLDivElement>(null);
  const targetWrap = useRef<HTMLDivElement>(null);
  const operation = useRef(0);
  const busyRef = useRef(false);
  const selected = initial.items.find((item) => item.path === selectedPath) ?? initial.items[0] ?? null;
  const currentPath = useRef(selected?.path);
  currentPath.current = selected?.path;

  useEffect(() => {
    if (!selectedPath && initial.items[0]) onSelectedPathChange(initial.items[0].path);
    if (selectedPath && !initial.items.some((item) => item.path === selectedPath)) onSelectedPathChange(initial.items[0]?.path ?? null);
  }, [initial, onSelectedPathChange, selectedPath]);
  useEffect(() => {
    const token = ++operation.current;
    busyRef.current = false;
    setBusy(false);
    setEditing(false);
    setOpenMenu(null);
    setDraft("");
    setRoute({});
    setRaw(null);
    setError(null);
    if (!selected) return;
    const path = selected.path;
    readCapsule(path).then((result) => {
      if (operation.current === token && currentPath.current === path) setRaw(result);
    }).catch((err) => {
      if (operation.current === token && currentPath.current === path) setError(String(err));
    });
    return () => { operation.current++; };
  }, [selected?.path]);
  useEffect(() => {
    if (!openMenu) return;
    const wrap = { state: stateWrap, more: moreWrap, source: sourceWrap, target: targetWrap }[openMenu];
    const outside = (event: PointerEvent) => {
      if (!wrap.current?.contains(event.target as Node)) setOpenMenu(null);
    };
    const escape = (event: KeyboardEvent) => { if (event.key === "Escape") setOpenMenu(null); };
    document.addEventListener("pointerdown", outside);
    document.addEventListener("keydown", escape);
    return () => {
      document.removeEventListener("pointerdown", outside);
      document.removeEventListener("keydown", escape);
    };
  }, [openMenu]);
  function toggleMenu(menu: OpenMenu) { setOpenMenu((current) => current === menu ? null : menu); }

  async function retryPreview() {
    if (!selected || busyRef.current || editing) return;
    const path = selected.path;
    const token = ++operation.current;
    busyRef.current = true;
    setBusy(true);
    setRaw(null);
    setError(null);
    try {
      const result = await readCapsule(path);
      if (operation.current === token && currentPath.current === path) setRaw(result);
    } catch (err) {
      if (operation.current === token && currentPath.current === path) setError(String(err));
    } finally {
      if (operation.current === token && currentPath.current === path) {
        busyRef.current = false;
        setBusy(false);
      }
    }
  }

  async function beginEditing() {
    if (!selected || !raw || raw.truncated || raw.error || busyRef.current) return;
    const path = selected.path;
    const token = ++operation.current;
    busyRef.current = true;
    setBusy(true);
    setOpenMenu(null);
    setError(null);
    try {
      const content = await readCapsuleEditor(path, raw.text);
      if (operation.current === token && currentPath.current === path) {
        setDraft(content);
        setRoute({});
        setEditing(true);
      }
    } catch (err) {
      if (operation.current === token && currentPath.current === path) setError(String(err));
    } finally {
      if (operation.current === token && currentPath.current === path) {
        busyRef.current = false;
        setBusy(false);
      }
    }
  }
  async function runAction(action: (item: CapsuleSummary) => Promise<void>, finishEditing = false) {
    if (!selected || busyRef.current) return;
    const path = selected.path;
    const token = ++operation.current;
    busyRef.current = true;
    setBusy(true);
    setError(null);
    setOpenMenu(null);
    try {
      await action(selected);
      if (operation.current === token && currentPath.current === path) {
        // A successful write has committed, even if refreshing the preview fails.
        if (finishEditing) setEditing(false);
        setRaw(null);
      }
      await onChanged();
      const result = await readCapsule(path);
      if (operation.current === token && currentPath.current === path) setRaw(result);
    } catch (err) {
      if (operation.current === token && currentPath.current === path) setError(String(err));
    } finally {
      if (operation.current === token && currentPath.current === path) {
        busyRef.current = false;
        setBusy(false);
      }
    }
  }
  function selectAgent(which: "source" | "target", agent: Agent) {
    setRoute((current) => ({ ...current, [which]: agent }));
    // Preserve malformed drafts; the native save command reports their location.
    try {
      const content = JSON.parse(draft);
      if (content && typeof content === "object" && !Array.isArray(content)) {
        content[`${which}_agent`] = agent;
        setDraft(JSON.stringify(content, null, 2));
      }
    } catch { /* Selecting a route must not discard incomplete body edits. */ }
    setOpenMenu(null);
  }
  function runMenuAction(action: (item: CapsuleSummary) => void) {
    if (!selected) return;
    setOpenMenu(null);
    action(selected);
  }
  function agentSelect(which: "source" | "target") {
    if (!selected) return null;
    const fallback = which === "source" ? selected.source_agent : selected.target_agent;
    const agent = route[which] ?? draftAgent(draft, `${which}_agent`, fallback);
    return <div className="capsule-agent-select" ref={which === "source" ? sourceWrap : targetWrap}>
      <button className="status-chip" aria-label={t(which === "source" ? "capsuleSourceAgent" : "capsuleTargetAgent")} aria-expanded={openMenu === which} disabled={busy} onClick={() => toggleMenu(which)}><AgentLabel agent={agent} t={t} /><ChevronDown size={14} /></button>
      {openMenu === which && <div className="capsule-state-menu capsule-agent-menu">{(["codex", "claude-code"] as const).map((option) => <button key={option} className={agent === option ? "active" : undefined} onClick={() => selectAgent(which, option)}><AgentLabel agent={option} t={t} /></button>)}</div>}
    </div>;
  }
  return <div className="capsule-detail-layout view">
    {!selected && <section className="panel capsule-detail-panel"><div className="empty">{t("selectCapsule")}</div></section>}
    {selected && <>
      <div className="capsule-detail-topline">
        <div className="capsule-status-wrap" ref={stateWrap}>
          <span className="capsule-status-label">{t("statusLabel")} :</span>
          <button className="status-chip" disabled={busy || editing} title={t("changeState")} aria-expanded={openMenu === "state"} onClick={() => toggleMenu("state")}><span>{stateLabel(t, selected.state)}</span><ChevronDown size={14} /></button>
          {openMenu === "state" && <div className="capsule-state-menu">{capsuleStates.map((state) => <button key={state} className={state === selected.state ? "active" : undefined} disabled={busy} onClick={() => void runAction(async (item) => { await setCapsuleState(item.path, state); })}>{stateLabel(t, state)}</button>)}</div>}
        </div>
        <div className="capsule-detail-actions">
          {editing ? <>
            <button disabled={busy || !draft.trim()} onClick={() => { if (raw) void runAction((item) => saveCapsule(item.path, draft, raw.text, route), true); }}>{t("save")}</button>
            <button className="ghost" disabled={busy} onClick={() => { setEditing(false); setDraft(""); setRoute({}); setOpenMenu(null); setError(null); }}>{t("cancel")}</button>
          </> : <button className="ghost" disabled={busy || !raw || raw.truncated || Boolean(raw.error)} onClick={() => void beginEditing()}><Edit3 size={15} /><span>{busy ? t("loadingCapsuleEditor") : t("editCapsule")}</span></button>}
          <div className="capsule-more-wrap" ref={moreWrap}>
            <button className="icon-button" title={t("moreActions")} aria-expanded={openMenu === "more"} disabled={busy || editing} onClick={() => toggleMenu("more")}><MoreHorizontal size={18} /></button>
            {openMenu === "more" && <div className="capsule-detail-menu">
              <button className="danger-item" onClick={() => runMenuAction(onDeleteCapsule)}><Trash2 size={15} /><span>{t("deleteCapsule")}</span></button>
              <button onClick={() => runMenuAction(onCopyPath)}><Copy size={15} /><span>{t("copyPath")}</span></button>
              <button onClick={() => runMenuAction(onOpenFolder)}><FolderOpen size={15} /><span>{t("openFolder")}</span></button>
              <button onClick={() => runMenuAction(onOpenWith)}><ExternalLink size={15} /><span>{t("openWith")}</span></button>
            </div>}
          </div>
        </div>
      </div>
      <section className="panel capsule-detail-panel">
        <div className="capsule-title-stack">
          <h3>{selected.summary_preview || selected.capsule_id}</h3>
          {editing ? <div className="capsule-route-editor">{agentSelect("source")}<span aria-hidden="true">→</span>{agentSelect("target")}</div> : <p>{selected.source_agent} -&gt; {selected.target_agent}</p>}
        </div>
        {(error || raw?.error) && <p className="inline-error" role="alert">{error || raw?.error}</p>}
        {!editing && !busy && (error || raw?.error) && <button className="ghost" onClick={() => void retryPreview()}>{t("refresh")}</button>}
        {editing ? <textarea className="capsule-raw-editor" aria-label={t("capsuleRawEditor")} value={draft} spellCheck={false} disabled={busy} onChange={(event) => { setDraft(event.target.value); setRoute({}); }} /> : <pre>{raw?.text ?? t("rawPlaceholder")}</pre>}
      </section>
    </>}
  </div>;
}
