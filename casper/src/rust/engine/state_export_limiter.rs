use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use comm::rust::peer_node::PeerNode;
use tokio::sync::{Semaphore, SemaphorePermit};
use tokio::time::Instant;

pub const MAX_CONCURRENT_EXPORTS: usize = 2;
pub const MAX_PENDING_REQUESTS_PER_PEER: usize = 128;
pub const QUEUED_STREAM_BYTES_BUDGET: usize = 256 * 1024 * 1024;
pub const QUEUED_STREAM_BYTES_WAIT: Duration = Duration::from_secs(30);
const QUEUED_STREAM_BYTES_POLL: Duration = Duration::from_millis(50);

pub struct StateExportLimiter {
    exports: Semaphore,
    pending: Mutex<HashMap<PeerNode, usize>>,
    max_pending_per_peer: usize,
    queued_bytes_budget: usize,
    queued_bytes_wait: Duration,
}

pub struct PeerSlot<'a> {
    limiter: &'a StateExportLimiter,
    peer: PeerNode,
}

impl Drop for PeerSlot<'_> {
    fn drop(&mut self) {
        let mut pending = self
            .limiter
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(count) = pending.get_mut(&self.peer) {
            *count -= 1;
            if *count == 0 {
                pending.remove(&self.peer);
            }
        }
    }
}

impl Default for StateExportLimiter {
    fn default() -> Self {
        Self::new(
            MAX_CONCURRENT_EXPORTS,
            MAX_PENDING_REQUESTS_PER_PEER,
            QUEUED_STREAM_BYTES_BUDGET,
            QUEUED_STREAM_BYTES_WAIT,
        )
    }
}

impl StateExportLimiter {
    pub fn new(
        max_concurrent_exports: usize,
        max_pending_per_peer: usize,
        queued_bytes_budget: usize,
        queued_bytes_wait: Duration,
    ) -> Self {
        Self {
            exports: Semaphore::new(max_concurrent_exports),
            pending: Mutex::new(HashMap::new()),
            max_pending_per_peer,
            queued_bytes_budget,
            queued_bytes_wait,
        }
    }

    pub fn reserve(&self, peer: &PeerNode) -> Option<PeerSlot<'_>> {
        let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        let count = pending.entry(peer.clone()).or_insert(0);
        if *count >= self.max_pending_per_peer {
            return None;
        }
        *count += 1;
        Some(PeerSlot {
            limiter: self,
            peer: peer.clone(),
        })
    }

    pub async fn acquire(&self, queued_bytes: impl Fn() -> usize) -> Option<SemaphorePermit<'_>> {
        let permit = self.exports.acquire().await.ok()?;
        let deadline = Instant::now() + self.queued_bytes_wait;
        while queued_bytes() >= self.queued_bytes_budget {
            if Instant::now() >= deadline {
                return None;
            }
            tokio::time::sleep(QUEUED_STREAM_BYTES_POLL).await;
        }
        Some(permit)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use comm::rust::peer_node::{Endpoint, NodeIdentifier};
    use prost::bytes::Bytes;

    use super::*;

    fn peer(name: &'static str) -> PeerNode {
        PeerNode {
            id: NodeIdentifier {
                key: Bytes::from_static(name.as_bytes()),
            },
            endpoint: Endpoint::new(format!("{name}.invalid"), 40400, 40404),
        }
    }

    #[test]
    fn reserve_caps_pending_requests_per_peer_and_releases_on_drop() {
        let limiter = StateExportLimiter::new(1, 2, usize::MAX, Duration::ZERO);
        let a = peer("a");

        let first = limiter.reserve(&a).expect("first slot");
        let second = limiter.reserve(&a).expect("second slot");
        assert!(limiter.reserve(&a).is_none(), "third slot exceeds the cap");
        assert!(
            limiter.reserve(&peer("b")).is_some(),
            "the cap applies per peer"
        );

        drop(first);
        let third = limiter.reserve(&a).expect("a released slot is reusable");
        drop((second, third));
        assert!(limiter.pending.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn acquire_limits_concurrent_exports() {
        let limiter = StateExportLimiter::new(2, usize::MAX, usize::MAX, Duration::ZERO);
        let first = limiter.acquire(|| 0).await.expect("first permit");
        let _second = limiter.acquire(|| 0).await.expect("second permit");

        let third = limiter.acquire(|| 0);
        tokio::pin!(third);
        assert!(
            futures::poll!(third.as_mut()).is_pending(),
            "a third export waits for a permit"
        );

        drop(first);
        assert!(third.await.is_some());
    }

    #[tokio::test(start_paused = true)]
    async fn acquire_waits_while_queued_bytes_exceed_the_budget() {
        let limiter = StateExportLimiter::new(1, usize::MAX, 100, Duration::from_secs(30));
        let queued = Arc::new(AtomicUsize::new(100));

        let drained = queued.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(5)).await;
            drained.store(99, Ordering::SeqCst);
        });
        let started = Instant::now();
        assert!(limiter
            .acquire(|| queued.load(Ordering::SeqCst))
            .await
            .is_some());
        assert!(started.elapsed() >= Duration::from_secs(5));

        queued.store(100, Ordering::SeqCst);
        assert!(
            limiter
                .acquire(|| queued.load(Ordering::SeqCst))
                .await
                .is_none(),
            "the request is dropped when the queue does not drain in time"
        );
    }
}
