//! 运行时与导航命令适配层。

use crate::config;
use crate::error::{CommandError, CommandResult};
use crate::hotkeys::HotkeyUpdateResult;
use crate::runtime::RuntimePhase;
use crate::state::{Activity, PageId, RuntimeStatus, SharedState};
use tauri::Emitter;

#[tauri::command]
pub fn set_current_page(
    page: String,
    state: tauri::State<SharedState>,
    app: tauri::AppHandle,
) -> CommandResult<()> {
    let page_id = PageId::try_from(page.as_str())?;
    let status = {
        let mut app_state = state
            .inner()
            .lock()
            .map_err(|error| CommandError::from(format!("Failed to lock state: {error}")))?;
        if app_state.activity != Activity::Idle {
            return Err(format!("busy: {:?} is active", app_state.activity).into());
        }
        app_state.navigation = page_id;
        app_state.active_custom_sequence_id = None;
        let status = app_state.runtime_status();
        log::info!("[navigation] page={:?}, status={:?}", page_id, status);
        status
    };
    emit_status(&app, status);
    Ok(())
}

#[tauri::command]
pub fn update_hotkeys(
    hotkeys: config::HotkeyConfig,
    state: tauri::State<SharedState>,
) -> CommandResult<HotkeyUpdateResult> {
    crate::hotkeys::update_hotkeys(&state, hotkeys).map_err(CommandError::from)
}

#[tauri::command]
pub fn stop_simulation(
    state: tauri::State<SharedState>,
    app: tauri::AppHandle,
) -> CommandResult<()> {
    match crate::runner::SimulationRunner::stop(&app, state.inner())? {
        crate::runtime::StopOutcome::Stopped => Ok(()),
        crate::runtime::StopOutcome::AlreadyIdle => Err("not_running".into()),
    }
}

#[tauri::command]
pub fn get_runtime_status(state: tauri::State<SharedState>) -> CommandResult<RuntimeStatus> {
    let app_state = state
        .inner()
        .lock()
        .map_err(|error| CommandError::from(format!("Failed to lock state: {error}")))?;
    if let Some(runtime) = &app_state.runtime {
        if matches!(
            runtime.snapshot().phase,
            RuntimePhase::Error { .. } | RuntimePhase::Shutdown
        ) {
            return Ok(RuntimeStatus::Error);
        }
    }
    Ok(app_state.runtime_status())
}

#[tauri::command]
pub fn enter_custom_sequence(
    id: String,
    state: tauri::State<SharedState>,
    app: tauri::AppHandle,
) -> CommandResult<()> {
    let status = {
        let mut app_state = state
            .inner()
            .lock()
            .map_err(|error| CommandError::from(format!("Failed to lock state: {error}")))?;
        if app_state.activity != Activity::Idle {
            return Err(format!("busy: {:?} is active", app_state.activity).into());
        }
        if !app_state
            .config
            .custom_sequences
            .iter()
            .any(|sequence| sequence.id == id)
        {
            return Err("custom_sequence_not_found".into());
        }
        app_state.navigation = PageId::Custom;
        app_state.active_custom_sequence_id = Some(id.clone());
        log::info!("[navigation] active custom sequence id={id}");
        app_state.runtime_status()
    };
    emit_status(&app, status);
    Ok(())
}

fn emit_status(app: &tauri::AppHandle, status: RuntimeStatus) {
    if let Err(error) = app.emit(
        "runtime_status_changed",
        serde_json::json!({ "status": status }),
    ) {
        log::error!("[runtime_cmd] failed to emit status: {error}");
    }
}
