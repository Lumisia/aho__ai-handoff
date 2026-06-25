pub mod backup;
pub mod claude;
pub mod codex_config;
pub mod codex_hooks;
pub mod detect;
pub mod diff;
pub mod duplicate;
pub mod state;

pub use backup::{backup_file, backup_path};
pub use codex_hooks::{apply, managed_command, remove, EVENTS};
pub use detect::{detect_agents, targets_for, AgentPresence, InstallTargets};
pub use state::{load, save, state_path, ClaudeState, CodexState, FileMod, InstallState};
