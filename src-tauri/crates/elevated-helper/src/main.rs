#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(windows)]
mod windows_helper {
    use mimic_elevation_protocol::{
        claimed_file_name, parse_helper_args, pending_file_name, request_payload, Action,
        Invocation, EXIT_BAD_ARGUMENTS, EXIT_INTEGRITY_FAILURE, EXIT_OPERATION_FAILURE,
        HELPER_FILE_NAME, MAX_REQUEST_BYTES,
    };
    use sha2::{Digest, Sha256};
    use std::io::Read;
    use std::path::{Path, PathBuf};

    const INSTALLER_SHA256: &str =
        "e137863a79da797f08e7a137280ff2a123809044a888fd75ce9c973198915abe";
    const MAX_INSTALLER_BYTES: u64 = 10 * 1024 * 1024;

    pub fn run() -> i32 {
        let arguments: Vec<std::ffi::OsString> = std::env::args_os().collect();
        let invocation = match parse_helper_args(&arguments) {
            Ok(invocation) => invocation,
            Err(_) => return EXIT_BAD_ARGUMENTS,
        };
        if !is_process_elevated().unwrap_or(false) {
            return EXIT_OPERATION_FAILURE;
        }

        let root = match application_root() {
            Ok(root) => root,
            Err(_) => return EXIT_BAD_ARGUMENTS,
        };
        if verify_caller(&root, invocation.caller_pid).is_err()
            || consume_request(&root, &invocation).is_err()
        {
            return EXIT_BAD_ARGUMENTS;
        }

        let result = match invocation.action {
            Action::Install => run_installer(&root, "/install"),
            Action::Uninstall => run_installer(&root, "/uninstall"),
            Action::Reboot => reboot_system(),
        };
        match result {
            Ok(()) => 0,
            Err(error) if error == "resource_integrity_failed" => EXIT_INTEGRITY_FAILURE,
            Err(_) => EXIT_OPERATION_FAILURE,
        }
    }

    fn application_root() -> Result<PathBuf, String> {
        let executable = std::env::current_exe()
            .map_err(|_| "helper_executable_unavailable".to_string())?
            .canonicalize()
            .map_err(|_| "helper_executable_unavailable".to_string())?;
        let driver_dir = executable
            .parent()
            .ok_or_else(|| "helper_layout_invalid".to_string())?;
        let root = driver_dir
            .parent()
            .ok_or_else(|| "helper_layout_invalid".to_string())?;

        if !executable.file_name().is_some_and(|name| {
            name.to_string_lossy()
                .eq_ignore_ascii_case(HELPER_FILE_NAME)
        }) || !driver_dir
            .file_name()
            .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("driver"))
        {
            return Err("helper_layout_invalid".to_string());
        }
        ensure_real_directory(root)?;
        ensure_real_directory(driver_dir)?;
        Ok(root.to_path_buf())
    }

    fn is_process_elevated() -> Result<bool, String> {
        use std::mem::{size_of, MaybeUninit};
        use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
        use windows_sys::Win32::Security::{
            GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
        };
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

        let mut token: HANDLE = std::ptr::null_mut();
        // SAFETY: GetCurrentProcess returns a valid pseudo-handle and token points to writable storage.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err("helper_token_unavailable".to_string());
        }

        let mut elevation = MaybeUninit::<TOKEN_ELEVATION>::zeroed();
        let mut returned = 0_u32;
        // SAFETY: token is open, the output buffer has the declared size, and returned is writable.
        let query_ok = unsafe {
            GetTokenInformation(
                token,
                TokenElevation,
                elevation.as_mut_ptr().cast(),
                size_of::<TOKEN_ELEVATION>() as u32,
                &mut returned,
            )
        };
        // SAFETY: token was returned by OpenProcessToken and is closed exactly once.
        let close_ok = unsafe { CloseHandle(token) };
        if query_ok == 0 || close_ok == 0 || returned < size_of::<TOKEN_ELEVATION>() as u32 {
            return Err("helper_token_unavailable".to_string());
        }
        // SAFETY: GetTokenInformation succeeded and initialized the complete TOKEN_ELEVATION value.
        Ok(unsafe { elevation.assume_init() }.TokenIsElevated != 0)
    }

    fn verify_caller(root: &Path, caller_pid: u32) -> Result<(), String> {
        use std::os::windows::ffi::OsStringExt;
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
        };

        // SAFETY: caller_pid is a validated nonzero PID and no raw pointers are passed.
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, caller_pid) };
        if process.is_null() {
            return Err("helper_caller_unavailable".to_string());
        }
        let mut image = [0_u16; 32_768];
        let mut length = image.len() as u32;
        // SAFETY: process is open and image/length describe a writable buffer.
        let query_ok =
            unsafe { QueryFullProcessImageNameW(process, 0, image.as_mut_ptr(), &mut length) };
        // SAFETY: process was returned by OpenProcess and is closed exactly once.
        let close_ok = unsafe { CloseHandle(process) };
        if query_ok == 0 || close_ok == 0 || length == 0 || length as usize > image.len() {
            return Err("helper_caller_unavailable".to_string());
        }

        let caller = PathBuf::from(std::ffi::OsString::from_wide(&image[..length as usize]));
        let expected = root.join("mimic.exe");
        if !caller
            .to_string_lossy()
            .eq_ignore_ascii_case(&expected.to_string_lossy())
        {
            return Err("helper_caller_mismatch".to_string());
        }
        ensure_regular_file(&caller)?;
        Ok(())
    }

    fn consume_request(root: &Path, invocation: &Invocation) -> Result<(), String> {
        let data_dir = root.join("data");
        let temp_dir = data_dir.join("temp");
        ensure_real_directory(&data_dir)?;
        ensure_real_directory(&temp_dir)?;

        let pending = temp_dir.join(
            pending_file_name(&invocation.nonce)
                .map_err(|_| "helper_protocol_invalid".to_string())?,
        );
        let claimed = temp_dir.join(
            claimed_file_name(&invocation.nonce)
                .map_err(|_| "helper_protocol_invalid".to_string())?,
        );
        std::fs::rename(&pending, &claimed)
            .map_err(|_| "helper_request_unavailable".to_string())?;

        let result = (|| {
            ensure_regular_file(&claimed)?;
            let metadata = std::fs::metadata(&claimed)
                .map_err(|_| "helper_request_unavailable".to_string())?;
            if metadata.len() > MAX_REQUEST_BYTES {
                return Err("helper_protocol_invalid".to_string());
            }
            let payload = std::fs::read_to_string(&claimed)
                .map_err(|_| "helper_request_unavailable".to_string())?;
            if payload != request_payload(invocation) {
                return Err("helper_protocol_invalid".to_string());
            }
            Ok(())
        })();
        let removed =
            std::fs::remove_file(&claimed).map_err(|_| "helper_request_consume_failed".to_string());
        result.and(removed)
    }

    fn run_installer(root: &Path, action: &str) -> Result<(), String> {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

        if !matches!(action, "/install" | "/uninstall") {
            return Err("helper_protocol_invalid".to_string());
        }
        let installer = root.join("driver").join("install-interception.exe");
        verify_installer(&installer)?;

        let status = std::process::Command::new(installer)
            .arg(action)
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map_err(|_| "installer_execute_failed".to_string())?;
        status
            .success()
            .then_some(())
            .ok_or_else(|| "installer_exit_failed".to_string())
    }

    fn verify_installer(path: &Path) -> Result<(), String> {
        ensure_regular_file(path).map_err(|_| "resource_integrity_failed".to_string())?;
        let metadata =
            std::fs::metadata(path).map_err(|_| "resource_integrity_failed".to_string())?;
        if metadata.len() == 0 || metadata.len() > MAX_INSTALLER_BYTES {
            return Err("resource_integrity_failed".to_string());
        }
        let actual = sha256_file(path)?;
        (actual == INSTALLER_SHA256)
            .then_some(())
            .ok_or_else(|| "resource_integrity_failed".to_string())
    }

    fn sha256_file(path: &Path) -> Result<String, String> {
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
        Ok(format!("{:x}", hasher.finalize()))
    }

    fn reboot_system() -> Result<(), String> {
        use std::os::windows::ffi::OsStringExt;
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
        use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

        let mut system_directory = [0_u16; 32_768];
        // SAFETY: the buffer is writable and its capacity is passed accurately.
        let length = unsafe {
            GetSystemDirectoryW(system_directory.as_mut_ptr(), system_directory.len() as u32)
        } as usize;
        if length == 0 || length >= system_directory.len() {
            return Err("system_directory_unavailable".to_string());
        }
        let shutdown = PathBuf::from(std::ffi::OsString::from_wide(&system_directory[..length]))
            .join("shutdown.exe");
        ensure_regular_file(&shutdown)?;

        let status = std::process::Command::new(shutdown)
            .args(["/r", "/t", "0"])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map_err(|_| "reboot_execute_failed".to_string())?;
        status
            .success()
            .then_some(())
            .ok_or_else(|| "reboot_exit_failed".to_string())
    }

    fn path_is_reparse(metadata: &std::fs::Metadata) -> bool {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }

    fn ensure_real_directory(path: &Path) -> Result<(), String> {
        let metadata =
            std::fs::symlink_metadata(path).map_err(|_| "helper_path_invalid".to_string())?;
        if !metadata.is_dir() || path_is_reparse(&metadata) {
            return Err("helper_path_invalid".to_string());
        }
        Ok(())
    }

    fn ensure_regular_file(path: &Path) -> Result<(), String> {
        let metadata =
            std::fs::symlink_metadata(path).map_err(|_| "helper_path_invalid".to_string())?;
        if !metadata.is_file() || path_is_reparse(&metadata) {
            return Err("helper_path_invalid".to_string());
        }
        Ok(())
    }
}

fn main() {
    #[cfg(windows)]
    std::process::exit(windows_helper::run());

    #[cfg(not(windows))]
    std::process::exit(mimic_elevation_protocol::EXIT_OPERATION_FAILURE);
}
