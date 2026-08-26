//! Interception 输入驱动适配器。
//!
//! context 由 Runtime Actor 线程创建并独占。发送必须确认实际写入一条 stroke；缓存设备
//! 失效时会在合法设备范围内重新发现，不能把零发送误报为成功。

use super::device::{DeviceCache, DeviceKind};
use super::input_driver::{DriverError, InputDriver};
use crate::simulation::event::MouseButton;
use crate::simulation::mouse::CoordinateMapper;
use interception::{KeyState, MouseFlags, MouseState, ScanCode, Stroke};
use std::convert::TryFrom;

const WHEEL_DELTA: i32 = 120;

pub struct InterceptionDriver {
    context: interception::Interception,
    device_cache: DeviceCache,
    coord_mapper: CoordinateMapper,
}

impl InterceptionDriver {
    pub fn new() -> Result<Self, DriverError> {
        let context = interception::Interception::new().ok_or(DriverError::NotReady)?;
        Ok(Self {
            context,
            device_cache: DeviceCache::new(),
            coord_mapper: CoordinateMapper::new(),
        })
    }

    fn send_stroke(&mut self, kind: DeviceKind, stroke: Stroke) -> Result<(), DriverError> {
        if let Some(device) = self.device_cache.cached(kind) {
            if self.context.send(device, &[stroke]) == 1 {
                return Ok(());
            }
            log::warn!(
                "[InterceptionDriver] cached {:?} device {} failed",
                kind,
                device
            );
        }

        for device in DeviceCache::candidates(kind) {
            if Some(device) == self.device_cache.cached(kind) {
                continue;
            }
            if self.context.send(device, &[stroke]) == 1 {
                self.device_cache.remember(kind, device);
                return Ok(());
            }
        }

        Err(DriverError::SendFailed(format!(
            "no {:?} device accepted the event",
            kind
        )))
    }
}

impl InputDriver for InterceptionDriver {
    fn send_keyboard(&mut self, scan_code: u16, is_press: bool) -> Result<(), DriverError> {
        let stroke = build_keyboard_stroke(scan_code, is_press)?;
        self.send_stroke(DeviceKind::Keyboard, stroke)
    }

    fn send_mouse_move(&mut self, x: i32, y: i32) -> Result<(), DriverError> {
        let (norm_x, norm_y) = self.coord_mapper.to_normalized(x, y);
        self.send_stroke(
            DeviceKind::Mouse,
            Stroke::Mouse {
                state: MouseState::empty(),
                flags: MouseFlags::MOVE_ABSOLUTE,
                rolling: 0,
                x: norm_x,
                y: norm_y,
                information: 0,
            },
        )
    }

    fn send_mouse_button(
        &mut self,
        button: MouseButton,
        is_press: bool,
    ) -> Result<(), DriverError> {
        let state = match (button, is_press) {
            (MouseButton::Left, true) => MouseState::LEFT_BUTTON_DOWN,
            (MouseButton::Left, false) => MouseState::LEFT_BUTTON_UP,
            (MouseButton::Right, true) => MouseState::RIGHT_BUTTON_DOWN,
            (MouseButton::Right, false) => MouseState::RIGHT_BUTTON_UP,
            (MouseButton::Middle, true) => MouseState::MIDDLE_BUTTON_DOWN,
            (MouseButton::Middle, false) => MouseState::MIDDLE_BUTTON_UP,
        };
        self.send_stroke(
            DeviceKind::Mouse,
            Stroke::Mouse {
                state,
                flags: MouseFlags::empty(),
                rolling: 0,
                x: 0,
                y: 0,
                information: 0,
            },
        )
    }

    fn send_mouse_wheel(&mut self, delta: i32) -> Result<(), DriverError> {
        let rolling = delta
            .checked_mul(WHEEL_DELTA)
            .and_then(|value| i16::try_from(value).ok())
            .ok_or_else(|| DriverError::SendFailed("mouse wheel delta overflow".to_string()))?;
        self.send_stroke(
            DeviceKind::Mouse,
            Stroke::Mouse {
                state: MouseState::WHEEL,
                flags: MouseFlags::empty(),
                rolling,
                x: 0,
                y: 0,
                information: 0,
            },
        )
    }
}

fn build_keyboard_stroke(scan_code: u16, is_press: bool) -> Result<Stroke, DriverError> {
    let key_state = if is_press {
        KeyState::empty()
    } else {
        KeyState::UP
    };
    let state = if scan_code > 127 {
        key_state | KeyState::E0
    } else {
        key_state
    };
    let code = ScanCode::try_from(scan_code)
        .map_err(|_| DriverError::SendFailed(format!("invalid scan code: {scan_code}")))?;
    Ok(Stroke::Keyboard {
        code,
        state,
        information: 0,
    })
}
