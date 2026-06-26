use clap::{Parser, Subcommand, ValueEnum};

pub mod commands;

#[derive(Debug, Parser)]
#[command(name = "ai-handoff")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
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
    },
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
        Commands::Hook { event, agent } => commands::hook::run(&event, agent),
        Commands::Daemon { action } => commands::daemon::run(action),
        Commands::Doctor { json } => commands::doctor::run(json),
        Commands::Checkpoint { message } => commands::checkpoint::run(message),
        Commands::Dashboard => commands::dashboard::run(),
        Commands::Install {
            dry_run,
            yes,
            agents,
            no_plugin,
        } => commands::install::run(
            dry_run,
            yes,
            if agents.is_empty() {
                None
            } else {
                Some(agents)
            },
            no_plugin,
        ),
        Commands::Uninstall {
            keep_store,
            purge_store,
        } => commands::uninstall::run(keep_store, purge_store),
        Commands::Statusline => commands::statusline::run(),
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
            Commands::Hook { event, agent } => {
                assert_eq!(event, "session-start");
                assert_eq!(agent, AgentArg::Codex);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parses_dashboard_command() {
        let cli = Cli::try_parse_from(["ai-handoff", "dashboard"]).unwrap();

        assert!(matches!(cli.command, Commands::Dashboard));
    }

    #[test]
    fn parses_statusline_command() {
        let cli = Cli::try_parse_from(["ai-handoff", "statusline"]).unwrap();

        assert!(matches!(cli.command, Commands::Statusline));
    }

    #[test]
    fn install_defaults_to_plugin_mode_and_accepts_no_plugin_flag() {
        let default = Cli::try_parse_from(["ai-handoff", "install"]).unwrap();
        match default.command {
            Commands::Install { no_plugin, .. } => assert!(!no_plugin),
            other => panic!("unexpected command: {other:?}"),
        }

        let legacy = Cli::try_parse_from(["ai-handoff", "install", "--no-plugin"]).unwrap();
        match legacy.command {
            Commands::Install { no_plugin, .. } => assert!(no_plugin),
            other => panic!("unexpected command: {other:?}"),
        }
    }
}
