// 提示音录制 — DESIGN 20 / 阶段 18
//
// 用 cpal 采集系统默认麦克风，累积 i16/mono PCM 到内存缓冲，停止时用 hound
// 写入 WAV 并覆盖 exe 同级 data/audio 目录下的 `按键开启.wav` / `按键关闭.wav`。
//
// 线程模型：cpal 的 Stream 是 !Send，无法跨命令存放，因此由一个专用录制线程
// 创建并持有 Stream，命令通过 channel 发停止/取消信号；音频缓冲与最新峰值经
// Arc<Mutex<>> 共享。波形幅度由录制线程按 ~30fps 经事件推送，避免在音频回调里
// 直接 emit。

use crate::state::{ActivityLease, SharedState};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use log::{error, info};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// 最大录制时长（秒）— REQUIREMENTS 3.14
const MAX_DURATION_SECS: u32 = 5;
/// 波形 / 自动停止检查间隔（约 30fps）
const TICK_MS: u64 = 33;
static AUDIO_TEMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn emit_recorder_event(app: &AppHandle, event: &str, payload: serde_json::Value) {
    if let Err(error) = app.emit(event, payload) {
        error!("[recorder] failed to emit {event}: {error}");
    }
}

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

struct RecordingTask {
    token: u64,
    control: Option<Sender<RecCtrl>>,
    join: Option<JoinHandle<()>>,
    finished: bool,
    activity_lease: Option<ActivityLease>,
}

struct RecordingController {
    next_token: u64,
    active: Option<RecordingTask>,
}

struct RecordingHandleInner {
    controller: Mutex<RecordingController>,
}

impl Drop for RecordingHandleInner {
    fn drop(&mut self) {
        let Ok(controller) = self.controller.get_mut() else {
            return;
        };
        if let Some(mut task) = controller.active.take() {
            if let Some(control) = task.control.take() {
                if control.send(RecCtrl::Cancel).is_err() {
                    log::debug!("[recorder] worker already stopped during controller drop");
                }
            }
            if let Some(join) = task.join.take() {
                if join.join().is_err() {
                    error!("[recorder] worker panicked during controller drop");
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct RecordingHandle {
    inner: Arc<RecordingHandleInner>,
}

pub fn new_handle() -> RecordingHandle {
    RecordingHandle {
        inner: Arc::new(RecordingHandleInner {
            controller: Mutex::new(RecordingController {
                next_token: 1,
                active: None,
            }),
        }),
    }
}

/// 开始录制 — DESIGN 20.5
///
/// target: "start" -> 按键开启.wav, "stop" -> 按键关闭.wav。
/// 运行态守卫在 lib.rs 命令层完成；此处再做设备可用性检查。
pub fn start_recording(
    app: AppHandle,
    state: SharedState,
    handle: RecordingHandle,
    activity_lease: ActivityLease,
    target: String,
) -> Result<(), String> {
    if !matches!(target.as_str(), "start" | "stop") {
        return Err("invalid target".to_string());
    }

    // 回收上一轮已完成线程；活动线程则拒绝重复启动。
    let previous_join = {
        let mut controller = handle
            .inner
            .controller
            .lock()
            .map_err(|error| format!("lock recording controller: {error}"))?;
        match controller.active.as_ref() {
            Some(task) if !task.finished => {
                return Err("recording already in progress".to_string());
            }
            Some(_) => controller
                .active
                .take()
                .and_then(|mut task| task.join.take()),
            None => None,
        }
    };
    if let Some(join) = previous_join {
        join.join()
            .map_err(|_| "previous recording thread panicked".to_string())?;
    }

    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| "no_input_device".to_string())?;
    let default_config = device
        .default_input_config()
        .map_err(|error| format!("default_input_config: {error}"))?;
    let sample_rate = default_config.sample_rate().0;
    let channels = default_config.channels() as usize;
    let sample_format = default_config.sample_format();
    if !(8_000..=192_000).contains(&sample_rate) || !(1..=8).contains(&channels) {
        return Err("unsupported_input_format".to_string());
    }
    let config: cpal::StreamConfig = default_config.into();

    info!(
        "[recorder] start target={} rate={} ch={} fmt={:?}",
        target, sample_rate, channels, sample_format
    );

    let token = {
        let mut controller = handle
            .inner
            .controller
            .lock()
            .map_err(|error| format!("lock recording controller: {error}"))?;
        let token = controller.next_token;
        controller.next_token = controller.next_token.saturating_add(1);
        token
    };
    let (control_tx, control_rx) = std::sync::mpsc::channel::<RecCtrl>();
    let (start_tx, start_rx) = std::sync::mpsc::sync_channel(1);
    let buffer = Arc::new(Mutex::new(RecBuf {
        samples: Vec::with_capacity((sample_rate * MAX_DURATION_SECS) as usize),
        latest_peak: 0.0,
    }));
    let worker_handle = Arc::downgrade(&handle.inner);
    let join = std::thread::Builder::new()
        .name(format!("mimic-recorder-{token}"))
        .spawn(move || {
            if start_rx.recv().is_err() {
                return;
            }
            run_recording_thread(
                app,
                state,
                worker_handle,
                token,
                buffer,
                control_rx,
                device,
                config,
                sample_format,
                channels,
                sample_rate,
                target,
            );
        })
        .map_err(|error| format!("failed to spawn recording thread: {error}"))?;

    {
        let mut controller = handle
            .inner
            .controller
            .lock()
            .map_err(|error| format!("lock recording controller: {error}"))?;
        controller.active = Some(RecordingTask {
            token,
            control: Some(control_tx),
            join: Some(join),
            finished: false,
            activity_lease: Some(activity_lease),
        });
    }
    if start_tx.send(()).is_err() {
        let failed_task = {
            let mut controller = handle
                .inner
                .controller
                .lock()
                .map_err(|error| format!("lock recording controller: {error}"))?;
            if controller.active.as_ref().map(|task| task.token) == Some(token) {
                controller.active.take()
            } else {
                None
            }
        };
        if let Some(mut task) = failed_task {
            task.control.take();
            if let Some(join) = task.join.take() {
                if join.join().is_err() {
                    error!("[recorder] failed-start worker panicked");
                }
            }
        }
        return Err("recording thread failed before startup".to_string());
    }
    Ok(())
}

/// 停止录制（保存）— 仅发信号，结果经 recording_finished 事件返回。
pub fn stop_recording(handle: &RecordingHandle) -> Result<(), String> {
    send_control(handle, RecCtrl::Stop)
}

/// 取消录制（不写文件）。
pub fn cancel_recording(handle: &RecordingHandle) -> Result<(), String> {
    send_control(handle, RecCtrl::Cancel)
}

fn send_control(handle: &RecordingHandle, command: RecCtrl) -> Result<(), String> {
    let controller = handle
        .inner
        .controller
        .lock()
        .map_err(|error| format!("lock recording controller: {error}"))?;
    let control = controller
        .active
        .as_ref()
        .filter(|task| !task.finished)
        .and_then(|task| task.control.as_ref())
        .ok_or_else(|| "no recording in progress".to_string())?;
    control
        .send(command)
        .map_err(|_| "recording worker unavailable".to_string())
}
#[allow(clippy::too_many_arguments)]
fn run_recording_thread(
    app: AppHandle,
    state: SharedState,
    handle: std::sync::Weak<RecordingHandleInner>,
    token: u64,
    buf: Arc<Mutex<RecBuf>>,
    ctrl_rx: Receiver<RecCtrl>,
    device: cpal::Device,
    config: cpal::StreamConfig,
    sample_format: SampleFormat,
    channels: usize,
    sample_rate: u32,
    target: String,
) {
    // 构建输入流（回调内降为 mono i16 累积 + 记录峰值）
    let stream = match build_input_stream(&device, &config, sample_format, channels, buf.clone()) {
        Ok(s) => s,
        Err(e) => {
            error!("[recorder] build stream failed: {}", e);
            finish_idle(&app, &state, &handle, token);
            emit_recorder_event(&app, "recording_error", serde_json::json!({ "error": e }));
            return;
        }
    };
    if let Err(e) = stream.play() {
        error!("[recorder] stream.play failed: {}", e);
        finish_idle(&app, &state, &handle, token);
        emit_recorder_event(
            &app,
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
                let (peak, len) = match buf.lock() {
                    Ok(buffer) => (buffer.latest_peak, buffer.samples.len()),
                    Err(error) => {
                        error!("[recorder] sample buffer poisoned: {error}");
                        emit_recorder_event(
                            &app,
                            "recording_error",
                            serde_json::json!({ "error": "sample_buffer_unavailable" }),
                        );
                        cancelled = true;
                        break;
                    }
                };
                emit_recorder_event(
                    &app,
                    "recording_amplitude",
                    serde_json::json!({ "level": peak }),
                );
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
        Ok(mut buffer) => {
            buffer.samples.truncate(max_samples);
            std::mem::take(&mut buffer.samples)
        }
        Err(error) => {
            error!("[recorder] failed to take sample buffer: {error}");
            emit_recorder_event(
                &app,
                "recording_error",
                serde_json::json!({ "error": "sample_buffer_unavailable" }),
            );
            finish_idle(&app, &state, &handle, token);
            return;
        }
    };
    let duration_ms = if sample_rate > 0 {
        (samples.len() as u64 * 1000 / sample_rate as u64) as u32
    } else {
        0
    };

    if cancelled {
        info!("[recorder] cancelled, no buffer retained");
        emit_recorder_event(
            &app,
            "recording_finished",
            serde_json::json!({ "target": target, "cancelled": true, "durationMs": duration_ms }),
        );
        finish_idle(&app, &state, &handle, token);
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

    // 存到 AppState.recording_buffer 供 save_trimmed 命令读取。
    let store_result = (|| {
        let app_state = state
            .lock()
            .map_err(|error| format!("lock state after recording: {error}"))?;
        let mut recording_buffer = app_state
            .recording_buffer
            .lock()
            .map_err(|error| format!("lock recording buffer: {error}"))?;
        *recording_buffer = Some((samples, sample_rate));
        Ok::<(), String>(())
    })();
    if let Err(error) = store_result {
        error!("[recorder] failed to publish recording buffer: {error}");
        emit_recorder_event(
            &app,
            "recording_error",
            serde_json::json!({ "error": "recording_buffer_unavailable" }),
        );
        finish_idle(&app, &state, &handle, token);
        return;
    }

    // 推送完整数据到前端进入剪裁态。
    emit_recorder_event(
        &app,
        "recording_finished",
        serde_json::json!({
            "target": target,
            "cancelled": false,
            "durationMs": duration_ms,
            "samplesBase64": samples_base64,
            "sampleRate": sample_rate,
        }),
    );

    // 数据和完成事件均已发布后才释放活动租约，防止新录音与旧任务尾部交叠。
    finish_idle(&app, &state, &handle, token);
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

    let paths = crate::paths::PortablePaths::current()?;
    paths.ensure_data_dirs()?;
    let final_path = paths.audio_dir().join(file_name);
    crate::paths::ensure_regular_file_or_missing(&final_path)?;

    let temporary = write_wav_candidate(trimmed, sample_rate)?;
    let commit_result = crate::sound::commit_wav(file_name, &temporary, &final_path);
    if commit_result.is_err() {
        match std::fs::remove_file(&temporary) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => log::warn!(
                "[recorder] failed to remove rejected WAV {}: {error}",
                temporary.display()
            ),
        }
    }
    commit_result?;
    info!(
        "[recorder] trimmed audio and memory cache committed to {}",
        final_path.display()
    );

    // 清空已提交的录音缓冲；失败时不能伪装为完整成功。
    let app_state = state
        .lock()
        .map_err(|error| format!("lock state after audio save: {error}"))?;
    let mut recording_buffer = app_state
        .recording_buffer
        .lock()
        .map_err(|error| format!("lock recording buffer after save: {error}"))?;
    *recording_buffer = None;

    Ok(())
}

/// 恢复运行状态到页面就绪态并清空录制句柄。
fn finish_idle(
    app: &AppHandle,
    state: &SharedState,
    handle: &std::sync::Weak<RecordingHandleInner>,
    token: u64,
) {
    let lease = handle.upgrade().and_then(|handle| {
        let mut controller = handle.controller.lock().ok()?;
        let task = controller.active.as_mut()?;
        (task.token == token).then(|| {
            task.control = None;
            task.finished = true;
            task.activity_lease.take()
        })?
    });
    let Some(lease) = lease else {
        return;
    };

    // 先释放活动租约，再读取派生状态；陈旧 token 无法释放新会话。
    drop(lease);
    let new_status = match state.lock() {
        Ok(state) => state.runtime_status(),
        Err(error) => {
            error!("[recorder] failed to derive status after finish: {error}");
            return;
        }
    };
    emit_recorder_event(
        app,
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

/// 将 16-bit mono PCM WAV 写入并同步到 data/temp，返回待提交候选文件。
fn write_wav_candidate(samples: &[i16], sample_rate: u32) -> Result<std::path::PathBuf, String> {
    if !(8_000..=192_000).contains(&sample_rate)
        || samples.len() > sample_rate as usize * MAX_DURATION_SECS as usize
    {
        return Err("invalid audio bounds".to_string());
    }

    let paths = crate::paths::PortablePaths::current()?;
    paths.ensure_data_dirs()?;
    let counter = AUDIO_TEMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temporary = paths
        .temp_dir()
        .join(format!("audio-{}-{counter}.wav", std::process::id()));
    let result = (|| {
        let specification = hound::WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| format!("create wav: {error}"))?;
        let mut writer = hound::WavWriter::new(file, specification)
            .map_err(|error| format!("initialize wav: {error}"))?;
        for sample in samples {
            writer
                .write_sample(*sample)
                .map_err(|error| format!("write sample: {error}"))?;
        }
        writer
            .finalize()
            .map_err(|error| format!("finalize wav: {error}"))?;

        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| format!("reopen wav: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("flush wav: {error}"))?;
        let reader =
            hound::WavReader::open(&temporary).map_err(|error| format!("validate wav: {error}"))?;
        let spec = reader.spec();
        if spec.channels != 1
            || spec.sample_rate != sample_rate
            || spec.bits_per_sample != 16
            || spec.sample_format != hound::SampleFormat::Int
            || reader.duration() as usize != samples.len()
        {
            return Err("written wav validation failed".to_string());
        }
        drop(reader);
        Ok(temporary.clone())
    })();

    if result.is_err() {
        match std::fs::remove_file(&temporary) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => log::warn!(
                "[recorder] failed to remove invalid WAV candidate {}: {error}",
                temporary.display()
            ),
        }
    }
    result
}
