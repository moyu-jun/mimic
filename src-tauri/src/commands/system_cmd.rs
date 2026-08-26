//! Read-only operating-system status commands.

use crate::error::{CommandError, CommandResult};

#[tauri::command]
pub fn get_admin_status() -> CommandResult<bool> {
    crate::driver::is_process_elevated().map_err(CommandError::from)
}
