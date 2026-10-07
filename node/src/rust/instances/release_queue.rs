use std::collections::VecDeque;
use std::sync::Mutex;

use tokio::sync::Notify;

pub struct ReleaseQueue<T> {
    state: Mutex<Lanes<T>>,
    available: Notify,
    gossip_share_after: usize,
    gossip_capacity: usize,
}

struct Lanes<T> {
    released: VecDeque<T>,
    gossip: VecDeque<T>,
    released_streak: usize,
}

impl<T> ReleaseQueue<T> {
    pub fn new(gossip_share_after: usize, gossip_capacity: usize) -> Self {
        Self {
            state: Mutex::new(Lanes {
                released: VecDeque::new(),
                gossip: VecDeque::new(),
                released_streak: 0,
            }),
            available: Notify::new(),
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

    pub fn push_released(&self, item: T) {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .released
            .push_back(item);
        self.available.notify_one();
    }

    pub async fn pop(&self) -> Option<T> {
        loop {
            if let Some(item) = self.take() {
                return Some(item);
            }
            self.available.notified().await;
        }
    }

    fn take(&self) -> Option<T> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let gossip_turn =
            state.released_streak >= self.gossip_share_after && !state.gossip.is_empty();
        if !gossip_turn {
            if let Some(item) = state.released.pop_front() {
                state.released_streak += 1;
                return Some(item);
            }
        }
        let item = state.gossip.pop_front();
        if item.is_some() {
            state.released_streak = 0;
        }
        item
    }
}
