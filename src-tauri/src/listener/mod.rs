// 输入监听层。
//
// Interception context 在线程内创建、使用和销毁；外部只持有可关闭、可 Join 的 Handle。

mod filter;
mod hotkey;

use crate::state::{Activity, AppState, SharedState};
use interception::Stroke;
use log::{error, info};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
use std::sync::{Arc, Mutex, Weak};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tauri::AppHandle;

pub struct ListenerHandle {
    shutdown_tx: SyncSender<()>,
    join: Mutex<Option<JoinHandle<()>>>,
}

impl Drop for ListenerHandle {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.try_send(());
        if let Ok(join) = self.join.get_mut() {
            if let Some(join) = join.take() {
                let _ = join.join();
            }
        }
    }
}

/// 启动监听线程并等待其完成 context 与过滤器初始化。
pub fn start_listener(app: AppHandle, state: SharedState) -> Result<ListenerHandle, String> {
    let (shutdown_tx, shutdown_rx) = mpsc::sync_channel(1);
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let join = thread::Builder::new()
        .name("mimic-input-listener".to_string())
        .spawn(move || run_listener(app, Arc::downgrade(&state), shutdown_rx, ready_tx))
        .map_err(|error| format!("failed to spawn listener thread: {error}"))?;

    match ready_rx.recv_timeout(Duration::from_secs(2)) {
        Ok(Ok(())) => Ok(ListenerHandle {
            shutdown_tx,
            join: Mutex::new(Some(join)),
        }),
        Ok(Err(error)) => {
            let _ = join.join();
            Err(error)
        }
        Err(_) => {
            let _ = shutdown_tx.try_send(());
            let _ = join.join();
            Err("listener initialization timed out".to_string())
        }
    }
}

fn run_listener(
    app: AppHandle,
    state: Weak<Mutex<AppState>>,
    shutdown_rx: Receiver<()>,
    ready_tx: SyncSender<Result<(), String>>,
) {
    let interception = match interception::Interception::new() {
        Some(context) => context,
        None => {
            let _ = ready_tx.send(Err("failed to create listener context".to_string()));
            return;
        }
    };
    filter::set_input_filters(&interception);
    if ready_tx.send(Ok(())).is_err() {
        return;
    }

    info!("[listener] listener thread started");
    let mut hotkey_debouncer = hotkey::HotkeyDebouncer::default();
    loop {
        match shutdown_rx.try_recv() {
            Ok(()) | Err(TryRecvError::Disconnected) => break,
            Err(TryRecvError::Empty) => {}
        }

        let device = interception.wait_with_timeout(Duration::from_millis(50));
        let Some(state) = state.upgrade() else {
            break;
        };
        if device == 0 {
            continue;
        }

        if interception::is_mouse(device) {
            handle_mouse_device(&app, &state, &interception, device);
            continue;
        }
        if !interception::is_keyboard(device) {
            continue;
        }

        let mut strokes = hotkey::keyboard_stroke_buffer();
        let count = interception.receive(device, &mut strokes);
        if count <= 0 {
            if count < 0 {
                error!("[listener] keyboard receive failed: {count}");
            }
            continue;
        }
        for stroke in strokes.iter().take(count as usize) {
            hotkey::handle_keyboard_stroke(
                &app,
                &state,
                &interception,
                device,
                stroke,
                &mut hotkey_debouncer,
            );
        }
    }
    info!("[listener] listener thread stopped");
}

/// 透传鼠标事件；拾取态下左键按下完成坐标拾取。
fn handle_mouse_device(
    app: &AppHandle,
    state: &SharedState,
    interception: &interception::Interception,
    device: i32,
) {
    use interception::{MouseFlags, MouseState};

    let mut strokes = [Stroke::Mouse {
        state: MouseState::empty(),
        flags: MouseFlags::empty(),
        rolling: 0,
        x: 0,
        y: 0,
        information: 0,
    }; 16];

    let count = interception.receive(device, &mut strokes);
    if count <= 0 {
        if count < 0 {
            error!("[listener] mouse receive failed: {count}");
        }
        return;
    }

    let slice = &strokes[..count as usize];
    let sent = interception.send(device, slice);
    if sent != count {
        error!("[listener] mouse pass-through incomplete: sent={sent}, expected={count}");
    }

    let left_down = slice.iter().any(|stroke| {
        matches!(
            stroke,
            Stroke::Mouse { state, .. } if state.contains(MouseState::LEFT_BUTTON_DOWN)
        )
    });
    if !left_down {
        return;
    }

    let picking = state
        .lock()
        .map(|state| state.activity == Activity::PickingMouse)
        .unwrap_or(false);
    if !picking {
        return;
    }

    match read_cursor_pos() {
        Some((x, y)) => crate::mouse_picker::finish_pick(app, state, x, y),
        None => {
            error!("[listener] GetCursorPos failed during pick");
            crate::mouse_picker::fail_pick(app, state, "无法读取鼠标坐标");
        }
    }
}

#[cfg(windows)]
fn read_cursor_pos() -> Option<(i32, i32)> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut point = POINT { x: 0, y: 0 };
    if unsafe { GetCursorPos(&mut point) } != 0 {
        Some((point.x, point.y))
    } else {
        None
    }
}

#[cfg(not(windows))]
fn read_cursor_pos() -> Option<(i32, i32)> {
    None
}
