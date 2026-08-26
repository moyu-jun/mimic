//! Stable command-boundary errors and centralized recovery classification.

use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ErrorRecoveryPolicy {
    CriticalRuntime,
    LocalOperation,
    OptionalAudio,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    pub recovery: ErrorRecoveryPolicy,
}

pub type CommandResult<T> = Result<T, CommandError>;

impl CommandError {
    fn new(
        code: impl Into<String>,
        message: impl Into<String>,
        retryable: bool,
        recovery: ErrorRecoveryPolicy,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable,
            recovery,
        }
    }

    pub fn from_message(message: impl Into<String>) -> Self {
        let message = message.into();
        let lower = message.to_ascii_lowercase();

        let (code, retryable, recovery) = if lower.starts_with("busy") {
            ("busy", true, ErrorRecoveryPolicy::LocalOperation)
        } else if lower.contains("resource_integrity_failed") {
            (
                "resource_integrity_failed",
                false,
                ErrorRecoveryPolicy::LocalOperation,
            )
        } else if lower.contains("elevation_cancelled")
            || lower.contains("error 1223")
            || lower.contains("declined")
        {
            (
                "elevation_cancelled",
                true,
                ErrorRecoveryPolicy::LocalOperation,
            )
        } else if lower.contains("not_found") || lower.contains("not found") {
            ("not_found", true, ErrorRecoveryPolicy::LocalOperation)
        } else if lower.starts_with("optional_audio")
            || lower.starts_with("audio_warmup")
            || lower.starts_with("sound_playback")
        {
            (
                stable_prefix(&lower).unwrap_or("optional_audio_failed"),
                true,
                ErrorRecoveryPolicy::OptionalAudio,
            )
        } else if lower.contains("no_input_device")
            || lower.contains("audio")
            || lower.contains("sound")
            || lower.contains("wav")
            || lower.contains("recording")
            || lower.contains("input_format")
        {
            (
                stable_prefix(&lower).unwrap_or("audio_operation_failed"),
                true,
                ErrorRecoveryPolicy::LocalOperation,
            )
        } else if lower.contains("runtime")
            || lower.contains("listener")
            || lower.contains("release")
            || lower.contains("driver send")
        {
            (
                stable_prefix(&lower).unwrap_or("runtime_failure"),
                false,
                ErrorRecoveryPolicy::CriticalRuntime,
            )
        } else if lower.contains("lock state") || lower.contains("lock poisoned") {
            (
                "state_unavailable",
                true,
                ErrorRecoveryPolicy::CriticalRuntime,
            )
        } else if lower.starts_with("invalid")
            || lower.starts_with("unsupported")
            || lower.contains("conflict")
        {
            (
                stable_prefix(&lower).unwrap_or("invalid_request"),
                false,
                ErrorRecoveryPolicy::LocalOperation,
            )
        } else {
            (
                stable_prefix(&lower).unwrap_or("operation_failed"),
                true,
                ErrorRecoveryPolicy::LocalOperation,
            )
        };

        if matches!(
            code,
            "busy"
                | "elevation_cancelled"
                | "invalid_request"
                | "not_running"
                | "no_active_mouse_pick_session"
        ) {
            log::warn!("[command_error] code={code} recovery={recovery:?} detail={message}");
        } else {
            log::error!("[command_error] code={code} recovery={recovery:?} detail={message}");
        }
        Self::new(code, safe_message(code), retryable, recovery)
    }
}

fn safe_message(code: &str) -> &'static str {
    match code {
        "busy" => "Operation is currently busy.",
        "resource_integrity_failed" => "A required resource failed integrity verification.",
        "elevation_cancelled" => "The elevated operation was cancelled.",
        "not_found" => "A required resource was not found.",
        "no_input_device" => "No audio input device is available.",
        "state_unavailable" => "Application state is temporarily unavailable.",
        code if code.starts_with("invalid") || code.starts_with("unsupported") => {
            "The request is invalid."
        }
        _ => "The operation failed.",
    }
}
fn stable_prefix(message: &str) -> Option<&str> {
    let prefix = match message.split_once(':') {
        Some((prefix, _)) => prefix,
        None if !message.bytes().any(|byte| byte.is_ascii_whitespace()) => message,
        None => return None,
    };
    (!prefix.is_empty()
        && prefix.len() <= 64
        && prefix
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'))
    .then_some(prefix)
}

impl From<String> for CommandError {
    fn from(message: String) -> Self {
        Self::from_message(message)
    }
}

impl From<&str> for CommandError {
    fn from(message: &str) -> Self {
        Self::from_message(message)
    }
}

impl Display for CommandError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for CommandError {}

#[cfg(test)]
mod tests {
    use super::{CommandError, ErrorRecoveryPolicy};

    #[test]
    fn classifies_busy_as_retryable_local_error() {
        let error = CommandError::from("busy: Recording is active");
        assert_eq!(error.code, "busy");
        assert!(error.retryable);
        assert_eq!(error.recovery, ErrorRecoveryPolicy::LocalOperation);
    }

    #[test]
    fn classifies_recording_device_failure_as_local() {
        let error = CommandError::from("no_input_device");
        assert_eq!(error.code, "no_input_device");
        assert_eq!(error.recovery, ErrorRecoveryPolicy::LocalOperation);
    }

    #[test]
    fn classifies_warmup_as_optional_degradation() {
        let error = CommandError::from("audio_warmup_failed");
        assert_eq!(error.recovery, ErrorRecoveryPolicy::OptionalAudio);
    }

    #[test]
    fn classifies_runtime_release_failure_as_critical() {
        let error = CommandError::from("runtime release failed");
        assert_eq!(error.recovery, ErrorRecoveryPolicy::CriticalRuntime);
        assert!(!error.retryable);
    }

    #[test]
    fn does_not_promote_arbitrary_text_to_error_code() {
        let error = CommandError::from("Something went wrong");
        assert_eq!(error.code, "operation_failed");
    }

    #[test]
    fn command_dto_does_not_expose_internal_paths() {
        let error = CommandError::from(r#"installer_execute_failed:C:\Users\private\driver.exe"#);
        assert_eq!(error.code, "installer_execute_failed");
        assert!(!error.message.contains("private"));
        assert!(!error.message.contains("driver.exe"));
    }
}
