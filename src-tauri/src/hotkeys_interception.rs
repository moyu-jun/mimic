// Interception 驱动热键监听 — DESIGN 8.3
//
// 基于 Interception 驱动实现全局热键监听，支持所有按键包括修饰键（Ctrl/Alt/Shift）。
// 替代 tauri-plugin-global-shortcut 以突破 Windows RegisterHotKey API 限制。

use crate::simulation::action::{Action, ActionSequence};
use crate::simulation::event::MouseButton;
use crate::simulation::executor::Scheduler;
use crate::simulation::keyboard::KeyAction;
use crate::simulation::mouse::MouseAction;
use crate::state::{RuntimeStatus, SendInterception, SharedState};
use interception::{KeyState, ScanCode, Stroke};
use log::{error, info};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

/// 启动热键监听线程 — DESIGN 8.3
///
/// 长生命周期线程：循环调用 wait() → receive() → 匹配热键 → 处理 → send()（或阻断）。
/// 热键配置从 AppState.config.hotkeys 动态读取，无需重启监听线程。
/// 实现状态机门控（Idle → Running*）与页面过滤（keyboard/mouse）。
pub fn start_hotkey_listener(
    app: AppHandle,
    state: SharedState,
    ctx: Arc<Mutex<Option<SendInterception>>>,
) -> Result<(), String> {
    std::thread::spawn(move || {
        info!("[hotkeys_interception] listener thread started");

        // 设置键盘事件过滤器（仅一次，在循环外）
        let filter_set = {
            let ctx_guard = match ctx.lock() {
                Ok(g) => g,
                Err(e) => {
                    error!(
                        "[hotkeys_interception] failed to lock context for filter: {}",
                        e
                    );
                    return;
                }
            };

            match ctx_guard.as_ref() {
                Some(i) => {
                    use interception::{Filter, KeyFilter, MouseFilter};
                    // Predicate: 匹配所有键盘设备
                    extern "C" fn is_keyboard_device(device: i32) -> bool {
                        interception::is_keyboard(device)
                    }
                    // Predicate: 匹配所有鼠标设备
                    extern "C" fn is_mouse_device(device: i32) -> bool {
                        interception::is_mouse(device)
                    }
                    // 键盘过滤器: DOWN + UP 事件（热键监听）
                    i.0.set_filter(
                        is_keyboard_device,
                        Filter::KeyFilter(KeyFilter::DOWN | KeyFilter::UP),
                    );
                    // 鼠标过滤器: 仅左键按下（坐标拾取用，平时透传零影响）
                    i.0.set_filter(
                        is_mouse_device,
                        Filter::MouseFilter(MouseFilter::LEFT_BUTTON_DOWN),
                    );
                    info!("[hotkeys_interception] keyboard + mouse filter set");
                    true
                }
                None => {
                    error!("[hotkeys_interception] context not available for filter setup");
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
                    error!("[hotkeys_interception] failed to lock context: {}", e);
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
                use interception::{MouseFlags, MouseState};
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
                    continue;
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
                    continue;
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
                        Some((x, y)) => {
                            crate::mouse_picker::finish_pick(&app, &state, x, y);
                        }
                        None => {
                            error!("[hotkeys_interception] GetCursorPos failed during pick");
                            // 坐标读取失败也恢复窗口，避免界面卡在 PickingMouse
                            crate::mouse_picker::finish_pick(&app, &state, 0, 0);
                        }
                    }
                }
                continue;
            }

            if !interception::is_keyboard(device) {
                continue;
            }

            // 接收事件
            let mut strokes = [Stroke::Keyboard {
                code: ScanCode::Esc,
                state: KeyState::empty(),
                information: 0,
            }; 16];

            let count = interception.receive(device, &mut strokes);
            if count == 0 {
                continue;
            }

            // 处理每个 stroke
            for stroke in strokes.iter().take(count as usize) {
                match stroke {
                    Stroke::Keyboard {
                        code,
                        state: key_state,
                        ..
                    } => {
                        // 仅处理按下事件（忽略抬起）— DESIGN 8.3
                        if key_state.contains(KeyState::UP) {
                            interception.send(device, &[*stroke]);
                            continue;
                        }

                        // 读取当前热键配置
                        let (start_scan_code, stop_scan_code, current_page, runtime_status) = {
                            let app_state = match state.lock() {
                                Ok(s) => s,
                                Err(e) => {
                                    error!("[hotkeys_interception] failed to lock state: {}", e);
                                    interception.send(device, &[*stroke]);
                                    continue;
                                }
                            };
                            (
                                app_state.config.hotkeys.start.scan_code,
                                app_state.config.hotkeys.stop.scan_code,
                                app_state.current_page.clone(),
                                app_state.runtime_status.clone(),
                            )
                        };

                        // 统一热键匹配逻辑 — 支持启动和停止键相同的 toggle 场景
                        let is_start_key = *code as u16 == start_scan_code;
                        let is_stop_key = *code as u16 == stop_scan_code;

                        if is_start_key || is_stop_key {
                            // 诊断日志 — 热键匹配成功时记录上下文
                            info!(
                                "[hotkeys_interception] hotkey matched: code={}, start_code={}, stop_code={}, page={}, status={:?}",
                                *code as u16, start_scan_code, stop_scan_code, current_page, runtime_status
                            );

                            // 页面过滤 — REQUIREMENTS 3.6
                            if current_page.as_str() != "keyboard"
                                && current_page.as_str() != "mouse"
                            {
                                info!(
                                    "[hotkeys_interception] hotkey blocked by page filter: current_page={}",
                                    current_page
                                );
                                interception.send(device, &[*stroke]);
                                continue;
                            }

                            // 状态机门控：根据当前状态决定行为（支持 toggle）
                            match runtime_status {
                                RuntimeStatus::Idle
                                | RuntimeStatus::ReadyKeyboard
                                | RuntimeStatus::ReadyMouse
                                    if is_start_key =>
                                {
                                    // Idle/Ready* 状态下按启动键 → 启动模拟
                                    info!(
                                        "[hotkeys_interception] state machine: START branch matched, calling handle_start_hotkey"
                                    );
                                    handle_start_hotkey(&app, &state, current_page.as_str());
                                    // 阻断热键事件，不透传到系统
                                    continue;
                                }
                                RuntimeStatus::RunningKeyboard | RuntimeStatus::RunningMouse
                                    if is_stop_key =>
                                {
                                    // Running 状态下按停止键 → 停止模拟
                                    info!(
                                        "[hotkeys_interception] state machine: STOP branch matched"
                                    );
                                    handle_stop_hotkey(&app, &state);
                                    // 阻断热键事件
                                    continue;
                                }
                                RuntimeStatus::Idle if is_stop_key => {
                                    // Idle 状态下按停止键 → 阻断（不透传）
                                    info!("[hotkeys_interception] state machine: IDLE+STOP branch, ignoring");
                                    continue;
                                }
                                _ => {
                                    // 状态不匹配（如 Running 时按启动键），透传
                                    info!(
                                        "[hotkeys_interception] state machine: FALLTHROUGH branch (no match), passing through. is_start_key={}, is_stop_key={}",
                                        is_start_key, is_stop_key
                                    );
                                    interception.send(device, &[*stroke]);
                                    continue;
                                }
                            }
                        }

                        // 非热键事件，透传到系统
                        interception.send(device, &[*stroke]);
                    }
                    _ => {
                        // 非键盘事件（理论上不会到达这里），透传
                        interception.send(device, &[*stroke]);
                    }
                }
            }
        }
    });

    Ok(())
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

/// 启动热键回调 — 状态机门控 + 页面过滤 — DESIGN 8.3 / 阶段 15
fn handle_start_hotkey(app: &AppHandle, state: &SharedState, current_page: &str) {
    info!(
        "[hotkeys_interception] handle_start_hotkey called: current_page={}",
        current_page
    );
    if current_page == "keyboard" {
        handle_start_keyboard(app, state);
    } else {
        handle_start_mouse(app, state);
    }
}

/// 按键模拟启动分支
fn handle_start_keyboard(app: &AppHandle, state: &SharedState) {
    let new_status = RuntimeStatus::RunningKeyboard;
    info!("[hotkeys_interception] start triggered: Idle -> RunningKeyboard");

    let (enabled, event_tx, stop_flag) = {
        let mut app_state = match state.lock() {
            Ok(s) => s,
            Err(e) => {
                error!(
                    "[hotkeys_interception] start_keyboard: failed to lock state: {}",
                    e
                );
                return;
            }
        };
        app_state.runtime_status = new_status.clone();
        app_state
            .stop_flag
            .store(false, std::sync::atomic::Ordering::Relaxed);
        let enabled = app_state
            .config
            .keyboard_configs
            .iter()
            .filter(|c| c.enabled)
            .cloned()
            .collect::<Vec<_>>();
        (
            enabled,
            app_state.event_tx.clone(),
            app_state.stop_flag.clone(),
        )
    };

    // 启动提示音 — 仅在确实有勾选动作（真正进入循环）时播放；
    // 尽早调用（emit 之前），使音频设备初始化与 IPC 事件派发并行，降低感知延迟。
    // 空列表会在下方 worker 线程内立即回退 Idle，不算启动生效，不播放。
    if !enabled.is_empty() {
        crate::sound::play_start();
    }

    if let Err(e) = app.emit(
        "runtime_status_changed",
        serde_json::json!({ "status": new_status }),
    ) {
        error!(
            "[hotkeys_interception] failed to emit runtime_status_changed: {}",
            e
        );
    }

    let app_clone = app.clone();
    let state_clone = state.clone();
    std::thread::spawn(move || {
        info!(
            "[hotkeys_interception] keyboard simulation loop started, {} configs",
            enabled.len()
        );

        if enabled.is_empty() {
            info!("[hotkeys_interception] no enabled keyboard configs, stopping immediately");
            if let Ok(mut s) = state_clone.lock() {
                s.runtime_status = RuntimeStatus::Idle;
            }
            let _ = app_clone.emit(
                "runtime_status_changed",
                serde_json::json!({ "status": RuntimeStatus::Idle }),
            );
            return;
        }

        // 构建动作序列：每个启用配置 → 对应 Action + 执行后间隔
        let mut sequence = ActionSequence::new();
        for cfg in &enabled {
            let action = match cfg.action_type {
                crate::config::KeyActionType::Press => Action::Keyboard(KeyAction::Press {
                    scan_code: cfg.scan_code,
                }),
                crate::config::KeyActionType::Hold => Action::Keyboard(KeyAction::Hold {
                    scan_code: cfg.scan_code,
                    duration_ms: cfg.hold_duration_ms.unwrap_or(100),
                }),
                crate::config::KeyActionType::Combo => Action::Keyboard(KeyAction::Combo {
                    modifiers: cfg.modifiers.clone(),
                    key: cfg.scan_code,
                }),
            };
            sequence.add(action, cfg.interval_ms);
        }

        // Scheduler 循环发送事件，间隔以 Delay 事件交由 worker 串行执行
        Scheduler::new(event_tx).execute_loop(&sequence, &stop_flag);
    });
}

/// 鼠标模拟启动分支 — ARCHITECTURE v3.0 重构
///
/// 列表为空或全部坐标为 null 时直接忽略热键。
fn handle_start_mouse(app: &AppHandle, state: &SharedState) {
    // 前置检查：先取出有效配置
    let valid: Vec<crate::config::MouseConfig> = {
        let app_state = match state.lock() {
            Ok(s) => s,
            Err(e) => {
                error!(
                    "[hotkeys_interception] start_mouse: failed to lock state: {}",
                    e
                );
                return;
            }
        };
        app_state
            .config
            .mouse_configs
            .iter()
            .filter(|c| c.enabled && c.x.is_some() && c.y.is_some())
            .cloned()
            .collect()
    };

    if valid.is_empty() {
        info!(
            "[hotkeys_interception] mouse start ignored: no valid configs (empty or all disabled/null)"
        );
        return;
    }

    // 有有效配置，正式启动模拟
    let new_status = RuntimeStatus::RunningMouse;
    info!(
        "[hotkeys_interception] start triggered: -> RunningMouse, {} valid configs",
        valid.len()
    );

    let (event_tx, stop_flag) = {
        let mut app_state = match state.lock() {
            Ok(s) => s,
            Err(e) => {
                error!(
                    "[hotkeys_interception] start_mouse: failed to lock state: {}",
                    e
                );
                return;
            }
        };
        app_state.runtime_status = new_status.clone();
        app_state
            .stop_flag
            .store(false, std::sync::atomic::Ordering::Relaxed);
        (app_state.event_tx.clone(), app_state.stop_flag.clone())
    };

    // 启动提示音 — 已确认有有效坐标，真正进入鼠标模拟循环。
    // 尽早调用（emit 之前），使音频设备初始化与 IPC 事件派发并行，降低感知延迟。
    crate::sound::play_start();

    if let Err(e) = app.emit(
        "runtime_status_changed",
        serde_json::json!({ "status": new_status }),
    ) {
        error!(
            "[hotkeys_interception] failed to emit runtime_status_changed: {}",
            e
        );
    }

    std::thread::spawn(move || {
        info!(
            "[hotkeys_interception] mouse simulation loop started, {} valid configs",
            valid.len()
        );

        // 构建动作序列：每个有效配置 → 对应 MouseAction + 执行后间隔
        let mut sequence = ActionSequence::new();
        for cfg in &valid {
            let action = match cfg.action_type {
                crate::config::MouseActionType::ClickLeft => {
                    let (x, y) = (cfg.x.unwrap(), cfg.y.unwrap());
                    Action::Mouse(MouseAction::Click {
                        button: MouseButton::Left,
                        x,
                        y,
                    })
                }
                crate::config::MouseActionType::ClickRight => {
                    let (x, y) = (cfg.x.unwrap(), cfg.y.unwrap());
                    Action::Mouse(MouseAction::Click {
                        button: MouseButton::Right,
                        x,
                        y,
                    })
                }
                crate::config::MouseActionType::ClickMiddle => {
                    let (x, y) = (cfg.x.unwrap(), cfg.y.unwrap());
                    Action::Mouse(MouseAction::Click {
                        button: MouseButton::Middle,
                        x,
                        y,
                    })
                }
                crate::config::MouseActionType::ScrollUp => Action::Mouse(MouseAction::Scroll {
                    delta: cfg.scroll_delta.unwrap_or(1),
                }),
                crate::config::MouseActionType::ScrollDown => Action::Mouse(MouseAction::Scroll {
                    delta: -cfg.scroll_delta.unwrap_or(1),
                }),
                crate::config::MouseActionType::Drag => {
                    let from = (cfg.x.unwrap(), cfg.y.unwrap());
                    let to = (
                        cfg.drag_to_x.unwrap_or(from.0),
                        cfg.drag_to_y.unwrap_or(from.1),
                    );
                    Action::Mouse(MouseAction::Drag {
                        button: MouseButton::Left,
                        from,
                        to,
                    })
                }
            };
            sequence.add(action, cfg.interval_ms);
        }

        Scheduler::new(event_tx).execute_loop(&sequence, &stop_flag);
    });
}

/// 停止热键回调 — 状态机门控 — DESIGN 8.3
fn handle_stop_hotkey(app: &AppHandle, state: &SharedState) {
    info!("[hotkeys_interception] stop triggered: Running* -> Idle");

    // 停止提示音 — 本函数仅在 Running* 状态下按停止键时调用，即停止真正生效。
    crate::sound::play_stop();

    // 设置停止标记
    {
        let app_state = match state.lock() {
            Ok(s) => s,
            Err(e) => {
                error!("[hotkeys_interception] stop: failed to lock state: {}", e);
                return;
            }
        };
        app_state
            .stop_flag
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    // 等待一小段时间让模拟循环退出
    std::thread::sleep(std::time::Duration::from_millis(50));

    // 更新状态
    {
        let mut app_state = match state.lock() {
            Ok(s) => s,
            Err(e) => {
                error!(
                    "[hotkeys_interception] stop: failed to lock state after wait: {}",
                    e
                );
                return;
            }
        };
        app_state.runtime_status = RuntimeStatus::Idle;
    }

    // 发送 runtime_status_changed 事件
    if let Err(e) = app.emit(
        "runtime_status_changed",
        serde_json::json!({ "status": RuntimeStatus::Idle }),
    ) {
        error!(
            "[hotkeys_interception] failed to emit runtime_status_changed: {}",
            e
        );
    }
}
