// 动作序列定义 — ARCHITECTURE v3.0
//
// ActionSequence 支持键盘/鼠标混合编排，每步带独立执行后间隔。

use super::Action;

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
