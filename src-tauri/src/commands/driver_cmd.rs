// 驱动与管理员权限相关命令 — ARCHITECTURE v3.0 阶段 A

use crate::admin;
use crate::driver;
use crate::state::{RuntimeStatus, SharedState};

/// 当前进程是否以管理员身份运行 — DESIGN 14.1 / 阶段 10
///
/// 失败一律视为非管理员（admin 模块内部已记录 warn 日志）。
// ADMIN_POLICY: Runtime detection only — no requireAdministrator manifest entry.
#[tauri::command]
pub fn get_admin_status() -> bool {
    admin::is_admin()
}

/// 检测 Interception 驱动状态 — DESIGN 12.2 / 阶段 11
#[tauri::command]
pub fn check_driver_status(state: tauri::State<SharedState>) -> Result<String, String> {
    let app_state = state
        .inner()
        .lock()
        .map_err(|e| format!("Failed to lock state: {}", e))?;
    Ok(serde_json::to_string(&app_state.driver_status)
        .unwrap_or_else(|_| "\"NotInstalled\"".to_string()))
}

/// 安装 Interception 驱动 — DESIGN 12.3 / 阶段 11
///
/// 通过 ShellExecuteW("runas") 以管理员身份调用外置安装器。
/// 成功调度后返回 Ok，调用方应重新调 check_driver_status 刷新。
///
/// 前置条件：必须以管理员权限运行（否则返回 Err 提示用户重启）。
#[tauri::command]
pub fn install_interception_driver(state: tauri::State<SharedState>) -> Result<(), String> {
    // 权限守卫 — 驱动安装必须管理员权限（阶段 11 遗漏修复）
    if !admin::is_admin() {
        log::warn!("[install_driver] rejected: not running as admin");
        return Err("permission_denied".to_string());
    }

    // 运行态守卫 — DESIGN 6.1
    {
        let app_state = state
            .inner()
            .lock()
            .map_err(|e| format!("Failed to lock state: {}", e))?;
        match app_state.runtime_status {
            RuntimeStatus::RunningKeyboard
            | RuntimeStatus::RunningMouse
            | RuntimeStatus::PickingMouse
            | RuntimeStatus::Recording => {
                return Err("busy: simulation running".to_string());
            }
            _ => {}
        }
    }

    driver::install_driver()?;

    // 安装后重新检测并更新 state
    let new_status = driver::check_interception_driver();
    if let Ok(mut app_state) = state.inner().lock() {
        app_state.driver_status = new_status;
    }

    Ok(())
}

/// 卸载 Interception 驱动 — 与 install_interception_driver 对称
///
/// 以管理员身份调用安装器 `/uninstall`。卸载后通常需重启系统才彻底移除。
/// 前置条件：必须以管理员权限运行（否则返回 `permission_denied`）。
#[tauri::command]
pub fn uninstall_interception_driver(state: tauri::State<SharedState>) -> Result<(), String> {
    // 权限守卫 — 与安装一致，非管理员拒绝
    if !admin::is_admin() {
        log::warn!("[uninstall_driver] rejected: not running as admin");
        return Err("permission_denied".to_string());
    }

    // 运行态守卫 — 模拟运行中不允许卸载
    {
        let app_state = state
            .inner()
            .lock()
            .map_err(|e| format!("Failed to lock state: {}", e))?;
        match app_state.runtime_status {
            RuntimeStatus::RunningKeyboard
            | RuntimeStatus::RunningMouse
            | RuntimeStatus::PickingMouse
            | RuntimeStatus::Recording => {
                return Err("busy: simulation running".to_string());
            }
            _ => {}
        }
    }

    driver::uninstall_driver()?;

    // 卸载后重新检测并更新 state
    let new_status = driver::check_interception_driver();
    if let Ok(mut app_state) = state.inner().lock() {
        app_state.driver_status = new_status;
    }

    Ok(())
}

/// 重启系统 — 驱动安装后需重启加载（阶段 11 优化）
///
/// 需管理员权限；非管理员返回 `permission_denied`。
#[tauri::command]
pub fn reboot_system() -> Result<(), String> {
    if !admin::is_admin() {
        log::warn!("[reboot] rejected: not running as admin");
        return Err("permission_denied".to_string());
    }
    log::info!("[reboot] user requested system reboot");
    driver::reboot_system()
}

/// 以管理员身份重启自身 — DESIGN 14.1 / 阶段 10
///
/// 触发 UAC 提示；用户取消或 ShellExecuteW 失败时返回 Err 字符串。
/// 成功调度后由前端立即调用 `app.exit()` 关闭当前进程，避免双开。
// ADMIN_POLICY: 通过 ShellExecuteW("runas") 触发用户级 UAC,无静默提权。
#[tauri::command]
pub fn request_admin_restart(app: tauri::AppHandle) -> Result<(), String> {
    log::info!("[admin] user requested elevation restart");
    admin::restart_as_admin().map_err(|e| {
        log::error!("[admin] restart_as_admin failed: {}", e);
        e
    })?;
    // 调度成功后给前端 200ms 让其完成 UI 反馈，再退出当前进程
    let app_handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(200));
        app_handle.exit(0);
    });
    Ok(())
}
