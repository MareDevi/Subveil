//! Bounded JSON-line protocol for the isolated native inference process.
use serde::{Deserialize, Serialize};

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
    /// Source language code.
    pub source: String,
    /// Target language code.
    pub target: String,
    /// Remaining monotonic budget at dispatch.
    pub timeout_ms: u64,
}
/// Helper result; stdout carries only these protocol messages.
#[derive(Deserialize, Serialize)]
pub enum Output {
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
    /// Startup or protocol failure.
    Error(String),
}
