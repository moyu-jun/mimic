//! Shared, dependency-free protocol between the normal application and elevated helper.

use std::ffi::OsString;
use std::fmt;

pub const VERSION: &str = "v1";
pub const HELPER_FILE_NAME: &str = "mimic-elevated-helper.exe";
pub const NONCE_BYTES: usize = 16;
pub const NONCE_LENGTH: usize = NONCE_BYTES * 2;
pub const MAX_REQUEST_BYTES: u64 = 256;

pub const EXIT_BAD_ARGUMENTS: i32 = 64;
pub const EXIT_INTEGRITY_FAILURE: i32 = 65;
pub const EXIT_OPERATION_FAILURE: i32 = 70;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtocolError;

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid elevation protocol value")
    }
}

impl std::error::Error for ProtocolError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Install,
    Uninstall,
    Reboot,
}

impl Action {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "install" => Some(Self::Install),
            "uninstall" => Some(Self::Uninstall),
            "reboot" => Some(Self::Reboot),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Uninstall => "uninstall",
            Self::Reboot => "reboot",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub action: Action,
    pub caller_pid: u32,
    pub nonce: String,
}

pub fn valid_nonce(value: &str) -> bool {
    value.len() == NONCE_LENGTH
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Helper argv schema: helper.exe v1 <action> <caller-pid> <nonce>.
pub fn parse_helper_args(arguments: &[OsString]) -> Result<Invocation, ProtocolError> {
    if arguments.len() != 5 || arguments.get(1).and_then(|value| value.to_str()) != Some(VERSION) {
        return Err(ProtocolError);
    }
    let action = arguments[2]
        .to_str()
        .and_then(Action::parse)
        .ok_or(ProtocolError)?;
    let caller_pid = arguments[3]
        .to_str()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value != 0)
        .ok_or(ProtocolError)?;
    let nonce = arguments[4]
        .to_str()
        .filter(|value| valid_nonce(value))
        .ok_or(ProtocolError)?;

    Ok(Invocation {
        action,
        caller_pid,
        nonce: nonce.to_string(),
    })
}

pub fn request_payload(invocation: &Invocation) -> String {
    format!(
        "{VERSION}|{}|{}|{}\n",
        invocation.action.as_str(),
        invocation.caller_pid,
        invocation.nonce
    )
}

pub fn pending_file_name(nonce: &str) -> Result<String, ProtocolError> {
    valid_nonce(nonce)
        .then(|| format!("elevation-{nonce}.request"))
        .ok_or(ProtocolError)
}

pub fn claimed_file_name(nonce: &str) -> Result<String, ProtocolError> {
    valid_nonce(nonce)
        .then(|| format!("elevation-{nonce}.claimed"))
        .ok_or(ProtocolError)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn accepts_only_versioned_whitelisted_actions() {
        let nonce = "0123456789abcdef0123456789abcdef";
        for (name, action) in [
            ("install", Action::Install),
            ("uninstall", Action::Uninstall),
            ("reboot", Action::Reboot),
        ] {
            assert_eq!(
                parse_helper_args(&args(&["helper.exe", VERSION, name, "42", nonce])),
                Ok(Invocation {
                    action,
                    caller_pid: 42,
                    nonce: nonce.to_string(),
                })
            );
        }
    }

    #[test]
    fn rejects_invalid_shape_pid_nonce_or_action() {
        let nonce = "0123456789abcdef0123456789abcdef";
        for values in [
            vec!["helper.exe", "v2", "install", "42", nonce],
            vec!["helper.exe", VERSION, "shell", "42", nonce],
            vec!["helper.exe", VERSION, "install", "0", nonce],
            vec!["helper.exe", VERSION, "install", "bad", nonce],
            vec!["helper.exe", VERSION, "install", "42", "short"],
            vec!["helper.exe", VERSION, "install", "42", nonce, "extra"],
        ] {
            assert!(parse_helper_args(&args(&values)).is_err());
        }
    }

    #[test]
    fn payload_and_file_names_bind_validated_fields() {
        let invocation = Invocation {
            action: Action::Uninstall,
            caller_pid: 77,
            nonce: "0123456789abcdef0123456789abcdef".to_string(),
        };
        assert_eq!(
            request_payload(&invocation),
            "v1|uninstall|77|0123456789abcdef0123456789abcdef\n"
        );
        assert_eq!(
            pending_file_name(&invocation.nonce).unwrap(),
            "elevation-0123456789abcdef0123456789abcdef.request"
        );
        assert!(claimed_file_name("..").is_err());
    }
}
