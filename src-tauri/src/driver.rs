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

const HELPER_SWITCH: &str = "--mimic-elevated-helper";
const HELPER_PROTOCOL_VERSION: &str = "v1";
const HELPER_NONCE_BYTES: usize = 16;
const HELPER_NONCE_LENGTH: usize = HELPER_NONCE_BYTES * 2;
const MAX_HELPER_REQUEST_BYTES: u64 = 256;
const HELPER_BAD_ARGUMENTS: i32 = 64;
const HELPER_INTEGRITY_FAILURE: i32 = 65;
const HELPER_OPERATION_FAILURE: i32 = 70;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HelperAction {
    Install,
    Uninstall,
    Reboot,
}

impl HelperAction {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "install" => Some(Self::Install),
            "uninstall" => Some(Self::Uninstall),
            "reboot" => Some(Self::Reboot),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Uninstall => "uninstall",
            Self::Reboot => "reboot",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HelperInvocation {
    action: HelperAction,
    parent_pid: u32,
    nonce: String,
}

fn valid_helper_nonce(value: &str) -> bool {
    value.len() == HELPER_NONCE_LENGTH
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn parse_helper_invocation(
    arguments: &[std::ffi::OsString],
) -> Option<Result<HelperInvocation, ()>> {
    if arguments.get(1).and_then(|value| value.to_str()) != Some(HELPER_SWITCH) {
        return None;
    }
    if arguments.len() != 6
        || arguments.get(2).and_then(|value| value.to_str()) != Some(HELPER_PROTOCOL_VERSION)
    {
        return Some(Err(()));
    }
    let Some(action) = arguments[3].to_str().and_then(HelperAction::parse) else {
        return Some(Err(()));
    };
    let Some(parent_pid) = arguments[4]
        .to_str()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value != 0)
    else {
        return Some(Err(()));
    };
    let Some(nonce) = arguments[5]
        .to_str()
        .filter(|value| valid_helper_nonce(value))
    else {
        return Some(Err(()));
    };
    Some(Ok(HelperInvocation {
        action,
        parent_pid,
        nonce: nonce.to_string(),
    }))
}

/// Parse the restricted helper protocol before Tauri starts.
pub(crate) fn elevated_helper_exit_code() -> Option<i32> {
    #[cfg(windows)]
    {
        let arguments: Vec<std::ffi::OsString> = std::env::args_os().collect();
        let invocation = match parse_helper_invocation(&arguments)? {
            Ok(invocation) => invocation,
            Err(()) => return Some(HELPER_BAD_ARGUMENTS),
        };
        if !is_process_elevated().unwrap_or(false) {
            return Some(HELPER_OPERATION_FAILURE);
        }
        if verify_helper_parent(invocation.parent_pid).is_err()
            || consume_helper_request(&invocation).is_err()
        {
            return Some(HELPER_BAD_ARGUMENTS);
        }
        let result = match invocation.action {
            HelperAction::Install => run_installer_direct("/install"),
            HelperAction::Uninstall => run_installer_direct("/uninstall"),
            HelperAction::Reboot => reboot_system_direct(),
        };
        Some(match result {
            Ok(()) => 0,
            Err(error) if error.contains("resource_integrity_failed") => HELPER_INTEGRITY_FAILURE,
            Err(_) => HELPER_OPERATION_FAILURE,
        })
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Elevate the current signed executable in restricted helper mode and wait for completion.
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

    let action =
        HelperAction::parse(action).ok_or_else(|| "helper_protocol_invalid".to_string())?;
    if matches!(action, HelperAction::Install | HelperAction::Uninstall) {
        let installer = crate::paths::PortablePaths::current()?
            .driver_dir()
            .join("install-interception.exe");
        verify_installer_integrity(&installer)?;
    }

    let executable = std::env::current_exe()
        .map_err(|error| format!("helper_executable_unavailable:{error}"))?;
    if !executable.is_file() {
        return Err("helper_executable_unavailable".to_string());
    }

    let invocation = HelperInvocation {
        action,
        parent_pid: std::process::id(),
        nonce: generate_helper_nonce()?,
    };
    let _request = create_helper_request(&invocation)?;

    let verb = encode_wide("runas");
    let file: Vec<u16> = executable
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let parameters = encode_wide(&format!(
        "{HELPER_SWITCH} {HELPER_PROTOCOL_VERSION} {} {} {}",
        invocation.action.as_str(),
        invocation.parent_pid,
        invocation.nonce
    ));
    let mut execute: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    execute.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    execute.fMask = SEE_MASK_NOCLOSEPROCESS;
    execute.lpVerb = verb.as_ptr();
    execute.lpFile = file.as_ptr();
    execute.lpParameters = parameters.as_ptr();
    execute.nShow = SW_HIDE;

    if unsafe { ShellExecuteExW(&mut execute) } == 0 {
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

    let wait = unsafe { WaitForSingleObject(execute.hProcess, INFINITE) };
    if wait != WAIT_OBJECT_0 {
        unsafe { CloseHandle(execute.hProcess) };
        return Err(format!("helper_wait_failed:{wait}"));
    }

    let mut exit_code = 0_u32;
    let exit_ok = unsafe { GetExitCodeProcess(execute.hProcess, &mut exit_code) };
    let exit_error = (exit_ok == 0).then(|| unsafe { GetLastError() });
    unsafe { CloseHandle(execute.hProcess) };
    if let Some(code) = exit_error {
        return Err(format!("helper_exit_query_failed:{code}"));
    }
    match exit_code as i32 {
        0 => Ok(()),
        HELPER_INTEGRITY_FAILURE => Err("resource_integrity_failed".to_string()),
        HELPER_BAD_ARGUMENTS => Err("helper_protocol_invalid".to_string()),
        code => Err(format!("elevated_helper_failed:{code}")),
    }
}

#[cfg(windows)]
fn generate_helper_nonce() -> Result<String, String> {
    use windows_sys::Win32::Security::Cryptography::{
        BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
    };

    let mut bytes = [0_u8; HELPER_NONCE_BYTES];
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

fn helper_request_payload(invocation: &HelperInvocation) -> String {
    format!(
        "{HELPER_PROTOCOL_VERSION}|{}|{}|{}\n",
        invocation.action.as_str(),
        invocation.parent_pid,
        invocation.nonce
    )
}

#[cfg(windows)]
fn helper_request_paths(nonce: &str) -> Result<(std::path::PathBuf, std::path::PathBuf), String> {
    if !valid_helper_nonce(nonce) {
        return Err("helper_protocol_invalid".to_string());
    }
    let paths = crate::paths::PortablePaths::current()?;
    paths.ensure_data_dirs()?;
    Ok((
        paths.temp_dir().join(format!("elevation-{nonce}.request")),
        paths.temp_dir().join(format!("elevation-{nonce}.claimed")),
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
fn create_helper_request(invocation: &HelperInvocation) -> Result<HelperRequestGuard, String> {
    use std::io::Write;

    let (pending, claimed) = helper_request_paths(&invocation.nonce)?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&pending)
        .map_err(|error| format!("helper_request_create_failed:{error}"))?;
    if let Err(error) = file
        .write_all(helper_request_payload(invocation).as_bytes())
        .and_then(|()| file.sync_all())
    {
        drop(file);
        let _ = std::fs::remove_file(&pending);
        return Err(format!("helper_request_write_failed:{error}"));
    }
    Ok(HelperRequestGuard { pending, claimed })
}

#[cfg(windows)]
fn consume_helper_request(invocation: &HelperInvocation) -> Result<(), String> {
    let (pending, claimed) = helper_request_paths(&invocation.nonce)?;
    std::fs::rename(&pending, &claimed).map_err(|_| "helper_request_unavailable".to_string())?;

    let result = (|| {
        crate::paths::ensure_regular_file_or_missing(&claimed)
            .map_err(|_| "helper_protocol_invalid".to_string())?;
        let metadata =
            std::fs::metadata(&claimed).map_err(|_| "helper_request_unavailable".to_string())?;
        if !metadata.is_file() || metadata.len() > MAX_HELPER_REQUEST_BYTES {
            return Err("helper_protocol_invalid".to_string());
        }
        let payload = std::fs::read_to_string(&claimed)
            .map_err(|_| "helper_request_unavailable".to_string())?;
        if payload != helper_request_payload(invocation) {
            return Err("helper_protocol_invalid".to_string());
        }
        Ok(())
    })();
    if let Err(error) = std::fs::remove_file(&claimed) {
        log::warn!(
            "[driver] failed to consume helper request {}: {error}",
            claimed.display()
        );
        return Err("helper_request_consume_failed".to_string());
    }
    result
}

#[cfg(windows)]
fn verify_helper_parent(parent_pid: u32) -> Result<(), String> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, parent_pid) };
    if process.is_null() {
        return Err("helper_parent_unavailable".to_string());
    }
    let mut image = [0_u16; 32_768];
    let mut length = image.len() as u32;
    let query_ok =
        unsafe { QueryFullProcessImageNameW(process, 0, image.as_mut_ptr(), &mut length) };
    unsafe { CloseHandle(process) };
    if query_ok == 0 || length == 0 || length as usize > image.len() {
        return Err("helper_parent_unavailable".to_string());
    }

    let parent = std::path::PathBuf::from(std::ffi::OsString::from_wide(&image[..length as usize]));
    let current =
        std::env::current_exe().map_err(|_| "helper_executable_unavailable".to_string())?;
    if !parent
        .to_string_lossy()
        .eq_ignore_ascii_case(&current.to_string_lossy())
    {
        return Err("helper_parent_mismatch".to_string());
    }
    Ok(())
}

/// The elevated branch revalidates the fixed installer and invokes it without a shell.
#[cfg(windows)]
fn run_installer_direct(action_param: &str) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

    if !matches!(action_param, "/install" | "/uninstall") {
        return Err("helper_protocol_invalid".to_string());
    }
    let installer = crate::paths::PortablePaths::current()?
        .driver_dir()
        .join("install-interception.exe");
    verify_installer_integrity(&installer)?;

    let status = std::process::Command::new(&installer)
        .arg(action_param)
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|error| format!("installer_execute_failed:{error}"))?;
    if !status.success() {
        return Err(format!(
            "installer_exit_failed:{}",
            status.code().unwrap_or(-1)
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn reboot_system_direct() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

    let shutdown = system_shutdown_path()?;
    let status = std::process::Command::new(shutdown)
        .args(["/r", "/t", "0"])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|error| format!("reboot_execute_failed:{error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "reboot_exit_failed:{}",
            status.code().unwrap_or(-1)
        ))
    }
}

#[cfg(windows)]
fn system_shutdown_path() -> Result<std::path::PathBuf, String> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;

    let mut system_directory = [0_u16; 32_768];
    let length = unsafe {
        GetSystemDirectoryW(system_directory.as_mut_ptr(), system_directory.len() as u32)
    } as usize;
    if length == 0 || length >= system_directory.len() {
        return Err("system_directory_unavailable".to_string());
    }
    Ok(
        std::path::PathBuf::from(std::ffi::OsString::from_wide(&system_directory[..length]))
            .join("shutdown.exe"),
    )
}
/// 将 Rust 字符串编码为 null 结尾的 UTF-16 宽字符序列
#[cfg(windows)]
fn encode_wide(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
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

    let mut file =
        std::fs::File::open(path).map_err(|_| "resource_integrity_failed".to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| "resource_integrity_failed".to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual = format!("{:x}", hasher.finalize());
    if actual != INSTALLER_SHA256 {
        log::error!("[driver] installer SHA-256 mismatch; elevation refused");
        return Err("resource_integrity_failed".to_string());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::{
        helper_request_payload, parse_helper_invocation, HelperAction, HelperInvocation,
        HELPER_PROTOCOL_VERSION, HELPER_SWITCH,
    };
    use std::ffi::OsString;

    fn arguments(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn helper_protocol_accepts_only_exact_actions() {
        let nonce = "0123456789abcdef0123456789abcdef";
        for (name, expected) in [
            ("install", HelperAction::Install),
            ("uninstall", HelperAction::Uninstall),
            ("reboot", HelperAction::Reboot),
        ] {
            assert_eq!(
                parse_helper_invocation(&arguments(&[
                    "mimic.exe",
                    HELPER_SWITCH,
                    HELPER_PROTOCOL_VERSION,
                    name,
                    "42",
                    nonce,
                ])),
                Some(Ok(HelperInvocation {
                    action: expected,
                    parent_pid: 42,
                    nonce: nonce.to_string(),
                }))
            );
        }
    }

    #[test]
    fn helper_protocol_rejects_unknown_or_extra_arguments() {
        assert_eq!(parse_helper_invocation(&arguments(&["mimic.exe"])), None);
        assert_eq!(
            parse_helper_invocation(&arguments(&[
                "mimic.exe",
                HELPER_SWITCH,
                HELPER_PROTOCOL_VERSION,
                "shell",
                "42",
                "0123456789abcdef0123456789abcdef",
            ])),
            Some(Err(()))
        );
        assert_eq!(
            parse_helper_invocation(&arguments(&[
                "mimic.exe",
                HELPER_SWITCH,
                HELPER_PROTOCOL_VERSION,
                "install",
                "42",
                "0123456789abcdef0123456789abcdef",
                "extra",
            ])),
            Some(Err(()))
        );
        for (pid, nonce) in [
            ("0", "0123456789abcdef0123456789abcdef"),
            ("not-a-pid", "0123456789abcdef0123456789abcdef"),
            ("42", "too-short"),
            ("42", "0123456789ABCDEF0123456789ABCDEF"),
        ] {
            assert_eq!(
                parse_helper_invocation(&arguments(&[
                    "mimic.exe",
                    HELPER_SWITCH,
                    HELPER_PROTOCOL_VERSION,
                    "install",
                    pid,
                    nonce,
                ])),
                Some(Err(()))
            );
        }
    }

    #[test]
    fn helper_request_payload_binds_every_protocol_field() {
        let invocation = HelperInvocation {
            action: HelperAction::Uninstall,
            parent_pid: 77,
            nonce: "0123456789abcdef0123456789abcdef".to_string(),
        };
        assert_eq!(
            helper_request_payload(&invocation),
            "v1|uninstall|77|0123456789abcdef0123456789abcdef\n"
        );
    }
}
