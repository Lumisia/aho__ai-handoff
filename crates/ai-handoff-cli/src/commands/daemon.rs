use crate::DaemonAction;
use std::path::Path;

pub fn run(action: DaemonAction, stay_alive: bool, home: Option<&Path>) -> anyhow::Result<i32> {
    if let Some(home) = home {
        std::env::set_var("AI_HANDOFF_HOME", home);
    }

    match action {
        DaemonAction::Run => Ok(ai_handoff_daemon::run(stay_alive)),
        DaemonAction::Status => {
            let status = if crate::daemon_supply::ping_daemon(std::time::Duration::from_millis(750))
            {
                "reachable"
            } else {
                "unreachable"
            };
            println!("daemon status: {status}");
            Ok(0)
        }
    }
}
