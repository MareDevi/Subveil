use crate::{Config, ProviderTranslation, Shared};
use anyhow::ensure;
use std::collections::{HashMap, HashSet, VecDeque};
use subveil_core::subtitle::{SegmentKey, TranscriptEvent, TranscriptSegment};

#[derive(Default)]
struct Row {
    key: Option<SegmentKey>,
    source: String,
    target: String,
    source_final: bool,
    target_final: bool,
    start: u64,
    end: u64,
    emitted_end: u64,
}
pub(super) struct TextState<'a> {
    rows: HashMap<String, Row>,
    order: VecDeque<String>,
    retired: HashSet<String>,
    seen: HashSet<String>,
    seen_order: VecDeque<String>,
    session: u64,
    next: &'a mut u64,
    language: &'a str,
    pub base_ms: u64,
    pub audio_ms: u64,
}
impl<'a> TextState<'a> {
    pub fn new(config: &'a Config, session: u64, next: &'a mut u64) -> Self {
        Self {
            rows: HashMap::new(),
            order: VecDeque::new(),
            retired: HashSet::new(),
            seen: HashSet::new(),
            seen_order: VecDeque::new(),
            session,
            next,
            language: if config.provider.automatic_language() {
                "auto"
            } else {
                &config.language
            },
            base_ms: 0,
            audio_ms: 0,
        }
    }
    pub fn duplicate(&mut self, event: &str) -> bool {
        if event.is_empty() {
            return false;
        }
        if self.seen.contains(event) {
            return true;
        }
        if self.seen_order.len() == 256
            && let Some(id) = self.seen_order.pop_front()
        {
            self.seen.remove(&id);
        }
        self.seen.insert(event.into());
        self.seen_order.push_back(event.into());
        false
    }
    fn row(&mut self, id: &str) -> anyhow::Result<Option<&mut Row>> {
        ensure!(!id.is_empty() && id.len() <= 512, "服务返回无效字幕标识");
        if self.retired.contains(id) {
            return Ok(None);
        }
        if !self.rows.contains_key(id) {
            ensure!(self.retired.len() < 512, "字幕标识达到会话上限，重建连接");
            if self.order.len() == 64
                && let Some(old) = self.order.pop_front()
            {
                self.rows.remove(&old);
                self.retired.insert(old);
            }
            self.rows.insert(id.into(), Row::default());
            self.order.push_back(id.into());
        }
        Ok(self.rows.get_mut(id))
    }
    #[expect(
        clippy::too_many_arguments,
        reason = "Wire revisions carry identity, text and timestamps separately"
    )]
    pub fn source(
        &mut self,
        id: &str,
        text: &str,
        append: bool,
        final_: bool,
        start: u64,
        end: u64,
        shared: &Shared,
    ) -> anyhow::Result<()> {
        ensure!(start <= end, "服务返回无效字幕时间");
        let Some(row) = self.row(id)? else {
            return Ok(());
        };
        if row.source_final {
            return Ok(());
        }
        if final_ && !append && text.trim().is_empty() {
            row.source_final = true;
            return Ok(());
        }
        let changed = update(&mut row.source, text, append, id == "continuous")?;
        row.start = start;
        let refresh = end.saturating_sub(row.emitted_end) >= 500;
        row.end = end;
        if row.source.is_empty() {
            return Ok(());
        }
        if !changed && !refresh && !final_ {
            return Ok(());
        }
        let old = row.key;
        let key = SegmentKey {
            session_id: self.session,
            segment_id: old.map(|k| k.segment_id).unwrap_or_else(|| {
                let id = *self.next;
                *self.next = self.next.saturating_add(1);
                id
            }),
            revision: old.map(|k| k.revision.saturating_add(1)).unwrap_or(1),
        };
        let row = self
            .rows
            .get_mut(id)
            .ok_or_else(|| anyhow::anyhow!("字幕状态已失效"))?;
        row.key = Some(key);
        row.emitted_end = end;
        row.source_final = final_;
        let transcript = TranscriptSegment {
            key,
            text: row.source.clone(),
            language: if self.language == "auto" {
                None
            } else {
                Some(self.language.into())
            },
            start_ms: self.base_ms.saturating_add(start),
            end_ms: self.base_ms.saturating_add(end),
        };
        shared.emit(if final_ {
            TranscriptEvent::Final(transcript)
        } else {
            TranscriptEvent::Partial(transcript)
        });
        if !row.target.is_empty() {
            shared.translate(ProviderTranslation {
                key,
                text: row.target.clone(),
                is_final: row.target_final,
            });
        }
        Ok(())
    }
    pub fn target(
        &mut self,
        id: &str,
        text: &str,
        append: bool,
        final_: bool,
        shared: &Shared,
    ) -> anyhow::Result<()> {
        let Some(row) = self.row(id)? else {
            return Ok(());
        };
        if row.target_final {
            return Ok(());
        }
        update(&mut row.target, text, append, id == "continuous")?;
        row.target_final = final_;
        if let Some(key) = row.key
            && !row.target.is_empty()
        {
            shared.translate(ProviderTranslation {
                key,
                text: row.target.clone(),
                is_final: final_,
            });
        }
        Ok(())
    }
}
fn update(current: &mut String, text: &str, append: bool, rolling: bool) -> anyhow::Result<bool> {
    ensure!(text.len() <= 16_384, "字幕文本超出上限");
    let before = current.clone();
    if append {
        current.push_str(text);
    } else if !text.is_empty() {
        current.clear();
        current.push_str(text);
    }
    if rolling && current.len() > 8_192 {
        let mut cut = current.len() - 8_192;
        while !current.is_char_boundary(cut) {
            cut += 1;
        }
        current.drain(..cut);
    }
    ensure!(current.len() <= 16_384, "单段字幕超出上限，重建连接");
    Ok(*current != before)
}
