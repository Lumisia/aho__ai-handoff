use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::Parser;

#[derive(Clone, Debug, Parser, PartialEq, Eq)]
#[command(name = "ai-handoff-host")]
pub struct HostArgs {
    /// Fixed AI Handoff home used by the OS-managed background daemon.
    #[arg(long)]
    pub home: PathBuf,
    /// Keep running instead of exiting after the configured idle timeout.
    #[arg(long)]
    pub stay_alive: bool,
}

impl HostArgs {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.home.is_absolute(),
            "AI Handoff host home must be absolute"
        );
        Ok(())
    }
}

pub fn run(args: HostArgs) -> anyhow::Result<i32> {
    args.validate()
        .context("validate background host arguments")?;
    install_shutdown_listener(&args.home)?;
    run_with(args, ai_handoff_daemon::run)
}

fn shutdown_event_name(home: &Path) -> String {
    let normalized = home.to_string_lossy().replace('/', "\\").to_lowercase();
    let hash = normalized
        .as_bytes()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325_u64, |value, byte| {
            (value ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
        });
    format!(r"Local\AIHandoffHost-{hash:016x}")
}

#[cfg(windows)]
fn install_shutdown_listener(home: &Path) -> anyhow::Result<()> {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject, INFINITE};

    let name = HSTRING::from(shutdown_event_name(home));
    let event = unsafe { CreateEventW(None, true, false, &name) }
        .context("create native host shutdown event")?;
    // HANDLE is intentionally !Send in windows-rs. The numeric kernel handle
    // value is process-local and safe to transfer to the one owning waiter.
    let event_raw = event.0 as isize;
    std::thread::spawn(move || unsafe {
        let event = HANDLE(event_raw as *mut std::ffi::c_void);
        let result = WaitForSingleObject(event, INFINITE);
        let _ = CloseHandle(event);
        if result == WAIT_OBJECT_0 {
            std::process::exit(0);
        }
    });
    Ok(())
}

#[cfg(not(windows))]
fn install_shutdown_listener(_home: &Path) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(windows)]
pub(crate) fn signal_shutdown(home: &Path) -> anyhow::Result<bool> {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenEventW, SetEvent, EVENT_MODIFY_STATE};

    let name = HSTRING::from(shutdown_event_name(home));
    let event = match unsafe { OpenEventW(EVENT_MODIFY_STATE, false, &name) } {
        Ok(event) => event,
        Err(error) if error.code().0 as u32 == 0x8007_0002 => return Ok(false),
        Err(error) => return Err(error).context("open native host shutdown event"),
    };
    unsafe {
        SetEvent(event).context("signal native host shutdown event")?;
        CloseHandle(event).context("close native host shutdown event")?;
    }
    Ok(true)
}

#[cfg(not(windows))]
pub(crate) fn signal_shutdown(_home: &Path) -> anyhow::Result<bool> {
    Ok(false)
}

fn run_with(args: HostArgs, daemon: impl FnOnce(bool) -> i32) -> anyhow::Result<i32> {
    args.validate()
        .context("validate background host arguments")?;
    std::env::set_var("AI_HANDOFF_HOME", &args.home);
    Ok(daemon(args.stay_alive))
}

#[cfg(test)]
mod tests {
    use super::*;

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn shutdown_event_identity_is_case_insensitive_and_home_scoped() {
        let upper = shutdown_event_name(std::path::Path::new(r"C:\Users\ME\.ai-handoff"));
        let lower = shutdown_event_name(std::path::Path::new(r"c:/users/me/.ai-handoff"));
        let other = shutdown_event_name(std::path::Path::new(r"C:\Users\ME\other-home"));

        assert_eq!(upper, lower);
        assert_ne!(upper, other);
        assert!(upper.starts_with(r"Local\AIHandoffHost-"));
    }

    #[test]
    fn host_args_require_an_absolute_home() {
        let absolute = std::env::current_dir().unwrap().join("host-home");
        let args =
            HostArgs::try_parse_from(["ai-handoff-host", "--home", absolute.to_str().unwrap()])
                .unwrap();
        assert_eq!(args.home, absolute);
        assert!(!args.stay_alive);
        assert!(args.validate().is_ok());

        let relative =
            HostArgs::try_parse_from(["ai-handoff-host", "--home", "relative-home"]).unwrap();
        assert!(relative.validate().is_err());
    }

    #[test]
    fn host_runtime_sets_home_before_running_daemon() {
        let _guard = ENV_LOCK.lock().unwrap();
        let home = std::env::current_dir().unwrap().join("host-runtime-home");
        let previous = std::env::var_os("AI_HANDOFF_HOME");
        let result = run_with(
            HostArgs {
                home: home.clone(),
                stay_alive: true,
            },
            |stay_alive| {
                assert!(stay_alive);
                assert_eq!(
                    std::env::var_os("AI_HANDOFF_HOME"),
                    Some(home.into_os_string())
                );
                17
            },
        )
        .unwrap();

        if let Some(previous) = previous {
            std::env::set_var("AI_HANDOFF_HOME", previous);
        } else {
            std::env::remove_var("AI_HANDOFF_HOME");
        }
        assert_eq!(result, 17);
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires AI_HANDOFF_LIVE_EXE pointing to a built ai-handoff-host.exe"]
    fn live_shutdown_event_stops_host_and_releases_its_executable() {
        use std::process::Stdio;
        use std::time::{Duration, Instant};

        let _guard = ENV_LOCK.lock().unwrap();
        let source = PathBuf::from(
            std::env::var_os("AI_HANDOFF_LIVE_EXE")
                .expect("set AI_HANDOFF_LIVE_EXE to ai-handoff-host.exe"),
        );
        let temp = tempfile::tempdir().unwrap();
        let host = temp.path().join("ai-handoff-host.exe");
        let home = temp.path().join("home");
        std::fs::copy(source, &host).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(
            home.join("config.toml"),
            "[daemon]\nidle_timeout_seconds = 60\n",
        )
        .unwrap();

        let previous = std::env::var_os("AI_HANDOFF_HOME");
        std::env::set_var("AI_HANDOFF_HOME", &home);
        let mut child = std::process::Command::new(&host)
            .arg("--home")
            .arg(&home)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();

        let ready_deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < ready_deadline
            && !crate::daemon_supply::ping_daemon(Duration::from_millis(250))
        {
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(
            crate::daemon_supply::ping_daemon(Duration::from_millis(250)),
            "native host did not become healthy"
        );
        assert!(signal_shutdown(&home).unwrap());

        let exit_deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < exit_deadline && child.try_wait().unwrap().is_none() {
            std::thread::sleep(Duration::from_millis(25));
        }
        if child.try_wait().unwrap().is_none() {
            let _ = child.kill();
            panic!("native host ignored the shutdown event");
        }
        std::fs::remove_file(&host).expect("exited native host executable must be unlocked");

        if let Some(previous) = previous {
            std::env::set_var("AI_HANDOFF_HOME", previous);
        } else {
            std::env::remove_var("AI_HANDOFF_HOME");
        }
    }
}
