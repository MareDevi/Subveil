use crate::{Config, Continuity, Shared, Transcript};
use anyhow::{Context as _, bail, ensure};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::time::Duration;
use subveil_audio::FrameReceiver;
use tokio::time::Instant;
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{Message, client::IntoClientRequest, protocol::WebSocketConfig},
};

pub(crate) fn run(
    config: &Config,
    session: u64,
    frames: &FrameReceiver,
    shared: &Shared,
) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(async {
        tokio::select! {
            result = reconnect(config, session, frames, shared) => result,
            _ = async { while !shared.stopped() { tokio::time::sleep(Duration::from_millis(20)).await; } } => Ok(()),
        }
    });
    runtime.shutdown_timeout(Duration::from_millis(100));
    result
}

async fn reconnect(
    config: &Config,
    session: u64,
    frames: &FrameReceiver,
    shared: &Shared,
) -> anyhow::Result<()> {
    let mut transcript = Transcript::new(session, &config.language);
    let mut recovery = crate::recovery::Recovery::default();
    loop {
        // Never replay audio accumulated while disconnected or after consent withdrawal.
        while frames.pop().is_some() {}
        shared.status("正在连接 Deepgram · 系统音频将上传", frames, false);
        let started = Instant::now();
        let result = connected(config, frames, shared, &mut transcript).await;
        match result {
            Ok(()) => return Ok(()),
            Err(error) => {
                transcript.reset();
                let Some(delay) = recovery.next(&error, started.elapsed()) else {
                    return Err(error.context("识别已停止；修正配置或稍后重新开始"));
                };
                shared.status(
                    format!("{error}；{} 秒后重试（音频仍获准上传）", delay.as_secs()),
                    frames,
                    true,
                );
                tokio::time::sleep(delay).await;
            }
        }
    }
}

async fn connected(
    config: &Config,
    frames: &FrameReceiver,
    shared: &Shared,
    transcript: &mut Transcript,
) -> anyhow::Result<()> {
    let url = format!(
        "wss://api.deepgram.com/v1/listen?model={}&language={}&encoding=linear16&sample_rate={}&channels=1&interim_results=true&endpointing={}&smart_format=true",
        config.model("nova-3"),
        config.language,
        config.sample_rate,
        config.vad.trailing_silence_ms
    );
    let mut request = url.into_client_request()?;
    let mut auth = format!("Token {}", config.setting("DEEPGRAM_API_KEY")?)
        .parse::<tokio_tungstenite::tungstenite::http::HeaderValue>()
        .map_err(|_| anyhow::anyhow!("云端凭据格式无效"))?;
    auth.set_sensitive(true);
    request.headers_mut().insert("Authorization", auth);
    let limits = WebSocketConfig::default()
        .max_message_size(Some(128 * 1_024))
        .max_frame_size(Some(128 * 1_024));
    // Do not expose handshake error bodies: they may contain provider text or headers.
    let (socket, _) = tokio::time::timeout(
        Duration::from_secs(10),
        connect_async_with_config(request, Some(limits), true),
    )
    .await
    .context("云端连接超时")?
    .map_err(crate::recovery::handshake)?;
    while frames.pop().is_some() {}
    stream_audio(socket, config, frames, shared, transcript).await
}

async fn stream_audio<S>(
    socket: tokio_tungstenite::WebSocketStream<S>,
    config: &Config,
    frames: &FrameReceiver,
    shared: &Shared,
    transcript: &mut Transcript,
) -> anyhow::Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (mut sink, mut stream) = socket.split();
    let mut interval = tokio::time::interval(Duration::from_millis(20));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut continuity = Continuity::default();
    let mut mapper = Mapper::default();
    let mut last_audio = Instant::now();
    let mut last_response = Instant::now();
    let mut last_keepalive = Instant::now();
    loop {
        tokio::select! {
            response = stream.next() => {
                let message = response.context("云端连接已关闭")?.map_err(|_| anyhow::anyhow!("云端接收失败"))?;
                last_response = Instant::now();
                match message {
                    Message::Text(text) => {
                        if let Some(event) = mapper.parse(text.as_ref(), transcript)? { shared.emit(event); }
                    }
                    Message::Close(_) => bail!("云端连接已关闭"),
                    Message::Ping(data) => { tokio::time::timeout(Duration::from_secs(3), sink.send(Message::Pong(data))).await.context("云端写入超时")?.map_err(|_| anyhow::anyhow!("云端写入失败"))?; }
                    _ => {}
                }
            }
            _ = interval.tick() => {
                if last_audio.elapsed() >= Duration::from_secs(2) { bail!("系统音频中断，重建识别上下文"); }
                if last_response.elapsed() >= Duration::from_secs(30) { bail!("云端响应超时"); }
                for _ in 0..16 {
                    let Some(frame) = frames.pop() else { break; };
                    ensure!(continuity.accept(&frame, config.sample_rate), "音频丢帧、过期或设备切换，重建识别上下文");
                    mapper.base_ms.get_or_insert(frame.sample_offset * 1_000 / u64::from(frame.rate));
                    let bytes: Vec<u8> = frame.samples[..frame.len].iter().flat_map(|s| ((s.clamp(-1., 1.) * i16::MAX as f32) as i16).to_le_bytes()).collect();
                    tokio::time::timeout(Duration::from_secs(3), sink.send(Message::Binary(bytes.into()))).await.context("云端写入超时")?.map_err(|_| anyhow::anyhow!("云端写入失败"))?;
                    last_audio = Instant::now();
                }
                if last_keepalive.elapsed() >= Duration::from_secs(3) {
                    tokio::time::timeout(Duration::from_secs(3), sink.send(Message::text(r#"{"type":"KeepAlive"}"#))).await.context("云端保活超时")?.map_err(|_| anyhow::anyhow!("云端保活失败"))?;
                    last_keepalive = Instant::now();
                }
                shared.status("Deepgram ASR · 正在上传系统音频", frames, false);
            }
        }
    }
}

#[derive(Deserialize)]
struct Response {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    start: f64,
    #[serde(default)]
    duration: f64,
    #[serde(default)]
    is_final: bool,
    channel: Option<Channel>,
}
#[derive(Deserialize)]
struct Channel {
    alternatives: Vec<Alternative>,
}
#[derive(Deserialize)]
struct Alternative {
    transcript: String,
}
#[derive(Default)]
struct Mapper {
    base_ms: Option<u64>,
    final_end_ms: Option<u64>,
}
impl Mapper {
    fn parse(
        &mut self,
        text: &str,
        transcript: &mut Transcript,
    ) -> anyhow::Result<Option<subveil_core::subtitle::TranscriptEvent>> {
        let result: Response =
            serde_json::from_str(text).map_err(|_| anyhow::anyhow!("云端返回无效 JSON"))?;
        if result.kind == "Error" {
            bail!("云端返回服务错误");
        }
        if result.kind != "Results" {
            return Ok(None);
        }
        ensure!(
            result.start.is_finite()
                && result.duration.is_finite()
                && result.start >= 0.
                && result.duration >= 0.
                && result.start + result.duration < 604_800.,
            "云端返回无效时间范围"
        );
        let start = (result.start * 1_000.).round() as u64;
        let end = ((result.start + result.duration) * 1_000.).round() as u64;
        if self
            .final_end_ms
            .is_some_and(|final_end| start < final_end || end <= final_end)
        {
            return Ok(None);
        }
        let text = result
            .channel
            .and_then(|c| c.alternatives.into_iter().next())
            .context("云端结果缺少 alternatives")?
            .transcript;
        if text.trim().is_empty() {
            if result.is_final {
                transcript.reset();
                self.final_end_ms = Some(end);
            }
            return Ok(None);
        }
        let base = self.base_ms.unwrap_or(0);
        let event = transcript.update(
            text,
            base.saturating_add(start),
            base.saturating_add(end),
            result.is_final,
        );
        if result.is_final && event.is_some() {
            self.final_end_ms = Some(end);
        }
        Ok(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn response(text: &str, start: f64, final_: bool) -> String {
        serde_json::json!({"type":"Results", "start":start, "duration":1.0, "is_final":final_, "speech_final":false, "channel":{"alternatives":[{"transcript":text}]}}).to_string()
    }
    #[test]
    fn final_is_stable_even_without_speech_final_and_duplicates_are_ignored() {
        let mut mapper = Mapper {
            base_ms: Some(2_000),
            ..Default::default()
        };
        let mut t = Transcript::new(9, "en");
        assert!(matches!(
            mapper
                .parse(&response("hello", 0., false), &mut t)
                .unwrap_or_else(|e| panic!("{e}")),
            Some(subveil_core::subtitle::TranscriptEvent::Partial(_))
        ));
        let event = mapper
            .parse(&response("hello world", 0., true), &mut t)
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(
            matches!(event, Some(subveil_core::subtitle::TranscriptEvent::Final(s)) if s.start_ms == 2_000 && s.end_ms == 3_000 && s.key.revision == 2)
        );
        assert!(
            mapper
                .parse(&response("duplicate", 0., true), &mut t)
                .unwrap_or_else(|e| panic!("{e}"))
                .is_none()
        );
        assert!(
            mapper
                .parse(&response("next", 1., true), &mut t)
                .unwrap_or_else(|e| panic!("{e}"))
                .is_some()
        );
    }
    #[test]
    fn errors_never_include_server_payload_or_text() {
        let mut m = Mapper::default();
        let mut t = Transcript::new(1, "ja");
        for data in [
            r#"{"type":"Error","description":"private payload"}"#,
            "private payload",
        ] {
            let error = m
                .parse(data, &mut t)
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default();
            assert!(!error.is_empty() && !error.contains("private payload"));
        }
        assert!(m.parse(&response("text", -1., true), &mut t).is_err());
    }
}

#[cfg(test)]
mod transport_tests {
    use super::*;
    use std::sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64},
    };
    use subveil_audio::{AudioFrame, FrameReceiver};
    use tokio_tungstenite::{WebSocketStream, tungstenite::protocol::Role};

    #[test]
    fn duplex_websocket_sends_pcm_and_recovers_with_new_segment_after_close() -> anyhow::Result<()>
    {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        runtime.block_on(async { tokio::time::timeout(Duration::from_secs(2), async {
            let (sender, frames) = FrameReceiver::channel();
            let shared = Shared { wake: subveil_core::signal::Signal::default(),stop: AtomicBool::new(false), events: crossbeam_queue::ArrayQueue::new(64), translations: crossbeam_queue::ArrayQueue::new(64), snapshot: Mutex::new(Default::default()), dropped: AtomicU64::new(0)};
            let config = Config::default();
            let mut transcript = Transcript::new(88, "en");
            for sequence in 0..2 {
                let (client_io, server_io) = tokio::io::duplex(16_384);
                let client = WebSocketStream::from_raw_socket(client_io, Role::Client, None).await;
                let mut server = WebSocketStream::from_raw_socket(server_io, Role::Server, None).await;
                sender.push(AudioFrame {samples: [0.5; 2048], len: 160, generation: sequence + 1, sequence, sample_offset: sequence * 160, captured: std::time::Instant::now(), rate: 16_000});
                let service = async {
                    let data = server.next().await.context("missing PCM")??;
                    ensure!(matches!(data, Message::Binary(ref b) if b.len() == 320 && b[0..2] == 16_383_i16.to_le_bytes()), "wrong PCM encoding");
                    for final_ in [false, true] {
                        let response = serde_json::json!({"type":"Results", "start":0., "duration":0.01, "is_final":final_, "channel":{"alternatives":[{"transcript":"hello"}]}});
                        server.send(Message::text(response.to_string())).await?;
                    }
                    server.close(None).await?;
                    Ok::<(), anyhow::Error>(())
                };
                let (service_result, stream_result) = tokio::join!(service, stream_audio(client, &config, &frames, &shared, &mut transcript));
                service_result?;
                ensure!(stream_result.is_err(), "closed service must trigger reconnect");
                transcript.reset();
            }
            let mut ids = Vec::new();
            while let Some(event) = shared.events.pop() {
                if let subveil_core::subtitle::TranscriptEvent::Final(s) = event { ids.push(s.key.segment_id); }
            }
            ensure!(ids == [1, 3], "reconnection must not revise finalized segments: {ids:?}");
            Ok::<(), anyhow::Error>(())
        }).await })??;
        Ok(())
    }
}

#[cfg(test)]
mod timeout_tests {
    use super::*;
    use std::sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64},
    };
    use tokio_tungstenite::{WebSocketStream, tungstenite::protocol::Role};

    #[test]
    fn missing_audio_times_out_and_stalled_service_response_times_out() -> anyhow::Result<()> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .start_paused(true)
            .build()?;
        runtime.block_on(async {
            for continuous_audio in [false, true] {
                let (sender, frames) = FrameReceiver::channel();
                let shared = Shared { wake: subveil_core::signal::Signal::default(),stop: AtomicBool::new(false), events: crossbeam_queue::ArrayQueue::new(64), translations: crossbeam_queue::ArrayQueue::new(64), snapshot: Mutex::new(Default::default()), dropped: AtomicU64::new(0)};
                let (client_io, server_io) = tokio::io::duplex(16_384);
                let client = WebSocketStream::from_raw_socket(client_io, Role::Client, None).await;
                let mut server = WebSocketStream::from_raw_socket(server_io, Role::Server, None).await;
                let config = Config::default();
                let mut transcript = Transcript::new(1, "en");
                let service = async {
                    let mut interval = tokio::time::interval(Duration::from_millis(10));
                    let mut sequence = 0;
                    loop {
                        tokio::select! {
                            _ = interval.tick(), if continuous_audio => {
                                sender.push(subveil_audio::AudioFrame {samples:[0.; 2048],len:160,generation:1,sequence,sample_offset:sequence * 160,captured:std::time::Instant::now(),rate:16_000});
                                sequence += 1;
                            }
                            message = server.next() => { if message.is_none() { break; } }
                        }
                    }
                };
                let result = tokio::select! {
                    result = stream_audio(client, &config, &frames, &shared, &mut transcript) => result,
                    _ = service => Err(anyhow::anyhow!("fixture ended")),
                    _ = tokio::time::sleep(Duration::from_secs(35)) => Err(anyhow::anyhow!("fixture deadline")),
                };
                let error = result.err().map(|e| e.to_string()).unwrap_or_default();
                ensure!(error.contains(if continuous_audio {"云端响应超时"} else {"系统音频中断"}), "unexpected error: {error}");
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod empty_final_tests {
    use super::*;
    #[test]
    fn empty_final_closes_identity_without_inventing_text() -> anyhow::Result<()> {
        let mut mapper = Mapper::default();
        let mut transcript = Transcript::new(1, "ja");
        for (text, start, final_) in [("仮", 0., false), ("", 0., true), ("次", 1., false)] {
            let json = serde_json::json!({"type":"Results","start":start,"duration":1.,"is_final":final_,"channel":{"alternatives":[{"transcript":text}]}});
            let event = mapper.parse(&json.to_string(), &mut transcript)?;
            if text.is_empty() {
                assert!(event.is_none());
            }
            if text == "次" {
                assert!(
                    matches!(event, Some(subveil_core::subtitle::TranscriptEvent::Partial(s)) if s.key.segment_id == 2)
                );
            }
        }
        Ok(())
    }
}
