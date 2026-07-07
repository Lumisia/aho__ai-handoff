use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionStatus {
    Ok,
    Warning,
    Error,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionReport {
    pub status: PermissionStatus,
    pub message: String,
}

pub fn ensure_private_dir(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    harden_dir(path)
}

/// Create a subdirectory that INHERITS its parent's ACL instead of being
/// hardened with an explicit private ACL.
///
/// This is the correct shape for the IPC `requests`/`responses`/`dead_letter`
/// dirs: the IPC root is hardened private, and agent sandboxes (Codex on
/// Windows) grant their restricted token an ACE on that root because it is a
/// configured writable root. Hardening the subdirs with `/inheritance:r` (the
/// old behavior) stripped that inherited sandbox ACE, so sandboxed hooks could
/// not write requests or read responses — the daemon looked dead even while
/// running. On Windows this also REPAIRS dirs broken by older versions by
/// re-enabling inheritance.
pub fn ensure_inherited_subdir(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(windows)]
    {
        // Best effort, mirroring harden_dir: inside an agent sandbox the
        // restricted token may lack WRITE_DAC even on dirs it just created,
        // and a hard error here would fail the hook that only needed the dir
        // to exist. The real repair is guaranteed by the unsandboxed daemon
        // startup and `ai-handoff install` paths, where icacls succeeds.
        let _ = no_window_command("icacls")
            .arg(path)
            .arg("/inheritance:e")
            .output();
    }
    #[cfg(unix)]
    {
        // On unix the private parent (0700) already gates access by path;
        // keep the subdir equally private. Mac/Linux sandboxes grant by path
        // rules, not ACL inheritance, so this does not lock agents out.
        harden_dir(path)?;
    }
    Ok(())
}

/// Atomically write a file that INHERITS the directory ACL (no per-file
/// hardening). Used for IPC request/response payloads: a response hardened
/// with an explicit private ACL cannot be READ back by a sandboxed hook
/// (restricted tokens need their own ACE), which turned every hook call into
/// `daemon_unavailable` even when the daemon answered.
pub fn write_shared_atomic(path: &Path, tmp: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.is_dir() {
            ensure_inherited_subdir(parent)?;
        }
    }
    write_file_with_private_mode(tmp, bytes)?;
    match std::fs::rename(tmp, path) {
        Ok(()) => Ok(()),
        Err(error) if path.exists() => {
            std::fs::remove_file(path)?;
            std::fs::rename(tmp, path).map_err(|second_error| {
                let _ = std::fs::remove_file(tmp);
                if second_error.kind() == std::io::ErrorKind::Other {
                    error
                } else {
                    second_error
                }
            })
        }
        Err(error) => {
            let _ = std::fs::remove_file(tmp);
            Err(error)
        }
    }
}

pub fn write_private_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        ensure_private_dir(parent)?;
    }
    write_file_with_private_mode(path, bytes)?;
    harden_file(path)
}

pub fn write_private_atomic(path: &Path, tmp: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        ensure_private_dir(parent)?;
    }
    write_file_with_private_mode(tmp, bytes)?;
    harden_file(tmp)?;
    match std::fs::rename(tmp, path) {
        Ok(()) => {}
        Err(error) if path.exists() => {
            std::fs::remove_file(path)?;
            std::fs::rename(tmp, path).map_err(|second_error| {
                let _ = std::fs::remove_file(tmp);
                if second_error.kind() == std::io::ErrorKind::Other {
                    error
                } else {
                    second_error
                }
            })?;
        }
        Err(error) => {
            let _ = std::fs::remove_file(tmp);
            return Err(error);
        }
    }
    harden_file(path)
}

/// Atomically write a PRIVATE file without re-ACLing its parent directory.
///
/// For live agent credential files (`~/.claude/.credentials.json`,
/// `~/.codex/auth.json`): the file itself must stay private (Claude Code
/// creates it 0600), but the parent is the AGENT'S home — hardening it with an
/// explicit private ACL could strip a sandbox ACE the agent depends on, the
/// same failure mode `ensure_inherited_subdir` exists to repair for the IPC
/// dirs. So the parent is only created when missing, never re-ACLed.
pub fn write_private_atomic_file(path: &Path, tmp: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.is_dir() {
            std::fs::create_dir_all(parent)?;
        }
    }
    write_file_with_private_mode(tmp, bytes)?;
    harden_file(tmp)?;
    match std::fs::rename(tmp, path) {
        Ok(()) => {}
        Err(error) if path.exists() => {
            std::fs::remove_file(path)?;
            std::fs::rename(tmp, path).map_err(|second_error| {
                let _ = std::fs::remove_file(tmp);
                if second_error.kind() == std::io::ErrorKind::Other {
                    error
                } else {
                    second_error
                }
            })?;
        }
        Err(error) => {
            let _ = std::fs::remove_file(tmp);
            return Err(error);
        }
    }
    harden_file(path)
}

pub fn touch_private_file(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        ensure_private_dir(parent)?;
    }
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    harden_file(path)
}

pub fn harden_private_file(path: &Path) -> std::io::Result<()> {
    harden_file(path)
}

pub fn private_dir_status(path: &Path) -> PermissionReport {
    private_status(path, true)
}

pub fn private_file_status(path: &Path) -> PermissionReport {
    private_status(path, false)
}

/// Status of an IPC subdirectory that must INHERIT its parent's ACL (see
/// [`ensure_inherited_subdir`]). On Windows, "no inherited ACEs" is the broken
/// state older versions left behind: sandboxed agents cannot write there and
/// hooks/skills silently degrade to `daemon_unavailable`.
pub fn inherited_subdir_status(path: &Path) -> PermissionReport {
    let meta = match std::fs::metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return PermissionReport {
                status: PermissionStatus::Missing,
                message: "missing".into(),
            };
        }
        Err(error) => {
            return PermissionReport {
                status: PermissionStatus::Error,
                message: error.to_string(),
            };
        }
    };
    if !meta.is_dir() {
        return PermissionReport {
            status: PermissionStatus::Error,
            message: "path exists but is not a directory".into(),
        };
    }
    platform_inherited_subdir_status(path)
}

#[cfg(windows)]
fn platform_inherited_subdir_status(path: &Path) -> PermissionReport {
    let output = match no_window_command("icacls").arg(path).output() {
        Ok(output) => output,
        Err(error) => {
            return PermissionReport {
                status: PermissionStatus::Error,
                message: error.to_string(),
            };
        }
    };
    if !output.status.success() {
        return PermissionReport {
            status: PermissionStatus::Error,
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        };
    }
    let text = String::from_utf8_lossy(&output.stdout);
    if text.contains("(I)") {
        PermissionReport {
            status: PermissionStatus::Ok,
            message: "inherits the IPC root ACL".into(),
        }
    } else {
        PermissionReport {
            status: PermissionStatus::Warning,
            message: "inheritance disabled — sandboxed agents cannot use IPC here; \
                      run `ai-handoff install --yes` or restart the daemon to repair"
                .into(),
        }
    }
}

#[cfg(not(windows))]
fn platform_inherited_subdir_status(path: &Path) -> PermissionReport {
    platform_private_status(path, true)
}

fn private_status(path: &Path, is_dir: bool) -> PermissionReport {
    let meta = match std::fs::metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return PermissionReport {
                status: PermissionStatus::Missing,
                message: "missing".into(),
            };
        }
        Err(error) => {
            return PermissionReport {
                status: PermissionStatus::Error,
                message: error.to_string(),
            };
        }
    };
    if meta.is_dir() != is_dir {
        return PermissionReport {
            status: PermissionStatus::Error,
            message: if is_dir {
                "path exists but is not a directory"
            } else {
                "path exists but is not a file"
            }
            .into(),
        };
    }
    platform_private_status(path, is_dir)
}

#[cfg(unix)]
fn platform_private_status(path: &Path, is_dir: bool) -> PermissionReport {
    use std::os::unix::fs::PermissionsExt;

    let mode = match std::fs::metadata(path) {
        Ok(meta) => meta.permissions().mode() & 0o777,
        Err(error) => {
            return PermissionReport {
                status: PermissionStatus::Error,
                message: error.to_string(),
            };
        }
    };
    if mode & 0o077 == 0 {
        PermissionReport {
            status: PermissionStatus::Ok,
            message: format!("mode {mode:o} is private"),
        }
    } else {
        PermissionReport {
            status: PermissionStatus::Warning,
            message: format!(
                "{} mode {mode:o} allows group/other access",
                if is_dir { "directory" } else { "file" }
            ),
        }
    }
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[cfg(windows)]
fn no_window_command(program: &str) -> std::process::Command {
    use std::os::windows::process::CommandExt;

    let mut command = std::process::Command::new(program);
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

#[cfg(windows)]
fn platform_private_status(path: &Path, _is_dir: bool) -> PermissionReport {
    let output = match no_window_command("icacls").arg(path).output() {
        Ok(output) => output,
        Err(error) => {
            return PermissionReport {
                status: PermissionStatus::Error,
                message: error.to_string(),
            };
        }
    };
    if !output.status.success() {
        return PermissionReport {
            status: PermissionStatus::Error,
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        };
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let lower = text.to_ascii_lowercase();
    let broad = [
        "everyone",
        "authenticated users",
        "builtin\\users",
        "s-1-1-0",
        "s-1-5-11",
        "s-1-5-32-545",
        "(i)",
    ];
    if let Some(token) = broad.iter().find(|token| lower.contains(**token)) {
        PermissionReport {
            status: PermissionStatus::Warning,
            message: format!("broad or inherited ACL entry present: {token}"),
        }
    } else {
        PermissionReport {
            status: PermissionStatus::Ok,
            message: "ACL is private".into(),
        }
    }
}

#[cfg(not(any(unix, windows)))]
fn platform_private_status(_path: &Path, _is_dir: bool) -> PermissionReport {
    PermissionReport {
        status: PermissionStatus::Warning,
        message: "permission checks are not implemented for this platform".into(),
    }
}

#[cfg(unix)]
fn harden_dir(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}

#[cfg(unix)]
fn harden_file(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(unix)]
fn write_file_with_private_mode(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)
}

#[cfg(windows)]
fn harden_dir(path: &Path) -> std::io::Result<()> {
    let _ = apply_windows_acl(path, true);
    Ok(())
}

#[cfg(windows)]
fn harden_file(path: &Path) -> std::io::Result<()> {
    let _ = apply_windows_acl(path, false);
    Ok(())
}

#[cfg(windows)]
fn write_file_with_private_mode(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, bytes)
}

#[cfg(windows)]
fn apply_windows_acl(path: &Path, is_dir: bool) -> std::io::Result<()> {
    let user_sid = current_user_sid()?;
    let suffix = if is_dir { ":(OI)(CI)F" } else { ":F" };
    let grants = [
        format!("*{user_sid}{suffix}"),
        format!("*S-1-5-18{suffix}"),
        format!("*S-1-5-32-544{suffix}"),
    ];
    let output = no_window_command("icacls")
        .arg(path)
        .arg("/inheritance:r")
        .arg("/grant:r")
        .arg(&grants[0])
        .arg("/grant:r")
        .arg(&grants[1])
        .arg("/grant:r")
        .arg(&grants[2])
        .arg("/remove:g")
        .arg("*S-1-1-0")
        .arg("*S-1-5-11")
        .arg("*S-1-5-32-545")
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ))
    }
}

#[cfg(windows)]
fn current_user_sid() -> std::io::Result<String> {
    let output = no_window_command("whoami")
        .args(["/user", "/fo", "csv", "/nh"])
        .output()?;
    if !output.status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let fields = parse_csv_line(text.trim());
    fields
        .last()
        .filter(|sid| sid.starts_with("S-1-"))
        .cloned()
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("could not parse current user SID from whoami output: {text}"),
            )
        })
}

#[cfg(windows)]
fn parse_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' if in_quotes && chars.peek() == Some(&'"') => {
                field.push('"');
                let _ = chars.next();
            }
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                fields.push(field.trim().to_string());
                field.clear();
            }
            _ => field.push(ch),
        }
    }
    fields.push(field.trim().to_string());
    fields
}

#[cfg(not(any(unix, windows)))]
fn harden_dir(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn harden_file(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn write_file_with_private_mode(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, bytes)
}
