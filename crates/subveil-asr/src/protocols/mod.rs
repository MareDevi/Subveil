//! Bounded transport for integrated cloud speech services.
mod connection;
mod json;
mod state;
mod volcano;

use crate::{Config, Continuity, Provider, Shared};
use anyhow::{Context as _, ensure};
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use subveil_audio::FrameReceiver;
use tokio::time::Instant;
use tokio_tungstenite::{
    WebSocketStream, connect_async_with_config,
    tungstenite::{Message, protocol::WebSocketConfig},
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
    let result=runtime.block_on(async{
        tokio::select!{
            result=reconnect(config,session,frames,shared)=>result,
            _=async{while !shared.stopped(){tokio::time::sleep(Duration::from_millis(20)).await;}}=>Ok(()),
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
    let mut next = 1;
    let mut recovery = crate::recovery::Recovery::default();
    loop {
        while frames.pop().is_some() {}
        shared.status(
            format!(
                "连接 {} · 音频发送至 {}",
                config.provider.name(),
                config.provider.destination()
            ),
            frames,
            false,
        );
        let started = Instant::now();
        if let Err(error) = connected(config, session, &mut next, frames, shared).await {
            let Some(delay) = recovery.next(&error, started.elapsed()) else {
                return Err(error.context("识别已停止；修正配置或稍后重新开始"));
            };
            shared.status(
                format!("{error}；{} 秒后重试（本次上传仍获准）", delay.as_secs()),
                frames,
                true,
            );
            tokio::time::sleep(delay).await;
        }
    }
}
async fn connected(
    config: &Config,
    session: u64,
    next: &mut u64,
    frames: &FrameReceiver,
    shared: &Shared,
) -> anyhow::Result<()> {
    let plan = connection::prepare(config).map_err(|_| crate::recovery::Failure::Configuration)?;
    let limits = WebSocketConfig::default()
        .max_message_size(Some(1_048_576))
        .max_frame_size(Some(1_048_576));
    // Never format handshake errors: URLs and provider bodies may contain credentials.
    let (mut socket, _) = tokio::time::timeout(
        Duration::from_secs(10),
        connect_async_with_config(plan.request, Some(limits), true),
    )
    .await
    .context("云端握手超时")?
    .map_err(crate::recovery::handshake)?;
    if plan.wait_created {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let message = socket
                    .next()
                    .await
                    .context("云端初始化连接关闭")?
                    .map_err(|_| anyhow::anyhow!("云端初始化接收失败"))?;
                if let Message::Text(text) = message {
                    let v: serde_json::Value = serde_json::from_str(text.as_ref())
                        .map_err(|_| anyhow::anyhow!("云端初始化 JSON 无效"))?;
                    if v["type"] == "error" || v.get("error").is_some() {
                        return Err(crate::recovery::service(&v).into());
                    }
                    if matches!(
                        v["type"].as_str(),
                        Some(
                            "session.created"
                                | "conversation.created"
                                | "transcription_session.created"
                        )
                    ) {
                        return Ok::<(), anyhow::Error>(());
                    }
                } else if matches!(message, Message::Close(_)) {
                    anyhow::bail!("云端初始化连接关闭");
                }
            }
        })
        .await
        .context("云端会话创建超时")??;
    }
    if let Some(init) = plan.init {
        send(&mut socket, init).await?;
    }
    stream(
        socket,
        config,
        session,
        next,
        frames,
        shared,
        &plan.id,
        plan.chunk_ms,
    )
    .await
}
async fn send<S>(socket: &mut S, message: Message) -> anyhow::Result<()>
where
    S: futures_util::Sink<Message> + Unpin,
{
    tokio::time::timeout(Duration::from_secs(3), socket.send(message))
        .await
        .context("云端写入超时")?
        .map_err(|_| anyhow::anyhow!("云端写入失败"))
}
#[expect(
    clippy::too_many_arguments,
    reason = "Transport inputs remain explicit for in-memory protocol verification"
)]
async fn stream<S>(
    socket: WebSocketStream<S>,
    config: &Config,
    session: u64,
    next: &mut u64,
    frames: &FrameReceiver,
    shared: &Shared,
    id: &str,
    chunk_ms: u32,
) -> anyhow::Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (mut sink, mut source) = socket.split();
    let mut json = json::Decoder::new(config.provider);
    let mut binary = volcano::Decoder::default();
    let mut state = state::TextState::new(config, session, next);
    let mut tick = tokio::time::interval(Duration::from_millis(10));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut ready = false;
    let started = Instant::now();
    let mut last_response = started;
    let mut last_audio = started;
    let mut next_send = started;
    let mut continuity = Continuity::default();
    let mut pending = Vec::new();
    let mut received = 0_u64;
    let mut positioned = false;
    let chunk = config.sample_rate as usize * chunk_ms as usize / 1000 * 2;
    loop {
        tokio::select! {
            message=source.next()=>{
                let message=message.context("云端连接关闭")?.map_err(|_|anyhow::anyhow!("云端接收失败"))?;
                last_response=Instant::now();
                let acknowledged=match message{
                    Message::Text(text)=>json.parse(text.as_ref(),&mut state,shared)?,
                    Message::Binary(data) if config.provider==Provider::Volcano=>binary.parse(&data,&mut state,shared)?,
                    Message::Close(_)=>anyhow::bail!("云端关闭会话"),
                    Message::Ping(data)=>{send(&mut sink,Message::Pong(data)).await?;false}
                    _=>false,
                };
                if !ready&&acknowledged{ready=true;while frames.pop().is_some(){}last_audio=Instant::now();next_send=Instant::now()+Duration::from_millis(u64::from(chunk_ms));}
            }
            _=tick.tick()=>{
                if !ready{ensure!(started.elapsed()<Duration::from_secs(10),"云端配置确认超时");continue;}
                ensure!(last_response.elapsed()<Duration::from_secs(30),"云端响应超时");
                for _ in 0..16{
                    let Some(frame)=frames.pop()else{break;};
                    ensure!(continuity.accept(&frame,config.sample_rate),"音频不连续、过期或设备切换，重建会话");
                    if !positioned{state.base_ms=frame.sample_offset*1000/u64::from(frame.rate);positioned=true;}
                    last_audio=Instant::now();
                    pending.extend(frame.samples[..frame.len].iter().flat_map(|v|((v.clamp(-1.,1.)*i16::MAX as f32)as i16).to_le_bytes()));
                    ensure!(pending.len()<=config.sample_rate as usize,"上传积压超过 500 ms，重建会话");
                }
                ensure!(last_audio.elapsed()<Duration::from_secs(2),"系统音频中断");
                if pending.len()>=chunk&&Instant::now()>=next_send{
                    let message=connection::audio(config.provider,id,&pending[..chunk]);
                    send(&mut sink,message).await?;
                    pending.drain(..chunk);received+=chunk as u64/2;
                    state.audio_ms=received*1000/u64::from(config.sample_rate);
                    next_send+=Duration::from_millis(u64::from(chunk_ms));
                }
                shared.status(format!("{} · 正在上传至 {}{}",config.provider.name(),config.provider.destination(),if matches!(config.provider,Provider::OpenAi|Provider::AzureOpenAi|Provider::Gemini){" · 连续文字流"}else{""}),frames,false);
            }
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod transport_tests {
    use super::*;
    use tokio_tungstenite::tungstenite::protocol::Role;
    #[test]
    fn paced_audio_waits_for_configuration_and_receives_integrated_translation()
    -> anyhow::Result<()> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .start_paused(true)
            .build()?;
        runtime.block_on(async{
            let (sender,frames)=FrameReceiver::channel();let shared=super::tests::shared();
            let config=Config{provider:Provider::Baidu,..Default::default()};let mut next=1;
            let (a,b)=tokio::io::duplex(16_384);
            let client=WebSocketStream::from_raw_socket(a,Role::Client,None).await;
            let mut server=WebSocketStream::from_raw_socket(b,Role::Server,None).await;
            let fixture=async{
                server.send(Message::text(r#"{"code":0,"data":{"status":"STA"}}"#)).await?;
                let mut clock=tokio::time::interval(Duration::from_millis(10));let mut sequence=0;let mut times=Vec::new();
                loop{
                    tokio::select!{
                        _=clock.tick()=>{
                            sender.push(subveil_audio::AudioFrame{samples:[0.5;2048],len:160,generation:1,sequence,sample_offset:sequence*160,captured:std::time::Instant::now(),rate:16_000});sequence+=1;
                        }
                        message=server.next()=>{
                            let Message::Binary(data)=message.context("missing frame")?? else{continue;};
                            ensure!(data.len()==1280,"Baidu expects 40 ms PCM packets");times.push(Instant::now());
                            if times.len()==2{
                                ensure!(times[1].duration_since(times[0])>=Duration::from_millis(40),"audio packets must be paced");
                                server.send(Message::text(r#"{"code":0,"data":{"status":"TRN","result":{"type":"FIN","sentence":"Hello","sentence_trans":"你好"}}}"#)).await?;
                                server.close(None).await?;return Ok::<(),anyhow::Error>(());
                            }
                        }
                    }
                }
            };
            let (fixture,result)=tokio::time::timeout(Duration::from_secs(2),async{tokio::join!(fixture,stream(client,&config,7,&mut next,&frames,&shared,"test",40))}).await?;
            fixture?;ensure!(result.is_err(),"closed service must cause reconnect");
            let state=super::tests::collect(&shared);super::tests::bilingual(&state,"Hello","你好",true);Ok(())
        })
    }
    #[test]
    fn no_audio_leaves_socket_before_service_configuration_acknowledgement() -> anyhow::Result<()> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .start_paused(true)
            .build()?;
        runtime.block_on(async{
            let (sender,frames)=FrameReceiver::channel();
            sender.push(subveil_audio::AudioFrame{samples:[0.5;2048],len:2048,generation:1,sequence:0,sample_offset:0,captured:std::time::Instant::now(),rate:16_000});
            let config=Config{provider:Provider::Baidu,..Default::default()};let shared=super::tests::shared();let mut next=1;
            let (a,b)=tokio::io::duplex(16_384);let client=WebSocketStream::from_raw_socket(a,Role::Client,None).await;let mut server=WebSocketStream::from_raw_socket(b,Role::Server,None).await;
            let error=tokio::select!{
                result=stream(client,&config,7,&mut next,&frames,&shared,"test",40)=>result.err().map(|e|e.to_string()).unwrap_or_default(),
                _=server.next()=>anyhow::bail!("audio leaked before acknowledgement"),
            };
            ensure!(error.contains("配置确认超时"),"unexpected result: {error}");Ok(())
        })
    }
}
