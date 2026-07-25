// 鼠标动作类型 — ARCHITECTURE v2.0
//
// MouseAction 是业务层可理解的鼠标操作单元，会被展开为一系列原子 SimulationEvent。

use crate::simulation::event::{MouseButton, SimulationEvent};

/// 点击时移动到位后按下前的稳定延迟（毫秒）
const CLICK_SETTLE_MS: u64 = 5;

/// 点击时按下到释放的延迟（毫秒）
const CLICK_HOLD_MS: u64 = 10;

/// 拖拽时移动到位后的稳定延迟（毫秒）
const DRAG_MOVE_MS: u64 = 10;

/// 拖拽时按下后开始移动前的延迟（毫秒）
const DRAG_PRESS_MS: u64 = 20;

/// 鼠标动作类型
#[derive(Debug, Clone)]
pub enum MouseAction {
    /// 移动到绝对屏幕坐标
    #[allow(dead_code)]
    MoveTo { x: i32, y: i32 },

    /// 点击（移动 + 按下 + 释放）
    Click { button: MouseButton, x: i32, y: i32 },

    /// 仅按下（不释放）
    #[allow(dead_code)]
    Down { button: MouseButton },

    /// 仅释放
    #[allow(dead_code)]
    Up { button: MouseButton },

    /// 长按（按下 → 保持 → 释放）
    #[allow(dead_code)]
    Hold {
        button: MouseButton,
        duration_ms: u64,
    },

    /// 滚轮滚动（正数向上，负数向下，单位为刻度）
    #[allow(dead_code)]
    Scroll { delta: i32 },

    /// 拖拽（移动到起点 → 按住 → 移动到终点 → 释放，预留）
    #[allow(dead_code)]
    Drag {
        button: MouseButton,
        from: (i32, i32),
        to: (i32, i32),
    },
}

impl MouseAction {
    /// 将动作展开为事件序列
    pub fn to_events(&self) -> Vec<SimulationEvent> {
        match self {
            MouseAction::MoveTo { x, y } => {
                vec![SimulationEvent::MouseMove { x: *x, y: *y }]
            }

            MouseAction::Click { button, x, y } => vec![
                SimulationEvent::MouseMove { x: *x, y: *y },
                SimulationEvent::Delay {
                    ms: CLICK_SETTLE_MS,
                },
                SimulationEvent::MouseButtonDown { button: *button },
                SimulationEvent::Delay { ms: CLICK_HOLD_MS },
                SimulationEvent::MouseButtonUp { button: *button },
            ],

            MouseAction::Down { button } => {
                vec![SimulationEvent::MouseButtonDown { button: *button }]
            }

            MouseAction::Up { button } => {
                vec![SimulationEvent::MouseButtonUp { button: *button }]
            }

            MouseAction::Hold {
                button,
                duration_ms,
            } => vec![
                SimulationEvent::MouseButtonDown { button: *button },
                SimulationEvent::Delay { ms: *duration_ms },
                SimulationEvent::MouseButtonUp { button: *button },
            ],

            MouseAction::Scroll { delta } => {
                vec![SimulationEvent::MouseWheel { delta: *delta }]
            }

            MouseAction::Drag { button, from, to } => vec![
                SimulationEvent::MouseMove {
                    x: from.0,
                    y: from.1,
                },
                SimulationEvent::Delay { ms: DRAG_MOVE_MS },
                SimulationEvent::MouseButtonDown { button: *button },
                SimulationEvent::Delay { ms: DRAG_PRESS_MS },
                SimulationEvent::MouseMove { x: to.0, y: to.1 },
                SimulationEvent::Delay { ms: DRAG_MOVE_MS },
                SimulationEvent::MouseButtonUp { button: *button },
            ],
        }
    }
}
