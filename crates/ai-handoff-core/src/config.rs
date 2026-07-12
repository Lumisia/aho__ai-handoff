//! Typed, file-backed AI Handoff config (`~/.ai-handoff/config.toml`).
//!
//! Defaults are embedded in these types' `Default` impls — there is no
//! separate defaults file. `parse`/`resolve` are pure;
//! `load` is the only IO entry point. A missing or malformed config resolves to
//! defaults so a hook is never broken by a bad config.

use std::collections::HashMap;
use std::fmt;
use std::path::Path;

use serde::{de, Deserialize};
use toml_edit::{value, DocumentMut, Item, Table};

use crate::trigger::{BurnRate, TriggerMode};

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Config {
    pub triggers: Triggers,
    pub autostart: Autostart,
    pub daemon: DaemonConfig,
    pub statusline: Statusline,
    pub language: Language,
    pub capsule: CapsuleConfig,
    pub theme: ThemeConfig,
    pub gui: GuiConfig,
    pub gui_theme: GuiThemeConfig,
    pub project_overrides: HashMap<String, ProjectOverride>,
}

/// Desktop-GUI behavior toggles (theme options live in [`GuiThemeConfig`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct GuiConfig {
    /// When the five-hour usage hits the trigger threshold, the running GUI
    /// shows a popup asking whether to switch to another saved account slot.
    /// Ask-only — the GUI never switches accounts by itself.
    pub limit_switch_prompt: bool,
    /// Check GitHub Releases (at most once a day) for a newer stable version
    /// and show a badge in the GUI. Check-only — installing stays manual.
    pub update_check: bool,
    /// Closing the GUI window hides it to the system tray instead of quitting.
    pub close_to_tray: bool,
}

impl Default for GuiConfig {
    fn default() -> Self {
        Self {
            limit_switch_prompt: true,
            update_check: true,
            close_to_tray: false,
        }
    }
}

/// UI language preference. Serialized as a short code; defaults to English.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    #[default]
    En,
    Ko,
    Ja,
}

impl<'de> Deserialize<'de> for Language {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Ok(match raw.trim().to_ascii_lowercase().as_str() {
            "ko" => Language::Ko,
            "ja" => Language::Ja,
            // Removed language codes migrate to English instead of making the
            // entire config unreadable for existing users.
            _ => Language::En,
        })
    }
}

/// Statusline display options. Opt-in: defaults to enabled (`show = true`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Statusline {
    pub show: bool,
}

impl Default for Statusline {
    fn default() -> Self {
        Self { show: true }
    }
}

pub const DEFAULT_DAEMON_IDLE_TIMEOUT_SECONDS: u64 = 60;
pub const MAX_DAEMON_IDLE_TIMEOUT_SECONDS: u64 = 3600;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct DaemonConfig {
    pub idle_timeout_seconds: u64,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            idle_timeout_seconds: DEFAULT_DAEMON_IDLE_TIMEOUT_SECONDS,
        }
    }
}

impl DaemonConfig {
    pub fn idle_timeout_seconds(self) -> u64 {
        self.idle_timeout_seconds
            .clamp(1, MAX_DAEMON_IDLE_TIMEOUT_SECONDS)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct CapsuleConfig {
    pub format: CapsuleFormat,
    pub language: Language,
    pub next_prompt_max_items: usize,
    pub remaining_max_items: usize,
    pub done_max_items: usize,
    pub risks_max_items: usize,
}

impl Default for CapsuleConfig {
    fn default() -> Self {
        Self {
            format: CapsuleFormat::Json,
            language: Language::En,
            next_prompt_max_items: DEFAULT_CAPSULE_ITEM_LIMIT,
            remaining_max_items: DEFAULT_CAPSULE_ITEM_LIMIT,
            done_max_items: DEFAULT_CAPSULE_ITEM_LIMIT,
            risks_max_items: DEFAULT_CAPSULE_ITEM_LIMIT,
        }
    }
}

pub const DEFAULT_CAPSULE_ITEM_LIMIT: usize = 5;
pub const MAX_CAPSULE_ITEM_LIMIT: usize = 50;

fn clamp_capsule_item_limit(value: usize) -> usize {
    value.clamp(1, MAX_CAPSULE_ITEM_LIMIT)
}

impl CapsuleConfig {
    pub fn next_prompt_limit(self) -> usize {
        clamp_capsule_item_limit(self.next_prompt_max_items)
    }

    pub fn remaining_limit(self) -> usize {
        clamp_capsule_item_limit(self.remaining_max_items)
    }

    pub fn done_limit(self) -> usize {
        clamp_capsule_item_limit(self.done_max_items)
    }

    pub fn risks_limit(self) -> usize {
        clamp_capsule_item_limit(self.risks_max_items)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CapsuleFormat {
    #[default]
    Json,
    Md,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ThemeConfig {
    pub preset: ThemePreset,
    pub codex_color: ColorSpec,
    pub claude_color: ColorSpec,
    pub focus_border_color: ColorSpec,
    pub selection_bg_color: ColorSpec,
    pub selection_fg_color: ColorSpec,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        theme_config_for_preset(ThemePreset::Default)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreset {
    #[default]
    Default,
    HighContrast,
    Mono,
    Custom,
}

pub fn theme_config_for_preset(preset: ThemePreset) -> ThemeConfig {
    let (codex, claude, focus, bg, fg) = match preset {
        ThemePreset::Default | ThemePreset::Custom => {
            ("#B996EB", "#E68C1E", "#FFA500", "cyan", "black")
        }
        ThemePreset::HighContrast => (
            "light-blue",
            "light-yellow",
            "light-yellow",
            "white",
            "black",
        ),
        ThemePreset::Mono => ("white", "gray", "white", "white", "black"),
    };
    ThemeConfig {
        preset,
        codex_color: ColorSpec::trusted(codex),
        claude_color: ColorSpec::trusted(claude),
        focus_border_color: ColorSpec::trusted(focus),
        selection_bg_color: ColorSpec::trusted(bg),
        selection_fg_color: ColorSpec::trusted(fg),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct GuiThemeConfig {
    pub preset: GuiThemePreset,
    /// How the active theme is chosen: follow the OS, or force light/dark.
    pub mode: GuiThemeMode,
    /// Catalog id of the theme used in light mode (or "custom").
    pub light_theme: String,
    /// Catalog id of the theme used in dark mode (or "custom").
    pub dark_theme: String,
    pub codex_color: ColorSpec,
    pub claude_color: ColorSpec,
    pub focus_border_color: ColorSpec,
    pub selection_bg_color: ColorSpec,
    pub selection_fg_color: ColorSpec,
    pub app_bg_color: ColorSpec,
    pub sidebar_bg_color: ColorSpec,
    pub panel_bg_color: ColorSpec,
    pub text_color: ColorSpec,
}

impl Default for GuiThemeConfig {
    fn default() -> Self {
        gui_theme_config_for_preset(GuiThemePreset::White)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GuiThemePreset {
    #[default]
    White,
    Dark,
    DarkGreen,
    Custom,
}

/// Whether the desktop GUI follows the OS light/dark setting or is forced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GuiThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

pub fn gui_theme_mode_str(mode: GuiThemeMode) -> &'static str {
    match mode {
        GuiThemeMode::System => "system",
        GuiThemeMode::Light => "light",
        GuiThemeMode::Dark => "dark",
    }
}

/// A named, built-in GUI theme (famous editor/app palettes). The frontend maps
/// these nine colors onto its CSS variables; `dark` places the theme in the
/// dark or light gallery and drives the "system" auto-switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NamedTheme {
    pub id: &'static str,
    pub name: &'static str,
    pub dark: bool,
    pub codex: &'static str,
    pub claude: &'static str,
    pub focus: &'static str,
    pub selection_bg: &'static str,
    pub selection_fg: &'static str,
    pub app_bg: &'static str,
    pub sidebar_bg: &'static str,
    pub panel_bg: &'static str,
    pub text: &'static str,
}

/// The built-in theme catalog: the three original presets plus famous
/// light/dark palettes (Dracula, Nord, Tokyo Night, Notion, GitHub, …).
pub fn theme_catalog() -> &'static [NamedTheme] {
    const T: &[NamedTheme] = &[
        // --- Light -------------------------------------------------------
        NamedTheme {
            id: "white",
            name: "Light",
            dark: false,
            codex: "#6F7380",
            claude: "#8A7252",
            focus: "#2F6F50",
            selection_bg: "#244F3B",
            selection_fg: "#FFFFFF",
            app_bg: "#F6F7F5",
            sidebar_bg: "#EEF1ED",
            panel_bg: "#FFFFFF",
            text: "#17201C",
        },
        NamedTheme {
            id: "notion",
            name: "Notion",
            dark: false,
            codex: "#2383E2",
            claude: "#D9730D",
            focus: "#2383E2",
            selection_bg: "#D3E5F3",
            selection_fg: "#1A1A1A",
            app_bg: "#FFFFFF",
            sidebar_bg: "#F7F6F3",
            panel_bg: "#FFFFFF",
            text: "#37352F",
        },
        NamedTheme {
            id: "github_light",
            name: "GitHub Light",
            dark: false,
            codex: "#0969DA",
            claude: "#9A6700",
            focus: "#0969DA",
            selection_bg: "#DDF4FF",
            selection_fg: "#0969DA",
            app_bg: "#FFFFFF",
            sidebar_bg: "#F6F8FA",
            panel_bg: "#FFFFFF",
            text: "#24292F",
        },
        NamedTheme {
            id: "solarized_light",
            name: "Solarized Light",
            dark: false,
            codex: "#2AA198",
            claude: "#CB4B16",
            focus: "#268BD2",
            selection_bg: "#E6DFC8",
            selection_fg: "#073642",
            app_bg: "#FDF6E3",
            sidebar_bg: "#EEE8D5",
            panel_bg: "#FDF6E3",
            text: "#657B83",
        },
        NamedTheme {
            id: "catppuccin_latte",
            name: "Catppuccin Latte",
            dark: false,
            codex: "#1E66F5",
            claude: "#FE640B",
            focus: "#8839EF",
            selection_bg: "#DCE0E8",
            selection_fg: "#4C4F69",
            app_bg: "#EFF1F5",
            sidebar_bg: "#E6E9EF",
            panel_bg: "#FFFFFF",
            text: "#4C4F69",
        },
        // Retro 8-bit hardware panel: warm beige with a chunky arcade red.
        NamedTheme {
            id: "eight_bit",
            name: "8-Bit",
            dark: false,
            codex: "#7B7B8E",
            claude: "#C13129",
            focus: "#C13129",
            selection_bg: "#C13129",
            selection_fg: "#F1F1E6",
            app_bg: "#E5E5DD",
            sidebar_bg: "#D8D8D1",
            panel_bg: "#F1F1E6",
            text: "#3D3D4B",
        },
        // Refined editorial light: paper white with a vermilion accent.
        NamedTheme {
            id: "vermilion",
            name: "Vermilion",
            dark: false,
            codex: "#5F5F5F",
            claude: "#D84424",
            focus: "#D84424",
            selection_bg: "#FBE3DC",
            selection_fg: "#A82E12",
            app_bg: "#F7F5F3",
            sidebar_bg: "#F1EEEB",
            panel_bg: "#FFFFFF",
            text: "#1A1A1A",
        },
        // --- Dark --------------------------------------------------------
        NamedTheme {
            id: "dark",
            name: "Dark",
            dark: true,
            codex: "#8D918D",
            claude: "#8D918D",
            focus: "#8B8374",
            selection_bg: "#3B3E3D",
            selection_fg: "#F1EFE7",
            app_bg: "#121314",
            sidebar_bg: "#171918",
            panel_bg: "#1D2020",
            text: "#E8E6DC",
        },
        NamedTheme {
            id: "dark_green",
            name: "Dark green",
            dark: true,
            codex: "#A9ADB8",
            claude: "#C3A77B",
            focus: "#68A782",
            selection_bg: "#315F49",
            selection_fg: "#F4F7F4",
            app_bg: "#171B19",
            sidebar_bg: "#1D231F",
            panel_bg: "#202622",
            text: "#EDF2ED",
        },
        NamedTheme {
            id: "dracula",
            name: "Dracula",
            dark: true,
            codex: "#8BE9FD",
            claude: "#FFB86C",
            focus: "#BD93F9",
            selection_bg: "#44475A",
            selection_fg: "#F8F8F2",
            app_bg: "#282A36",
            sidebar_bg: "#21222C",
            panel_bg: "#343746",
            text: "#F8F8F2",
        },
        NamedTheme {
            id: "nord",
            name: "Nord",
            dark: true,
            codex: "#81A1C1",
            claude: "#D08770",
            focus: "#88C0D0",
            selection_bg: "#434C5E",
            selection_fg: "#ECEFF4",
            app_bg: "#2E3440",
            sidebar_bg: "#292E39",
            panel_bg: "#3B4252",
            text: "#D8DEE9",
        },
        NamedTheme {
            id: "one_dark",
            name: "One Dark",
            dark: true,
            codex: "#56B6C2",
            claude: "#D19A66",
            focus: "#61AFEF",
            selection_bg: "#3E4451",
            selection_fg: "#FFFFFF",
            app_bg: "#282C34",
            sidebar_bg: "#21252B",
            panel_bg: "#2C313A",
            text: "#ABB2BF",
        },
        NamedTheme {
            id: "monokai",
            name: "Monokai",
            dark: true,
            codex: "#66D9EF",
            claude: "#FD971F",
            focus: "#A6E22E",
            selection_bg: "#49483E",
            selection_fg: "#F8F8F2",
            app_bg: "#272822",
            sidebar_bg: "#1E1F1C",
            panel_bg: "#2D2E28",
            text: "#F8F8F2",
        },
        NamedTheme {
            id: "tokyo_night",
            name: "Tokyo Night",
            dark: true,
            codex: "#7DCFFF",
            claude: "#FF9E64",
            focus: "#7AA2F7",
            selection_bg: "#283457",
            selection_fg: "#C0CAF5",
            app_bg: "#1A1B26",
            sidebar_bg: "#16161E",
            panel_bg: "#24283B",
            text: "#A9B1D6",
        },
        NamedTheme {
            id: "catppuccin_mocha",
            name: "Catppuccin Mocha",
            dark: true,
            codex: "#89B4FA",
            claude: "#FAB387",
            focus: "#CBA6F7",
            selection_bg: "#45475A",
            selection_fg: "#CDD6F4",
            app_bg: "#1E1E2E",
            sidebar_bg: "#181825",
            panel_bg: "#313244",
            text: "#CDD6F4",
        },
        NamedTheme {
            id: "gruvbox",
            name: "Gruvbox",
            dark: true,
            codex: "#83A598",
            claude: "#FABD2F",
            focus: "#FE8019",
            selection_bg: "#504945",
            selection_fg: "#FBF1C7",
            app_bg: "#282828",
            sidebar_bg: "#1D2021",
            panel_bg: "#32302F",
            text: "#EBDBB2",
        },
        NamedTheme {
            id: "rose_pine",
            name: "Rosé Pine",
            dark: true,
            codex: "#9CCFD8",
            claude: "#F6C177",
            focus: "#C4A7E7",
            selection_bg: "#403D52",
            selection_fg: "#E0DEF4",
            app_bg: "#191724",
            sidebar_bg: "#1F1D2E",
            panel_bg: "#26233A",
            text: "#E0DEF4",
        },
        NamedTheme {
            id: "github_dark",
            name: "GitHub Dark",
            dark: true,
            codex: "#58A6FF",
            claude: "#D29922",
            focus: "#58A6FF",
            selection_bg: "#1F6FEB",
            selection_fg: "#FFFFFF",
            app_bg: "#0D1117",
            sidebar_bg: "#010409",
            panel_bg: "#161B22",
            text: "#C9D1D9",
        },
        NamedTheme {
            id: "solarized_dark",
            name: "Solarized Dark",
            dark: true,
            codex: "#2AA198",
            claude: "#CB4B16",
            focus: "#268BD2",
            selection_bg: "#094D5A",
            selection_fg: "#FDF6E3",
            app_bg: "#002B36",
            sidebar_bg: "#073642",
            panel_bg: "#073642",
            text: "#93A1A1",
        },
    ];
    T
}

/// Whether a catalog id names a known theme (used to validate slot ids).
pub fn theme_id_is_known(id: &str) -> bool {
    id == "custom" || theme_catalog().iter().any(|theme| theme.id == id)
}

pub fn gui_theme_config_for_preset(preset: GuiThemePreset) -> GuiThemeConfig {
    let (codex, claude, focus, selection_bg, selection_fg, app_bg, sidebar_bg, panel_bg, text) =
        match preset {
            GuiThemePreset::White | GuiThemePreset::Custom => (
                "#6F7380", "#8A7252", "#2F6F50", "#244F3B", "#FFFFFF", "#F6F7F5", "#EEF1ED",
                "#FFFFFF", "#17201C",
            ),
            GuiThemePreset::Dark => (
                "#8D918D", "#8D918D", "#8B8374", "#3B3E3D", "#F1EFE7", "#121314", "#171918",
                "#1D2020", "#E8E6DC",
            ),
            GuiThemePreset::DarkGreen => (
                "#A9ADB8", "#C3A77B", "#68A782", "#315F49", "#F4F7F4", "#171B19", "#1D231F",
                "#202622", "#EDF2ED",
            ),
        };
    GuiThemeConfig {
        preset,
        mode: GuiThemeMode::System,
        light_theme: "white".into(),
        dark_theme: "dark".into(),
        codex_color: ColorSpec::trusted(codex),
        claude_color: ColorSpec::trusted(claude),
        focus_border_color: ColorSpec::trusted(focus),
        selection_bg_color: ColorSpec::trusted(selection_bg),
        selection_fg_color: ColorSpec::trusted(selection_fg),
        app_bg_color: ColorSpec::trusted(app_bg),
        sidebar_bg_color: ColorSpec::trusted(sidebar_bg),
        panel_bg_color: ColorSpec::trusted(panel_bg),
        text_color: ColorSpec::trusted(text),
    }
}

fn legacy_dark_gui_theme_config() -> GuiThemeConfig {
    GuiThemeConfig {
        preset: GuiThemePreset::Dark,
        mode: GuiThemeMode::System,
        light_theme: "white".into(),
        dark_theme: "dark".into(),
        codex_color: ColorSpec::trusted("#C8A7FF"),
        claude_color: ColorSpec::trusted("#FFB05C"),
        focus_border_color: ColorSpec::trusted("#FF9F43"),
        selection_bg_color: ColorSpec::trusted("#FF79C6"),
        selection_fg_color: ColorSpec::trusted("#111318"),
        app_bg_color: ColorSpec::trusted("#0B0D14"),
        sidebar_bg_color: ColorSpec::trusted("#111522"),
        panel_bg_color: ColorSpec::trusted("#191D2A"),
        text_color: ColorSpec::trusted("#F8F8F2"),
    }
}

fn gui_theme_colors_eq(a: &GuiThemeConfig, b: &GuiThemeConfig) -> bool {
    a.focus_border_color == b.focus_border_color
        && a.selection_bg_color == b.selection_bg_color
        && a.selection_fg_color == b.selection_fg_color
        && a.app_bg_color == b.app_bg_color
        && a.sidebar_bg_color == b.sidebar_bg_color
        && a.panel_bg_color == b.panel_bg_color
        && a.text_color == b.text_color
}

/// Resolve GUI presets to their current built-in colors.
///
/// Older config files may contain the previous built-in dark tuple. Treat that
/// exact tuple as the dark preset, but preserve non-canonical values as custom.
pub fn effective_gui_theme_config(theme: &GuiThemeConfig) -> GuiThemeConfig {
    let white = gui_theme_config_for_preset(GuiThemePreset::White);
    let dark = gui_theme_config_for_preset(GuiThemePreset::Dark);
    let dark_green = gui_theme_config_for_preset(GuiThemePreset::DarkGreen);
    let legacy_dark = legacy_dark_gui_theme_config();

    match theme.preset {
        GuiThemePreset::White => {
            if gui_theme_colors_eq(theme, &white) {
                white
            } else {
                let mut custom = theme.clone();
                custom.preset = GuiThemePreset::Custom;
                custom
            }
        }
        GuiThemePreset::Dark => {
            if gui_theme_colors_eq(theme, &dark_green) {
                dark_green
            } else if gui_theme_colors_eq(theme, &white)
                || gui_theme_colors_eq(theme, &dark)
                || gui_theme_colors_eq(theme, &legacy_dark)
            {
                dark
            } else {
                let mut custom = theme.clone();
                custom.preset = GuiThemePreset::Custom;
                custom
            }
        }
        GuiThemePreset::DarkGreen => {
            if gui_theme_colors_eq(theme, &white) || gui_theme_colors_eq(theme, &dark_green) {
                dark_green
            } else {
                let mut custom = theme.clone();
                custom.preset = GuiThemePreset::Custom;
                custom
            }
        }
        GuiThemePreset::Custom => {
            if gui_theme_colors_eq(theme, &white) {
                white
            } else if gui_theme_colors_eq(theme, &dark) {
                dark
            } else if gui_theme_colors_eq(theme, &dark_green) {
                dark_green
            } else {
                theme.clone()
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorSpec(String);

impl ColorSpec {
    fn trusted(value: &str) -> Self {
        Self(value.to_string())
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        let value = value.trim();
        if value.is_empty() {
            return Err("color must not be empty".into());
        }
        if parse_color_rgb(value).is_some() || parse_indexed_color(value).is_some() {
            return Ok(Self(value.to_string()));
        }
        Err("expected a named color, #RRGGBB, or 0..255 indexed color".into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn rgb(&self) -> Option<(u8, u8, u8)> {
        parse_color_rgb(&self.0)
    }
}

impl Default for ColorSpec {
    fn default() -> Self {
        Self::trusted("white")
    }
}

impl<'de> Deserialize<'de> for ColorSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(de::Error::custom)
    }
}

impl fmt::Display for ColorSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

fn parse_color_rgb(value: &str) -> Option<(u8, u8, u8)> {
    let lower = value.trim().to_ascii_lowercase().replace('_', "-");
    if let Some(hex) = lower.strip_prefix('#') {
        if hex.len() == 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            return Some((r, g, b));
        }
        return None;
    }
    let named = match lower.as_str() {
        "black" => (0, 0, 0),
        "red" => (128, 0, 0),
        "green" => (0, 128, 0),
        "yellow" => (128, 128, 0),
        "blue" => (0, 0, 128),
        "magenta" => (128, 0, 128),
        "cyan" => (0, 255, 255),
        "gray" | "grey" => (128, 128, 128),
        "dark-gray" | "dark-grey" => (64, 64, 64),
        "light-red" => (255, 85, 85),
        "light-green" => (85, 255, 85),
        "light-yellow" => (255, 255, 85),
        "light-blue" => (85, 85, 255),
        "light-magenta" => (255, 85, 255),
        "light-cyan" => (85, 255, 255),
        "white" => (255, 255, 255),
        "orange" => (255, 165, 0),
        "purple" => (185, 150, 235),
        _ => return ansi_16_rgb(&lower),
    };
    Some(named)
}

fn parse_indexed_color(value: &str) -> Option<u8> {
    let n: u16 = value.trim().parse().ok()?;
    if n <= 255 {
        Some(n as u8)
    } else {
        None
    }
}

fn ansi_16_rgb(value: &str) -> Option<(u8, u8, u8)> {
    let n = parse_indexed_color(value)?;
    Some(match n {
        0 => (0, 0, 0),
        1 => (128, 0, 0),
        2 => (0, 128, 0),
        3 => (128, 128, 0),
        4 => (0, 0, 128),
        5 => (128, 0, 128),
        6 => (0, 128, 128),
        7 => (192, 192, 192),
        8 => (128, 128, 128),
        9 => (255, 0, 0),
        10 => (0, 255, 0),
        11 => (255, 255, 0),
        12 => (0, 0, 255),
        13 => (255, 0, 255),
        14 => (0, 255, 255),
        15 => (255, 255, 255),
        _ => return None,
    })
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Triggers {
    pub five_hour: FiveHour,
}

/// Run the daemon automatically at logon. Opt-in: defaults to disabled (the
/// `bool` default), so a fresh install registers no autostart unless the user
/// sets `[autostart] enabled = true`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Autostart {
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(default)]
pub struct FiveHour {
    pub enabled: bool,
    pub threshold_percent: f64,
    pub mode: ModeCfg,
    pub burn_rate: BurnRateCfg,
}

impl Default for FiveHour {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold_percent: 80.0,
            mode: ModeCfg::Ask,
            burn_rate: BurnRateCfg::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModeCfg {
    Off,
    Ask,
    Auto,
}

impl ModeCfg {
    pub fn to_trigger_mode(self) -> TriggerMode {
        match self {
            ModeCfg::Off => TriggerMode::Off,
            ModeCfg::Ask => TriggerMode::Ask,
            ModeCfg::Auto => TriggerMode::Auto,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(default)]
pub struct BurnRateCfg {
    pub enabled: bool,
    pub runway_minutes: f64,
}

impl Default for BurnRateCfg {
    fn default() -> Self {
        Self {
            enabled: false,
            runway_minutes: 30.0,
        }
    }
}

impl BurnRateCfg {
    pub fn to_burn(self) -> BurnRate {
        BurnRate {
            enabled: self.enabled,
            runway_minutes: self.runway_minutes,
        }
    }
}

/// A per-project override: every field optional, deep-merged over the global.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct ProjectOverride {
    pub triggers: TriggersOverride,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct TriggersOverride {
    pub five_hour: FiveHourOverride,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct FiveHourOverride {
    pub enabled: Option<bool>,
    pub threshold_percent: Option<f64>,
    pub mode: Option<ModeCfg>,
    pub burn_rate: Option<BurnRateOverride>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct BurnRateOverride {
    pub enabled: Option<bool>,
    pub runway_minutes: Option<f64>,
}

/// The concrete trigger inputs after applying any project override.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedTrigger {
    pub enabled: bool,
    pub threshold: f64,
    pub mode: TriggerMode,
    pub burn: BurnRate,
}

/// Parse config text. Propagates TOML/type errors so `load` can fall back.
pub fn parse(text: &str) -> Result<Config, toml::de::Error> {
    toml::from_str(text)
}

/// Resolve the effective trigger for a project: apply the matching
/// `project_overrides[fingerprint]` (if any) over the global config, then clamp.
pub fn resolve(cfg: &Config, fingerprint: &str) -> ResolvedTrigger {
    let g = &cfg.triggers.five_hour;
    let mut enabled = g.enabled;
    let mut threshold = g.threshold_percent;
    let mut mode = g.mode;
    let mut burn = g.burn_rate;

    if let Some(ov) = cfg.project_overrides.get(fingerprint) {
        let f = &ov.triggers.five_hour;
        if let Some(v) = f.enabled {
            enabled = v;
        }
        if let Some(v) = f.threshold_percent {
            threshold = v;
        }
        if let Some(v) = f.mode {
            mode = v;
        }
        if let Some(b) = f.burn_rate.as_ref() {
            if let Some(v) = b.enabled {
                burn.enabled = v;
            }
            if let Some(v) = b.runway_minutes {
                burn.runway_minutes = v;
            }
        }
    }

    ResolvedTrigger {
        enabled,
        threshold: threshold.clamp(0.0, 100.0),
        mode: mode.to_trigger_mode(),
        burn: burn.to_burn(),
    }
}

/// Load config from the default path, falling back to defaults on any error.
pub fn load() -> Config {
    load_from(&crate::paths::config_path())
}

/// Load config from `path`. A missing file or any parse error yields
/// `Config::default()` — the daemon must never break a hook over a bad config.
pub fn load_from(path: &Path) -> Config {
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text).unwrap_or_default(),
        Err(_) => Config::default(),
    }
}

// ---------------------------------------------------------------------------
// Write API (`config get` / `config set`)
//
// Editing is **never-clobber**: `set_value` parses the existing `config.toml`
// with `toml_edit`, changes exactly the one requested leaf (creating only the
// implicit parent tables it needs), and re-serializes — every other table,
// key, comment and string is preserved byte-for-byte. The set of editable
// keys is a fixed whitelist with per-key type/range validation, so a bad
// value is rejected before any write rather than corrupting the daemon's view.
// ---------------------------------------------------------------------------

/// Error from a [`set_value`] / [`get_value`] call.
#[derive(Debug, thiserror::Error)]
pub enum ConfigWriteError {
    #[error("unknown config key: {0}")]
    UnknownKey(String),
    #[error("invalid value for {key}: {message}")]
    InvalidValue { key: String, message: String },
    #[error("config.toml parse error: {0}")]
    Parse(#[from] toml_edit::TomlError),
    #[error("config key {key} sits under a non-table node; refusing to overwrite it")]
    ShapeConflict { key: String },
}

/// The value type accepted for an editable key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueKind {
    Bool,
    /// A float clamped to the inclusive `0..=100` range.
    Percent,
    /// A strictly-positive float.
    PosFloat,
    /// A small positive item count.
    Count,
    /// Positive whole seconds.
    Seconds,
    /// One of `off` / `ask` / `auto`.
    Mode,
    /// A UI language code: `en` / `ko` / `ja`.
    Lang,
    /// Capsule on-disk format: `json` / `md`.
    CapsuleFormat,
    /// Theme preset name.
    ThemePreset,
    /// GUI theme preset name.
    GuiThemePreset,
    /// GUI theme mode: `system` / `light` / `dark`.
    GuiThemeMode,
    /// A GUI theme catalog id (or `custom`).
    GuiThemeName,
    /// Terminal color: named, `#RRGGBB`, or indexed `0..255`.
    Color,
}

/// The whitelist of user-editable keys (dotted) and their value types.
const SETTABLE: &[(&str, ValueKind)] = &[
    ("triggers.five_hour.enabled", ValueKind::Bool),
    ("triggers.five_hour.threshold_percent", ValueKind::Percent),
    ("triggers.five_hour.mode", ValueKind::Mode),
    ("triggers.five_hour.burn_rate.enabled", ValueKind::Bool),
    (
        "triggers.five_hour.burn_rate.runway_minutes",
        ValueKind::PosFloat,
    ),
    ("autostart.enabled", ValueKind::Bool),
    ("daemon.idle_timeout_seconds", ValueKind::Seconds),
    ("statusline.show", ValueKind::Bool),
    ("language", ValueKind::Lang),
    ("capsule.format", ValueKind::CapsuleFormat),
    ("capsule.language", ValueKind::Lang),
    ("capsule.next_prompt_max_items", ValueKind::Count),
    ("capsule.remaining_max_items", ValueKind::Count),
    ("capsule.done_max_items", ValueKind::Count),
    ("capsule.risks_max_items", ValueKind::Count),
    ("theme.preset", ValueKind::ThemePreset),
    ("theme.codex_color", ValueKind::Color),
    ("theme.claude_color", ValueKind::Color),
    ("theme.focus_border_color", ValueKind::Color),
    ("theme.selection_bg_color", ValueKind::Color),
    ("theme.selection_fg_color", ValueKind::Color),
];

const GUI_SETTABLE: &[(&str, ValueKind)] = &[
    ("gui.limit_switch_prompt", ValueKind::Bool),
    ("gui.update_check", ValueKind::Bool),
    ("gui.close_to_tray", ValueKind::Bool),
    ("gui_theme.preset", ValueKind::GuiThemePreset),
    ("gui_theme.mode", ValueKind::GuiThemeMode),
    ("gui_theme.light_theme", ValueKind::GuiThemeName),
    ("gui_theme.dark_theme", ValueKind::GuiThemeName),
    ("gui_theme.codex_color", ValueKind::Color),
    ("gui_theme.claude_color", ValueKind::Color),
    ("gui_theme.focus_border_color", ValueKind::Color),
    ("gui_theme.selection_bg_color", ValueKind::Color),
    ("gui_theme.selection_fg_color", ValueKind::Color),
    ("gui_theme.app_bg_color", ValueKind::Color),
    ("gui_theme.sidebar_bg_color", ValueKind::Color),
    ("gui_theme.panel_bg_color", ValueKind::Color),
    ("gui_theme.text_color", ValueKind::Color),
];

/// The editable config keys, in display order (for `config list`).
pub fn settable_keys() -> impl Iterator<Item = &'static str> {
    SETTABLE.iter().map(|(k, _)| *k)
}

pub fn gui_settable_keys() -> impl Iterator<Item = &'static str> {
    GUI_SETTABLE.iter().map(|(k, _)| *k)
}

/// The public value-kind of an editable key, for UIs that edit config (e.g. the
/// TUI Settings tab): bool toggles, mode cycles, numbers step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyKind {
    Bool,
    /// Float in `0..=100`.
    Percent,
    /// Strictly-positive float.
    PosFloat,
    /// Positive count in `1..=50`.
    Count,
    /// Positive seconds in `1..=3600`.
    Seconds,
    /// `off` / `ask` / `auto`.
    Mode,
    /// `en` / `ko` / `ja`.
    Lang,
    /// `json` / `md`.
    CapsuleFormat,
    /// `default` / `high_contrast` / `mono` / `custom`.
    ThemePreset,
    /// `white` / `dark` / `custom`.
    GuiThemePreset,
    /// `system` / `light` / `dark`.
    GuiThemeMode,
    /// A GUI theme catalog id.
    GuiThemeName,
    /// Terminal color string.
    Color,
}

/// The kind of an editable key, or `None` if `key` is not editable.
pub fn key_kind(key: &str) -> Option<KeyKind> {
    SETTABLE
        .iter()
        .chain(GUI_SETTABLE.iter())
        .find(|(k, _)| *k == key)
        .map(|(_, v)| match v {
            ValueKind::Bool => KeyKind::Bool,
            ValueKind::Percent => KeyKind::Percent,
            ValueKind::PosFloat => KeyKind::PosFloat,
            ValueKind::Count => KeyKind::Count,
            ValueKind::Seconds => KeyKind::Seconds,
            ValueKind::Mode => KeyKind::Mode,
            ValueKind::Lang => KeyKind::Lang,
            ValueKind::CapsuleFormat => KeyKind::CapsuleFormat,
            ValueKind::ThemePreset => KeyKind::ThemePreset,
            ValueKind::GuiThemePreset => KeyKind::GuiThemePreset,
            ValueKind::GuiThemeMode => KeyKind::GuiThemeMode,
            ValueKind::GuiThemeName => KeyKind::GuiThemeName,
            ValueKind::Color => KeyKind::Color,
        })
}

impl ValueKind {
    /// Validate `raw` for this kind and convert it to a TOML item.
    fn to_item(self, key: &str, raw: &str) -> Result<Item, ConfigWriteError> {
        let invalid = |message: &str| ConfigWriteError::InvalidValue {
            key: key.to_string(),
            message: message.to_string(),
        };
        Ok(match self {
            ValueKind::Bool => {
                let b: bool = raw
                    .parse()
                    .map_err(|_| invalid("expected `true` or `false`"))?;
                value(b)
            }
            ValueKind::Mode => match raw {
                "off" | "ask" | "auto" => value(raw),
                _ => return Err(invalid("expected one of `off`, `ask`, `auto`")),
            },
            ValueKind::Lang => match raw {
                "en" | "ko" | "ja" => value(raw),
                _ => return Err(invalid("expected one of `en`, `ko`, `ja`")),
            },
            ValueKind::CapsuleFormat => match raw {
                "json" | "md" => value(raw),
                _ => return Err(invalid("expected one of `json`, `md`")),
            },
            ValueKind::ThemePreset => match raw {
                "default" | "high_contrast" | "mono" | "custom" => value(raw),
                _ => {
                    return Err(invalid(
                        "expected one of `default`, `high_contrast`, `mono`, `custom`",
                    ))
                }
            },
            ValueKind::GuiThemeMode => match raw {
                "system" | "light" | "dark" => value(raw),
                _ => return Err(invalid("expected one of `system`, `light`, `dark`")),
            },
            ValueKind::GuiThemeName => {
                if theme_id_is_known(raw) {
                    value(raw)
                } else {
                    return Err(invalid("unknown GUI theme id"));
                }
            }
            ValueKind::GuiThemePreset => match raw {
                "white" | "dark" | "dark_green" | "custom" => value(raw),
                _ => {
                    return Err(invalid(
                        "expected one of `white`, `dark`, `dark_green`, `custom`",
                    ))
                }
            },
            ValueKind::Color => {
                ColorSpec::parse(raw).map_err(|message| ConfigWriteError::InvalidValue {
                    key: key.to_string(),
                    message,
                })?;
                value(raw)
            }
            ValueKind::Percent => {
                let n: f64 = raw.parse().map_err(|_| invalid("expected a number"))?;
                if !(0.0..=100.0).contains(&n) {
                    return Err(invalid("must be between 0 and 100"));
                }
                value(n)
            }
            ValueKind::PosFloat => {
                let n: f64 = raw.parse().map_err(|_| invalid("expected a number"))?;
                if n.is_nan() || n <= 0.0 {
                    return Err(invalid("must be greater than 0"));
                }
                value(n)
            }
            ValueKind::Count => {
                let n: usize = raw
                    .parse()
                    .map_err(|_| invalid("expected a positive integer"))?;
                if n == 0 || n > MAX_CAPSULE_ITEM_LIMIT {
                    return Err(invalid("must be between 1 and 50"));
                }
                value(n as i64)
            }
            ValueKind::Seconds => {
                let n: u64 = raw
                    .parse()
                    .map_err(|_| invalid("expected a positive integer"))?;
                if n == 0 || n > MAX_DAEMON_IDLE_TIMEOUT_SECONDS {
                    return Err(invalid("must be between 1 and 3600"));
                }
                value(n as i64)
            }
        })
    }
}

/// Set `key` to `raw` in `existing` config text (or a fresh document when
/// `existing` is `None`), returning the new serialized `config.toml`.
///
/// Rejects unknown keys and out-of-range/ill-typed values before touching the
/// document. Parse errors on a present-but-corrupt file are propagated so the
/// caller aborts rather than clobbering it with a blank document.
pub fn set_value(existing: Option<&str>, key: &str, raw: &str) -> Result<String, ConfigWriteError> {
    let kind = SETTABLE
        .iter()
        .chain(GUI_SETTABLE.iter())
        .find(|(k, _)| *k == key)
        .map(|(_, v)| *v)
        .ok_or_else(|| ConfigWriteError::UnknownKey(key.to_string()))?;
    let item = kind.to_item(key, raw)?;

    let mut doc: DocumentMut = match existing {
        Some(s) => s.parse::<DocumentMut>()?,
        None => DocumentMut::new(),
    };

    let segments: Vec<&str> = key.split('.').collect();
    let (last, parents) = segments.split_last().expect("whitelist keys are non-empty");
    let mut table = doc.as_table_mut();
    for seg in parents {
        let entry = table.entry(seg).or_insert_with(|| {
            let mut t = Table::new();
            t.set_implicit(true);
            Item::Table(t)
        });
        table = entry
            .as_table_mut()
            .ok_or_else(|| ConfigWriteError::ShapeConflict {
                key: key.to_string(),
            })?;
    }
    table.insert(last, item);
    if key == "theme.preset" {
        let preset = match raw {
            "default" => ThemePreset::Default,
            "high_contrast" => ThemePreset::HighContrast,
            "mono" => ThemePreset::Mono,
            "custom" => ThemePreset::Custom,
            _ => unreachable!("theme preset was validated above"),
        };
        if preset != ThemePreset::Custom {
            let preset_theme = theme_config_for_preset(preset);
            table.insert(
                "codex_color",
                value(preset_theme.codex_color.as_str().to_string()),
            );
            table.insert(
                "claude_color",
                value(preset_theme.claude_color.as_str().to_string()),
            );
            table.insert(
                "focus_border_color",
                value(preset_theme.focus_border_color.as_str().to_string()),
            );
            table.insert(
                "selection_bg_color",
                value(preset_theme.selection_bg_color.as_str().to_string()),
            );
            table.insert(
                "selection_fg_color",
                value(preset_theme.selection_fg_color.as_str().to_string()),
            );
        }
    }
    if key == "gui_theme.preset" {
        let preset = match raw {
            "white" => GuiThemePreset::White,
            "dark" => GuiThemePreset::Dark,
            "dark_green" => GuiThemePreset::DarkGreen,
            "custom" => GuiThemePreset::Custom,
            _ => unreachable!("gui theme preset was validated above"),
        };
        if preset != GuiThemePreset::Custom {
            let preset_theme = gui_theme_config_for_preset(preset);
            table.insert(
                "focus_border_color",
                value(preset_theme.focus_border_color.as_str().to_string()),
            );
            table.insert(
                "selection_bg_color",
                value(preset_theme.selection_bg_color.as_str().to_string()),
            );
            table.insert(
                "selection_fg_color",
                value(preset_theme.selection_fg_color.as_str().to_string()),
            );
            table.insert(
                "app_bg_color",
                value(preset_theme.app_bg_color.as_str().to_string()),
            );
            table.insert(
                "sidebar_bg_color",
                value(preset_theme.sidebar_bg_color.as_str().to_string()),
            );
            table.insert(
                "panel_bg_color",
                value(preset_theme.panel_bg_color.as_str().to_string()),
            );
            table.insert(
                "text_color",
                value(preset_theme.text_color.as_str().to_string()),
            );
        }
    }
    // Editing an individual GUI color marks the theme custom. Mode/light/dark
    // slot selections do not — they pick catalog themes, not ad-hoc colors.
    const GUI_NON_COLOR_KEYS: [&str; 4] = [
        "gui_theme.preset",
        "gui_theme.mode",
        "gui_theme.light_theme",
        "gui_theme.dark_theme",
    ];
    if key.starts_with("gui_theme.") && !GUI_NON_COLOR_KEYS.contains(&key) {
        table.insert(
            "preset",
            value(gui_theme_preset_str(GuiThemePreset::Custom)),
        );
    }

    let text = doc.to_string();
    if key.starts_with("theme.") {
        let cfg: Config = toml::from_str(&text).map_err(|e| ConfigWriteError::InvalidValue {
            key: key.to_string(),
            message: e.to_string(),
        })?;
        validate_theme_contrast(&cfg.theme, key)?;
    }
    if key.starts_with("gui_theme.") {
        let cfg: Config = toml::from_str(&text).map_err(|e| ConfigWriteError::InvalidValue {
            key: key.to_string(),
            message: e.to_string(),
        })?;
        validate_color_contrast(
            &cfg.gui_theme.selection_bg_color,
            &cfg.gui_theme.selection_fg_color,
            key,
        )?;
    }

    Ok(text)
}

/// Read the **effective** value of `key` from a resolved [`Config`] (so an
/// unset key reports its built-in default), formatted as the daemon sees it.
pub fn get_value(cfg: &Config, key: &str) -> Result<String, ConfigWriteError> {
    let f = &cfg.triggers.five_hour;
    let gui_theme = effective_gui_theme_config(&cfg.gui_theme);
    Ok(match key {
        "triggers.five_hour.enabled" => f.enabled.to_string(),
        "triggers.five_hour.threshold_percent" => fmt_f64(f.threshold_percent),
        "triggers.five_hour.mode" => mode_str(f.mode).to_string(),
        "triggers.five_hour.burn_rate.enabled" => f.burn_rate.enabled.to_string(),
        "triggers.five_hour.burn_rate.runway_minutes" => fmt_f64(f.burn_rate.runway_minutes),
        "autostart.enabled" => cfg.autostart.enabled.to_string(),
        "daemon.idle_timeout_seconds" => cfg.daemon.idle_timeout_seconds().to_string(),
        "statusline.show" => cfg.statusline.show.to_string(),
        "language" => lang_str(cfg.language).to_string(),
        "capsule.format" => capsule_format_str(cfg.capsule.format).to_string(),
        "capsule.language" => lang_str(cfg.capsule.language).to_string(),
        "capsule.next_prompt_max_items" => cfg.capsule.next_prompt_limit().to_string(),
        "capsule.remaining_max_items" => cfg.capsule.remaining_limit().to_string(),
        "capsule.done_max_items" => cfg.capsule.done_limit().to_string(),
        "capsule.risks_max_items" => cfg.capsule.risks_limit().to_string(),
        "theme.preset" => theme_preset_str(cfg.theme.preset).to_string(),
        "theme.codex_color" => cfg.theme.codex_color.to_string(),
        "theme.claude_color" => cfg.theme.claude_color.to_string(),
        "theme.focus_border_color" => cfg.theme.focus_border_color.to_string(),
        "theme.selection_bg_color" => cfg.theme.selection_bg_color.to_string(),
        "theme.selection_fg_color" => cfg.theme.selection_fg_color.to_string(),
        "gui.limit_switch_prompt" => cfg.gui.limit_switch_prompt.to_string(),
        "gui.update_check" => cfg.gui.update_check.to_string(),
        "gui.close_to_tray" => cfg.gui.close_to_tray.to_string(),
        "gui_theme.preset" => gui_theme_preset_str(gui_theme.preset).to_string(),
        "gui_theme.mode" => gui_theme_mode_str(gui_theme.mode).to_string(),
        "gui_theme.light_theme" => gui_theme.light_theme.clone(),
        "gui_theme.dark_theme" => gui_theme.dark_theme.clone(),
        "gui_theme.codex_color" => gui_theme.codex_color.to_string(),
        "gui_theme.claude_color" => gui_theme.claude_color.to_string(),
        "gui_theme.focus_border_color" => gui_theme.focus_border_color.to_string(),
        "gui_theme.selection_bg_color" => gui_theme.selection_bg_color.to_string(),
        "gui_theme.selection_fg_color" => gui_theme.selection_fg_color.to_string(),
        "gui_theme.app_bg_color" => gui_theme.app_bg_color.to_string(),
        "gui_theme.sidebar_bg_color" => gui_theme.sidebar_bg_color.to_string(),
        "gui_theme.panel_bg_color" => gui_theme.panel_bg_color.to_string(),
        "gui_theme.text_color" => gui_theme.text_color.to_string(),
        _ => return Err(ConfigWriteError::UnknownKey(key.to_string())),
    })
}

/// Read the built-in default value for an editable key, formatted like
/// [`get_value`]. UIs use this for reset-to-default and detail previews.
pub fn default_value(key: &str) -> Result<String, ConfigWriteError> {
    get_value(&Config::default(), key)
}

fn mode_str(mode: ModeCfg) -> &'static str {
    match mode {
        ModeCfg::Off => "off",
        ModeCfg::Ask => "ask",
        ModeCfg::Auto => "auto",
    }
}

/// Short code for a [`Language`] (matches its serde representation).
pub fn lang_str(lang: Language) -> &'static str {
    match lang {
        Language::En => "en",
        Language::Ko => "ko",
        Language::Ja => "ja",
    }
}

pub fn capsule_format_str(format: CapsuleFormat) -> &'static str {
    match format {
        CapsuleFormat::Json => "json",
        CapsuleFormat::Md => "md",
    }
}

pub fn theme_preset_str(preset: ThemePreset) -> &'static str {
    match preset {
        ThemePreset::Default => "default",
        ThemePreset::HighContrast => "high_contrast",
        ThemePreset::Mono => "mono",
        ThemePreset::Custom => "custom",
    }
}

pub fn gui_theme_preset_str(preset: GuiThemePreset) -> &'static str {
    match preset {
        GuiThemePreset::White => "white",
        GuiThemePreset::Dark => "dark",
        GuiThemePreset::DarkGreen => "dark_green",
        GuiThemePreset::Custom => "custom",
    }
}

fn validate_theme_contrast(theme: &ThemeConfig, key: &str) -> Result<(), ConfigWriteError> {
    validate_color_contrast(&theme.selection_bg_color, &theme.selection_fg_color, key)
}

fn validate_color_contrast(
    bg_spec: &ColorSpec,
    fg_spec: &ColorSpec,
    key: &str,
) -> Result<(), ConfigWriteError> {
    let Some(bg) = bg_spec.rgb() else {
        return Ok(());
    };
    let Some(fg) = fg_spec.rgb() else {
        return Ok(());
    };
    if contrast_ratio(bg, fg) >= 4.5 {
        return Ok(());
    }
    Err(ConfigWriteError::InvalidValue {
        key: key.to_string(),
        message: "selection foreground/background contrast must be at least 4.5:1".to_string(),
    })
}

fn contrast_ratio(a: (u8, u8, u8), b: (u8, u8, u8)) -> f64 {
    let la = relative_luminance(a);
    let lb = relative_luminance(b);
    let (lighter, darker) = if la >= lb { (la, lb) } else { (lb, la) };
    (lighter + 0.05) / (darker + 0.05)
}

fn relative_luminance((r, g, b): (u8, u8, u8)) -> f64 {
    fn channel(v: u8) -> f64 {
        let s = v as f64 / 255.0;
        if s <= 0.03928 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
}

/// Format a float the way a user typed it: drop a redundant `.0` tail.
fn fmt_f64(n: f64) -> String {
    if n.fract() == 0.0 && n.is_finite() {
        format!("{}", n as i64)
    } else {
        n.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_is_all_defaults() {
        let c = parse("").unwrap();
        let f = c.triggers.five_hour;
        assert!(f.enabled);
        assert_eq!(f.threshold_percent, 80.0);
        assert_eq!(f.mode, ModeCfg::Ask);
        assert!(!f.burn_rate.enabled);
        assert_eq!(f.burn_rate.runway_minutes, 30.0);
        assert!(c.project_overrides.is_empty());
    }

    #[test]
    fn autostart_defaults_to_disabled() {
        assert!(!parse("").unwrap().autostart.enabled);
        assert!(!Config::default().autostart.enabled);
    }

    #[test]
    fn autostart_can_be_enabled_in_config() {
        let c = parse("[autostart]\nenabled = true\n").unwrap();
        assert!(c.autostart.enabled);
        // unrelated sections still default
        assert_eq!(c.triggers.five_hour.threshold_percent, 80.0);
    }

    #[test]
    fn parses_full_global_config() {
        let c = parse(
            "[triggers.five_hour]\n\
             enabled = true\n\
             threshold_percent = 70\n\
             mode = \"auto\"\n\
             [triggers.five_hour.burn_rate]\n\
             enabled = true\n\
             runway_minutes = 15\n",
        )
        .unwrap();
        let f = c.triggers.five_hour;
        assert_eq!(f.threshold_percent, 70.0);
        assert_eq!(f.mode, ModeCfg::Auto);
        assert!(f.burn_rate.enabled);
        assert_eq!(f.burn_rate.runway_minutes, 15.0);
    }

    #[test]
    fn mode_parses_each_lowercase_variant() {
        for (text, want) in [
            ("off", ModeCfg::Off),
            ("ask", ModeCfg::Ask),
            ("auto", ModeCfg::Auto),
        ] {
            let c = parse(&format!("[triggers.five_hour]\nmode = \"{text}\"\n")).unwrap();
            assert_eq!(c.triggers.five_hour.mode, want);
        }
    }

    #[test]
    fn unknown_mode_is_parse_error() {
        assert!(parse("[triggers.five_hour]\nmode = \"weird\"\n").is_err());
    }

    #[test]
    fn partial_section_keeps_other_defaults() {
        // only threshold given; mode/enabled/burn stay default
        let c = parse("[triggers.five_hour]\nthreshold_percent = 55\n").unwrap();
        let f = c.triggers.five_hour;
        assert_eq!(f.threshold_percent, 55.0);
        assert_eq!(f.mode, ModeCfg::Ask);
        assert!(f.enabled);
    }

    #[test]
    fn resolve_without_override_uses_global() {
        let c = parse("[triggers.five_hour]\nthreshold_percent = 80\nmode = \"ask\"\n").unwrap();
        let r = resolve(&c, "fp-none");
        assert!(r.enabled);
        assert_eq!(r.threshold, 80.0);
        assert_eq!(r.mode, TriggerMode::Ask);
        assert!(!r.burn.enabled);
    }

    #[test]
    fn resolve_applies_matching_override_and_inherits_rest() {
        let c = parse(
            "[triggers.five_hour]\n\
             threshold_percent = 80\n\
             mode = \"ask\"\n\
             [project_overrides.\"fpX\".triggers.five_hour]\n\
             threshold_percent = 90\n\
             mode = \"auto\"\n",
        )
        .unwrap();
        let rx = resolve(&c, "fpX");
        assert_eq!(rx.threshold, 90.0);
        assert_eq!(rx.mode, TriggerMode::Auto);
        // a different project still gets the global values
        let ry = resolve(&c, "fpY");
        assert_eq!(ry.threshold, 80.0);
        assert_eq!(ry.mode, TriggerMode::Ask);
    }

    #[test]
    fn resolve_override_with_only_threshold_keeps_global_mode() {
        let c = parse(
            "[triggers.five_hour]\nmode = \"auto\"\n\
             [project_overrides.\"fpX\".triggers.five_hour]\nthreshold_percent = 50\n",
        )
        .unwrap();
        let r = resolve(&c, "fpX");
        assert_eq!(r.threshold, 50.0);
        assert_eq!(r.mode, TriggerMode::Auto); // inherited
    }

    #[test]
    fn resolve_clamps_threshold_into_0_100() {
        let hi = parse("[triggers.five_hour]\nthreshold_percent = 150\n").unwrap();
        assert_eq!(resolve(&hi, "x").threshold, 100.0);
        let lo = parse("[triggers.five_hour]\nthreshold_percent = -5\n").unwrap();
        assert_eq!(resolve(&lo, "x").threshold, 0.0);
    }

    #[test]
    fn load_from_missing_file_is_default() {
        let dir = tempfile::tempdir().unwrap();
        let c = load_from(&dir.path().join("config.toml"));
        assert_eq!(c, Config::default());
        assert_eq!(c.triggers.five_hour.threshold_percent, 80.0);
    }

    #[test]
    fn load_from_malformed_file_is_default_not_panic() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(&p, "this = = not valid toml").unwrap();
        assert_eq!(load_from(&p), Config::default());
    }

    #[test]
    fn load_from_valid_file_parses() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(&p, "[triggers.five_hour]\nthreshold_percent = 42\n").unwrap();
        assert_eq!(load_from(&p).triggers.five_hour.threshold_percent, 42.0);
    }

    #[test]
    fn statusline_defaults_to_show_true() {
        assert!(parse("").unwrap().statusline.show);
        assert!(Config::default().statusline.show);
    }

    #[test]
    fn statusline_show_false_parses() {
        let c = parse("[statusline]\nshow = false\n").unwrap();
        assert!(!c.statusline.show);
        // unrelated sections still default
        assert_eq!(c.triggers.five_hour.threshold_percent, 80.0);
    }

    #[test]
    fn daemon_idle_timeout_defaults_and_is_editable() {
        assert_eq!(Config::default().daemon.idle_timeout_seconds, 60);
        assert_eq!(
            get_value(&Config::default(), "daemon.idle_timeout_seconds").unwrap(),
            "60"
        );
        assert_eq!(
            key_kind("daemon.idle_timeout_seconds"),
            Some(KeyKind::Seconds)
        );

        let text = set_value(None, "daemon.idle_timeout_seconds", "120").unwrap();
        let cfg = parse(&text).unwrap();
        assert_eq!(cfg.daemon.idle_timeout_seconds, 120);
        assert_eq!(
            get_value(&cfg, "daemon.idle_timeout_seconds").unwrap(),
            "120"
        );

        assert!(matches!(
            set_value(None, "daemon.idle_timeout_seconds", "0").unwrap_err(),
            ConfigWriteError::InvalidValue { .. }
        ));
        assert!(matches!(
            set_value(None, "daemon.idle_timeout_seconds", "3601").unwrap_err(),
            ConfigWriteError::InvalidValue { .. }
        ));
    }

    // --- write API -------------------------------------------------------

    #[test]
    fn set_value_on_empty_creates_minimal_toml_that_reparses() {
        let text = set_value(None, "triggers.five_hour.threshold_percent", "70").unwrap();
        // Round-trips through the typed parser to the requested value.
        assert_eq!(
            parse(&text).unwrap().triggers.five_hour.threshold_percent,
            70.0
        );
        // Only an implicit parent header is emitted, not an empty `[triggers]`.
        assert!(text.contains("[triggers.five_hour]"));
        assert!(!text.contains("\n[triggers]\n"));
    }

    #[test]
    fn set_value_preserves_unrelated_content_and_comments() {
        let existing = "\
# keep me
[triggers.five_hour]
threshold_percent = 80
mode = \"ask\"  # inline note

[autostart]
enabled = true
";
        let text = set_value(Some(existing), "triggers.five_hour.mode", "auto").unwrap();
        assert!(text.contains("# keep me"));
        assert!(text.contains("[autostart]"));
        // foreign key untouched
        let c = parse(&text).unwrap();
        assert_eq!(c.triggers.five_hour.mode, ModeCfg::Auto);
        assert_eq!(c.triggers.five_hour.threshold_percent, 80.0);
        assert!(c.autostart.enabled);
    }

    #[test]
    fn set_value_overwrites_existing_leaf() {
        let existing = "[statusline]\nshow = true\n";
        let text = set_value(Some(existing), "statusline.show", "false").unwrap();
        assert!(!parse(&text).unwrap().statusline.show);
    }

    #[test]
    fn set_value_rejects_unknown_key() {
        let err = set_value(None, "triggers.five_hour.nope", "1").unwrap_err();
        assert!(matches!(err, ConfigWriteError::UnknownKey(_)));
    }

    #[test]
    fn set_value_rejects_out_of_range_percent() {
        let err = set_value(None, "triggers.five_hour.threshold_percent", "150").unwrap_err();
        assert!(matches!(err, ConfigWriteError::InvalidValue { .. }));
    }

    #[test]
    fn set_value_rejects_bad_bool_and_mode_and_nonpositive() {
        assert!(matches!(
            set_value(None, "autostart.enabled", "yes").unwrap_err(),
            ConfigWriteError::InvalidValue { .. }
        ));
        assert!(matches!(
            set_value(None, "triggers.five_hour.mode", "loud").unwrap_err(),
            ConfigWriteError::InvalidValue { .. }
        ));
        assert!(matches!(
            set_value(None, "triggers.five_hour.burn_rate.runway_minutes", "0").unwrap_err(),
            ConfigWriteError::InvalidValue { .. }
        ));
    }

    #[test]
    fn set_value_propagates_parse_error_without_clobbering() {
        let err = set_value(Some("this = = not toml"), "statusline.show", "false").unwrap_err();
        assert!(matches!(err, ConfigWriteError::Parse(_)));
    }

    #[test]
    fn set_value_errors_on_non_table_parent() {
        // `triggers` exists but as a scalar — never overwrite it.
        let err = set_value(Some("triggers = 5\n"), "triggers.five_hour.mode", "auto").unwrap_err();
        assert!(matches!(err, ConfigWriteError::ShapeConflict { .. }));
    }

    #[test]
    fn set_value_round_trips_nested_burn_rate() {
        let text = set_value(None, "triggers.five_hour.burn_rate.runway_minutes", "15").unwrap();
        let text = set_value(Some(&text), "triggers.five_hour.burn_rate.enabled", "true").unwrap();
        let c = parse(&text).unwrap();
        assert!(c.triggers.five_hour.burn_rate.enabled);
        assert_eq!(c.triggers.five_hour.burn_rate.runway_minutes, 15.0);
    }

    #[test]
    fn get_value_reports_effective_defaults_and_set_values() {
        let cfg = Config::default();
        assert_eq!(
            get_value(&cfg, "triggers.five_hour.threshold_percent").unwrap(),
            "80"
        );
        assert_eq!(get_value(&cfg, "triggers.five_hour.mode").unwrap(), "ask");
        assert_eq!(get_value(&cfg, "autostart.enabled").unwrap(), "false");
        assert_eq!(get_value(&cfg, "statusline.show").unwrap(), "true");

        let cfg =
            parse("[triggers.five_hour]\nthreshold_percent = 42.5\nmode = \"auto\"\n").unwrap();
        assert_eq!(
            get_value(&cfg, "triggers.five_hour.threshold_percent").unwrap(),
            "42.5"
        );
        assert_eq!(get_value(&cfg, "triggers.five_hour.mode").unwrap(), "auto");
    }

    #[test]
    fn get_value_rejects_unknown_key() {
        assert!(matches!(
            get_value(&Config::default(), "bogus.key").unwrap_err(),
            ConfigWriteError::UnknownKey(_)
        ));
    }

    #[test]
    fn key_kind_maps_each_settable_key() {
        assert_eq!(key_kind("autostart.enabled"), Some(KeyKind::Bool));
        assert_eq!(key_kind("triggers.five_hour.mode"), Some(KeyKind::Mode));
        assert_eq!(
            key_kind("triggers.five_hour.threshold_percent"),
            Some(KeyKind::Percent)
        );
        assert_eq!(
            key_kind("triggers.five_hour.burn_rate.runway_minutes"),
            Some(KeyKind::PosFloat)
        );
        assert_eq!(key_kind("nope"), None);
        // every advertised key has a kind
        for k in settable_keys() {
            assert!(key_kind(k).is_some(), "no kind for {k}");
        }
    }

    #[test]
    fn settable_keys_match_get_value_domain() {
        // every advertised key is readable
        let cfg = Config::default();
        for key in settable_keys() {
            assert!(get_value(&cfg, key).is_ok(), "key {key} not readable");
        }
        assert_eq!(settable_keys().count(), 21);
    }

    #[test]
    fn gui_settable_keys_match_get_value_domain() {
        let cfg = Config::default();
        for key in gui_settable_keys() {
            assert!(get_value(&cfg, key).is_ok(), "key {key} not readable");
            assert!(key_kind(key).is_some(), "no kind for {key}");
        }
        assert_eq!(gui_settable_keys().count(), 16);
    }

    #[test]
    fn gui_limit_switch_prompt_defaults_on_and_round_trips() {
        assert!(Config::default().gui.limit_switch_prompt);
        assert_eq!(
            get_value(&Config::default(), "gui.limit_switch_prompt").unwrap(),
            "true"
        );

        let text = set_value(None, "gui.limit_switch_prompt", "false").unwrap();
        let cfg = parse(&text).unwrap();
        assert!(!cfg.gui.limit_switch_prompt);
        assert_eq!(get_value(&cfg, "gui.limit_switch_prompt").unwrap(), "false");
    }

    #[test]
    fn gui_update_check_defaults_on_and_round_trips() {
        assert!(Config::default().gui.update_check);
        assert_eq!(
            get_value(&Config::default(), "gui.update_check").unwrap(),
            "true"
        );

        let text = set_value(None, "gui.update_check", "false").unwrap();
        let cfg = parse(&text).unwrap();
        assert!(!cfg.gui.update_check);
        assert_eq!(get_value(&cfg, "gui.update_check").unwrap(), "false");
    }

    #[test]
    fn gui_close_to_tray_defaults_off_and_round_trips() {
        assert!(!Config::default().gui.close_to_tray);
        assert_eq!(
            get_value(&Config::default(), "gui.close_to_tray").unwrap(),
            "false"
        );

        let text = set_value(None, "gui.close_to_tray", "true").unwrap();
        let cfg = parse(&text).unwrap();
        assert!(cfg.gui.close_to_tray);
        assert_eq!(get_value(&cfg, "gui.close_to_tray").unwrap(), "true");
    }

    #[test]
    fn language_defaults_to_en_and_parses_each_code() {
        assert_eq!(Config::default().language, Language::En);
        assert_eq!(get_value(&Config::default(), "language").unwrap(), "en");
        for (text, want) in [
            ("en", Language::En),
            ("ko", Language::Ko),
            ("ja", Language::Ja),
        ] {
            let c = parse(&format!("language = \"{text}\"\n")).unwrap();
            assert_eq!(c.language, want);
        }
        assert_eq!(parse("language = \"zh\"\n").unwrap().language, Language::En);
    }

    #[test]
    fn set_value_language_round_trips_and_rejects_bad_code() {
        let text = set_value(None, "language", "ko").unwrap();
        assert_eq!(parse(&text).unwrap().language, Language::Ko);
        assert!(matches!(
            set_value(None, "language", "fr").unwrap_err(),
            ConfigWriteError::InvalidValue { .. }
        ));
    }

    #[test]
    fn key_kind_maps_language() {
        assert_eq!(key_kind("language"), Some(KeyKind::Lang));
    }

    #[test]
    fn defaults_include_capsule_format_and_theme() {
        let cfg = Config::default();
        assert_eq!(cfg.capsule.format, CapsuleFormat::Json);
        assert_eq!(cfg.capsule.language, Language::En);
        assert_eq!(cfg.capsule.next_prompt_max_items, 5);
        assert_eq!(cfg.capsule.remaining_max_items, 5);
        assert_eq!(cfg.capsule.done_max_items, 5);
        assert_eq!(cfg.capsule.risks_max_items, 5);
        assert_eq!(cfg.theme.preset, ThemePreset::Default);
        assert_eq!(cfg.theme.codex_color.as_str(), "#B996EB");
        assert_eq!(cfg.theme.claude_color.as_str(), "#E68C1E");
        assert_eq!(cfg.theme.focus_border_color.as_str(), "#FFA500");
        assert_eq!(cfg.theme.selection_bg_color.as_str(), "cyan");
        assert_eq!(cfg.theme.selection_fg_color.as_str(), "black");
        assert_eq!(cfg.gui_theme.preset, GuiThemePreset::White);
        assert_eq!(cfg.gui_theme.codex_color.as_str(), "#6F7380");
        assert_eq!(cfg.gui_theme.claude_color.as_str(), "#8A7252");
        assert_eq!(cfg.gui_theme.app_bg_color.as_str(), "#F6F7F5");
    }

    #[test]
    fn default_value_reports_theme_and_capsule_defaults() {
        assert_eq!(default_value("capsule.format").unwrap(), "json");
        assert_eq!(default_value("capsule.language").unwrap(), "en");
        assert_eq!(default_value("capsule.remaining_max_items").unwrap(), "5");
        assert_eq!(default_value("theme.preset").unwrap(), "default");
        assert_eq!(
            default_value("theme.focus_border_color").unwrap(),
            "#FFA500"
        );
        assert_eq!(default_value("gui_theme.preset").unwrap(), "white");
        assert_eq!(
            default_value("gui_theme.sidebar_bg_color").unwrap(),
            "#EEF1ED"
        );
    }

    #[test]
    fn set_value_accepts_capsule_format_theme_and_color() {
        let text = set_value(None, "capsule.format", "md").unwrap();
        let text = set_value(Some(&text), "capsule.language", "ko").unwrap();
        let text = set_value(Some(&text), "capsule.remaining_max_items", "3").unwrap();
        let text = set_value(Some(&text), "theme.preset", "high_contrast").unwrap();
        let text = set_value(Some(&text), "theme.codex_color", "#B996EB").unwrap();
        let cfg = parse(&text).unwrap();

        assert_eq!(cfg.capsule.format, CapsuleFormat::Md);
        assert_eq!(cfg.capsule.language, Language::Ko);
        assert_eq!(cfg.capsule.remaining_max_items, 3);
        assert_eq!(cfg.theme.preset, ThemePreset::HighContrast);
        assert_eq!(cfg.theme.codex_color.as_str(), "#B996EB");
    }

    #[test]
    fn capsule_language_is_editable_and_rejects_bad_code() {
        assert_eq!(key_kind("capsule.language"), Some(KeyKind::Lang));

        for (raw, want) in [
            ("ko", Language::Ko),
            ("ja", Language::Ja),
            ("en", Language::En),
        ] {
            let text = set_value(None, "capsule.language", raw).unwrap();
            let cfg = parse(&text).unwrap();
            assert_eq!(cfg.capsule.language, want);
            assert_eq!(get_value(&cfg, "capsule.language").unwrap(), raw);
        }

        assert!(matches!(
            set_value(None, "capsule.language", "fr").unwrap_err(),
            ConfigWriteError::InvalidValue { .. }
        ));
    }

    #[test]
    fn capsule_item_limits_are_editable_positive_counts() {
        assert_eq!(
            key_kind("capsule.next_prompt_max_items"),
            Some(KeyKind::Count)
        );

        let text = set_value(None, "capsule.done_max_items", "9").unwrap();
        assert_eq!(parse(&text).unwrap().capsule.done_max_items, 9);

        let err = set_value(None, "capsule.risks_max_items", "0").unwrap_err();
        assert!(matches!(err, ConfigWriteError::InvalidValue { .. }));
    }

    #[test]
    fn setting_theme_preset_writes_preset_colors() {
        let text = set_value(None, "theme.preset", "mono").unwrap();
        let cfg = parse(&text).unwrap();

        assert_eq!(cfg.theme.preset, ThemePreset::Mono);
        assert_eq!(cfg.theme.codex_color.as_str(), "white");
        assert_eq!(cfg.theme.claude_color.as_str(), "gray");
        assert_eq!(cfg.theme.focus_border_color.as_str(), "white");
        assert_eq!(cfg.theme.selection_bg_color.as_str(), "white");
        assert_eq!(cfg.theme.selection_fg_color.as_str(), "black");
    }

    #[test]
    fn setting_gui_theme_preset_writes_preset_colors() {
        let text = set_value(None, "gui_theme.preset", "dark").unwrap();
        let cfg = parse(&text).unwrap();

        assert_eq!(cfg.gui_theme.preset, GuiThemePreset::Dark);
        assert_eq!(cfg.gui_theme.focus_border_color.as_str(), "#8B8374");
        assert_eq!(cfg.gui_theme.selection_bg_color.as_str(), "#3B3E3D");
        assert_eq!(cfg.gui_theme.selection_fg_color.as_str(), "#F1EFE7");
        assert_eq!(cfg.gui_theme.app_bg_color.as_str(), "#121314");
        assert_eq!(cfg.gui_theme.sidebar_bg_color.as_str(), "#171918");
        assert_eq!(cfg.gui_theme.panel_bg_color.as_str(), "#1D2020");
        assert_eq!(cfg.gui_theme.text_color.as_str(), "#E8E6DC");

        let text = set_value(None, "gui_theme.preset", "dark_green").unwrap();
        let cfg = parse(&text).unwrap();

        assert_eq!(cfg.gui_theme.preset, GuiThemePreset::DarkGreen);
        assert_eq!(cfg.gui_theme.focus_border_color.as_str(), "#68A782");
        assert_eq!(cfg.gui_theme.selection_bg_color.as_str(), "#315F49");
        assert_eq!(cfg.gui_theme.app_bg_color.as_str(), "#171B19");
        assert_eq!(cfg.gui_theme.panel_bg_color.as_str(), "#202622");
        assert_eq!(cfg.theme.preset, ThemePreset::Default);
    }

    #[test]
    fn setting_gui_theme_color_switches_preset_to_custom() {
        let text = set_value(None, "gui_theme.preset", "dark").unwrap();
        let text = set_value(Some(&text), "gui_theme.codex_color", "#123456").unwrap();
        let cfg = parse(&text).unwrap();

        assert_eq!(cfg.gui_theme.preset, GuiThemePreset::Custom);
        assert_eq!(cfg.gui_theme.codex_color.as_str(), "#123456");
        assert_eq!(cfg.gui_theme.selection_bg_color.as_str(), "#3B3E3D");
    }

    #[test]
    fn high_contrast_preset_uses_distinct_agent_colors() {
        let theme = theme_config_for_preset(ThemePreset::HighContrast);

        assert_eq!(theme.codex_color.as_str(), "light-blue");
        assert_eq!(theme.claude_color.as_str(), "light-yellow");
        assert_ne!(theme.codex_color, theme.claude_color);
    }

    #[test]
    fn set_value_rejects_invalid_color_and_bad_selection_contrast() {
        assert!(matches!(
            set_value(None, "theme.codex_color", "not a color").unwrap_err(),
            ConfigWriteError::InvalidValue { .. }
        ));

        let text = set_value(None, "theme.selection_bg_color", "white").unwrap();
        let err = set_value(Some(&text), "theme.selection_fg_color", "white").unwrap_err();
        assert!(matches!(err, ConfigWriteError::InvalidValue { .. }));
    }

    #[test]
    fn set_then_load_from_disk_is_visible_to_daemon() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        let text = set_value(None, "triggers.five_hour.threshold_percent", "65").unwrap();
        std::fs::write(&p, text).unwrap();
        // The daemon's loader sees the written value.
        assert_eq!(load_from(&p).triggers.five_hour.threshold_percent, 65.0);
    }
}
