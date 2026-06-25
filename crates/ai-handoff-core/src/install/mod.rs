pub mod backup;
pub mod detect;
pub mod state;

pub use backup::{backup_file, backup_path};
pub use detect::{detect_agents, targets_for, AgentPresence, InstallTargets};
pub use state::{load, save, state_path, ClaudeState, CodexState, FileMod, InstallState};
