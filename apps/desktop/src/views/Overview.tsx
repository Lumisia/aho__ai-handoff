import { useEffect, useState } from "react";
import { getAccountReport } from "../api";
import type { AccountReport, AccountWindow, CheckRow, DashboardSnapshot } from "../types";
import type { Translator } from "../i18n";

function StatusCard({ row }: { row: CheckRow }) {
  return (
    <article className={`card ${row.status}`}>
      <div className="card-head">
        <span>{row.label}</span>
        <strong>{row.status}</strong>
      </div>
      <p>{row.message}</p>
      {row.path && <code>{row.path}</code>}
    </article>
  );
}

function pct(value: number) {
  return `${Math.round(value)}%`;
}

function LimitBar({
  agent,
  label,
  value,
  t,
}: {
  agent: "codex" | "claude";
  label: string;
  value?: AccountWindow | null;
  t: Translator;
}) {
  const used = value ? Math.max(0, Math.min(100, value.used_percent)) : 0;
  return (
    <article className="limit-card">
      <div>
        <strong>{label}</strong>
        <span>{value ? `${pct(value.remaining_percent)} ${t("left")}` : t("noSample")}</span>
      </div>
      <div className={`usage-bar ${agent}`}>
        <span style={{ width: `${used}%` }} />
      </div>
    </article>
  );
}

export default function Overview({ snapshot, t }: { snapshot: DashboardSnapshot; t: Translator }) {
  const [accounts, setAccounts] = useState<AccountReport | null>(null);
  const [loadingAccounts, setLoadingAccounts] = useState(true);

  useEffect(() => {
    getAccountReport()
      .then(setAccounts)
      .catch(() => setAccounts(null))
      .finally(() => setLoadingAccounts(false));
  }, []);

  const topRows = [
    snapshot.daemon,
    snapshot.autostart,
    snapshot.codex_hooks,
    snapshot.codex_config,
    snapshot.claude_settings,
    snapshot.ipc,
    snapshot.store,
  ];

  return (
    <div className="view">
      <section className="metrics">
        <div>
          <span>{t("pending")}</span>
          <strong>{snapshot.capsules.pending_count}</strong>
        </div>
        <div>
          <span>{t("totalCapsules")}</span>
          <strong>{snapshot.capsules.items.length}</strong>
        </div>
        <div>
          <span>{t("skippedFiles")}</span>
          <strong>{snapshot.capsules.skipped}</strong>
        </div>
        <div>
          <span>{t("autostart")}</span>
          <strong>{snapshot.install_state.autostart}</strong>
        </div>
      </section>
      <section className="panel">
        <div className="panel-title">
          <div>
            <h3>{t("agentLimits")}</h3>
            <p>{t("agentLimitsHelp")}</p>
          </div>
        </div>
        {loadingAccounts && <section className="loading-screen compact-loading">{t("loadingAccounts")}</section>}
        <div className="overview-limits">
          <LimitBar agent="claude" label={`${t("claude")} 5h`} value={accounts?.claude.five_hour} t={t} />
          <LimitBar agent="claude" label={`${t("claude")} weekly`} value={accounts?.claude.weekly} t={t} />
          <LimitBar agent="codex" label={`${t("codex")} 5h`} value={accounts?.codex.five_hour} t={t} />
          <LimitBar agent="codex" label={`${t("codex")} weekly`} value={accounts?.codex.weekly} t={t} />
        </div>
      </section>
      <section className="grid">
        {topRows.map((row) => (
          <StatusCard key={row.id} row={row} />
        ))}
      </section>
    </div>
  );
}
