//! Project fingerprint — a stable per-project identifier shared by every agent
//! (Claude, Codex, ...) working in the same checkout. The fingerprint selects
//! the capsule bucket, so it MUST stay byte-for-byte compatible with the v1
//! canonical hash algorithm: if two agents in the same repo compute different
//! fingerprints they land in different buckets and handoff silently fails.
//!
//! Basis priority: `remote.origin.url` -> gitroot -> path. The chosen basis
//! string is prefixed `remote:` / `gitroot:` / `path:`, and the fingerprint is
//! the first 24 lowercase-hex chars of `sha256(value)`.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Which signal produced the fingerprint basis.
#[derive(Debug, PartialEq, Eq)]
pub enum BasisType {
    Remote,
    Gitroot,
    Path,
}

/// The basis value used to derive the fingerprint. `value` is always prefixed
/// with `remote:` / `gitroot:` / `path:` exactly as v1 hashes it.
#[derive(Debug)]
pub struct Basis {
    pub kind: BasisType,
    pub value: String,
}

/// Fingerprint plus the basis it was derived from.
#[derive(Debug)]
pub struct FingerprintInfo {
    pub fingerprint: String,
    pub basis: Basis,
}

/// Outcome of running a git command.
///
/// `Failed { blocked }` mirrors v1's `{ ok:false, blocked }`: `blocked` is true
/// when the git binary could not run at all (EPERM/ENOENT/EACCES — sandbox block
/// or missing binary) and false when git ran but exited non-zero (e.g. not a
/// repo). Only `blocked == true` justifies the filesystem fallback.
pub enum GitResult {
    Ok(String),
    Failed { blocked: bool },
}

/// Abstraction over invoking git, so tests can inject canned behavior.
pub trait GitRunner {
    fn run(&self, cwd: &Path, args: &[&str]) -> GitResult;
}

/// The shared (common dir) git config carries remotes; resolved `.git` info.
struct GitDirInfo {
    root: PathBuf,
    git_dir: PathBuf,
    common_dir: PathBuf,
}

fn sha24(value: &str) -> String {
    let mut h = Sha256::new();
    h.update(value.as_bytes());
    let digest = h.finalize();
    let mut hex = String::with_capacity(64);
    for b in digest.iter() {
        use std::fmt::Write;
        let _ = write!(hex, "{b:02x}");
    }
    hex[..24].to_string()
}

/// `^[a-zA-Z][a-zA-Z0-9+.-]*://`
fn is_scheme_url(u: &str) -> bool {
    let b = u.as_bytes();
    let mut i = 0;
    if i >= b.len() || !b[i].is_ascii_alphabetic() {
        return false;
    }
    i += 1;
    while i < b.len() && (b[i].is_ascii_alphanumeric() || matches!(b[i], b'+' | b'.' | b'-')) {
        i += 1;
    }
    u[i..].starts_with("://")
}

/// `^[A-Za-z]:[\\/]`
fn is_windows_drive(u: &str) -> bool {
    let b = u.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')
}

/// git scp syntax `[user@]host:path` — a colon before any slash, and not a
/// scheme URL or a Windows drive path. The user part is OPTIONAL.
fn is_scp_like(u: &str) -> bool {
    if is_scheme_url(u) || is_windows_drive(u) {
        return false;
    }
    // ^[^/:]+:  (a colon, with no slash before it)
    match u.find(':') {
        Some(c) => c > 0 && !u[..c].contains('/'),
        None => false,
    }
}

/// Strip credentials: userinfo for scheme URLs, then query/fragment for scheme
/// URLs. scp-style and Windows-drive paths are left untouched.
///
/// The userinfo class is `[^/?#]*@` (everything up to the authority terminator)
/// matched against the LAST `@` before the authority ends, so a password that
/// itself contains `@` is fully removed and a `@` inside a query/fragment is not
/// mistaken for userinfo.
fn sanitize_remote_url(url: &str) -> String {
    let mut out = url.to_string();
    if is_scheme_url(&out) {
        if let Some(scheme_end) = out.find("://") {
            let after = scheme_end + 3;
            let authority_end = out[after..]
                .find(['/', '?', '#'])
                .map(|p| after + p)
                .unwrap_or(out.len());
            if let Some(at_rel) = out[after..authority_end].rfind('@') {
                let at = after + at_rel;
                out.replace_range(after..=at, "");
            }
        }
        // strip query/fragment
        if let Some(q) = out.find(['?', '#']) {
            out.truncate(q);
        }
    }
    out
}

/// Lexically resolve `cleaned` against `root` the way Node `path.resolve` does,
/// using the PLATFORM-DEFAULT resolver — `path.win32.resolve` on Windows,
/// `path.posix.resolve` on Unix — to stay byte-for-byte identical to v1, which
/// calls Node's `resolve` (and thus inherits the OS-specific variant). The split
/// matters: a relative remote like `../upstream.git` against `C:/repo/root`
/// resolves to `C:\repo\upstream.git` on Windows but `/repo/upstream.git` on
/// Unix; emitting the POSIX form on Windows would split capsule buckets.
///
/// Resolution is LEXICAL only — segments are collapsed without touching the
/// filesystem, because the remote target may not exist locally and
/// canonicalizing would make the fingerprint depend on filesystem state.
///
/// Callers normalize `root`/`cleaned` to forward slashes before calling, but
/// the Windows variant accepts both `/` and `\` as separators (as `path.win32`
/// does) for safety.
fn lexical_resolve(root: &str, cleaned: &str) -> String {
    #[cfg(windows)]
    {
        lexical_resolve_win32(root, cleaned)
    }
    #[cfg(not(windows))]
    {
        lexical_resolve_posix(root, cleaned)
    }
}

/// `path.posix.resolve(root, cleaned)` for the cases v1 reaches: `cleaned` is
/// always relative, so the combined path joins with `/`, collapses `.`/`..`, and
/// keeps the POSIX `/` separator.
#[cfg_attr(windows, allow(dead_code))]
fn lexical_resolve_posix(root: &str, cleaned: &str) -> String {
    let combined = format!("{root}/{cleaned}");
    let is_absolute = combined.starts_with('/');
    let joined = collapse_segments(&combined, is_absolute).join("/");
    if is_absolute {
        format!("/{joined}")
    } else if joined.is_empty() {
        ".".to_string()
    } else {
        joined
    }
}

/// `path.win32.resolve(root, cleaned)` for the cases v1 reaches. Treats both `/`
/// and `\` as separators, collapses `.`/`..` lexically, and emits a `\`-joined
/// path with a preserved (or, for a drive-less rooted path, cwd-derived) drive —
/// matching Node's win32 resolver exactly. `cleaned` is always relative here.
#[cfg(windows)]
fn lexical_resolve_win32(root: &str, cleaned: &str) -> String {
    let to_slash = |s: &str| s.replace('\\', "/");
    let root_s = to_slash(root);
    let cleaned_s = to_slash(cleaned);

    // Split the root's drive (`C:`) from its path. A root from
    // `git rev-parse --show-toplevel` always carries a drive on Windows; the
    // drive-less rooted case (`/repo/root`) is mirrored faithfully for
    // completeness — Node prepends the cwd's drive there.
    let (drive, root_path) = split_win32_drive(&root_s);
    let drive = drive.unwrap_or_else(cwd_drive);

    let combined = format!("{root_path}/{cleaned_s}");
    // After the drive is removed, a Windows root path is absolute iff it begins
    // with a separator (path.win32 treats `\` and `/` as roots).
    let is_absolute = combined.starts_with('/');
    let joined = collapse_segments(&combined, is_absolute).join("\\");
    if joined.is_empty() {
        format!("{drive}\\")
    } else {
        format!("{drive}\\{joined}")
    }
}

/// Collapse `.`/`..` segments of a `/`-separated path. For an absolute path,
/// `..` at the root is a no-op (matches `path.resolve`); for a relative path the
/// leading `..` segments are preserved.
fn collapse_segments(combined: &str, is_absolute: bool) -> Vec<&str> {
    let mut stack: Vec<&str> = Vec::new();
    for seg in combined.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if let Some(last) = stack.last() {
                    if *last != ".." {
                        stack.pop();
                    } else {
                        stack.push("..");
                    }
                } else if !is_absolute {
                    stack.push("..");
                }
            }
            other => stack.push(other),
        }
    }
    stack
}

/// Split a leading `X:` drive designator off a forward-slashed path. Returns
/// `(Some("C:"), "/repo/root")` for `C:/repo/root`, `(Some("C:"), "repo/root")`
/// for the drive-relative `C:repo/root`, and `(None, "/repo/root")` when there
/// is no drive.
#[cfg(windows)]
fn split_win32_drive(p: &str) -> (Option<String>, &str) {
    let b = p.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        let drive: String = p[..2].to_string();
        (Some(drive), &p[2..])
    } else {
        (None, p)
    }
}

/// The drive of the process cwd (`C:`), used only for a drive-less rooted path
/// — the same value Node's `path.win32.resolve` prepends in that case.
#[cfg(windows)]
fn cwd_drive() -> String {
    std::env::current_dir()
        .ok()
        .and_then(|p| {
            let s = p.to_string_lossy().replace('\\', "/");
            let (d, _) = split_win32_drive(&s);
            d
        })
        .unwrap_or_else(|| "C:".to_string())
}

// --- Filesystem fallback (runs ONLY when git is blocked) -------------------

/// Decode a git config value to match `git config --get`: strip an inline
/// comment from an unquoted value, and for a quoted value honour the closing
/// quote and backslash escapes (`\" \\ \n \t \b`).
fn decode_git_config_value(raw: &str) -> String {
    let mut out = String::new();
    let mut in_quote = false;
    let chars: Vec<char> = raw.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '\\' && i + 1 < chars.len() {
            i += 1;
            let n = chars[i];
            let decoded = match n {
                'n' => '\n',
                't' => '\t',
                'b' => '\u{0008}', // backspace, matches JS '\b'
                other => other,
            };
            out.push(decoded);
            i += 1;
            continue;
        }
        if ch == '"' {
            in_quote = !in_quote;
            i += 1;
            continue;
        }
        if !in_quote && (ch == '#' || ch == ';') {
            break; // inline comment
        }
        out.push(ch);
        i += 1;
    }
    out.trim().to_string()
}

/// Parse `remote.origin.url` out of a git config file's text. The last value
/// wins for a single-valued key. Returns None if origin has no url.
fn parse_remote_origin_url(text: &str) -> Option<String> {
    let mut in_origin = false;
    let mut url: Option<String> = None;
    for raw in text.split('\n') {
        let line = raw.trim_end_matches('\r').trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some((sec_name, sec_sub)) = parse_section_header(line) {
            in_origin = sec_name.to_lowercase() == "remote" && sec_sub.as_deref() == Some("origin");
            continue;
        }
        if in_origin {
            if let Some((key, val)) = parse_key_value(line) {
                if key.to_lowercase() == "url" {
                    url = Some(decode_git_config_value(val));
                }
            }
        }
    }
    url
}

/// Match `^\[([\w.-]+)(?:\s+"(.*)")?\]$`. Returns (section, optional subsection).
fn parse_section_header(line: &str) -> Option<(String, Option<String>)> {
    let inner = line.strip_prefix('[')?.strip_suffix(']')?;
    // section name: [\w.-]+ — `\w` is ASCII-only in v1's JS regex, so match
    // ASCII alphanumerics (not Unicode `is_alphanumeric`) to stay byte-parity.
    let name_end = inner
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-'))
        .unwrap_or(inner.len());
    let name = &inner[..name_end];
    if name.is_empty() {
        return None;
    }
    let rest = &inner[name_end..];
    if rest.is_empty() {
        return Some((name.to_string(), None));
    }
    // rest must be: \s+"(.*)"
    let trimmed = rest.trim_start();
    if trimmed.len() == rest.len() {
        // no whitespace separating -> not a valid header form
        return None;
    }
    let sub = trimmed.strip_prefix('"')?.strip_suffix('"')?;
    Some((name.to_string(), Some(sub.to_string())))
}

/// Match `^([A-Za-z0-9_-]+)\s*=\s*(.*)$`. Returns (key, value).
fn parse_key_value(line: &str) -> Option<(&str, &str)> {
    let eq = line.find('=')?;
    let key = line[..eq].trim_end();
    if key.is_empty()
        || !key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return None;
    }
    let val = line[eq + 1..].trim_start();
    Some((key, val))
}

/// Locate the repo by walking up from cwd, reading the filesystem only.
/// Mirrors `git rev-parse --show-toplevel` for common cases, resolving worktree
/// `.git` file pointers and their shared common dir. Returns None when no repo.
fn find_git_dir_fs(cwd: &Path) -> Option<GitDirInfo> {
    let mut dir = std::fs::canonicalize(cwd)
        .map(strip_verbatim_prefix)
        .unwrap_or_else(|_| cwd.to_path_buf());
    loop {
        let dotgit = dir.join(".git");
        if let Ok(meta) = std::fs::metadata(&dotgit) {
            let mut git_dir = dotgit.clone();
            if meta.is_file() {
                let txt = std::fs::read_to_string(&dotgit).unwrap_or_default();
                let pointer = parse_gitdir_pointer(&txt)?;
                let p = Path::new(&pointer);
                git_dir = if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    // resolve(dir, pointer) — lexical join then normalize
                    PathBuf::from(lexical_resolve(
                        &dir.to_string_lossy(),
                        &pointer.replace('\\', "/"),
                    ))
                };
            }
            let mut common_dir = git_dir.clone();
            if let Ok(cd_raw) = std::fs::read_to_string(git_dir.join("commondir")) {
                let cd = cd_raw.trim();
                if !cd.is_empty() {
                    let p = Path::new(cd);
                    common_dir = if p.is_absolute() {
                        p.to_path_buf()
                    } else {
                        PathBuf::from(lexical_resolve(
                            &git_dir.to_string_lossy(),
                            &cd.replace('\\', "/"),
                        ))
                    };
                }
            }
            return Some(GitDirInfo {
                root: dir,
                git_dir,
                common_dir,
            });
        }
        match dir.parent() {
            Some(parent) if parent != dir => dir = parent.to_path_buf(),
            _ => return None,
        }
    }
}

/// Extract `<path>` from a `.git` file's `gitdir: <path>` pointer.
/// Mirrors v1's `/gitdir:\s*(.+?)\s*$/m`.
fn parse_gitdir_pointer(txt: &str) -> Option<String> {
    for line in txt.split('\n') {
        let line = line.trim_end_matches('\r');
        if let Some(rest) = line.trim_start().strip_prefix("gitdir:") {
            let p = rest.trim();
            if !p.is_empty() {
                return Some(p.to_string());
            }
        }
    }
    None
}

/// Filesystem fallback for `git config --get remote.origin.url`.
fn read_remote_origin_url_fs(git_info: &Option<GitDirInfo>) -> Option<String> {
    let info = git_info.as_ref()?;
    for base in [&info.common_dir, &info.git_dir] {
        let cfg = base.join("config");
        let txt = match std::fs::read_to_string(&cfg) {
            Ok(t) => t,
            Err(_) => continue,
        };
        if let Some(url) = parse_remote_origin_url(&txt) {
            return Some(url);
        }
    }
    None
}

/// Strip the Windows `\\?\` verbatim (and `\\?\UNC\`) prefix that
/// `std::fs::canonicalize` produces but v1's `realpathSync` does not.
fn strip_verbatim_prefix(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        // \\?\UNC\server\share -> \\server\share
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        return PathBuf::from(rest.to_string());
    }
    p
}

/// Canonicalize like v1's `realpathSync`, stripping the Windows verbatim prefix.
/// Falls back to the input string if canonicalize fails.
fn realpath_str(p: &str) -> String {
    std::fs::canonicalize(p)
        .map(strip_verbatim_prefix)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| p.to_string())
}

/// Core fingerprint computation, parameterized over the git runner.
pub fn fingerprint_info(cwd: &Path, git: &dyn GitRunner) -> FingerprintInfo {
    // `blocked` is a Cell so the `git_get` closure can record a block via a
    // shared borrow, leaving `blocked` readable between calls (a `&mut` capture
    // would borrow it for the whole closure lifetime and the reads below would
    // not compile).
    let blocked = std::cell::Cell::new(false);
    let git_get = |args: &[&str]| -> Option<String> {
        match git.run(cwd, args) {
            GitResult::Ok(v) => {
                let t = v.trim().to_string();
                if t.is_empty() {
                    None
                } else {
                    Some(t)
                }
            }
            GitResult::Failed { blocked: b } => {
                if b {
                    blocked.set(true);
                }
                None
            }
        }
    };

    // Resolve the .git location from the filesystem at most once, lazily, and
    // only when needed (when git is blocked).
    let mut fs_info: Option<Option<GitDirInfo>> = None;

    // 1) remote.origin.url
    let mut url = git_get(&["config", "--get", "remote.origin.url"]);
    if url.is_none() && blocked.get() {
        let info = fs_info.get_or_insert_with(|| find_git_dir_fs(cwd));
        url = read_remote_origin_url_fs(info);
    }
    if let Some(raw) = url {
        let cleaned = sanitize_remote_url(&raw);
        let value = if !is_scheme_url(&cleaned)
            && !is_scp_like(&cleaned)
            && !Path::new(&cleaned).is_absolute()
            && !is_windows_drive(&cleaned)
        {
            // Relative local remote: anchor to the repo root, resolved lexically.
            let root = git_get(&["rev-parse", "--show-toplevel"])
                .or_else(|| {
                    if blocked.get() {
                        let info = fs_info.get_or_insert_with(|| find_git_dir_fs(cwd));
                        info.as_ref().map(|g| g.root.to_string_lossy().into_owned())
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| cwd.to_string_lossy().into_owned());
            lexical_resolve(&root.replace('\\', "/"), &cleaned.replace('\\', "/"))
        } else {
            cleaned
        };
        return finish(BasisType::Remote, format!("remote:{value}"));
    }

    // 2) gitroot
    let mut root = git_get(&["rev-parse", "--show-toplevel"]);
    if root.is_none() && blocked.get() {
        let info = fs_info.get_or_insert_with(|| find_git_dir_fs(cwd));
        root = info.as_ref().map(|g| g.root.to_string_lossy().into_owned());
    }
    if let Some(r) = root {
        let resolved = realpath_str(&r);
        return finish(BasisType::Gitroot, format!("gitroot:{resolved}"));
    }

    // 3) path
    let resolved = realpath_str(&cwd.to_string_lossy());
    finish(BasisType::Path, format!("path:{resolved}"))
}

fn finish(kind: BasisType, value: String) -> FingerprintInfo {
    FingerprintInfo {
        fingerprint: sha24(&value),
        basis: Basis { kind, value },
    }
}

/// Real git runner using `std::process::Command`.
///
/// Maps a spawn failure whose OS error kind is `PermissionDenied`/`NotFound`
/// (sandbox block or missing binary) to `blocked:true`; a non-zero exit (git
/// ran but the command failed) maps to `blocked:false`.
pub struct RealGit;

impl GitRunner for RealGit {
    fn run(&self, cwd: &Path, args: &[&str]) -> GitResult {
        // no_window_command: the daemon computes fingerprints inside the
        // windowless background host, where a plain `git` spawn opens a
        // visible terminal window on Windows.
        let mut cmd = crate::process::no_window_command("git");
        cmd.arg("-C").arg(cwd).args(args);
        cmd.stdin(std::process::Stdio::null());
        match cmd.output() {
            Ok(out) => {
                if out.status.success() {
                    GitResult::Ok(String::from_utf8_lossy(&out.stdout).to_string())
                } else {
                    GitResult::Failed { blocked: false }
                }
            }
            Err(e) => {
                let blocked = matches!(
                    e.kind(),
                    std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::NotFound
                );
                GitResult::Failed { blocked }
            }
        }
    }
}

/// Compute the project fingerprint using the real git binary.
pub fn fingerprint(cwd: &Path) -> String {
    fingerprint_info(cwd, &RealGit).fingerprint
}

/// When `cwd` is inside a LINKED `git worktree`, the root directory of the
/// primary checkout it belongs to; `None` for a plain checkout or a non-repo.
/// Filesystem-only (no git process): the worktree is detected by its
/// `commondir` pointing away from its own git dir, and the main working
/// directory is the parent of the shared `.git`. Lets callers deriving a
/// display name skip an auto-generated worktree basename.
pub fn linked_worktree_main_root(cwd: &Path) -> Option<PathBuf> {
    let info = find_git_dir_fs(cwd)?;
    if info.common_dir == info.git_dir {
        return None; // plain checkout: no commondir redirection
    }
    if info.common_dir.file_name().and_then(|n| n.to_str()) == Some(".git") {
        info.common_dir.parent().map(Path::to_path_buf)
    } else {
        None // bare-style shared dir: no primary working directory to name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // A GitRunner that always reports the binary as blocked (sandbox EPERM).
    struct Blocked;
    impl GitRunner for Blocked {
        fn run(&self, _: &Path, _: &[&str]) -> GitResult {
            GitResult::Failed { blocked: true }
        }
    }
    // A GitRunner that ran but found no repo (exit status, NOT blocked).
    struct RanNoRepo;
    impl GitRunner for RanNoRepo {
        fn run(&self, _: &Path, _: &[&str]) -> GitResult {
            GitResult::Failed { blocked: false }
        }
    }
    // A canned runner returning a fixed remote url for `config --get remote.origin.url`.
    struct FixedRemote(&'static str);
    impl GitRunner for FixedRemote {
        fn run(&self, _: &Path, args: &[&str]) -> GitResult {
            if args == ["config", "--get", "remote.origin.url"] {
                GitResult::Ok(self.0.to_string())
            } else if args == ["rev-parse", "--show-toplevel"] {
                GitResult::Ok("/repo/root".to_string())
            } else {
                GitResult::Failed { blocked: false }
            }
        }
    }
    // Like FixedRemote but with a caller-chosen `--show-toplevel` root, so a test
    // can pin a drive-rooted Windows root (`C:/repo/root`) and stay deterministic.
    struct FixedRemoteRoot {
        url: &'static str,
        root: &'static str,
    }
    impl GitRunner for FixedRemoteRoot {
        fn run(&self, _: &Path, args: &[&str]) -> GitResult {
            if args == ["config", "--get", "remote.origin.url"] {
                GitResult::Ok(self.url.to_string())
            } else if args == ["rev-parse", "--show-toplevel"] {
                GitResult::Ok(self.root.to_string())
            } else {
                GitResult::Failed { blocked: false }
            }
        }
    }

    #[test]
    fn deterministic_24_hex_for_path_basis() {
        let dir = tempfile::tempdir().unwrap();
        let a = fingerprint_info(dir.path(), &RanNoRepo);
        let b = fingerprint_info(dir.path(), &RanNoRepo);
        assert_eq!(a.fingerprint, b.fingerprint);
        assert_eq!(a.basis.kind, BasisType::Path);
        assert_eq!(a.fingerprint.len(), 24);
        assert!(a
            .fingerprint
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[test]
    fn strips_userinfo_credentials() {
        let info = fingerprint_info(
            Path::new("/x"),
            &FixedRemote("https://user:USERINFO_SECRET@example.invalid/org/repo.git"),
        );
        assert_eq!(info.basis.kind, BasisType::Remote);
        assert!(!info.basis.value.contains("USERINFO_SECRET"));
    }

    #[test]
    fn strips_userinfo_with_at_in_password() {
        // Password itself contains '@'; the LAST '@' before authority end is the
        // delimiter, so the whole password tail must be removed.
        let info = fingerprint_info(
            Path::new("/x"),
            &FixedRemote("https://user:p@ssSECRET@example.invalid/org/repo.git"),
        );
        assert!(!info.basis.value.contains("SECRET"));
        assert_eq!(
            info.basis.value,
            "remote:https://example.invalid/org/repo.git"
        );
    }

    #[test]
    fn strips_query_and_fragment_credentials() {
        let info = fingerprint_info(
            Path::new("/x"),
            &FixedRemote(
                "https://example.invalid/org/repo.git?access_token=QUERY_SECRET#FRAG_SECRET",
            ),
        );
        assert!(!info.basis.value.contains("QUERY_SECRET"));
        assert!(!info.basis.value.contains("FRAG_SECRET"));
    }

    #[test]
    fn at_in_query_without_path_is_not_userinfo() {
        // host?token=ab@cd with no path: the '@' is inside the query, not
        // userinfo. The host must survive; the query is then stripped.
        let info = fingerprint_info(
            Path::new("/x"),
            &FixedRemote("https://example.invalid?token=ab@cdSECRET"),
        );
        assert!(!info.basis.value.contains("SECRET"));
        assert_eq!(info.basis.value, "remote:https://example.invalid");
    }

    #[test]
    fn leaves_scp_ssh_untouched() {
        let info = fingerprint_info(
            Path::new("/x"),
            &FixedRemote("git@example.invalid:org/repo.git"),
        );
        assert_eq!(info.basis.value, "remote:git@example.invalid:org/repo.git");
    }

    #[test]
    fn ran_no_repo_keeps_path_basis() {
        let dir = tempfile::tempdir().unwrap();
        let info = fingerprint_info(dir.path(), &RanNoRepo);
        assert_eq!(info.basis.kind, BasisType::Path);
    }

    // When git is blocked (sandbox EPERM), the filesystem fallback must read the
    // remote from a real `.git/config` and yield the SAME remote basis a working
    // git would. This is the P1 regression that motivated the whole port: a
    // blocked agent must land in the same capsule bucket as its peer.
    #[test]
    fn blocked_runner_reads_remote_from_git_config_fs() {
        let dir = tempfile::tempdir().unwrap();
        let git = dir.path().join(".git");
        std::fs::create_dir(&git).unwrap();
        std::fs::write(
            git.join("config"),
            "[core]\n\trepositoryformatversion = 0\n[remote \"origin\"]\n\turl = https://example.invalid/org/repo.git\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n",
        )
        .unwrap();
        let info = fingerprint_info(dir.path(), &Blocked);
        assert_eq!(info.basis.kind, BasisType::Remote);
        assert_eq!(
            info.basis.value,
            "remote:https://example.invalid/org/repo.git"
        );
    }

    // The fs fallback also decodes a QUOTED config value the way `git config
    // --get` does, so the fingerprint matches working git (and the credential
    // sanitizer still sees an unquoted URL).
    #[test]
    fn blocked_runner_decodes_quoted_config_value() {
        let dir = tempfile::tempdir().unwrap();
        let git = dir.path().join(".git");
        std::fs::create_dir(&git).unwrap();
        std::fs::write(
            git.join("config"),
            "[remote \"origin\"]\n\turl = \"https://example.invalid/org/repo.git\"\n",
        )
        .unwrap();
        let info = fingerprint_info(dir.path(), &Blocked);
        assert_eq!(
            info.basis.value,
            "remote:https://example.invalid/org/repo.git"
        );
    }

    // A git that RAN but found no repo (non-blocked exit) must NOT trigger the fs
    // fallback even if an ancestor `.git` exists on disk — it keeps the path
    // basis. (RanNoRepo never reports blocked, so the walk is skipped.)
    #[test]
    fn non_blocked_exit_does_not_trigger_fs_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let git = dir.path().join(".git");
        std::fs::create_dir(&git).unwrap();
        std::fs::write(
            git.join("config"),
            "[remote \"origin\"]\n\turl = https://example.invalid/org/repo.git\n",
        )
        .unwrap();
        // Even though a readable .git/config with a remote exists, a non-blocked
        // failure must keep the path basis.
        let info = fingerprint_info(dir.path(), &RanNoRepo);
        assert_eq!(info.basis.kind, BasisType::Path);
    }

    // HAZARD 1: relative local remote resolved lexically with the PLATFORM-DEFAULT
    // `path.resolve` (`path.win32.resolve` on Windows, `path.posix.resolve` on
    // Unix), matching v1. Each expected string below is the exact output of the
    // corresponding Node resolver, derived (not hand-guessed) via:
    //   node -e "const p=require('path'); console.log(p.posix.resolve('/repo/root','../upstream.git'))"
    //     -> /repo/upstream.git
    //   node -e "const p=require('path'); console.log(p.win32.resolve('C:/repo/root','../upstream.git'))"
    //     -> C:\repo\upstream.git
    #[cfg(unix)]
    #[test]
    fn relative_remote_resolves_lexically() {
        // FixedRemote pins the toplevel root to `/repo/root`.
        let info = fingerprint_info(Path::new("/x"), &FixedRemote("../upstream.git"));
        assert_eq!(info.basis.kind, BasisType::Remote);
        // path.posix.resolve('/repo/root','../upstream.git')
        assert_eq!(info.basis.value, "remote:/repo/upstream.git");
    }

    #[cfg(windows)]
    #[test]
    fn relative_remote_resolves_lexically() {
        // Drive-rooted Windows toplevel (as `git rev-parse --show-toplevel`
        // emits on Windows), so the result is deterministic and drive-preserving.
        let info = fingerprint_info(
            Path::new("C:/x"),
            &FixedRemoteRoot {
                url: "../upstream.git",
                root: "C:/repo/root",
            },
        );
        assert_eq!(info.basis.kind, BasisType::Remote);
        // path.win32.resolve('C:/repo/root','../upstream.git')
        assert_eq!(info.basis.value, r"remote:C:\repo\upstream.git");

        // Nested `../../` case: path.win32.resolve('C:/repo/root','../../x/y.git')
        //   -> C:\x\y.git
        let nested = fingerprint_info(
            Path::new("C:/x"),
            &FixedRemoteRoot {
                url: "../../x/y.git",
                root: "C:/repo/root",
            },
        );
        assert_eq!(nested.basis.value, r"remote:C:\x\y.git");
    }

    // A linked worktree must resolve to the MAIN checkout's root via its
    // commondir, not to the worktree directory itself.
    #[test]
    fn main_root_resolves_linked_worktree() {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("myproject");
        let wt_meta = main.join(".git").join("worktrees").join("wt");
        std::fs::create_dir_all(&wt_meta).unwrap();
        std::fs::write(wt_meta.join("commondir"), "../..\n").unwrap();

        let worktree = tmp.path().join("loving-sanderson-9a5e09");
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(
            worktree.join(".git"),
            format!("gitdir: {}\n", wt_meta.to_string_lossy()),
        )
        .unwrap();

        let root = linked_worktree_main_root(&worktree).expect("worktree resolves to main root");
        assert_eq!(root.file_name().and_then(|n| n.to_str()), Some("myproject"));
    }

    // A plain checkout has no commondir redirection and must stay `None`, so
    // label derivation keeps using the cwd itself.
    #[test]
    fn main_root_is_none_for_plain_checkout() {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("plainproject");
        std::fs::create_dir_all(main.join(".git")).unwrap();
        assert!(linked_worktree_main_root(&main).is_none());
    }

    // HAZARD 2: a canonicalized path basis must have no Windows `\\?\` prefix.
    #[test]
    fn canonicalized_path_has_no_verbatim_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let info = fingerprint_info(dir.path(), &RanNoRepo);
        assert!(info.basis.value.starts_with("path:"));
        assert!(
            !info.basis.value.contains(r"\\?\"),
            "verbatim prefix leaked into basis: {}",
            info.basis.value
        );
    }

    // Parity guard: this exact value is computed by v1 and must never change.
    #[test]
    fn parity_scp_remote_hash_matches_v1() {
        let info = fingerprint_info(
            Path::new("/x"),
            &FixedRemote("git@github.com:Lumisia/claude-codex-auto-handoff.git"),
        );
        // Produced by the v1 canonical hash:
        // sha256("remote:git@github.com:Lumisia/claude-codex-auto-handoff.git")[0..24]
        let expected = "7974ce42a41cfb682ca3d093";
        assert_eq!(info.fingerprint, expected);
    }

    // Step-6 GATE: cross-check against the real repo using the actual git
    // binary. Ignored by default (depends on the host repo + git on PATH); run
    // explicitly with the repo path in AH_FP_REAL_CWD:
    //   AH_FP_REAL_CWD="$PWD" cargo test -p ai-handoff-core \
    //       real_repo_fingerprint -- --ignored --nocapture
    // The printed 24-hex must equal v1's `projectFingerprint(process.cwd())`.
    #[test]
    #[ignore = "needs real repo + git; run via AH_FP_REAL_CWD"]
    fn real_repo_fingerprint() {
        let cwd = std::env::var("AH_FP_REAL_CWD").expect("set AH_FP_REAL_CWD to the repo dir");
        let info = fingerprint_info(Path::new(&cwd), &RealGit);
        println!("V2_REAL_FINGERPRINT={}", info.fingerprint);
        println!("V2_REAL_BASIS={:?}", info.basis);
    }

    // Helper to drive the fs-fallback path for an arbitrary remote URL, printing
    // value+fingerprint so they can be diffed against the Node parity script.
    // Ignored: developer cross-check only.
    #[test]
    #[ignore = "cross-check helper; diff against _fp_parity_check.mjs"]
    fn print_tricky_url_parity() {
        let cases = [
            "https://user:p@ss@host.example/r.git",
            "https://host.example?token=ab@cd",
            "git@host.example:org/repo.git",
            "ssh://git@host.example/org/repo.git",
            "C:/Users/x/repo.git",
            "C:\\Users\\x\\repo.git",
            "https://host.example/a/../b/repo.git",
        ];
        for c in cases {
            let dir = tempfile::tempdir().unwrap();
            let git = dir.path().join(".git");
            std::fs::create_dir(&git).unwrap();
            std::fs::write(
                git.join("config"),
                format!("[remote \"origin\"]\n\turl = {c}\n"),
            )
            .unwrap();
            let info = fingerprint_info(dir.path(), &Blocked);
            println!("{c:?}\t{}\t{}", info.basis.value, info.fingerprint);
        }
        // quoted value with trailing comment
        let dir = tempfile::tempdir().unwrap();
        let git = dir.path().join(".git");
        std::fs::create_dir(&git).unwrap();
        std::fs::write(
            git.join("config"),
            "[remote \"origin\"]\n\turl = \"https://host.example/r.git\" ; trailing comment\n",
        )
        .unwrap();
        let info = fingerprint_info(dir.path(), &Blocked);
        println!(
            "QUOTED_WITH_COMMENT\t{}\t{}",
            info.basis.value, info.fingerprint
        );
    }
}
