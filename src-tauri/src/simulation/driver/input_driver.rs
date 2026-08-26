// 输入设备驱动抽象 — ARCHITECTURE v2.0
//
// 通过 InputDriver trait 抽象底层驱动，使模拟逻辑与具体驱动实现解耦。
// 当前实现：InterceptionDriver。未来可能：SendInputDriver、MockDriver（测试用）。
//
// 注：ARCHITECTURE 文档中此文件命名为 trait.rs，但 `trait` 是 Rust 保留字，
// 无法作为模块名（`mod trait;` 不合法），故实际命名为 input_driver.rs。

use crate::simulation::event::MouseButton;

/// 输入设备驱动抽象
pub trait InputDriver {
    /// 发送键盘事件
    ///
    /// - `scan_code`: 硬件扫描码（0-127 标准键，>127 为 E0 扩展键）
    /// - `is_press`: true=按下，false=释放
    fn send_keyboard(&mut self, scan_code: u16, is_press: bool) -> Result<(), DriverError>;

    /// 发送鼠标移动（绝对屏幕坐标，内部归一化到 0-65535）
    fn send_mouse_move(&mut self, x: i32, y: i32) -> Result<(), DriverError>;

    /// 发送鼠标按键事件
    ///
    /// - `is_press`: true=按下，false=释放
    fn send_mouse_button(&mut self, button: MouseButton, is_press: bool)
        -> Result<(), DriverError>;

    /// 发送鼠标滚轮事件（delta 正数向上，负数向下，单位为刻度）
    fn send_mouse_wheel(&mut self, delta: i32) -> Result<(), DriverError>;
}

/// 驱动错误类型
#[derive(Debug)]
pub enum DriverError {
    /// 驱动未就绪（context 为 None）
    NotReady,
    /// 发送失败
    SendFailed(String),
}

impl std::fmt::Display for DriverError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            DriverError::NotReady => write!(f, "Driver not ready"),
            DriverError::SendFailed(msg) => write!(f, "Send failed: {}", msg),
        }
    }
}

impl std::error::Error for DriverError {}
