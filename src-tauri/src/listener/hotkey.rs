// 热键匹配 + 状态机门控 — ARCHITECTURE v3.0 阶段 C
//
// 由 hotkeys_interception.rs 拆分而来：处理单个键盘 stroke，
// 命中启动/停止热键时按 current_page 选 builder 调 SimulationRunner，否则透传。

use crate::runner::{KeyboardSequenceBuilder, MouseSequenceBuilder, SimulationRunner};
use crate::state::{RuntimeStatus, SharedState};
use interception::{Interception, KeyState, ScanCode, Stroke};
use log::{error, info};
use tauri::AppHandle;

/// 处理单个键盘 stroke — 热键匹配 + 状态机门控。
///
/// 命中启动/停止热键则调用 SimulationRunner 并阻断事件；其余情况透传到系统。
pub fn handle_keyboard_stroke(
    app: &AppHandle,
    state: &SharedState,
    interception: &Interception,
    device: i32,
    stroke: &Stroke,
) {
    let (code, key_state) = match stroke {
        Stroke::Keyboard {
            code, state: ks, ..
        } => (code, ks),
        _ => {
            // 非键盘事件（理论上不会到达这里），透传
            interception.send(device, &[*stroke]);
            return;
        }
    };

    // 仅处理按下事件（忽略抬起）— DESIGN 8.3
    if key_state.contains(KeyState::UP) {
        interception.send(device, &[*stroke]);
        return;
    }

    // 读取当前热键配置
    let (start_scan_code, stop_scan_code, current_page, runtime_status) = {
        let app_state = match state.lock() {
            Ok(s) => s,
            Err(e) => {
                error!("[listener] failed to lock state: {}", e);
                interception.send(device, &[*stroke]);
                return;
            }
        };
        (
            app_state.config.hotkeys.start.scan_code,
            app_state.config.hotkeys.stop.scan_code,
            app_state.current_page.clone(),
            app_state.runtime_status.clone(),
        )
    };

    // 统一热键匹配逻辑 — 支持启动和停止键相同的 toggle 场景
    let is_start_key = *code as u16 == start_scan_code;
    let is_stop_key = *code as u16 == stop_scan_code;

    if !is_start_key && !is_stop_key {
        // 非热键事件，透传到系统
        interception.send(device, &[*stroke]);
        return;
    }

    // 诊断日志 — 热键匹配成功时记录上下文
    info!(
        "[listener] hotkey matched: code={}, start_code={}, stop_code={}, page={}, status={:?}",
        *code as u16, start_scan_code, stop_scan_code, current_page, runtime_status
    );

    // 页面过滤 — REQUIREMENTS 3.6
    if current_page.as_str() != "keyboard" && current_page.as_str() != "mouse" {
        info!(
            "[listener] hotkey blocked by page filter: current_page={}",
            current_page
        );
        interception.send(device, &[*stroke]);
        return;
    }

    // 状态机门控：根据当前状态决定行为（支持 toggle）
    match runtime_status {
        RuntimeStatus::Idle | RuntimeStatus::ReadyKeyboard | RuntimeStatus::ReadyMouse
            if is_start_key =>
        {
            // Idle/Ready* 状态下按启动键 → 启动模拟
            info!("[listener] state machine: START branch matched");
            handle_start_hotkey(app, state, current_page.as_str());
            // 阻断热键事件，不透传到系统
        }
        RuntimeStatus::RunningKeyboard | RuntimeStatus::RunningMouse if is_stop_key => {
            // Running 状态下按停止键 → 停止模拟
            info!("[listener] state machine: STOP branch matched");
            SimulationRunner::stop(app, state);
            // 阻断热键事件
        }
        RuntimeStatus::Idle if is_stop_key => {
            // Idle 状态下按停止键 → 阻断（不透传）
            info!("[listener] state machine: IDLE+STOP branch, ignoring");
        }
        _ => {
            // 状态不匹配（如 Running 时按启动键），透传
            info!(
                "[listener] state machine: FALLTHROUGH branch, passing through. is_start_key={}, is_stop_key={}",
                is_start_key, is_stop_key
            );
            interception.send(device, &[*stroke]);
        }
    }
}

/// 启动热键回调 — 按当前页选 SequenceBuilder，交由 SimulationRunner 统一编排。
fn handle_start_hotkey(app: &AppHandle, state: &SharedState, current_page: &str) {
    info!(
        "[listener] handle_start_hotkey called: current_page={}",
        current_page
    );
    if current_page == "keyboard" {
        SimulationRunner::start(app, state, &KeyboardSequenceBuilder);
    } else {
        SimulationRunner::start(app, state, &MouseSequenceBuilder);
    }
}

/// 分配一个键盘 stroke 缓冲区（供监听循环 receive 使用）。
pub fn keyboard_stroke_buffer() -> [Stroke; 16] {
    [Stroke::Keyboard {
        code: ScanCode::Esc,
        state: KeyState::empty(),
        information: 0,
    }; 16]
}
