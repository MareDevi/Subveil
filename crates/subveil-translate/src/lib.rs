//! Final-only text translation with bounded queues, deadlines and session cancellation.
#![deny(missing_docs)]
mod http;
use crossbeam_queue::ArrayQueue;
use futures_util::future::BoxFuture;
pub use http::{HttpTranslator, Provider};
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use subveil_core::subtitle::{SegmentKey, TranscriptSegment};

/// One immutable source revision and bounded preceding source context.
#[derive(Clone)]
#[non_exhaustive]
pub struct Request {
    /// Exact source identity.
    pub segment: TranscriptSegment,
    /// Target language code.
    pub target: String,
    /// Previous source sentences; never future text.
    pub context: String,
    /// Monotonic deadline including queue wait and retries.
    pub deadline: Instant,
}
impl Request {
    /// Build a request. Caller submits only accepted Final revisions.
    pub fn new(segment: TranscriptSegment, target: String) -> Self {
        Self {
            segment,
            target,
            context: String::new(),
            deadline: Instant::now() + Duration::from_secs(6),
        }
    }
}
/// A backend must not log source text, responses or credentials.
pub trait Translator: Send + 'static {
    /// Translate one source revision. Dropping the future cancels network work.
    fn translate<'a>(&'a self, request: &'a Request) -> BoxFuture<'a, Result<String, String>>;
}
/// Result associated with the exact source identity.
#[non_exhaustive]
pub struct Response {
    /// Source revision.
    pub key: SegmentKey,
    /// Bounded translated text or sanitized error.
    pub result: Result<String, String>,
}
struct Shared {
    stop: AtomicBool,
    input: ArrayQueue<Request>,
    output: ArrayQueue<Response>,
    dropped: AtomicU64,
}
/// One session's independent translation worker. Drop cancels without blocking GUI.
pub struct Translation {
    shared: Arc<Shared>,
}
impl Translation {
    /// Start a backend for one immutable session identity.
    pub fn start(backend: impl Translator, session: u64) -> anyhow::Result<Self> {
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            input: ArrayQueue::new(8),
            output: ArrayQueue::new(64),
            dropped: AtomicU64::new(0),
        });
        let state = shared.clone();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        thread::Builder::new()
            .name("subveil-translate".into())
            .spawn(move || runtime.block_on(run(backend, session, &state)))?;
        Ok(Self { shared })
    }
    /// Submit a Final. Evict oldest waiting request under overload and report its failure.
    pub fn submit(&self, request: Request) {
        if self.shared.stop.load(Ordering::Relaxed) {
            return;
        }
        if let Some(old) = self.shared.input.force_push(request) {
            self.shared.dropped.fetch_add(1, Ordering::Relaxed);
            emit(
                &self.shared,
                Response {
                    key: old.segment.key,
                    result: Err("翻译队列已满；已丢弃旧请求".into()),
                },
            );
        }
    }
    /// Poll one response without waiting.
    pub fn pop(&self) -> Option<Response> {
        self.shared.output.pop()
    }
    /// Count input/output evictions without exposing text.
    pub fn dropped(&self) -> u64 {
        self.shared.dropped.load(Ordering::Relaxed)
    }
    /// Cancel in-flight work and queued text. Results must also pass SubtitleState guards.
    pub fn stop(&self) {
        self.shared.stop.store(true, Ordering::Relaxed);
    }
}
impl Drop for Translation {
    fn drop(&mut self) {
        self.stop();
    }
}
fn emit(shared: &Shared, result: Response) {
    if shared.output.force_push(result).is_some() {
        shared.dropped.fetch_add(1, Ordering::Relaxed);
    }
}
async fn run(backend: impl Translator, session: u64, shared: &Shared) {
    let mut history = VecDeque::<String>::new();
    while !shared.stop.load(Ordering::Relaxed) {
        let Some(mut request) = shared.input.pop() else {
            tokio::time::sleep(Duration::from_millis(10)).await;
            continue;
        };
        if request.segment.key.session_id != session {
            continue;
        }
        let result = if request.segment.text.is_empty()
            || request.segment.text.len() > 16_384
            || !matches!(request.target.as_str(), "zh" | "en" | "ja")
        {
            Err("翻译请求文本或目标语种无效".into())
        } else if Instant::now() >= request.deadline {
            Err("翻译请求已过期".into())
        } else {
            request.context = history.iter().cloned().collect::<Vec<_>>().join("\n");
            let future = backend.translate(&request);
            tokio::pin!(future);
            loop {
                tokio::select! {
                    result = &mut future => break result,
                    _ = tokio::time::sleep_until(request.deadline.into()) => break Err("翻译超时".into()),
                    _ = tokio::time::sleep(Duration::from_millis(10)) => {
                        if shared.stop.load(Ordering::Relaxed) { return; }
                    }
                }
            }
        };
        if shared.stop.load(Ordering::Relaxed) {
            return;
        }
        let result = result.and_then(|text| {
            if text.trim().is_empty() || text.len() > 16_384 {
                Err("译文为空或超过长度限制".into())
            } else {
                Ok(text)
            }
        });
        // Retain at most three short source sentences, never an unlimited transcript.
        history.push_back(request.segment.text.chars().take(1024).collect());
        while history.len() > 3 {
            history.pop_front();
        }
        emit(
            shared,
            Response {
                key: request.segment.key,
                result,
            },
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Echo;
    impl Translator for Echo {
        fn translate<'a>(&'a self, request: &'a Request) -> BoxFuture<'a, Result<String, String>> {
            Box::pin(async move { Ok(format!("中文:{}", request.segment.text)) })
        }
    }
    fn request(session: u64, id: u64) -> Request {
        Request::new(
            TranscriptSegment {
                key: SegmentKey {
                    session_id: session,
                    segment_id: id,
                    revision: 2,
                },
                text: "hello".into(),
                language: Some("en".into()),
                start_ms: 0,
                end_ms: 100,
            },
            "zh".into(),
        )
    }
    #[test]
    fn worker_preserves_identity_and_expired_requests_fail() -> anyhow::Result<()> {
        let worker = Translation::start(Echo, 9)?;
        worker.submit(request(8, 1));
        let mut expired = request(9, 2);
        expired.deadline = Instant::now() - Duration::from_secs(1);
        worker.submit(expired);
        worker.submit(request(9, 3));
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut responses = Vec::new();
        while Instant::now() < deadline && responses.len() < 2 {
            if let Some(response) = worker.pop() {
                responses.push(response);
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(responses.len(), 2);
        assert!(responses[0].result.is_err());
        assert_eq!(responses[1].key, request(9, 3).segment.key);
        assert_eq!(responses[1].result.as_deref(), Ok("中文:hello"));
        Ok(())
    }
    struct Hanging(Arc<AtomicBool>);
    impl Translator for Hanging {
        fn translate<'a>(&'a self, _: &'a Request) -> BoxFuture<'a, Result<String, String>> {
            Box::pin(async move {
                struct Guard(Arc<AtomicBool>);
                impl Drop for Guard {
                    fn drop(&mut self) {
                        self.0.store(true, Ordering::Relaxed);
                    }
                }
                let _guard = Guard(self.0.clone());
                std::future::pending().await
            })
        }
    }
    #[test]
    fn cancellation_drops_inflight_future_and_overload_is_bounded() -> anyhow::Result<()> {
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker = Translation::start(Hanging(cancelled.clone()), 9)?;
        worker.submit(request(9, 1));
        thread::sleep(Duration::from_millis(50));
        for id in 2..20 {
            worker.submit(request(9, id));
        }
        assert!(worker.dropped() > 0);
        assert_eq!(worker.shared.input.len(), 8);
        worker.stop();
        let deadline = Instant::now() + Duration::from_secs(1);
        while !cancelled.load(Ordering::Relaxed) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(cancelled.load(Ordering::Relaxed));
        Ok(())
    }
    #[test]
    fn deadline_drops_stalled_future_without_blocking_recognition() -> anyhow::Result<()> {
        let dropped = Arc::new(AtomicBool::new(false));
        let worker = Translation::start(Hanging(dropped.clone()), 9)?;
        let mut item = request(9, 1);
        item.deadline = Instant::now() + Duration::from_millis(30);
        worker.submit(item);
        let limit = Instant::now() + Duration::from_secs(1);
        let mut response = None;
        while response.is_none() && Instant::now() < limit {
            response = worker.pop();
            thread::sleep(Duration::from_millis(5));
        }
        assert!(
            response
                .is_some_and(|response| response.result.is_err_and(|error| error == "翻译超时"))
        );
        assert!(dropped.load(Ordering::Relaxed));
        Ok(())
    }
    struct FailFirst;
    impl Translator for FailFirst {
        fn translate<'a>(&'a self, request: &'a Request) -> BoxFuture<'a, Result<String, String>> {
            Box::pin(async move {
                if request.segment.key.segment_id == 1 {
                    Err("测试服务错误".into())
                } else {
                    Ok("后续译文".into())
                }
            })
        }
    }
    #[test]
    fn service_error_does_not_stop_later_segments() -> anyhow::Result<()> {
        let worker = Translation::start(FailFirst, 9)?;
        worker.submit(request(9, 1));
        worker.submit(request(9, 2));
        let limit = Instant::now() + Duration::from_secs(1);
        let mut responses = Vec::new();
        while responses.len() < 2 && Instant::now() < limit {
            if let Some(response) = worker.pop() {
                responses.push(response);
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(responses.len(), 2);
        assert!(responses[0].result.is_err());
        assert_eq!(responses[1].result.as_deref(), Ok("后续译文"));
        Ok(())
    }
}
