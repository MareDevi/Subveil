//! Capacity-one, coalescing notification without retaining subtitle payloads.
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Waker},
};

#[derive(Default)]
struct State {
    pending: AtomicBool,
    waker: Mutex<Option<Waker>>,
}

/// A single-consumer wake signal. Clones may notify from background workers.
#[derive(Clone, Default)]
pub struct Signal(Arc<State>);
impl Signal {
    /// Coalesce repeated notifications; never enqueue audio or text.
    pub fn notify(&self) {
        self.0.pending.store(true, Ordering::Release);
        let waker = self.0.waker.lock().ok().and_then(|mut slot| slot.take());
        if let Some(waker) = waker {
            waker.wake();
        }
    }
    /// Wait on an async executor. The consumer must drain its bounded queues.
    pub fn wait(&self) -> impl Future<Output = ()> + '_ {
        Waiting(self)
    }
}
struct Waiting<'a>(&'a Signal);
impl Future for Waiting<'_> {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let state = &self.0.0;
        if state.pending.swap(false, Ordering::AcqRel) {
            return Poll::Ready(());
        }
        if let Ok(mut slot) = state.waker.lock() {
            *slot = Some(cx.waker().clone());
            if state.pending.swap(false, Ordering::AcqRel) {
                slot.take();
                return Poll::Ready(());
            }
        }
        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::task::Wake;
    struct Counter(std::sync::atomic::AtomicUsize);
    impl Wake for Counter {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    #[test]
    fn notifications_coalesce_and_wake_registered_consumer() {
        let signal = Signal::default();
        let count = Arc::new(Counter(std::sync::atomic::AtomicUsize::new(0)));
        let waker = Waker::from(count.clone());
        let mut cx = Context::from_waker(&waker);
        let mut wait = std::pin::pin!(signal.wait());
        assert!(wait.as_mut().poll(&mut cx).is_pending());
        signal.notify();
        signal.notify();
        assert_eq!(count.0.load(Ordering::Relaxed), 1);
        assert!(wait.as_mut().poll(&mut cx).is_ready());
        let mut next = std::pin::pin!(signal.wait());
        assert!(next.as_mut().poll(&mut cx).is_pending());
    }
}
