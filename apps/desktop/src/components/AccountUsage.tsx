import type { AccountWindow, SlotUsageReport } from "../types";
import type { Translator } from "../i18n";

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

export function ResetCreditsBlock({ usage, t }: { usage?: SlotUsageReport | null; t: Translator }) {
  const details = usage?.reset_credit_details ?? [];
  const count = resetCreditCount(usage);
  if (count <= 0 && details.length === 0) return null;
  return (
    <div className="reset-credit-block">
      <strong className="reset-credit-title">{resetCreditLabel(count, t)}</strong>
      {details.length > 0 && (
        <div className="credit-list">
          {details.map((credit, index) => (
            <small key={`${credit.granted_at}-${credit.expires_at}-${index}`}>
              {compactDate(credit.granted_at)} / {compactDate(credit.expires_at)}
            </small>
          ))}
        </div>
      )}
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
