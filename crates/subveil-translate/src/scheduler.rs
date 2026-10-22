//! Conservative, revisable stable-prefix scheduling over native ASR hypotheses.
use std::collections::VecDeque;
use subveil_core::subtitle::TranscriptSegment;

/// A bounded scheduler for one active source segment. Prefixes are never permanent truth.
#[derive(Default)]
pub struct StreamingTranslationScheduler {
    identity: Option<(u64, u64)>,
    hypotheses: VecDeque<String>,
    candidate: String,
    candidate_since: u64,
    submitted: String,
    last_submit: Option<u64>,
}
impl StreamingTranslationScheduler {
    /// Reset on session changes, stop, or mode changes.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    /// Record an accepted Partial. Stale events must be rejected by SubtitleState first.
    pub fn observe(&mut self, segment: &TranscriptSegment, now_ms: u64) {
        let identity = (segment.key.session_id, segment.key.segment_id);
        if self.identity != Some(identity) {
            self.reset();
            self.identity = Some(identity);
        }
        self.hypotheses.push_back(segment.text.clone());
        while self.hypotheses.len() > 3 {
            self.hypotheses.pop_front();
        }
        if self.hypotheses.len() < 3 {
            return;
        }
        let Some(first) = self.hypotheses.front() else {
            return;
        };
        let mut count = first.chars().count();
        for text in self.hypotheses.iter().skip(1) {
            count = count.min(
                first
                    .chars()
                    .zip(text.chars())
                    .take_while(|(a, b)| a == b)
                    .count(),
            );
        }
        let prefix: String = first.chars().take(count).collect();
        let candidate = boundary(&prefix, segment.language.as_deref() == Some("en"));
        if candidate != self.candidate {
            // Growth has already survived three hypotheses. Keep the initial stable
            // clause's wait; continuous speech must not restart the timer forever.
            if self.candidate.chars().count() < 12 || !candidate.starts_with(&self.candidate) {
                self.candidate_since = now_ms;
            }
            self.candidate = candidate;
        }
    }
    /// Poll stable time even when ASR has no new hypothesis. At most one request per 1500 ms.
    pub fn poll(&mut self, segment: &TranscriptSegment, now_ms: u64) -> Option<TranscriptSegment> {
        if self.identity != Some((segment.key.session_id, segment.key.segment_id))
            || self.hypotheses.len() < 3
            || (self.candidate.chars().count() < 12
                && !(self.candidate.chars().count() >= 4 && sentence_end(&self.candidate)))
            || !segment.text.starts_with(&self.candidate)
            || self.candidate == self.submitted
            || now_ms.saturating_sub(self.candidate_since) < 600
            || self
                .last_submit
                .is_some_and(|time| now_ms.saturating_sub(time) < 1500)
        {
            return None;
        }
        // A changed prefix is provisional even if it is shorter than the previous one.
        self.submitted.clone_from(&self.candidate);
        self.last_submit = Some(now_ms);
        let mut source = segment.clone();
        source.text.clone_from(&self.candidate);
        Some(source)
    }
}
fn sentence_end(text: &str) -> bool {
    text.ends_with(['。', '！', '？', '!', '?', '.'])
}
fn boundary(prefix: &str, english: bool) -> String {
    // Prefer a complete sentence over its unfinished tail. This is still a revisable
    // translation prefix; recognition owns Final identities and audio times.
    let sentence = prefix
        .char_indices()
        .filter_map(|(i, c)| {
            if !matches!(c, '。' | '！' | '？' | '!' | '?' | '.') {
                return None;
            }
            let end = i + c.len_utf8();
            if c.is_ascii()
                && prefix[end..]
                    .chars()
                    .next()
                    .is_some_and(|c| !c.is_whitespace())
            {
                return None;
            }
            if c == '.' {
                let word = prefix[..end]
                    .split_whitespace()
                    .next_back()
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if ["mr.", "mrs.", "ms.", "dr.", "prof.", "e.g.", "i.e."].contains(&word.as_str())
                    || (word.len() == 2 && word.as_bytes()[0].is_ascii_alphabetic())
                {
                    return None;
                }
            }
            Some(end)
        })
        .next_back();
    if let Some(end) = sentence
        && prefix[..end].trim().chars().count() >= 4
    {
        return prefix[..end].trim().into();
    }
    if english {
        // Drop the last token unless a whitespace or punctuation boundary was observed.
        let end = prefix
            .char_indices()
            .filter_map(|(i, c)| {
                if c.is_whitespace() {
                    Some(i)
                } else if matches!(c, '.' | ',' | '!' | '?' | ';' | ':') {
                    Some(i + c.len_utf8())
                } else {
                    None
                }
            })
            .next_back()
            .unwrap_or(0);
        prefix[..end].trim().into()
    } else {
        // Japanese/Chinese have no whitespace word boundary. Require a clause boundary.
        let end = prefix
            .char_indices()
            .filter_map(|(i, c)| {
                matches!(c, '、' | '。' | '，' | '！' | '？' | ',' | '.' | '!' | '?')
                    .then_some(i + c.len_utf8())
            })
            .next_back()
            .unwrap_or(0);
        prefix[..end].trim().into()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use subveil_core::subtitle::SegmentKey;
    fn source(revision: u32, text: &str) -> TranscriptSegment {
        TranscriptSegment {
            key: SegmentKey {
                session_id: 1,
                segment_id: 1,
                revision,
            },
            text: text.into(),
            language: Some("en".into()),
            start_ms: 0,
            end_ms: 2000,
        }
    }
    #[test]
    fn sentence_prefix_closes_before_unfinished_tail_without_inventing_final() -> anyhow::Result<()>
    {
        assert_eq!(boundary("Stop now! The driver is still", true), "Stop now!");
        assert_eq!(
            boundary("今日は雨です。明日は晴れそう", false),
            "今日は雨です。"
        );
        assert_eq!(
            boundary("Dr. Smith paid 3.14 dollars and", true),
            "Dr. Smith paid 3.14 dollars"
        );
        let mut scheduler = StreamingTranslationScheduler::default();
        for revision in 1..=3 {
            scheduler.observe(&source(revision, "Stop now! unfinished"), 0);
        }
        assert!(
            scheduler
                .poll(&source(3, "Stop now! unfinished"), 599)
                .is_none()
        );
        let accepted = scheduler
            .poll(&source(3, "Stop now! unfinished"), 600)
            .ok_or_else(|| anyhow::anyhow!("stable sentence absent"))?;
        assert_eq!(accepted.text, "Stop now!");
        assert_eq!(accepted.key, source(3, "").key);
        // A rewritten recognition hypothesis cannot reuse the preceding sentence.
        assert!(scheduler.poll(&source(4, "Keep going!"), 3000).is_none());
        Ok(())
    }
    #[test]
    fn growing_stable_prefix_does_not_starve_continuous_speech() {
        let mut scheduler = StreamingTranslationScheduler::default();
        for revision in 1..=9 {
            let text = format!(
                "the driver is closing the gap {} ",
                "again ".repeat(revision as usize)
            );
            let source = source(revision, &text);
            let now = u64::from(revision) * 200;
            scheduler.observe(&source, now);
            if revision == 6 {
                assert!(scheduler.poll(&source, now).is_some());
            }
            if revision == 9 {
                assert!(scheduler.poll(&source, now).is_none());
            }
        }
    }
    #[test]
    fn stable_prefix_waits_and_preserves_latest_revision() {
        let mut scheduler = StreamingTranslationScheduler::default();
        for (revision, text) in [
            (1, "Verstappen is closing the gap"),
            (2, "Verstappen is closing the gap to"),
            (3, "Verstappen is closing the gap to Leclerc"),
        ] {
            scheduler.observe(&source(revision, text), u64::from(revision) * 100);
        }
        let latest = source(3, "Verstappen is closing the gap to Leclerc");
        assert!(scheduler.poll(&latest, 899).is_none());
        let prefix = scheduler.poll(&latest, 900);
        assert_eq!(
            prefix.as_ref().map(|s| s.text.as_str()),
            Some("Verstappen is closing the")
        );
        assert_eq!(prefix.map(|s| s.key), Some(latest.key));
        assert!(scheduler.poll(&latest, 3000).is_none());
        assert!(
            scheduler
                .poll(&source(4, "Different sentence"), 4000)
                .is_none()
        );
    }
    #[test]
    fn unicode_boundary_revision_and_session_reset_are_safe() {
        assert_eq!(
            boundary("今日は良い天気ですね、次の", false),
            "今日は良い天気ですね、"
        );
        assert!(boundary("今日は良い天気ですね", false).is_empty());
        assert_eq!(boundary("the driver is clos", true), "the driver is");
        let mut scheduler = StreamingTranslationScheduler::default();
        for revision in 1..=3 {
            scheduler.observe(&source(revision, "the driver is closing "), 0);
        }
        assert!(
            scheduler
                .poll(&source(3, "the driver is closing "), 600)
                .is_some()
        );
        let mut other = source(4, "the driver is closing ");
        other.key.session_id = 2;
        scheduler.observe(&other, 1000);
        assert!(scheduler.poll(&other, 4000).is_none());
    }
}
