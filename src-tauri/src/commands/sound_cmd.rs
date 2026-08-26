//! 提示音录制与播放命令。

use crate::error::{CommandError, CommandResult};
use crate::sound;
use crate::sound_recorder;
use crate::state::{Activity, ActivityLease, SharedState};

#[tauri::command]
pub fn start_recording(
    target: String,
    state: tauri::State<SharedState>,
    app: tauri::AppHandle,
) -> CommandResult<()> {
    let lease = ActivityLease::acquire(state.inner(), Activity::Recording)?;
    let handle = state
        .inner()
        .lock()
        .map_err(|error| CommandError::from(format!("Failed to lock state: {error}")))?
        .recording
        .clone();

    if let Err(error) =
        sound_recorder::start_recording(app, state.inner().clone(), handle, lease, target)
    {
        return Err(error.into());
    }
    Ok(())
}

#[tauri::command]
pub fn stop_recording(state: tauri::State<SharedState>) -> CommandResult<()> {
    let handle = state
        .inner()
        .lock()
        .map_err(|error| CommandError::from(format!("Failed to lock state: {error}")))?
        .recording
        .clone();
    sound_recorder::stop_recording(&handle).map_err(CommandError::from)
}

#[tauri::command]
pub fn cancel_recording(state: tauri::State<SharedState>) -> CommandResult<()> {
    let handle = state
        .inner()
        .lock()
        .map_err(|error| CommandError::from(format!("Failed to lock state: {error}")))?
        .recording
        .clone();
    sound_recorder::cancel_recording(&handle).map_err(CommandError::from)
}

#[tauri::command]
pub fn save_trimmed_audio(
    target: String,
    start_ms: u32,
    end_ms: u32,
    state: tauri::State<SharedState>,
) -> CommandResult<()> {
    sound_recorder::save_trimmed_audio(state.inner().clone(), target, start_ms, end_ms)
        .map_err(CommandError::from)
}

#[tauri::command]
pub fn preview_sound(target: String) -> CommandResult<()> {
    match target.as_str() {
        "start" => sound::play_start(),
        "stop" => sound::play_stop(),
        _ => return Err("invalid_audio_target".into()),
    }
    Ok(())
}

#[tauri::command]
pub fn get_sound_status() -> (bool, bool) {
    sound::sound_files_exist()
}
