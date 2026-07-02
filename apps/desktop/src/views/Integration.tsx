import { useEffect, useMemo, useState } from "react";
import { getCachedIntegrationReport, readLogs, runDoctor, runRepairAction } from "../api";
import type { CheckRow, DashboardSnapshot, IntegrationReport, LogFile, RepairAction } from "../types";
import type { Translator } from "../i18n";

function CheckCard({ row }: { row: CheckRow }) {
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

function commandText(action: RepairAction) {
  return action.command ? `ai-handoff ${action.command.join(" ")}` : "manual";
}

export default function Integration({ initial, t }: { initial: DashboardSnapshot; t: Translator }) {
  const [report, setReport] = useState<IntegrationReport | null>(() => getCachedIntegrationReport());
  const [logs, setLogs] = useState<LogFile[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [lastOutput, setLastOutput] = useState<string | null>(null);

  const snapshot = report?.snapshot ?? initial;
  const repairs = report?.repairs ?? [];
  const doctorLines = useMemo(() => report?.doctor.lines ?? [], [report]);

  useEffect(() => {
    readLogs().then(setLogs).catch(() => setLogs([]));
  }, []);

  async function doctor() {
    setBusy("doctor");
    setError(null);
    try {
      const next = await runDoctor();
      setReport(next);
      setLogs(await readLogs());
      setLastOutput(next.doctor.lines.join("\n"));
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(null);
    }
  }

  async function repair(action: RepairAction) {
    if (action.manual) return;
    if (action.requires_confirm && !window.confirm(`Run "${commandText(action)}"?`)) return;
    setBusy(action.id);
    setError(null);
    try {
      const result = await runRepairAction(action.id);
      setReport(result.report);
      setLogs(await readLogs());
      setLastOutput(
        [
          result.action.label,
          `exit: ${result.exit_code ?? "spawned"}`,
          result.output.trim() || "(no output)",
        ].join("\n"),
      );
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(null);
    }
  }

  return (
    <div className="integration-layout">
      <section className="panel">
        <div className="panel-title">
          <div>
            <h3>{t("doctor")}</h3>
            <p>{t("doctorHelp")}</p>
          </div>
          <button onClick={() => void doctor()} disabled={busy !== null}>
            {busy === "doctor" ? t("running") : t("runDoctor")}
          </button>
        </div>
        {error && <div className="banner error">{error}</div>}
        {busy === "doctor" && <section className="loading-screen">Running doctor...</section>}
        {report && (
          <div className="doctor-summary">
            <div>
              <span>Daemon</span>
              <strong>{report.doctor.daemon}</strong>
            </div>
            <div>
              <span>Checks</span>
              <strong>
                {report.doctor.ok}/{report.doctor.warn}/{report.doctor.fail}
              </strong>
            </div>
            <div>
              <span>Accounts</span>
              <strong>
                Cx {report.doctor.codex_accounts} / Cl {report.doctor.claude_accounts}
              </strong>
            </div>
            <div>
              <span>Elapsed</span>
              <strong>{report.doctor.elapsed_ms}ms</strong>
            </div>
          </div>
        )}
        {doctorLines.length > 0 && <pre>{doctorLines.join("\n")}</pre>}
        <div className="grid compact">
          {snapshot.checks.map((row) => (
            <CheckCard key={row.id} row={row} />
          ))}
        </div>
      </section>

      <section className="panel">
        <div className="panel-title">
          <div>
            <h3>{t("repairCenter")}</h3>
            <p>{t("repairHelp")}</p>
          </div>
          <span className="pill">{t("confirmRequired")}</span>
        </div>
        {repairs.length === 0 && <div className="empty">{t("runDoctor")}</div>}
        {repairs.map((action) => (
          <article className="repair-action" key={action.id}>
            <div>
              <strong>{action.label}</strong>
              <p>{action.detail}</p>
              <code>{commandText(action)}</code>
              <small>Recommended by: {action.recommended_by.join(", ")}</small>
            </div>
            <button
              className={action.manual ? "ghost" : "refresh"}
              disabled={action.manual || busy !== null}
              onClick={() => void repair(action)}
            >
              {action.manual ? t("manual") : busy === action.id ? t("running") : t("run")}
            </button>
          </article>
        ))}
      </section>

      <section className="panel">
        <div className="panel-title">
          <h3>{t("runOutput")}</h3>
        </div>
        <pre>{lastOutput ?? t("noRunOutput")}</pre>
      </section>

      <section className="panel">
        <div className="panel-title">
          <h3>{t("logs")}</h3>
          <button className="ghost" onClick={() => void readLogs({ force: true }).then(setLogs)}>
            {t("refresh")}
          </button>
        </div>
        <div className="log-grid">
          {logs.map((log) => (
            <article key={log.name}>
              <strong>{log.name}</strong>
              <pre>{log.result.error ?? log.result.text ?? "Empty log."}</pre>
            </article>
          ))}
        </div>
      </section>
    </div>
  );
}
