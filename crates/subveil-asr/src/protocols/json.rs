use super::state::TextState;
use crate::{Provider, Shared};
use anyhow::{Context as _, ensure};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};

pub(super) struct Decoder {
    provider: Provider,
    links: HashMap<String, String>,
    order: VecDeque<String>,
    sentence: u64,
    sentence_start: u64,
    starts: HashMap<String, u64>,
    pending_turns: VecDeque<String>,
}
impl Decoder {
    pub fn new(provider: Provider) -> Self {
        Self {
            provider,
            links: HashMap::new(),
            order: VecDeque::new(),
            sentence: 0,
            sentence_start: 0,
            starts: HashMap::new(),
            pending_turns: VecDeque::new(),
        }
    }
    fn link(&mut self, target: &str, source: &str) -> anyhow::Result<()> {
        ensure!(
            target.len() <= 512 && source.len() <= 512,
            "云端消息标识过长"
        );
        if self.order.len() == 128
            && let Some(old) = self.order.pop_front()
        {
            self.links.remove(&old);
        }
        if !self.links.contains_key(target) {
            self.order.push_back(target.into());
        }
        self.links.insert(target.into(), source.into());
        Ok(())
    }
    pub fn parse(
        &mut self,
        text: &str,
        state: &mut TextState<'_>,
        shared: &Shared,
    ) -> anyhow::Result<bool> {
        ensure!(text.len() <= 1_048_576, "云端 JSON 超出上限");
        let v: Value = serde_json::from_str(text).map_err(|_| anyhow::anyhow!("云端 JSON 无效"))?;
        if v.get("error").is_some()
            || v["type"] == "error"
            || v.get("code")
                .is_some_and(|c| c.as_i64().is_none_or(|c| c != 0))
            || v["header"]["event"] == "task-failed"
        {
            anyhow::bail!("云端返回服务错误");
        }
        let kind = v["type"].as_str().unwrap_or("");
        let event_id = v["event_id"].as_str().unwrap_or("");
        ensure!(event_id.len() <= 512, "云端事件标识过长");
        if state.duplicate(event_id) {
            return Ok(false);
        }
        if kind == "session.updated" || kind == "transcription_session.updated" {
            return Ok(true);
        }
        match self.provider {
            Provider::Gemini => {
                if v.get("setupComplete").is_some() {
                    return Ok(true);
                }
                if let Some(go) = v.get("goAway") {
                    let _ = go;
                    anyhow::bail!("Gemini 请求轮换连接");
                }
                let content = &v["serverContent"];
                if let Some(s) = content["inputTranscription"]["text"].as_str() {
                    state.source("continuous", s, true, false, 0, state.audio_ms, shared)?;
                }
                if let Some(t) = content["outputTranscription"]["text"].as_str() {
                    state.target("continuous", t, true, false, shared)?;
                }
            }
            Provider::Tencent => {
                if v.get("result").is_none() {
                    return Ok(true);
                }
                let result = &v["result"];
                let id = v["sentence_id"]
                    .as_str()
                    .context("腾讯结果缺少 sentence_id")?;
                let start = result["start_time"]
                    .as_u64()
                    .context("腾讯结果缺少起始时间")?;
                let end = result["end_time"]
                    .as_u64()
                    .context("腾讯结果缺少结束时间")?;
                let final_ = result["sentence_end"]
                    .as_bool()
                    .context("腾讯结果缺少结束标记")?;
                state.source(
                    id,
                    str_(&result["source_text"]),
                    false,
                    final_,
                    start,
                    end,
                    shared,
                )?;
                state.target(id, str_(&result["target_text"]), false, final_, shared)?;
            }
            Provider::Baidu => {
                let data = &v["data"];
                if data["status"] == "STA" {
                    return Ok(true);
                }
                if data["status"] == "END" {
                    anyhow::bail!("百度语音会话已结束");
                }
                if data["status"] == "TRN" {
                    let r = &data["result"];
                    let final_ = r["type"] == "FIN";
                    let id = format!("sentence:{}", self.sentence);
                    state.source(
                        &id,
                        str_(&r[if final_ { "sentence" } else { "asr" }]),
                        false,
                        final_,
                        self.sentence_start,
                        state.audio_ms,
                        shared,
                    )?;
                    state.target(
                        &id,
                        str_(
                            &r[if final_ {
                                "sentence_trans"
                            } else {
                                "asr_trans"
                            }],
                        ),
                        false,
                        final_,
                        shared,
                    )?;
                    if final_ {
                        self.sentence = self.sentence.saturating_add(1);
                        self.sentence_start = state.audio_ms;
                    }
                }
            }
            Provider::CustomDashScope => {
                if v["header"]["event"] == "task-started" {
                    return Ok(true);
                }
                if v["header"]["event"] == "result-generated" {
                    let r = &v["payload"]["output"]["sentence"];
                    let start = r["begin_time"]
                        .as_u64()
                        .context("DashScope 结果缺少时间标识")?;
                    let end = r["end_time"].as_u64().unwrap_or(state.audio_ms).max(start);
                    let id = format!("start:{start}");
                    state.source(
                        &id,
                        str_(&r["text"]),
                        false,
                        r["sentence_end"].as_bool().unwrap_or(false),
                        start,
                        end,
                        shared,
                    )?;
                }
            }
            Provider::OpenAi | Provider::AzureOpenAi => match kind {
                "session.input_transcript.delta" => state.source(
                    "continuous",
                    str_(&v["delta"]),
                    true,
                    false,
                    0,
                    state.audio_ms,
                    shared,
                )?,
                "session.output_transcript.delta" => {
                    state.target("continuous", str_(&v["delta"]), true, false, shared)?
                }
                "session.closed" => anyhow::bail!("实时翻译会话已关闭"),
                _ => {}
            },
            _ => self.realtime(kind, &v, state, shared)?,
        }
        Ok(false)
    }
    fn realtime(
        &mut self,
        kind: &str,
        v: &Value,
        state: &mut TextState<'_>,
        shared: &Shared,
    ) -> anyhow::Result<()> {
        match kind {
            "input_audio_buffer.speech_started" => {
                if let Some(id) = v["item_id"].as_str() {
                    ensure!(
                        id.len() <= 512 && self.starts.len() < 128,
                        "语音起点标识超过上限"
                    );
                    self.starts.insert(
                        id.into(),
                        v["audio_start_ms"].as_u64().unwrap_or(state.audio_ms),
                    );
                }
            }
            "conversation.item.created" | "conversation.item.added" => {
                let item = &v["item"];
                if item["role"] == "assistant" {
                    let id = item["id"].as_str().context("译文缺少消息标识")?;
                    if let Some(source) = v["previous_item_id"].as_str() {
                        self.link(id, source)?;
                    }
                }
            }
            "conversation.item.input_audio_transcription.delta"
            | "conversation.item.input_audio_transcription.text"
            | "conversation.item.input_audio_transcription.updated"
            | "conversation.item.input_audio_transcription.completed" => {
                let id = v["item_id"].as_str().context("原文缺少消息标识")?;
                let final_ = kind.ends_with(".completed");
                let append = kind.ends_with(".delta");
                let value = if append {
                    str_(&v["delta"]).into()
                } else if final_ || kind.ends_with(".updated") {
                    str_(&v["transcript"]).into()
                } else {
                    format!("{}{}", str_(&v["text"]), str_(&v["stash"]))
                };
                let start = self.starts.get(id).copied().unwrap_or(0);
                state.source(
                    id,
                    &value,
                    append,
                    final_,
                    start,
                    state.audio_ms.max(start),
                    shared,
                )?;
                if final_ {
                    self.starts.remove(id);
                }
            }
            "input_audio_buffer.committed" if self.provider == Provider::Xai => {
                if let Some(id) = v["item_id"].as_str() {
                    ensure!(self.pending_turns.len() < 32, "待翻译回合超过上限");
                    self.pending_turns.push_back(id.into());
                }
            }
            "response.created" if self.provider == Provider::Xai => {
                let id = str_(&v["response"]["id"]);
                // Bind a response once. Overlapping responses never borrow the latest sentence later.
                if let Some(source) = self.pending_turns.pop_front() {
                    self.link(id, &source)?;
                }
            }
            "response.text.delta"
            | "response.output_text.delta"
            | "response.audio_transcript.delta"
            | "response.output_audio_transcript.delta"
            | "response.text.done"
            | "response.output_text.done"
            | "response.audio_transcript.done"
            | "response.output_audio_transcript.done" => {
                let key = if self.provider == Provider::Xai {
                    str_(&v["response_id"])
                } else {
                    str_(&v["item_id"])
                };
                if let Some(source) = self.links.get(key) {
                    let final_ = kind.ends_with(".done");
                    let text = if final_ {
                        v.get("text")
                            .or_else(|| v.get("transcript"))
                            .and_then(Value::as_str)
                            .unwrap_or("")
                    } else {
                        str_(&v["delta"])
                    };
                    state.target(source, text, !final_, final_, shared)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}
fn str_(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}
