// 运行器层 — ARCHITECTURE v3.0 阶段 B
//
// SimulationRunner 封装一次模拟运行的完整生命周期（启动→循环→停止），
// 替代原 hotkeys_interception.rs 中重复的 handle_start_keyboard / handle_start_mouse。
// 监听层只需按当前页选一个 SequenceBuilder 传入，键鼠混合共用同一条链路。

mod builder;

pub use builder::{
    CustomSequenceBuilder, KeyboardSequenceBuilder, MouseSequenceBuilder, SequenceBuilder,
};

use crate::simulation::executor::Scheduler;
use crate::state::{RuntimeStatus, SharedState};
use log::{error, info};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter};

pub struct SimulationRunner;

impl SimulationRunner {
    /// 启动一次模拟运行（参数化，键鼠混合共用）：
    ///   1. builder.build(config) → 若 None 直接忽略（不切状态/不播音/不 emit）
    ///   2. 置 running_status + 清 stop_flag
    ///   3. play_start() + emit runtime_status_changed
    ///   4. spawn 生产者线程：Scheduler::execute_loop(sequence, stop_flag)
    pub fn start(app: &AppHandle, state: &SharedState, builder: &dyn SequenceBuilder) {
        let new_status = builder.running_status();
        info!("[runner] start called: target_status={:?}", new_status);

        // 先构建序列并取出运行所需句柄；None 表示无有效动作，静默忽略本次启动。
        let (sequence, event_tx, stop_flag) = {
            let mut app_state = match state.lock() {
                Ok(s) => {
                    info!(
                        "[runner] current state: page={}, status={:?}, active_custom_id={:?}",
                        s.current_page, s.runtime_status, s.active_custom_sequence_id
                    );
                    s
                }
                Err(e) => {
                    error!("[runner] start: failed to lock state: {}", e);
                    return;
                }
            };

            info!("[runner] calling builder.build()...");
            let sequence = match builder.build(&app_state.config) {
                Some(seq) => {
                    info!("[runner] builder.build() returned {} steps", seq.steps.len());
                    seq
                }
                None => {
                    info!("[runner] start ignored: builder produced no valid actions");
                    return;
                }
            };

            app_state.runtime_status = new_status.clone();
            app_state.stop_flag.store(false, Ordering::Relaxed);
            (
                sequence,
                app_state.event_tx.clone(),
                app_state.stop_flag.clone(),
            )
        };

        info!("[runner] start triggered: -> {:?}", new_status);

        // 启动提示音 — 已确认有有效动作，真正进入模拟循环。
        // 尽早调用（emit 之前），使音频设备初始化与 IPC 事件派发并行，降低感知延迟。
        crate::sound::play_start();

        if let Err(e) = app.emit(
            "runtime_status_changed",
            serde_json::json!({ "status": new_status }),
        ) {
            error!("[runner] failed to emit runtime_status_changed: {}", e);
        }

        std::thread::spawn(move || {
            info!(
                "[runner] simulation loop started, {} steps",
                sequence.steps.len()
            );
            Scheduler::new(event_tx).execute_loop(&sequence, &stop_flag);
        });
    }

    /// 停止当前模拟运行：置 stop_flag → 短等待 → 回 Idle + play_stop + emit。
    pub fn stop(app: &AppHandle, state: &SharedState) {
        info!("[runner] stop triggered: Running* -> Idle");

        // 停止提示音 — 本函数仅在 Running* 状态下调用，即停止真正生效。
        crate::sound::play_stop();

        // 设置停止标记
        {
            let app_state = match state.lock() {
                Ok(s) => s,
                Err(e) => {
                    error!("[runner] stop: failed to lock state: {}", e);
                    return;
                }
            };
            app_state.stop_flag.store(true, Ordering::Relaxed);
        }

        // 等待一小段时间让模拟循环退出
        std::thread::sleep(std::time::Duration::from_millis(50));

        // 更新状态 — 自定义序列停止后回 ReadyCustom（仍在详情页，可再启动）；其余回 Idle。
        let new_status = {
            let mut app_state = match state.lock() {
                Ok(s) => s,
                Err(e) => {
                    error!("[runner] stop: failed to lock state after wait: {}", e);
                    return;
                }
            };
            let status = if app_state.active_custom_sequence_id.is_some() {
                RuntimeStatus::ReadyCustom
            } else {
                RuntimeStatus::Idle
            };
            app_state.runtime_status = status.clone();
            status
        };

        // 发送 runtime_status_changed 事件
        if let Err(e) = app.emit(
            "runtime_status_changed",
            serde_json::json!({ "status": new_status }),
        ) {
            error!("[runner] failed to emit runtime_status_changed: {}", e);
        }
    }
}
