// 统一动作定义 — ARCHITECTURE v2.0
//
// Action 是业务层抽象（键盘/鼠标动作），会被展开为原子 SimulationEvent 发送给 worker。
// ActionSequence 支持键盘/鼠标混合编排，每步带独立执行后间隔。

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

/// 动作步骤 = 动作 + 执行后的等待时间（毫秒）
///
/// interval_ms 会被转换为 Delay 事件追加在动作事件之后（方案 B）。
#[derive(Debug, Clone)]
pub struct ActionStep {
    pub action: Action,
    pub interval_ms: u64,
}

/// 动作序列（支持键盘/鼠标混合）
#[derive(Debug, Clone)]
pub struct ActionSequence {
    pub steps: Vec<ActionStep>,
}

impl ActionSequence {
    pub fn new() -> Self {
        Self { steps: Vec::new() }
    }

    /// 添加动作步骤
    pub fn add(&mut self, action: Action, interval_ms: u64) {
        self.steps.push(ActionStep {
            action,
            interval_ms,
        });
    }

    /// 序列是否为空
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}
