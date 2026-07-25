// 输入过滤器设置 — ARCHITECTURE v3.0 阶段 C
//
// 由 hotkeys_interception.rs 拆分而来：设置键盘 + 鼠标事件过滤器（仅一次，在监听循环外）。

use interception::Interception;
use log::info;

// Predicate: 匹配所有键盘设备
extern "C" fn is_keyboard_device(device: i32) -> bool {
    interception::is_keyboard(device)
}

// Predicate: 匹配所有鼠标设备
extern "C" fn is_mouse_device(device: i32) -> bool {
    interception::is_mouse(device)
}

/// 设置键盘 + 鼠标事件过滤器。
///
/// 键盘：DOWN + UP 事件（热键监听）；鼠标：仅左键按下（坐标拾取用，平时透传零影响）。
pub fn set_input_filters(interception: &Interception) {
    use interception::{Filter, KeyFilter, MouseFilter};
    interception.set_filter(
        is_keyboard_device,
        Filter::KeyFilter(KeyFilter::DOWN | KeyFilter::UP),
    );
    interception.set_filter(
        is_mouse_device,
        Filter::MouseFilter(MouseFilter::LEFT_BUTTON_DOWN),
    );
    info!("[listener] keyboard + mouse filter set");
}
