//! 模拟运行入口。
//!
//! SequenceBuilder 只负责从配置生成不可变序列；所有运行生命周期由 Runtime Actor 管理。

mod builder;

pub use builder::{
    CustomSequenceBuilder, KeyboardSequenceBuilder, MouseSequenceBuilder, SequenceBuilder,
};

use crate::runtime::StopOutcome;
use crate::state::{Activity, SharedState, SimulationMode};
use log::{error, info};
use tauri::AppHandle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartOutcome {
    Started,
    NoExecutableActions,
}

pub struct SimulationRunner;

impl SimulationRunner {
    /// 构建序列并同步提交给 Runtime Actor。
    ///
    /// 无有效动作时静默忽略；Actor 确认启动后才播放启动提示音。
    pub fn start(
        _app: &AppHandle,
        state: &SharedState,
        builder: &dyn SequenceBuilder,
    ) -> Result<StartOutcome, String> {
        let mode = builder.mode();

        let (sequence, runtime) = {
            let mut app_state = state
                .lock()
                .map_err(|error| format!("Failed to lock state: {error}"))?;
            builder.validate_start(&app_state.config)?;
            let Some(sequence) = builder.build(&app_state.config) else {
                info!("[runner] start ignored: no executable actions");
                return Ok(StartOutcome::NoExecutableActions);
            };
            let runtime = app_state
                .runtime
                .clone()
                .ok_or_else(|| "runtime unavailable".to_string())?;
            app_state.acquire_activity(Activity::Simulating)?;
            app_state.simulation_mode = Some(match mode {
                crate::runtime::RuntimeMode::Keyboard => SimulationMode::Keyboard,
                crate::runtime::RuntimeMode::Mouse => SimulationMode::Mouse,
                crate::runtime::RuntimeMode::Custom => SimulationMode::Custom,
            });
            (sequence, runtime)
        };

        let run_id = runtime.start(sequence, mode).map_err(|error| {
            if let Ok(mut app_state) = state.lock() {
                app_state.release_activity(Activity::Simulating);
            }
            error!("[runner] start rejected: {}", error);
            error.to_string()
        })?;
        info!("[runner] run {} accepted: {:?}", run_id, mode);
        crate::sound::play_start();
        Ok(StartOutcome::Started)
    }

    /// 同步停止当前任务。
    ///
    /// 返回成功时 Actor 已终止旧任务并完成输入释放，不再使用固定 sleep 猜测完成。
    pub fn stop(_app: &AppHandle, state: &SharedState) -> Result<StopOutcome, String> {
        let runtime = {
            let app_state = state
                .lock()
                .map_err(|error| format!("Failed to lock state: {error}"))?;
            app_state
                .runtime
                .clone()
                .ok_or_else(|| "runtime unavailable".to_string())?
        };

        let outcome = runtime.stop().map_err(|error| error.to_string())?;
        if outcome == StopOutcome::Stopped {
            crate::sound::play_stop();
        }
        Ok(outcome)
    }
}
