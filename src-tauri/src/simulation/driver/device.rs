// 设备缓存管理 — ARCHITECTURE v2.0
//
// 避免每次发送事件都扫描设备编号，启动时扫描一次并缓存。
// Interception 设备编号规则：键盘 1-10，鼠标 11-20（INTERCEPTION_MAX_DEVICE = 20）。

use std::sync::atomic::{AtomicU8, Ordering};

/// 设备缓存管理器
pub struct DeviceCache {
    keyboard_device: AtomicU8,
    mouse_device: AtomicU8,
}

impl DeviceCache {
    pub fn new() -> Self {
        Self {
            keyboard_device: AtomicU8::new(0),
            mouse_device: AtomicU8::new(0),
        }
    }

    /// 扫描并缓存设备编号（调用时机：InterceptionDriver 创建时）
    pub fn scan(&self) {
        if let Some(kb) = (1..=10).find(|d| interception::is_keyboard(*d)) {
            self.keyboard_device.store(kb as u8, Ordering::Relaxed);
        }
        if let Some(ms) = (11..=20).find(|d| interception::is_mouse(*d)) {
            self.mouse_device.store(ms as u8, Ordering::Relaxed);
        }
    }

    /// 获取键盘设备编号（0 表示未找到）
    pub fn get_keyboard(&self) -> Option<u8> {
        match self.keyboard_device.load(Ordering::Relaxed) {
            0 => None,
            dev => Some(dev),
        }
    }

    /// 获取鼠标设备编号（0 表示未找到）
    pub fn get_mouse(&self) -> Option<u8> {
        match self.mouse_device.load(Ordering::Relaxed) {
            0 => None,
            dev => Some(dev),
        }
    }
}
