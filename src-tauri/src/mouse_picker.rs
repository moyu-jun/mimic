// 鼠标坐标拾取 — DESIGN 11.2 / TASKS 阶段 14
//
// 拾取机制（2026-06-10 第二次修订：复用热键监听 context）：
//   - 历史方案 A：WH_MOUSE_LL 用户态 hook —— 被独占全屏游戏绕过，失效。
//   - 历史方案 B：worker context 单独 wait/receive 鼠标 —— 同进程双 context
//     竞争同一设备事件分发，worker context 从未设过 filter，实际收不到鼠标事件。
//   - 当前方案 C：复用「热键监听线程」的 listener context。该 context 已被证明
//     能正常 receive（键盘热键工作正常）。listener 启动时同时设键盘 + 鼠标左键 filter，
//     平时鼠标左键透传（零影响），仅在 PickingMouse 状态下捕获坐标。
//     单 context 是 Interception 标准用法，避免多 context 未定义行为，全屏游戏同样有效。
//
// 流程：
//   - start_pick_mouse_position 命令：置 PickingMouse、记录 row_id、隐藏窗口（不开线程）。
//   - listener 线程在 wait 循环中收到鼠标左键按下，若处于 PickingMouse：
//     用 GetCursorPos 读屏幕坐标 → 透传点击 → 调 finish_pick 恢复窗口 + emit。
//
// 坐标说明：Interception 鼠标 stroke 的 x/y 是移动量而非屏幕坐标，故用 GetCursorPos
// 读取系统光标位置作为拾取结果（单显示器 / 标准 DPI）。

use crate::state::{RuntimeStatus, SharedState};
use tauri::{AppHandle, Emitter, Manager};

/// 拾取入口 — 置 PickingMouse + 记录 row_id + 隐藏窗口。
///
/// 实际坐标捕获由热键监听线程在 PickingMouse 状态下完成（见 finish_pick）。
pub fn start_pick_mouse_position(
    app: AppHandle,
    state: SharedState,
    row_id: String,
) -> Result<(), String> {
    log::info!("[mouse_picker] start picking for row {}", row_id);

    // 1. 状态置 PickingMouse + 记录目标行
    {
        let mut app_state = state
            .lock()
            .map_err(|e| format!("Failed to lock state: {}", e))?;
        app_state.runtime_status = RuntimeStatus::PickingMouse;
        app_state.pick_row_id = Some(row_id);
    }
    emit_status(&app, RuntimeStatus::PickingMouse);

    // 2. 隐藏主窗口（best-effort：拿不到窗口时记录但不中断拾取）
    if let Some(win) = app.get_webview_window("main") {
        if let Err(e) = win.hide() {
            log::warn!("[mouse_picker] failed to hide window: {}", e);
        }
    } else {
        log::warn!("[mouse_picker] main window not found, picking without hiding");
    }

    Ok(())
}

/// 拾取完成处理 — 由 listener 线程在捕获到左键坐标后调用。
///
/// 恢复窗口、状态回 ReadyMouse、清除 pick_row_id、发送 mouse_position_picked。
pub fn finish_pick(app: &AppHandle, state: &SharedState, x: i32, y: i32) {
    let row_id = {
        let mut app_state = match state.lock() {
            Ok(s) => s,
            Err(e) => {
                log::error!("[mouse_picker] finish_pick: failed to lock state: {}", e);
                return;
            }
        };
        app_state.runtime_status = RuntimeStatus::ReadyMouse;
        app_state.pick_row_id.take()
    };

    let row_id = match row_id {
        Some(id) => id,
        None => {
            log::warn!("[mouse_picker] finish_pick called but pick_row_id is None");
            restore_window_on_main(app);
            emit_status(app, RuntimeStatus::ReadyMouse);
            return;
        }
    };

    log::info!("[mouse_picker] picked ({}, {}) for row {}", x, y, row_id);
    restore_window_on_main(app);
    emit_status(app, RuntimeStatus::ReadyMouse);

    if let Err(e) = app.emit(
        "mouse_position_picked",
        serde_json::json!({ "rowId": row_id, "x": x, "y": y }),
    ) {
        log::error!("[mouse_picker] failed to emit mouse_position_picked: {}", e);
    }
}

/// 发送 runtime_status_changed 事件。
fn emit_status(app: &AppHandle, status: RuntimeStatus) {
    if let Err(e) = app.emit(
        "runtime_status_changed",
        serde_json::json!({ "status": status }),
    ) {
        log::error!(
            "[mouse_picker] failed to emit runtime_status_changed: {}",
            e
        );
    }
}

/// 在主线程恢复并聚焦主窗口（show + unminimize + set_focus）。
///
/// 窗口在主线程创建，从后台线程直接 show()/set_focus() 受 Windows 前台锁定限制
/// 不可靠，必须 marshal 回主线程执行。
fn restore_window_on_main(app: &AppHandle) {
    let app_clone = app.clone();
    let dispatched = app.run_on_main_thread(move || match app_clone.get_webview_window("main") {
        Some(win) => {
            if let Err(e) = win.unminimize() {
                log::warn!("[mouse_picker] unminimize failed: {}", e);
            }
            if let Err(e) = win.show() {
                log::error!("[mouse_picker] window show failed: {}", e);
            }
            if let Err(e) = win.set_focus() {
                log::warn!("[mouse_picker] set_focus failed: {}", e);
            }
            log::info!("[mouse_picker] window restored on main thread");
        }
        None => {
            log::error!("[mouse_picker] main window not found during restore");
        }
    });
    if let Err(e) = dispatched {
        log::error!("[mouse_picker] run_on_main_thread dispatch failed: {}", e);
    }
}
