// Interception 驱动实现 — ARCHITECTURE v2.0
//
// 实现 InputDriver trait，封装 Interception 驱动的具体调用细节。
// 与 listener 共享同一个 SendInterception context（仅 send，非阻塞）。

use super::device::DeviceCache;
use super::input_driver::{DriverError, InputDriver};
use crate::simulation::event::MouseButton;
use crate::simulation::mouse::CoordinateMapper;
use crate::state::SendInterception;
use interception::{KeyState, MouseFlags, MouseState, ScanCode, Stroke};
use log::warn;
use std::convert::TryFrom;
use std::sync::{Arc, Mutex, MutexGuard};

/// Interception 滚轮单位：1 刻度 = 120 个滚轮单位（WHEEL_DELTA）
const WHEEL_DELTA: i32 = 120;

/// Interception 驱动适配器
pub struct InterceptionDriver {
    /// Interception context（与 listener 共享同一个 context）
    context: Arc<Mutex<Option<SendInterception>>>,
    /// 设备缓存（键盘/鼠标设备编号）
    device_cache: DeviceCache,
    /// 坐标转换器
    coord_mapper: CoordinateMapper,
}

impl InterceptionDriver {
    /// 创建驱动实例（应用启动时，worker 线程创建前）
    pub fn new(context: Arc<Mutex<Option<SendInterception>>>) -> Self {
        let driver = Self {
            context,
            device_cache: DeviceCache::new(),
            coord_mapper: CoordinateMapper::new(),
        };
        driver.device_cache.scan();
        driver
    }

    /// 获取 Interception context（加锁）
    fn get_context(&self) -> Result<MutexGuard<'_, Option<SendInterception>>, DriverError> {
        self.context
            .lock()
            .map_err(|e| DriverError::SendFailed(format!("Lock failed: {}", e)))
    }
}

impl InputDriver for InterceptionDriver {
    fn send_keyboard(&self, scan_code: u16, is_press: bool) -> Result<(), DriverError> {
        let device = self
            .device_cache
            .get_keyboard()
            .ok_or_else(|| DriverError::DeviceNotFound("keyboard".to_string()))?;

        let ctx_guard = self.get_context()?;
        let interception = ctx_guard.as_ref().ok_or(DriverError::NotReady)?;

        let stroke = build_keyboard_stroke(scan_code, is_press);
        interception.0.send(device as i32, &[stroke]);
        Ok(())
    }

    fn send_mouse_move(&self, x: i32, y: i32) -> Result<(), DriverError> {
        let device = self
            .device_cache
            .get_mouse()
            .ok_or_else(|| DriverError::DeviceNotFound("mouse".to_string()))?;

        let ctx_guard = self.get_context()?;
        let interception = ctx_guard.as_ref().ok_or(DriverError::NotReady)?;

        // 坐标转换：屏幕坐标 → 归一化坐标 (0-65535)
        let (norm_x, norm_y) = self.coord_mapper.to_normalized(x, y);

        let stroke = Stroke::Mouse {
            state: MouseState::empty(),
            flags: MouseFlags::MOVE_ABSOLUTE,
            rolling: 0,
            x: norm_x,
            y: norm_y,
            information: 0,
        };
        interception.0.send(device as i32, &[stroke]);
        Ok(())
    }

    fn send_mouse_button(&self, button: MouseButton, is_press: bool) -> Result<(), DriverError> {
        let device = self
            .device_cache
            .get_mouse()
            .ok_or_else(|| DriverError::DeviceNotFound("mouse".to_string()))?;

        let ctx_guard = self.get_context()?;
        let interception = ctx_guard.as_ref().ok_or(DriverError::NotReady)?;

        let state = match (button, is_press) {
            (MouseButton::Left, true) => MouseState::LEFT_BUTTON_DOWN,
            (MouseButton::Left, false) => MouseState::LEFT_BUTTON_UP,
            (MouseButton::Right, true) => MouseState::RIGHT_BUTTON_DOWN,
            (MouseButton::Right, false) => MouseState::RIGHT_BUTTON_UP,
            (MouseButton::Middle, true) => MouseState::MIDDLE_BUTTON_DOWN,
            (MouseButton::Middle, false) => MouseState::MIDDLE_BUTTON_UP,
            // 鼠标侧键（预留）
            (MouseButton::Side1, _) | (MouseButton::Side2, _) => {
                warn!("[InterceptionDriver] side buttons not yet implemented");
                return Err(DriverError::SendFailed(
                    "Side buttons not supported yet".to_string(),
                ));
            }
        };

        let stroke = Stroke::Mouse {
            state,
            flags: MouseFlags::empty(),
            rolling: 0,
            x: 0,
            y: 0,
            information: 0,
        };
        interception.0.send(device as i32, &[stroke]);
        Ok(())
    }

    fn send_mouse_wheel(&self, delta: i32) -> Result<(), DriverError> {
        let device = self
            .device_cache
            .get_mouse()
            .ok_or_else(|| DriverError::DeviceNotFound("mouse".to_string()))?;

        let ctx_guard = self.get_context()?;
        let interception = ctx_guard.as_ref().ok_or(DriverError::NotReady)?;

        // 滚轮单位转换：1 刻度 = 120 个滚轮单位（WHEEL_DELTA）
        let rolling = (delta * WHEEL_DELTA) as i16;

        let stroke = Stroke::Mouse {
            state: MouseState::WHEEL,
            flags: MouseFlags::empty(),
            rolling,
            x: 0,
            y: 0,
            information: 0,
        };
        interception.0.send(device as i32, &[stroke]);
        Ok(())
    }

    fn is_ready(&self) -> bool {
        self.context
            .lock()
            .map(|guard| guard.is_some())
            .unwrap_or(false)
    }
}

/// 构建键盘 Stroke
fn build_keyboard_stroke(scan_code: u16, is_press: bool) -> Stroke {
    let key_state = if is_press {
        KeyState::empty()
    } else {
        KeyState::UP
    };

    // E0 扩展键标记（scan_code > 127）
    let state_flags = if scan_code > 127 {
        key_state | KeyState::E0
    } else {
        key_state
    };

    let code = ScanCode::try_from(scan_code).unwrap_or_else(|_| {
        warn!(
            "[InterceptionDriver] invalid scan_code {}, using Esc",
            scan_code
        );
        ScanCode::Esc
    });

    Stroke::Keyboard {
        code,
        state: state_flags,
        information: 0,
    }
}
