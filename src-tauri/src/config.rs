//! 配置模型、校验与原子持久化。
//!
//! 启动时配置缺失则生成代码内置默认值；文件过大、格式错误或违反任一边界时，直接
//! 用默认配置覆盖。用户更新使用 validate -> atomic persist -> swap，失败不更新内存。

use crate::paths::PortablePaths;
use ini::Ini;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

pub const DEFAULT_INTERVAL_MS: u64 = 20;
pub const MIN_INTERVAL_MS: u64 = 5;
pub const MAX_INTERVAL_MS: u64 = 3_600_000;
pub const MAX_KEYBOARD_CONFIGS: usize = 500;
pub const MAX_MOUSE_CONFIGS: usize = 500;
pub const MAX_CUSTOM_SEQUENCES: usize = 100;
pub const MAX_CUSTOM_ACTIONS: usize = 1_000;
pub const MAX_SEQUENCE_NAME_CHARS: usize = 64;
pub const MAX_CONFIG_FILE_BYTES: u64 = 5 * 1024 * 1024;

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(1);
static CONFIG_TRANSACTION: Mutex<()> = Mutex::new(());

/// 串行化配置候选读取、原子写盘和内存提交的完整事务。
pub fn transaction_guard() -> Result<MutexGuard<'static, ()>, String> {
    CONFIG_TRANSACTION
        .lock()
        .map_err(|_| "config transaction lock poisoned".to_string())
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
}

impl Default for LogLevel {
    fn default() -> Self {
        if cfg!(debug_assertions) {
            Self::Info
        } else {
            Self::Error
        }
    }
}

impl LogLevel {
    pub fn as_filter(self) -> log::LevelFilter {
        match self {
            Self::Error => log::LevelFilter::Error,
            Self::Warn => log::LevelFilter::Warn,
            Self::Info => log::LevelFilter::Info,
            Self::Debug => log::LevelFilter::Debug,
        }
    }

    fn as_ini_value(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
        }
    }

    fn from_ini_value(value: &str) -> Result<Self, String> {
        match value.to_ascii_lowercase().as_str() {
            "error" => Ok(Self::Error),
            "warn" => Ok(Self::Warn),
            "info" => Ok(Self::Info),
            "debug" => Ok(Self::Debug),
            _ => Err(format!("invalid log level: {value}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum MouseActionType {
    #[default]
    #[serde(rename = "click_left")]
    Left,
    #[serde(rename = "click_right")]
    Right,
    #[serde(rename = "click_middle")]
    Middle,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CapturedKey {
    pub key_label: String,
    pub scan_code: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KeyboardConfig {
    pub id: String,
    pub enabled: bool,
    pub key_label: String,
    pub scan_code: u16,
    pub interval_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MouseConfig {
    pub id: String,
    pub enabled: bool,
    #[serde(default)]
    pub action_type: MouseActionType,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub interval_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CustomAction {
    Keyboard(KeyboardConfig),
    Mouse(MouseConfig),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CustomSequence {
    pub id: String,
    pub name: String,
    pub actions: Vec<CustomAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyConfig {
    pub start: CapturedKey,
    pub stop: CapturedKey,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub keyboard_configs: Vec<KeyboardConfig>,
    pub mouse_configs: Vec<MouseConfig>,
    #[serde(default)]
    pub custom_sequences: Vec<CustomSequence>,
    pub hotkeys: HotkeyConfig,
    #[serde(default)]
    pub log_level: LogLevel,
}

pub fn default_config() -> AppConfig {
    AppConfig {
        keyboard_configs: vec![KeyboardConfig {
            id: "default-keyboard-1".to_string(),
            enabled: true,
            key_label: "F".to_string(),
            scan_code: 33,
            interval_ms: DEFAULT_INTERVAL_MS,
        }],
        mouse_configs: vec![MouseConfig {
            id: "default-mouse-1".to_string(),
            enabled: true,
            action_type: MouseActionType::Left,
            x: None,
            y: None,
            interval_ms: DEFAULT_INTERVAL_MS,
        }],
        custom_sequences: Vec::new(),
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
        log_level: LogLevel::default(),
    }
}

pub fn config_path() -> Result<PathBuf, String> {
    Ok(PortablePaths::current()?.config_file())
}

pub fn validate_config(config: &AppConfig) -> Result<(), String> {
    ensure_max(
        "keyboard configs",
        config.keyboard_configs.len(),
        MAX_KEYBOARD_CONFIGS,
    )?;
    ensure_max(
        "mouse configs",
        config.mouse_configs.len(),
        MAX_MOUSE_CONFIGS,
    )?;
    ensure_max(
        "custom sequences",
        config.custom_sequences.len(),
        MAX_CUSTOM_SEQUENCES,
    )?;

    validate_hotkey(&config.hotkeys.start, "start hotkey")?;
    validate_hotkey(&config.hotkeys.stop, "stop hotkey")?;
    validate_keyboard_configs(&config.keyboard_configs, "keyboard")?;
    for keyboard in &config.keyboard_configs {
        if keyboard.scan_code == config.hotkeys.start.scan_code
            || keyboard.scan_code == config.hotkeys.stop.scan_code
        {
            return Err(format!(
                "keyboard {} conflicts with a global hotkey",
                keyboard.id
            ));
        }
    }
    validate_mouse_configs(&config.mouse_configs, "mouse")?;
    ensure_unique_ids(
        config
            .keyboard_configs
            .iter()
            .map(|config| config.id.as_str()),
        "keyboard",
    )?;
    ensure_unique_ids(
        config.mouse_configs.iter().map(|config| config.id.as_str()),
        "mouse",
    )?;
    ensure_unique_ids(
        config
            .custom_sequences
            .iter()
            .map(|sequence| sequence.id.as_str()),
        "custom sequence",
    )?;

    for sequence in &config.custom_sequences {
        let name_chars = sequence.name.chars().count();
        if name_chars == 0 || name_chars > MAX_SEQUENCE_NAME_CHARS {
            return Err(format!(
                "custom sequence {} name must contain 1..={} characters",
                sequence.id, MAX_SEQUENCE_NAME_CHARS
            ));
        }
        ensure_max(
            &format!("custom sequence {} actions", sequence.id),
            sequence.actions.len(),
            MAX_CUSTOM_ACTIONS,
        )?;

        let mut action_ids = HashSet::new();
        for action in &sequence.actions {
            let (id, interval_ms) = match action {
                CustomAction::Keyboard(config) => {
                    validate_keyboard(config, "custom keyboard")?;
                    (&config.id, config.interval_ms)
                }
                CustomAction::Mouse(config) => {
                    validate_mouse(config, "custom mouse")?;
                    (&config.id, config.interval_ms)
                }
            };
            validate_interval(interval_ms, "custom action")?;
            if id.is_empty() || !action_ids.insert(id.as_str()) {
                return Err(format!(
                    "custom sequence {} has empty or duplicate action id: {}",
                    sequence.id, id
                ));
            }
        }
    }

    Ok(())
}

fn validate_keyboard_configs(configs: &[KeyboardConfig], kind: &str) -> Result<(), String> {
    for config in configs {
        validate_keyboard(config, kind)?;
    }
    Ok(())
}

fn validate_mouse_configs(configs: &[MouseConfig], kind: &str) -> Result<(), String> {
    for config in configs {
        validate_mouse(config, kind)?;
    }
    Ok(())
}

fn validate_hotkey(key: &CapturedKey, kind: &str) -> Result<(), String> {
    if key.key_label.trim().is_empty() || interception::ScanCode::try_from(key.scan_code).is_err() {
        return Err(format!("{kind} is invalid"));
    }
    Ok(())
}

fn validate_keyboard(config: &KeyboardConfig, kind: &str) -> Result<(), String> {
    if config.id.is_empty() {
        return Err(format!("{kind} id must not be empty"));
    }
    if config.key_label.trim().is_empty()
        || interception::ScanCode::try_from(config.scan_code).is_err()
    {
        return Err(format!("{kind} key is invalid"));
    }
    validate_interval(config.interval_ms, kind)
}

fn validate_mouse(config: &MouseConfig, kind: &str) -> Result<(), String> {
    if config.id.is_empty() {
        return Err(format!("{kind} id must not be empty"));
    }
    validate_interval(config.interval_ms, kind)
}
fn validate_interval(interval_ms: u64, kind: &str) -> Result<(), String> {
    if !(MIN_INTERVAL_MS..=MAX_INTERVAL_MS).contains(&interval_ms) {
        return Err(format!(
            "{kind} interval {interval_ms} is outside {MIN_INTERVAL_MS}..={MAX_INTERVAL_MS}"
        ));
    }
    Ok(())
}

fn ensure_max(kind: &str, actual: usize, maximum: usize) -> Result<(), String> {
    if actual > maximum {
        Err(format!("{kind} count {actual} exceeds {maximum}"))
    } else {
        Ok(())
    }
}

fn ensure_unique_ids<'a>(ids: impl IntoIterator<Item = &'a str>, kind: &str) -> Result<(), String> {
    let mut seen = HashSet::new();
    for id in ids {
        if id.is_empty() || !seen.insert(id) {
            return Err(format!("{kind} has empty or duplicate id: {id}"));
        }
    }
    Ok(())
}

pub fn load_or_init_graceful() -> (AppConfig, Option<String>) {
    match load_or_init() {
        Ok(config) => (config, None),
        Err(error) => {
            log::error!("[config] fallback to in-memory default: {}", error);
            (default_config(), Some(error))
        }
    }
}

pub fn load_or_init() -> Result<AppConfig, String> {
    let paths = PortablePaths::current()?;
    paths.ensure_data_dirs()?;
    let path = paths.config_file();
    crate::paths::ensure_regular_file_or_missing(&path)?;

    if !path.exists() {
        let default = default_config();
        save_to_path(&default, &path)?;
        return Ok(default);
    }

    let loaded = file_size_within_limit(&path)
        .and_then(|()| load_from_ini(&path))
        .and_then(|config| {
            validate_config(&config)?;
            Ok(config)
        });

    match loaded {
        Ok(config) => Ok(config),
        Err(error) => {
            log::error!(
                "[config] invalid config, overwriting with embedded defaults: {}",
                error
            );
            let default = default_config();
            save_to_path(&default, &path)?;
            Ok(default)
        }
    }
}

fn file_size_within_limit(path: &Path) -> Result<(), String> {
    let size = std::fs::metadata(path)
        .map_err(|error| format!("failed to read config metadata: {error}"))?
        .len();
    if size > MAX_CONFIG_FILE_BYTES {
        Err(format!(
            "config file size {size} exceeds {MAX_CONFIG_FILE_BYTES}"
        ))
    } else {
        Ok(())
    }
}

fn load_from_ini(path: &Path) -> Result<AppConfig, String> {
    let ini =
        Ini::load_from_file(path).map_err(|error| format!("failed to load INI file: {error}"))?;
    decode_ini(&ini)
}

fn decode_ini(ini: &Ini) -> Result<AppConfig, String> {
    let hotkeys_section = ini
        .section(Some("hotkeys"))
        .ok_or_else(|| "missing [hotkeys] section".to_string())?;
    let start_label = required(hotkeys_section.get("start_label"), "start_label")?;
    let start_scan_code = parse_u16(
        required(hotkeys_section.get("start_scan_code"), "start_scan_code")?,
        "start_scan_code",
    )?;
    let stop_label = required(hotkeys_section.get("stop_label"), "stop_label")?;
    let stop_scan_code = parse_u16(
        required(hotkeys_section.get("stop_scan_code"), "stop_scan_code")?,
        "stop_scan_code",
    )?;

    let keyboard_configs = parse_json_section(ini, "keyboard", "configs")?;
    let mouse_configs = parse_json_section(ini, "mouse", "configs")?;
    let custom_sequences = match ini
        .section(Some("custom"))
        .and_then(|section| section.get("sequences"))
    {
        Some(value) => serde_json::from_str(value)
            .map_err(|error| format!("failed to parse custom sequences: {error}"))?,
        None => Vec::new(),
    };
    let log_level = match ini
        .section(Some("logging"))
        .and_then(|section| section.get("level"))
    {
        Some(value) => LogLevel::from_ini_value(value)?,
        None => LogLevel::default(),
    };

    Ok(AppConfig {
        keyboard_configs,
        mouse_configs,
        custom_sequences,
        hotkeys: HotkeyConfig {
            start: CapturedKey {
                key_label: start_label.to_string(),
                scan_code: start_scan_code,
            },
            stop: CapturedKey {
                key_label: stop_label.to_string(),
                scan_code: stop_scan_code,
            },
        },
        log_level,
    })
}

fn required<'a>(value: Option<&'a str>, field: &str) -> Result<&'a str, String> {
    value.ok_or_else(|| format!("missing {field}"))
}

fn parse_u16(value: &str, field: &str) -> Result<u16, String> {
    value
        .parse()
        .map_err(|error| format!("invalid {field}: {error}"))
}

fn parse_json_section<T>(ini: &Ini, section: &str, key: &str) -> Result<T, String>
where
    T: for<'de> Deserialize<'de>,
{
    let value = ini
        .section(Some(section))
        .and_then(|section| section.get(key))
        .ok_or_else(|| format!("missing [{section}] {key}"))?;
    serde_json::from_str(value)
        .map_err(|error| format!("failed to parse [{section}] {key}: {error}"))
}

pub fn save(config: &AppConfig) -> Result<(), String> {
    let paths = PortablePaths::current()?;
    paths.ensure_data_dirs()?;
    save_to_path(config, &paths.config_file())
}

fn save_to_path(config: &AppConfig, path: &Path) -> Result<(), String> {
    validate_config(config)?;
    crate::paths::ensure_regular_file_or_missing(path)?;

    let mut ini = Ini::new();
    ini.with_section(Some("hotkeys"))
        .set("start_label", &config.hotkeys.start.key_label)
        .set(
            "start_scan_code",
            config.hotkeys.start.scan_code.to_string(),
        )
        .set("stop_label", &config.hotkeys.stop.key_label)
        .set("stop_scan_code", config.hotkeys.stop.scan_code.to_string());

    let keyboard_json = serde_json::to_string(&config.keyboard_configs)
        .map_err(|error| format!("failed to serialize keyboard configs: {error}"))?;
    ini.with_section(Some("keyboard"))
        .set("configs", keyboard_json);

    let mouse_json = serde_json::to_string(&config.mouse_configs)
        .map_err(|error| format!("failed to serialize mouse configs: {error}"))?;
    ini.with_section(Some("mouse")).set("configs", mouse_json);

    let custom_json = serde_json::to_string(&config.custom_sequences)
        .map_err(|error| format!("failed to serialize custom sequences: {error}"))?;
    ini.with_section(Some("custom"))
        .set("sequences", custom_json);
    ini.with_section(Some("logging"))
        .set("level", config.log_level.as_ini_value());

    let (temporary, mut file) = create_temporary_file(path)?;
    let write_result = (|| {
        ini.write_to(&mut file)
            .map_err(|error| format!("failed to write temporary config: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("failed to flush temporary config: {error}"))?;
        let size = file
            .metadata()
            .map_err(|error| format!("failed to read temporary config metadata: {error}"))?
            .len();
        if size > MAX_CONFIG_FILE_BYTES {
            return Err(format!(
                "config file size {size} exceeds {MAX_CONFIG_FILE_BYTES}"
            ));
        }
        drop(file);
        crate::paths::atomic_replace(&temporary, path)
    })();

    if write_result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    write_result
}

fn create_temporary_file(path: &Path) -> Result<(PathBuf, std::fs::File), String> {
    for _ in 0..16 {
        let temporary = temporary_path(path);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => return Ok((temporary, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("failed to create temporary config: {error}")),
        }
    }
    Err("failed to allocate unique temporary config".to_string())
}

fn temporary_path(path: &Path) -> PathBuf {
    let counter = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let file_name = format!("mimic.ini.tmp-{}-{counter}", std::process::id());
    path.with_file_name(file_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        validate_config(&default_config()).unwrap();
    }

    #[test]
    fn rejects_interval_outside_bounds() {
        let mut config = default_config();
        config.keyboard_configs[0].interval_ms = MIN_INTERVAL_MS - 1;
        assert!(validate_config(&config).unwrap_err().contains("interval"));

        config.keyboard_configs[0].interval_ms = MAX_INTERVAL_MS + 1;
        assert!(validate_config(&config).unwrap_err().contains("interval"));
    }

    #[test]
    fn rejects_duplicate_ids_instead_of_rewriting_them() {
        let mut config = default_config();
        config
            .keyboard_configs
            .push(config.keyboard_configs[0].clone());
        assert!(validate_config(&config).unwrap_err().contains("duplicate"));
    }

    #[test]
    fn rejects_sequence_name_and_action_count_limits() {
        let mut config = default_config();
        config.custom_sequences.push(CustomSequence {
            id: "sequence".to_string(),
            name: "字".repeat(MAX_SEQUENCE_NAME_CHARS + 1),
            actions: Vec::new(),
        });
        assert!(validate_config(&config).unwrap_err().contains("name"));

        config.custom_sequences[0].name = "ok".to_string();
        let action = CustomAction::Keyboard(config.keyboard_configs[0].clone());
        config.custom_sequences[0].actions = vec![action; MAX_CUSTOM_ACTIONS + 1];
        assert!(validate_config(&config).unwrap_err().contains("actions"));
    }

    #[test]
    fn rejects_unsupported_mouse_action_types_during_decode() {
        let encoded = r#"{
            "id":"mouse", "enabled":true, "actionType":"drag",
            "x":0, "y":0, "intervalMs":20
        }"#;
        assert!(serde_json::from_str::<MouseConfig>(encoded).is_err());
    }

    #[test]
    fn rejects_independent_keyboard_hotkey_conflict() {
        let mut config = default_config();
        config.keyboard_configs[0].scan_code = config.hotkeys.start.scan_code;
        assert!(validate_config(&config).unwrap_err().contains("conflicts"));
    }

    #[test]
    fn deterministic_arbitrary_ini_inputs_never_panic() {
        const ALPHABET: &[u8] =
            b"[]=\n\r{}\\\":,abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 _-";

        for seed in 0..512_u64 {
            let mut state = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let length = (next_random(&mut state) % 2_048) as usize;
            let input: String = (0..length)
                .map(|_| {
                    let index = (next_random(&mut state) as usize) % ALPHABET.len();
                    ALPHABET[index] as char
                })
                .collect();

            if let Ok(ini) = Ini::load_from_str(&input) {
                if let Ok(config) = decode_ini(&ini) {
                    let _ = validate_config(&config);
                }
            }
        }
    }

    fn next_random(state: &mut u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }

    #[test]
    fn config_transaction_serializes_writers() {
        let first = transaction_guard().unwrap();
        let (started_tx, started_rx) = std::sync::mpsc::sync_channel(1);
        let (acquired_tx, acquired_rx) = std::sync::mpsc::sync_channel(1);
        let join = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            let _second = transaction_guard().unwrap();
            acquired_tx.send(()).unwrap();
        });

        started_rx.recv().unwrap();
        assert!(acquired_rx
            .recv_timeout(std::time::Duration::from_millis(25))
            .is_err());
        drop(first);
        acquired_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        join.join().unwrap();
    }
}
