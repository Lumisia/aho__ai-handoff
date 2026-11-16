import { FolderOpen, RefreshCw } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
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
  DashboardSnapshot,
  SlotUsageReport,
} from "../types";
import type { Translator } from "../i18n";
import { LimitBar, ResetCreditsBlock } from "../components/AccountUsage";
import type { AgentId } from "../components/AccountUsage";

function knownPlan(...values: (string | null | undefined)[]) {
  for (const value of values) {
    const plan = value?.trim();
    if (plan && plan.toLowerCase() !== "unknown") return plan;
  }
  return null;
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
  // A new account report invalidates cached values and in-flight slot lookups.
  const [usageState, setUsageState] = useState<{
    report: AccountAgentReport;
    values: Record<string, SlotUsageReport>;
  }>({ report: data, values: {} });
  const usage = usageState.report === data ? usageState.values : {};
  const [usageBusy, setUsageBusy] = useState<string | null>(null);
  const usageGeneration = useRef(0);
  const activeSlot = data.slots.find((slot) => slot.active);
  const activeSlotLabel = activeSlot?.label;
  const activeUsage = activeSlot ? usage[activeSlot.label] ?? data.active_usage : null;
  const displayFiveHour = activeUsage ? activeUsage.five_hour : data.five_hour;
  const displayWeekly = activeUsage ? activeUsage.weekly : data.weekly;
  const activeDisplay = activeSlot?.email ?? activeSlot?.label ?? data.active;
  // Local samples may belong to a previous account; only provider reports are bound.
  const reportPlan = data.usage_source === "provider_api" ? data.plan : null;
  const activePlan = knownPlan(activeUsage?.plan, activeSlot?.plan, reportPlan);

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
    const generation = ++usageGeneration.current;
    setUsageState({ report: data, values: {} });
    setUsageBusy(null);

    // A forced report already includes the provider's response. Otherwise,
    // query the active saved slot using its own credential, once per report.
    if (activeSlotLabel && data.usage_source !== "provider_api") {
      setUsageBusy(activeSlotLabel);
      refreshAccountSlotUsage(agent, activeSlotLabel)
        .then((result) => {
          if (usageGeneration.current === generation) {
            setUsageState((current) => ({
              report: data,
              values: { ...current.values, [activeSlotLabel]: result },
            }));
          }
        })
        .catch(() => {
          // Keep the local sample when provider usage is unavailable.
        })
        .finally(() => {
          if (usageGeneration.current === generation) {
            setUsageBusy((current) => (current === activeSlotLabel ? null : current));
          }
        });
    }
    return () => {
      ++usageGeneration.current;
    };
  }, [agent, data, activeSlotLabel]);

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
    const generation = usageGeneration.current;
    setUsageBusy(label);
    try {
      const result = await refreshAccountSlotUsage(agent, label);
      if (usageGeneration.current === generation) {
        setUsageState((current) => ({
          report: data,
          values: { ...current.values, [label]: result },
        }));
      }
    } catch (err) {
      if (usageGeneration.current === generation) {
        onError(err instanceof Error ? err.message : String(err));
      }
    } finally {
      if (usageGeneration.current === generation) {
        setUsageBusy((current) => (current === label ? null : current));
      }
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
          {activeDisplay && activePlan && <p>{`${t("plan")}: ${activePlan}`}</p>}
        </div>
        <div className="account-actions">
          <button className="ghost" onClick={addAccount} disabled={login !== null}>
            {t("addAccount")}
          </button>
        </div>
      </div>

      {activeSlot && (
        <div className="account-limit-stack">
          <LimitBar agent={agent} label="5h" value={displayFiveHour} t={t} />
          <LimitBar agent={agent} label={t("weekly")} value={displayWeekly} t={t} />
          <ResetCreditsBlock agent={agent} usage={activeUsage} t={t} />
        </div>
      )}

      {message && <div className="banner">{message}</div>}
      {activeSlot && !displayFiveHour && !displayWeekly && <div className="empty">{t("noSampleHint")}</div>}

      <div className="slot-list">
        {data.slots.length === 0 && <div className="empty">{t("noActiveSlot")}</div>}
        {data.slots.map((slot) => {
          const slotUsage = usage[slot.label] ?? (slot.active ? data.active_usage : null);
          const slotPlan = knownPlan(slotUsage?.plan, slot.plan, slot.active ? reportPlan : null);
          return (
            <article className={`slot-row ${slot.active ? "active" : ""}`} key={slot.label}>
              <div className="slot-main">
                <div className="slot-name-line">
                  <strong>{slot.email ?? slot.label}</strong>
                  {slot.active && <span>{t("active")}</span>}
                </div>
                {slotPlan && <small>{`${t("plan")}: ${slotPlan}`}</small>}
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
              {slotUsage && (
                <div className="slot-usage">
                  <LimitBar agent={agent} label="5h" value={slotUsage.five_hour} t={t} />
                  <LimitBar agent={agent} label={t("weekly")} value={slotUsage.weekly} t={t} />
                  <ResetCreditsBlock agent={agent} usage={slotUsage} t={t} />
                </div>
              )}
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
  const reportGeneration = useRef(0);

  useEffect(() => {
    void loadAccounts(false);
    return () => { ++reportGeneration.current; };
  }, []);

  async function loadAccounts(force: boolean) {
    const generation = ++reportGeneration.current;
    setLoading(true);
    setError(null);
    try {
      const next = await getAccountReport({ force });
      if (reportGeneration.current === generation) setReport(next);
    } catch (err) {
      if (reportGeneration.current === generation) setError(err instanceof Error ? err.message : String(err));
    } finally {
      if (reportGeneration.current === generation) setLoading(false);
    }
  }

  const acceptAccountReport = useCallback((next: AccountReport) => {
    ++reportGeneration.current;
    setReport(next);
    setLoading(false);
    setError(null);
  }, []);

  async function openFolder() {
    try {
      await openAccountsFolder();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  return (
    <div className="view account-view">
      <header className="account-page-head">
        <h2>{t("accounts")}</h2>
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
      </header>

      {error && <section className="banner error">{error}</section>}
      {loading && !report && <section className="loading-screen">{t("loadingAccounts")}</section>}
      {report && (
        <div className="account-grid">
          <AgentPanel agent="codex" data={report.codex} onRefresh={acceptAccountReport} onError={setError} t={t} />
          <AgentPanel agent="claude" data={report.claude} onRefresh={acceptAccountReport} onError={setError} t={t} />
        </div>
      )}
    </div>
  );
}
