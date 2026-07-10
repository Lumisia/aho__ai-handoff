//! Shared process-spawn helper for everything reachable from the background
//! host. The host runs with the Windows GUI subsystem and therefore has no
//! console; a console-subsystem child spawned from it without
//! `CREATE_NO_WINDOW` allocates a brand-new console, which the OS opens as a
//! visible terminal window (Windows Terminal on Windows 11). Route every such
//! spawn through [`no_window_command`] so no code path can flash a window.

use std::ffi::OsStr;
use std::process::Command;

/// `Command::new(program)` that never opens a console window on Windows.
pub fn no_window_command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}
