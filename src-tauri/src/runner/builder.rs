// 序列构建器 — ARCHITECTURE v3.0 阶段 B
//
// 把「某个页面的配置」转换为统一的 ActionSequence。
// 每种模拟模式（键盘 / 鼠标 / 未来混合）实现一个 builder。
// 新增模式 = 新增一个 builder，监听层与 runner 完全不用改。

use crate::config::{
    AppConfig, CustomAction, KeyActionType, KeyboardConfig, MouseActionType, MouseConfig,
};
use crate::simulation::action::{Action, ActionSequence};
use crate::simulation::event::MouseButton;
use crate::simulation::keyboard::KeyAction;
use crate::simulation::mouse::MouseAction;
use crate::state::RuntimeStatus;

/// 配置 → 统一 ActionSequence 的可插拔转换器。
pub trait SequenceBuilder {
    /// 从当前配置构建序列；返回 None 表示「无有效动作，忽略本次启动」
    /// （对应鼠标「坐标全空则忽略」、键盘「无勾选则不启动」的语义）。
    fn build(&self, config: &AppConfig) -> Option<ActionSequence>;

    /// 该模式对应的运行态（受前端字符串契约约束）。
    fn running_status(&self) -> RuntimeStatus;
}

/// KeyboardConfig → Action（键盘三种动作类型）。供键盘/自定义 builder 共用。
fn keyboard_config_to_action(cfg: &KeyboardConfig) -> Action {
    match cfg.action_type {
        KeyActionType::Press => Action::Keyboard(KeyAction::Press {
            scan_code: cfg.scan_code,
        }),
        KeyActionType::Hold => Action::Keyboard(KeyAction::Hold {
            scan_code: cfg.scan_code,
            duration_ms: cfg.hold_duration_ms.unwrap_or(100),
        }),
        KeyActionType::Combo => Action::Keyboard(KeyAction::Combo {
            modifiers: cfg.modifiers.clone(),
            key: cfg.scan_code,
        }),
    }
}

/// MouseConfig → Action；坐标全空返回 None（无效动作）。供鼠标/自定义 builder 共用。
fn mouse_config_to_action(cfg: &MouseConfig) -> Option<Action> {
    let (x, y) = match (cfg.x, cfg.y) {
        (Some(x), Some(y)) => (x, y),
        _ => return None,
    };
    let action = match cfg.action_type {
        MouseActionType::ClickLeft => Action::Mouse(MouseAction::Click {
            button: MouseButton::Left,
            x,
            y,
        }),
        MouseActionType::ClickRight => Action::Mouse(MouseAction::Click {
            button: MouseButton::Right,
            x,
            y,
        }),
        MouseActionType::ClickMiddle => Action::Mouse(MouseAction::Click {
            button: MouseButton::Middle,
            x,
            y,
        }),
        MouseActionType::ScrollUp => Action::Mouse(MouseAction::Scroll {
            delta: cfg.scroll_delta.unwrap_or(1),
        }),
        MouseActionType::ScrollDown => Action::Mouse(MouseAction::Scroll {
            delta: -cfg.scroll_delta.unwrap_or(1),
        }),
        MouseActionType::Drag => {
            let from = (x, y);
            let to = (
                cfg.drag_to_x.unwrap_or(from.0),
                cfg.drag_to_y.unwrap_or(from.1),
            );
            Action::Mouse(MouseAction::Drag {
                button: MouseButton::Left,
                from,
                to,
            })
        }
    };
    Some(action)
}

/// 键盘 builder：勾选项 → 对应 KeyAction，无勾选返回 None。
pub struct KeyboardSequenceBuilder;

impl SequenceBuilder for KeyboardSequenceBuilder {
    fn build(&self, config: &AppConfig) -> Option<ActionSequence> {
        let mut sequence = ActionSequence::new();
        for cfg in config.keyboard_configs.iter().filter(|c| c.enabled) {
            sequence.add(keyboard_config_to_action(cfg), cfg.interval_ms);
        }

        if sequence.is_empty() {
            None
        } else {
            Some(sequence)
        }
    }

    fn running_status(&self) -> RuntimeStatus {
        RuntimeStatus::RunningKeyboard
    }
}

/// 鼠标 builder：有效坐标 → 对应 MouseAction，全空返回 None。
pub struct MouseSequenceBuilder;

impl SequenceBuilder for MouseSequenceBuilder {
    fn build(&self, config: &AppConfig) -> Option<ActionSequence> {
        let mut sequence = ActionSequence::new();
        for cfg in config.mouse_configs.iter().filter(|c| c.enabled) {
            if let Some(action) = mouse_config_to_action(cfg) {
                sequence.add(action, cfg.interval_ms);
            }
        }

        if sequence.is_empty() {
            None
        } else {
            Some(sequence)
        }
    }

    fn running_status(&self) -> RuntimeStatus {
        RuntimeStatus::RunningMouse
    }
}

/// 自定义序列 builder：按构造时传入的序列 id 取出对应序列，
/// 遍历其 actions 按 kind 分派（未勾选 / 鼠标坐标全空跳过）。
/// 找不到序列或序列无有效动作 → 返回 None（静默忽略启动）。
///
/// sequence_id 由调用方（hotkey.rs）从 AppState.active_custom_sequence_id 读出后注入，
/// 使 build 仍只依赖 &AppConfig，便于单测。
pub struct CustomSequenceBuilder {
    pub sequence_id: String,
}

impl SequenceBuilder for CustomSequenceBuilder {
    fn build(&self, config: &AppConfig) -> Option<ActionSequence> {
        log::info!(
            "[CustomSequenceBuilder] build called: sequence_id={}, total_sequences={}",
            self.sequence_id,
            config.custom_sequences.len()
        );

        let seq_cfg = config
            .custom_sequences
            .iter()
            .find(|s| s.id == self.sequence_id)?;

        log::info!(
            "[CustomSequenceBuilder] found sequence: name='{}', actions={}",
            seq_cfg.name,
            seq_cfg.actions.len()
        );

        let mut sequence = ActionSequence::new();
        for (idx, action) in seq_cfg.actions.iter().enumerate() {
            match action {
                CustomAction::Keyboard(cfg) if cfg.enabled => {
                    log::info!(
                        "[CustomSequenceBuilder] action[{}]: Keyboard enabled, key={}, interval={}",
                        idx,
                        cfg.key_label,
                        cfg.interval_ms
                    );
                    sequence.add(keyboard_config_to_action(cfg), cfg.interval_ms);
                }
                CustomAction::Mouse(cfg) if cfg.enabled => {
                    log::info!(
                        "[CustomSequenceBuilder] action[{}]: Mouse enabled, type={:?}, x={:?}, y={:?}, interval={}",
                        idx,
                        cfg.action_type,
                        cfg.x,
                        cfg.y,
                        cfg.interval_ms
                    );
                    if let Some(a) = mouse_config_to_action(cfg) {
                        sequence.add(a, cfg.interval_ms);
                    } else {
                        log::warn!(
                            "[CustomSequenceBuilder] action[{}]: Mouse coords invalid, skipped",
                            idx
                        );
                    }
                }
                CustomAction::Keyboard(cfg) => {
                    log::info!(
                        "[CustomSequenceBuilder] action[{}]: Keyboard disabled, key={}, skipped",
                        idx,
                        cfg.key_label
                    );
                }
                CustomAction::Mouse(cfg) => {
                    log::info!(
                        "[CustomSequenceBuilder] action[{}]: Mouse disabled, type={:?}, skipped",
                        idx,
                        cfg.action_type
                    );
                }
            }
        }

        log::info!(
            "[CustomSequenceBuilder] build result: {} valid steps",
            sequence.steps.len()
        );

        if sequence.is_empty() {
            None
        } else {
            Some(sequence)
        }
    }

    fn running_status(&self) -> RuntimeStatus {
        RuntimeStatus::RunningCustom
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CapturedKey, CustomSequence, HotkeyConfig, KeyboardConfig, MouseConfig};

    fn empty_config() -> AppConfig {
        AppConfig {
            keyboard_configs: Vec::new(),
            mouse_configs: Vec::new(),
            custom_sequences: Vec::new(),
            hotkeys: HotkeyConfig {
                start: CapturedKey {
                    key_label: "F12".to_string(),
                    scan_code: 88,
                },
                stop: CapturedKey {
                    key_label: "F12".to_string(),
                    scan_code: 88,
                },
            },
        }
    }

    fn kb(enabled: bool, action_type: KeyActionType) -> KeyboardConfig {
        KeyboardConfig {
            id: "k".to_string(),
            enabled,
            action_type,
            key_label: "F".to_string(),
            scan_code: 33,
            hold_duration_ms: None,
            modifiers: Vec::new(),
            interval_ms: 20,
        }
    }

    fn ms(enabled: bool, x: Option<i32>, y: Option<i32>) -> MouseConfig {
        MouseConfig {
            id: "m".to_string(),
            enabled,
            action_type: MouseActionType::ClickLeft,
            x,
            y,
            scroll_delta: None,
            drag_to_x: None,
            drag_to_y: None,
            interval_ms: 20,
        }
    }

    #[test]
    fn keyboard_empty_returns_none() {
        assert!(KeyboardSequenceBuilder.build(&empty_config()).is_none());
    }

    #[test]
    fn keyboard_all_disabled_returns_none() {
        let mut cfg = empty_config();
        cfg.keyboard_configs = vec![kb(false, KeyActionType::Press)];
        assert!(KeyboardSequenceBuilder.build(&cfg).is_none());
    }

    #[test]
    fn keyboard_enabled_builds_steps() {
        let mut cfg = empty_config();
        cfg.keyboard_configs = vec![
            kb(true, KeyActionType::Press),
            kb(false, KeyActionType::Hold),
        ];
        let seq = KeyboardSequenceBuilder.build(&cfg).unwrap();
        assert_eq!(seq.steps.len(), 1);
    }

    #[test]
    fn mouse_empty_returns_none() {
        assert!(MouseSequenceBuilder.build(&empty_config()).is_none());
    }

    #[test]
    fn mouse_null_coords_returns_none() {
        let mut cfg = empty_config();
        cfg.mouse_configs = vec![ms(true, None, None), ms(true, Some(10), None)];
        assert!(MouseSequenceBuilder.build(&cfg).is_none());
    }

    #[test]
    fn mouse_valid_coords_builds_steps() {
        let mut cfg = empty_config();
        cfg.mouse_configs = vec![ms(true, Some(10), Some(20)), ms(false, Some(1), Some(2))];
        let seq = MouseSequenceBuilder.build(&cfg).unwrap();
        assert_eq!(seq.steps.len(), 1);
    }

    #[test]
    fn running_status_matches_mode() {
        assert_eq!(
            KeyboardSequenceBuilder.running_status(),
            RuntimeStatus::RunningKeyboard
        );
        assert_eq!(
            MouseSequenceBuilder.running_status(),
            RuntimeStatus::RunningMouse
        );
        assert_eq!(
            CustomSequenceBuilder {
                sequence_id: "x".to_string()
            }
            .running_status(),
            RuntimeStatus::RunningCustom
        );
    }

    fn custom_builder(id: &str) -> CustomSequenceBuilder {
        CustomSequenceBuilder {
            sequence_id: id.to_string(),
        }
    }

    #[test]
    fn custom_unknown_id_returns_none() {
        let mut cfg = empty_config();
        cfg.custom_sequences = vec![CustomSequence {
            id: "seq-1".to_string(),
            name: "s".to_string(),
            actions: vec![CustomAction::Keyboard(kb(true, KeyActionType::Press))],
        }];
        // 激活 id 与任何序列都不匹配 → None
        assert!(custom_builder("nope").build(&cfg).is_none());
    }

    #[test]
    fn custom_empty_sequence_returns_none() {
        let mut cfg = empty_config();
        cfg.custom_sequences = vec![CustomSequence {
            id: "seq-1".to_string(),
            name: "s".to_string(),
            actions: Vec::new(),
        }];
        assert!(custom_builder("seq-1").build(&cfg).is_none());
    }

    #[test]
    fn custom_mixed_actions_builds_in_order_and_filters() {
        let mut cfg = empty_config();
        cfg.custom_sequences = vec![CustomSequence {
            id: "seq-1".to_string(),
            name: "s".to_string(),
            actions: vec![
                CustomAction::Keyboard(kb(true, KeyActionType::Press)), // 有效
                CustomAction::Mouse(ms(true, None, None)),              // 坐标全空 → 跳过
                CustomAction::Mouse(ms(true, Some(10), Some(20))),      // 有效
                CustomAction::Keyboard(kb(false, KeyActionType::Press)), // 未勾选 → 跳过
            ],
        }];
        let seq = custom_builder("seq-1").build(&cfg).unwrap();
        // 只保留 2 个有效动作，顺序 = 数组顺序
        assert_eq!(seq.steps.len(), 2);
    }
}
