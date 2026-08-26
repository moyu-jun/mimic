//! Runtime Actor 内部的 Interception 设备选择缓存。

#[derive(Debug, Clone, Copy)]
pub enum DeviceKind {
    Keyboard,
    Mouse,
}

pub struct DeviceCache {
    keyboard: Option<i32>,
    mouse: Option<i32>,
}

impl DeviceCache {
    pub fn new() -> Self {
        Self {
            keyboard: None,
            mouse: None,
        }
    }

    pub fn cached(&self, kind: DeviceKind) -> Option<i32> {
        match kind {
            DeviceKind::Keyboard => self.keyboard,
            DeviceKind::Mouse => self.mouse,
        }
    }

    pub fn remember(&mut self, kind: DeviceKind, device: i32) {
        match kind {
            DeviceKind::Keyboard => self.keyboard = Some(device),
            DeviceKind::Mouse => self.mouse = Some(device),
        }
    }

    pub fn candidates(kind: DeviceKind) -> std::ops::RangeInclusive<i32> {
        match kind {
            DeviceKind::Keyboard => 1..=10,
            DeviceKind::Mouse => 11..=20,
        }
    }
}
