use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use ::windows::core::{Interface, BSTR};
use ::windows::Win32::Foundation::{RPC_E_CHANGED_MODE, VARIANT_FALSE, VARIANT_TRUE};
use ::windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use ::windows::Win32::System::TaskScheduler::{
    IExecAction, ITaskFolder, ITaskService, TaskScheduler, TASK_ACTION_EXEC, TASK_CREATE_OR_UPDATE,
    TASK_INSTANCES_IGNORE_NEW, TASK_LOGON_INTERACTIVE_TOKEN, TASK_RUNLEVEL_LUA,
};
use ::windows::Win32::System::Variant::VARIANT;
use anyhow::Context;

use super::{WINDOWS_FOLDER, WINDOWS_TASK};

const TASK_SECURITY_SDDL: &str = "D:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FA;;;OW)(A;;FRFX;;;WD)";
const NO_EXECUTION_TIME_LIMIT: &str = "PT0S";

#[derive(Clone, Debug, PartialEq, Eq)]
struct TaskIdentity {
    folder: String,
    task: String,
}

impl TaskIdentity {
    fn new(folder: &str, task: &str) -> anyhow::Result<Self> {
        anyhow::ensure!(!folder.trim().is_empty(), "task folder cannot be empty");
        anyhow::ensure!(!task.trim().is_empty(), "task name cannot be empty");
        anyhow::ensure!(
            !folder.contains(['\\', '/']),
            "task folder must be a single path component"
        );
        anyhow::ensure!(
            !task.contains(['\\', '/']),
            "task name must be a single path component"
        );
        Ok(Self {
            folder: folder.to_string(),
            task: task.to_string(),
        })
    }

    fn production() -> Self {
        Self::new(WINDOWS_FOLDER, WINDOWS_TASK).expect("production task identity is valid")
    }

    #[cfg(test)]
    fn full_id(&self) -> String {
        format!(r"\{}\{}", self.folder, self.task)
    }

    fn folder_path(&self) -> String {
        format!(r"\{}", self.folder)
    }
}

struct ComApartment {
    uninitialize: bool,
}

impl ComApartment {
    fn initialize() -> anyhow::Result<Self> {
        let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if result.is_ok() {
            return Ok(Self { uninitialize: true });
        }
        if result == RPC_E_CHANGED_MODE {
            return Ok(Self {
                uninitialize: false,
            });
        }
        Err(::windows::core::Error::from_hresult(result))
            .context("initialize COM for Task Scheduler")
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.uninitialize {
            unsafe { CoUninitialize() };
        }
    }
}

fn path_bstr(path: &Path) -> BSTR {
    BSTR::from_wide(&path.as_os_str().encode_wide().collect::<Vec<_>>())
}

fn action_arguments_bstr(home: &Path) -> BSTR {
    let mut wide: Vec<u16> = "--home \"".encode_utf16().collect();
    wide.extend(home.as_os_str().encode_wide());
    wide.push('"' as u16);
    BSTR::from_wide(&wide)
}

fn connect_service() -> anyhow::Result<ITaskService> {
    let service: ITaskService = unsafe {
        CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER)
            .context("create Task Scheduler service")?
    };
    let empty = VARIANT::default();
    unsafe {
        service
            .Connect(&empty, &empty, &empty, &empty)
            .context("connect to Task Scheduler")?;
    }
    Ok(service)
}

fn root_folder(service: &ITaskService) -> anyhow::Result<ITaskFolder> {
    unsafe {
        service
            .GetFolder(&BSTR::from(r"\"))
            .context("open Task Scheduler root folder")
    }
}

fn get_or_create_folder(
    root: &ITaskFolder,
    identity: &TaskIdentity,
) -> anyhow::Result<ITaskFolder> {
    let name = BSTR::from(identity.folder.as_str());
    let folder = unsafe {
        match root.GetFolder(&name) {
            Ok(folder) => folder,
            Err(error) if is_missing(&error) => root
                .CreateFolder(&name, &VARIANT::default())
                .context("create AI Handoff Task Scheduler folder")?,
            Err(error) => return Err(error).context("open AI Handoff Task Scheduler folder"),
        }
    };
    unsafe {
        folder
            .SetSecurityDescriptor(&BSTR::from(TASK_SECURITY_SDDL), 0)
            .context("set AI Handoff task folder permissions")?;
    }
    Ok(folder)
}

fn open_folder(
    service: &ITaskService,
    identity: &TaskIdentity,
) -> anyhow::Result<Option<ITaskFolder>> {
    unsafe {
        match service.GetFolder(&BSTR::from(identity.folder_path())) {
            Ok(folder) => Ok(Some(folder)),
            Err(error) if is_missing(&error) => Ok(None),
            Err(error) => Err(error).context("open AI Handoff Task Scheduler folder"),
        }
    }
}

pub(super) fn install(exe: &Path, home: &Path) -> anyhow::Result<()> {
    install_at(exe, home, &TaskIdentity::production())
}

fn install_at(exe: &Path, home: &Path, identity: &TaskIdentity) -> anyhow::Result<()> {
    anyhow::ensure!(
        exe.is_absolute(),
        "host launcher executable path must be absolute"
    );
    anyhow::ensure!(home.is_absolute(), "AI Handoff home path must be absolute");

    let _apartment = ComApartment::initialize()?;
    let service = connect_service()?;
    let root = root_folder(&service)?;
    let folder = get_or_create_folder(&root, identity)?;
    let definition = unsafe { service.NewTask(0).context("create task definition")? };

    unsafe {
        definition
            .RegistrationInfo()
            .context("open task registration info")?
            .SetAuthor(&BSTR::from("AI Handoff"))
            .context("set task author")?;

        let settings = definition.Settings().context("open task settings")?;
        settings
            .SetAllowDemandStart(VARIANT_TRUE)
            .context("allow on-demand task start")?;
        settings
            .SetEnabled(VARIANT_TRUE)
            .context("enable host launcher task")?;
        settings
            .SetMultipleInstances(TASK_INSTANCES_IGNORE_NEW)
            .context("set single daemon task instance policy")?;
        settings
            .SetExecutionTimeLimit(&BSTR::from(NO_EXECUTION_TIME_LIMIT))
            .context("remove Task Scheduler execution time limit")?;
        settings
            .SetDisallowStartIfOnBatteries(VARIANT_FALSE)
            .context("allow daemon launch on battery")?;
        settings
            .SetStopIfGoingOnBatteries(VARIANT_FALSE)
            .context("keep daemon running on battery")?;

        let principal = definition.Principal().context("open task principal")?;
        principal
            .SetLogonType(TASK_LOGON_INTERACTIVE_TOKEN)
            .context("set interactive-token logon")?;
        principal
            .SetRunLevel(TASK_RUNLEVEL_LUA)
            .context("set least-privilege task run level")?;

        let action: IExecAction = definition
            .Actions()
            .context("open task actions")?
            .Create(TASK_ACTION_EXEC)
            .context("create executable task action")?
            .cast()
            .context("cast task action to executable action")?;
        action
            .SetPath(&path_bstr(exe))
            .context("set host launcher executable")?;
        action
            .SetArguments(&action_arguments_bstr(home))
            .context("set host launcher arguments")?;
        action
            .SetWorkingDirectory(&path_bstr(home))
            .context("set host launcher working directory")?;

        let empty = VARIANT::default();
        let task = folder
            .RegisterTaskDefinition(
                &BSTR::from(identity.task.as_str()),
                &definition,
                TASK_CREATE_OR_UPDATE.0,
                &empty,
                &empty,
                TASK_LOGON_INTERACTIVE_TOKEN,
                &empty,
            )
            .context("register AI Handoff daemon task")?;
        task.SetSecurityDescriptor(&BSTR::from(TASK_SECURITY_SDDL), 0)
            .context("set AI Handoff daemon task permissions")?;
    }
    Ok(())
}

pub(super) fn launch() -> anyhow::Result<()> {
    launch_at(&TaskIdentity::production())
}

fn launch_at(identity: &TaskIdentity) -> anyhow::Result<()> {
    let _apartment = ComApartment::initialize()?;
    let service = connect_service()?;
    let folder = open_folder(&service, identity)?
        .ok_or_else(|| anyhow::anyhow!("AI Handoff host launcher folder is missing"))?;
    let task = unsafe {
        folder
            .GetTask(&BSTR::from(identity.task.as_str()))
            .context("open AI Handoff daemon task")?
    };
    unsafe {
        task.Run(&VARIANT::default())
            .context("run AI Handoff daemon task")?;
    }
    Ok(())
}

pub(super) fn inspect() -> anyhow::Result<Option<String>> {
    inspect_at(&TaskIdentity::production())
}

fn inspect_at(identity: &TaskIdentity) -> anyhow::Result<Option<String>> {
    let _apartment = ComApartment::initialize()?;
    let service = connect_service()?;
    let Some(folder) = open_folder(&service, identity)? else {
        return Ok(None);
    };
    let task = unsafe {
        match folder.GetTask(&BSTR::from(identity.task.as_str())) {
            Ok(task) => task,
            Err(error) if is_missing(&error) => return Ok(None),
            Err(error) => return Err(error).context("inspect AI Handoff daemon task"),
        }
    };
    let xml = unsafe {
        task.Xml()
            .context("read AI Handoff daemon task definition")?
    };
    Ok(Some(String::from_utf16_lossy(&xml)))
}

pub(super) fn remove() -> anyhow::Result<()> {
    remove_at(&TaskIdentity::production())
}

fn remove_at(identity: &TaskIdentity) -> anyhow::Result<()> {
    let _apartment = ComApartment::initialize()?;
    let service = connect_service()?;
    let Some(folder) = open_folder(&service, identity)? else {
        return Ok(());
    };

    unsafe {
        match folder.GetTask(&BSTR::from(identity.task.as_str())) {
            Ok(_) => folder
                .DeleteTask(&BSTR::from(identity.task.as_str()), 0)
                .context("remove AI Handoff daemon task")?,
            Err(error) if is_missing(&error) => {}
            Err(error) => return Err(error).context("inspect AI Handoff daemon task"),
        }

        let task_count = folder
            .GetTasks(0)
            .context("list remaining AI Handoff tasks")?
            .Count()
            .context("count remaining AI Handoff tasks")?;
        let folder_count = folder
            .GetFolders(0)
            .context("list remaining AI Handoff task folders")?
            .Count()
            .context("count remaining AI Handoff task folders")?;
        if task_count == 0 && folder_count == 0 {
            let root = root_folder(&service)?;
            root.DeleteFolder(&BSTR::from(identity.folder.as_str()), 0)
                .context("remove empty AI Handoff Task Scheduler folder")?;
        }
    }
    Ok(())
}

fn is_missing(error: &::windows::core::Error) -> bool {
    matches!(
        error.code().0 as u32,
        0x8007_0002 | 0x8007_0003 | 0x8004_130f
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::windows::core::HRESULT;

    #[test]
    fn task_policy_is_demand_only_and_has_no_scheduler_time_limit() {
        assert_eq!(NO_EXECUTION_TIME_LIMIT, "PT0S");
        assert!(TASK_SECURITY_SDDL.contains(";;;WD)"));
        assert!(TASK_SECURITY_SDDL.contains("FRFX"));
    }

    #[test]
    fn recognizes_missing_folder_and_task_errors_only() {
        for code in [0x8007_0002_u32, 0x8007_0003, 0x8004_130f] {
            assert!(is_missing(&::windows::core::Error::from_hresult(HRESULT(
                code as i32
            ))));
        }
        assert!(!is_missing(&::windows::core::Error::from_hresult(HRESULT(
            0x8007_0005_u32 as i32
        ))));
    }

    #[test]
    fn validation_task_identity_is_distinct_from_production() {
        let identity = TaskIdentity::new("AIHandoffValidation-123", "Daemon-123").unwrap();

        assert_eq!(identity.folder, "AIHandoffValidation-123");
        assert_eq!(identity.task, "Daemon-123");
        assert_eq!(identity.full_id(), r"\AIHandoffValidation-123\Daemon-123");
        assert_ne!(identity.full_id(), super::super::WINDOWS_TASK_ID);
    }

    #[test]
    #[ignore = "requires an interactive Windows Task Scheduler session and AI_HANDOFF_LIVE_EXE"]
    fn live_validation_task_launches_healthy_daemon_and_is_removed() {
        struct Cleanup {
            identity: TaskIdentity,
            previous_home: Option<std::ffi::OsString>,
        }

        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = remove_at(&self.identity);
                if let Some(previous_home) = &self.previous_home {
                    std::env::set_var("AI_HANDOFF_HOME", previous_home);
                } else {
                    std::env::remove_var("AI_HANDOFF_HOME");
                }
            }
        }

        let exe = std::path::PathBuf::from(
            std::env::var_os("AI_HANDOFF_LIVE_EXE")
                .expect("set AI_HANDOFF_LIVE_EXE to the built ai-handoff executable"),
        );
        assert!(exe.is_absolute(), "live executable path must be absolute");
        assert!(
            exe.is_file(),
            "live executable does not exist: {}",
            exe.display()
        );

        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let identity = TaskIdentity::new(
            &format!("AIHandoffValidation-{}", &suffix[..8]),
            &format!("Daemon-{}", &suffix[..8]),
        )
        .unwrap();
        assert_ne!(identity.full_id(), super::super::WINDOWS_TASK_ID);

        let home = tempfile::tempdir().unwrap();
        std::fs::write(
            home.path().join("config.toml"),
            "[daemon]\nidle_timeout_seconds = 1\n",
        )
        .unwrap();
        let previous_home = std::env::var_os("AI_HANDOFF_HOME");
        std::env::set_var("AI_HANDOFF_HOME", home.path());
        let cleanup = Cleanup {
            identity: identity.clone(),
            previous_home,
        };

        remove_at(&identity).unwrap();
        install_at(&exe, home.path(), &identity).unwrap();
        let xml = inspect_at(&identity)
            .unwrap()
            .expect("validation task must be registered");
        assert!(super::super::action_xml_matches(&xml, &exe, home.path()));
        assert!(xml.contains(&home.path().to_string_lossy().to_string()));

        launch_at(&identity).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut healthy = false;
        while std::time::Instant::now() < deadline {
            if crate::daemon_supply::ping_daemon(std::time::Duration::from_millis(250)) {
                healthy = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(healthy, "validation task did not launch a healthy daemon");

        remove_at(&identity).unwrap();
        assert!(inspect_at(&identity).unwrap().is_none());
        std::thread::sleep(std::time::Duration::from_millis(1_500));
        drop(cleanup);
    }

    #[test]
    #[ignore = "orchestrated install/launch/verify/remove Codex sandbox boundary test"]
    fn live_cross_boundary_validation_phase() {
        let phase = std::env::var("AI_HANDOFF_LIVE_PHASE")
            .expect("set AI_HANDOFF_LIVE_PHASE to install, launch, verify, or remove");
        let exe = std::path::PathBuf::from(
            std::env::var_os("AI_HANDOFF_LIVE_EXE")
                .expect("set AI_HANDOFF_LIVE_EXE to the built ai-handoff executable"),
        );
        let home = std::path::PathBuf::from(
            std::env::var_os("AI_HANDOFF_LIVE_HOME")
                .expect("set AI_HANDOFF_LIVE_HOME to an isolated absolute directory"),
        );
        let identity = TaskIdentity::new(
            &std::env::var("AI_HANDOFF_LIVE_FOLDER").expect("set AI_HANDOFF_LIVE_FOLDER"),
            &std::env::var("AI_HANDOFF_LIVE_TASK").expect("set AI_HANDOFF_LIVE_TASK"),
        )
        .unwrap();
        assert!(exe.is_absolute() && exe.is_file());
        assert!(home.is_absolute());
        assert_ne!(identity.full_id(), super::super::WINDOWS_TASK_ID);
        std::env::set_var("AI_HANDOFF_HOME", &home);

        match phase.as_str() {
            "install" => {
                std::fs::create_dir_all(&home).unwrap();
                std::fs::write(
                    home.join("config.toml"),
                    "[daemon]\nidle_timeout_seconds = 60\n",
                )
                .unwrap();
                remove_at(&identity).unwrap();
                install_at(&exe, &home, &identity).unwrap();
                let xml = inspect_at(&identity)
                    .unwrap()
                    .expect("validation task must be registered");
                assert!(super::super::action_xml_matches(&xml, &exe, &home));
            }
            "launch" => {
                launch_at(&identity).unwrap();
            }
            "verify" => {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                while std::time::Instant::now() < deadline {
                    if crate::daemon_supply::ping_daemon(std::time::Duration::from_millis(250)) {
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                panic!("registered validation task did not launch a healthy daemon");
            }
            "remove" => {
                remove_at(&identity).unwrap();
                assert!(inspect_at(&identity).unwrap().is_none());
            }
            other => panic!("unknown AI_HANDOFF_LIVE_PHASE: {other}"),
        }
    }
}
