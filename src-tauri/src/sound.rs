// 热键提示音 — 启动/停止热键生效时播放
//
// 仅 Windows。使用 waveOut API 直接播放内存 PCM，预先打开设备 + 预先准备缓冲，
// 触发时仅需 waveOutReset + waveOutWrite，端到端延迟 < 15ms。
//
// 声音文件位于 exe 同级 data/audio 目录：
//   - data/audio/按键开启.wav —— 启动热键生效（进入 Running*）时播放
//   - data/audio/按键关闭.wav —— 停止热键生效（Running* → Idle）时播放
//
// 低延迟策略：
//   1. 启动时 waveOutOpen 打开设备（44100/16-bit/mono），设备常驻不关闭。
//   2. 加载 wav 文件后解析出 PCM 数据，waveOutPrepareHeader 预备缓冲。
//   3. 触发时 waveOutReset（打断旧播放）+ waveOutWrite（队列新缓冲），~5ms 完成。
//   4. 无需 keepalive — 设备始终处于打开状态，无冷启动开销。
//   5. 录制保存先预构建候选设备与缓冲，再原子发布文件并切换缓存。

fn publish_candidate<T, F>(
    slot: &mut Option<T>,
    candidate: T,
    replace_file: F,
) -> Result<Option<T>, String>
where
    F: FnOnce() -> Result<(), String>,
{
    replace_file()?;
    Ok(slot.replace(candidate))
}

#[cfg(windows)]
mod inner {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock};
    use windows_sys::Win32::Media::Audio::{
        waveOutClose, waveOutOpen, waveOutPrepareHeader, waveOutReset, waveOutUnprepareHeader,
        waveOutWrite, CALLBACK_NULL, HWAVEOUT, WAVEFORMATEX, WAVEHDR, WAVE_FORMAT_PCM, WAVE_MAPPER,
    };

    pub const FILE_START: &str = "按键开启.wav";
    pub const FILE_STOP: &str = "按键关闭.wav";

    struct PreparedBuf {
        hdr: Box<WAVEHDR>,
        _pcm: Arc<Vec<u8>>,
    }

    // SAFETY: WAVEHDR + HWAVEOUT are thread-safe when access is serialized by Mutex.
    unsafe impl Send for PreparedBuf {}

    /// wav 文件解析结果
    #[derive(Clone, Copy)]
    struct WavInfo {
        pcm_offset: usize,
        pcm_len: usize,
        channels: u16,
        sample_rate: u32,
        bits_per_sample: u16,
    }

    struct WaveDevice {
        handle: HWAVEOUT,
        bufs: HashMap<&'static str, PreparedBuf>,
    }

    unsafe impl Send for WaveDevice {}

    static DEVICE: OnceLock<Mutex<Option<WaveDevice>>> = OnceLock::new();

    fn device_mutex() -> &'static Mutex<Option<WaveDevice>> {
        DEVICE.get_or_init(|| Mutex::new(None))
    }

    fn audio_file_path(file_name: &str) -> Option<std::path::PathBuf> {
        crate::paths::PortablePaths::current()
            .ok()
            .map(|paths| paths.audio_dir().join(file_name))
    }

    fn read_wav_bytes(file_name: &str) -> Option<Vec<u8>> {
        let path = audio_file_path(file_name)?;
        if let Err(error) = crate::paths::ensure_regular_file_or_missing(&path) {
            log::error!("[sound] rejected audio file target: {error}");
            return None;
        }
        if !path.exists() {
            log::warn!("[sound] file not found: {}", path.display());
            return None;
        }
        match std::fs::read(&path) {
            Ok(bytes) => {
                log::info!("[sound] loaded {} ({} bytes)", path.display(), bytes.len());
                Some(bytes)
            }
            Err(e) => {
                log::error!("[sound] read failed {}: {}", path.display(), e);
                None
            }
        }
    }

    /// 从 wav 字节中解析格式信息和 PCM 数据位置。
    pub(super) fn fuzz_validate(input: &[u8]) {
        let _ = parse_wav(input);
    }

    fn parse_wav(raw: &[u8]) -> Option<WavInfo> {
        if raw.len() < 12 || raw.get(0..4)? != b"RIFF" || raw.get(8..12)? != b"WAVE" {
            return None;
        }

        let mut format: Option<(u16, u32, u16)> = None;
        let mut position = 12_usize;
        loop {
            let header_end = position.checked_add(8)?;
            if header_end > raw.len() {
                break;
            }
            let chunk_id = raw.get(position..position + 4)?;
            let size_bytes: [u8; 4] = raw.get(position + 4..header_end)?.try_into().ok()?;
            let chunk_size = u32::from_le_bytes(size_bytes) as usize;
            let data_start = header_end;

            if chunk_id == b"fmt " {
                if chunk_size < 16 || data_start.checked_add(16)? > raw.len() {
                    return None;
                }
                let format_tag =
                    u16::from_le_bytes(raw.get(data_start..data_start + 2)?.try_into().ok()?);
                if format_tag != 1 {
                    return None;
                }
                let channels =
                    u16::from_le_bytes(raw.get(data_start + 2..data_start + 4)?.try_into().ok()?);
                let sample_rate =
                    u32::from_le_bytes(raw.get(data_start + 4..data_start + 8)?.try_into().ok()?);
                let bits_per_sample =
                    u16::from_le_bytes(raw.get(data_start + 14..data_start + 16)?.try_into().ok()?);
                if !(1..=8).contains(&channels)
                    || !(8_000..=192_000).contains(&sample_rate)
                    || !matches!(bits_per_sample, 8 | 16 | 24 | 32)
                {
                    return None;
                }
                format = Some((channels, sample_rate, bits_per_sample));
            } else if chunk_id == b"data" {
                let (channels, sample_rate, bits_per_sample) = format?;
                let available = raw.len().checked_sub(data_start)?;
                let pcm_len = chunk_size.min(available);
                if pcm_len == 0 {
                    return None;
                }
                return Some(WavInfo {
                    pcm_offset: data_start,
                    pcm_len,
                    channels,
                    sample_rate,
                    bits_per_sample,
                });
            }

            let padded_size = chunk_size.checked_add(chunk_size & 1)?;
            position = data_start.checked_add(padded_size)?;
            if position > raw.len() {
                return None;
            }
        }
        None
    }
    fn open_device_with_format(
        channels: u16,
        sample_rate: u32,
        bits_per_sample: u16,
    ) -> Option<HWAVEOUT> {
        let block_align = channels.checked_mul(bits_per_sample)?.checked_div(8)?;
        let average_bytes = sample_rate.checked_mul(block_align as u32)?;
        let fmt = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_PCM as u16,
            nChannels: channels,
            nSamplesPerSec: sample_rate,
            nAvgBytesPerSec: average_bytes,
            nBlockAlign: block_align,
            wBitsPerSample: bits_per_sample,
            cbSize: 0,
        };
        let mut handle: HWAVEOUT = std::ptr::null_mut();
        let result = unsafe { waveOutOpen(&mut handle, WAVE_MAPPER, &fmt, 0, 0, CALLBACK_NULL) };
        if result == 0 {
            log::info!(
                "[sound] waveOut opened: {}ch {}Hz {}bit",
                channels,
                sample_rate,
                bits_per_sample
            );
            Some(handle)
        } else {
            log::error!("[sound] waveOutOpen failed: error {}", result);
            None
        }
    }

    fn prepare_buf(handle: HWAVEOUT, pcm_data: Arc<Vec<u8>>) -> Option<PreparedBuf> {
        let mut hdr = Box::new(WAVEHDR {
            lpData: pcm_data.as_ptr() as *mut u8,
            dwBufferLength: u32::try_from(pcm_data.len()).ok()?,
            dwBytesRecorded: 0,
            dwUser: 0,
            dwFlags: 0,
            dwLoops: 0,
            lpNext: std::ptr::null_mut(),
            reserved: 0,
        });
        let result = unsafe {
            waveOutPrepareHeader(
                handle,
                hdr.as_mut() as *mut WAVEHDR,
                std::mem::size_of::<WAVEHDR>() as u32,
            )
        };
        if result != 0 {
            log::error!("[sound] waveOutPrepareHeader failed: {}", result);
            return None;
        }
        Some(PreparedBuf {
            hdr,
            _pcm: pcm_data,
        })
    }

    fn build_device_for(file_name: &'static str, raw: Vec<u8>) -> Result<WaveDevice, String> {
        let info = parse_wav(&raw).ok_or_else(|| format!("invalid wav: {file_name}"))?;
        let format = (info.channels, info.sample_rate, info.bits_per_sample);
        let handle = open_device_with_format(info.channels, info.sample_rate, info.bits_per_sample)
            .ok_or_else(|| format!("open audio device failed for {file_name}"))?;
        let mut device = WaveDevice {
            handle,
            bufs: HashMap::new(),
        };

        let pcm_end = info
            .pcm_offset
            .checked_add(info.pcm_len)
            .ok_or_else(|| format!("wav pcm range overflow: {file_name}"))?;
        let pcm = Arc::new(
            raw.get(info.pcm_offset..pcm_end)
                .ok_or_else(|| format!("invalid wav pcm range: {file_name}"))?
                .to_vec(),
        );
        let target = prepare_buf(device.handle, pcm)
            .ok_or_else(|| format!("prepare audio buffer failed: {file_name}"))?;
        device.bufs.insert(file_name, target);

        let other = if file_name == FILE_START {
            FILE_STOP
        } else {
            FILE_START
        };
        if let Some(other_raw) = read_wav_bytes(other) {
            if let Some(other_info) = parse_wav(&other_raw) {
                let other_format = (
                    other_info.channels,
                    other_info.sample_rate,
                    other_info.bits_per_sample,
                );
                if other_format == format {
                    let other_end = other_info
                        .pcm_offset
                        .checked_add(other_info.pcm_len)
                        .ok_or_else(|| format!("wav pcm range overflow: {other}"))?;
                    let other_pcm = Arc::new(
                        other_raw
                            .get(other_info.pcm_offset..other_end)
                            .ok_or_else(|| format!("invalid wav pcm range: {other}"))?
                            .to_vec(),
                    );
                    if let Some(buffer) = prepare_buf(device.handle, other_pcm) {
                        device.bufs.insert(other, buffer);
                    } else {
                        log::warn!("[sound] optional buffer prepare failed: {}", other);
                    }
                } else {
                    log::warn!(
                        "[sound] {} format {:?} differs from device {:?}, skipping",
                        other,
                        other_format,
                        format
                    );
                }
            } else {
                log::warn!("[sound] optional wav is invalid: {}", other);
            }
        }

        Ok(device)
    }

    /// 打开 waveOut 设备并加载提示音缓冲。
    pub fn init() -> Result<(), String> {
        let mut selected = None;
        for file_name in [FILE_START, FILE_STOP] {
            if let Some(raw) = read_wav_bytes(file_name) {
                if parse_wav(&raw).is_some() {
                    selected = Some((file_name, raw));
                    break;
                }
                log::warn!("[sound] invalid wav file: {}", file_name);
            }
        }
        let (file_name, raw) = selected.ok_or_else(|| "no valid wav files found".to_string())?;
        let candidate = build_device_for(file_name, raw)?;
        let mut guard = device_mutex()
            .lock()
            .map_err(|_| "audio device lock poisoned".to_string())?;
        let old = guard.replace(candidate);
        drop(guard);
        drop(old);
        log::info!("[sound] audio warmup completed");
        Ok(())
    }
    pub fn play_file(file_name: &str) {
        let mut guard = match device_mutex().lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        let dev = match guard.as_mut() {
            Some(d) => d,
            None => {
                log::warn!("[sound] device not initialized");
                return;
            }
        };
        let buf = match dev.bufs.get_mut(file_name) {
            Some(b) => b,
            None => {
                log::warn!("[sound] no buffer for {}", file_name);
                return;
            }
        };
        unsafe {
            // 打断正在播放的旧声音
            waveOutReset(dev.handle);
            // 重置 flags 以便重新提交（WHDR_DONE 清除）
            buf.hdr.dwFlags &= !0x01; // clear WHDR_DONE
            buf.hdr.dwFlags |= 0x02; // ensure WHDR_PREPARED stays set
            waveOutWrite(
                dev.handle,
                buf.hdr.as_mut() as *mut WAVEHDR,
                std::mem::size_of::<WAVEHDR>() as u32,
            );
        }
    }

    /// 在新设备和缓冲全部准备完成后，原子发布 WAV 文件并切换内存缓存。
    pub fn commit_wav(
        file_name: &'static str,
        temporary: &std::path::Path,
        final_path: &std::path::Path,
    ) -> Result<(), String> {
        if !matches!(file_name, FILE_START | FILE_STOP) {
            return Err("invalid audio target".to_string());
        }
        crate::paths::ensure_regular_file_or_missing(temporary)?;
        crate::paths::ensure_regular_file_or_missing(final_path)?;
        let raw = std::fs::read(temporary)
            .map_err(|error| format!("read audio candidate failed: {error}"))?;
        let candidate = build_device_for(file_name, raw)?;
        let mut guard = device_mutex()
            .lock()
            .map_err(|_| "audio device lock poisoned".to_string())?;

        let old = super::publish_candidate(&mut guard, candidate, || {
            crate::paths::atomic_replace(temporary, final_path)
        })?;
        drop(guard);
        drop(old);
        log::info!("[sound] committed file and cache for {}", file_name);
        Ok(())
    }
    /// 停止当前播放。
    pub fn purge_playing() {
        let guard = match device_mutex().lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if let Some(dev) = guard.as_ref() {
            unsafe {
                waveOutReset(dev.handle);
            }
        }
    }

    /// 查询提示音文件是否存在。
    pub fn sound_files_exist() -> (bool, bool) {
        match crate::paths::PortablePaths::current() {
            Ok(paths) => {
                let audio_dir = paths.audio_dir();
                let start = audio_dir.join(FILE_START);
                let stop = audio_dir.join(FILE_STOP);
                (
                    start.exists() && crate::paths::ensure_regular_file_or_missing(&start).is_ok(),
                    stop.exists() && crate::paths::ensure_regular_file_or_missing(&stop).is_ok(),
                )
            }
            Err(_) => (false, false),
        }
    }

    impl Drop for WaveDevice {
        fn drop(&mut self) {
            unsafe {
                waveOutReset(self.handle);
                for (_, buf) in self.bufs.iter_mut() {
                    waveOutUnprepareHeader(
                        self.handle,
                        buf.hdr.as_mut() as *mut WAVEHDR,
                        std::mem::size_of::<WAVEHDR>() as u32,
                    );
                }
                waveOutClose(self.handle);
            }
        }
    }
    #[cfg(test)]
    mod tests {
        use super::parse_wav;

        fn minimal_pcm_wav() -> Vec<u8> {
            let mut wav = Vec::new();
            wav.extend_from_slice(b"RIFF");
            wav.extend_from_slice(&38_u32.to_le_bytes());
            wav.extend_from_slice(b"WAVEfmt ");
            wav.extend_from_slice(&16_u32.to_le_bytes());
            wav.extend_from_slice(&1_u16.to_le_bytes());
            wav.extend_from_slice(&1_u16.to_le_bytes());
            wav.extend_from_slice(&44_100_u32.to_le_bytes());
            wav.extend_from_slice(&88_200_u32.to_le_bytes());
            wav.extend_from_slice(&2_u16.to_le_bytes());
            wav.extend_from_slice(&16_u16.to_le_bytes());
            wav.extend_from_slice(b"data");
            wav.extend_from_slice(&2_u32.to_le_bytes());
            wav.extend_from_slice(&0_i16.to_le_bytes());
            wav
        }

        #[test]
        fn parses_minimal_pcm_wav() {
            let wav = minimal_pcm_wav();
            let info = parse_wav(&wav).unwrap();
            assert_eq!(info.pcm_len, 2);
            assert_eq!(info.channels, 1);
            assert_eq!(info.sample_rate, 44_100);
            assert_eq!(info.bits_per_sample, 16);
        }

        #[test]
        fn every_truncated_prefix_is_rejected_without_panic() {
            let wav = minimal_pcm_wav();
            for length in 0..wav.len() {
                assert!(std::panic::catch_unwind(|| parse_wav(&wav[..length])).is_ok());
            }
        }

        #[test]
        fn oversized_chunk_length_is_rejected_without_panic() {
            let mut wav = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
            wav.extend_from_slice(&u32::MAX.to_le_bytes());
            assert!(parse_wav(&wav).is_none());
        }

        #[test]
        fn deterministic_malformed_inputs_never_panic() {
            let mut seed = 0x8d26_1f4b_u32;
            for length in 0..512 {
                let mut bytes = vec![0_u8; length];
                for byte in &mut bytes {
                    seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    *byte = (seed >> 24) as u8;
                }
                if length >= 12 && length % 3 == 0 {
                    bytes[0..4].copy_from_slice(b"RIFF");
                    bytes[8..12].copy_from_slice(b"WAVE");
                }
                assert!(std::panic::catch_unwind(|| parse_wav(&bytes)).is_ok());
            }
        }
    }
}

/// 启动提示音文件名。
pub const FILE_START: &str = "按键开启.wav";
/// 停止提示音文件名。
pub const FILE_STOP: &str = "按键关闭.wav";

pub struct AudioWarmupHandle {
    join: std::sync::Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl Drop for AudioWarmupHandle {
    fn drop(&mut self) {
        if let Ok(join) = self.join.get_mut() {
            if let Some(join) = join.take() {
                let _ = join.join();
            }
        }
    }
}

/// 后台静默完成 WAV 读取、设备打开和播放缓冲准备，并回报健康结果。
pub fn warm_up_in_background<F>(on_complete: F) -> Result<AudioWarmupHandle, String>
where
    F: FnOnce(Result<(), String>) + Send + 'static,
{
    let join = std::thread::Builder::new()
        .name("mimic-audio-warmup".to_string())
        .spawn(move || on_complete(init()))
        .map_err(|error| format!("failed to spawn audio warmup: {error}"))?;
    Ok(AudioWarmupHandle {
        join: std::sync::Mutex::new(Some(join)),
    })
}

/// 初始化音频设备并加载提示音缓冲 — 仅由后台预热线程调用。
#[cfg(windows)]
pub fn init() -> Result<(), String> {
    inner::init()
}

#[cfg(not(windows))]
pub fn init() -> Result<(), String> {
    Ok(())
}

/// 播放启动提示音。
#[cfg(windows)]
pub fn play_start() {
    inner::play_file(inner::FILE_START);
}

/// 播放停止提示音。
#[cfg(windows)]
pub fn play_stop() {
    inner::play_file(inner::FILE_STOP);
}

#[cfg(not(windows))]
pub fn play_start() {}

#[cfg(not(windows))]
pub fn play_stop() {}

/// 停止当前播放 — 录制覆盖前调用。
#[cfg(windows)]
pub fn purge_playing() {
    inner::purge_playing();
}

#[cfg(not(windows))]
pub fn purge_playing() {}

/// 原子发布候选 WAV，并仅在发布成功后切换常驻播放缓存。
#[cfg(windows)]
pub fn commit_wav(
    file_name: &'static str,
    temporary: &std::path::Path,
    final_path: &std::path::Path,
) -> Result<(), String> {
    inner::commit_wav(file_name, temporary, final_path)
}

#[cfg(not(windows))]
pub fn commit_wav(
    _file_name: &'static str,
    temporary: &std::path::Path,
    final_path: &std::path::Path,
) -> Result<(), String> {
    crate::paths::atomic_replace(temporary, final_path)
}

/// 查询提示音文件是否存在。
pub fn sound_files_exist() -> (bool, bool) {
    #[cfg(windows)]
    {
        inner::sound_files_exist()
    }
    #[cfg(not(windows))]
    {
        (false, false)
    }
}
/// Side-effect-free WAV parser entry used by bounded and continuous fuzzing.
pub(crate) fn fuzz_validate_wav_bytes(input: &[u8]) {
    #[cfg(windows)]
    {
        inner::fuzz_validate(input);
    }
    #[cfg(not(windows))]
    {
        let _ = input;
    }
}

#[cfg(test)]
mod transaction_tests {
    use super::publish_candidate;

    #[test]
    fn failed_file_publish_keeps_old_audio_candidate() {
        let mut slot = Some("old");
        let result = publish_candidate(&mut slot, "new", || {
            Err("fault-injected audio replace failure".to_string())
        });

        assert_eq!(
            result,
            Err("fault-injected audio replace failure".to_string())
        );
        assert_eq!(slot, Some("old"));
    }

    #[test]
    fn successful_file_publish_swaps_audio_candidate_once() {
        let mut slot = Some("old");
        let replaced = publish_candidate(&mut slot, "new", || Ok(())).unwrap();

        assert_eq!(replaced, Some("old"));
        assert_eq!(slot, Some("new"));
    }
}
