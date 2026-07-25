// 坐标系转换 — ARCHITECTURE v2.0
//
// 负责屏幕坐标 → Interception 归一化坐标（0-65535）的转换。
// 当前版本：单显示器 + 标准 DPI（沿用旧 mouse_worker 的转换逻辑）。

/// 归一化坐标上限（Interception 绝对坐标范围 0-65535）
const NORM_MAX: i64 = 65535;

/// 坐标系管理器
pub struct CoordinateMapper {
    screen_width: i32,
    screen_height: i32,
}

impl CoordinateMapper {
    /// 创建坐标映射器（读取主显示器分辨率）
    pub fn new() -> Self {
        let (w, h) = Self::get_screen_size();
        Self {
            screen_width: w,
            screen_height: h,
        }
    }

    /// 屏幕坐标 → Interception 归一化坐标 (0-65535)
    ///
    /// 无法获取屏幕尺寸时回退为原始坐标（不转换）。
    pub fn to_normalized(&self, x: i32, y: i32) -> (i32, i32) {
        if self.screen_width <= 0 || self.screen_height <= 0 {
            return (x, y);
        }
        let nx = (x as i64 * NORM_MAX / self.screen_width as i64) as i32;
        let ny = (y as i64 * NORM_MAX / self.screen_height as i64) as i32;
        (nx, ny)
    }

    /// 获取主显示器分辨率
    fn get_screen_size() -> (i32, i32) {
        #[cfg(windows)]
        {
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN,
            };
            unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) }
        }
        #[cfg(not(windows))]
        {
            (0, 0)
        }
    }
}
