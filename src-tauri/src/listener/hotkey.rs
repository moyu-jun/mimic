//! 全局热键路由。
//!
//! 监听线程只做匹配、去抖和输入处置，启动/停止生命周期统一交给 SimulationRunner。

use crate::runner::{
    CustomSequenceBuilder, KeyboardSequenceBuilder, MouseSequenceBuilder, SimulationRunner,
    StartOutcome,
};
use crate::state::{PageId, RuntimeStatus, SharedState};
use interception::{Interception, KeyState, ScanCode, Stroke};
use log::{error, info};
use std::collections::HashSet;
use tauri::{AppHandle, Emitter};

#[derive(Default)]
pub struct HotkeyDebouncer {
    consumed_down: HashSet<u16>,
}

impl HotkeyDebouncer {
    fn is_repeat(&self, scan_code: u16) -> bool {
        self.consumed_down.contains(&scan_code)
    }

    fn mark_consumed(&mut self, scan_code: u16) {
        self.consumed_down.insert(scan_code);
    }

    fn release(&mut self, scan_code: u16) {
        self.consumed_down.remove(&scan_code);
    }
}

/// 处理单个键盘 stroke。
///
/// 真正生效的首次 KeyDown 被消费；重复 KeyDown 被忽略，直到对应 KeyUp 清除去抖状态。
/// 状态不匹配或启动被忽略时继续透传。
pub fn handle_keyboard_stroke(
    app: &AppHandle,
    state: &SharedState,
    interception: &Interception,
    device: i32,
    stroke: &Stroke,
    debouncer: &mut HotkeyDebouncer,
) {
    let (code, key_state) = match stroke {
        Stroke::Keyboard {
            code,
            state: key_state,
            ..
        } => (*code as u16, key_state),
        _ => {
            pass_through(interception, device, stroke);
            return;
        }
    };

    if key_state.contains(KeyState::UP) {
        debouncer.release(code);
        pass_through(interception, device, stroke);
        return;
    }

    if code == ScanCode::Esc as u16 {
        let picking = state
            .lock()
            .map(|app_state| app_state.runtime_status() == RuntimeStatus::PickingMouse)
            .unwrap_or(false);
        if picking {
            crate::mouse_picker::cancel_pick(app, state, "user");
            return;
        }
    }

    let (start_scan_code, stop_scan_code, current_page, runtime_status, active_custom_id) = {
        let app_state = match state.lock() {
            Ok(state) => state,
            Err(error) => {
                error!("[listener] failed to lock state: {}", error);
                pass_through(interception, device, stroke);
                return;
            }
        };
        (
            app_state.config.hotkeys.start.scan_code,
            app_state.config.hotkeys.stop.scan_code,
            app_state.navigation,
            app_state.runtime_status(),
            app_state.active_custom_sequence_id.clone(),
        )
    };

    let is_start_key = code == start_scan_code;
    let is_stop_key = code == stop_scan_code;
    if !is_start_key && !is_stop_key {
        pass_through(interception, device, stroke);
        return;
    }

    if debouncer.is_repeat(code) {
        info!("[listener] ignored repeated KeyDown for hotkey {}", code);
        return;
    }

    if !matches!(
        current_page,
        PageId::Keyboard | PageId::Mouse | PageId::Custom
    ) {
        pass_through(interception, device, stroke);
        return;
    }

    let consumed = match runtime_status {
        RuntimeStatus::ReadyKeyboard | RuntimeStatus::ReadyMouse | RuntimeStatus::ReadyCustom
            if is_start_key =>
        {
            handle_start_hotkey(app, state, &runtime_status, active_custom_id.as_deref())
        }
        RuntimeStatus::RunningKeyboard
        | RuntimeStatus::RunningMouse
        | RuntimeStatus::RunningCustom
            if is_stop_key =>
        {
            match SimulationRunner::stop(app, state) {
                Ok(crate::runtime::StopOutcome::Stopped) => true,
                Ok(crate::runtime::StopOutcome::AlreadyIdle) => false,
                Err(error) => {
                    // 停止请求已经进入 Runtime；释放失败会进入 Error，仍应消费本次停止热键。
                    error!("[listener] stop failed: {}", error);
                    true
                }
            }
        }
        _ => false,
    };

    if consumed {
        debouncer.mark_consumed(code);
    } else {
        pass_through(interception, device, stroke);
    }
}

fn handle_start_hotkey(
    app: &AppHandle,
    state: &SharedState,
    runtime_status: &RuntimeStatus,
    active_custom_id: Option<&str>,
) -> bool {
    let result = match runtime_status {
        RuntimeStatus::ReadyKeyboard => {
            SimulationRunner::start(app, state, &KeyboardSequenceBuilder)
        }
        RuntimeStatus::ReadyMouse => SimulationRunner::start(app, state, &MouseSequenceBuilder),
        RuntimeStatus::ReadyCustom => match active_custom_id {
            Some(id) => SimulationRunner::start(
                app,
                state,
                &CustomSequenceBuilder {
                    sequence_id: id.to_string(),
                },
            ),
            None => return false,
        },
        _ => return false,
    };

    match result {
        Ok(StartOutcome::Started) => true,
        Ok(StartOutcome::NoExecutableActions) => false,
        Err(error) => {
            error!("[listener] start failed: {}", error);
            let _ = app.emit(
                "simulation_start_failed",
                serde_json::json!({ "error": error }),
            );
            false
        }
    }
}

fn pass_through(interception: &Interception, device: i32, stroke: &Stroke) {
    let sent = interception.send(device, &[*stroke]);
    if sent != 1 {
        error!("[listener] keyboard pass-through failed: sent={sent}");
    }
}

pub fn keyboard_stroke_buffer() -> [Stroke; 16] {
    [Stroke::Keyboard {
        code: ScanCode::Esc,
        state: KeyState::empty(),
        information: 0,
    }; 16]
}

#[cfg(test)]
mod tests {
    use super::HotkeyDebouncer;

    #[test]
    fn consumed_key_repeats_until_key_up() {
        let mut debouncer = HotkeyDebouncer::default();
        assert!(!debouncer.is_repeat(88));

        debouncer.mark_consumed(88);
        assert!(debouncer.is_repeat(88));

        debouncer.release(88);
        assert!(!debouncer.is_repeat(88));
    }

    #[test]
    fn releasing_other_key_does_not_clear_consumed_key() {
        let mut debouncer = HotkeyDebouncer::default();
        debouncer.mark_consumed(88);
        debouncer.release(87);
        assert!(debouncer.is_repeat(88));
    }
}
