//! macOS Keychain adapter for the Claude Code credential.
//!
//! On macOS, Claude Code stores its OAuth credential as a generic password
//! (service `"Claude Code-credentials"`) instead of `~/.claude/.credentials.json`.
//! A plain file swap therefore cannot switch the live login there. This module
//! uses the native Security framework for writes so credential JSON never
//! appears in process arguments. Read/metadata paths still use `security find`
//! because those do not place the secret in argv.
//!
//! Only the item's *password payload* (the credentials JSON) is read/written —
//! nothing is logged, and no network is involved.

/// The generic-password service name Claude Code uses.
#[cfg(target_os = "macos")]
const CLAUDE_SERVICE: &str = "Claude Code-credentials";

/// Escape hatch: `AI_HANDOFF_NO_KEYCHAIN=1` disables every Keychain
/// interaction. The test suite sets it so running tests on a developer's Mac
/// can never touch (let alone overwrite) the real Claude credential item.
#[cfg(target_os = "macos")]
fn keychain_disabled() -> bool {
    std::env::var_os("AI_HANDOFF_NO_KEYCHAIN").is_some_and(|value| !value.is_empty())
}

/// Read the Claude credential JSON from the login Keychain.
#[cfg(target_os = "macos")]
pub fn read_claude_credentials() -> std::io::Result<Vec<u8>> {
    if keychain_disabled() {
        return Err(std::io::Error::other("Keychain access disabled"));
    }
    let output = std::process::Command::new("security")
        .args(["find-generic-password", "-s", CLAUDE_SERVICE, "-w"])
        .output()?;
    if !output.status.success() {
        return Err(std::io::Error::other(format!(
            "security find-generic-password failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let raw = String::from_utf8_lossy(&output.stdout).into_owned();
    Ok(decode_security_password_output(&raw))
}

/// Write the Claude credential JSON into the login Keychain, replacing the
/// existing item. The account name is preserved from the existing item when
/// present, falling back to `$USER`.
#[cfg(target_os = "macos")]
pub fn write_claude_credentials(credential_json: &[u8]) -> std::io::Result<()> {
    if keychain_disabled() {
        return Err(std::io::Error::other("Keychain access disabled"));
    }
    let account = existing_claude_account()
        .or_else(|| std::env::var("USER").ok())
        .unwrap_or_else(|| "claude".to_string());
    macos_security::write_generic_password(CLAUDE_SERVICE, &account, credential_json)
}

#[cfg(target_os = "macos")]
mod macos_security {
    use std::ffi::{c_char, c_void, CString};
    use std::io;
    use std::ptr::{null, null_mut};

    type OSStatus = i32;
    type SecKeychainItemRef = *mut c_void;

    const ERR_SEC_SUCCESS: OSStatus = 0;
    const ERR_SEC_ITEM_NOT_FOUND: OSStatus = -25300;

    #[link(name = "Security", kind = "framework")]
    extern "C" {
        fn SecKeychainFindGenericPassword(
            keychain_or_array: *const c_void,
            service_name_length: u32,
            service_name: *const c_char,
            account_name_length: u32,
            account_name: *const c_char,
            password_length: *mut u32,
            password_data: *mut *mut c_void,
            item_ref: *mut SecKeychainItemRef,
        ) -> OSStatus;

        fn SecKeychainItemModifyAttributesAndData(
            item_ref: SecKeychainItemRef,
            attr_list: *const c_void,
            length: u32,
            data: *const c_void,
        ) -> OSStatus;

        fn SecKeychainAddGenericPassword(
            keychain: *const c_void,
            service_name_length: u32,
            service_name: *const c_char,
            account_name_length: u32,
            account_name: *const c_char,
            password_length: u32,
            password_data: *const c_void,
            item_ref: *mut SecKeychainItemRef,
        ) -> OSStatus;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: *const c_void);
    }

    pub(super) fn write_generic_password(
        service: &str,
        account: &str,
        password: &[u8],
    ) -> io::Result<()> {
        let service =
            CString::new(service).map_err(|_| io::Error::other("Keychain service contains NUL"))?;
        let account =
            CString::new(account).map_err(|_| io::Error::other("Keychain account contains NUL"))?;
        let service_len = checked_len(service.as_bytes().len(), "service")?;
        let account_len = checked_len(account.as_bytes().len(), "account")?;
        let password_len = checked_len(password.len(), "password")?;

        let mut item_ref: SecKeychainItemRef = null_mut();
        let find_status = unsafe {
            SecKeychainFindGenericPassword(
                null(),
                service_len,
                service.as_ptr(),
                account_len,
                account.as_ptr(),
                null_mut(),
                null_mut(),
                &mut item_ref,
            )
        };

        if find_status == ERR_SEC_SUCCESS {
            let modify_status = unsafe {
                SecKeychainItemModifyAttributesAndData(
                    item_ref,
                    null(),
                    password_len,
                    password.as_ptr().cast(),
                )
            };
            if !item_ref.is_null() {
                unsafe { CFRelease(item_ref.cast()) };
            }
            return status_to_result(modify_status, "SecKeychainItemModifyAttributesAndData");
        }

        if find_status != ERR_SEC_ITEM_NOT_FOUND {
            return status_to_result(find_status, "SecKeychainFindGenericPassword");
        }

        let add_status = unsafe {
            SecKeychainAddGenericPassword(
                null(),
                service_len,
                service.as_ptr(),
                account_len,
                account.as_ptr(),
                password_len,
                password.as_ptr().cast(),
                null_mut(),
            )
        };
        status_to_result(add_status, "SecKeychainAddGenericPassword")
    }

    fn checked_len(len: usize, what: &str) -> io::Result<u32> {
        u32::try_from(len)
            .map_err(|_| io::Error::other(format!("Keychain {what} length exceeds u32")))
    }

    fn status_to_result(status: OSStatus, operation: &str) -> io::Result<()> {
        if status == ERR_SEC_SUCCESS {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "{operation} failed with OSStatus {status}"
            )))
        }
    }
}

/// Whether a Claude credential item exists in the Keychain at all.
#[cfg(target_os = "macos")]
pub fn claude_item_exists() -> bool {
    if keychain_disabled() {
        return false;
    }
    std::process::Command::new("security")
        .args(["find-generic-password", "-s", CLAUDE_SERVICE])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// The account (`acct`) attribute of the existing Claude Keychain item, so a
/// rewrite keeps the item identity Claude Code expects.
#[cfg(target_os = "macos")]
fn existing_claude_account() -> Option<String> {
    let output = std::process::Command::new("security")
        .args(["find-generic-password", "-s", CLAUDE_SERVICE])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_security_account(&String::from_utf8_lossy(&output.stdout))
}

/// Parse the `"acct"<blob>="…"` attribute from `security find-generic-password`
/// metadata output.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse_security_account(metadata: &str) -> Option<String> {
    let line = metadata.lines().find(|line| line.contains("\"acct\""))?;
    let start = line.find("=\"")? + 2;
    let rest = &line[start..];
    let end = rest.rfind('"')?;
    let account = &rest[..end];
    (!account.is_empty()).then(|| account.to_string())
}

/// `security … -w` prints the password verbatim when it is printable, or as a
/// hex blob (`0x4A534F4E…  "JSON…"` style raw hex) otherwise. Normalize both
/// to raw bytes.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn decode_security_password_output(raw: &str) -> Vec<u8> {
    let trimmed = raw.trim_end_matches(['\r', '\n']);
    if let Some(hex) = trimmed.strip_prefix("0x") {
        // `0x<HEX>  "<lossy>"` — take the hex run before whitespace.
        let hex: String = hex.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
        if hex.len().is_multiple_of(2) {
            if let Ok(bytes) = decode_hex(&hex) {
                return bytes;
            }
        }
    }
    trimmed.as_bytes().to_vec()
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn decode_hex(hex: &str) -> Result<Vec<u8>, ()> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| ()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_password_output_is_passed_through() {
        assert_eq!(
            decode_security_password_output("{\"claudeAiOauth\":{}}\n"),
            b"{\"claudeAiOauth\":{}}".to_vec()
        );
    }

    #[test]
    fn hex_password_output_is_decoded() {
        // "0x" + hex("{}") + the lossy echo security appends.
        assert_eq!(
            decode_security_password_output("0x7B7D  \"{}\"\n"),
            b"{}".to_vec()
        );
    }

    #[test]
    fn account_attribute_is_parsed_from_metadata() {
        let metadata = "keychain: \"/Users/dev/Library/Keychains/login.keychain-db\"\n\
                        attributes:\n    \"acct\"<blob>=\"dev\"\n    \"svce\"<blob>=\"Claude Code-credentials\"\n";
        assert_eq!(parse_security_account(metadata).as_deref(), Some("dev"));
        assert_eq!(parse_security_account("no attributes"), None);
    }
}
