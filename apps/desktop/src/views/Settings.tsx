import { useEffect, useMemo, useRef, useState } from "react";
import { HexColorPicker } from "react-colorful";
import {
  Bot,
  Box,
  CircleHelp,
  DownloadCloud,
  Languages,
  Palette,
  Play,
  RotateCcw,
  SlidersHorizontal,
  Trash2,
  Zap,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import {
  getAppVersion,
  getConfigSettings,
  resetConfigValue,
  runAppUninstall,
  runAppUpdate,
  setConfigValue,
} from "../api";
import type { ConfigRow } from "../types";
import type { Translator } from "../i18n";

const settingLabelKeys: Record<string, string> = {
  "triggers.five_hour.enabled": "settingLabelFiveHourEnabled",
  "triggers.five_hour.threshold_percent": "settingLabelFiveHourThreshold",
  "triggers.five_hour.mode": "settingLabelFiveHourMode",
  "triggers.five_hour.burn_rate.enabled": "settingLabelBurnRateEnabled",
  "triggers.five_hour.burn_rate.runway_minutes": "settingLabelBurnRateRunway",
  "autostart.enabled": "settingLabelAutostart",
  "daemon.idle_timeout_seconds": "settingLabelDaemonIdleTimeout",
  "statusline.show": "settingLabelStatusline",
  language: "settingLabelLanguage",
  "capsule.format": "settingLabelCapsuleFormat",
  "capsule.language": "settingLabelCapsuleLanguage",
  "capsule.next_prompt_max_items": "settingLabelCapsuleNextPromptMax",
  "capsule.remaining_max_items": "settingLabelCapsuleRemainingMax",
  "capsule.done_max_items": "settingLabelCapsuleDoneMax",
  "capsule.risks_max_items": "settingLabelCapsuleRisksMax",
  "theme.preset": "settingLabelThemePreset",
  "theme.codex_color": "settingLabelCodexColor",
  "theme.claude_color": "settingLabelClaudeColor",
  "theme.focus_border_color": "settingLabelFocusBorderColor",
  "theme.selection_bg_color": "settingLabelSelectionBgColor",
  "theme.selection_fg_color": "settingLabelSelectionFgColor",
  "gui_theme.preset": "settingLabelGuiThemePreset",
  "gui_theme.codex_color": "settingLabelGuiCodexColor",
  "gui_theme.claude_color": "settingLabelGuiClaudeColor",
  "gui_theme.focus_border_color": "settingLabelGuiFocusBorderColor",
  "gui_theme.selection_bg_color": "settingLabelGuiSelectionBgColor",
  "gui_theme.selection_fg_color": "settingLabelGuiSelectionFgColor",
  "gui_theme.app_bg_color": "settingLabelGuiAppBgColor",
  "gui_theme.sidebar_bg_color": "settingLabelGuiSidebarBgColor",
  "gui_theme.panel_bg_color": "settingLabelGuiPanelBgColor",
  "gui_theme.text_color": "settingLabelGuiTextColor",
};

const settingHelpKeys: Record<string, string> = {
  "triggers.five_hour.enabled": "settingHelpFiveHourEnabled",
  "triggers.five_hour.threshold_percent": "settingHelpFiveHourThreshold",
  "triggers.five_hour.mode": "settingHelpFiveHourMode",
  "triggers.five_hour.burn_rate.enabled": "settingHelpBurnRateEnabled",
  "triggers.five_hour.burn_rate.runway_minutes": "settingHelpBurnRateRunway",
  "autostart.enabled": "settingHelpAutostart",
  "daemon.idle_timeout_seconds": "settingHelpDaemonIdleTimeout",
  "statusline.show": "settingHelpStatusline",
  language: "settingHelpLanguage",
  "capsule.format": "settingHelpCapsuleFormat",
  "capsule.language": "settingHelpCapsuleLanguage",
  "capsule.next_prompt_max_items": "settingHelpCapsuleNextPromptMax",
  "capsule.remaining_max_items": "settingHelpCapsuleRemainingMax",
  "capsule.done_max_items": "settingHelpCapsuleDoneMax",
  "capsule.risks_max_items": "settingHelpCapsuleRisksMax",
  "gui_theme.preset": "settingHelpGuiThemePreset",
  "gui_theme.codex_color": "settingHelpGuiCodexColor",
  "gui_theme.claude_color": "settingHelpGuiClaudeColor",
  "gui_theme.focus_border_color": "settingHelpGuiFocusBorderColor",
  "gui_theme.selection_bg_color": "settingHelpGuiSelectionBgColor",
  "gui_theme.selection_fg_color": "settingHelpGuiSelectionFgColor",
  "gui_theme.app_bg_color": "settingHelpGuiAppBgColor",
  "gui_theme.sidebar_bg_color": "settingHelpGuiSidebarBgColor",
  "gui_theme.panel_bg_color": "settingHelpGuiPanelBgColor",
  "gui_theme.text_color": "settingHelpGuiTextColor",
};

const categoryIcons: Record<string, LucideIcon> = {
  all: SlidersHorizontal,
  automation: Play,
  triggers: Zap,
  capsule: Box,
  language: Languages,
  theme: Palette,
  agents: Bot,
  advanced: SlidersHorizontal,
  update: DownloadCloud,
};

// Synthetic category: holds the version/update/uninstall actions, not rows.
const UPDATE_CATEGORY = "update";

const namedColors: Record<string, string> = {
  black: "#000000",
  blue: "#000080",
  cyan: "#00FFFF",
  gray: "#808080",
  green: "#008000",
  orange: "#FFA500",
  purple: "#B996EB",
  red: "#800000",
  white: "#FFFFFF",
  yellow: "#808000",
  "dark-gray": "#404040",
  "light-blue": "#5555FF",
  "light-cyan": "#55FFFF",
  "light-green": "#55FF55",
  "light-magenta": "#FF55FF",
  "light-red": "#FF5555",
  "light-yellow": "#FFFF55",
};

function settingLabel(row: ConfigRow, t: Translator) {
  return t(settingLabelKeys[row.key] ?? row.key);
}

function settingHelp(row: ConfigRow, t: Translator) {
  const key = settingHelpKeys[row.key];
  return key ? t(key) : row.description;
}

function displaySettingValue(row: ConfigRow, value = row.value) {
  if (row.kind === "gui_theme_preset" && value === "dark") return "Dracula";
  return value;
}

function optionsFor(row: ConfigRow) {
  switch (row.kind) {
    case "bool":
      return ["true", "false"];
    case "mode":
      return ["off", "ask", "auto"];
    case "language":
      return ["ko", "ja", "zh", "en"];
    case "capsule_format":
      return ["json", "md"];
    case "theme_preset":
      return ["default", "high_contrast", "mono", "custom"];
    case "gui_theme_preset":
      return ["white", "dark", "custom"];
    default:
      return null;
  }
}

function colorInputValue(value: string) {
  if (/^#[0-9a-fA-F]{6}$/.test(value)) return value;
  return namedColors[value.toLowerCase()] ?? "#FF79C6";
}

function SettingValueEditor({
  row,
  busy,
  t,
  open,
  onToggle,
  onClose,
  onCommit,
}: {
  row: ConfigRow;
  busy: boolean;
  t: Translator;
  open: boolean;
  onToggle: () => void;
  onClose: () => void;
  onCommit: (row: ConfigRow, value: string) => Promise<void>;
}) {
  const [draft, setDraft] = useState(row.value);
  const [placement, setPlacement] = useState<"above" | "below">("below");
  const buttonRef = useRef<HTMLButtonElement | null>(null);
  const options = optionsFor(row);
  const isColor = row.kind === "color";
  const isNumber =
    row.kind === "percent" || row.kind === "positive_float" || row.kind === "count" || row.kind === "seconds";
  const numberMax =
    row.kind === "percent" ? 100 : row.kind === "count" ? 50 : row.kind === "seconds" ? 3600 : undefined;
  const numberStep = row.kind === "seconds" ? 5 : 1;

  useEffect(() => {
    setDraft(row.value);
  }, [row.key, row.value]);

  function toggleOpen() {
    const rect = buttonRef.current?.getBoundingClientRect();
    if (rect) {
      setPlacement(window.innerHeight - rect.bottom < 300 ? "above" : "below");
    }
    onToggle();
  }

  async function commit(value: string) {
    await onCommit(row, value);
    onClose();
  }

  return (
    <div className="setting-value-cell">
      <button ref={buttonRef} className="setting-value-button" disabled={busy} onClick={toggleOpen}>
        <code>{displaySettingValue(row)}</code>
      </button>
      {open && (
        <div className={`setting-popover ${placement}`}>
          {options && (
            <div className="option-list" aria-label={t("chooseValue")}>
              {options.map((option) => (
                <button
                  key={option}
                  className={option === row.value ? "option active" : "option"}
                  disabled={busy}
                  onClick={() => void commit(option)}
                >
                  {displaySettingValue(row, option)}
                </button>
              ))}
            </div>
          )}
          {isNumber && (
            <div className="inline-editor">
              <input
                type="number"
                min={row.kind === "percent" ? 0 : 1}
                max={numberMax}
                step={numberStep}
                value={draft}
                onChange={(event) => setDraft(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") void commit(draft);
                }}
              />
              <button disabled={busy || !draft.trim()} onClick={() => void commit(draft)}>
                {t("apply")}
              </button>
            </div>
          )}
          {isColor && (
            <div className="color-editor">
              <HexColorPicker color={colorInputValue(draft)} onChange={setDraft} />
              <div className="color-hue-note" aria-hidden="true" />
              <div className="color-picker-row">
                <input
                  type="color"
                  value={colorInputValue(draft)}
                  onChange={(event) => setDraft(event.target.value)}
                />
                <input
                  value={draft}
                  onChange={(event) => setDraft(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") void commit(draft);
                  }}
                  placeholder="#FF79C6"
                />
              </div>
              <button className="full-width" disabled={busy || !draft.trim()} onClick={() => void commit(draft)}>
                {t("apply")}
              </button>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function UpdatePanel({ t }: { t: Translator }) {
  const [version, setVersion] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getAppVersion()
      .then(setVersion)
      .catch((err) => setError(err instanceof Error ? err.message : String(err)));
  }, []);

  async function runAction(action: () => Promise<{ message: string }>, doneKey: string) {
    setBusy(true);
    setError(null);
    try {
      await action();
      setMessage(t(doneKey));
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setBusy(false);
    }
  }

  return (
    <div className="update-panel">
      {error && <div className="banner error">{error}</div>}
      {message && <div className="banner">{message}</div>}
      <div className="update-row">
        <div className="update-row-text">
          <strong>{t("settingUpdateVersion")}</strong>
          <span>{t("settingUpdateVersionHelp")}</span>
        </div>
        <code className="update-version">{version ? `v${version}` : "..."}</code>
      </div>
      <div className="update-row">
        <div className="update-row-text">
          <strong>{t("settingUpdateRun")}</strong>
          <span>{t("settingUpdateRunHelp")}</span>
        </div>
        <button
          disabled={busy}
          onClick={() => void runAction(runAppUpdate, "settingUpdateStarted")}
        >
          <DownloadCloud size={15} />
          <span>{t("settingUpdateRun")}</span>
        </button>
      </div>
      <div className="update-row">
        <div className="update-row-text">
          <strong className="update-danger-text">{t("settingUpdateUninstall")}</strong>
          <span>{t("settingUpdateUninstallHelp")}</span>
        </div>
        <button className="update-danger-button" disabled={busy} onClick={() => setConfirming(true)}>
          <Trash2 size={15} />
          <span>{t("settingUpdateUninstall")}</span>
        </button>
      </div>
      {confirming && (
        <div className="modal-backdrop" onMouseDown={() => setConfirming(false)}>
          <section
            className="confirm-modal"
            role="alertdialog"
            aria-modal="true"
            onMouseDown={(event) => event.stopPropagation()}
          >
            <h3>{t("settingUpdateConfirmTitle")}</h3>
            <p>{t("settingUpdateConfirmBody")}</p>
            <div className="confirm-modal-actions">
              <button disabled={busy} onClick={() => setConfirming(false)}>
                {t("cancel")}
              </button>
              <button
                className="update-danger-button"
                disabled={busy}
                onClick={() => {
                  setConfirming(false);
                  void runAction(runAppUninstall, "settingUninstallStarted");
                }}
              >
                {t("settingUpdateUninstall")}
              </button>
            </div>
          </section>
        </div>
      )}
    </div>
  );
}

export default function SettingsView({
  onThemeChanged,
  t,
}: {
  onThemeChanged?: () => Promise<void> | void;
  t: Translator;
}) {
  const [rows, setRows] = useState<ConfigRow[]>([]);
  const [category, setCategory] = useState("all");
  const [query, setQuery] = useState("");
  const [helpKey, setHelpKey] = useState<string | null>(null);
  const [editingKey, setEditingKey] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    getConfigSettings()
      .then(setRows)
      .catch((err) => setError(err instanceof Error ? err.message : String(err)))
      .finally(() => setLoading(false));
  }, []);

  const categories = useMemo(() => {
    const set = new Set(rows.map((row) => row.category));
    return ["all", ...Array.from(set), UPDATE_CATEGORY];
  }, [rows]);

  const filtered = rows.filter((row) => {
    const matchesCategory = category === "all" || row.category === category;
    const haystack =
      `${settingLabel(row, t)} ${row.key} ${row.value} ${displaySettingValue(row)} ${settingHelp(row, t)}`.toLowerCase();
    return matchesCategory && haystack.includes(query.toLowerCase());
  });

  async function commit(row: ConfigRow, value: string) {
    setBusy(true);
    setError(null);
    try {
      const next = await setConfigValue(row.key, value);
      setRows(next);
      if (row.key.startsWith("gui_theme.") || row.key.startsWith("theme.") || row.key === "language") {
        await onThemeChanged?.();
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }

  async function reset(row: ConfigRow) {
    setBusy(true);
    setError(null);
    try {
      const next = await resetConfigValue(row.key);
      setRows(next);
      if (row.key.startsWith("gui_theme.") || row.key.startsWith("theme.") || row.key === "language") {
        await onThemeChanged?.();
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="settings-layout">
      <aside className="settings-categories">
        <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder={t("searchSettings")} />
        {categories.map((item) => {
          const Icon = categoryIcons[item] ?? SlidersHorizontal;
          return (
            <button
              key={item}
              className={category === item ? "category active" : "category"}
              onClick={() => {
                setCategory(item);
                setEditingKey(null);
              }}
            >
              <Icon size={16} />
              <span>{t(item)}</span>
              <small>
                {item === UPDATE_CATEGORY
                  ? ""
                  : item === "all"
                    ? rows.length
                    : rows.filter((row) => row.category === item).length}
              </small>
            </button>
          );
        })}
      </aside>
      <section className="settings-main">
        {error && <div className="banner error">{error}</div>}
        {loading && <section className="loading-screen">{t("loadSettings")}</section>}
        {category === UPDATE_CATEGORY && <UpdatePanel t={t} />}
        {category !== UPDATE_CATEGORY && (
        <div className="setting-table">
          <div className="table-row head">
            <span>{t("setting")}</span>
            <span>{t("value")}</span>
          </div>
          {filtered.map((row) => (
            <div className="setting-row-wrap" key={row.key}>
              <div className="table-row setting-row">
                <div className="setting-name">
                  <span>{settingLabel(row, t)}</span>
                  <button
                    className="help-button"
                    title={t("showHelp")}
                    onClick={() => {
                      setEditingKey(null);
                      setHelpKey((current) => (current === row.key ? null : row.key));
                    }}
                  >
                    <CircleHelp size={15} />
                  </button>
                </div>
                <div className="setting-actions">
                  <SettingValueEditor
                    row={row}
                    busy={busy}
                    t={t}
                    open={editingKey === row.key}
                    onToggle={() => {
                      setHelpKey(null);
                      setEditingKey((current) => (current === row.key ? null : row.key));
                    }}
                    onClose={() => setEditingKey(null)}
                    onCommit={commit}
                  />
                  <button className="reset-icon" title={t("reset")} disabled={busy} onClick={() => void reset(row)}>
                    <RotateCcw size={14} />
                  </button>
                </div>
              </div>
              {helpKey === row.key && (
                <div className="setting-help-bubble">
                  <p>{settingHelp(row, t)}</p>
                  <dl>
                    <div>
                      <dt>{t("current")}</dt>
                      <dd>{displaySettingValue(row)}</dd>
                    </div>
                    <div>
                      <dt>{t("default")}</dt>
                      <dd>{displaySettingValue(row, row.default_value)}</dd>
                    </div>
                  </dl>
                </div>
              )}
            </div>
          ))}
        </div>
        )}
      </section>
    </div>
  );
}
