// 模拟时序常量 — ARCHITECTURE v3.0
//
// 集中管理所有动作内部的延迟常量，便于调优和维护。
// 这些常量影响模拟的"人类感"，过短可能触发防作弊检测，过长则影响效率。

// === 键盘时序 ===

/// 单次按键内部的按下→释放延迟（毫秒）
pub const KEY_PRESS_HOLD_MS: u64 = 1;

/// 组合键中每个修饰键之间的延迟（毫秒）
pub const KEY_COMBO_STEP_MS: u64 = 5;

// === 鼠标时序 ===

/// 点击时移动到位后按下前的稳定延迟（毫秒）
pub const MOUSE_CLICK_SETTLE_MS: u64 = 5;

/// 点击时按下到释放的延迟（毫秒）
pub const MOUSE_CLICK_HOLD_MS: u64 = 10;

/// 拖拽时移动到位后的稳定延迟（毫秒）
pub const MOUSE_DRAG_MOVE_MS: u64 = 10;

/// 拖拽时按下后开始移动前的延迟（毫秒）
pub const MOUSE_DRAG_PRESS_MS: u64 = 20;
