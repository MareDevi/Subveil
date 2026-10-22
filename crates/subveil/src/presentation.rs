//! Bounded, text-free state-to-paint diagnostics. Paint is not compositor presentation.
use std::{cell::RefCell, collections::VecDeque, rc::Rc, time::Instant};
use subveil_core::subtitle::SegmentKey;

const LIMIT: usize = 256;
#[derive(Clone, Default)]
pub(crate) struct Probe(Rc<RefCell<State>>);
#[derive(Default)]
struct State {
    next: u64,
    rows: VecDeque<Row>,
    source: Samples,
    translation: Samples,
}
struct Row {
    key: SegmentKey,
    source: Stage,
    translation: Option<Stage>,
}
struct Stage {
    serial: u64,
    accepted: Instant,
    painted: bool,
}
#[derive(Default)]
struct Samples {
    accepted: u64,
    painted: u64,
    superseded: u64,
    delays: VecDeque<u64>,
}
#[derive(Clone, Copy)]
pub(crate) struct Marker {
    key: SegmentKey,
    source: Option<u64>,
    translation: Option<u64>,
}
impl Samples {
    fn record(&mut self, delay_us: u64) {
        self.painted += 1;
        if self.delays.len() == LIMIT {
            self.delays.pop_front();
        }
        self.delays.push_back(delay_us);
    }
    fn summary(&self) -> String {
        if self.delays.is_empty() {
            return format!(
                "接收 {} · 绘制 0 · 未绘制即替换/淘汰 {} · 尚无绘制样本",
                self.accepted, self.superseded
            );
        }
        let mut sorted: Vec<_> = self.delays.iter().copied().collect();
        sorted.sort_unstable();
        let quantile =
            |p: usize| sorted[(sorted.len() * p).div_ceil(100).saturating_sub(1)] as f64 / 1000.;
        format!(
            "接收 {} · 绘制 {} · 未绘制即替换/淘汰 {} · 最近 {} 次 p50 {:.1} / p95 {:.1} ms",
            self.accepted,
            self.painted,
            self.superseded,
            sorted.len(),
            quantile(50),
            quantile(95)
        )
    }
}
impl Probe {
    pub fn reset(&self) {
        *self.0.borrow_mut() = State::default();
    }
    pub fn source(&self, key: SegmentKey, now: Instant) {
        let mut state = self.0.borrow_mut();
        if let Some(index) = state
            .rows
            .iter()
            .position(|r| r.key.session_id == key.session_id && r.key.segment_id == key.segment_id)
            && let Some(old) = state.rows.remove(index)
        {
            state.retire(old);
        }
        if state.rows.len() == LIMIT
            && let Some(old) = state.rows.pop_front()
        {
            state.retire(old);
        }
        state.next = state.next.saturating_add(1);
        let serial = state.next;
        state.rows.push_back(Row {
            key,
            source: Stage {
                serial,
                accepted: now,
                painted: false,
            },
            translation: None,
        });
        state.source.accepted += 1;
    }
    pub fn translation(&self, key: SegmentKey, now: Instant) {
        let mut state = self.0.borrow_mut();
        let Some(index) = state
            .rows
            .iter()
            .position(|r| r.key.session_id == key.session_id && r.key.segment_id == key.segment_id)
        else {
            return;
        };
        if state.rows[index]
            .translation
            .as_ref()
            .is_some_and(|s| !s.painted)
        {
            state.translation.superseded += 1;
        }
        state.next = state.next.saturating_add(1);
        let serial = state.next;
        state.rows[index].translation = Some(Stage {
            serial,
            accepted: now,
            painted: false,
        });
        state.translation.accepted += 1;
    }
    pub fn marker(&self, key: SegmentKey, source: bool, translation: bool) -> Option<Marker> {
        let state = self.0.borrow();
        let row = state.rows.iter().find(|r| r.key == key)?;
        Some(Marker {
            key,
            source: source.then_some(row.source.serial),
            translation: translation
                .then_some(row.translation.as_ref())
                .flatten()
                .map(|s| s.serial),
        })
    }
    pub fn paint(&self, marker: Marker, now: Instant) {
        // Never wait, notify an Entity, or request another frame while painting.
        let Ok(mut state) = self.0.try_borrow_mut() else {
            return;
        };
        let Some(index) = state.rows.iter().position(|r| r.key == marker.key) else {
            return;
        };
        let row = &mut state.rows[index];
        let source = paint_stage(&mut row.source, marker.source, now);
        let translation = row
            .translation
            .as_mut()
            .and_then(|s| paint_stage(s, marker.translation, now));
        if let Some(delay) = source {
            state.source.record(delay);
            tracing::debug!(
                stage = "source",
                delay_us = delay,
                "subtitle state-to-paint"
            );
        }
        if let Some(delay) = translation {
            state.translation.record(delay);
            tracing::debug!(
                stage = "translation",
                delay_us = delay,
                "subtitle state-to-paint"
            );
        }
    }
    pub fn summaries(&self) -> (String, String) {
        let state = self.0.borrow();
        (state.source.summary(), state.translation.summary())
    }
}
impl State {
    fn retire(&mut self, row: Row) {
        self.source.superseded += u64::from(!row.source.painted);
        self.translation.superseded += u64::from(row.translation.is_some_and(|s| !s.painted));
    }
}
fn paint_stage(stage: &mut Stage, serial: Option<u64>, now: Instant) -> Option<u64> {
    if serial != Some(stage.serial) || stage.painted || now < stage.accepted {
        return None;
    }
    stage.painted = true;
    Some(
        now.duration_since(stage.accepted)
            .as_micros()
            .min(u128::from(u64::MAX)) as u64,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn paint_counts_once_and_rejects_stale_revision_and_translation_marker() -> anyhow::Result<()> {
        let probe = Probe::default();
        let mut key = SegmentKey {
            session_id: 1,
            segment_id: 1,
            revision: 1,
        };
        let now = Instant::now();
        probe.source(key, now);
        probe.translation(key, now);
        let old = probe
            .marker(key, true, true)
            .ok_or_else(|| anyhow::anyhow!("marker"))?;
        probe.translation(key, now + Duration::from_millis(2));
        probe.paint(old, now + Duration::from_millis(5));
        let current = probe
            .marker(key, true, true)
            .ok_or_else(|| anyhow::anyhow!("marker"))?;
        probe.paint(current, now + Duration::from_millis(10));
        probe.paint(current, now + Duration::from_millis(20));
        let state = probe.0.borrow();
        assert_eq!(state.source.delays, [5000]);
        assert_eq!(state.translation.delays, [8000]);
        assert_eq!(state.translation.superseded, 1);
        drop(state);
        key.revision = 2;
        probe.source(key, now);
        probe.paint(old, now + Duration::from_secs(1));
        assert_eq!(probe.0.borrow().source.painted, 1);
        for id in 2..=1000 {
            key.segment_id = id;
            probe.source(key, now);
        }
        assert_eq!(probe.0.borrow().rows.len(), LIMIT);
        assert_eq!(probe.0.borrow().source.superseded, 744);
        probe.reset();
        assert!(probe.0.borrow().rows.is_empty());
        Ok(())
    }
}
