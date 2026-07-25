// 序列构建器 — ARCHITECTURE v3.0 阶段 B
//
// 把「某个页面的配置」转换为统一的 ActionSequence。
// 每种模拟模式（键盘 / 鼠标 / 未来混合）实现一个 builder。
// 新增模式 = 新增一个 builder，监听层与 runner 完全不用改。

use crate::config::{AppConfig, KeyActionType, MouseActionType};
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

/// 键盘 builder：勾选项 → 对应 KeyAction，无勾选返回 None。
pub struct KeyboardSequenceBuilder;

impl SequenceBuilder for KeyboardSequenceBuilder {
    fn build(&self, config: &AppConfig) -> Option<ActionSequence> {
        let mut sequence = ActionSequence::new();
        for cfg in config.keyboard_configs.iter().filter(|c| c.enabled) {
            let action = match cfg.action_type {
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
            };
            sequence.add(action, cfg.interval_ms);
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
        for cfg in config
            .mouse_configs
            .iter()
            .filter(|c| c.enabled && c.x.is_some() && c.y.is_some())
        {
            // 上面 filter 已保证 x/y 为 Some
            let (x, y) = (cfg.x.unwrap(), cfg.y.unwrap());
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
            sequence.add(action, cfg.interval_ms);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CapturedKey, HotkeyConfig, KeyboardConfig, MouseConfig};

    fn empty_config() -> AppConfig {
        AppConfig {
            keyboard_configs: Vec::new(),
            mouse_configs: Vec::new(),
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
    }
}
