// 热键提示音 — 启动/停止热键生效时播放
//
// 仅 Windows。使用 waveOut API 直接播放内存 PCM，预先打开设备 + 预先准备缓冲，
// 触发时仅需 waveOutReset + waveOutWrite，端到端延迟 < 15ms。
//
// 声音文件位于 exe 同级 audio 目录：
//   - audio/按键开启.wav —— 启动热键生效（进入 Running*）时播放
//   - audio/按键关闭.wav —— 停止热键生效（Running* → Idle）时播放
//
// 低延迟策略：
//   1. 启动时 waveOutOpen 打开设备（44100/16-bit/mono），设备常驻不关闭。
//   2. 加载 wav 文件后解析出 PCM 数据，waveOutPrepareHeader 预备缓冲。
//   3. 触发时 waveOutReset（打断旧播放）+ waveOutWrite（队列新缓冲），~5ms 完成。
//   4. 无需 keepalive — 设备始终处于打开状态，无冷启动开销。
//   5. 录制覆盖 wav 后由 reload_cache 重新解析 + 替换缓冲。

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
    struct WavInfo {
        pcm_offset: usize,
        pcm_len: usize,
        channels: u16,
        sample_rate: u32,
        bits_per_sample: u16,
    }

    struct WaveDevice {
        handle: HWAVEOUT,
        format: (u16, u32, u16), // (channels, sample_rate, bits)
        bufs: HashMap<&'static str, PreparedBuf>,
    }

    unsafe impl Send for WaveDevice {}

    static DEVICE: OnceLock<Mutex<Option<WaveDevice>>> = OnceLock::new();

    fn device_mutex() -> &'static Mutex<Option<WaveDevice>> {
        DEVICE.get_or_init(|| Mutex::new(None))
    }

    fn audio_file_path(file_name: &str) -> Option<std::path::PathBuf> {
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|d| d.join("audio").join(file_name)))
    }

    fn read_wav_bytes(file_name: &str) -> Option<Vec<u8>> {
        let path = audio_file_path(file_name)?;
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
    fn parse_wav(raw: &[u8]) -> Option<WavInfo> {
        if raw.len() < 44 {
            return None;
        }
        if &raw[0..4] != b"RIFF" || &raw[8..12] != b"WAVE" {
            return None;
        }
        let mut channels: u16 = 0;
        let mut sample_rate: u32 = 0;
        let mut bits_per_sample: u16 = 0;
        let mut fmt_found = false;
        let mut pos = 12;
        while pos + 8 <= raw.len() {
            let chunk_id = &raw[pos..pos + 4];
            let chunk_size =
                u32::from_le_bytes([raw[pos + 4], raw[pos + 5], raw[pos + 6], raw[pos + 7]])
                    as usize;
            if chunk_id == b"fmt " && chunk_size >= 16 {
                let fmt_tag = u16::from_le_bytes([raw[pos + 8], raw[pos + 9]]);
                if fmt_tag != 1 {
                    log::warn!("[sound] not PCM format (tag={})", fmt_tag);
                    return None;
                }
                channels = u16::from_le_bytes([raw[pos + 10], raw[pos + 11]]);
                sample_rate = u32::from_le_bytes([
                    raw[pos + 12],
                    raw[pos + 13],
                    raw[pos + 14],
                    raw[pos + 15],
                ]);
                bits_per_sample = u16::from_le_bytes([raw[pos + 22], raw[pos + 23]]);
                fmt_found = true;
            }
            if chunk_id == b"data" {
                if !fmt_found {
                    return None;
                }
                let pcm_offset = pos + 8;
                let pcm_len = chunk_size.min(raw.len() - pcm_offset);
                return Some(WavInfo {
                    pcm_offset,
                    pcm_len,
                    channels,
                    sample_rate,
                    bits_per_sample,
                });
            }
            pos += 8 + ((chunk_size + 1) & !1);
        }
        None
    }

    fn open_device_with_format(
        channels: u16,
        sample_rate: u32,
        bits_per_sample: u16,
    ) -> Option<HWAVEOUT> {
        let block_align = channels * bits_per_sample / 8;
        let fmt = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_PCM as u16,
            nChannels: channels,
            nSamplesPerSec: sample_rate,
            nAvgBytesPerSec: sample_rate * block_align as u32,
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
            dwBufferLength: pcm_data.len() as u32,
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

    fn unprepare_buf(handle: HWAVEOUT, buf: &mut PreparedBuf) {
        unsafe {
            waveOutReset(handle);
            waveOutUnprepareHeader(
                handle,
                buf.hdr.as_mut() as *mut WAVEHDR,
                std::mem::size_of::<WAVEHDR>() as u32,
            );
        }
    }

    fn load_single(handle: HWAVEOUT, file_name: &'static str) -> Option<PreparedBuf> {
        let raw = read_wav_bytes(file_name)?;
        let info = parse_wav(&raw)?;
        let pcm: Vec<u8> = raw[info.pcm_offset..info.pcm_offset + info.pcm_len].to_vec();
        let pcm_arc = Arc::new(pcm);
        prepare_buf(handle, pcm_arc)
    }

    /// 打开 waveOut 设备并加载两个提示音缓冲。
    pub fn init() {
        // 先解析所有可用文件，确定格式
        let files: Vec<(&'static str, Vec<u8>, WavInfo)> = [FILE_START, FILE_STOP]
            .iter()
            .filter_map(|&name| {
                let raw = read_wav_bytes(name)?;
                let info = parse_wav(&raw)?;
                Some((name, raw, info))
            })
            .collect();

        if files.is_empty() {
            log::warn!("[sound] no valid wav files found, audio disabled");
            return;
        }

        // 用第一个文件的格式打开设备
        let first = &files[0].2;
        let handle =
            match open_device_with_format(first.channels, first.sample_rate, first.bits_per_sample)
            {
                Some(h) => h,
                None => return,
            };

        let device_format = (first.channels, first.sample_rate, first.bits_per_sample);
        let mut bufs = HashMap::new();

        for (name, raw, info) in &files {
            let file_format = (info.channels, info.sample_rate, info.bits_per_sample);
            if file_format != device_format {
                log::warn!(
                    "[sound] {} format {:?} differs from device {:?}, skipping",
                    name,
                    file_format,
                    device_format
                );
                continue;
            }
            let pcm: Vec<u8> = raw[info.pcm_offset..info.pcm_offset + info.pcm_len].to_vec();
            let pcm_arc = Arc::new(pcm);
            if let Some(buf) = prepare_buf(handle, pcm_arc) {
                bufs.insert(*name, buf);
                log::info!("[sound] prepared buffer for {}", name);
            }
        }

        let mut guard = device_mutex().lock().unwrap();
        *guard = Some(WaveDevice {
            handle,
            format: device_format,
            bufs,
        });
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

    /// 重新加载指定 wav 进缓冲 — 录制覆盖后调用。
    pub fn reload_cache(file_name: &'static str) {
        let mut guard = match device_mutex().lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        let dev = match guard.as_mut() {
            Some(d) => d,
            None => return,
        };
        // 停止播放后卸载旧缓冲
        unsafe {
            waveOutReset(dev.handle);
        }
        if let Some(mut old) = dev.bufs.remove(file_name) {
            unprepare_buf(dev.handle, &mut old);
        }
        // 读取新文件
        let raw = match read_wav_bytes(file_name) {
            Some(r) => r,
            None => return,
        };
        let info = match parse_wav(&raw) {
            Some(i) => i,
            None => return,
        };
        // 格式变化时需要重新打开设备
        let file_format = (info.channels, info.sample_rate, info.bits_per_sample);
        if file_format != dev.format {
            log::info!(
                "[sound] format changed {:?} -> {:?}, reopening device",
                dev.format,
                file_format
            );
            // 卸载所有旧缓冲
            for (_, mut buf) in dev.bufs.drain() {
                unprepare_buf(dev.handle, &mut buf);
            }
            unsafe {
                waveOutClose(dev.handle);
            }
            match open_device_with_format(info.channels, info.sample_rate, info.bits_per_sample) {
                Some(h) => {
                    dev.handle = h;
                    dev.format = file_format;
                }
                None => {
                    // 设备打开失败，整个 sound 不可用
                    *guard = None;
                    return;
                }
            }
            // 重新加载另一个文件（如果存在且格式匹配）
            let other = if file_name == FILE_START {
                FILE_STOP
            } else {
                FILE_START
            };
            if let Some(buf) = load_single(dev.handle, other) {
                dev.bufs.insert(other, buf);
            }
        }
        // 加载目标文件
        let pcm: Vec<u8> = raw[info.pcm_offset..info.pcm_offset + info.pcm_len].to_vec();
        let pcm_arc = Arc::new(pcm);
        if let Some(buf) = prepare_buf(dev.handle, pcm_arc) {
            dev.bufs.insert(file_name, buf);
            log::info!("[sound] reloaded buffer for {}", file_name);
        }
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
        let dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()));
        match dir {
            Some(d) => {
                let audio_dir = d.join("audio");
                (
                    audio_dir.join(FILE_START).exists(),
                    audio_dir.join(FILE_STOP).exists(),
                )
            }
            None => (false, false),
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
}

/// 启动提示音文件名。
pub const FILE_START: &str = "按键开启.wav";
/// 停止提示音文件名。
pub const FILE_STOP: &str = "按键关闭.wav";

/// 初始化音频设备并加载提示音缓冲 — 在 setup 阶段调用一次。
#[cfg(windows)]
pub fn init() {
    inner::init();
}

#[cfg(not(windows))]
pub fn init() {}

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

/// 重新加载指定 wav 进缓冲 — 录制覆盖后调用。
#[cfg(windows)]
pub fn reload_cache(file_name: &'static str) {
    inner::reload_cache(file_name);
}

#[cfg(not(windows))]
pub fn reload_cache(_file_name: &'static str) {}

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
