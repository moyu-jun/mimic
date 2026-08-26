//! Portable 目录解析。
//!
//! 所有路径从可执行文件目录派生，不依赖当前工作目录，也不写入 AppData。

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct PortablePaths {
    root: PathBuf,
}

impl PortablePaths {
    pub fn current() -> Result<Self, String> {
        let executable = std::env::current_exe()
            .map_err(|error| format!("failed to resolve executable path: {error}"))?
            .canonicalize()
            .map_err(|error| format!("failed to canonicalize executable path: {error}"))?;
        Self::from_executable(&executable)
    }

    pub fn from_executable(executable: &Path) -> Result<Self, String> {
        let root = executable
            .parent()
            .ok_or_else(|| "executable has no parent directory".to_string())?;
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    pub fn data_dir(&self) -> PathBuf {
        self.root.join("data")
    }

    pub fn config_file(&self) -> PathBuf {
        self.data_dir().join("mimic.ini")
    }

    pub fn audio_dir(&self) -> PathBuf {
        self.data_dir().join("audio")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.data_dir().join("logs")
    }

    pub fn temp_dir(&self) -> PathBuf {
        self.data_dir().join("temp")
    }

    pub fn driver_dir(&self) -> PathBuf {
        self.root.join("driver")
    }

    pub fn ensure_data_dirs(&self) -> Result<(), String> {
        ensure_real_directory(&self.root)?;
        for directory in [
            self.data_dir(),
            self.audio_dir(),
            self.logs_dir(),
            self.temp_dir(),
        ] {
            match std::fs::create_dir(&directory) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(format!("failed to create portable data directory: {error}"))
                }
            }
            ensure_real_directory(&directory)?;
        }
        Ok(())
    }

    /// 首次启动时把随程序分发的默认 WAV 复制到 data/audio。
    /// 已存在的用户录音永不覆盖。
    pub fn seed_default_audio(&self, file_names: &[&str]) -> Result<(), String> {
        let packaged_audio = self.root.join("audio");
        let user_audio = self.audio_dir();
        std::fs::create_dir_all(&user_audio)
            .map_err(|error| format!("failed to create {}: {error}", user_audio.display()))?;

        for file_name in file_names {
            let source = packaged_audio.join(file_name);
            let target = user_audio.join(file_name);
            ensure_regular_file_or_missing(&target)?;
            if target.exists() || !source.is_file() {
                continue;
            }
            ensure_regular_file_or_missing(&source)?;
            std::fs::copy(&source, &target).map_err(|error| {
                format!(
                    "failed to seed audio {} -> {}: {error}",
                    source.display(),
                    target.display()
                )
            })?;
        }
        Ok(())
    }
}

fn path_is_link_like(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn ensure_real_directory(path: &Path) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("portable directory unavailable: {error}"))?;
    if !metadata.is_dir() || path_is_link_like(&metadata) {
        return Err("portable directory must not be a link or reparse point".to_string());
    }
    Ok(())
}

/// Reject a user-controlled link/reparse point before opening or replacing a fixed file target.
pub fn ensure_regular_file_or_missing(path: &Path) -> Result<(), String> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("portable file metadata unavailable: {error}")),
    };
    if !metadata.is_file() || path_is_link_like(&metadata) {
        return Err("portable file must be a regular non-link file".to_string());
    }
    Ok(())
}

#[cfg(windows)]
pub fn atomic_replace(source: &Path, destination: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    if unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(format!(
            "atomic replace failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn atomic_replace(source: &Path, destination: &Path) -> Result<(), String> {
    std::fs::rename(source, destination).map_err(|error| format!("atomic replace failed: {error}"))
}
#[cfg(test)]
mod tests {
    use super::PortablePaths;
    use std::path::Path;

    #[test]
    fn derives_all_mutable_paths_under_data() {
        let paths = PortablePaths::from_executable(Path::new("C:/Mimic/Mimic.exe")).unwrap();
        assert_eq!(paths.config_file(), Path::new("C:/Mimic/data/mimic.ini"));
        assert_eq!(paths.audio_dir(), Path::new("C:/Mimic/data/audio"));
        assert_eq!(paths.logs_dir(), Path::new("C:/Mimic/data/logs"));
        assert_eq!(paths.temp_dir(), Path::new("C:/Mimic/data/temp"));
        assert_eq!(paths.driver_dir(), Path::new("C:/Mimic/driver"));
    }
}
