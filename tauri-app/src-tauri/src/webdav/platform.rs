// OS-level mount helpers and platform diagnostics.
//
// Windows: map the gateway as a drive letter via WNetAddConnection2W (the
// password stays in memory, never on a command line). Requires the WebClient
// service, which is missing on Home editions and needs registry tweaks to
// accept Basic auth over plain HTTP — diagnose() reports each, apply_fix()
// repairs via an elevated PowerShell one-shot.
//
// macOS: hand a webdav:// URL (credentials embedded, localhost-only) to
// Finder via `open`; unmount via diskutil after locating the volume in
// `mount` output.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnosis {
    pub platform: String,
    /// "running" | "stopped" | "transition" | "not_installed" | "unknown"
    pub webclient_state: Option<String>,
    /// Registry BasicAuthLevel; 1 = HTTPS-only (blocks our loopback HTTP).
    pub basic_auth_level: Option<u32>,
    /// Registry FileSizeLimitInBytes (WebClient per-file transfer cap).
    pub file_size_limit_in_bytes: Option<u64>,
    /// Stable issue codes; frontend maps each to localized copy.
    pub issues: Vec<String>,
    pub fixable: bool,
}

const REG_PATH: &str = r"SYSTEM\CurrentControlSet\Services\WebClient\Parameters";
const DEFAULT_FILE_SIZE_LIMIT: u64 = 50_000_000;

pub fn diagnose() -> Diagnosis {
    #[cfg(windows)]
    {
        let webclient_state = webclient_service_state();
        let basic_auth_level = reg_dword("BasicAuthLevel");
        let file_size_limit = reg_dword("FileSizeLimitInBytes").map(|v| v as u64);

        let mut issues = Vec::new();
        match webclient_state.as_deref() {
            Some("not_installed") => issues.push("webclient_not_installed".to_string()),
            Some("stopped") => issues.push("webclient_stopped".to_string()),
            _ => {}
        }
        if let Some(level) = basic_auth_level {
            if level < 2 {
                issues.push("basic_auth_blocked".to_string());
            }
        } else {
            // Missing value = default 1 (HTTPS only), which blocks loopback HTTP.
            issues.push("basic_auth_blocked".to_string());
        }
        if let Some(limit) = file_size_limit {
            if (limit as u64) < 1_000_000_000 {
                issues.push("file_size_limited".to_string());
            }
        } else {
            issues.push("file_size_limited".to_string());
        }

        let fixable = !issues.is_empty() && !issues.contains(&"webclient_not_installed".to_string());
        Diagnosis {
            platform: "windows".to_string(),
            webclient_state,
            basic_auth_level,
            file_size_limit_in_bytes: file_size_limit.or(Some(DEFAULT_FILE_SIZE_LIMIT)),
            issues,
            // Not-installed (Home) cannot be fixed by us; everything else can.
            fixable,
        }
    }

    #[cfg(target_os = "macos")]
    {
        Diagnosis {
            platform: "macos".to_string(),
            webclient_state: None,
            basic_auth_level: None,
            file_size_limit_in_bytes: None,
            issues: Vec::new(),
            fixable: false,
        }
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    {
        Diagnosis {
            platform: "other".to_string(),
            webclient_state: None,
            basic_auth_level: None,
            file_size_limit_in_bytes: None,
            issues: vec!["platform_not_supported".to_string()],
            fixable: false,
        }
    }
}

/// Map the gateway URL as an OS mount; returns the drive letter ("X:") on
/// Windows or the webdav URL handed to Finder on macOS.
pub fn mount_drive(
    port: u16,
    mount_id: &str,
    username: &str,
    password: &str,
    preferred_letter: Option<&str>,
) -> Result<String, String> {
    #[cfg(windows)]
    {
        let _ = username;
        windows_mount(port, mount_id, password, preferred_letter)
    }

    #[cfg(target_os = "macos")]
    {
        let _ = preferred_letter;
        let url = format!(
            "webdav://{}:{}@127.0.0.1:{}/m/{}",
            username, password, port, mount_id
        );
        std::process::Command::new("open")
            .arg(&url)
            .spawn()
            .map_err(|e| format!("failed to open Finder mount: {}", e))?;
        Ok(url)
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = (port, mount_id, username, password, preferred_letter);
        Err("platform_not_supported".to_string())
    }
}

pub fn unmount_drive(
    port: u16,
    mount_id: &str,
    letter: Option<&str>,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        let _ = port; // letter-based cancel needs no port
        windows_unmount(mount_id, letter)
    }

    #[cfg(target_os = "macos")]
    {
        let _ = letter;
        macos_unmount(port, mount_id)
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = (port, mount_id, letter);
        Err("platform_not_supported".to_string())
    }
}

/// Best-effort OS-level mount detection. `false` does not rule out a manual
/// mount the OS reports in a way we cannot see.
pub fn detect_mounted(port: u16, mount_id: &str, letter: Option<&str>) -> bool {
    #[cfg(windows)]
    {
        let _ = port; // letter-based detection needs no port
        windows_detect(mount_id, letter)
    }

    #[cfg(target_os = "macos")]
    {
        let _ = letter;
        macos_find_volume_dir(port, mount_id).is_some()
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = (port, mount_id, letter);
        false
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Windows
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(windows)]
mod win {
    use std::path::Path;

    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::NetworkManagement::WNet::{
        WNetAddConnection2W, WNetCancelConnection2W, WNetGetConnectionW, NET_CONNECT_FLAGS,
        NETRESOURCEW, RESOURCETYPE_DISK,
    };

    pub fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn remote_name(port: u16, mount_id: &str) -> String {
        // WebClient UNC form for non-SSL WebDAV: \\server@port\path
        format!(r"\\127.0.0.1@{}\m\{}", port, mount_id)
    }

    pub fn letter_in_use(letter: &str) -> bool {
        Path::new(&format!("{}:\\", letter)).exists()
    }

    pub fn pick_letter(preferred: Option<&str>) -> Option<String> {
        if let Some(l) = preferred {
            let l = l.trim().trim_end_matches(':').to_uppercase();
            if l.len() == 1 && l.as_bytes()[0].is_ascii_alphabetic() && !letter_in_use(&l) {
                return Some(l);
            }
        }
        // Prefer late letters; they are least likely taken by removable drives.
        for code in (b'D'..=b'Z').rev() {
            let l = (code as char).to_string();
            if !letter_in_use(&l) {
                return Some(l);
            }
        }
        None
    }

    pub fn add_connection(
        remote: &str,
        password: &str,
        local_letter: Option<&str>,
    ) -> u32 {
        let mut remote_w = wide(remote);
        let password_w = wide(password);
        let mut local_w = local_letter.map(|l| wide(&format!("{}:", l.trim_end_matches(':'))));

        let mut netres = NETRESOURCEW::default();
        netres.dwType = RESOURCETYPE_DISK;
        netres.lpRemoteName = PWSTR(remote_w.as_mut_ptr());
        if let Some(ref mut lw) = local_w {
            netres.lpLocalName = PWSTR(lw.as_mut_ptr());
        }

        let err = unsafe {
            WNetAddConnection2W(
                &netres,
                PCWSTR::from_raw(password_w.as_ptr()),
                PCWSTR::null(),
                NET_CONNECT_FLAGS(0),
            )
        };
        // keep the wide buffers alive until the call returns
        let _ = (&remote_w, &password_w, &local_w);
        err.0
    }

    pub fn cancel(name: &str) -> u32 {
        let name_w = wide(name);
        let err = unsafe {
            WNetCancelConnection2W(PCWSTR::from_raw(name_w.as_ptr()), NET_CONNECT_FLAGS(0), true)
        };
        let _ = &name_w;
        err.0
    }

    /// Query the remote path of a locally mapped drive letter.
    pub fn get_remote(letter: &str) -> Option<String> {
        let local_w = wide(&format!("{}:", letter.trim_end_matches(':')));
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let err = unsafe {
            WNetGetConnectionW(
                PCWSTR::from_raw(local_w.as_ptr()),
                Some(PWSTR(buf.as_mut_ptr())),
                &mut len,
            )
        };
        if err.0 != 0 {
            return None;
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(len as usize);
        Some(String::from_utf16_lossy(&buf[..end]))
    }
}

#[cfg(windows)]
fn windows_mount(
    port: u16,
    mount_id: &str,
    password: &str,
    preferred_letter: Option<&str>,
) -> Result<String, String> {
    let remote = win::remote_name(port, mount_id);
    let letter = win::pick_letter(preferred_letter).ok_or("no_free_drive_letter")?;

    let mut err = win::add_connection(&remote, password, Some(&letter));
    if err == 1219 {
        // ERROR_SESSION_CREDENTIAL_CONFLICT: a stale session to the same
        // server exists; force-cancel and retry once with our credentials.
        let root = format!(r"\\127.0.0.1@{}", port);
        let _ = win::cancel(&root);
        err = win::add_connection(&remote, password, Some(&letter));
    }
    match err {
        0 => Ok(letter),
        // 5: WebClient refusing Basic over HTTP (BasicAuthLevel=1) is the
        // common cause; 1326/86: credential rejection; 1208: WebClient sick.
        5 => Err("access_denied_check_diagnostics".to_string()),
        86 | 1326 => Err("invalid_credentials".to_string()),
        1208 => Err("webclient_error_check_diagnostics".to_string()),
        code => Err(format!("mount_failed_windows_error_{}", code)),
    }
}

#[cfg(windows)]
fn windows_unmount(mount_id: &str, letter: Option<&str>) -> Result<(), String> {
    let target = match letter {
        Some(l) => Some(l.trim_end_matches(':').to_uppercase()),
        None => windows_find_letter(mount_id),
    };
    match target {
        Some(l) => {
            let err = win::cancel(&format!("{}:", l));
            if err == 0 || err == 2250 {
                Ok(())
            } else {
                Err(format!("unmount_failed_windows_error_{}", err))
            }
        }
        None => Err("mount_not_found".to_string()),
    }
}

#[cfg(windows)]
fn windows_find_letter(mount_id: &str) -> Option<String> {
    for code in b'D'..=b'Z' {
        let letter = (code as char).to_string();
        if let Some(remote) = win::get_remote(&letter) {
            if remote.ends_with(&format!(r"\m\{}", mount_id)) {
                return Some(letter);
            }
        }
    }
    None
}

#[cfg(windows)]
fn windows_detect(mount_id: &str, letter: Option<&str>) -> bool {
    match letter {
        Some(l) => win::get_remote(l).is_some(),
        None => windows_find_letter(mount_id).is_some(),
    }
}

#[cfg(windows)]
fn reg_dword(name: &str) -> Option<u32> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    let hk = winreg::RegKey::predef(HKEY_LOCAL_MACHINE);
    let key = hk.open_subkey(REG_PATH).ok()?;
    key.get_value::<u32, _>(name).ok()
}

/// `sc query WebClient` with locale-independent state parsing (the numeric
/// state code, not the localized word): 1 stopped, 4 running.
#[cfg(windows)]
fn webclient_service_state() -> Option<String> {
    let out = std::process::Command::new("sc.exe")
        .args(["query", "WebClient"])
        .output()
        .ok()?;
    if out.status.code() == Some(1060) {
        return Some("not_installed".to_string());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        if line.contains("STATE") {
            if let Some(num) = line
                .split(':')
                .nth(1)
                .and_then(|rest| rest.split_whitespace().next())
                .and_then(|tok| tok.parse::<u32>().ok())
            {
                return Some(
                    match num {
                        1 => "stopped",
                        4 => "running",
                        2 | 3 | 5 | 6 | 7 => "transition",
                        _ => "unknown",
                    }
                    .to_string(),
                );
            }
        }
    }
    Some("unknown".to_string())
}

/// Elevated one-shot repair: BasicAuthLevel=2, remove the 50MB transfer cap,
/// ensure WebClient is started. The inner script travels as a base64
/// -EncodedCommand so quoting never survives the UAC boundary.
#[cfg(windows)]
pub fn apply_fix() -> Result<(), String> {
    const FIX_SCRIPT: &str = r#"
$ErrorActionPreference = 'Continue'
reg.exe add "HKLM\SYSTEM\CurrentControlSet\Services\WebClient\Parameters" /v BasicAuthLevel /t REG_DWORD /d 2 /f | Out-Null
reg.exe add "HKLM\SYSTEM\CurrentControlSet\Services\WebClient\Parameters" /v FileSizeLimitInBytes /t REG_DWORD /d 4294967295 /f | Out-Null
sc.exe config WebClient start= demand | Out-Null
net.exe stop WebClient /y 2>$null | Out-Null
net.exe start WebClient
exit 0
"#;
    let utf16: Vec<u8> = FIX_SCRIPT
        .encode_utf16()
        .flat_map(|u| u.to_le_bytes())
        .collect();
    let encoded = {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD.encode(utf16)
    };
    let outer = format!(
        "Start-Process powershell -Verb RunAs -Wait -ArgumentList '-NoProfile','-EncodedCommand','{}'",
        encoded
    );
    let out = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", &outer])
        .output()
        .map_err(|e| format!("failed to launch elevated fix: {}", e))?;
    if !out.status.success() {
        return Err("fix_cancelled_or_failed".to_string());
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// macOS
// ─────────────────────────────────────────────────────────────────────────────

/// Find the /Volumes directory backing our webdav mount, via `mount` output:
/// `//user@127.0.0.1:3980/m/{id} on /Volumes/{name} (webdav, ...)`.
#[cfg(target_os = "macos")]
fn macos_find_volume_dir(port: u16, mount_id: &str) -> Option<String> {
    let out = std::process::Command::new("mount")
        .output()
        .ok()?;
    let needle = format!("127.0.0.1:{}/m/{}", port, mount_id);
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        if !line.contains(&needle) {
            continue;
        }
        if let Some(idx) = line.find(" on ") {
            let rest = &line[idx + 4..];
            let dir = rest.split_whitespace().next().unwrap_or("");
            if !dir.is_empty() {
                return Some(dir.to_string());
            }
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn macos_unmount(port: u16, mount_id: &str) -> Result<(), String> {
    let dir = macos_find_volume_dir(port, mount_id).ok_or("mount_not_found")?;
    let out = std::process::Command::new("diskutil")
        .args(["unmount", "force", &dir])
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "unmount failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

#[cfg(not(windows))]
pub fn apply_fix() -> Result<(), String> {
    Err("fix_not_supported".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnosis_serializes_with_issue_codes() {
        let d = Diagnosis {
            platform: "windows".to_string(),
            webclient_state: Some("stopped".to_string()),
            basic_auth_level: Some(1),
            file_size_limit_in_bytes: Some(50_000_000),
            issues: vec!["webclient_stopped".to_string(), "basic_auth_blocked".to_string()],
            fixable: true,
        };
        let json = serde_json::to_value(&d).unwrap();
        assert_eq!(json["platform"], "windows");
        assert_eq!(json["webclientState"], "stopped");
        assert_eq!(json["fileSizeLimitInBytes"], 50_000_000);
        assert_eq!(json["issues"][0], "webclient_stopped");
    }
}
