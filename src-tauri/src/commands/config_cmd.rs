//! 配置命令边界。

use crate::config::{self, AppConfig, LogLevel};
use crate::error::{CommandError, CommandResult};
use crate::state::{Activity, ActivityLease, SharedState};

/// 返回内存中的当前配置。
#[tauri::command]
pub fn load_config(state: tauri::State<SharedState>) -> CommandResult<AppConfig> {
    let app_state = state
        .inner()
        .lock()
        .map_err(|error| CommandError::from(format!("Failed to lock state: {error}")))?;
    Ok(app_state.config.clone())
}

/// 保存完整配置。事务锁串行化写者，活动租约阻止运行任务与写盘交叠。
#[tauri::command]
pub fn persist_config(config: AppConfig, state: tauri::State<SharedState>) -> CommandResult<()> {
    let _transaction = config::transaction_guard()?;
    let log_level =
        commit_candidate(state.inner(), config, config::save).map_err(CommandError::from)?;
    log::set_max_level(log_level.as_filter());
    Ok(())
}

/// 单独更新日志等级；事务锁覆盖候选读取、原子写盘和内存提交。
#[tauri::command]
pub fn update_log_level(level: LogLevel, state: tauri::State<SharedState>) -> CommandResult<()> {
    let _transaction = config::transaction_guard()?;
    let mut candidate = state
        .inner()
        .lock()
        .map_err(|error| CommandError::from(format!("Failed to lock state: {error}")))?
        .config
        .clone();
    candidate.log_level = level;

    commit_candidate(state.inner(), candidate, config::save).map_err(CommandError::from)?;
    log::set_max_level(level.as_filter());
    log::info!("[update_log_level] log level changed to {:?}", level);
    Ok(())
}

fn commit_candidate(
    state: &SharedState,
    candidate: AppConfig,
    persist: impl FnOnce(&AppConfig) -> Result<(), String>,
) -> Result<LogLevel, String> {
    let _activity = ActivityLease::acquire(state, Activity::PersistingConfig)?;
    persist(&candidate).map_err(|error| {
        log::error!("[config] candidate persist failed: {error}");
        error
    })?;

    let log_level = candidate.log_level;
    state
        .lock()
        .map_err(|error| format!("Failed to lock state: {error}"))?
        .config = candidate;
    Ok(log_level)
}

/// 读取启动时配置写盘失败的警告。
#[tauri::command]
pub fn get_init_warning(state: tauri::State<SharedState>) -> Option<String> {
    state.inner().lock().ok()?.config_warning.clone()
}

#[cfg(test)]
mod tests {
    use super::commit_candidate;
    use crate::state::{Activity, AppState, DriverStatus, PageId, RuntimeHealth, SharedState};
    use std::sync::{Arc, Mutex};

    fn test_state() -> SharedState {
        Arc::new(Mutex::new(AppState {
            config: crate::config::default_config(),
            config_warning: None,
            navigation: PageId::Home,
            activity: Activity::Idle,
            simulation_mode: None,
            runtime_health: RuntimeHealth::Healthy,
            driver_status: DriverStatus::Ready,
            pick_session: None,
            next_pick_token: 1,
            picker_timeout: None,
            active_custom_sequence_id: None,
            runtime: None,
            recording: crate::sound_recorder::new_handle(),
            recording_buffer: Arc::new(Mutex::new(None)),
        }))
    }

    #[test]
    fn failed_persist_keeps_memory_and_releases_activity() {
        let state = test_state();
        let original = state.lock().unwrap().config.clone();
        let mut candidate = original.clone();
        candidate.log_level = crate::config::LogLevel::Debug;

        let error = commit_candidate(&state, candidate, |_| Err("injected write failure".into()))
            .unwrap_err();
        assert!(error.contains("injected"));
        let state = state.lock().unwrap();
        assert_eq!(state.config, original);
        assert_eq!(state.activity, Activity::Idle);
    }

    #[test]
    fn successful_persist_commits_memory_and_releases_activity() {
        let state = test_state();
        let mut candidate = state.lock().unwrap().config.clone();
        candidate.log_level = crate::config::LogLevel::Debug;

        commit_candidate(&state, candidate.clone(), |_| Ok(())).unwrap();
        let state = state.lock().unwrap();
        assert_eq!(state.config, candidate);
        assert_eq!(state.activity, Activity::Idle);
    }
}
