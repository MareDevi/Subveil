use subveil_core::subtitle::{SegmentKey, SubtitleState, TranscriptEvent, TranscriptSegment};

const SAMPLES: [(&str, &str, &str); 3] = [
    (
        "en",
        "The lights go out, and the race is underway!",
        "灯光熄灭，比赛正式开始！",
    ),
    (
        "ja",
        "音声をクラウドに送信せず、字幕ウィンドウの動作を確認しています。",
        "正在验证字幕窗口，当前没有向云端发送音频。",
    ),
    (
        "en",
        "This longer subtitle demonstrates how English source text and Chinese translations wrap across several lines while the transparent overlay lets you keep watching the video and using the controls underneath it.",
        "这段较长的模拟字幕用于验证英文原文和中文译文的多行换行效果。透明字幕浮窗不会挡住视频画面；开启鼠标穿透后，你仍然可以点击字幕下面的播放器控件，也可以关闭主控制窗口后继续查看字幕。",
    ),
];

/// One bounded update per timer tick; never replay missed events after a stall.
pub fn advance(state: &mut SubtitleState, tick: u64, now_ms: u64) -> bool {
    let phase = tick % 56;
    let sequence = tick / 56;
    let (language, source, translation) = SAMPLES[(sequence % SAMPLES.len() as u64) as usize];
    let key = SegmentKey {
        session_id: state.session_id(),
        segment_id: sequence + 1,
        revision: 3,
    };
    match phase {
        0 | 4 | 8 => {
            let revision = match phase {
                0 => 1,
                4 => 2,
                _ => 3,
            };
            let character_count = source.chars().count();
            let text = source
                .chars()
                .take(match phase {
                    0 => character_count / 3,
                    4 => character_count * 2 / 3,
                    _ => character_count,
                })
                .collect();
            let transcript = TranscriptSegment {
                key: SegmentKey { revision, ..key },
                text,
                language: Some(language.into()),
                start_ms: sequence * 14_000,
                end_ms: sequence * 14_000 + phase * 250,
            };
            if phase == 8 {
                let changed = state.apply(TranscriptEvent::Final(transcript), now_ms);
                state.request_translation(key);
                changed
            } else {
                state.apply(TranscriptEvent::Partial(transcript), now_ms)
            }
        }
        12 => state.finish_translation(key, Ok(translation.into()), now_ms),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use subveil_core::subtitle::TranslationState;

    #[test]
    fn mock_revises_then_translates_and_expires_without_external_services() {
        let mut state = SubtitleState::new(1, 2, 8_000);
        for tick in 0..=44 {
            advance(&mut state, tick, tick * 250);
            state.advance_presentation(tick * 250);
            state.expire(tick * 250);
            if tick == 12 {
                assert_eq!(state.history().len(), 1);
                assert!(matches!(
                    state.current().map(|row| &row.translation),
                    Some(TranslationState::Complete(_))
                ));
            }
        }
        assert!(state.current().is_none());
    }
}
