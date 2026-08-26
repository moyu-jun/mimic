//! Interception driver detection and restricted elevated maintenance.
//!
//! The normal Tauri process never elevates itself into the UI path. For install, uninstall, or
//! reboot it launches the current executable with a fixed helper switch through `runas`. The
//! elevated branch runs before Tauri initialization, accepts exactly one built-in action through a
//! versioned one-time request, verifies both its elevated token and the calling executable,
//! revalidates the pinned installer hash, and invokes fixed executables and arguments without a
//! shell.
use crate::state::DriverStatus;
use sha2::{Digest, Sha256};
use std::io::Read;

/// Read the current process token elevation state without requesting elevation.
pub(crate) fn is_process_elevated() -> Result<bool, String> {
    #[cfg(windows)]
    {
        use std::mem::{size_of, MaybeUninit};
        use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
        use windows_sys::Win32::Security::{
            GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
        };
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

        let mut token: HANDLE = std::ptr::null_mut();
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(format!(
                "admin_status_open_token_failed:{}",
                std::io::Error::last_os_error()
            ));
        }

        let mut elevation = MaybeUninit::<TOKEN_ELEVATION>::zeroed();
        let mut returned = 0_u32;
        let query_ok = unsafe {
            GetTokenInformation(
                token,
                TokenElevation,
                elevation.as_mut_ptr().cast(),
                size_of::<TOKEN_ELEVATION>() as u32,
                &mut returned,
            )
        };
        let query_error = (query_ok == 0).then(std::io::Error::last_os_error);
        unsafe { CloseHandle(token) };

        if let Some(error) = query_error {
            return Err(format!("admin_status_query_failed:{error}"));
        }
        if returned < size_of::<TOKEN_ELEVATION>() as u32 {
            return Err("admin_status_query_failed:short_response".to_string());
        }
        Ok(unsafe { elevation.assume_init() }.TokenIsElevated != 0)
    }
    #[cfg(not(windows))]
    {
        Ok(false)
    }
}
/// 检测 Interception 驱动当前状态
pub fn check_interception_driver() -> DriverStatus {
    #[cfg(windows)]
    {
        check_driver_windows()
    }
    #[cfg(not(windows))]
    {
        DriverStatus::NotInstalled
    }
}

/// 执行驱动安装（需管理员权限）
///
/// 成功调度安装器后返回 Ok(())；安装器本身的执行结果无法同步获取，
/// 调用者应在安装后重新 check_interception_driver() 判断状态。
pub fn install_driver() -> Result<(), String> {
    #[cfg(windows)]
    {
        run_elevated_helper_windows("install")
    }
    #[cfg(not(windows))]
    {
        Err("Driver installation is only supported on Windows".to_string())
    }
}

/// 执行驱动卸载（需管理员权限）
///
/// 与 install_driver() 对称：以管理员身份调用同一安装器的 `/uninstall` 参数。
/// 卸载后通常需重启系统才彻底移除，调用者应重新 check_interception_driver()。
pub fn uninstall_driver() -> Result<(), String> {
    #[cfg(windows)]
    {
        run_elevated_helper_windows("uninstall")
    }
    #[cfg(not(windows))]
    {
        Err("Driver uninstallation is only supported on Windows".to_string())
    }
}

/// 触发系统重启 — 驱动安装后需重启才会加载
///
/// 通过 `shutdown /r /t 0` 立即重启。需管理员权限——
/// 函数顶部加 `is_admin()` 防御检查，避免外层守卫被回归改坏后悄悄退化。
pub fn reboot_system() -> Result<(), String> {
    #[cfg(windows)]
    {
        run_elevated_helper_windows("reboot")
    }
    #[cfg(not(windows))]
    {
        Err("Reboot is only supported on Windows".to_string())
    }
}

// ─── Windows 实现 ───────────────────────────────────────────────────────────

#[cfg(windows)]
fn check_driver_windows() -> DriverStatus {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, HKEY_LOCAL_MACHINE, KEY_READ,
    };

    // 阶段 13：先尝试创建 context，成功则 Ready
    if let Some(_ctx) = interception::Interception::new() {
        log::info!("[driver] Interception context created successfully, driver ready");
        return DriverStatus::Ready;
    }

    // Context 创建失败，检查注册表判断是否已安装但需重启
    let keyboard_path = encode_wide("SYSTEM\\CurrentControlSet\\Services\\keyboard");
    let mouse_path = encode_wide("SYSTEM\\CurrentControlSet\\Services\\mouse");
    let service_paths: &[&[u16]] = &[&keyboard_path, &mouse_path];

    for path in service_paths {
        let mut hkey = std::ptr::null_mut();
        let status =
            unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, path.as_ptr(), 0, KEY_READ, &mut hkey) };
        if status == 0 {
            unsafe { RegCloseKey(hkey) };
            log::info!("[driver] registry service key found but context failed, need reboot");
            return DriverStatus::InstalledNeedReboot;
        }
    }

    log::info!("[driver] no registry service key found, driver not installed");
    DriverStatus::NotInstalled
}

#[cfg(windows)]
const INSTALLER_SHA256: &str = "e137863a79da797f08e7a137280ff2a123809044a888fd75ce9c973198915abe";
const MAX_INSTALLER_BYTES: u64 = 10 * 1024 * 1024;

use mimic_elevation_protocol::{
    claimed_file_name, pending_file_name, request_payload, Action, Invocation, EXIT_BAD_ARGUMENTS,
    EXIT_INTEGRITY_FAILURE, HELPER_FILE_NAME, NONCE_BYTES, VERSION,
};

const MAX_HELPER_BYTES: u64 = 20 * 1024 * 1024;

/// Launch the independently built helper through UAC and wait for its structured exit status.
#[cfg(windows)]
fn run_elevated_helper_windows(action: &str) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, WaitForSingleObject, INFINITE,
    };
    use windows_sys::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let action = Action::parse(action).ok_or_else(|| "helper_protocol_invalid".to_string())?;
    let paths = crate::paths::PortablePaths::current()?;
    paths.ensure_data_dirs()?;

    if matches!(action, Action::Install | Action::Uninstall) {
        verify_installer_integrity(&paths.driver_dir().join("install-interception.exe"))?;
    }

    let helper = paths.driver_dir().join(HELPER_FILE_NAME);
    verify_helper_integrity(&helper)?;

    let invocation = Invocation {
        action,
        caller_pid: std::process::id(),
        nonce: generate_helper_nonce()?,
    };
    let _request = create_helper_request(&paths, &invocation)?;

    let verb = encode_wide("runas");
    let file: Vec<u16> = helper
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let parameters = encode_wide(&format!(
        "{VERSION} {} {} {}",
        invocation.action.as_str(),
        invocation.caller_pid,
        invocation.nonce
    ));
    // SAFETY: zero is the documented initialization for SHELLEXECUTEINFOW before cbSize is set.
    let mut execute: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    execute.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    execute.fMask = SEE_MASK_NOCLOSEPROCESS;
    execute.lpVerb = verb.as_ptr();
    execute.lpFile = file.as_ptr();
    execute.lpParameters = parameters.as_ptr();
    execute.nShow = SW_HIDE;

    // SAFETY: all UTF-16 buffers are NUL-terminated and live through the call.
    if unsafe { ShellExecuteExW(&mut execute) } == 0 {
        // SAFETY: GetLastError is read immediately after the failed Win32 call.
        let code = unsafe { GetLastError() };
        return Err(if code == 1223 {
            "elevation_cancelled".to_string()
        } else {
            format!("helper_launch_failed:{code}")
        });
    }
    if execute.hProcess.is_null() {
        return Err("helper_process_handle_missing".to_string());
    }

    // SAFETY: hProcess is a valid process handle returned by ShellExecuteExW.
    let wait = unsafe { WaitForSingleObject(execute.hProcess, INFINITE) };
    if wait != WAIT_OBJECT_0 {
        // SAFETY: the process handle is owned by this function and closed exactly once.
        unsafe { CloseHandle(execute.hProcess) };
        return Err(format!("helper_wait_failed:{wait}"));
    }

    let mut exit_code = 0_u32;
    // SAFETY: hProcess remains open and exit_code is writable.
    let exit_ok = unsafe { GetExitCodeProcess(execute.hProcess, &mut exit_code) };
    // SAFETY: GetLastError is read immediately after a failed query.
    let exit_error = (exit_ok == 0).then(|| unsafe { GetLastError() });
    // SAFETY: the process handle is owned by this function and closed exactly once.
    unsafe { CloseHandle(execute.hProcess) };
    if let Some(code) = exit_error {
        return Err(format!("helper_exit_query_failed:{code}"));
    }

    match exit_code as i32 {
        0 => Ok(()),
        EXIT_INTEGRITY_FAILURE => Err("resource_integrity_failed".to_string()),
        EXIT_BAD_ARGUMENTS => Err("helper_protocol_invalid".to_string()),
        code => Err(format!("elevated_helper_failed:{code}")),
    }
}

#[cfg(windows)]
fn generate_helper_nonce() -> Result<String, String> {
    use windows_sys::Win32::Security::Cryptography::{
        BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
    };

    let mut bytes = [0_u8; NONCE_BYTES];
    // SAFETY: a null algorithm handle is required with SYSTEM_PREFERRED_RNG and the buffer is valid.
    let status = unsafe {
        BCryptGenRandom(
            std::ptr::null_mut(),
            bytes.as_mut_ptr(),
            bytes.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status < 0 {
        return Err(format!("helper_nonce_generation_failed:{status}"));
    }
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(windows)]
fn helper_request_paths(
    paths: &crate::paths::PortablePaths,
    nonce: &str,
) -> Result<(std::path::PathBuf, std::path::PathBuf), String> {
    let pending = pending_file_name(nonce).map_err(|_| "helper_protocol_invalid".to_string())?;
    let claimed = claimed_file_name(nonce).map_err(|_| "helper_protocol_invalid".to_string())?;
    Ok((
        paths.temp_dir().join(pending),
        paths.temp_dir().join(claimed),
    ))
}

#[cfg(windows)]
struct HelperRequestGuard {
    pending: std::path::PathBuf,
    claimed: std::path::PathBuf,
}

#[cfg(windows)]
impl Drop for HelperRequestGuard {
    fn drop(&mut self) {
        for path in [&self.pending, &self.claimed] {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => log::warn!(
                    "[driver] failed to remove helper request {}: {error}",
                    path.display()
                ),
            }
        }
    }
}

#[cfg(windows)]
fn create_helper_request(
    paths: &crate::paths::PortablePaths,
    invocation: &Invocation,
) -> Result<HelperRequestGuard, String> {
    use std::io::Write;

    let (pending, claimed) = helper_request_paths(paths, &invocation.nonce)?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&pending)
        .map_err(|error| format!("helper_request_create_failed:{error}"))?;
    if let Err(error) = file
        .write_all(request_payload(invocation).as_bytes())
        .and_then(|()| file.sync_all())
    {
        drop(file);
        let _ = std::fs::remove_file(&pending);
        return Err(format!("helper_request_write_failed:{error}"));
    }
    Ok(HelperRequestGuard { pending, claimed })
}

#[cfg(windows)]
fn verify_helper_integrity(path: &std::path::Path) -> Result<(), String> {
    let expected = option_env!("MIMIC_HELPER_SHA256")
        .filter(|value| is_sha256(value))
        .ok_or_else(|| "helper_integrity_config_missing".to_string())?;
    crate::paths::ensure_regular_file_or_missing(path)
        .map_err(|_| "helper_integrity_failed".to_string())?;
    let metadata =
        std::fs::metadata(path).map_err(|_| "helper_executable_unavailable".to_string())?;
    if metadata.len() == 0 || metadata.len() > MAX_HELPER_BYTES {
        return Err("helper_integrity_failed".to_string());
    }

    verify_sha256(path, expected, "helper_integrity_failed").inspect_err(|_| {
        log::error!("[driver] elevated helper SHA-256 mismatch; elevation refused");
    })
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn sha256_file(path: &std::path::Path, error_code: &str) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|_| error_code.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|_| error_code.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn verify_sha256(path: &std::path::Path, expected: &str, error_code: &str) -> Result<(), String> {
    if !is_sha256(expected) || sha256_file(path, error_code)? != expected {
        return Err(error_code.to_string());
    }
    Ok(())
}

#[cfg(windows)]
fn encode_wide(value: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}
#[cfg(windows)]
fn verify_installer_integrity(path: &std::path::Path) -> Result<(), String> {
    crate::paths::ensure_regular_file_or_missing(path)
        .map_err(|_| "resource_integrity_failed".to_string())?;
    let metadata = std::fs::metadata(path).map_err(|_| "driver_installer_not_found".to_string())?;
    if !metadata.is_file() || metadata.len() > MAX_INSTALLER_BYTES {
        log::error!("[driver] installer integrity rejected: invalid file or size");
        return Err("resource_integrity_failed".to_string());
    }

    verify_sha256(path, INSTALLER_SHA256, "resource_integrity_failed").inspect_err(|_| {
        log::error!("[driver] installer SHA-256 mismatch; elevation refused");
    })
}
#[cfg(test)]
mod tests {
    use super::{is_sha256, sha256_file, verify_sha256};

    #[test]
    fn helper_hash_configuration_must_be_exact_sha256() {
        assert!(is_sha256(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        ));
        assert!(!is_sha256("missing"));
        assert!(!is_sha256(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdeg"
        ));
    }

    #[test]
    fn tampered_file_fails_pinned_hash_verification() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "mimic-hash-test-{}-{unique}.bin",
            std::process::id()
        ));
        std::fs::write(&path, b"trusted helper").unwrap();
        let expected = sha256_file(&path, "test").unwrap();
        assert!(verify_sha256(&path, &expected, "integrity").is_ok());

        std::fs::write(&path, b"tampered helper").unwrap();
        assert_eq!(
            verify_sha256(&path, &expected, "integrity"),
            Err("integrity".to_string())
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn embedded_helper_hash_is_valid_when_release_pipeline_supplies_it() {
        if let Some(value) = option_env!("MIMIC_HELPER_SHA256") {
            assert!(is_sha256(value));
        }
    }
}
