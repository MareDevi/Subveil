//! Sample-clock energy segmentation. This is not a neural speech detector.

use anyhow::ensure;

/// Energy segmentation settings, measured in milliseconds.
#[derive(Clone, Debug)]
pub struct VadConfig {
    /// RMS above this threshold counts as speech.
    pub threshold: f32,
    /// Minimum voiced duration required to retain a segment.
    pub min_speech_ms: u32,
    /// Silence retained before speech onset.
    pub pre_roll_ms: u32,
    /// Silence required to finalize speech.
    pub trailing_silence_ms: u32,
    /// Hard segment duration limit.
    pub max_segment_ms: u32,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            threshold: 0.01,
            min_speech_ms: 150,
            pre_roll_ms: 200,
            trailing_silence_ms: 600,
            max_segment_ms: 15_000,
        }
    }
}

impl VadConfig {
    /// Reject unreasonable or contradictory settings before capture starts.
    pub fn validate(&self) -> anyhow::Result<()> {
        ensure!(
            self.threshold.is_finite() && self.threshold > 0. && self.threshold <= 1.,
            "VAD threshold must be in (0, 1]"
        );
        ensure!(
            self.min_speech_ms > 0 && self.min_speech_ms <= self.max_segment_ms,
            "invalid minimum speech duration"
        );
        ensure!(self.pre_roll_ms <= 2_000, "VAD pre-roll exceeds 2 seconds");
        ensure!(
            self.min_speech_ms.saturating_add(self.pre_roll_ms) <= self.max_segment_ms,
            "VAD minimum speech plus pre-roll exceeds maximum segment"
        );
        ensure!(
            self.trailing_silence_ms > 0 && self.trailing_silence_ms <= 5_000,
            "invalid trailing silence"
        );
        ensure!(
            (500..=30_000).contains(&self.max_segment_ms),
            "maximum segment must be 500..=30000 ms"
        );
        Ok(())
    }
}

#[derive(Debug, PartialEq)]
pub(crate) enum Boundary {
    Quiet,
    Speech { start: u64, end: u64, final_: bool },
    Rejected,
}

pub(crate) struct Vad {
    config: VadConfig,
    start: Option<u64>,
    voiced: u64,
    silence: u64,
    start_floor: u64,
}

impl Vad {
    pub fn new(config: VadConfig) -> Self {
        Self {
            config,
            start: None,
            voiced: 0,
            silence: 0,
            start_floor: 0,
        }
    }

    pub fn reset(&mut self) {
        self.start = None;
        self.voiced = 0;
        self.silence = 0;
        self.start_floor = 0;
    }

    // A segment buffer cannot supply pre-roll that was already committed or evicted.
    pub fn retain_from(&mut self, offset: u64) {
        self.start_floor = offset;
        if let Some(start) = &mut self.start {
            *start = (*start).max(offset);
        }
    }

    pub fn accept(&mut self, samples: &[f32], rate: u32, offset: u64) -> Boundary {
        let energy = samples.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>();
        let speech = !samples.is_empty()
            && (energy / samples.len() as f64).sqrt() >= f64::from(self.config.threshold);
        let end = offset + samples.len() as u64;
        let to_samples = |ms: u32| u64::from(ms) * u64::from(rate) / 1_000;
        if speech {
            self.start.get_or_insert(
                offset
                    .saturating_sub(to_samples(self.config.pre_roll_ms))
                    .max(self.start_floor),
            );
            self.voiced += samples.len() as u64;
            self.silence = 0;
        } else if self.start.is_some() {
            self.silence += samples.len() as u64;
        }
        let Some(start) = self.start else {
            return Boundary::Quiet;
        };
        let final_ = self.silence >= to_samples(self.config.trailing_silence_ms)
            || end.saturating_sub(start) >= to_samples(self.config.max_segment_ms);
        let accepted = self.voiced >= to_samples(self.config.min_speech_ms);
        if final_ {
            self.reset();
        }
        if accepted {
            Boundary::Speech { start, end, final_ }
        } else if final_ {
            Boundary::Rejected
        } else {
            Boundary::Quiet
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn speech_keeps_preroll_and_finishes_after_silence() {
        let mut vad = Vad::new(VadConfig::default());
        assert_eq!(vad.accept(&[0.; 100], 1_000, 0), Boundary::Quiet);
        assert_eq!(
            vad.accept(&[0.1; 200], 1_000, 100),
            Boundary::Speech {
                start: 0,
                end: 300,
                final_: false
            }
        );
        assert_eq!(
            vad.accept(&[0.; 600], 1_000, 300),
            Boundary::Speech {
                start: 0,
                end: 900,
                final_: true
            }
        );
        assert_eq!(vad.accept(&[0.; 600], 1_000, 900), Boundary::Quiet);
    }
    #[test]
    fn click_is_rejected_and_continuous_speech_is_bounded() {
        let mut vad = Vad::new(VadConfig {
            max_segment_ms: 500,
            ..Default::default()
        });
        vad.accept(&[0.1; 10], 1_000, 0);
        assert_eq!(vad.accept(&[0.; 600], 1_000, 10), Boundary::Rejected);
        assert_eq!(
            vad.accept(&[0.1; 500], 1_000, 610),
            Boundary::Speech {
                start: 410,
                end: 1110,
                final_: true
            }
        );
    }
    #[test]
    fn invalid_parameters_are_rejected() {
        assert!(
            VadConfig {
                threshold: f32::NAN,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            VadConfig {
                pre_roll_ms: 2_001,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
}
