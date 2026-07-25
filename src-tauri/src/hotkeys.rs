// 全局热键注册与管理 — DESIGN 6 / DESIGN 8.3 / 阶段 13
//
// 阶段 13：热键配置管理，实际监听由 hotkeys_interception 模块处理。

use crate::config::{self, HotkeyConfig};
use crate::state::SharedState;
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// 热键更新结果 — DESIGN 6.2
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyUpdateResult {
    /// 与已持久化配置对比是否有变化
    pub changed: bool,
    /// 新热键注册是否成功
    pub registered: bool,
    /// 是否成功写入 mimic.ini
    pub persisted: bool,
    /// 可选错误消息
    pub message: Option<String>,
}

/// 更新热键配置 — DESIGN 6.2 / 阶段 13
///
/// 流程：对比变化 → 校验冲突 → 持久化 → 更新内存。
/// Interception 热键由后台监听线程统一处理，不需要注册/注销。
pub fn update_hotkeys(
    state: &SharedState,
    new_hotkeys: HotkeyConfig,
) -> Result<HotkeyUpdateResult, String> {
    let old_hotkeys = {
        let app_state = state
            .lock()
            .map_err(|e| format!("Failed to lock state: {}", e))?;
        app_state.config.hotkeys.clone()
    };

    // 对比变化
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

    // 热键与按键列表冲突校验 — DESIGN 15.6 反馈 Q6
    let keyboard_scan_codes: HashSet<u16> = {
        let app_state = state
            .lock()
            .map_err(|e| format!("Failed to lock state: {}", e))?;
        app_state
            .config
            .keyboard_configs
            .iter()
            .map(|c| c.scan_code)
            .collect()
    };

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

    // 持久化配置
    let mut full_config = {
        let app_state = state
            .lock()
            .map_err(|e| format!("Failed to lock state: {}", e))?;
        app_state.config.clone()
    };
    full_config.hotkeys = new_hotkeys.clone();

    if let Err(e) = config::save(&full_config) {
        error!("[hotkeys] persist failed: {}", e);
        return Ok(HotkeyUpdateResult {
            changed: true,
            registered: true,
            persisted: false,
            message: Some(format!("配置持久化失败: {}", e)),
        });
    }

    // 更新内存
    {
        let mut app_state = state
            .lock()
            .map_err(|e| format!("Failed to lock state: {}", e))?;
        app_state.config.hotkeys = new_hotkeys.clone();
    }

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
