// 鼠标坐标拾取命令 — ARCHITECTURE v3.0 阶段 A

use crate::mouse_picker;
use crate::state::{RuntimeStatus, SharedState};

/// 鼠标坐标拾取 — DESIGN 11.2 / 阶段 14（2026-06-10 改用 listener 监听）
///
/// 仅可从 ReadyMouse 状态进入；运行 / 拾取中直接拒绝（运行态守卫，DESIGN 6.1）。
/// 进入后切到 PickingMouse、记录 row_id、隐藏窗口；实际坐标捕获由热键监听线程
/// （已同时监听键盘+鼠标左键）在 PickingMouse 状态下完成。
#[tauri::command]
pub fn start_pick_mouse_position(
    row_id: String,
    state: tauri::State<SharedState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    // 运行态守卫 — DESIGN 6.1：Running* / PickingMouse 时拒绝
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

    mouse_picker::start_pick_mouse_position(app, state.inner().clone(), row_id)
}
