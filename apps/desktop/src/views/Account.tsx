import { useEffect, useState } from "react";
import {
  captureCurrentAccount,
  deleteAccountSlot,
  getAccountReport,
  launchAccountSlot,
  pollAccountLogin,
  refreshAccountSlotUsage,
  startAccountLogin,
  switchAccountSlot,
} from "../api";
import type {
  AccountAgentReport,
  AccountLoginSession,
  AccountReport,
  AccountWindow,
  DashboardSnapshot,
  SlotUsageReport,
} from "../types";
import type { Translator } from "../i18n";

type AgentId = "codex" | "claude";

function pct(value: number) {
  return `${Math.round(value)}%`;
}

function resetText(window: AccountWindow | null | undefined, t: Translator) {
  if (!window?.resets_at) return t("resetUnknown");
  return new Date(window.resets_at * 1000).toLocaleString();
}

function dateText(value: string) {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
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
    <div className="account-limit">
      <div>
        <span>{label}</span>
        <strong>{value ? `${pct(value.remaining_percent)} ${t("left")}` : t("noSample")}</strong>
      </div>
      <div className={`usage-bar ${agent}`}>
        <span style={{ width: `${used}%` }} />
      </div>
      <small>{resetText(value, t)}</small>
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

  async function addAccount() {
    try {
      const session = await startAccountLogin(agent);
      setLogin(session);
      setMessage(session.message);
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    }
  }

  async function capture() {
    try {
      onRefresh(await captureCurrentAccount(agent));
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

  async function launch(label: string) {
    try {
      const result = await launchAccountSlot(agent, label);
      setMessage(result.message);
      onRefresh(result.report);
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    }
  }

  async function refreshUsage(label: string) {
    setUsageBusy(label);
    try {
      const result = await refreshAccountSlotUsage(agent, label);
      setUsage((current) => ({ ...current, [label]: result }));
      setMessage(`${title} "${label}" usage refreshed.`);
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
    <section className="panel account-panel">
      <div className="panel-title">
        <div>
          <h3>{title}</h3>
            <p>{data.active ? `${t("active")}: ${data.active}` : t("noActiveSlot")}</p>
        </div>
        <div className="actions">
          <button className="ghost" onClick={addAccount} disabled={login !== null}>
            {t("addAccount")}
          </button>
          <button className="ghost" onClick={capture}>
            {t("captureCurrent")}
          </button>
        </div>
      </div>
      {message && <div className="banner">{message}</div>}

      <div className="limit-stack">
        <LimitBar agent={agent} label="5h" value={data.five_hour} t={t} />
        <LimitBar agent={agent} label="Weekly" value={data.weekly} t={t} />
      </div>
      {data.usage_source === "none" && <div className="empty">{t("noSampleHint")}</div>}

      <div className="path-strip">
        <span>{t("vaultRoot")}</span>
        <code>{data.root}</code>
      </div>

      <div className="slot-list">
        {data.slots.length === 0 && <div className="empty">{t("noActiveSlot")}</div>}
        {data.slots.map((slot) => {
          const slotUsage = usage[slot.label];
          return (
            <article className={`slot-row ${slot.active ? "active" : ""}`} key={slot.label}>
              <div>
                <strong>{slot.email ?? slot.label}</strong>
                <span>{slot.plan ?? slot.source ?? t("savedAccount")}</span>
                <code>{slot.path}</code>
                {slotUsage && (
                  <div className="slot-usage">
                    <div className="slot-usage-meta">
                      <span>{t("plan")}: {slotUsage.plan ?? slot.plan ?? "unknown"}</span>
                      {slotUsage.reset_credits !== null && slotUsage.reset_credits !== undefined && (
                        <span>{t("resetCredits")}: {slotUsage.reset_credits}</span>
                      )}
                    </div>
                    <LimitBar agent={agent} label="5h" value={slotUsage.five_hour} t={t} />
                    <LimitBar agent={agent} label="Weekly" value={slotUsage.weekly} t={t} />
                    {slotUsage.reset_credit_details.length > 0 && (
                      <div className="credit-list">
                        {slotUsage.reset_credit_details.slice(0, 3).map((credit) => (
                          <small key={`${credit.granted_at}-${credit.expires_at}`}>
                            {dateText(credit.expires_at)}
                          </small>
                        ))}
                      </div>
                    )}
                  </div>
                )}
              </div>
              <div className="actions">
                <button
                  className="ghost"
                  disabled={usageBusy === slot.label}
                  onClick={() => refreshUsage(slot.label)}
                >
                  {t("usageButton")}
                </button>
                <button className="ghost" onClick={() => launch(slot.label)}>
                  {t("launchCli")}
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

export default function Account({ snapshot, t }: { snapshot: DashboardSnapshot; t: Translator }) {
  const [report, setReport] = useState<AccountReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const accountRoot = `${snapshot.paths.ai_home}\\accounts`;

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

  return (
    <div className="view">
      <section className="panel">
        <div className="panel-title">
          <div>
            <h3>{t("accounts")}</h3>
            <p>{t("accountsHelp")}</p>
          </div>
          <button className="refresh" onClick={() => void loadAccounts(true)}>
            {t("refresh")}
          </button>
        </div>
        <div className="detail-grid">
          <span>{t("accountRoot")}</span>
          <code>{accountRoot}</code>
          <span>{t("launcher")}</span>
          <code>{snapshot.install_state.launcher ?? t("notInstalled")}</code>
          <span>{t("installed")}</span>
          <code>{snapshot.install_state.installed_at || "missing"}</code>
        </div>
      </section>

      {error && <section className="banner error">{error}</section>}
      {loading && <section className="loading-screen">{t("loadingAccounts")}</section>}
      {report && (
        <div className="account-grid">
          <AgentPanel agent="codex" data={report.codex} onRefresh={setReport} onError={setError} t={t} />
          <AgentPanel agent="claude" data={report.claude} onRefresh={setReport} onError={setError} t={t} />
        </div>
      )}
    </div>
  );
}
