//! Fixed-size PCM handoff. Process callbacks never allocate or wait for UI.
use crate::Snapshot;
use crossbeam_queue::ArrayQueue;
use std::time::Instant;

const SAMPLES: usize = 2048;
const CAPACITY: usize = 16;

pub(crate) struct Block {
    pub samples: [f32; SAMPLES],
    pub len: usize,
    pub generation: u64,
    pub sequence: u64,
    pub sample_offset: u64,
    pub captured: Instant,
    pub rate: u32,
}

pub(crate) struct PcmQueue {
    pub queue: ArrayQueue<Block>,
    pub blocks: u64,
    pub samples: u64,
    pub dropped_blocks: u64,
    pub dropped_samples: u64,
    pub invalid: u64,
}

impl PcmQueue {
    pub fn new() -> Self {
        Self {
            queue: ArrayQueue::new(CAPACITY),
            blocks: 0,
            samples: 0,
            dropped_blocks: 0,
            dropped_samples: 0,
            invalid: 0,
        }
    }

    pub fn push(&mut self, bytes: &[u8], generation: u64, rate: u32) {
        if bytes.is_empty() || !bytes.len().is_multiple_of(4) {
            self.invalid += 1;
            return;
        }
        let now = Instant::now();
        for part in bytes.chunks(SAMPLES * 4) {
            let mut block = Block {
                samples: [0.; SAMPLES],
                len: part.len() / 4,
                generation,
                sequence: self.blocks,
                sample_offset: self.samples,
                captured: now,
                rate,
            };
            for (sample, raw) in block.samples.iter_mut().zip(part.as_chunks::<4>().0) {
                let value = f32::from_le_bytes(*raw);
                *sample = if value.is_finite() {
                    value.clamp(-1., 1.)
                } else {
                    0.
                };
            }
            self.blocks += 1;
            self.samples += block.len as u64;
            if let Some(old) = self.queue.force_push(block) {
                self.dropped_blocks += 1;
                self.dropped_samples += old.len as u64;
            }
        }
    }

    pub fn push_silence(&mut self, bytes: usize, generation: u64, rate: u32) {
        if !bytes.is_multiple_of(4) {
            self.invalid += 1;
            return;
        }
        let zeros = [0_u8; SAMPLES * 4];
        let mut left = bytes;
        while left > 0 {
            let size = left.min(zeros.len());
            self.push(&zeros[..size], generation, rate);
            left -= size;
        }
    }

    pub fn discard(&mut self) {
        while let Some(block) = self.queue.pop() {
            self.dropped_blocks += 1;
            self.dropped_samples += block.len as u64;
        }
    }

    pub fn drain(&mut self, stats: &mut Snapshot, generation: u64, last: &mut Instant) {
        let mut energy = 0_f64;
        let mut count = 0;
        let mut peak = 0_f32;
        while let Some(block) = self.queue.pop() {
            if block.generation != generation || block.rate != stats.sample_rate {
                self.dropped_blocks += 1;
                self.dropped_samples += block.len as u64;
                continue;
            }
            // Offset and sequence stay attached for a future ASR consumer to detect gaps.
            let _position = (block.sequence, block.sample_offset);
            if block.captured.elapsed().as_millis() > 250 {
                self.dropped_blocks += 1;
                self.dropped_samples += block.len as u64;
                continue;
            }
            *last = block.captured;
            for &sample in &block.samples[..block.len] {
                energy += f64::from(sample).powi(2);
                peak = peak.max(sample.abs());
            }
            count += block.len;
        }
        if count > 0 {
            stats.rms = (energy / count as f64).sqrt() as f32;
            stats.peak = peak;
            stats.silent = stats.rms < 0.001;
        }
        stats.blocks = self.blocks;
        stats.samples = self.samples;
        stats.dropped_blocks = self.dropped_blocks;
        stats.dropped_samples = self.dropped_samples;
        stats.invalid_chunks = self.invalid;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overflow_keeps_latest_and_counts_exact_samples() {
        let mut q = PcmQueue::new();
        for n in 0..20 {
            q.push(&(n as f32 / 20.).to_le_bytes(), 1, 48000);
        }
        assert_eq!(
            (q.queue.len(), q.dropped_blocks, q.dropped_samples),
            (16, 4, 4)
        );
        assert_eq!(
            q.queue.pop().map(|b| (b.sequence, b.sample_offset)),
            Some((4, 4))
        );
    }
    #[test]
    fn splits_large_chunks_and_rejects_misalignment() {
        let mut q = PcmQueue::new();
        q.push(&vec![0; (SAMPLES + 1) * 4], 1, 48000);
        q.push(&[1, 2, 3], 1, 48000);
        assert_eq!((q.blocks, q.samples, q.invalid), (2, 2049, 1));
    }
    #[test]
    fn levels_sanitize_nonfinite_and_reject_old_generation() {
        let mut q = PcmQueue::new();
        q.push(&1_f32.to_le_bytes(), 1, 48000);
        let bytes: Vec<_> = [f32::NAN, f32::INFINITY, -2., 0.]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        q.push(&bytes, 2, 48000);
        let mut s = Snapshot::default();
        let mut last = Instant::now();
        q.drain(&mut s, 2, &mut last);
        assert_eq!((s.rms, s.peak, s.silent), (0.5, 1., false));
        crate::expire_level(&mut s, Instant::now() - std::time::Duration::from_secs(1));
        assert_eq!((s.rms, s.silent), (0., true));
    }
    #[test]
    fn stale_pcm_is_discarded_and_counted() {
        let mut q = PcmQueue::new();
        q.push(&0.5_f32.to_le_bytes(), 1, 48000);
        let mut block = q
            .queue
            .pop()
            .unwrap_or_else(|| unreachable!("test inserted one block"));
        block.captured = Instant::now() - std::time::Duration::from_secs(1);
        assert!(q.queue.push(block).is_ok());
        let mut s = Snapshot::default();
        let mut last = Instant::now();
        q.drain(&mut s, 1, &mut last);
        assert_eq!((s.rms, s.dropped_blocks, s.dropped_samples), (0., 1, 1));
    }

    #[test]
    fn target_change_flushes_pending_pcm_without_resetting_counters() {
        let mut q = PcmQueue::new();
        q.push(&[0; 16], 1, 48000);
        q.discard();
        assert_eq!(
            (
                q.queue.len(),
                q.blocks,
                q.samples,
                q.dropped_blocks,
                q.dropped_samples
            ),
            (0, 1, 4, 1, 4)
        );
        q.push(&[0; 4], 2, 48000);
        assert_eq!(
            q.queue
                .pop()
                .map(|b| (b.generation, b.sequence, b.sample_offset)),
            Some((2, 1, 4))
        );
    }
    #[test]
    fn spa_empty_chunks_produce_silence_with_correct_duration() {
        let mut q = PcmQueue::new();
        q.push_silence((SAMPLES + 1) * 4, 1, 48000);
        let mut s = Snapshot::default();
        let mut last = Instant::now();
        q.drain(&mut s, 1, &mut last);
        assert_eq!((s.samples, s.blocks, s.rms, s.silent), (2049, 2, 0., true));
    }
}
