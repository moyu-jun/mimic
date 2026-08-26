//! 应用状态投影与服务句柄。
//!
//! Navigation、Activity、SimulationMode 和 RuntimeHealth 是事实来源；RuntimeStatus 仅是
//! 对前端兼容的派生 DTO，不再参与后端状态控制。

use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

use crate::config::AppConfig;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PageId {
    Home,
    Keyboard,
    Mouse,
    Custom,
    Settings,
}

impl TryFrom<&str> for PageId {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "home" => Ok(Self::Home),
            "keyboard" => Ok(Self::Keyboard),
            "mouse" => Ok(Self::Mouse),
            "custom" => Ok(Self::Custom),
            "settings" => Ok(Self::Settings),
            _ => Err(format!("invalid page: {value}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activity {
    Idle,
    Simulating,
    Recording,
    PickingMouse,
    DriverMaintenance,
    PersistingConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimulationMode {
    Keyboard,
    Mouse,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeHealth {
    Healthy,
    Degraded { capability: &'static str },
    Error { code: &'static str },
}

/// 前端兼容状态 DTO。只能通过 `AppState::runtime_status()` 派生。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RuntimeStatus {
    Idle,
    ReadyKeyboard,
    ReadyMouse,
    RunningKeyboard,
    RunningMouse,
    ReadyCustom,
    RunningCustom,
    PickingMouse,
    Recording,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DriverStatus {
    NotInstalled,
    InstalledNeedReboot,
    Ready,
    Error,
}

pub struct AppState {
    pub config: AppConfig,
    pub config_warning: Option<String>,
    pub navigation: PageId,
    pub activity: Activity,
    pub simulation_mode: Option<SimulationMode>,
    pub runtime_health: RuntimeHealth,
    pub driver_status: DriverStatus,
    pub pick_session: Option<crate::mouse_picker::PickSession>,
    pub next_pick_token: u64,
    pub picker_timeout: Option<crate::mouse_picker::PickerTimeoutHandle>,
    pub active_custom_sequence_id: Option<String>,
    pub runtime: Option<crate::runtime::RuntimeHandle>,
    pub recording: crate::sound_recorder::RecordingHandle,
    pub recording_buffer: RecordingBuffer,
}

impl AppState {
    pub fn runtime_status(&self) -> RuntimeStatus {
        if matches!(self.runtime_health, RuntimeHealth::Error { .. }) {
            return RuntimeStatus::Error;
        }
        match self.activity {
            Activity::Simulating => match self.simulation_mode {
                Some(SimulationMode::Keyboard) => RuntimeStatus::RunningKeyboard,
                Some(SimulationMode::Mouse) => RuntimeStatus::RunningMouse,
                Some(SimulationMode::Custom) => RuntimeStatus::RunningCustom,
                None => RuntimeStatus::Error,
            },
            Activity::Recording => RuntimeStatus::Recording,
            Activity::PickingMouse => RuntimeStatus::PickingMouse,
            Activity::DriverMaintenance => RuntimeStatus::Idle,
            Activity::Idle | Activity::PersistingConfig => match self.navigation {
                PageId::Keyboard => RuntimeStatus::ReadyKeyboard,
                PageId::Mouse => RuntimeStatus::ReadyMouse,
                PageId::Custom if self.active_custom_sequence_id.is_some() => {
                    RuntimeStatus::ReadyCustom
                }
                _ => RuntimeStatus::Idle,
            },
        }
    }

    pub fn acquire_activity(&mut self, activity: Activity) -> Result<(), String> {
        if !matches!(self.activity, Activity::Idle) {
            return Err(format!("busy: {:?} is active", self.activity));
        }
        if !matches!(
            activity,
            Activity::DriverMaintenance | Activity::PersistingConfig
        ) && matches!(self.runtime_health, RuntimeHealth::Error { .. })
        {
            return Err("runtime is in error state".to_string());
        }
        self.activity = activity;
        Ok(())
    }

    pub fn release_activity(&mut self, expected: Activity) {
        if self.activity == expected {
            self.activity = Activity::Idle;
            if expected == Activity::Simulating {
                self.simulation_mode = None;
            }
        }
    }
}

impl Drop for AppState {
    fn drop(&mut self) {
        if let Some(runtime) = &self.runtime {
            if let Err(error) = runtime.shutdown() {
                log::error!("[state] runtime shutdown during AppState drop failed: {error}");
            }
        }
    }
}

pub type RecordingBuffer = Arc<Mutex<Option<(Vec<i16>, u32)>>>;
pub type SharedState = Arc<Mutex<AppState>>;

/// 对跨函数局部活动提供异常安全的自动释放。
pub struct ActivityLease {
    state: std::sync::Weak<Mutex<AppState>>,
    activity: Activity,
    armed: bool,
}

impl ActivityLease {
    pub fn acquire(state: &SharedState, activity: Activity) -> Result<Self, String> {
        state
            .lock()
            .map_err(|error| format!("Failed to lock state: {error}"))?
            .acquire_activity(activity)?;
        Ok(Self {
            state: Arc::downgrade(state),
            activity,
            armed: true,
        })
    }

    fn release_inner(&mut self) {
        if !self.armed {
            return;
        }
        self.armed = false;
        if let Some(state) = self.state.upgrade() {
            if let Ok(mut state) = state.lock() {
                state.release_activity(self.activity);
            }
        }
    }
}

impl Drop for ActivityLease {
    fn drop(&mut self) {
        self.release_inner();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_list_and_detail_have_distinct_legacy_status() {
        let mut state = test_state();
        state.navigation = PageId::Custom;
        assert_eq!(state.runtime_status(), RuntimeStatus::Idle);
        state.active_custom_sequence_id = Some("sequence".to_string());
        assert_eq!(state.runtime_status(), RuntimeStatus::ReadyCustom);
    }

    #[test]
    fn activity_matrix_rejects_every_second_activity() {
        for first in [
            Activity::Simulating,
            Activity::Recording,
            Activity::PickingMouse,
            Activity::DriverMaintenance,
            Activity::PersistingConfig,
        ] {
            let mut state = test_state();
            state.acquire_activity(first).unwrap();
            for second in [
                Activity::Simulating,
                Activity::Recording,
                Activity::PickingMouse,
                Activity::DriverMaintenance,
                Activity::PersistingConfig,
            ] {
                assert!(state.acquire_activity(second).is_err());
            }
            state.release_activity(first);
            assert_eq!(state.activity, Activity::Idle);
        }
    }

    #[test]
    fn generated_activity_sequences_preserve_single_owner_invariant() {
        let activities = [
            Activity::Simulating,
            Activity::Recording,
            Activity::PickingMouse,
            Activity::DriverMaintenance,
            Activity::PersistingConfig,
        ];

        for seed in 0..256_u64 {
            let state = Arc::new(Mutex::new(test_state()));
            let mut lease: Option<ActivityLease> = None;
            let mut random = seed.wrapping_add(0xd1b5_4a32_d192_ed03);

            for _ in 0..256 {
                random ^= random << 13;
                random ^= random >> 7;
                random ^= random << 17;
                let activity = activities[(random as usize) % activities.len()];

                if random & 1 == 0 {
                    if lease.is_none() {
                        lease = Some(ActivityLease::acquire(&state, activity).unwrap());
                    } else {
                        assert!(ActivityLease::acquire(&state, activity).is_err());
                    }
                } else {
                    drop(lease.take());
                }

                let current = state.lock().unwrap().activity;
                assert_eq!(
                    current == Activity::Idle,
                    lease.is_none(),
                    "activity ownership diverged for seed {seed}"
                );
            }
            drop(lease);
            assert_eq!(state.lock().unwrap().activity, Activity::Idle);
        }
    }

    #[test]
    fn activity_lease_releases_on_drop() {
        let state = Arc::new(Mutex::new(test_state()));
        let lease = ActivityLease::acquire(&state, Activity::Recording).unwrap();
        assert_eq!(state.lock().unwrap().activity, Activity::Recording);
        drop(lease);
        assert_eq!(state.lock().unwrap().activity, Activity::Idle);
    }
    fn test_state() -> AppState {
        AppState {
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
        }
    }
}
