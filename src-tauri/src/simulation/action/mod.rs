// 统一动作定义 — ARCHITECTURE v3.0
//
// Action 是业务层抽象（键盘/鼠标动作），会被展开为 Runtime Actor 内部推进的原子 SimulationEvent。

mod sequence;

pub use sequence::ActionSequence;

use crate::simulation::event::SimulationEvent;
use crate::simulation::keyboard::KeyAction;
use crate::simulation::mouse::MouseAction;

/// 统一动作类型（业务层抽象）
#[derive(Debug, Clone)]
pub enum Action {
    /// 键盘动作
    Keyboard(KeyAction),
    /// 鼠标动作
    Mouse(MouseAction),
    /// 显式延迟（一般由 ActionStep 的 interval_ms 生成）
    #[allow(dead_code)]
    Delay(u64),
}

impl Action {
    /// 将动作展开为事件序列
    pub fn to_events(&self) -> Vec<SimulationEvent> {
        match self {
            Action::Keyboard(ka) => ka.to_events(),
            Action::Mouse(ma) => ma.to_events(),
            Action::Delay(ms) => vec![SimulationEvent::Delay { ms: *ms }],
        }
    }
}
