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
    /// A cloud speech service is still revising its own translation.
    Streaming(String),
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
    awaiting_read: bool,
    read_until_ms: Option<u64>,
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

    /// Cancel a session while retaining its bounded history and visible text.
    pub fn stop(&mut self) {
        self.session_id = self.session_id.saturating_add(1);
        for entry in &mut self.entries {
            if matches!(
                entry.translation,
                TranslationState::Pending | TranslationState::Streaming(_)
            ) {
                entry.translation = TranslationState::Failed("会话已停止，翻译已取消".into());
            }
        }
    }

    /// Return the reading row followed by the latest source, with at most two pairs.
    pub fn visible(&self) -> Vec<&Subtitle> {
        let reading = self
            .entries
            .iter()
            .find(|entry| entry.read_until_ms.is_some() && !entry.expired);
        let latest = self.current();
        let mut rows = Vec::with_capacity(2);
        if let Some(entry) = reading {
            rows.push(entry);
        }
        if let Some(entry) = latest
            && reading.is_none_or(|row| row.transcript.key != entry.transcript.key)
        {
            rows.push(entry);
        }
        rows
    }

    /// Schedule completed pairs in source order; each receives at least three seconds.
    /// The bounded history may still evict rows under sustained overload.
    pub fn advance_presentation(&mut self, now_ms: u64) -> bool {
        let mut changed = false;
        for entry in &mut self.entries {
            if entry.read_until_ms.is_some_and(|until| now_ms >= until) {
                entry.read_until_ms = None;
                entry.awaiting_read = false;
                changed = true;
            }
        }
        if !self
            .entries
            .iter()
            .any(|entry| entry.read_until_ms.is_some())
            && let Some(entry) = self.entries.iter_mut().find(|entry| entry.awaiting_read)
        {
            entry.read_until_ms = Some(now_ms.saturating_add(3_000));
            changed = true;
        }
        changed
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
                awaiting_read: false,
                read_until_ms: None,
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
            awaiting_read: false,
            read_until_ms: None,
        });
        while self.entries.len() > self.history_limit {
            self.entries.pop_front();
        }
        true
    }

    /// Apply an integrated speech service's translation to the exact source revision.
    /// Partial translations may accompany Partial speech; expired results only update history.
    pub fn apply_provider_translation(
        &mut self,
        key: SegmentKey,
        text: String,
        is_final: bool,
        now_ms: u64,
    ) -> bool {
        let ttl_ms = self.ttl_ms;
        if text.trim().is_empty() || text.len() > 16_384 {
            return false;
        }
        let Some(entry) = self.matching(key) else {
            return false;
        };
        if matches!(entry.translation, TranslationState::Complete(_)) {
            return false;
        }
        let visible = !entry.expired && now_ms < entry.expires_at_ms;
        entry.translation = if is_final {
            TranslationState::Complete(text)
        } else {
            TranslationState::Streaming(text)
        };
        if visible {
            entry.expires_at_ms = now_ms.saturating_add(ttl_ms);
            entry.awaiting_read = is_final;
        } else {
            entry.expired = true;
        }
        true
    }

    /// Mark a final revision pending, once. Partial events do not trigger translation.
    pub fn request_translation(&mut self, key: SegmentKey) -> bool {
        let Some(entry) = self.matching(key) else {
            return false;
        };
        if entry.expired || !entry.is_final || entry.translation != TranslationState::NotRequested {
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
        if entry.translation != TranslationState::Pending {
            return false;
        }
        let visible = !entry.expired && now_ms < entry.expires_at_ms;
        entry.translation = match result {
            Ok(text) => TranslationState::Complete(text),
            Err(error) => TranslationState::Failed(error),
        };
        if visible {
            entry.expires_at_ms = now_ms.saturating_add(ttl_ms);
            entry.awaiting_read = matches!(entry.translation, TranslationState::Complete(_));
        } else {
            entry.expired = true;
        }
        true
    }

    /// Expire visible rows using monotonic session time. Return whether anything changed.
    pub fn expire(&mut self, now_ms: u64) -> bool {
        let mut changed = false;
        for entry in &mut self.entries {
            if !entry.expired && !entry.awaiting_read && now_ms >= entry.expires_at_ms {
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
            .find(|entry| entry.transcript.key == key)
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
        assert!(state.expire(1_000));
        assert!(state.finish_translation(transcript.key, Ok("too late".into()), 1_000));
        assert_eq!(
            state.history()[0].translation,
            TranslationState::Complete("too late".into())
        );
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
    fn late_translation_updates_history_without_returning_to_overlay_even_without_expire_tick() {
        for expire_first in [false, true] {
            let mut state = SubtitleState::new(1, 8, 8_000);
            let first = segment(1, 1, 1, "first");
            state.apply(TranscriptEvent::Final(first.clone()), 0);
            state.request_translation(first.key);
            state.apply(TranscriptEvent::Partial(segment(1, 2, 1, "next")), 8_500);
            if expire_first {
                state.expire(9_000);
            }
            assert!(state.finish_translation(first.key, Ok("迟到译文".into()), 9_000));
            state.advance_presentation(9_000);
            assert_eq!(
                state.history()[0].translation,
                TranslationState::Complete("迟到译文".into())
            );
            assert_eq!(
                state
                    .visible()
                    .iter()
                    .map(|row| row.transcript.key.segment_id)
                    .collect::<Vec<_>>(),
                vec![2]
            );
        }
    }

    #[test]
    fn completed_pair_keeps_reading_slot_while_new_sources_arrive() {
        let mut state = SubtitleState::new(1, 8, 1_000);
        let first = segment(1, 1, 1, "first");
        state.apply(TranscriptEvent::Final(first.clone()), 0);
        state.request_translation(first.key);
        state.apply(TranscriptEvent::Partial(segment(1, 2, 1, "next")), 100);
        state.finish_translation(first.key, Ok("第一句".into()), 200);
        state.advance_presentation(200);
        state.apply(TranscriptEvent::Partial(segment(1, 3, 1, "third")), 2_000);
        state.expire(2_000);
        state.advance_presentation(3_199);
        assert_eq!(state.visible()[0].transcript.key, first.key);
        assert_eq!(state.visible().len(), 2);
        state.advance_presentation(3_200);
        state.expire(3_200);
        assert!(state.visible().is_empty());
    }

    #[test]
    fn completed_pairs_queue_in_source_order_and_each_gets_three_seconds() {
        let mut state = SubtitleState::new(1, 8, 1_000);
        for id in 1..=3 {
            let row = segment(1, id, 1, "source");
            state.apply(TranscriptEvent::Final(row.clone()), 0);
            state.request_translation(row.key);
        }
        for id in [2, 1, 3] {
            assert!(state.finish_translation(
                segment(1, id, 1, "source").key,
                Ok("译文".into()),
                100
            ));
        }
        for (now, id) in [(100, 1), (3_100, 2), (6_100, 3)] {
            state.advance_presentation(now);
            state.expire(now);
            assert_eq!(state.visible()[0].transcript.key.segment_id, id);
        }
        state.advance_presentation(9_100);
        state.expire(9_100);
        assert!(state.visible().is_empty());
    }

    #[test]
    fn stop_retains_history_cancels_pending_work_and_rejects_late_events() {
        let mut state = SubtitleState::new(1, 8, 8_000);
        let row = segment(1, 1, 1, "keep this");
        state.apply(TranscriptEvent::Final(row.clone()), 0);
        state.request_translation(row.key);
        state.stop();
        assert_eq!(state.history().len(), 1);
        assert!(matches!(
            state.history()[0].translation,
            TranslationState::Failed(_)
        ));
        assert!(!state.finish_translation(row.key, Ok("late".into()), 100));
        assert!(!state.apply(TranscriptEvent::Final(segment(1, 2, 1, "late")), 100));
        assert!(!state.apply_provider_translation(row.key, "late".into(), true, 100));
        assert!(state.current().is_some());
        state.reset(3);
        assert!(state.history().is_empty());
    }

    #[test]
    fn evicted_pending_translation_cannot_return_to_history() {
        let mut state = SubtitleState::new(1, 1, 8_000);
        let row = segment(1, 1, 1, "old");
        state.apply(TranscriptEvent::Final(row.clone()), 0);
        state.request_translation(row.key);
        state.apply(TranscriptEvent::Final(segment(1, 2, 1, "new")), 1);
        assert!(!state.finish_translation(row.key, Ok("late".into()), 100));
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

#[cfg(test)]
mod integrated_translation_tests {
    use super::*;
    fn segment(revision: u32) -> TranscriptSegment {
        TranscriptSegment {
            key: SegmentKey {
                session_id: 1,
                segment_id: 1,
                revision,
            },
            text: "source".into(),
            language: Some("en".into()),
            start_ms: 0,
            end_ms: 100,
        }
    }
    #[test]
    fn provider_translation_tracks_partial_revision_and_rejects_late_results() {
        let mut state = SubtitleState::new(1, 8, 1_000);
        state.apply(TranscriptEvent::Partial(segment(1)), 0);
        assert!(state.apply_provider_translation(segment(1).key, "译文".into(), false, 10));
        assert!(matches!(
            state.current().map(|r| &r.translation),
            Some(TranslationState::Streaming(_))
        ));
        state.apply(TranscriptEvent::Final(segment(2)), 20);
        assert!(!state.apply_provider_translation(segment(1).key, "旧译文".into(), true, 30));
        assert!(state.apply_provider_translation(segment(2).key, "最终译文".into(), true, 30));
        assert!(!state.apply_provider_translation(segment(2).key, "重复修订".into(), false, 40));
    }
    #[test]
    fn expired_or_cancelled_provider_translations_never_revive_subtitles() {
        let mut state = SubtitleState::new(1, 8, 1_000);
        state.apply(TranscriptEvent::Final(segment(1)), 0);
        assert!(state.apply_provider_translation(segment(1).key, "过期".into(), true, 1_000));
        assert!(state.current().is_none());
        state.reset(2);
        assert!(!state.apply_provider_translation(segment(1).key, "取消".into(), true, 1_001));
    }
}
