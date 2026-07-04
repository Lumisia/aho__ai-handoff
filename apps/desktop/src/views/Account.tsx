import { FolderOpen, RefreshCw } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import {
  deleteAccountSlot,
  getAccountReport,
  openAccountsFolder,
  pollAccountLogin,
  refreshAccountSlotUsage,
  startAccountLogin,
  switchAccountSlot,
} from "../api";
import type {
  AccountAgentReport,
  AccountLoginSession,
  AccountReport,
  AccountSlotRow,
  AccountWindow,
  DashboardSnapshot,
  ResetCreditRow,
  SlotUsageReport,
} from "../types";
import type { Translator } from "../i18n";

type AgentId = "codex" | "claude";

function pct(value: number) {
  return `${Math.round(value)}%`;
}

function compactDateFromSeconds(value: number) {
  return new Date(value * 1000).toLocaleString(undefined, {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function compactDate(value: string) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString(undefined, {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function resetSummary(
  fiveHour: AccountWindow | null | undefined,
  weekly: AccountWindow | null | undefined,
  t: Translator,
) {
  const values = [fiveHour?.resets_at, weekly?.resets_at]
    .filter((value): value is number => typeof value === "number")
    .map(compactDateFromSeconds);
  return values.length > 0 ? `${t("reset")} - ${values.join(" / ")}` : `${t("reset")} - ${t("resetUnknown")}`;
}

function creditSummary(credits: ResetCreditRow[] | undefined) {
  const first = credits?.[0];
  if (!first) return null;
  return `${compactDate(first.granted_at)} / ${compactDate(first.expires_at)}`;
}

function slotSubline(slot: AccountSlotRow, t: Translator) {
  const plan = slot.plan ? `${t("plan")}: ${slot.plan}` : `${t("plan")}: unknown`;
  return slot.source ? `${plan} - ${slot.source}` : plan;
}

function LimitBar({
  agent,
  label,
  value,
  t,
}: {
  agent: AgentId;
  label: string;
  value?: AccountWindow | null;
  t: Translator;
}) {
  const used = value ? Math.max(0, Math.min(100, value.used_percent)) : 0;
  return (
    <div className={`account-limit-row ${agent}`}>
      <strong>{label}</strong>
      <div className="usage-bar" aria-hidden="true">
        <span style={{ width: `${used}%` }} />
      </div>
      <span>{value ? `${pct(value.remaining_percent)} ${t("left")}` : t("noSample")}</span>
    </div>
  );
}

function AgentPanel({
  agent,
  data,
  onRefresh,
  onError,
  t,
}: {
  agent: AgentId;
  data: AccountAgentReport;
  onRefresh: (report: AccountReport) => void;
  onError: (error: string) => void;
  t: Translator;
}) {
  const title = agent === "codex" ? "Codex" : "Claude";
  const [login, setLogin] = useState<AccountLoginSession | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [usage, setUsage] = useState<Record<string, SlotUsageReport>>({});
  const [usageBusy, setUsageBusy] = useState<string | null>(null);
  const autoUsageTried = useRef<Set<string>>(new Set());
  const activeSlot = data.slots.find((slot) => slot.active);
  const activeSlotLabel = activeSlot?.label;
  const activeUsage = activeSlot ? usage[activeSlot.label] : null;
  const displayFiveHour = activeUsage?.five_hour ?? data.five_hour;
  const displayWeekly = activeUsage?.weekly ?? data.weekly;
  const activeCredits = agent === "codex" ? creditSummary(activeUsage?.reset_credit_details) : null;
  const activeDisplay = activeSlot?.email ?? activeSlot?.label ?? data.active;
  const activePlan = activeUsage?.plan ?? activeSlot?.plan ?? data.plan;

  useEffect(() => {
    if (!login) return;
    const timer = window.setInterval(() => {
      pollAccountLogin(agent, login.home)
        .then((poll) => {
          setMessage(poll.message);
          if (poll.done) {
            if (poll.report) onRefresh(poll.report);
            setLogin(null);
          }
        })
        .catch((err) => {
          setMessage(err instanceof Error ? err.message : String(err));
          setLogin(null);
        });
    }, 2000);
    return () => window.clearInterval(timer);
  }, [agent, login, onRefresh]);

  useEffect(() => {
    if (agent !== "codex" || !activeSlotLabel || autoUsageTried.current.has(activeSlotLabel)) return;
    let cancelled = false;
    autoUsageTried.current.add(activeSlotLabel);
    setUsageBusy(activeSlotLabel);
    refreshAccountSlotUsage(agent, activeSlotLabel)
      .then((result) => {
        if (!cancelled) {
          setUsage((current) => ({ ...current, [activeSlotLabel]: result }));
        }
      })
      .catch(() => {
        // Keep the local sample when provider usage is unavailable.
      })
      .finally(() => {
        if (!cancelled) {
          setUsageBusy((current) => (current === activeSlotLabel ? null : current));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [agent, activeSlotLabel]);

  async function addAccount() {
    try {
      const session = await startAccountLogin(agent);
      setLogin(session);
      setMessage(session.message);
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    }
  }

  async function switchSlot(label: string) {
    if (!window.confirm(`${title} active account will switch to "${label}". Continue?`)) return;
    try {
      onRefresh(await switchAccountSlot(agent, label));
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    }
  }

  async function refreshUsage(label: string) {
    setUsageBusy(label);
    try {
      const result = await refreshAccountSlotUsage(agent, label);
      setUsage((current) => ({ ...current, [label]: result }));
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    } finally {
      setUsageBusy(null);
    }
  }

  async function deleteSlot(label: string) {
    if (!window.confirm(`Delete saved ${title} account slot "${label}"?`)) return;
    try {
      onRefresh(await deleteAccountSlot(agent, label));
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    }
  }

  return (
    <section className={`panel account-panel ${agent}`}>
      <div className="account-panel-head">
        <div>
          <h3>{title}</h3>
          <p>{activeDisplay ? `${t("active")}: ${activeDisplay}` : t("noActiveSlot")}</p>
          {activeDisplay && <p>{`${t("plan")}: ${activePlan ?? "unknown"}`}</p>}
        </div>
        <div className="account-actions">
          <button className="ghost" onClick={addAccount} disabled={login !== null}>
            {t("addAccount")}
          </button>
        </div>
      </div>

      <div className="account-limit-stack">
        <LimitBar agent={agent} label="5h" value={displayFiveHour} t={t} />
        <LimitBar agent={agent} label={t("weekly")} value={displayWeekly} t={t} />
        <div className="account-reset-line">{resetSummary(displayFiveHour, displayWeekly, t)}</div>
        {activeCredits && (
          <div className="account-credit-summary">
            {t("resetCredits")} - {activeCredits}
          </div>
        )}
      </div>

      {message && <div className="banner">{message}</div>}
      {data.usage_source === "none" && <div className="empty">{t("noSampleHint")}</div>}

      <div className="slot-list">
        {data.slots.length === 0 && <div className="empty">{t("noActiveSlot")}</div>}
        {data.slots.map((slot) => {
          const slotUsage = usage[slot.label];
          return (
            <article className={`slot-row ${slot.active ? "active" : ""}`} key={slot.label}>
              <div className="slot-main">
                <div className="slot-name-line">
                  <strong>{slot.email ?? slot.label}</strong>
                  {slot.active && <span>{t("active")}</span>}
                </div>
                <small>{slotSubline(slot, t)}</small>
                {slotUsage && (
                  <div className="slot-usage">
                    <div className="slot-usage-meta">
                      <span>
                        {t("plan")}: {slotUsage.plan ?? slot.plan ?? "unknown"}
                      </span>
                      {slotUsage.reset_credits !== null && slotUsage.reset_credits !== undefined && (
                        <span>
                          {t("resetCredits")}: {slotUsage.reset_credits}
                        </span>
                      )}
                    </div>
                    <LimitBar agent={agent} label="5h" value={slotUsage.five_hour} t={t} />
                    <LimitBar agent={agent} label={t("weekly")} value={slotUsage.weekly} t={t} />
                    {slotUsage.reset_credit_details.length > 0 && (
                      <div className="credit-list">
                        {slotUsage.reset_credit_details.slice(0, 3).map((credit) => (
                          <small key={`${credit.granted_at}-${credit.expires_at}`}>
                            {compactDate(credit.granted_at)} / {compactDate(credit.expires_at)}
                          </small>
                        ))}
                      </div>
                    )}
                  </div>
                )}
              </div>
              <div className="account-actions slot-actions">
                <button className="ghost" disabled={usageBusy === slot.label} onClick={() => refreshUsage(slot.label)}>
                  {t("usageButton")}
                </button>
                <button className="ghost" disabled={slot.active} onClick={() => switchSlot(slot.label)}>
                  {t("switch")}
                </button>
                <button className="danger" onClick={() => deleteSlot(slot.label)}>
                  {t("delete")}
                </button>
              </div>
            </article>
          );
        })}
      </div>
    </section>
  );
}

export default function Account({ snapshot: _snapshot, t }: { snapshot: DashboardSnapshot; t: Translator }) {
  const [report, setReport] = useState<AccountReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    void loadAccounts(false);
  }, []);

  async function loadAccounts(force: boolean) {
    setLoading(true);
    setError(null);
    try {
      setReport(await getAccountReport({ force }));
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }

  async function openFolder() {
    try {
      await openAccountsFolder();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  return (
    <div className="view account-view">
      <div className="account-page-head">
        <h3>{t("accounts")}</h3>
        <div className="account-page-actions">
          <button className="ghost icon-text-button" onClick={() => void openFolder()}>
            <FolderOpen size={15} aria-hidden="true" />
            {t("openFolder")}
          </button>
          <button className="ghost icon-text-button" onClick={() => void loadAccounts(true)} disabled={loading}>
            <RefreshCw size={15} aria-hidden="true" />
            {loading ? t("loadingAccounts") : t("refresh")}
          </button>
        </div>
      </div>

      {error && <section className="banner error">{error}</section>}
      {loading && !report && <section className="loading-screen">{t("loadingAccounts")}</section>}
      {report && (
        <div className="account-grid">
          <AgentPanel agent="codex" data={report.codex} onRefresh={setReport} onError={setError} t={t} />
          <AgentPanel agent="claude" data={report.claude} onRefresh={setReport} onError={setError} t={t} />
        </div>
      )}
    </div>
  );
}
