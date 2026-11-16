import type { AccountWindow, SlotUsageReport } from "../types";
import type { Translator } from "../i18n";
import { useEffect, useState } from "react";

export type AgentId = "codex" | "claude";

export function pct(value: number) {
  return `${Math.round(value)}%`;
}

export function compactDateFromSeconds(value: number) {
  return new Date(value * 1000).toLocaleString(undefined, {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function compactDate(value: string) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString(undefined, {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function resetCreditCount(usage?: SlotUsageReport | null) {
  const detailCount = usage?.reset_credit_details?.length ?? 0;
  const explicitCount = usage?.reset_credits;
  if (explicitCount === null || explicitCount === undefined) return detailCount;
  return Math.max(explicitCount, detailCount);
}

function resetCreditLabel(count: number, t: Translator) {
  return `${t("resetCredits")} ${count}${t("resetCreditCountSuffix")}`;
}

export function ResetCreditsBlock({ agent, usage, t }: { agent?: AgentId; usage?: SlotUsageReport | null; t: Translator }) {
  if (agent === "claude") return <ClaudeResetCredits usage={usage} t={t} />;
  const details = usage?.reset_credit_details ?? [];
  const count = resetCreditCount(usage);
  if (count <= 0 && details.length === 0) return null;
  return (
    <div className="reset-credit-block codex">
      <strong className="reset-credit-title">{resetCreditLabel(count, t)}</strong>
      {details.length > 0 && (
        <div className="credit-list">
          {details.map((credit, index) => (
            <small key={`${credit.kind}-${credit.expires_at}-${index}`}>
              {t(credit.kind === "full" ? "resetFull" : "resetOther")}
              {" / "}{credit.expires_at ? `${t("resetExpires")} ${compactDate(credit.expires_at)}` : t("resetExpiryUnknown")}
            </small>
          ))}
        </div>
      )}
    </div>
  );
}

function ClaudeResetCredits({ usage, t }: { usage?: SlotUsageReport | null; t: Translator }) {
  const [now, setNow] = useState(() => Date.now() / 1000);
  const inventory = usage?.claude_reset_credits;
  const credits = inventory?.filter((credit) =>
    (credit.starts_at == null || credit.starts_at <= now) &&
    (credit.expires_at == null || credit.expires_at > now));
  const count = credits?.reduce((sum, credit) => sum + credit.remaining, 0);
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout>;
    const update = () => {
      const current = Date.now() / 1000;
      setNow(current);
      // Remove expired offers even if the user leaves this screen open.
      const nextExpiry = inventory?.reduce((next, credit) =>
        credit.expires_at != null && credit.expires_at > current
          ? Math.min(next, credit.expires_at) : next, Infinity);
      if (nextExpiry == null || !Number.isFinite(nextExpiry)) return;
      timer = setTimeout(update, Math.min(2_147_483_647, Math.max(1, nextExpiry * 1000 - Date.now())));
    };
    update();
    return () => clearTimeout(timer);
  }, [inventory]);
  return (
    <div className="reset-credit-block claude">
      <strong className="reset-credit-title">
        {count === undefined ? `${t("resetCredits")} · ${t("resetCreditsUnavailable")}` : resetCreditLabel(count, t)}
      </strong>
      {credits?.map((credit, index) => (
        <small key={`${credit.kind}-${credit.expires_at}-${index}`}>
          {t(({ full: "resetFull", weekly: "resetWeekly", session: "resetSession",
            scoped: "resetScoped", other: "resetOther" })[credit.kind])} {credit.remaining}{t("resetCreditCountSuffix")}
          {" · "}{t("resetExpires")} {credit.expires_at == null ? t("resetUnknown") : compactDateFromSeconds(credit.expires_at)}
        </small>
      ))}
    </div>
  );
}

export function LimitBar({
  agent,
  label,
  value,
  t,
  compact,
}: {
  agent: AgentId;
  label: string;
  value?: AccountWindow | null;
  t: Translator;
  /** Slim single-line variant (used in the limit-switch popup): the reset time
   * moves to a hover tooltip so account cards stay short. */
  compact?: boolean;
}) {
  const used = value ? Math.max(0, Math.min(100, value.used_percent)) : 0;
  const reset = value?.resets_at ? compactDateFromSeconds(value.resets_at) : null;

  if (compact) {
    return (
      <div
        className={`account-limit-compact ${agent}`}
        title={reset ? `${t("resetsAt")} ${reset}` : undefined}
      >
        <span className="alc-label">{label}</span>
        <div className="usage-bar" aria-hidden="true">
          <span style={{ width: `${used}%` }} />
        </div>
        <span className="alc-pct">
          {value ? `${pct(value.remaining_percent)} ${t("left")}` : t("noSample")}
        </span>
      </div>
    );
  }

  return (
    <div className={`account-limit-row ${agent}`}>
      <strong>{label}</strong>
      <div className="usage-bar" aria-hidden="true">
        <span style={{ width: `${used}%` }} />
      </div>
      <span className="limit-right">
        <span className="limit-pct">{value ? `${pct(value.remaining_percent)} ${t("left")}` : t("noSample")}</span>
        {reset && (
          <small className="limit-reset">
            {t("resetsAt")} {reset}
          </small>
        )}
      </span>
    </div>
  );
}
