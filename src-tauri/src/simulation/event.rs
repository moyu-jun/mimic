// 统一模拟事件定义 — ARCHITECTURE v3.0
//
// SimulationEvent 是 Runtime Actor 内部推进的原子输入事件。
// 所有延迟（动作内部 + 步骤间隔）统一用 Delay 表示，由 Actor 单线程串行执行，
// 确保时序精确（长按不会与后续间隔并行）。

/// 统一模拟事件 — 仅作为 Actor 内部执行模型，不跨任务排队
#[derive(Debug, Clone)]
pub enum SimulationEvent {
    // === 键盘事件 ===
    /// 按下键盘按键
    KeyDown { scan_code: u16 },
    /// 释放键盘按键
    KeyUp { scan_code: u16 },

    // === 鼠标事件 ===
    /// 移动鼠标到绝对屏幕坐标
    MouseMove { x: i32, y: i32 },
    /// 按下鼠标按键
    MouseButtonDown { button: MouseButton },
    /// 释放鼠标按键
    MouseButtonUp { button: MouseButton },
    /// 滚轮滚动（正数向上，负数向下，单位为刻度）
    MouseWheel { delta: i32 },

    // === 控制事件 ===
    /// 可中断延迟；Actor 同时等待控制命令。
    Delay { ms: u64 },
}

/// 鼠标按钮类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    /// 左键
    Left,
    /// 右键
    Right,
    /// 中键
    Middle,
}
