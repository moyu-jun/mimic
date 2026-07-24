// 按键模拟 worker — DESIGN 8.4 / 阶段 13
//
// 从 channel 接收 ActionEvent，转译为 Interception Stroke 并发送。
// 复用与热键监听共享的 Interception context。
// 状态机门控：仅在 RunningKeyboard 状态下执行模拟。

use crate::state::{RuntimeStatus, SendInterception, SharedState};
use interception::{KeyState, ScanCode, Stroke};
use log::{error, info, warn};
use std::convert::TryFrom;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

/// 模拟事件类型 — DESIGN 8.4
#[derive(Debug, Clone)]
pub enum ActionEvent {
    /// 按键按下
    KeyPress { scan_code: u16 },
    /// 按键释放
    KeyRelease { scan_code: u16 },
    /// 停止信号（保留，当前通过 stop_flag 实现停止）
    #[allow(dead_code)]
    Stop,
}

/// 启动按键模拟 worker 线程 — DESIGN 8.4
///
/// 长生命周期线程：循环接收 ActionEvent → 状态机门控 → 转译为 Stroke → send()。
/// 与热键监听共享同一 Interception context，避免驱动重复初始化。
pub fn start_keyboard_worker(
    rx: Receiver<ActionEvent>,
    state: SharedState,
    ctx: Arc<Mutex<Option<SendInterception>>>,
) -> Result<(), String> {
    std::thread::spawn(move || {
        info!("[keyboard_worker] worker thread started");

        loop {
            // 接收事件（阻塞）
            let event = match rx.recv() {
                Ok(e) => e,
                Err(e) => {
                    warn!("[keyboard_worker] channel closed: {}", e);
                    break;
                }
            };

            // 检查是否为停止信号
            if matches!(event, ActionEvent::Stop) {
                info!("[keyboard_worker] received stop signal");
                break;
            }

            // 状态机门控：仅在 RunningKeyboard 状态下执行模拟
            let is_running = {
                let app_state = match state.lock() {
                    Ok(s) => s,
                    Err(e) => {
                        error!("[keyboard_worker] failed to lock state: {}", e);
                        continue;
                    }
                };
                matches!(app_state.runtime_status, RuntimeStatus::RunningKeyboard)
            };

            if !is_running {
                warn!(
                    "[keyboard_worker] received event but not in RunningKeyboard state, skipping"
                );
                continue;
            }

            // 获取 Interception context
            let ctx_guard = match ctx.lock() {
                Ok(g) => g,
                Err(e) => {
                    error!("[keyboard_worker] failed to lock context: {}", e);
                    continue;
                }
            };

            let interception = match ctx_guard.as_ref() {
                Some(i) => &i.0,
                None => {
                    error!("[keyboard_worker] Interception context not available");
                    continue;
                }
            };

            // 转译事件为 Stroke 并发送
            let scan_code = match &event {
                ActionEvent::KeyPress { scan_code } => *scan_code,
                ActionEvent::KeyRelease { scan_code } => *scan_code,
                _ => continue,
            };

            let key_state = match &event {
                ActionEvent::KeyPress { .. } => KeyState::empty(),
                ActionEvent::KeyRelease { .. } => KeyState::UP,
                _ => continue,
            };

            // 处理 E0 扩展键（scan_code > 127）
            let state_flags = if scan_code > 127 {
                key_state | KeyState::E0
            } else {
                key_state
            };

            // 转换 u16 scan_code 为 ScanCode 枚举
            let code = ScanCode::try_from(scan_code).unwrap_or_else(|_| {
                warn!(
                    "[keyboard_worker] invalid scan_code {}, using Esc",
                    scan_code
                );
                ScanCode::Esc
            });

            let stroke = Stroke::Keyboard {
                code,
                state: state_flags,
                information: 0,
            };

            // 扫描 1-10 找第一个键盘设备
            let keyboard_device = (1..=10).find(|d| interception::is_keyboard(*d));
            let device = match keyboard_device {
                Some(d) => d,
                None => {
                    error!("[keyboard_worker] no keyboard device found");
                    continue;
                }
            };

            // 发送到驱动
            interception.send(device, &[stroke]);
        }

        info!("[keyboard_worker] worker thread exited");
    });

    Ok(())
}
