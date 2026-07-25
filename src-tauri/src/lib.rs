// Mimic 应用后端入口 — DESIGN 6 / DESIGN 13.1
//
// 阶段 10：
//   - 接入 tauri-plugin-log（开发 info / release error，targets: Stdout + LogDir）
//   - setup 顺序按 DESIGN 13.1 微调：日志先于配置加载，便于后者出错时被记录
//   - 新增 admin 模块与命令：get_admin_status / request_admin_restart
//   - 关键启动事件改用 log::{info,error,warn}

mod admin;
mod commands;
mod config;
mod driver;
mod hotkeys;
mod listener;
mod mouse_picker;
mod runner;
mod simulation;
mod simulation_worker;
mod sound;
mod sound_recorder;
mod state;

use simulation::event::SimulationEvent;
use state::{AppState, RuntimeStatus, SendInterception, SharedState};
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc, Mutex};
use tauri::Manager;
use tauri_plugin_log::{Target, TargetKind};

// 导入命令实现 — ARCHITECTURE v3.0 阶段 A
use commands::config_cmd::*;
use commands::driver_cmd::*;
use commands::pick_cmd::*;
use commands::runtime_cmd::*;
use commands::sound_cmd::*;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // ADMIN_POLICY（2026-06-10 调整）：启动时不再主动请求 UAC 提权。
    // 应用普通权限即可运行：加载驱动、热键监听、按键/鼠标模拟均不需要管理员。
    // 仅「安装驱动」需要管理员，由 install_interception_driver 命令的权限守卫拦截，
    // 用户在首页看到 permission_denied 提示后，点击「以管理员身份重启」按钮触发 UAC。

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
    let log_level = if cfg!(debug_assertions) {
        log::LevelFilter::Info
    } else {
        log::LevelFilter::Error
    };

    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log_level)
                .targets([
                    Target::new(TargetKind::Stdout),
                    Target::new(TargetKind::LogDir { file_name: None }),
                ])
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            log::info!(
                "[setup] Mimic starting, version {}",
                env!("CARGO_PKG_VERSION")
            );

            // 音频设备初始化 — waveOut 预开设备 + 加载 PCM 缓冲
            // 设备常驻不关闭，触发时 waveOutReset + waveOutWrite 即时播放，< 15ms。
            sound::init();

            // 配置加载（路径 + 结果均记录日志）
            match config::config_path() {
                Ok(p) => log::info!("[setup] config path: {}", p.display()),
                Err(e) => log::error!("[setup] resolve config path failed: {}", e),
            }
            let (loaded_config, config_warning) = config::load_or_init_graceful();
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

            // 权限检测仅记录日志,不阻断启动 — DESIGN 14.1 降级启动
            // ADMIN_POLICY: Detect at startup, render result on home page, never block launch.
            let admin = admin::is_admin();
            log::info!(
                "[setup] admin status: {}",
                if admin { "elevated" } else { "limited" }
            );

            // 驱动检测 — DESIGN 13.1 步骤 3 / 阶段 11
            let driver_status = driver::check_interception_driver();
            log::info!("[setup] driver status: {:?}", driver_status);

            // 初始化 Interception 上下文 — DESIGN 8.3 / 阶段 13
            // 创建监听专用 context（设置 filter + wait）
            let listener_ctx = if matches!(&driver_status, state::DriverStatus::Ready) {
                match interception::Interception::new() {
                    Some(ctx) => {
                        log::info!("[setup] Interception listener context created");
                        Arc::new(Mutex::new(Some(SendInterception(ctx))))
                    }
                    None => {
                        log::error!("[setup] Interception listener context creation failed");
                        Arc::new(Mutex::new(None))
                    }
                }
            } else {
                log::warn!("[setup] Interception not ready, listener context not created");
                Arc::new(Mutex::new(None))
            };

            // 创建 worker 专用 context（仅 send，非阻塞）
            let worker_ctx = if matches!(&driver_status, state::DriverStatus::Ready) {
                match interception::Interception::new() {
                    Some(ctx) => {
                        log::info!("[setup] Interception worker context created");
                        Arc::new(Mutex::new(Some(SendInterception(ctx))))
                    }
                    None => {
                        log::error!("[setup] Interception worker context creation failed");
                        Arc::new(Mutex::new(None))
                    }
                }
            } else {
                log::warn!("[setup] Interception not ready, worker context not created");
                Arc::new(Mutex::new(None))
            };

            // 创建统一模拟事件 channel — ARCHITECTURE v2.0
            // 键盘/鼠标事件统一走此 channel。有界通道防止生产者-消费者失衡时内存泄漏；
            // 容量放宽到 1024，因单个动作会展开为多个事件（含 Delay），避免高频序列阻塞生产者。
            let (event_tx, event_rx) = mpsc::sync_channel::<SimulationEvent>(1024);

            // 启动 Interception 热键监听线程 — DESIGN 8.3 / 阶段 13
            let shared_state: SharedState = Arc::new(Mutex::new(AppState {
                config: loaded_config,
                config_warning,
                current_page: "home".to_string(),
                runtime_status: RuntimeStatus::Idle,
                driver_status: driver_status.clone(),
                stop_flag: Arc::new(AtomicBool::new(false)),
                pick_row_id: None,
                interception_listener: listener_ctx.clone(),
                interception_worker: worker_ctx.clone(),
                event_tx: event_tx.clone(),
                recording: sound_recorder::new_handle(),
                recording_buffer: Arc::new(Mutex::new(None)),
            }));

            if matches!(&driver_status, state::DriverStatus::Ready) {
                if let Err(e) = listener::start_listener(
                    app.handle().clone(),
                    shared_state.clone(),
                    listener_ctx.clone(),
                ) {
                    log::error!("[setup] Interception hotkey listener failed: {}", e);
                } else {
                    log::info!("[setup] Interception hotkey listener started");
                }

                // 启动统一模拟 worker 线程 — ARCHITECTURE v2.0
                if let Err(e) = simulation_worker::start_simulation_worker(
                    event_rx,
                    shared_state.clone(),
                    worker_ctx.clone(),
                ) {
                    log::error!("[setup] simulation worker failed: {}", e);
                } else {
                    log::info!("[setup] simulation worker started");
                }
            } else {
                log::warn!("[setup] Interception not ready, hotkeys and simulation disabled");
            }

            app.manage(shared_state);

            log::info!("[setup] ready");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_config,
            persist_config,
            get_init_warning,
            get_admin_status,
            request_admin_restart,
            check_driver_status,
            install_interception_driver,
            uninstall_interception_driver,
            reboot_system,
            set_current_page,
            update_hotkeys,
            stop_simulation,
            get_runtime_status,
            start_pick_mouse_position,
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
