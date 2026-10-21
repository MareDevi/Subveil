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
        /// Native decoding time, excluding loading and IPC.
        inference_ms: u64,
    },
    /// Startup or protocol failure.
    Error(String),
}
