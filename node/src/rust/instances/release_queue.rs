use std::collections::VecDeque;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use casper::rust::metrics_constants::BLOCK_PROCESSOR_METRICS_SOURCE;
use tokio::sync::{Notify, Semaphore};

pub const RELEASE_QUEUE_WAIT_METRIC: &str = "block-processing.release.queue-wait.time";

pub struct ReleaseQueue<T> {
    state: Mutex<Lanes<T>>,
    available: Notify,
    room: Notify,
    gossip_share_after: usize,
    gossip_capacity: usize,
}

struct Lanes<T> {
    released: VecDeque<(Instant, T)>,
    gossip: VecDeque<T>,
    released_streak: usize,
    closed: bool,
}

impl<T> ReleaseQueue<T> {
    pub fn new(gossip_share_after: usize, gossip_capacity: usize) -> Self {
        Self {
            state: Mutex::new(Lanes {
                released: VecDeque::new(),
                gossip: VecDeque::new(),
                released_streak: 0,
                closed: false,
            }),
            available: Notify::new(),
            room: Notify::new(),
            gossip_share_after,
            gossip_capacity,
        }
    }

    pub fn push_gossip(&self, item: T) -> Result<(), T> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.gossip.len() >= self.gossip_capacity {
            return Err(item);
        }
        state.gossip.push_back(item);
        drop(state);
        self.available.notify_one();
        Ok(())
    }

    pub async fn push_gossip_wait(&self, mut item: T) {
        loop {
            let room = self.room.notified();
            match self.push_gossip(item) {
                Ok(()) => return,
                Err(rejected) => item = rejected,
            }
            room.await;
        }
    }

    pub fn close(&self) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).closed = true;
        self.available.notify_one();
    }

    pub fn push_released(&self, item: T) {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .released
            .push_back((Instant::now(), item));
        self.available.notify_one();
    }

    pub async fn pop(&self) -> Option<T> {
        loop {
            let available = self.available.notified();
            match self.take() {
                Taken::Item(item) => return Some(item),
                Taken::Closed => return None,
                Taken::Empty => available.await,
            }
        }
    }

    fn take(&self) -> Taken<T> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let gossip_turn =
            state.released_streak >= self.gossip_share_after && !state.gossip.is_empty();
        if !gossip_turn {
            if let Some((released_at, item)) = state.released.pop_front() {
                state.released_streak += 1;
                metrics::histogram!(RELEASE_QUEUE_WAIT_METRIC, "source" => BLOCK_PROCESSOR_METRICS_SOURCE)
                    .record(released_at.elapsed().as_secs_f64());
                return Taken::Item(item);
            }
        }
        match state.gossip.pop_front() {
            Some(item) => {
                state.released_streak = 0;
                drop(state);
                self.room.notify_one();
                Taken::Item(item)
            }
            None if state.closed => Taken::Closed,
            None => Taken::Empty,
        }
    }
}

enum Taken<T> {
    Item(T),
    Empty,
    Closed,
}

pub async fn run_scheduler<T, C, P, PFut, R, RFut>(
    queue: Arc<ReleaseQueue<T>>,
    permits: usize,
    process: P,
    release: R,
) where
    T: Send + 'static,
    C: Send + 'static,
    P: Fn(T) -> PFut + Send + Sync + 'static,
    PFut: Future<Output = C> + Send + 'static,
    R: Fn(C) -> RFut + Send + Sync + 'static,
    RFut: Future<Output = Vec<T>> + Send + 'static,
{
    let semaphore = Arc::new(Semaphore::new(permits));
    let process = Arc::new(process);
    let release = Arc::new(release);
    loop {
        let permit = match semaphore.clone().acquire_owned().await {
            Ok(permit) => permit,
            Err(_) => return,
        };
        let Some(item) = queue.pop().await else {
            return;
        };
        let queue = queue.clone();
        let process = process.clone();
        let release = release.clone();
        tokio::spawn(async move {
            let context = process(item).await;
            drop(permit);
            for released in release(context).await {
                queue.push_released(released);
            }
        });
    }
}
