#[test]
fn private_directory_and_file_are_hardened() {
    let dir = tempfile::tempdir().unwrap();
    let private_dir = dir.path().join("runtime").join("ipc");
    ai_handoff_core::secure_fs::ensure_private_dir(&private_dir).unwrap();

    let dir_report = ai_handoff_core::secure_fs::private_dir_status(&private_dir);
    #[cfg(not(windows))]
    assert_eq!(
        dir_report.status,
        ai_handoff_core::secure_fs::PermissionStatus::Ok,
        "{dir_report:?}"
    );
    #[cfg(windows)]
    assert!(
        matches!(
            dir_report.status,
            ai_handoff_core::secure_fs::PermissionStatus::Ok
                | ai_handoff_core::secure_fs::PermissionStatus::Warning
        ),
        "{dir_report:?}"
    );

    let file = private_dir.join("request.json");
    ai_handoff_core::secure_fs::write_private_atomic(
        &file,
        &file.with_extension("json.tmp"),
        b"{}",
    )
    .unwrap();
    let file_report = ai_handoff_core::secure_fs::private_file_status(&file);
    #[cfg(not(windows))]
    assert_eq!(
        file_report.status,
        ai_handoff_core::secure_fs::PermissionStatus::Ok,
        "{file_report:?}"
    );
    #[cfg(windows)]
    assert!(
        matches!(
            file_report.status,
            ai_handoff_core::secure_fs::PermissionStatus::Ok
                | ai_handoff_core::secure_fs::PermissionStatus::Warning
        ),
        "{file_report:?}"
    );
}

#[cfg(unix)]
#[test]
fn unix_private_modes_do_not_allow_group_or_other_access() {
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    let dir = tempfile::tempdir().unwrap();
    let private_dir = dir.path().join("runtime").join("ipc");
    ai_handoff_core::secure_fs::ensure_private_dir(&private_dir).unwrap();
    let file = private_dir.join("request.json");
    ai_handoff_core::secure_fs::write_private_atomic(
        &file,
        &file.with_extension("json.tmp"),
        b"{}",
    )
    .unwrap();

    assert_eq!(mode(&private_dir) & 0o077, 0);
    assert_eq!(mode(&file) & 0o077, 0);

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode()
    }
}

#[cfg(windows)]
#[test]
fn windows_private_acl_is_checked_after_best_effort_hardening() {
    let dir = tempfile::tempdir().unwrap();
    let private_dir = dir.path().join("runtime").join("ipc");
    ai_handoff_core::secure_fs::ensure_private_dir(&private_dir).unwrap();
    let file = private_dir.join("request.json");
    ai_handoff_core::secure_fs::write_private_atomic(
        &file,
        &file.with_extension("json.tmp"),
        b"{}",
    )
    .unwrap();

    let dir_report = ai_handoff_core::secure_fs::private_dir_status(&private_dir);
    let file_report = ai_handoff_core::secure_fs::private_file_status(&file);
    assert!(
        !matches!(
            dir_report.status,
            ai_handoff_core::secure_fs::PermissionStatus::Missing
                | ai_handoff_core::secure_fs::PermissionStatus::Error
        ),
        "{dir_report:?}"
    );
    assert!(
        !matches!(
            file_report.status,
            ai_handoff_core::secure_fs::PermissionStatus::Missing
                | ai_handoff_core::secure_fs::PermissionStatus::Error
        ),
        "{file_report:?}"
    );
}
