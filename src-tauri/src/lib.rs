// Mimic 应用后端入口 — DESIGN 6 / DESIGN 13.1
//
// 阶段 10：
//   - 接入 tauri-plugin-log（开发 info / release error，targets: Stdout + LogDir）
//   - setup 顺序按 DESIGN 13.1 微调：日志先于配置加载，便于后者出错时被记录
//   - 关键启动事件改用 log::{info,error,warn}

mod commands;
mod config;
mod driver;
mod error;
mod hotkeys;
mod listener;
mod mouse_picker;
mod paths;
mod runner;
mod runtime;
mod simulation;
mod sound;
mod sound_recorder;
mod state;

/// Parser-only entry points for fuzzing; they never touch files, devices, or global state.
#[doc(hidden)]
pub mod fuzzing {
    pub fn config_bytes(input: &[u8]) {
        super::config::fuzz_decode_bytes(input);
    }

    pub fn wav_bytes(input: &[u8]) {
        super::sound::fuzz_validate_wav_bytes(input);
    }
}

use config::LogLevel;
use paths::PortablePaths;
use state::{Activity, AppState, PageId, RuntimeHealth, SharedState, SimulationMode};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};
use tauri_plugin_log::{RotationStrategy, Target, TargetKind};

// 导入命令实现 — ARCHITECTURE v3.0 阶段 A
use commands::config_cmd::*;
use commands::driver_cmd::*;
use commands::pick_cmd::*;
use commands::runtime_cmd::*;
use commands::sound_cmd::*;
use commands::system_cmd::*;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 主 Tauri/WebView 进程始终保持普通权限。驱动维护和重启由独立 helper 按需请求 UAC。

    // DLL 加载策略（2026-06-12 调整）：
    // interception.dll 现在通过 build.rs 自动复制到 exe 同级目录，
    // Windows 加载器按标准搜索顺序（应用程序所在目录优先）自动加载，
    // 不再需要 SetDllDirectoryW 设置子目录搜索路径。

    // DESIGN 13.1 启动顺序（阶段 10 当前覆盖 1-2 + 权限检测）：
    //   1. 初始化日志   ← 由 plugin builder 在 setup 之前装配
    //   2. 加载/初始化 mimic.ini
    //   3. 检测驱动状态     ← 阶段 11 接入
    //   4. 注册全局热键     ← 阶段 12 接入
    //   5. 写入 SharedState
    let portable_paths = PortablePaths::current()
        .unwrap_or_else(|error| panic!("failed to resolve portable paths: {error}"));
    portable_paths
        .ensure_data_dirs()
        .unwrap_or_else(|error| panic!("failed to initialize data directories: {error}"));
    let logs_dir = portable_paths.logs_dir();

    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Debug)
                .max_file_size(5 * 1024 * 1024)
                .rotation_strategy(RotationStrategy::KeepSome(3))
                .targets([
                    Target::new(TargetKind::Stdout),
                    Target::new(TargetKind::Folder {
                        path: logs_dir,
                        file_name: None,
                    }),
                ])
                .build(),
        )
        .setup(move |app| {
            log::set_max_level(LogLevel::default().as_filter());
            log::info!(
                "[setup] Mimic starting, version {}",
                env!("CARGO_PKG_VERSION")
            );

            // 配置加载（路径 + 结果均记录日志）
            match config::config_path() {
                Ok(p) => log::info!("[setup] config path: {}", p.display()),
                Err(e) => log::error!("[setup] resolve config path failed: {}", e),
            }
            let (loaded_config, config_warning) = config::load_or_init_graceful();
            log::set_max_level(loaded_config.log_level.as_filter());
            if let Some(w) = &config_warning {
                log::error!(
                    "[setup] config write failed, falling back to in-memory default: {}",
                    w
                );
            } else {
                log::info!(
                    "[setup] config loaded: {} keyboard / {} mouse configs, hotkeys {} / {}",
                    loaded_config.keyboard_configs.len(),
                    loaded_config.mouse_configs.len(),
                    loaded_config.hotkeys.start.key_label,
                    loaded_config.hotkeys.stop.key_label,
                );
            }

            let audio_degraded = if let Err(error) =
                portable_paths.seed_default_audio(&[sound::FILE_START, sound::FILE_STOP])
            {
                log::error!("[setup] seed default audio failed: {error}");
                true
            } else {
                false
            };

            // 驱动检测 — DESIGN 13.1 步骤 3 / 阶段 11
            let driver_status = driver::check_interception_driver();
            log::info!("[setup] driver status: {:?}", driver_status);

            let shared_state: SharedState = Arc::new(Mutex::new(AppState {
                config: loaded_config,
                config_warning,
                navigation: PageId::Home,
                activity: Activity::Idle,
                simulation_mode: None,
                runtime_health: if audio_degraded {
                    RuntimeHealth::Degraded {
                        capability: "audio",
                    }
                } else {
                    RuntimeHealth::Healthy
                },
                driver_status: driver_status.clone(),
                pick_session: None,
                next_pick_token: 1,
                picker_timeout: None,
                active_custom_sequence_id: None,
                runtime: None,
                recording: sound_recorder::new_handle(),
                recording_buffer: Arc::new(Mutex::new(None)),
            }));

            // 启动后后台静默预开设备并加载 WAV；失败只降级音频能力。
            let audio_weak_state = Arc::downgrade(&shared_state);
            let audio_warmup = sound::warm_up_in_background(move |result| match result {
                Ok(()) => log::info!("[setup] audio warmup completed"),
                Err(error) => {
                    log::error!("[setup] audio warmup failed: {error}");
                    if let Some(state) = audio_weak_state.upgrade() {
                        if let Ok(mut state) = state.lock() {
                            if !matches!(state.runtime_health, RuntimeHealth::Error { .. }) {
                                state.runtime_health = RuntimeHealth::Degraded {
                                    capability: "audio",
                                };
                            }
                        }
                    }
                }
            });
            let picker_weak_state = Arc::downgrade(&shared_state);
            let picker_app = app.handle().clone();
            match mouse_picker::PickerTimeoutHandle::spawn(move |token| {
                if let Some(state) = picker_weak_state.upgrade() {
                    mouse_picker::timeout_pick(&picker_app, &state, token);
                }
            }) {
                Ok(timeout) => {
                    if let Ok(mut state) = shared_state.lock() {
                        state.picker_timeout = Some(timeout);
                    }
                    log::info!("[setup] mouse picker timeout service started");
                }
                Err(error) => {
                    log::error!("[setup] mouse picker timeout service failed: {}", error);
                }
            }

            if matches!(&driver_status, state::DriverStatus::Ready) {
                let weak_state = Arc::downgrade(&shared_state);
                let runtime_app = app.handle().clone();
                let sink = Arc::new(move |event| {
                    handle_runtime_event(&runtime_app, &weak_state, event);
                });

                match runtime::RuntimeHandle::spawn(
                    || {
                        simulation::driver::InterceptionDriver::new()
                            .map_err(|error| runtime::RuntimeError::Driver(error.to_string()))
                    },
                    sink,
                ) {
                    Ok(runtime) => {
                        if let Ok(mut state) = shared_state.lock() {
                            state.runtime = Some(runtime);
                        }
                        log::info!("[setup] Runtime Actor started");
                    }
                    Err(error) => {
                        log::error!("[setup] Runtime Actor failed: {}", error);
                        if let Ok(mut state) = shared_state.lock() {
                            state.runtime_health = RuntimeHealth::Error {
                                code: "runtime_unavailable",
                            };
                        }
                    }
                }

                match listener::start_listener(app.handle().clone(), shared_state.clone()) {
                    Ok(listener) => {
                        app.manage(listener);
                        log::info!("[setup] Interception hotkey listener started");
                    }
                    Err(error) => {
                        log::error!("[setup] Interception hotkey listener failed: {}", error);
                        if let Ok(mut state) = shared_state.lock() {
                            state.runtime_health = RuntimeHealth::Error {
                                code: "listener_unavailable",
                            };
                        }
                    }
                }
            } else {
                log::warn!("[setup] Interception not ready, hotkeys and simulation disabled");
            }

            app.manage(shared_state.clone());
            match audio_warmup {
                Ok(handle) => {
                    app.manage(handle);
                    log::info!("[setup] audio warmup started");
                }
                Err(error) => {
                    log::error!("[setup] audio warmup thread failed: {error}");
                    if let Ok(mut state) = shared_state.lock() {
                        if !matches!(state.runtime_health, RuntimeHealth::Error { .. }) {
                            state.runtime_health = RuntimeHealth::Degraded {
                                capability: "audio",
                            };
                        }
                    }
                }
            }

            log::info!("[setup] ready");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_config,
            persist_config,
            update_log_level,
            get_init_warning,
            get_admin_status,
            check_driver_status,
            install_interception_driver,
            uninstall_interception_driver,
            reboot_system,
            set_current_page,
            update_hotkeys,
            stop_simulation,
            get_runtime_status,
            enter_custom_sequence,
            start_pick_mouse_position,
            cancel_pick_mouse_position,
            start_recording,
            stop_recording,
            cancel_recording,
            save_trimmed_audio,
            preview_sound,
            get_sound_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn handle_runtime_event(
    app: &tauri::AppHandle,
    weak_state: &std::sync::Weak<Mutex<AppState>>,
    event: runtime::RuntimeEvent,
) {
    let Some(state) = weak_state.upgrade() else {
        return;
    };

    let status = match state.lock() {
        Ok(mut app_state) => {
            match event {
                runtime::RuntimeEvent::Started { run_id, mode } => {
                    log::info!("[runtime] run {} started: {:?}", run_id, mode);
                    app_state.activity = Activity::Simulating;
                    app_state.simulation_mode = Some(match mode {
                        runtime::RuntimeMode::Keyboard => SimulationMode::Keyboard,
                        runtime::RuntimeMode::Mouse => SimulationMode::Mouse,
                        runtime::RuntimeMode::Custom => SimulationMode::Custom,
                    });
                }
                runtime::RuntimeEvent::Stopped { run_id, mode } => {
                    log::info!("[runtime] run {} stopped: {:?}", run_id, mode);
                    app_state.release_activity(Activity::Simulating);
                }
                runtime::RuntimeEvent::Failed {
                    run_id,
                    message,
                    pressed_count,
                } => {
                    log::error!(
                        "[runtime] run {:?} failed, pressed_count={}: {}",
                        run_id,
                        pressed_count,
                        message
                    );
                    app_state.release_activity(Activity::Simulating);
                    app_state.runtime_health = RuntimeHealth::Error {
                        code: "runtime_failure",
                    };
                }
                runtime::RuntimeEvent::Shutdown => return,
            }
            app_state.runtime_status()
        }
        Err(error) => {
            log::error!("[runtime] failed to publish state: {}", error);
            return;
        }
    };

    if let Err(error) = app.emit(
        "runtime_status_changed",
        serde_json::json!({ "status": status }),
    ) {
        log::error!("[runtime] failed to emit state: {}", error);
    }
}
