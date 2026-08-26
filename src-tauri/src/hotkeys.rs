//! Global hotkey configuration. Interception listener reads the committed snapshot.

use crate::config::{self, HotkeyConfig};
use crate::state::{Activity, ActivityLease, SharedState};
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyUpdateResult {
    pub changed: bool,
    pub registered: bool,
    pub persisted: bool,
    pub message: Option<String>,
}

/// Validate and commit a new hotkey snapshot without holding AppState during disk I/O.
pub fn update_hotkeys(
    state: &SharedState,
    new_hotkeys: HotkeyConfig,
) -> Result<HotkeyUpdateResult, String> {
    let _transaction = config::transaction_guard()?;
    let _activity = ActivityLease::acquire(state, Activity::PersistingConfig)?;
    let mut candidate = state
        .lock()
        .map_err(|error| format!("Failed to lock state: {error}"))?
        .config
        .clone();

    let old_hotkeys = &candidate.hotkeys;
    let changed = old_hotkeys.start.scan_code != new_hotkeys.start.scan_code
        || old_hotkeys.stop.scan_code != new_hotkeys.stop.scan_code;
    if !changed {
        return Ok(HotkeyUpdateResult {
            changed: false,
            registered: true,
            persisted: true,
            message: None,
        });
    }

    let keyboard_scan_codes: HashSet<u16> = candidate
        .keyboard_configs
        .iter()
        .map(|config| config.scan_code)
        .collect();
    if keyboard_scan_codes.contains(&new_hotkeys.start.scan_code) {
        return Ok(HotkeyUpdateResult {
            changed: true,
            registered: false,
            persisted: false,
            message: Some(format!(
                "启动热键与按键模拟列表冲突: {}",
                new_hotkeys.start.key_label
            )),
        });
    }
    if keyboard_scan_codes.contains(&new_hotkeys.stop.scan_code) {
        return Ok(HotkeyUpdateResult {
            changed: true,
            registered: false,
            persisted: false,
            message: Some(format!(
                "停止热键与按键模拟列表冲突: {}",
                new_hotkeys.stop.key_label
            )),
        });
    }

    candidate.hotkeys = new_hotkeys.clone();
    if let Err(error) = config::save(&candidate) {
        error!("[hotkeys] persist failed: {}", error);
        return Ok(HotkeyUpdateResult {
            changed: true,
            registered: true,
            persisted: false,
            message: Some(format!("配置持久化失败: {error}")),
        });
    }
    state
        .lock()
        .map_err(|error| format!("Failed to lock state: {error}"))?
        .config = candidate;

    info!(
        "[hotkeys] updated: start={}, stop={}",
        new_hotkeys.start.key_label, new_hotkeys.stop.key_label
    );
    Ok(HotkeyUpdateResult {
        changed: true,
        registered: true,
        persisted: true,
        message: None,
    })
}
