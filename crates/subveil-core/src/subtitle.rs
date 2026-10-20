//! Bounded, monotonic-time subtitle state with revision and session guards.

use std::collections::VecDeque;

/// Identity carried by transcripts and every translation response.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SegmentKey {
    /// Recognition session identifier.
    pub session_id: u64,
    /// Segment identifier, unique within a session.
    pub segment_id: u64,
    /// Monotonic revision within a segment.
    pub revision: u32,
}

/// A revision of recognized speech, timed relative to session start.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TranscriptSegment {
    /// Identity and revision.
    pub key: SegmentKey,
    /// Recognized text.
    pub text: String,
    /// BCP-47 language tag when known.
    pub language: Option<String>,
    /// Audio start offset in milliseconds.
    pub start_ms: u64,
    /// Audio end offset in milliseconds.
    pub end_ms: u64,
}

/// Partial revisions replace the same segment; final revisions stabilize it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TranscriptEvent {
    /// Recognition is still in progress.
    Partial(TranscriptSegment),
    /// The recognizer has finalized this segment.
    Final(TranscriptSegment),
}

/// Translation lifecycle, independent of recognition progress.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TranslationState {
    /// No translation was requested for this revision.
    NotRequested,
    /// A translation is in flight.
    Pending,
    /// A matching translation completed.
    Complete(String),
    /// A matching translation failed.
    Failed(String),
}

/// A retained subtitle row; expired rows remain in bounded history.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Subtitle {
    /// Current transcript revision.
    pub transcript: TranscriptSegment,
    /// Whether recognition has finalized this row.
    pub is_final: bool,
    /// Translation status for the current revision.
    pub translation: TranslationState,
    /// Whether the display lifetime has ended.
    pub expired: bool,
    expires_at_ms: u64,
}

/// Subtitle state owned by a session, never by a window.
pub struct SubtitleState {
    session_id: u64,
    entries: VecDeque<Subtitle>,
    history_limit: usize,
    ttl_ms: u64,
    // Remember evicted identities without retaining unbounded tombstones.
    highest_segment_id: Option<u64>,
}

impl SubtitleState {
    /// Start an empty session. Segment IDs must increase for newly seen segments.
    /// History is bounded to 1..256 rows and lifetimes to 1..60 seconds.
    pub fn new(session_id: u64, history_limit: usize, ttl_ms: u64) -> Self {
        Self {
            session_id,
            entries: VecDeque::new(),
            history_limit: history_limit.clamp(1, 256),
            ttl_ms: ttl_ms.clamp(1_000, 60_000),
            highest_segment_id: None,
        }
    }

    /// Return the active session ID.
    pub fn session_id(&self) -> u64 {
        self.session_id
    }

    /// Discard old session state, so all late responses are rejected.
    pub fn reset(&mut self, session_id: u64) {
        self.session_id = session_id;
        self.entries.clear();
        self.highest_segment_id = None;
    }

    /// Access finite history from oldest to newest.
    pub fn history(&self) -> &VecDeque<Subtitle> {
        &self.entries
    }

    /// Return the newest unexpired subtitle, without reviving older rows.
    pub fn current(&self) -> Option<&Subtitle> {
        self.entries.back().filter(|entry| !entry.expired)
    }

    /// Apply a valid newer transcript or a same-revision partial-to-final promotion.
    /// Returns false for stale sessions, regressions, evicted IDs or invalid text/times.
    pub fn apply(&mut self, event: TranscriptEvent, now_ms: u64) -> bool {
        let (transcript, is_final) = match event {
            TranscriptEvent::Partial(segment) => (segment, false),
            TranscriptEvent::Final(segment) => (segment, true),
        };
        if transcript.key.session_id != self.session_id
            || transcript.text.trim().is_empty()
            || transcript.text.len() > 16_384
            || transcript.start_ms > transcript.end_ms
        {
            return false;
        }
        if let Some(existing) = self
            .entries
            .iter_mut()
            .find(|entry| entry.transcript.key.segment_id == transcript.key.segment_id)
        {
            let old = &existing.transcript;
            if existing.expired
                || (existing.is_final && !is_final)
                || transcript.key.revision < old.key.revision
                || (transcript.key.revision == old.key.revision
                    && (existing.is_final || !is_final || *old != transcript))
            {
                return false;
            }
            *existing = Subtitle {
                transcript,
                is_final,
                translation: TranslationState::NotRequested,
                expired: false,
                expires_at_ms: now_ms.saturating_add(self.ttl_ms),
            };
            return true;
        }
        if self
            .highest_segment_id
            .is_some_and(|id| transcript.key.segment_id <= id)
        {
            return false;
        }
        self.highest_segment_id = Some(transcript.key.segment_id);
        self.entries.push_back(Subtitle {
            transcript,
            is_final,
            translation: TranslationState::NotRequested,
            expired: false,
            expires_at_ms: now_ms.saturating_add(self.ttl_ms),
        });
        while self.entries.len() > self.history_limit {
            self.entries.pop_front();
        }
        true
    }

    /// Mark a final revision pending, once. Partial events do not trigger translation.
    pub fn request_translation(&mut self, key: SegmentKey) -> bool {
        let Some(entry) = self.matching(key) else {
            return false;
        };
        if !entry.is_final || entry.translation != TranslationState::NotRequested {
            return false;
        }
        entry.translation = TranslationState::Pending;
        true
    }

    /// Accept only a pending response for the exact current revision and session.
    pub fn finish_translation(
        &mut self,
        key: SegmentKey,
        result: Result<String, String>,
        now_ms: u64,
    ) -> bool {
        let ttl_ms = self.ttl_ms;
        let Some(entry) = self.matching(key) else {
            return false;
        };
        if entry.translation != TranslationState::Pending || now_ms >= entry.expires_at_ms {
            return false;
        }
        entry.translation = match result {
            Ok(text) => TranslationState::Complete(text),
            Err(error) => TranslationState::Failed(error),
        };
        entry.expires_at_ms = now_ms.saturating_add(ttl_ms);
        true
    }

    /// Expire visible rows using monotonic session time. Return whether anything changed.
    pub fn expire(&mut self, now_ms: u64) -> bool {
        let mut changed = false;
        for entry in &mut self.entries {
            if !entry.expired && now_ms >= entry.expires_at_ms {
                entry.expired = true;
                changed = true;
            }
        }
        changed
    }

    fn matching(&mut self, key: SegmentKey) -> Option<&mut Subtitle> {
        if key.session_id != self.session_id {
            return None;
        }
        self.entries
            .iter_mut()
            .find(|entry| entry.transcript.key == key && !entry.expired)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(session: u64, id: u64, revision: u32, text: &str) -> TranscriptSegment {
        TranscriptSegment {
            key: SegmentKey {
                session_id: session,
                segment_id: id,
                revision,
            },
            text: text.into(),
            language: Some("en".into()),
            start_ms: 0,
            end_ms: 100,
        }
    }

    #[test]
    fn partial_revisions_replace_one_row_and_final_cannot_regress() {
        let mut state = SubtitleState::new(1, 8, 1_000);
        state.apply(TranscriptEvent::Partial(segment(1, 1, 1, "Hello")), 0);
        state.apply(
            TranscriptEvent::Partial(segment(1, 1, 2, "Hello world")),
            10,
        );
        assert_eq!(state.history().len(), 1);
        assert!(!state.apply(TranscriptEvent::Partial(segment(1, 1, 1, "old")), 20));
        assert!(state.apply(TranscriptEvent::Final(segment(1, 1, 2, "Hello world")), 20));
        assert!(!state.apply(TranscriptEvent::Partial(segment(1, 1, 3, "regression")), 30));
        assert_eq!(
            state.current().map(|row| row.transcript.text.as_str()),
            Some("Hello world")
        );
    }

    #[test]
    fn late_translation_cannot_overwrite_newer_revision() {
        let mut state = SubtitleState::new(1, 8, 1_000);
        let old = segment(1, 1, 1, "first");
        let new = segment(1, 1, 2, "revised");
        state.apply(TranscriptEvent::Final(old.clone()), 0);
        assert!(state.request_translation(old.key));
        state.apply(TranscriptEvent::Final(new.clone()), 10);
        state.request_translation(new.key);
        assert!(!state.finish_translation(old.key, Ok("旧译文".into()), 20));
        assert!(state.finish_translation(new.key, Ok("新译文".into()), 20));
        assert!(!state.finish_translation(new.key, Ok("重复返回".into()), 30));
    }

    #[test]
    fn session_reset_rejects_cancelled_transcripts_and_translations() {
        let mut state = SubtitleState::new(1, 8, 1_000);
        let old = segment(1, 1, 1, "old session");
        state.apply(TranscriptEvent::Final(old.clone()), 0);
        state.request_translation(old.key);
        state.reset(2);
        assert!(!state.finish_translation(old.key, Ok("late".into()), 100));
        assert!(!state.apply(TranscriptEvent::Final(old), 100));
        assert!(state.history().is_empty());
    }

    #[test]
    fn expiration_does_not_revive_on_late_work() {
        let mut state = SubtitleState::new(1, 8, 1_000);
        let transcript = segment(1, 1, 1, "expire me");
        state.apply(TranscriptEvent::Final(transcript.clone()), 0);
        state.request_translation(transcript.key);
        assert!(!state.finish_translation(transcript.key, Ok("too late".into()), 1_000));
        assert!(state.expire(1_000));
        assert!(state.current().is_none());
        assert!(!state.apply(
            TranscriptEvent::Final(segment(1, 1, 2, "late revision")),
            1_001
        ));
    }

    #[test]
    fn history_stays_bounded_and_evicted_ids_cannot_return() {
        let mut state = SubtitleState::new(1, 2, 1_000);
        for id in 1..=1_000 {
            state.apply(TranscriptEvent::Final(segment(1, id, 1, "text")), id);
        }
        assert_eq!(state.history().len(), 2);
        assert!(!state.apply(TranscriptEvent::Final(segment(1, 1, 2, "evicted")), 1_001));
    }

    #[test]
    fn partial_does_not_request_translation_and_failure_keeps_source() {
        let mut state = SubtitleState::new(1, 8, 1_000);
        let transcript = segment(1, 1, 1, "source");
        state.apply(TranscriptEvent::Partial(transcript.clone()), 0);
        assert!(!state.request_translation(transcript.key));
        state.apply(TranscriptEvent::Final(transcript.clone()), 1);
        state.request_translation(transcript.key);
        state.finish_translation(transcript.key, Err("mock error".into()), 2);
        assert_eq!(
            state.current().map(|row| &row.translation),
            Some(&TranslationState::Failed("mock error".into()))
        );
        assert_eq!(
            state.current().map(|row| row.transcript.text.as_str()),
            Some("source")
        );
    }

    #[test]
    fn invalid_transcripts_are_rejected() {
        let mut state = SubtitleState::new(1, 8, 1_000);
        let mut invalid = segment(1, 1, 1, "text");
        invalid.start_ms = 200;
        assert!(!state.apply(TranscriptEvent::Final(invalid), 0));
        assert!(!state.apply(TranscriptEvent::Partial(segment(1, 1, 1, " ")), 0));
    }
}
