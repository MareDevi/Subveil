use crate::Request;
use std::collections::VecDeque;

/// Source-only memory. Never includes a draft, future segment or another route.
#[derive(Default)]
pub(super) struct SourceContext {
    route: Option<(u64, Option<String>, String)>,
    finals: VecDeque<(u64, String)>,
}
impl SourceContext {
    fn align(&mut self, request: &Request) {
        let route = (
            request.segment.key.session_id,
            request.segment.language.clone(),
            request.target.clone(),
        );
        if self.route.as_ref() != Some(&route) {
            self.finals.clear();
            self.route = Some(route);
        }
    }
    pub fn before(&mut self, request: &Request) -> String {
        self.align(request);
        self.finals
            .iter()
            .filter(|(id, _)| *id < request.segment.key.segment_id)
            .map(|(_, text)| text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
    pub fn observe(&mut self, request: &Request) {
        self.align(request);
        if request.provisional {
            return;
        }
        self.finals
            .retain(|(id, _)| *id != request.segment.key.segment_id);
        self.finals.push_back((
            request.segment.key.segment_id,
            request.segment.text.chars().take(1024).collect(),
        ));
        while self.finals.len() > 3 {
            self.finals.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use subveil_core::subtitle::{SegmentKey, TranscriptSegment};
    fn request(session: u64, segment: u64) -> Request {
        Request::new(
            TranscriptSegment {
                key: SegmentKey {
                    session_id: session,
                    segment_id: segment,
                    revision: 1,
                },
                text: format!("source {segment}"),
                language: Some("en".into()),
                start_ms: 0,
                end_ms: 100,
            },
            "zh".into(),
        )
    }
    #[test]
    fn context_rejects_drafts_future_text_self_and_other_routes() {
        let mut context = SourceContext::default();
        let first = request(1, 1);
        context.observe(&first);
        let mut next = request(1, 2);
        assert_eq!(context.before(&next), "source 1");
        next.provisional = true;
        context.observe(&next);
        assert_eq!(context.before(&request(1, 3)), "source 1");
        assert_eq!(context.before(&first), "");
        assert_eq!(context.before(&request(2, 2)), "");
        context.observe(&request(2, 1));
        let mut other = request(2, 2);
        other.target = "ja".into();
        assert_eq!(context.before(&other), "");
    }
}
