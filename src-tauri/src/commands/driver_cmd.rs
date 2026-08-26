//! 驱动维护命令。主程序始终保持普通权限，具体安装器/系统重启进程按操作请求 UAC。

use crate::driver;
use crate::error::{CommandError, CommandResult};
use crate::state::{Activity, ActivityLease, SharedState};

#[tauri::command]
pub fn check_driver_status(state: tauri::State<SharedState>) -> CommandResult<String> {
    let app_state = state
        .inner()
        .lock()
        .map_err(|error| CommandError::from(format!("Failed to lock state: {error}")))?;
    serde_json::to_string(&app_state.driver_status)
        .map_err(|error| CommandError::from(error.to_string()))
}

#[tauri::command]
pub fn install_interception_driver(state: tauri::State<SharedState>) -> CommandResult<()> {
    run_driver_maintenance(state.inner(), driver::install_driver).map_err(CommandError::from)
}

#[tauri::command]
pub fn uninstall_interception_driver(state: tauri::State<SharedState>) -> CommandResult<()> {
    run_driver_maintenance(state.inner(), driver::uninstall_driver).map_err(CommandError::from)
}

#[tauri::command]
pub fn reboot_system() -> CommandResult<()> {
    log::info!("[driver] user requested elevated system reboot");
    driver::reboot_system().map_err(CommandError::from)
}

fn run_driver_maintenance(
    state: &SharedState,
    operation: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let _lease = ActivityLease::acquire(state, Activity::DriverMaintenance)?;
    let result = operation();
    let detected = result
        .as_ref()
        .ok()
        .map(|_| driver::check_interception_driver());
    if let Ok(mut app_state) = state.lock() {
        if let Some(status) = detected {
            app_state.driver_status = status;
        }
    }
    result
}
