// 键盘动作类型 — ARCHITECTURE v3.0
//
// KeyAction 是业务层可理解的键盘操作单元，会被展开为一系列原子 SimulationEvent。

use crate::simulation::event::SimulationEvent;
use crate::simulation::timing::{KEY_COMBO_STEP_MS, KEY_PRESS_HOLD_MS};

/// 键盘动作类型
#[derive(Debug, Clone)]
pub enum KeyAction {
    /// 单次按键（按下 + 短暂延迟 + 释放）
    Press { scan_code: u16 },

    /// 仅按下（不释放），由外部逻辑控制释放时机
    #[allow(dead_code)]
    Down { scan_code: u16 },

    /// 仅释放，与 Down 配合使用
    #[allow(dead_code)]
    Up { scan_code: u16 },

    /// 长按（按下 → 保持指定时长 → 释放）
    #[allow(dead_code)]
    Hold { scan_code: u16, duration_ms: u64 },

    /// 组合键（修饰键 + 目标键，预留）
    #[allow(dead_code)]
    Combo { modifiers: Vec<u16>, key: u16 },
}

impl KeyAction {
    /// 将动作展开为事件序列
    pub fn to_events(&self) -> Vec<SimulationEvent> {
        match self {
            KeyAction::Press { scan_code } => vec![
                SimulationEvent::KeyDown {
                    scan_code: *scan_code,
                },
                SimulationEvent::Delay {
                    ms: KEY_PRESS_HOLD_MS,
                },
                SimulationEvent::KeyUp {
                    scan_code: *scan_code,
                },
            ],

            KeyAction::Down { scan_code } => vec![SimulationEvent::KeyDown {
                scan_code: *scan_code,
            }],

            KeyAction::Up { scan_code } => vec![SimulationEvent::KeyUp {
                scan_code: *scan_code,
            }],

            KeyAction::Hold {
                scan_code,
                duration_ms,
            } => vec![
                SimulationEvent::KeyDown {
                    scan_code: *scan_code,
                },
                SimulationEvent::Delay { ms: *duration_ms },
                SimulationEvent::KeyUp {
                    scan_code: *scan_code,
                },
            ],

            KeyAction::Combo { modifiers, key } => {
                let mut events = Vec::new();
                // 按下所有修饰键
                for &m in modifiers {
                    events.push(SimulationEvent::KeyDown { scan_code: m });
                    events.push(SimulationEvent::Delay {
                        ms: KEY_COMBO_STEP_MS,
                    });
                }
                // 按下目标键
                events.push(SimulationEvent::KeyDown { scan_code: *key });
                events.push(SimulationEvent::Delay {
                    ms: KEY_PRESS_HOLD_MS,
                });
                events.push(SimulationEvent::KeyUp { scan_code: *key });
                // 逆序释放修饰键
                for &m in modifiers.iter().rev() {
                    events.push(SimulationEvent::Delay {
                        ms: KEY_COMBO_STEP_MS,
                    });
                    events.push(SimulationEvent::KeyUp { scan_code: m });
                }
                events
            }
        }
    }
}
