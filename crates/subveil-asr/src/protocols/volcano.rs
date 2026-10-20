//! Doubao AST v2 protobuf subset. Field numbers follow the published wire schema.
use super::state::TextState;
use crate::Shared;
use anyhow::{Context as _, ensure};
use prost::Message as _;

#[derive(Clone, PartialEq, prost::Message)]
struct Meta {
    #[prost(string, tag = "6")]
    session_id: String,
}
#[derive(Clone, PartialEq, prost::Message)]
struct SourceAudio {
    #[prost(string, tag = "4")]
    format: String,
    #[prost(string, tag = "5")]
    codec: String,
    #[prost(int32, tag = "7")]
    rate: i32,
    #[prost(int32, tag = "8")]
    bits: i32,
    #[prost(int32, tag = "9")]
    channels: i32,
    #[prost(bytes = "vec", tag = "14")]
    data: Vec<u8>,
}
#[derive(Clone, PartialEq, prost::Message)]
struct Options {
    #[prost(string, tag = "1")]
    mode: String,
    #[prost(string, tag = "2")]
    source: String,
    #[prost(string, tag = "3")]
    target: String,
}
#[derive(Clone, PartialEq, prost::Message)]
struct Request {
    #[prost(message, optional, tag = "1")]
    meta: Option<Meta>,
    #[prost(int32, tag = "2")]
    event: i32,
    #[prost(message, optional, tag = "4")]
    audio: Option<SourceAudio>,
    #[prost(message, optional, tag = "6")]
    options: Option<Options>,
}
#[derive(Clone, PartialEq, prost::Message)]
struct ResponseMeta {
    #[prost(int32, tag = "3")]
    status_code: i32,
}
#[derive(Clone, PartialEq, prost::Message)]
struct Response {
    #[prost(message, optional, tag = "1")]
    meta: Option<ResponseMeta>,
    #[prost(int32, tag = "2")]
    event: i32,
    #[prost(string, tag = "4")]
    text: String,
    #[prost(int64, tag = "5")]
    start: i64,
    #[prost(int64, tag = "6")]
    end: i64,
}
pub(super) fn start(id: &str, source: &str, target: &str) -> Vec<u8> {
    Request {
        meta: Some(Meta {
            session_id: id.into(),
        }),
        event: 100,
        audio: Some(SourceAudio {
            format: "wav".into(),
            codec: "raw".into(),
            rate: 16_000,
            bits: 16,
            channels: 1,
            data: Vec::new(),
        }),
        options: Some(Options {
            mode: "s2t".into(),
            source: source.into(),
            target: target.into(),
        }),
    }
    .encode_to_vec()
}
pub(super) fn audio(id: &str, pcm: &[u8]) -> Vec<u8> {
    Request {
        meta: Some(Meta {
            session_id: id.into(),
        }),
        event: 200,
        audio: Some(SourceAudio {
            data: pcm.to_vec(),
            ..Default::default()
        }),
        options: None,
    }
    .encode_to_vec()
}
#[derive(Default)]
pub(super) struct Decoder {
    source: Option<String>,
    target: Option<String>,
    start: u64,
}
impl Decoder {
    pub fn parse(
        &mut self,
        data: &[u8],
        state: &mut TextState<'_>,
        shared: &Shared,
    ) -> anyhow::Result<bool> {
        ensure!(data.len() <= 1_048_576, "云端 protobuf 超出上限");
        let r = Response::decode(data).map_err(|_| anyhow::anyhow!("云端 protobuf 无效"))?;
        ensure!(
            r.meta
                .as_ref()
                .is_none_or(|m| matches!(m.status_code, 0 | 20_000_000)),
            "火山引擎返回服务错误"
        );
        ensure!(r.start >= 0 && r.end >= 0, "云端返回负时间戳");
        match r.event {
            150 => return Ok(true),
            152 | 153 => anyhow::bail!("火山引擎会话已结束或失败"),
            650 => {
                self.start = r.start as u64;
                self.source = Some(format!("start:{}", r.start));
            }
            651 | 652 => {
                let id = self.source.as_deref().context("火山原文缺少起始事件")?;
                state.source(
                    id,
                    &r.text,
                    false,
                    r.event == 652,
                    self.start,
                    (r.end as u64).max(self.start),
                    shared,
                )?;
            }
            653 => self.target = Some(format!("start:{}", r.start)),
            654 | 655 => {
                let id = self.target.as_deref().context("火山译文缺少起始事件")?;
                state.target(id, &r.text, false, r.event == 655, shared)?;
            }
            _ => {}
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn protobuf_start_and_audio_match_published_field_numbers() -> anyhow::Result<()> {
        let request = Request::decode(start("id", "ja", "zh").as_slice())?;
        assert_eq!(request.event, 100);
        assert!(
            request
                .options
                .is_some_and(|o| o.mode == "s2t" && o.source == "ja")
        );
        let request = Request::decode(audio("id", &[7; 2560]).as_slice())?;
        assert_eq!(request.event, 200);
        assert_eq!(request.audio.map(|a| a.data), Some(vec![7; 2560]));
        // Literal wire response: event=150, status_code=20000000 in response_meta tag 3.
        let response =
            Response::decode(&[0x0a, 0x05, 0x18, 0x80, 0xda, 0xc4, 0x09, 0x10, 0x96, 0x01][..])?;
        assert_eq!(response.event, 150);
        assert!(response.meta.is_some_and(|m| m.status_code == 20_000_000));
        Ok(())
    }
    #[test]
    fn protobuf_subtitle_timestamps_correlate_translation_without_latest_row_guess()
    -> anyhow::Result<()> {
        let config = crate::Config {
            provider: crate::Provider::Volcano,
            ..Default::default()
        };
        let shared = super::super::tests::shared();
        let mut next = 1;
        let mut text = TextState::new(&config, 7, &mut next);
        let mut decoder = Decoder::default();
        for (event, start, end, source) in [
            (650, 0, 0, ""),
            (651, 0, 0, "Hell"),
            (652, 0, 100, "Hello"),
            (653, 0, 0, ""),
            (655, 0, 100, "你好"),
        ] {
            let frame = Response {
                event,
                start,
                end,
                text: source.into(),
                meta: None,
            }
            .encode_to_vec();
            decoder.parse(&frame, &mut text, &shared)?;
        }
        let state = super::super::tests::collect(&shared);
        super::super::tests::bilingual(&state, "Hello", "你好", true);
        assert!(decoder.parse(&[0xff], &mut text, &shared).is_err());
        Ok(())
    }
}
