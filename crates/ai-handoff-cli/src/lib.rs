use clap::{Parser, Subcommand, ValueEnum};

pub mod commands;

#[derive(Debug, Parser)]
#[command(name = "ai-handoff")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    Hook {
        event: String,
        #[arg(long, value_enum, default_value_t = AgentArg::Codex)]
        agent: AgentArg,
    },
    Daemon {
        #[arg(value_enum)]
        action: DaemonAction,
    },
    Doctor {
        #[arg(long)]
        json: bool,
    },
    Checkpoint {
        #[arg(long)]
        message: Option<String>,
        /// Source agent writing the capsule (codex or claude-code). Sets the
        /// handoff direction; defaults to codex when omitted.
        #[arg(long)]
        agent: Option<String>,
        /// Read the JSON capsule body from this file instead of stdin. Avoids
        /// shell stdin quirks (PowerShell does not pipe to native stdin).
        #[arg(long)]
        file: Option<std::path::PathBuf>,
    },
    /// Consume the pending handoff capsule for this project (the /handoff
    /// skill's backend). Prints hook-style JSON; `{}` means nothing pending.
    Handoff {
        /// The agent consuming the capsule (sets which capsules match).
        #[arg(long, value_enum, default_value_t = AgentArg::Codex)]
        agent: AgentArg,
    },
    Tui,
    Dashboard,
    Install {
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        yes: bool,
        #[arg(long, value_delimiter = ',')]
        agents: Vec<String>,
        /// Use the legacy direct-hook patch instead of the plugin bundle.
        #[arg(long)]
        no_plugin: bool,
    },
    Uninstall {
        #[arg(long)]
        keep_store: bool,
        #[arg(long)]
        purge_store: bool,
    },
    Statusline,
    /// View or edit the shared config (applies to Claude and Codex).
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Enable, disable, or show the run-the-daemon-at-logon autostart entry.
    Autostart {
        #[arg(value_enum)]
        action: AutostartAction,
    },
    /// Inspect saved accounts and live status (add/switch/launch live in the TUI).
    Account {
        #[command(subcommand)]
        action: AccountAction,
    },
    /// Show estimated token usage from local Claude + Codex logs.
    Usage {
        /// Break down by this dimension instead of the default summary.
        #[arg(long, value_enum)]
        group_by: Option<GroupByArg>,
        /// Restrict to one agent.
        #[arg(long, value_enum)]
        source: Option<SourceArg>,
        /// Only count usage on or after this day (YYYY-MM-DD).
        #[arg(long)]
        since: Option<String>,
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum GroupByArg {
    Day,
    Model,
    Project,
    Source,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum SourceArg {
    Claude,
    Codex,
}

#[derive(Debug, Subcommand)]
pub enum ConfigAction {
    /// Print the effective value of a key (built-in default when unset).
    Get { key: String },
    /// Set a key to a value, writing ~/.ai-handoff/config.toml (never-clobber).
    Set { key: String, value: String },
    /// List every editable key with its effective value.
    List,
}

#[derive(Debug, Subcommand)]
pub enum AccountAction {
    /// List saved account slots for both agents.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Show the signed-in account + live plan/limits for both agents.
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Diagnose account setup (sign-in, vault slots, vendor CLIs on PATH).
    Doctor {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum AutostartAction {
    /// Register the daemon to run at logon and set autostart.enabled = true.
    On,
    /// Remove any logon entry (scheduled task + Run key) and set it false.
    Off,
    /// Print the config flag and whether a real entry is registered.
    Status,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum AgentArg {
    #[value(name = "claude-code")]
    ClaudeCode,
    Codex,
}

impl AgentArg {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum DaemonAction {
    Run,
    Status,
}

pub fn main_entry() -> anyhow::Result<i32> {
    run_cli(Cli::parse())
}

pub fn run_cli(cli: Cli) -> anyhow::Result<i32> {
    match cli.command {
        None | Some(Commands::Tui) => commands::tui::run(),
        Some(Commands::Hook { event, agent }) => commands::hook::run(&event, agent),
        Some(Commands::Daemon { action }) => commands::daemon::run(action),
        Some(Commands::Doctor { json }) => commands::doctor::run(json),
        Some(Commands::Checkpoint {
            message,
            agent,
            file,
        }) => commands::checkpoint::run(message, agent, file),
        Some(Commands::Handoff { agent }) => commands::handoff::run(agent),
        Some(Commands::Dashboard) => commands::dashboard::run(),
        Some(Commands::Install {
            dry_run,
            yes,
            agents,
            no_plugin,
        }) => commands::install::run(
            dry_run,
            yes,
            if agents.is_empty() {
                None
            } else {
                Some(agents)
            },
            no_plugin,
        ),
        Some(Commands::Uninstall {
            keep_store,
            purge_store,
        }) => commands::uninstall::run(keep_store, purge_store),
        Some(Commands::Statusline) => commands::statusline::run(),
        Some(Commands::Autostart { action }) => commands::autostart::run_cli(action),
        Some(Commands::Config { action }) => commands::config::run(action),
        Some(Commands::Account { action }) => commands::account::run(action),
        Some(Commands::Usage {
            group_by,
            source,
            since,
            json,
        }) => commands::usage::run(group_by, source, since, json),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parses_hook_command() {
        let cli = Cli::try_parse_from(["ai-handoff", "hook", "session-start", "--agent", "codex"])
            .unwrap();

        match cli.command {
            Some(Commands::Hook { event, agent }) => {
                assert_eq!(event, "session-start");
                assert_eq!(agent, AgentArg::Codex);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parses_handoff_command_with_agent() {
        let cli = Cli::try_parse_from(["ai-handoff", "handoff", "--agent", "claude-code"]).unwrap();
        match cli.command {
            Some(Commands::Handoff { agent }) => assert_eq!(agent, AgentArg::ClaudeCode),
            other => panic!("unexpected command: {other:?}"),
        }

        // Bare `handoff` defaults to codex (mirrors the hook command).
        let bare = Cli::try_parse_from(["ai-handoff", "handoff"]).unwrap();
        assert!(matches!(
            bare.command,
            Some(Commands::Handoff {
                agent: AgentArg::Codex
            })
        ));
    }

    #[test]
    fn parses_dashboard_command() {
        let cli = Cli::try_parse_from(["ai-handoff", "dashboard"]).unwrap();

        assert!(matches!(cli.command, Some(Commands::Dashboard)));
    }

    #[test]
    fn parses_no_command_for_tui_default() {
        let cli = Cli::try_parse_from(["ai-handoff"]).unwrap();

        assert!(cli.command.is_none());
    }

    #[test]
    fn parses_tui_command() {
        let cli = Cli::try_parse_from(["ai-handoff", "tui"]).unwrap();

        assert!(matches!(cli.command, Some(Commands::Tui)));
    }

    #[test]
    fn parses_statusline_command() {
        let cli = Cli::try_parse_from(["ai-handoff", "statusline"]).unwrap();

        assert!(matches!(cli.command, Some(Commands::Statusline)));
    }

    #[test]
    fn parses_config_get_set_list() {
        let get = Cli::try_parse_from(["ai-handoff", "config", "get", "statusline.show"]).unwrap();
        match get.command {
            Some(Commands::Config {
                action: ConfigAction::Get { key },
            }) => assert_eq!(key, "statusline.show"),
            other => panic!("unexpected command: {other:?}"),
        }

        let set = Cli::try_parse_from([
            "ai-handoff",
            "config",
            "set",
            "triggers.five_hour.mode",
            "auto",
        ])
        .unwrap();
        match set.command {
            Some(Commands::Config {
                action: ConfigAction::Set { key, value },
            }) => {
                assert_eq!(key, "triggers.five_hour.mode");
                assert_eq!(value, "auto");
            }
            other => panic!("unexpected command: {other:?}"),
        }

        let list = Cli::try_parse_from(["ai-handoff", "config", "list"]).unwrap();
        assert!(matches!(
            list.command,
            Some(Commands::Config {
                action: ConfigAction::List
            })
        ));
    }

    #[test]
    fn parses_usage_with_flags() {
        let cli = Cli::try_parse_from([
            "ai-handoff",
            "usage",
            "--group-by",
            "model",
            "--source",
            "codex",
            "--since",
            "2026-06-25",
            "--json",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::Usage {
                group_by,
                source,
                since,
                json,
            }) => {
                assert_eq!(group_by, Some(GroupByArg::Model));
                assert_eq!(source, Some(SourceArg::Codex));
                assert_eq!(since.as_deref(), Some("2026-06-25"));
                assert!(json);
            }
            other => panic!("unexpected command: {other:?}"),
        }

        // Bare `usage` defaults everything to None/false.
        let bare = Cli::try_parse_from(["ai-handoff", "usage"]).unwrap();
        assert!(matches!(
            bare.command,
            Some(Commands::Usage {
                group_by: None,
                source: None,
                since: None,
                json: false
            })
        ));
    }

    #[test]
    fn install_defaults_to_plugin_mode_and_accepts_no_plugin_flag() {
        let default = Cli::try_parse_from(["ai-handoff", "install"]).unwrap();
        match default.command {
            Some(Commands::Install { no_plugin, .. }) => assert!(!no_plugin),
            other => panic!("unexpected command: {other:?}"),
        }

        let legacy = Cli::try_parse_from(["ai-handoff", "install", "--no-plugin"]).unwrap();
        match legacy.command {
            Some(Commands::Install { no_plugin, .. }) => assert!(no_plugin),
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parses_account_actions() {
        match Cli::parse_from(["ai-handoff", "account", "list", "--json"]).command {
            Some(Commands::Account {
                action: AccountAction::List { json },
            }) => assert!(json),
            other => panic!("unexpected command: {other:?}"),
        }
        match Cli::parse_from(["ai-handoff", "account", "status"]).command {
            Some(Commands::Account {
                action: AccountAction::Status { json },
            }) => assert!(!json),
            other => panic!("unexpected command: {other:?}"),
        }
        match Cli::parse_from(["ai-handoff", "account", "doctor"]).command {
            Some(Commands::Account {
                action: AccountAction::Doctor { .. },
            }) => {}
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parses_autostart_actions() {
        for (arg, want) in [
            ("on", AutostartAction::On),
            ("off", AutostartAction::Off),
            ("status", AutostartAction::Status),
        ] {
            match Cli::parse_from(["ai-handoff", "autostart", arg]).command {
                Some(Commands::Autostart { action }) => assert_eq!(action, want),
                other => panic!("unexpected command: {other:?}"),
            }
        }
    }
}
