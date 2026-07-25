// 运行时状态与控制命令 — ARCHITECTURE v3.0 阶段 A

use crate::config;
use crate::hotkeys::HotkeyUpdateResult;
use crate::state::{RuntimeStatus, SharedState};
use tauri::Emitter;

/// 设置当前页面 — 阶段 12 / P2-3 修复
///
/// 后端记录当前页面，用于判断热键是否可触发（REQUIREMENTS 3.6）。
/// P2-3 修复: Idle 状态下切到 keyboard/mouse 页时自动切换到对应 Ready 状态。
#[tauri::command]
pub fn set_current_page(
    page: String,
    state: tauri::State<SharedState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
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

    let new_status = {
        let mut app_state = state
            .inner()
            .lock()
            .map_err(|e| format!("Failed to lock state: {}", e))?;
        app_state.current_page = page.clone();

        // P2-3: 非 Running*/PickingMouse 状态下根据页面切换到对应 Ready 状态
        // 修复: Ready 状态间也需要切换 (ReadyKeyboard ↔ ReadyMouse)
        match app_state.runtime_status {
            RuntimeStatus::Idle | RuntimeStatus::ReadyKeyboard | RuntimeStatus::ReadyMouse => {
                app_state.runtime_status = match page.as_str() {
                    "keyboard" => RuntimeStatus::ReadyKeyboard,
                    "mouse" => RuntimeStatus::ReadyMouse,
                    _ => RuntimeStatus::Idle,
                };
            }
            _ => {
                // Running*/PickingMouse/Error 状态不变
            }
        }

        log::info!(
            "[set_current_page] page={}, status={:?}",
            page,
            app_state.runtime_status
        );
        app_state.runtime_status.clone()
    };

    // 发送 runtime_status_changed 事件
    if let Err(e) = app.emit(
        "runtime_status_changed",
        serde_json::json!({ "status": new_status }),
    ) {
        log::error!("[set_current_page] failed to emit event: {}", e);
    }

    Ok(())
}

/// 更新热键配置 — 阶段 13 / DESIGN 6.2
///
/// 流程：对比变化 → 持久化 → 更新内存。
/// Interception 热键由后台监听线程统一处理，不需要注册/注销。
/// 返回结构化结果供前端分别提示持久化成功/失败。
#[tauri::command]
pub fn update_hotkeys(
    hotkeys: config::HotkeyConfig,
    state: tauri::State<SharedState>,
) -> Result<HotkeyUpdateResult, String> {
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

    crate::hotkeys::update_hotkeys(&state, hotkeys)
}

/// 停止模拟 — 阶段 12（仅切换状态）
///
/// 当前阶段仅将状态从 Running* 切回 Idle,不涉及真实 worker 停止（阶段 13 接入）。
#[tauri::command]
pub fn stop_simulation(
    state: tauri::State<SharedState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let new_status = {
        let mut app_state = state
            .inner()
            .lock()
            .map_err(|e| format!("Failed to lock state: {}", e))?;

        match app_state.runtime_status {
            RuntimeStatus::RunningKeyboard | RuntimeStatus::RunningMouse => {
                app_state.runtime_status = RuntimeStatus::Idle;
                RuntimeStatus::Idle
            }
            _ => {
                return Err("Not running".to_string());
            }
        }
    };

    // 发送 runtime_status_changed 事件
    if let Err(e) = app.emit(
        "runtime_status_changed",
        serde_json::json!({ "status": new_status }),
    ) {
        log::error!("[stop_simulation] failed to emit event: {}", e);
    }

    log::info!("[stop_simulation] simulation stopped");
    Ok(())
}

/// 获取当前运行状态 — 阶段 12
#[tauri::command]
pub fn get_runtime_status(state: tauri::State<SharedState>) -> Result<RuntimeStatus, String> {
    let app_state = state
        .inner()
        .lock()
        .map_err(|e| format!("Failed to lock state: {}", e))?;
    Ok(app_state.runtime_status.clone())
}
