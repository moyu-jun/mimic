// 提示音录制 — DESIGN 20 / 阶段 18
//
// 用 cpal 采集系统默认麦克风，累积 i16/mono PCM 到内存缓冲，停止时用 hound
// 写入 WAV 并原子覆盖 exe 同级 audio 目录下的 `按键开启.wav` / `按键关闭.wav`。
//
// 线程模型：cpal 的 Stream 是 !Send，无法跨命令存放，因此由一个专用录制线程
// 创建并持有 Stream，命令通过 channel 发停止/取消信号；音频缓冲与最新峰值经
// Arc<Mutex<>> 共享。波形幅度由录制线程按 ~30fps 经事件推送，避免在音频回调里
// 直接 emit。

use crate::state::{RuntimeStatus, SharedState};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use log::{error, info};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// 最大录制时长（秒）— REQUIREMENTS 3.14
const MAX_DURATION_SECS: u32 = 5;
/// 波形 / 自动停止检查间隔（约 30fps）
const TICK_MS: u64 = 33;

/// 录制线程控制信号
pub enum RecCtrl {
    Stop,
    Cancel,
}

/// 录制线程与命令共享的缓冲
struct RecBuf {
    /// mono i16 PCM 累积缓冲
    samples: Vec<i16>,
    /// 最近一个回调的峰值幅度（0.0~1.0），录制线程读取后推送波形事件
    latest_peak: f32,
}

/// 当前是否有录制进行中 — 持有控制信号发送端；None 表示空闲。
/// 录制线程结束时（停止 / 取消 / 超时）由线程自身清空。
pub type RecordingHandle = Arc<Mutex<Option<Sender<RecCtrl>>>>;

/// 创建空闲录制句柄（在 setup 中初始化并存入 AppState）
pub fn new_handle() -> RecordingHandle {
    Arc::new(Mutex::new(None))
}

/// 开始录制 — DESIGN 20.5
///
/// target: "start" -> 按键开启.wav, "stop" -> 按键关闭.wav。
/// 运行态守卫在 lib.rs 命令层完成；此处再做设备可用性检查。
pub fn start_recording(
    app: AppHandle,
    state: SharedState,
    handle: RecordingHandle,
    target: String,
) -> Result<(), String> {
    let file_name = match target.as_str() {
        "start" => "按键开启.wav",
        "stop" => "按键关闭.wav",
        _ => return Err("invalid target".to_string()),
    };

    // 已有录制进行中 → 拒绝
    {
        let h = handle.lock().map_err(|e| format!("lock handle: {}", e))?;
        if h.is_some() {
            return Err("recording already in progress".to_string());
        }
    }

    // 设备可用性检查（Stream 在线程内构建，这里仅探测默认输入设备是否存在）
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| "no_input_device".to_string())?;
    let default_cfg = device
        .default_input_config()
        .map_err(|e| format!("default_input_config: {}", e))?;

    let sample_rate = default_cfg.sample_rate().0;
    let channels = default_cfg.channels() as usize;
    let sample_format = default_cfg.sample_format();
    let config: cpal::StreamConfig = default_cfg.into();

    info!(
        "[recorder] start target={} rate={} ch={} fmt={:?}",
        target, sample_rate, channels, sample_format
    );

    let (ctrl_tx, ctrl_rx) = std::sync::mpsc::channel::<RecCtrl>();
    let buf = Arc::new(Mutex::new(RecBuf {
        samples: Vec::with_capacity((sample_rate * MAX_DURATION_SECS) as usize),
        latest_peak: 0.0,
    }));

    // 标记录制进行中
    {
        let mut h = handle.lock().map_err(|e| format!("lock handle: {}", e))?;
        *h = Some(ctrl_tx);
    }
    {
        let mut s = state.lock().map_err(|e| format!("lock state: {}", e))?;
        s.runtime_status = RuntimeStatus::Recording;
    }
    let _ = app.emit(
        "runtime_status_changed",
        serde_json::json!({ "status": RuntimeStatus::Recording }),
    );

    // 录制线程：构建并持有 Stream，循环检查信号 / 超时 / 推送波形
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));

    std::thread::spawn(move || {
        run_recording_thread(
            app,
            state,
            handle,
            buf,
            ctrl_rx,
            device,
            config,
            sample_format,
            channels,
            sample_rate,
            target,
            file_name,
            exe_dir,
        );
    });

    Ok(())
}

/// 停止录制（保存）— 仅发信号，结果经 recording_finished 事件返回。
pub fn stop_recording(handle: &RecordingHandle) -> Result<(), String> {
    let h = handle.lock().map_err(|e| format!("lock handle: {}", e))?;
    match h.as_ref() {
        Some(tx) => {
            let _ = tx.send(RecCtrl::Stop);
            Ok(())
        }
        None => Err("no recording in progress".to_string()),
    }
}

/// 取消录制（不写文件）
pub fn cancel_recording(handle: &RecordingHandle) -> Result<(), String> {
    let h = handle.lock().map_err(|e| format!("lock handle: {}", e))?;
    match h.as_ref() {
        Some(tx) => {
            let _ = tx.send(RecCtrl::Cancel);
            Ok(())
        }
        None => Err("no recording in progress".to_string()),
    }
}

#[allow(clippy::too_many_arguments)]
fn run_recording_thread(
    app: AppHandle,
    state: SharedState,
    handle: RecordingHandle,
    buf: Arc<Mutex<RecBuf>>,
    ctrl_rx: Receiver<RecCtrl>,
    device: cpal::Device,
    config: cpal::StreamConfig,
    sample_format: SampleFormat,
    channels: usize,
    sample_rate: u32,
    target: String,
    _file_name: &str,
    _exe_dir: Option<std::path::PathBuf>,
) {
    // 构建输入流（回调内降为 mono i16 累积 + 记录峰值）
    let stream = match build_input_stream(&device, &config, sample_format, channels, buf.clone()) {
        Ok(s) => s,
        Err(e) => {
            error!("[recorder] build stream failed: {}", e);
            finish_idle(&app, &state, &handle);
            let _ = app.emit("recording_error", serde_json::json!({ "error": e }));
            return;
        }
    };
    if let Err(e) = stream.play() {
        error!("[recorder] stream.play failed: {}", e);
        finish_idle(&app, &state, &handle);
        let _ = app.emit(
            "recording_error",
            serde_json::json!({ "error": format!("play: {}", e) }),
        );
        return;
    }

    let max_samples = (sample_rate * MAX_DURATION_SECS) as usize;
    let mut cancelled = false;

    loop {
        match ctrl_rx.recv_timeout(Duration::from_millis(TICK_MS)) {
            Ok(RecCtrl::Stop) => break,
            Ok(RecCtrl::Cancel) => {
                cancelled = true;
                break;
            }
            Err(RecvTimeoutError::Timeout) => {
                // 推送波形幅度 + 检查是否到达时长上限
                let (peak, len) = {
                    match buf.lock() {
                        Ok(b) => (b.latest_peak, b.samples.len()),
                        Err(_) => (0.0, 0),
                    }
                };
                let _ = app.emit("recording_amplitude", serde_json::json!({ "level": peak }));
                if len >= max_samples {
                    info!("[recorder] reached max duration, auto-stop");
                    break;
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    // 停止采集（drop stream）
    drop(stream);

    // 取出缓冲（截断到上限）
    let samples: Vec<i16> = match buf.lock() {
        Ok(mut b) => {
            b.samples.truncate(max_samples);
            std::mem::take(&mut b.samples)
        }
        Err(_) => Vec::new(),
    };
    let duration_ms = if sample_rate > 0 {
        (samples.len() as u64 * 1000 / sample_rate as u64) as u32
    } else {
        0
    };

    // 先恢复状态 + 清空句柄，避免后续操作长时间占用
    finish_idle(&app, &state, &handle);

    if cancelled {
        info!("[recorder] cancelled, no buffer retained");
        let _ = app.emit(
            "recording_finished",
            serde_json::json!({ "target": target, "cancelled": true, "durationMs": duration_ms }),
        );
        return;
    }

    // 阶段 18 剪裁：不立即写文件，改为 base64 编码推送前端 + 存缓冲待剪裁
    let samples_base64 = samples_to_base64(&samples);
    info!(
        "[recorder] recording completed: {} samples, {} ms, base64 {} bytes",
        samples.len(),
        duration_ms,
        samples_base64.len()
    );

    // 存到 AppState.recording_buffer 供 save_trimmed 命令读取
    if let Ok(s) = state.lock() {
        if let Ok(mut buf) = s.recording_buffer.lock() {
            *buf = Some((samples, sample_rate));
        }
    }

    // 推送完整数据到前端进入剪裁态
    let _ = app.emit(
        "recording_finished",
        serde_json::json!({
            "target": target,
            "cancelled": false,
            "durationMs": duration_ms,
            "samplesBase64": samples_base64,
            "sampleRate": sample_rate,
        }),
    );
}

/// 将 i16 PCM 数组编码为 base64（用于前端 Web Audio）。
fn samples_to_base64(samples: &[i16]) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    STANDARD.encode(&bytes)
}

/// 保存剪裁后的音频 — 阶段 18 剪裁命令。
///
/// 从 AppState.recording_buffer 读取全程 PCM，截取 [startMs, endMs) 片段，写 WAV。
pub fn save_trimmed_audio(
    state: SharedState,
    target: String,
    start_ms: u32,
    end_ms: u32,
) -> Result<(), String> {
    let file_name = match target.as_str() {
        "start" => crate::sound::FILE_START,
        "stop" => crate::sound::FILE_STOP,
        _ => return Err("invalid target".to_string()),
    };

    let (samples, sample_rate) = {
        let s = state.lock().map_err(|e| format!("lock state: {}", e))?;
        let buf_guard = s
            .recording_buffer
            .lock()
            .map_err(|e| format!("lock buffer: {}", e))?;
        match buf_guard.as_ref() {
            Some((samples, sr)) => (samples.clone(), *sr),
            None => return Err("no recording buffer".to_string()),
        }
    };

    if start_ms >= end_ms {
        return Err("invalid trim range".to_string());
    }

    let start_idx = ((start_ms as u64 * sample_rate as u64) / 1000) as usize;
    let end_idx = ((end_ms as u64 * sample_rate as u64) / 1000) as usize;
    let trimmed = &samples[start_idx.min(samples.len())..end_idx.min(samples.len())];

    if trimmed.is_empty() {
        return Err("trimmed audio is empty".to_string());
    }

    info!(
        "[recorder] saving trimmed: {}ms ~ {}ms ({} samples)",
        start_ms,
        end_ms,
        trimmed.len()
    );

    // 写前停止正在播放的提示音以释放文件句柄
    crate::sound::purge_playing();

    let dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .ok_or_else(|| "no exe dir".to_string())?;
    let audio_dir = dir.join("audio");
    let final_path = audio_dir.join(file_name);

    write_wav(&final_path, trimmed, sample_rate)?;
    info!("[recorder] trimmed audio saved to {}", final_path.display());

    // 写盘成功后刷新内存常驻缓存 — DESIGN 18.4
    crate::sound::reload_cache(file_name);

    // 清空缓冲
    if let Ok(s) = state.lock() {
        if let Ok(mut buf) = s.recording_buffer.lock() {
            *buf = None;
        }
    }

    Ok(())
}

/// 恢复运行状态到页面就绪态并清空录制句柄。
fn finish_idle(app: &AppHandle, state: &SharedState, handle: &RecordingHandle) {
    if let Ok(mut h) = handle.lock() {
        *h = None;
    }
    let new_status = {
        match state.lock() {
            Ok(mut s) => {
                // 录制仅在设置页发起，恢复为 Idle（设置页非可触发模拟页）
                s.runtime_status = match s.current_page.as_str() {
                    "keyboard" => RuntimeStatus::ReadyKeyboard,
                    "mouse" => RuntimeStatus::ReadyMouse,
                    _ => RuntimeStatus::Idle,
                };
                s.runtime_status.clone()
            }
            Err(_) => RuntimeStatus::Idle,
        }
    };
    let _ = app.emit(
        "runtime_status_changed",
        serde_json::json!({ "status": new_status }),
    );
}

/// 构建 cpal 输入流，回调内降为 mono i16 累积并记录峰值。
fn build_input_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_format: SampleFormat,
    channels: usize,
    buf: Arc<Mutex<RecBuf>>,
) -> Result<cpal::Stream, String> {
    let err_fn = |e| error!("[recorder] stream error: {}", e);
    let ch = channels.max(1);

    macro_rules! build {
        ($ty:ty, $to_i16:expr) => {{
            let buf = buf.clone();
            device
                .build_input_stream(
                    config,
                    move |data: &[$ty], _: &cpal::InputCallbackInfo| {
                        let conv: fn($ty) -> i16 = $to_i16;
                        if let Ok(mut b) = buf.lock() {
                            let mut peak: i32 = 0;
                            // 每 ch 个样本取第 0 声道降为 mono
                            for frame in data.chunks(ch) {
                                let s = conv(frame[0]);
                                b.samples.push(s);
                                let a = (s as i32).abs();
                                if a > peak {
                                    peak = a;
                                }
                            }
                            b.latest_peak = peak as f32 / i16::MAX as f32;
                        }
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| format!("build_input_stream: {}", e))
        }};
    }

    match sample_format {
        SampleFormat::F32 => build!(f32, |s: f32| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16),
        SampleFormat::I16 => build!(i16, |s: i16| s),
        SampleFormat::U16 => build!(u16, |s: u16| (s as i32 - 32768) as i16),
        other => Err(format!("unsupported sample format: {:?}", other)),
    }
}

/// 写 16-bit mono PCM WAV：先写 .tmp 再原子 rename。
fn write_wav(
    final_path: &std::path::Path,
    samples: &[i16],
    sample_rate: u32,
) -> Result<(), String> {
    let tmp_path = final_path.with_extension("wav.tmp");
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    {
        let mut writer =
            hound::WavWriter::create(&tmp_path, spec).map_err(|e| format!("create wav: {}", e))?;
        for &s in samples {
            writer
                .write_sample(s)
                .map_err(|e| format!("write sample: {}", e))?;
        }
        writer.finalize().map_err(|e| format!("finalize: {}", e))?;
    }
    std::fs::rename(&tmp_path, final_path).map_err(|e| {
        // rename 失败时清理临时文件
        let _ = std::fs::remove_file(&tmp_path);
        format!("rename: {}", e)
    })?;
    Ok(())
}
