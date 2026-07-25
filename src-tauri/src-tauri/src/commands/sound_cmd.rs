// 提示音录制与播放命令 — ARCHITECTURE v3.0 阶段 A

use crate::sound;
use crate::sound_recorder;
use crate::state::{RuntimeStatus, SharedState};

/// 开始录制提示音 — DESIGN 20.5 / 阶段 18
///
/// target: "start" -> 按键开启.wav, "stop" -> 按键关闭.wav。
/// 运行态守卫：Running* / PickingMouse / Recording 时拒绝。
#[tauri::command]
pub fn start_recording(
    target: String,
    state: tauri::State<SharedState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let handle = {
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
        app_state.recording.clone()
    };

    sound_recorder::start_recording(app, state.inner().clone(), handle, target)
}

/// 停止录制并保存 — 阶段 18
#[tauri::command]
pub fn stop_recording(state: tauri::State<SharedState>) -> Result<(), String> {
    let handle = {
        let app_state = state
            .inner()
            .lock()
            .map_err(|e| format!("Failed to lock state: {}", e))?;
        app_state.recording.clone()
    };
    sound_recorder::stop_recording(&handle)
}

/// 取消录制（不写文件）— 阶段 18
#[tauri::command]
pub fn cancel_recording(state: tauri::State<SharedState>) -> Result<(), String> {
    let handle = {
        let app_state = state
            .inner()
            .lock()
            .map_err(|e| format!("Failed to lock state: {}", e))?;
        app_state.recording.clone()
    };
    sound_recorder::cancel_recording(&handle)
}

/// 保存剪裁后音频 — 阶段 18 剪裁
///
/// 从内存缓冲读取全程 PCM，截取 [startMs, endMs) 写 WAV，清空缓冲。
#[tauri::command]
pub fn save_trimmed_audio(
    target: String,
    start_ms: u32,
    end_ms: u32,
    state: tauri::State<SharedState>,
) -> Result<(), String> {
    sound_recorder::save_trimmed_audio(state.inner().clone(), target, start_ms, end_ms)
}

/// 试听提示音 — 阶段 18
///
/// target: "start" -> 按键开启.wav, "stop" -> 按键关闭.wav。
/// 复用现有 sound 模块，文件缺失时仅记录日志，不报错。
#[tauri::command]
pub fn preview_sound(target: String) -> Result<(), String> {
    match target.as_str() {
        "start" => sound::play_start(),
        "stop" => sound::play_stop(),
        _ => return Err("invalid target".to_string()),
    }
    Ok(())
}

/// 查询提示音文件是否存在 — 阶段 18
///
/// 返回 [开启音存在, 关闭音存在]，供设置页展示状态。
#[tauri::command]
pub fn get_sound_status() -> (bool, bool) {
    sound::sound_files_exist()
}
