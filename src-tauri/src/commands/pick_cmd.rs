//! 鼠标坐标拾取命令。

use crate::error::{CommandError, CommandResult};
use crate::mouse_picker;
use crate::state::SharedState;

#[tauri::command]
pub fn start_pick_mouse_position(
    row_id: String,
    state: tauri::State<SharedState>,
    app: tauri::AppHandle,
) -> CommandResult<()> {
    mouse_picker::start_pick_mouse_position(app, state.inner().clone(), row_id)
        .map_err(CommandError::from)
}

#[tauri::command]
pub fn cancel_pick_mouse_position(
    state: tauri::State<SharedState>,
    app: tauri::AppHandle,
) -> CommandResult<()> {
    if mouse_picker::cancel_pick(&app, state.inner(), "user_cancelled") {
        Ok(())
    } else {
        Err("no_active_mouse_pick_session".into())
    }
}
