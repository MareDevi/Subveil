//! Bounded JSON-line protocol for the isolated native inference process.
use serde::{Deserialize, Serialize};

/// Wire compatibility changes independently of the application's display version.
pub const PROTOCOL_VERSION: u32 = 3;
/// Compiled helper identity, queried before weights or native devices are opened.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BuildInfo {
    /// Exact IPC protocol revision.
    pub protocol: u32,
    /// Workspace release version.
    pub version: String,
    /// Package build identifier shared by all bundled binaries.
    pub build_id: String,
    /// `translation` or `whisper`.
    pub helper: String,
    /// Compiled backends; not proof that a device can run inference.
    pub backends: Vec<String>,
}
impl BuildInfo {
    /// Identity for the current compile; packaging supplies one common build ID.
    pub fn current(helper: &str, backends: &[&str]) -> Self {
        Self {
            protocol: PROTOCOL_VERSION,
            version: env!("CARGO_PKG_VERSION").into(),
            build_id: option_env!("SUBVEIL_BUILD_ID")
                .unwrap_or("development")
                .into(),
            helper: helper.into(),
            backends: backends.iter().map(|s| (*s).into()).collect(),
        }
    }
    /// Reject mismatched packages and unsupported requested backends before loading.
    pub fn verify(&self, helper: &str, backend: &str) -> Result<(), &'static str> {
        let expected = Self::current(helper, &[]);
        if self.protocol != expected.protocol
            || self.version != expected.version
            || self.build_id != expected.build_id
            || self.helper != helper
        {
            return Err("推理程序版本不匹配；请重新安装同一套应用和 helper");
        }
        if !self.backends.iter().any(|s| s == backend) {
            return Err("推理程序未编译所选后端；请安装对应构建，或显式选择 CPU");
        }
        Ok(())
    }
}

/// Explicit device choice; errors never cause an automatic CPU retry.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub enum Compute {
    /// CPU inference.
    #[default]
    Cpu,
    /// Runtime device index from the current helper probe.
    Gpu(usize),
}
/// GPU reported by the inference runtime itself.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Device {
    /// Current process device index.
    pub index: usize,
    /// Backend and physical device description.
    pub name: String,
    /// Free device memory in bytes.
    pub free_bytes: usize,
}
/// One text request; never written to logs or disk.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    /// Correlation within one immutable subtitle session.
    pub id: u64,
    /// Source text.
    pub text: String,
    /// Previous authoritative source only; at most three 1024-character rows.
    #[serde(default)]
    pub context: String,
    /// User terminology. Treated as data, never as system instructions.
    #[serde(default)]
    pub glossary: String,
    /// Source language code.
    pub source: String,
    /// Target language code.
    pub target: String,
    /// Remaining monotonic budget at dispatch.
    pub timeout_ms: u64,
}
/// Cancellation is read concurrently with generation; request IDs are monotonic.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CancelInput {
    /// Cancel the current request with this identifier.
    pub cancel_id: u64,
}
/// Audio request header, followed by exactly `samples * 4` little-endian PCM bytes.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SpeechInput {
    /// Correlation within one worker lifetime.
    pub id: u64,
    /// Explicit source language; no translation task is used.
    pub language: String,
    /// Mono 16 kHz float sample count, at most 30 seconds.
    pub samples: usize,
    /// Decode budget; parent also enforces a hard process deadline.
    pub timeout_ms: u64,
}
/// Helper result; stdout carries only these protocol messages.
#[derive(Deserialize, Serialize)]
pub enum Output {
    /// First message of every invocation, before native initialization or model I/O.
    Capabilities(BuildInfo),
    /// Native devices, without loading a model.
    Devices(Vec<Device>),
    /// Successfully loaded model and selected device.
    Ready(String),
    /// Complete translation or sanitized error.
    Translation {
        /// Request correlation identifier.
        id: u64,
        /// Complete text or sanitized error.
        result: Result<String, String>,
    },
    /// Complete source transcription from one bounded audio request.
    Recognition {
        /// Request correlation identifier.
        id: u64,
        /// Source text or sanitized error.
        result: Result<String, String>,
        /// UTF-8 prefix whose token timestamps end within the 400 ms context window.
        /// None when timestamps are missing, invalid or cut through a UTF-8 sequence.
        #[serde(default)]
        overlap_prefix: Option<String>,
        /// Native decoding time, excluding loading and IPC.
        inference_ms: u64,
    },
    /// Startup or protocol failure.
    Error(String),
}

#[cfg(test)]
mod build_tests {
    use super::*;
    #[test]
    fn incompatible_helpers_and_uncompiled_backends_fail_closed() {
        let mut build = BuildInfo::current("whisper", &["cpu"]);
        assert!(build.verify("whisper", "cpu").is_ok());
        assert!(build.verify("whisper", "cuda").is_err());
        assert!(build.verify("translation", "cpu").is_err());
        build.protocol += 1;
        assert!(build.verify("whisper", "cpu").is_err());
        build = BuildInfo::current("whisper", &["cpu"]);
        build.build_id.push_str("-other");
        assert!(build.verify("whisper", "cpu").is_err());
    }
}
