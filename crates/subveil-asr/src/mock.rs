use crate::{
    Config, Continuity, Shared, Transcript,
    vad::{Boundary, Vad},
};
use std::{
    thread,
    time::{Duration, Instant},
};
use subveil_audio::{AudioFrame, FrameReceiver};

trait Recognizer {
    fn accept(
        &mut self,
        frame: &AudioFrame,
        transcript: &mut Transcript,
        shared: &Shared,
    ) -> anyhow::Result<()>;
    fn reset(&mut self);
}

struct Mock {
    vad: Vad,
    language: String,
}
impl Recognizer for Mock {
    fn accept(
        &mut self,
        frame: &AudioFrame,
        transcript: &mut Transcript,
        shared: &Shared,
    ) -> anyhow::Result<()> {
        if let Boundary::Speech { start, end, final_ } =
            self.vad
                .accept(&frame.samples[..frame.len], frame.rate, frame.sample_offset)
        {
            let elapsed = (end - start) * 1_000 / u64::from(frame.rate);
            let text = match (self.language.as_str(), elapsed >= 800) {
                ("ja", false) => "[Mock] これは",
                ("ja", true) => "[Mock] これは音声認識のテストです。",
                (_, false) => "[Mock] This is",
                (_, true) => "[Mock] This is an audio-driven test.",
            };
            if let Some(event) = transcript.update(
                text.into(),
                start * 1_000 / u64::from(frame.rate),
                end * 1_000 / u64::from(frame.rate),
                final_,
            ) {
                shared.emit(event);
            }
        }
        Ok(())
    }
    fn reset(&mut self) {
        self.vad.reset();
    }
}

pub(crate) fn run(
    config: &Config,
    session: u64,
    frames: &FrameReceiver,
    shared: &Shared,
) -> anyhow::Result<()> {
    let mut recognizer = Mock {
        vad: Vad::new(config.vad.clone()),
        language: config.language.clone(),
    };
    let mut transcript = Transcript::new(session, &config.language);
    let mut continuity = Continuity::default();
    let mut last = Instant::now();
    let mut timed_out = false;
    while !shared.stopped() {
        if let Some(frame) = frames.pop() {
            if !continuity.accept(&frame, config.sample_rate) {
                recognizer.reset();
                transcript.reset();
                shared.status("音频不连续；已重置识别", frames, true);
                continue;
            }
            last = Instant::now();
            timed_out = false;
            recognizer.accept(&frame, &mut transcript, shared)?;
            shared.status("Mock ASR · 仅音频驱动样本文字", frames, false);
        } else {
            if !timed_out && last.elapsed() >= Duration::from_secs(2) {
                recognizer.reset();
                transcript.reset();
                timed_out = true;
                shared.status("等待音频；识别上下文已重置", frames, true);
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
    Ok(())
}
