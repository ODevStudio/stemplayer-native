#![deny(unsafe_code)]

use serde_json::json;
use std::env;
use std::process::ExitCode;
use stemplayer_domain::{ErrorCode, PlayerError};

const EXIT_USAGE: u8 = 2;
const EXIT_VALIDATION: u8 = 3;
const EXIT_UNSUPPORTED: u8 = 4;
const EXIT_INTERNAL: u8 = 5;

enum Failure {
    Usage(PlayerError),
    Validation(PlayerError),
    Unsupported(PlayerError),
    Internal(PlayerError),
}

impl Failure {
    fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(_) => EXIT_USAGE,
            Self::Validation(_) => EXIT_VALIDATION,
            Self::Unsupported(_) => EXIT_UNSUPPORTED,
            Self::Internal(_) => EXIT_INTERNAL,
        }
    }

    fn error(&self) -> &PlayerError {
        match self {
            Self::Usage(value)
            | Self::Validation(value)
            | Self::Unsupported(value)
            | Self::Internal(value) => value,
        }
    }
}

fn main() -> ExitCode {
    match run(env::args().skip(1)) {
        Ok(value) => match serde_json::to_string(&value) {
            Ok(line) => {
                println!("{line}");
                ExitCode::SUCCESS
            }
            Err(_) => ExitCode::from(
                Failure::Internal(PlayerError::new(
                    ErrorCode::Internal,
                    "JSON serialization failed",
                ))
                .exit_code(),
            ),
        },
        Err(failure) => {
            if let Ok(line) = serde_json::to_string(failure.error()) {
                eprintln!("{line}");
            }
            ExitCode::from(failure.exit_code())
        }
    }
}

fn run(mut args: impl Iterator<Item = String>) -> Result<serde_json::Value, Failure> {
    let command = args.next().ok_or_else(usage_error)?;
    match command.as_str() {
        "version" if args.next().is_none() => Ok(json!({
            "schema": "stemplayer-probe-v1",
            "version": env!("CARGO_PKG_VERSION")
        })),
        "validate-config" if args.next().is_none() => Ok(json!({
            "schema": "stemplayer-probe-config-v1",
            "status": "valid",
            "features": enabled_features()
        })),
        "inspect" | "validate" | "playback-sim" | "benchmark" | "cache-inspect"
        | "hls-validate" => {
            require_arguments(&mut args, 1, command.as_str())?;
            Err(feature_error(command.as_str()))
        }
        "render" => {
            require_arguments(&mut args, 2, command.as_str())?;
            Err(feature_error(command.as_str()))
        }
        _ => Err(usage_error()),
    }
}

fn require_arguments(
    args: &mut impl Iterator<Item = String>,
    count: usize,
    command: &str,
) -> Result<(), Failure> {
    let values = args.take(count + 1).collect::<Vec<_>>();
    if values.len() != count {
        return Err(Failure::Usage(PlayerError::new(
            ErrorCode::ValueOutOfRange,
            format!("{command} expects {count} explicit argument(s)"),
        )));
    }
    if values.iter().any(String::is_empty) {
        return Err(Failure::Validation(PlayerError::new(
            ErrorCode::ValueOutOfRange,
            "Arguments must not be empty",
        )));
    }
    Ok(())
}

fn usage_error() -> Failure {
    Failure::Usage(PlayerError::new(
        ErrorCode::ValueOutOfRange,
        "Expected version, validate-config, inspect, validate, render, playback-sim, benchmark, cache-inspect, or hls-validate",
    ))
}

fn feature_error(feature: &str) -> Failure {
    Failure::Unsupported(PlayerError::new(
        ErrorCode::FeatureUnsupported,
        format!("{feature} is not implemented in Phase 2"),
    ))
}

fn enabled_features() -> Vec<&'static str> {
    [
        (cfg!(feature = "local-wav"), "local-wav"),
        (cfg!(feature = "local-mp4"), "local-mp4"),
        (cfg!(feature = "aac"), "aac"),
        (cfg!(feature = "alac"), "alac"),
        (cfg!(feature = "hls"), "hls"),
    ]
    .into_iter()
    .filter_map(|(enabled, name)| enabled.then_some(name))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_exit_class_is_stable() {
        assert_eq!(
            Failure::Internal(PlayerError::new(ErrorCode::Internal, "test")).exit_code(),
            5
        );
    }
}
