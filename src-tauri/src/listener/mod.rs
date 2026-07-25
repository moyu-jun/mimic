// 输入监听层 — ARCHITECTURE v3.0 阶段 C
//
// 由 hotkeys_interception.rs 拆分而来。职责：
//   - filter : 过滤器设置
//   - hotkey : 热键匹配 + 状态机门控 → 调 SimulationRunner
//   - mod    : start_listener 主循环（wait/receive + 鼠标透传/拾取分派 + 键盘转 hotkey）
//
// 监听层只负责「识别事件 + 决定调哪个编排动作」，不再自己构建序列、不自己 spawn。

mod filter;
mod hotkey;

use crate::state::{RuntimeStatus, SendInterception, SharedState};
use interception::Stroke;
use log::{error, info};
use std::sync::{Arc, Mutex};
use tauri::AppHandle;

/// 启动热键监听线程 — DESIGN 8.3
///
/// 长生命周期线程：循环调用 wait() → receive() → 匹配热键 → 处理 → send()（或阻断）。
/// 热键配置从 AppState.config.hotkeys 动态读取，无需重启监听线程。
/// 实现状态机门控（Idle → Running*）与页面过滤（keyboard/mouse）。
pub fn start_listener(
    app: AppHandle,
    state: SharedState,
    ctx: Arc<Mutex<Option<SendInterception>>>,
) -> Result<(), String> {
    std::thread::spawn(move || {
        info!("[listener] listener thread started");

        // 设置事件过滤器（仅一次，在循环外）
        let filter_set = {
            let ctx_guard = match ctx.lock() {
                Ok(g) => g,
                Err(e) => {
                    error!("[listener] failed to lock context for filter: {}", e);
                    return;
                }
            };
            match ctx_guard.as_ref() {
                Some(i) => {
                    filter::set_input_filters(&i.0);
                    true
                }
                None => {
                    error!("[listener] context not available for filter setup");
                    false
                }
            }
        };

        if !filter_set {
            return;
        }

        loop {
            // 检查 context 是否可用
            let ctx_guard = match ctx.lock() {
                Ok(g) => g,
                Err(e) => {
                    error!("[listener] failed to lock context: {}", e);
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    continue;
                }
            };

            let interception = match ctx_guard.as_ref() {
                Some(i) => &i.0,
                None => {
                    // Context 未初始化（驱动未就绪），休眠后重试
                    drop(ctx_guard);
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    continue;
                }
            };

            // 等待键盘 / 鼠标事件
            let device = interception.wait();

            // 鼠标事件分支 — 坐标拾取（PickingMouse 时捕获）/ 平时透传
            if interception::is_mouse(device) {
                handle_mouse_device(&app, &state, device, ctx_guard);
                continue;
            }

            if !interception::is_keyboard(device) {
                continue;
            }

            // 键盘分支：接收事件后逐个转 hotkey 处理
            let mut strokes = hotkey::keyboard_stroke_buffer();
            let count = interception.receive(device, &mut strokes);
            if count == 0 {
                continue;
            }
            for stroke in strokes.iter().take(count as usize) {
                hotkey::handle_keyboard_stroke(&app, &state, interception, device, stroke);
            }
        }
    });

    Ok(())
}

/// 处理鼠标设备事件 — 透传所有事件；PickingMouse 态下左键按下触发坐标拾取。
///
/// 拾取时需在调用 finish_pick 前释放 context 锁，故 ctx_guard 按值传入以便 drop。
fn handle_mouse_device(
    app: &AppHandle,
    state: &SharedState,
    device: i32,
    ctx_guard: std::sync::MutexGuard<'_, Option<SendInterception>>,
) {
    use interception::{MouseFlags, MouseState};

    // guard 已确认非 None（调用前 interception.wait() 成功）；再取一次内部句柄。
    let interception = match ctx_guard.as_ref() {
        Some(i) => &i.0,
        None => return,
    };

    let mut mstrokes = [Stroke::Mouse {
        state: MouseState::empty(),
        flags: MouseFlags::empty(),
        rolling: 0,
        x: 0,
        y: 0,
        information: 0,
    }; 16];

    let mcount = interception.receive(device, &mut mstrokes);
    if mcount == 0 {
        return;
    }

    // 透传所有鼠标事件，保持目标窗口行为不变；记录是否有左键按下
    let mut left_down = false;
    for stroke in mstrokes.iter().take(mcount as usize) {
        interception.send(device, &[*stroke]);
        if let Stroke::Mouse { state: ms, .. } = stroke {
            if ms.contains(MouseState::LEFT_BUTTON_DOWN) {
                left_down = true;
            }
        }
    }

    if !left_down {
        return;
    }

    // 是否处于拾取态
    let picking = match state.lock() {
        Ok(s) => matches!(s.runtime_status, RuntimeStatus::PickingMouse),
        Err(_) => false,
    };

    if picking {
        // 读屏幕坐标（Interception stroke 不含屏幕坐标）
        let coords = read_cursor_pos();
        // 先释放 context 锁，finish_pick 内部仅操作 state / 主线程
        drop(ctx_guard);
        match coords {
            Some((x, y)) => crate::mouse_picker::finish_pick(app, state, x, y),
            None => {
                error!("[listener] GetCursorPos failed during pick");
                // 坐标读取失败也恢复窗口，避免界面卡在 PickingMouse
                crate::mouse_picker::finish_pick(app, state, 0, 0);
            }
        }
    }
}

/// 读取系统光标屏幕坐标 — 坐标拾取用。
///
/// Interception 鼠标 stroke 的 x/y 是移动量而非屏幕坐标，故用 GetCursorPos
/// 读取系统光标位置。失败返回 None（极罕见）。
#[cfg(windows)]
fn read_cursor_pos() -> Option<(i32, i32)> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut pt = POINT { x: 0, y: 0 };
    if unsafe { GetCursorPos(&mut pt) } != 0 {
        Some((pt.x, pt.y))
    } else {
        None
    }
}

#[cfg(not(windows))]
fn read_cursor_pos() -> Option<(i32, i32)> {
    None
}
