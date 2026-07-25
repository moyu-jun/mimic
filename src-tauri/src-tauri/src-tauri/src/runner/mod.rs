// 运行器层 — ARCHITECTURE v3.0 阶段 B
//
// 封装一次模拟运行的完整生命周期，消除键盘/鼠标启动分支的重复。

pub mod builder;

use crate::simulation::action::ActionSequence;
use crate::simulation::executor::Scheduler;
use crate::sound;
use crate::state::{RuntimeStatus, SharedState};
use builder::SequenceBuilder;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// 封装一次模拟运行的完整生命周期，替代两个重复的 handle_start_* 分支。
///
/// start 流程（参数化，键鼠混合共用）：
///   1. builder.build(config) → 若 None 直接忽略（不切状态/不播音/不 emit）
///   2. 置 running_status + 清 stop_flag
///   3. play_start() + emit runtime_status_changed
///   4. spawn 生产者线程：Scheduler::execute_loop(sequence, stop_flag)
pub struct SimulationRunner;

impl SimulationRunner {
    /// 启动模拟运行
    pub fn start(
        app: &AppHandle,
        state: &SharedState,
        builder: &dyn SequenceBuilder,
    ) -> Result<(), String> {
        // 1. 构建序列
        let sequence = {
            let app_state = state
                .lock()
                .map_err(|e| format!("Failed to lock state: {}", e))?;
            builder.build(&app_state.config)
        };

        let sequence = match sequence {
            Some(seq) => seq,
            None => {
                // 无有效动作，直接返回（不切状态/不播音/不 emit）
                log::info!("[runner] no valid actions, ignoring start request");
                return Ok(());
            }
        };

        // 2. 更新状态
        let (running_status, event_tx, stop_flag) = {
            let mut app_state = state
                .lock()
                .map_err(|e| format!("Failed to lock state: {}", e))?;
            
            let running_status = builder.running_status();
            app_state.runtime_status = running_status.clone();
            app_state.stop_flag.store(false, Ordering::Release);

            (
                running_status,
                app_state.event_tx.clone(),
                app_state.stop_flag.clone(),
            )
        };

        // 3. 播放提示音 + emit
        sound::play_start();
        if let Err(e) = app.emit(
            "runtime_status_changed",
            serde_json::json!({ "status": running_status }),
        ) {
            log::error!("[runner] failed to emit runtime_status_changed: {}", e);
        }

        log::info!("[runner] simulation started, status={:?}", running_status);

        // 4. spawn 生产者线程
        thread::spawn(move || {
            Scheduler::execute_loop(sequence, event_tx, stop_flag);
            log::info!("[runner] producer thread exited");
        });

        Ok(())
    }

    /// 停止模拟运行
    pub fn stop(app: &AppHandle, state: &SharedState) -> Result<(), String> {
        let current_status = {
            let app_state = state
                .lock()
                .map_err(|e| format!("Failed to lock state: {}", e))?;
            app_state.runtime_status.clone()
        };

        // 仅 Running* 状态可停止
        match current_status {
            RuntimeStatus::RunningKeyboard | RuntimeStatus::RunningMouse => {}
            _ => return Err("Not running".to_string()),
        }

        // 置 stop_flag
        {
            let app_state = state
                .lock()
                .map_err(|e| format!("Failed to lock state: {}", e))?;
            app_state.stop_flag.store(true, Ordering::Release);
        }

        // 短等待让生产者线程退出
        thread::sleep(Duration::from_millis(100));

        // 切回 Idle
        {
            let mut app_state = state
                .lock()
                .map_err(|e| format!("Failed to lock state: {}", e))?;
            app_state.runtime_status = RuntimeStatus::Idle;
        }

        // 播放提示音 + emit
        sound::play_stop();
        if let Err(e) = app.emit(
            "runtime_status_changed",
            serde_json::json!({ "status": RuntimeStatus::Idle }),
        ) {
            log::error!("[runner] failed to emit runtime_status_changed: {}", e);
        }

        log::info!("[runner] simulation stopped");
        Ok(())
    }
}
