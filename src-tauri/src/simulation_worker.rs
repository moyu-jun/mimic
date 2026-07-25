// 统一模拟 Worker — ARCHITECTURE v3.0
//
// 常驻后台线程，替代旧的 keyboard_worker / mouse_worker，职责：
//   1. 从 channel 接收 SimulationEvent
//   2. 状态门控（仅在 Running* 状态下执行）
//   3. 调用驱动发送事件 / 执行延迟（sleep）
//
// 职责边界：驱动通信层 + 时序控制层。所有延迟（动作内部 + 步骤间隔）都在此线程串行执行。

use crate::simulation::driver::{InputDriver, InterceptionDriver};
use crate::simulation::event::SimulationEvent;
use crate::state::{RuntimeStatus, SendInterception, SharedState};
use log::{error, info, warn};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 启动统一模拟 worker 线程
pub fn start_simulation_worker(
    rx: Receiver<SimulationEvent>,
    state: SharedState,
    ctx: Arc<Mutex<Option<SendInterception>>>,
) -> Result<(), String> {
    let driver = InterceptionDriver::new(ctx);

    std::thread::spawn(move || {
        info!("[simulation_worker] worker thread started");

        loop {
            let event = match rx.recv() {
                Ok(e) => e,
                Err(e) => {
                    warn!("[simulation_worker] channel closed: {}", e);
                    break;
                }
            };

            if matches!(event, SimulationEvent::Stop) {
                info!("[simulation_worker] received stop signal");
                break;
            }

            // 状态门控：仅在 Running* 状态下执行
            let is_running = {
                let app_state = match state.lock() {
                    Ok(s) => s,
                    Err(e) => {
                        error!("[simulation_worker] failed to lock state: {}", e);
                        continue;
                    }
                };
                matches!(
                    app_state.runtime_status,
                    RuntimeStatus::RunningKeyboard | RuntimeStatus::RunningMouse
                )
            };

            if !is_running {
                warn!("[simulation_worker] event received but not running, skipping");
                continue;
            }

            if let Err(e) = execute_event(&driver, &event) {
                error!("[simulation_worker] event execution failed: {}", e);
            }
        }

        info!("[simulation_worker] worker thread exited");
    });

    Ok(())
}

/// 执行单个事件：调用驱动 API 或 sleep。
fn execute_event<D: InputDriver>(driver: &D, event: &SimulationEvent) -> Result<(), String> {
    match event {
        SimulationEvent::KeyDown { scan_code } => driver
            .send_keyboard(*scan_code, true)
            .map_err(|e| e.to_string())?,
        SimulationEvent::KeyUp { scan_code } => driver
            .send_keyboard(*scan_code, false)
            .map_err(|e| e.to_string())?,
        SimulationEvent::MouseMove { x, y } => {
            driver.send_mouse_move(*x, *y).map_err(|e| e.to_string())?
        }
        SimulationEvent::MouseButtonDown { button } => driver
            .send_mouse_button(*button, true)
            .map_err(|e| e.to_string())?,
        SimulationEvent::MouseButtonUp { button } => driver
            .send_mouse_button(*button, false)
            .map_err(|e| e.to_string())?,
        SimulationEvent::MouseWheel { delta } => {
            driver.send_mouse_wheel(*delta).map_err(|e| e.to_string())?
        }
        SimulationEvent::Delay { ms } => {
            // 关键：所有延迟在 worker 线程串行执行
            std::thread::sleep(Duration::from_millis(*ms));
        }
        SimulationEvent::Stop => {}
    }
    Ok(())
}
