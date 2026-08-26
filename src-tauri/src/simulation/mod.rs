// 模拟模块根 — ARCHITECTURE v3.0（方案 B：双线程 + 统一延迟模型）
//
// 职责分层：
//   - event    : 统一原子事件 SimulationEvent（驱动层）
//   - action   : 业务动作 Action / ActionSequence（业务层）
//   - keyboard : 键盘动作类型 KeyAction
//   - mouse    : 鼠标动作类型 MouseAction + 坐标转换 CoordinateMapper
//   - driver   : InputDriver trait + InterceptionDriver 实现（驱动抽象层）
//   - timing   : 统一时序常量（v3.0 新增）

pub mod action;
pub mod driver;
pub mod event;
pub mod keyboard;
pub mod mouse;
pub mod timing;
