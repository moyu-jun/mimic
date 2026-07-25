// 配置相关命令 — ARCHITECTURE v3.0 阶段 A

use crate::config::{self, AppConfig};
use crate::state::{RuntimeStatus, SharedState};

/// 加载配置命令 — 返回内存中的当前配置
#[tauri::command]
pub fn load_config(state: tauri::State<SharedState>) -> Result<AppConfig, String> {
    let app_state = state
        .inner()
        .lock()
        .map_err(|e| format!("Failed to lock state: {}", e))?;
    Ok(app_state.config.clone())
}

/// 持久化配置命令 — ARCHITECTURE v3.0 重构
///
/// 统一接口：保存到磁盘 + 更新内存 + 应用热键变更
/// 包含运行态守卫（Running*/PickingMouse/Recording 时拒绝）
#[tauri::command]
pub fn persist_config(config: AppConfig, state: tauri::State<SharedState>) -> Result<(), String> {
    // 运行态守卫
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

    // 先持久化，失败时内存状态不变
    config::save(&config).map_err(|e| {
        log::error!("[persist_config] persist failed: {}", e);
        e
    })?;

    // 写盘成功后才更新内存
    let mut app_state = state
        .inner()
        .lock()
        .map_err(|e| format!("Failed to lock state: {}", e))?;
    app_state.config = config;

    Ok(())
}

/// 读取启动时配置写盘失败的警告 — 阶段 9
#[tauri::command]
pub fn get_init_warning(state: tauri::State<SharedState>) -> Option<String> {
    state.inner().lock().ok()?.config_warning.clone()
}
