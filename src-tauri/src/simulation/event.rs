// 统一模拟事件定义 — ARCHITECTURE v2.0
//
// SimulationEvent 是驱动层的原子事件，worker 接收后直接调用驱动 API。
// 所有延迟（动作内部 + 步骤间隔）统一用 Delay 事件表示，由 worker 单线程串行执行，
// 确保时序精确（长按不会与后续间隔并行）。

/// 统一模拟事件 — 通过 channel 发送给 worker
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
    /// 延迟指定毫秒数（在 worker 线程执行 sleep）
    Delay { ms: u64 },
    /// 停止信号（保留，当前停止通过 stop_flag 实现）
    #[allow(dead_code)]
    Stop,
}

/// 鼠标按钮类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    /// 左键
    Left,
    /// 右键
    #[allow(dead_code)]
    Right,
    /// 中键
    #[allow(dead_code)]
    Middle,
    /// 侧键 1（通常是前进键，预留）
    #[allow(dead_code)]
    Side1,
    /// 侧键 2（通常是后退键，预留）
    #[allow(dead_code)]
    Side2,
}
