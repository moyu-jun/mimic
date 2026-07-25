// 配置模型与默认配置 — ARCHITECTURE v3.0 重构
//
// 本模块定义前后端共享的配置结构体，并提供默认配置初始化。
// 所有结构体必须标注 #[serde(rename_all = "camelCase")] 确保 Rust snake_case
// 字段序列化为前端 camelCase（key_label → keyLabel）。
//
// 重构要点：
// - 配置层使用 XxxConfig 命名（与模拟层 XxxAction 区分）
// - 增加动作类型枚举（KeyActionType / MouseActionType）
// - selected 改名为 enabled（更语义化）

use ini::Ini;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::env;
use std::path::PathBuf;

pub const DEFAULT_INTERVAL_MS: u64 = 20;
pub const MIN_INTERVAL_MS: u64 = 5;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum KeyActionType {
    #[default]
    Press,
    Hold,
    Combo,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum MouseActionType {
    #[default]
    ClickLeft,
    ClickRight,
    ClickMiddle,
    ScrollUp,
    ScrollDown,
    Drag,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapturedKey {
    pub key_label: String,
    pub scan_code: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyboardConfig {
    pub id: String,
    pub enabled: bool,
    #[serde(default)]
    pub action_type: KeyActionType,
    pub key_label: String,
    pub scan_code: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hold_duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifiers: Vec<u16>,
    pub interval_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MouseConfig {
    pub id: String,
    pub enabled: bool,
    #[serde(default)]
    pub action_type: MouseActionType,
    pub x: Option<i32>,
    pub y: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scroll_delta: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drag_to_x: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drag_to_y: Option<i32>,
    pub interval_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyConfig {
    pub start: CapturedKey,
    pub stop: CapturedKey,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub keyboard_configs: Vec<KeyboardConfig>,
    pub mouse_configs: Vec<MouseConfig>,
    pub hotkeys: HotkeyConfig,
}

pub fn default_config() -> AppConfig {
    AppConfig {
        keyboard_configs: vec![KeyboardConfig {
            id: "default-keyboard-1".to_string(),
            enabled: true,
            action_type: KeyActionType::Press,
            key_label: "F".to_string(),
            scan_code: 33,
            hold_duration_ms: None,
            modifiers: Vec::new(),
            interval_ms: DEFAULT_INTERVAL_MS,
        }],
        mouse_configs: vec![MouseConfig {
            id: "default-mouse-1".to_string(),
            enabled: true,
            action_type: MouseActionType::ClickLeft,
            x: None,
            y: None,
            scroll_delta: None,
            drag_to_x: None,
            drag_to_y: None,
            interval_ms: DEFAULT_INTERVAL_MS,
        }],
        hotkeys: HotkeyConfig {
            start: CapturedKey {
                key_label: "F12".to_string(),
                scan_code: 88,
            },
            stop: CapturedKey {
                key_label: "F12".to_string(),
                scan_code: 88,
            },
        },
    }
}

pub fn config_path() -> Result<PathBuf, String> {
    let exe_path = env::current_exe().map_err(|e| format!("Failed to get exe path: {}", e))?;
    let exe_dir = exe_path
        .parent()
        .ok_or_else(|| "Failed to get exe directory".to_string())?;
    Ok(exe_dir.join("mimic.ini"))
}

pub fn sanitize_config(config: &mut AppConfig) {
    for cfg in &mut config.keyboard_configs {
        if cfg.interval_ms < MIN_INTERVAL_MS {
            log::warn!(
                "[config] keyboard config {} intervalMs {} < {}, clamped",
                cfg.id,
                cfg.interval_ms,
                MIN_INTERVAL_MS
            );
            cfg.interval_ms = MIN_INTERVAL_MS;
        }
    }
    for cfg in &mut config.mouse_configs {
        if cfg.interval_ms < MIN_INTERVAL_MS {
            log::warn!(
                "[config] mouse config {} intervalMs {} < {}, clamped",
                cfg.id,
                cfg.interval_ms,
                MIN_INTERVAL_MS
            );
            cfg.interval_ms = MIN_INTERVAL_MS;
        }
    }

    dedupe_ids(
        &mut config.keyboard_configs,
        |c| &c.id,
        |c, new_id| c.id = new_id,
        "keyboard",
    );
    dedupe_ids(
        &mut config.mouse_configs,
        |c| &c.id,
        |c, new_id| c.id = new_id,
        "mouse",
    );
}

fn dedupe_ids<T>(
    configs: &mut [T],
    get_id: impl Fn(&T) -> &str,
    mut set_id: impl FnMut(&mut T, String),
    kind: &str,
) {
    let mut seen: HashSet<String> = HashSet::new();
    for cfg in configs.iter_mut() {
        let original = get_id(cfg).to_string();
        if seen.insert(original.clone()) {
            continue;
        }
        let mut counter = 1u32;
        let new_id = loop {
            let candidate = format!("{}-dup-{}", original, counter);
            if !seen.contains(&candidate) {
                break candidate;
            }
            counter += 1;
        };
        log::warn!(
            "[config] {} config duplicate id `{}`, renamed to `{}`",
            kind,
            original,
            new_id
        );
        seen.insert(new_id.clone());
        set_id(cfg, new_id);
    }
}

pub fn load_or_init_graceful() -> (AppConfig, Option<String>) {
    match load_or_init() {
        Ok(config) => (config, None),
        Err(e) => {
            log::error!("[config] fallback to in-memory default: {}", e);
            (default_config(), Some(e))
        }
    }
}

pub fn load_or_init() -> Result<AppConfig, String> {
    let path = config_path()?;

    if !path.exists() {
        log::info!("[config] mimic.ini not found, writing default");
        let default = default_config();
        save(&default)?;
        return Ok(default);
    }

    match load_from_ini(&path) {
        Ok(mut config) => {
            sanitize_config(&mut config);
            Ok(config)
        }
        Err(e) => {
            log::error!(
                "[config] failed to parse INI, overwriting with default: {}",
                e
            );
            let default = default_config();
            save(&default)?;
            Ok(default)
        }
    }
}

fn load_from_ini(path: &PathBuf) -> Result<AppConfig, String> {
    let ini = Ini::load_from_file(path).map_err(|e| format!("Failed to load INI file: {}", e))?;

    let hotkeys_section = ini
        .section(Some("hotkeys"))
        .ok_or_else(|| "Missing [hotkeys] section".to_string())?;

    let start_label = hotkeys_section
        .get("start_label")
        .ok_or_else(|| "Missing start_label".to_string())?;
    let start_scan_code: u16 = hotkeys_section
        .get("start_scan_code")
        .ok_or_else(|| "Missing start_scan_code".to_string())?
        .parse()
        .map_err(|e| format!("Invalid start_scan_code: {}", e))?;

    let stop_label = hotkeys_section
        .get("stop_label")
        .ok_or_else(|| "Missing stop_label".to_string())?;
    let stop_scan_code: u16 = hotkeys_section
        .get("stop_scan_code")
        .ok_or_else(|| "Missing stop_scan_code".to_string())?
        .parse()
        .map_err(|e| format!("Invalid stop_scan_code: {}", e))?;

    let hotkeys = HotkeyConfig {
        start: CapturedKey {
            key_label: start_label.to_string(),
            scan_code: start_scan_code,
        },
        stop: CapturedKey {
            key_label: stop_label.to_string(),
            scan_code: stop_scan_code,
        },
    };

    let keyboard_section = ini
        .section(Some("keyboard"))
        .ok_or_else(|| "Missing [keyboard] section".to_string())?;
    let keyboard_configs_json = keyboard_section
        .get("configs")
        .ok_or_else(|| "Missing keyboard configs".to_string())?;
    let keyboard_configs: Vec<KeyboardConfig> = serde_json::from_str(keyboard_configs_json)
        .map_err(|e| format!("Failed to parse keyboard configs: {}", e))?;

    let mouse_section = ini
        .section(Some("mouse"))
        .ok_or_else(|| "Missing [mouse] section".to_string())?;
    let mouse_configs_json = mouse_section
        .get("configs")
        .ok_or_else(|| "Missing mouse configs".to_string())?;
    let mouse_configs: Vec<MouseConfig> = serde_json::from_str(mouse_configs_json)
        .map_err(|e| format!("Failed to parse mouse configs: {}", e))?;

    Ok(AppConfig {
        keyboard_configs,
        mouse_configs,
        hotkeys,
    })
}

pub fn save(config: &AppConfig) -> Result<(), String> {
    let path = config_path()?;
    let mut sanitized = config.clone();
    sanitize_config(&mut sanitized);

    let mut ini = Ini::new();

    ini.with_section(Some("hotkeys"))
        .set("start_label", &sanitized.hotkeys.start.key_label)
        .set(
            "start_scan_code",
            sanitized.hotkeys.start.scan_code.to_string(),
        )
        .set("stop_label", &sanitized.hotkeys.stop.key_label)
        .set(
            "stop_scan_code",
            sanitized.hotkeys.stop.scan_code.to_string(),
        );

    let keyboard_json = serde_json::to_string(&sanitized.keyboard_configs)
        .map_err(|e| format!("Failed to serialize keyboard configs: {}", e))?;
    ini.with_section(Some("keyboard"))
        .set("configs", keyboard_json);

    let mouse_json = serde_json::to_string(&sanitized.mouse_configs)
        .map_err(|e| format!("Failed to serialize mouse configs: {}", e))?;
    ini.with_section(Some("mouse")).set("configs", mouse_json);

    let mut tmp_os = path.clone().into_os_string();
    tmp_os.push(".tmp");
    let tmp_path = PathBuf::from(tmp_os);

    if let Err(e) = ini.write_to_file(&tmp_path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(format!("Failed to write INI tmp file: {}", e));
    }

    if let Err(e) = std::fs::rename(&tmp_path, &path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(format!("Failed to atomically replace mimic.ini: {}", e));
    }

    Ok(())
}
