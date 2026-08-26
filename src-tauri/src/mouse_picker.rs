//! 鼠标坐标拾取会话。
//!
//! 每次拾取拥有唯一 token、原 Ready 状态和受管 30 秒超时；完成、取消、失败都只能
//! 结束匹配 token 的会话，旧回调不能污染新拾取。

use crate::state::{Activity, RuntimeStatus, SharedState};
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

const PICK_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
pub struct PickSession {
    token: u64,
    row_id: String,
    restore_status: RuntimeStatus,
}

enum TimeoutCommand {
    Arm { token: u64, timeout: Duration },
    Cancel { token: u64 },
    Shutdown,
}

struct PickerTimeoutInner {
    command_tx: SyncSender<TimeoutCommand>,
    join: Mutex<Option<JoinHandle<()>>>,
}

impl Drop for PickerTimeoutInner {
    fn drop(&mut self) {
        if self.command_tx.send(TimeoutCommand::Shutdown).is_err() {
            log::debug!("[mouse_picker] timeout service already stopped during drop");
        }
        if let Ok(join) = self.join.get_mut() {
            if let Some(join) = join.take() {
                if join.join().is_err() {
                    log::error!("[mouse_picker] timeout service panicked during drop");
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct PickerTimeoutHandle {
    inner: Arc<PickerTimeoutInner>,
}

impl PickerTimeoutHandle {
    pub fn spawn<F>(on_timeout: F) -> Result<Self, String>
    where
        F: Fn(u64) + Send + 'static,
    {
        let (command_tx, command_rx) = mpsc::sync_channel(4);
        let join = thread::Builder::new()
            .name("mimic-picker-timeout".to_string())
            .spawn(move || {
                let mut armed: Option<(u64, Instant)> = None;
                loop {
                    let command = match armed {
                        Some((token, deadline)) => {
                            let remaining = deadline.saturating_duration_since(Instant::now());
                            match command_rx.recv_timeout(remaining) {
                                Ok(command) => command,
                                Err(RecvTimeoutError::Timeout) => {
                                    armed = None;
                                    on_timeout(token);
                                    continue;
                                }
                                Err(RecvTimeoutError::Disconnected) => return,
                            }
                        }
                        None => match command_rx.recv() {
                            Ok(command) => command,
                            Err(_) => return,
                        },
                    };

                    match command {
                        TimeoutCommand::Arm { token, timeout } => {
                            armed = Some((token, Instant::now() + timeout));
                        }
                        TimeoutCommand::Cancel { token } => {
                            if armed.map(|current| current.0) == Some(token) {
                                armed = None;
                            }
                        }
                        TimeoutCommand::Shutdown => return,
                    }
                }
            })
            .map_err(|error| format!("failed to start picker timeout service: {error}"))?;

        Ok(Self {
            inner: Arc::new(PickerTimeoutInner {
                command_tx,
                join: Mutex::new(Some(join)),
            }),
        })
    }

    fn arm(&self, token: u64) -> Result<(), String> {
        self.inner
            .command_tx
            .send(TimeoutCommand::Arm {
                token,
                timeout: PICK_TIMEOUT,
            })
            .map_err(|_| "picker timeout service unavailable".to_string())
    }

    fn cancel(&self, token: u64) {
        if self
            .inner
            .command_tx
            .send(TimeoutCommand::Cancel { token })
            .is_err()
        {
            log::warn!("[mouse_picker] timeout service unavailable while cancelling token {token}");
        }
    }
}

pub fn start_pick_mouse_position(
    app: AppHandle,
    state: SharedState,
    row_id: String,
) -> Result<(), String> {
    let (session, timeout) = {
        let mut app_state = state
            .lock()
            .map_err(|error| format!("Failed to lock state: {error}"))?;

        let restore_status = match app_state.runtime_status() {
            RuntimeStatus::ReadyMouse => RuntimeStatus::ReadyMouse,
            RuntimeStatus::ReadyCustom => RuntimeStatus::ReadyCustom,
            _ => return Err("mouse picking is only available from a ready page".to_string()),
        };

        let token = app_state.next_pick_token;
        app_state.next_pick_token = app_state.next_pick_token.saturating_add(1);
        let session = PickSession {
            token,
            row_id,
            restore_status,
        };
        let timeout = app_state
            .picker_timeout
            .clone()
            .ok_or_else(|| "picker timeout service unavailable".to_string())?;
        app_state.acquire_activity(Activity::PickingMouse)?;
        app_state.pick_session = Some(session.clone());
        (session, timeout)
    };

    if let Err(error) = timeout.arm(session.token) {
        rollback_unstarted_pick(&state, session.token);
        return Err(error);
    }

    emit_status(&app, RuntimeStatus::PickingMouse);
    if let Some(window) = app.get_webview_window("main") {
        if let Err(error) = window.hide() {
            log::warn!("[mouse_picker] failed to hide window: {}", error);
        }
    }
    log::info!(
        "[mouse_picker] session {} started for row {}",
        session.token,
        session.row_id
    );
    Ok(())
}

pub fn finish_pick(app: &AppHandle, state: &SharedState, x: i32, y: i32) {
    let Some((session, timeout)) = take_session(state, None) else {
        log::warn!("[mouse_picker] finish ignored: no active session");
        return;
    };
    timeout.cancel(session.token);

    restore_window_on_main(app);
    emit_status(app, session.restore_status.clone());
    if let Err(error) = app.emit(
        "mouse_position_picked",
        serde_json::json!({ "rowId": session.row_id, "x": x, "y": y }),
    ) {
        log::error!("[mouse_picker] failed to emit result: {}", error);
    }
}

pub fn cancel_pick(app: &AppHandle, state: &SharedState, reason: &str) -> bool {
    let Some((session, timeout)) = take_session(state, None) else {
        return false;
    };
    timeout.cancel(session.token);
    finish_cancel(app, session, reason, false);
    true
}

pub fn fail_pick(app: &AppHandle, state: &SharedState, message: &str) {
    let Some((session, timeout)) = take_session(state, None) else {
        return;
    };
    timeout.cancel(session.token);
    finish_cancel(app, session, message, true);
}

pub fn timeout_pick(app: &AppHandle, state: &SharedState, token: u64) {
    let Some((session, _timeout)) = take_session(state, Some(token)) else {
        return;
    };
    finish_cancel(app, session, "timeout", false);
}

fn take_session(
    state: &SharedState,
    expected_token: Option<u64>,
) -> Option<(PickSession, PickerTimeoutHandle)> {
    let mut app_state = state.lock().ok()?;
    let session = app_state.pick_session.as_ref()?;
    if expected_token.is_some() && expected_token != Some(session.token) {
        return None;
    }

    let session = app_state.pick_session.take()?;
    app_state.release_activity(Activity::PickingMouse);
    let timeout = app_state.picker_timeout.clone()?;
    Some((session, timeout))
}

fn rollback_unstarted_pick(state: &SharedState, token: u64) {
    if let Ok(mut app_state) = state.lock() {
        let matches = app_state.pick_session.as_ref().map(|session| session.token) == Some(token);
        if matches && app_state.pick_session.take().is_some() {
            app_state.release_activity(Activity::PickingMouse);
        }
    }
}

fn finish_cancel(app: &AppHandle, session: PickSession, reason: &str, failed: bool) {
    restore_window_on_main(app);
    emit_status(app, session.restore_status);
    let event_name = if failed {
        "mouse_pick_failed"
    } else {
        "mouse_pick_cancelled"
    };
    if let Err(error) = app.emit(
        event_name,
        serde_json::json!({ "rowId": session.row_id, "reason": reason }),
    ) {
        log::error!("[mouse_picker] failed to emit {}: {}", event_name, error);
    }
}

fn emit_status(app: &AppHandle, status: RuntimeStatus) {
    if let Err(error) = app.emit(
        "runtime_status_changed",
        serde_json::json!({ "status": status }),
    ) {
        log::error!("[mouse_picker] failed to emit state: {}", error);
    }
}

fn restore_window_on_main(app: &AppHandle) {
    let app_clone = app.clone();
    if let Err(error) = app.run_on_main_thread(move || match app_clone.get_webview_window("main") {
        Some(window) => {
            if let Err(error) = window.unminimize() {
                log::warn!("[mouse_picker] unminimize failed: {}", error);
            }
            if let Err(error) = window.show() {
                log::error!("[mouse_picker] window show failed: {}", error);
            }
            if let Err(error) = window.set_focus() {
                log::warn!("[mouse_picker] focus failed: {}", error);
            }
        }
        None => log::error!("[mouse_picker] main window not found during restore"),
    }) {
        log::error!(
            "[mouse_picker] main-thread restore dispatch failed: {}",
            error
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_service_replaces_old_token_and_cancels_current() {
        let (tx, rx) = mpsc::channel();
        let handle = PickerTimeoutHandle::spawn(move |token| {
            let _ = tx.send(token);
        })
        .unwrap();

        handle
            .inner
            .command_tx
            .send(TimeoutCommand::Arm {
                token: 1,
                timeout: Duration::from_millis(10),
            })
            .unwrap();
        handle
            .inner
            .command_tx
            .send(TimeoutCommand::Arm {
                token: 2,
                timeout: Duration::from_millis(10),
            })
            .unwrap();
        handle.cancel(2);
        assert!(rx.recv_timeout(Duration::from_millis(30)).is_err());
    }
}
