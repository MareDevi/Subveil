//! Revisable text translation with bounded queues, deadlines and session cancellation.
#![deny(missing_docs)]
mod discovery;
mod http;
pub use discovery::text_models;
mod scheduler;
pub use scheduler::StreamingTranslationScheduler;
mod context;
mod local;
use crossbeam_queue::ArrayQueue;
use futures_util::future::BoxFuture;
pub use http::{HttpTranslator, Provider, configured_destination};
pub use local::{Compute, Device, LocalSnapshot, devices};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
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
    /// Whether this request translates a stable Partial prefix rather than a Final.
    pub provisional: bool,
    /// Exact source identity.
    pub segment: TranscriptSegment,
    /// Target language code.
    pub target: String,
    /// Previous source sentences; never future text.
    pub context: String,
    /// Optional user-owned terminology, bounded to 4096 bytes.
    pub glossary: String,
    /// Monotonic deadline including queue wait and retries.
    pub deadline: Instant,
}
impl Request {
    /// Build a Final request. Mark stable Partial requests provisional before submission.
    pub fn new(segment: TranscriptSegment, target: String) -> Self {
        Self {
            provisional: false,
            segment,
            target,
            context: String::new(),
            glossary: String::new(),
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
    /// Whether this response belongs to a provisional Partial request.
    pub provisional: bool,
    /// Source revision.
    pub key: SegmentKey,
    /// Bounded translated text or sanitized error.
    pub result: Result<String, String>,
}
struct Shared {
    wake: subveil_core::signal::Signal,
    stop: AtomicBool,
    closed: AtomicBool,
    input: ArrayQueue<Request>,
    preview: ArrayQueue<Request>,
    context: Mutex<context::SourceContext>,
    output: ArrayQueue<Response>,
    dropped: AtomicU64,
    latest: Mutex<VecDeque<(SegmentKey, bool)>>,
}
impl Shared {
    fn next(&self) -> Option<Request> {
        self.input.pop().or_else(|| self.preview.pop())
    }
    fn preempted(&self, request: &Request) -> bool {
        self.superseded(request) || (request.provisional && !self.input.is_empty())
    }
    fn superseded(&self, request: &Request) -> bool {
        self.latest
            .lock()
            .map(|latest| {
                !latest.iter().any(|(key, provisional)| {
                    *key == request.segment.key && *provisional == request.provisional
                })
            })
            .unwrap_or(true)
    }
    fn new() -> Self {
        Self {
            wake: subveil_core::signal::Signal::default(),
            stop: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            input: ArrayQueue::new(8),
            preview: ArrayQueue::new(1),
            context: Mutex::new(context::SourceContext::default()),
            output: ArrayQueue::new(64),
            dropped: AtomicU64::new(0),
            latest: Mutex::new(VecDeque::new()),
        }
    }
}
/// One session's independent translation worker. Drop cancels without blocking GUI.
pub struct Translation {
    shared: Arc<Shared>,
    local: Option<Arc<Mutex<LocalSnapshot>>>,
}
impl Translation {
    /// A capacity-one notification for background results and readiness changes.
    pub fn signal(&self) -> subveil_core::signal::Signal {
        self.shared.wake.clone()
    }
    /// Start a backend for one immutable session identity.
    pub fn start(backend: impl Translator, session: u64) -> anyhow::Result<Self> {
        let shared = Arc::new(Shared::new());
        let state = shared.clone();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        thread::Builder::new()
            .name("subveil-translate".into())
            .spawn(move || runtime.block_on(run(backend, session, &state)))?;
        Ok(Self {
            shared,
            local: None,
        })
    }
    /// Submit an accepted revision. Evict oldest waiting request under overload and report its failure.
    pub fn submit(&self, mut request: Request) {
        if self.shared.stop.load(Ordering::Relaxed) {
            return;
        }
        if let Ok(mut latest) = self.shared.latest.lock() {
            latest.retain(|(key, _)| {
                key.session_id != request.segment.key.session_id
                    || key.segment_id != request.segment.key.segment_id
            });
            latest.push_back((request.segment.key, request.provisional));
            while latest.len() > 64 {
                latest.pop_front();
            }
        }
        if self.shared.closed.load(Ordering::Acquire) {
            emit(
                &self.shared,
                Response {
                    provisional: request.provisional,
                    key: request.segment.key,
                    result: Err("本地翻译进程已停止；请重新开始识别".into()),
                },
            );
            return;
        }
        if let Ok(mut context) = self.shared.context.lock() {
            request.context = context.before(&request);
            context.observe(&request);
        }
        let queue = if request.provisional {
            &self.shared.preview
        } else {
            &self.shared.input
        };
        if let Some(old) = queue.force_push(request) {
            self.shared.dropped.fetch_add(1, Ordering::Relaxed);
            emit(
                &self.shared,
                Response {
                    provisional: old.provisional,
                    key: old.segment.key,
                    result: Err("翻译队列已满；已丢弃旧请求".into()),
                },
            );
        }
        // Close may race the push. Whichever side drains first reports each key once.
        if self.shared.closed.load(Ordering::Acquire) {
            fail_pending(&self.shared);
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
    shared.wake.notify();
}
fn fail_pending(shared: &Shared) {
    while let Some(request) = shared.next() {
        if !shared.stop.load(Ordering::Relaxed) {
            emit(
                shared,
                Response {
                    provisional: request.provisional,
                    key: request.segment.key,
                    result: Err("本地翻译进程已停止；请重新开始识别".into()),
                },
            );
        }
    }
}
async fn run(backend: impl Translator, session: u64, shared: &Shared) {
    while !shared.stop.load(Ordering::Relaxed) {
        let Some(request) = shared.next() else {
            tokio::time::sleep(Duration::from_millis(10)).await;
            continue;
        };
        if request.segment.key.session_id != session || shared.superseded(&request) {
            continue;
        }
        let result = if request.segment.text.is_empty()
            || request.segment.text.len() > 16_384
            || request.glossary.len() > 4096
            || !matches!(request.target.as_str(), "zh" | "en" | "ja")
        {
            Err("翻译请求文本或目标语种无效".into())
        } else if Instant::now() >= request.deadline {
            Err("翻译请求已过期".into())
        } else {
            let future = backend.translate(&request);
            tokio::pin!(future);
            loop {
                tokio::select! {
                    result = &mut future => break result,
                    _ = tokio::time::sleep_until(request.deadline.into()) => break Err("翻译超时".into()),
                    _ = tokio::time::sleep(Duration::from_millis(10)) => {
                        if shared.stop.load(Ordering::Relaxed) { return; }
                        if shared.preempted(&request) { break Err("翻译修订已被替代".into()); }
                    }
                }
            }
        };
        if shared.stop.load(Ordering::Relaxed) {
            return;
        }
        if shared.superseded(&request) {
            continue;
        }
        let result = result.and_then(|text| {
            if text.trim().is_empty() || text.len() > 16_384 {
                Err("译文为空或超过长度限制".into())
            } else {
                Ok(text)
            }
        });
        // Source context is captured at admission, before queue coalescing, so
        // failed translations do not erase preceding authoritative source text.
        emit(
            shared,
            Response {
                provisional: request.provisional,
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
    #[test]
    fn preview_overload_cannot_evict_waiting_final() {
        let worker = Translation {
            shared: Arc::new(Shared::new()),
            local: None,
        };
        worker.submit(request(9, 1));
        for id in 2..20 {
            let mut item = request(9, id);
            item.provisional = true;
            worker.submit(item);
        }
        assert_eq!(worker.shared.input.len(), 1);
        assert_eq!(worker.shared.preview.len(), 1);
        assert!(
            worker
                .shared
                .next()
                .is_some_and(|r| !r.provisional && r.segment.key.segment_id == 1)
        );
    }
    #[test]
    fn queued_partials_coalesce_and_final_supersedes_same_revision() -> anyhow::Result<()> {
        let shared = Arc::new(Shared::new());
        let worker = Translation {
            shared: shared.clone(),
            local: None,
        };
        let mut partial = request(9, 1);
        partial.provisional = true;
        worker.submit(partial.clone());
        let mut final_request = partial.clone();
        final_request.provisional = false;
        worker.submit(final_request.clone());
        assert!(shared.superseded(&partial));
        assert!(!shared.superseded(&final_request));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        runtime.block_on(async {
            tokio::select! {
                _ = run(Echo, 9, &shared) => {},
                _ = tokio::time::sleep(Duration::from_millis(180)) => {},
            }
        });
        let response = worker.pop();
        assert!(response.is_some_and(|r| !r.provisional && r.result.is_ok()));
        assert!(worker.pop().is_none());
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
