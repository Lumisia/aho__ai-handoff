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
        #[arg(long)]
        stay_alive: bool,
    },
    Doctor {
        #[arg(long)]
        json: bool,
        /// Repair what a local command can (runtime dirs, daemon start) and
        /// print next steps for anything that needs the user.
        #[arg(long)]
        fix: bool,
    },
    Checkpoint {
        /// Draft format (md or json), or the agent-facing guidance mode.
        #[arg(value_enum)]
        action: Option<CheckpointAction>,
        #[arg(long)]
        message: Option<String>,
        /// Agent id writing the capsule (e.g. codex, claude-code, grok).
        /// Defaults to codex when omitted.
        #[arg(long)]
        agent: Option<String>,
        /// Preferred consumer of the capsule (routing hint). Omitted means an
        /// open capsule that any agent may pick up.
        #[arg(long)]
        target: Option<String>,
        /// Read the JSON or Markdown capsule body from this file instead of
        /// stdin. Avoids native stdin quirks in shells such as PowerShell.
        #[arg(long)]
        file: Option<std::path::PathBuf>,
        /// Override the configured format. Primarily used with guidance.
        #[arg(long, value_enum)]
        format: Option<CheckpointFormatArg>,
        /// Emit compact JSON from the agent-facing guidance command.
        #[arg(long)]
        json: bool,
    },
    /// Consume the pending handoff capsule for this project (the /handoff
    /// skill's backend). Prints hook-style JSON; `{}` means nothing pending.
    Handoff {
        /// Agent id consuming the capsule. Open capsules and capsules
        /// targeting this agent match; any string id is accepted.
        #[arg(long, default_value = "codex")]
        agent: String,
        /// Preview the pending capsule without consuming it.
        #[arg(long)]
        peek: bool,
        /// Also claim a capsule that targets a different agent (recorded on
        /// the capsule as consumed_despite_target).
        #[arg(long)]
        force: bool,
        /// Consume exactly this capsule id (from `--peek` / the TUI), even if
        /// it targets another agent. Safer than --force when several capsules
        /// are pending.
        #[arg(long)]
        id: Option<String>,
    },
    /// Point a pending capsule at a different agent, or open it up for anyone
    /// when --to is omitted. The fix-up for a capsule saved with the wrong
    /// target.
    Retarget {
        /// The capsule id shown by `handoff --peek` / the TUI.
        capsule_id: String,
        /// New preferred consumer (e.g. grok). Omit to make the capsule open.
        #[arg(long)]
        to: Option<String>,
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
        /// Also remove the desktop GUI app (GUI removal includes the TUI/CLI).
        #[arg(long)]
        gui: bool,
        /// Remove everything: GUI + TUI/CLI + local store/log/ipc data.
        #[arg(long)]
        all: bool,
        /// Skip interactive confirmation prompts.
        #[arg(long)]
        yes: bool,
    },
    /// Update the CLI/TUI (and optionally the GUI) to the latest release.
    Update {
        /// Also download and run the latest desktop GUI installer.
        #[arg(long)]
        gui: bool,
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
pub enum CheckpointAction {
    Md,
    Json,
    Guidance,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum CheckpointFormatArg {
    Md,
    Json,
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
    /// Show the signed-in account + local plan/limits for both agents.
    Status {
        #[arg(long)]
        json: bool,
        /// Fetch Claude usage through the active saved slot's credential.
        #[arg(long)]
        fetch: bool,
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
        Some(Commands::Daemon { action, stay_alive }) => commands::daemon::run(action, stay_alive),
        Some(Commands::Doctor { json, fix }) => commands::doctor::run(json, fix),
        Some(Commands::Checkpoint {
            action,
            message,
            agent,
            target,
            file,
            format,
            json,
        }) => commands::checkpoint::run(action, format, json, message, agent, target, file),
        Some(Commands::Handoff {
            agent,
            peek,
            force,
            id,
        }) => commands::handoff::run(&agent, peek, force, id.as_deref()),
        Some(Commands::Retarget { capsule_id, to }) => commands::retarget::run(&capsule_id, to),
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
            gui,
            all,
            yes,
        }) => commands::uninstall::run(commands::uninstall::UninstallOptions {
            keep_store,
            purge_store,
            gui,
            all,
            yes,
        }),
        Some(Commands::Update { gui }) => commands::update::run(gui),
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
    fn parses_daemon_run_stay_alive_flag() {
        let cli = Cli::try_parse_from(["ai-handoff", "daemon", "run", "--stay-alive"]).unwrap();

        match cli.command {
            Some(Commands::Daemon { action, stay_alive }) => {
                assert_eq!(action, DaemonAction::Run);
                assert!(stay_alive);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parses_handoff_command_with_agent() {
        let cli = Cli::try_parse_from(["ai-handoff", "handoff", "--agent", "claude-code"]).unwrap();
        match cli.command {
            Some(Commands::Handoff {
                agent, peek, force, ..
            }) => {
                assert_eq!(agent, "claude-code");
                assert!(!peek);
                assert!(!force);
            }
            other => panic!("unexpected command: {other:?}"),
        }

        // Bare `handoff` defaults to codex (mirrors the hook command).
        let bare = Cli::try_parse_from(["ai-handoff", "handoff"]).unwrap();
        match bare.command {
            Some(Commands::Handoff {
                agent, peek, force, ..
            }) => {
                assert_eq!(agent, "codex");
                assert!(!peek);
                assert!(!force);
            }
            other => panic!("unexpected command: {other:?}"),
        }

        // --peek previews without consuming.
        let peek = Cli::try_parse_from(["ai-handoff", "handoff", "--peek"]).unwrap();
        assert!(matches!(
            peek.command,
            Some(Commands::Handoff { peek: true, .. })
        ));
    }

    #[test]
    fn parses_handoff_with_unknown_agent_and_force() {
        // Agent ids are open strings — future agents parse without a CLI change.
        let cli =
            Cli::try_parse_from(["ai-handoff", "handoff", "--agent", "grok", "--force"]).unwrap();
        match cli.command {
            Some(Commands::Handoff {
                agent, peek, force, ..
            }) => {
                assert_eq!(agent, "grok");
                assert!(!peek);
                assert!(force);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parses_handoff_with_explicit_capsule_id() {
        let cli = Cli::try_parse_from([
            "ai-handoff",
            "handoff",
            "--agent",
            "grok",
            "--id",
            "cap_20260709_010101_abcd",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::Handoff { agent, id, .. }) => {
                assert_eq!(agent, "grok");
                assert_eq!(id.as_deref(), Some("cap_20260709_010101_abcd"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parses_checkpoint_with_target() {
        let cli = Cli::try_parse_from([
            "ai-handoff",
            "checkpoint",
            "--agent",
            "claude-code",
            "--target",
            "grok",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::Checkpoint { agent, target, .. }) => {
                assert_eq!(agent.as_deref(), Some("claude-code"));
                assert_eq!(target.as_deref(), Some("grok"));
            }
            other => panic!("unexpected command: {other:?}"),
        }

        // No --target → open capsule (daemon stores target null).
        let open = Cli::try_parse_from(["ai-handoff", "checkpoint"]).unwrap();
        match open.command {
            Some(Commands::Checkpoint { target, .. }) => assert_eq!(target, None),
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parses_retarget_command() {
        let cli =
            Cli::try_parse_from(["ai-handoff", "retarget", "cap_123", "--to", "gemini"]).unwrap();
        match cli.command {
            Some(Commands::Retarget { capsule_id, to }) => {
                assert_eq!(capsule_id, "cap_123");
                assert_eq!(to.as_deref(), Some("gemini"));
            }
            other => panic!("unexpected command: {other:?}"),
        }

        // Without --to the capsule opens up for any agent.
        let open = Cli::try_parse_from(["ai-handoff", "retarget", "cap_123"]).unwrap();
        match open.command {
            Some(Commands::Retarget { capsule_id, to }) => {
                assert_eq!(capsule_id, "cap_123");
                assert_eq!(to, None);
            }
            other => panic!("unexpected command: {other:?}"),
        }
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
    fn parses_uninstall_scope_flags() {
        let bare = Cli::try_parse_from(["ai-handoff", "uninstall"]).unwrap();
        match bare.command {
            Some(Commands::Uninstall { gui, all, yes, .. }) => {
                assert!(!gui);
                assert!(!all);
                assert!(!yes);
            }
            other => panic!("unexpected command: {other:?}"),
        }

        let full =
            Cli::try_parse_from(["ai-handoff", "uninstall", "--gui", "--all", "--yes"]).unwrap();
        match full.command {
            Some(Commands::Uninstall { gui, all, yes, .. }) => {
                assert!(gui);
                assert!(all);
                assert!(yes);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parses_update_command() {
        let bare = Cli::try_parse_from(["ai-handoff", "update"]).unwrap();
        assert!(matches!(
            bare.command,
            Some(Commands::Update { gui: false })
        ));

        let gui = Cli::try_parse_from(["ai-handoff", "update", "--gui"]).unwrap();
        assert!(matches!(gui.command, Some(Commands::Update { gui: true })));
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
                action: AccountAction::Status { json, fetch },
            }) => {
                assert!(!json);
                assert!(!fetch);
            }
            other => panic!("unexpected command: {other:?}"),
        }
        match Cli::parse_from(["ai-handoff", "account", "status", "--fetch"]).command {
            Some(Commands::Account {
                action: AccountAction::Status { fetch, .. },
            }) => assert!(fetch),
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
    #[test]
    fn parses_checkpoint_format_actions_and_guidance() {
        let md = Cli::try_parse_from([
            "ai-handoff",
            "checkpoint",
            "md",
            "--agent",
            "codex",
            "--file",
            "checkpoint.md",
        ])
        .unwrap();
        match md.command {
            Some(Commands::Checkpoint {
                action: Some(CheckpointAction::Md),
                agent,
                file,
                ..
            }) => {
                assert_eq!(agent.as_deref(), Some("codex"));
                assert_eq!(file.unwrap(), std::path::PathBuf::from("checkpoint.md"));
            }
            other => panic!("unexpected command: {other:?}"),
        }

        let json = Cli::try_parse_from([
            "ai-handoff",
            "checkpoint",
            "json",
            "--agent",
            "claude-code",
            "--file",
            "checkpoint.json",
        ])
        .unwrap();
        assert!(matches!(
            json.command,
            Some(Commands::Checkpoint {
                action: Some(CheckpointAction::Json),
                ..
            })
        ));

        let guidance = Cli::try_parse_from([
            "ai-handoff",
            "checkpoint",
            "guidance",
            "--agent",
            "claude-code",
            "--format",
            "md",
            "--json",
        ])
        .unwrap();
        match guidance.command {
            Some(Commands::Checkpoint {
                action: Some(CheckpointAction::Guidance),
                format: Some(CheckpointFormatArg::Md),
                agent,
                json,
                ..
            }) => {
                assert_eq!(agent.as_deref(), Some("claude-code"));
                assert!(json);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }
}
