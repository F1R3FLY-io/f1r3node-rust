// See comm/src/main/scala/coop/rchain/comm/transport/buffer/LimitedBuffer.scala
// See comm/src/main/scala/coop/rchain/comm/transport/buffer/LimitedBufferObservable.scala
// See comm/src/main/scala/coop/rchain/comm/transport/buffer/ConcurrentQueue.scala

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use futures::Stream;

/// LimitedBuffer trait providing bounded buffering with overflow policy
pub trait LimitedBuffer<T> {
    /// Push the next element to the buffer
    /// Returns true if successfully enqueued, false if dropped due to overflow
    fn push_next(&self, elem: T) -> bool;

    /// Signal completion - no more elements will be pushed
    fn complete(&self);

    /// Check if the buffer is complete
    fn is_complete(&self) -> bool;
}

/// Observable interface for LimitedBuffer
pub trait LimitedBufferObservable<T>: LimitedBuffer<T> {
    type Subscription: Stream<Item = T> + Unpin;

    /// Subscribe to receive items from this buffer
    fn subscribe(&mut self) -> Option<Self::Subscription>;
}

/// Bounded single-consumer buffer that drops new elements when full.
///
/// Holds at most `buffer_size` elements. Completion drops the sender, so the
/// subscriber receives what is already queued and then ends.
#[derive(Debug)]
pub struct FlumeLimitedBuffer<T> {
    sender: Mutex<Option<flume::Sender<T>>>,
    receiver: flume::Receiver<T>,
    buffer_size: usize,
    subscribed: AtomicBool,
}

impl<T> FlumeLimitedBuffer<T> {
    /// Create a new FlumeLimitedBuffer with the specified buffer size
    pub fn drop_new(buffer_size: usize) -> Self {
        assert!(
            buffer_size > 0,
            "bufferSize must be a strictly positive number"
        );

        let (sender, receiver) = flume::bounded(buffer_size);
        Self {
            sender: Mutex::new(Some(sender)),
            receiver,
            buffer_size,
            subscribed: AtomicBool::new(false),
        }
    }

    /// Get the buffer size
    pub fn buffer_size(&self) -> usize { self.buffer_size }

    /// Complete the buffer and remove every element still queued.
    ///
    /// No push can succeed after this returns, so the result holds every
    /// element that the subscriber has not received.
    pub fn close_and_drain(&self) -> Vec<T> {
        self.sender.lock().expect("sender lock poisoned").take();
        self.receiver.drain().collect()
    }
}

impl<T> LimitedBuffer<T> for FlumeLimitedBuffer<T> {
    fn push_next(&self, elem: T) -> bool {
        match self.sender.lock().expect("sender lock poisoned").as_ref() {
            Some(sender) => sender.try_send(elem).is_ok(),
            None => false,
        }
    }

    fn complete(&self) { self.sender.lock().expect("sender lock poisoned").take(); }

    fn is_complete(&self) -> bool { self.sender.lock().expect("sender lock poisoned").is_none() }
}

impl<T: Send + 'static> LimitedBufferObservable<T> for FlumeLimitedBuffer<T> {
    type Subscription = flume::r#async::RecvStream<'static, T>;

    /// Returns the single subscription; later calls return `None`.
    fn subscribe(&mut self) -> Option<Self::Subscription> {
        if self.subscribed.swap(true, Ordering::AcqRel) {
            return None;
        }
        Some(self.receiver.clone().into_stream())
    }
}

/// Convenience constructor functions
impl<T> FlumeLimitedBuffer<T> {
    /// Create a new drop-new limited buffer observable
    pub fn drop_new_observable(buffer_size: usize) -> Self { Self::drop_new(buffer_size) }
}

#[cfg(test)]
mod tests {
    use futures::stream::StreamExt;

    use super::*;

    #[tokio::test]
    async fn test_limited_buffer_push_next() {
        let buffer = FlumeLimitedBuffer::<i32>::drop_new(2);

        // Should accept first two items
        assert!(buffer.push_next(1));
        assert!(buffer.push_next(2));

        // Should reject third item (buffer full)
        assert!(!buffer.push_next(3));

        assert!(!buffer.is_complete());
    }

    #[tokio::test]
    async fn test_limited_buffer_completion() {
        let buffer = FlumeLimitedBuffer::<i32>::drop_new(10);

        assert!(!buffer.is_complete());

        buffer.complete();
        assert!(buffer.is_complete());

        // Should reject new items after completion
        assert!(!buffer.push_next(1));
    }

    #[tokio::test]
    async fn test_limited_buffer_subscription() {
        let mut buffer = FlumeLimitedBuffer::<i32>::drop_new(10);

        // Push some items
        assert!(buffer.push_next(1));
        assert!(buffer.push_next(2));
        assert!(buffer.push_next(3));

        // Subscribe and read items
        let mut subscription = buffer.subscribe().expect("Should get subscription");

        let items: Vec<i32> = vec![
            subscription.next().await.unwrap(),
            subscription.next().await.unwrap(),
            subscription.next().await.unwrap(),
        ];

        assert_eq!(items, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn test_limited_buffer_completion_with_subscription() {
        let mut buffer = FlumeLimitedBuffer::<i32>::drop_new(10);

        let mut subscription = buffer.subscribe().expect("Should get subscription");

        // Push an item and complete
        assert!(buffer.push_next(42));
        buffer.complete();

        // Should receive the item
        assert_eq!(subscription.next().await, Some(42));

        // Should end the stream after completion
        assert_eq!(subscription.next().await, None);
    }

    #[test]
    fn test_buffer_size_validation() {
        // Should panic with zero buffer size
        std::panic::catch_unwind(|| {
            FlumeLimitedBuffer::<i32>::drop_new(0);
        })
        .expect_err("Should panic with zero buffer size");
    }

    #[tokio::test]
    async fn test_drop_new_behavior() {
        let mut buffer = FlumeLimitedBuffer::<String>::drop_new(2);

        // Fill buffer
        assert!(buffer.push_next("first".to_string()));
        assert!(buffer.push_next("second".to_string()));

        // These should be dropped
        assert!(!buffer.push_next("dropped1".to_string()));
        assert!(!buffer.push_next("dropped2".to_string()));

        // Subscribe and verify only first two items are present
        let mut subscription = buffer.subscribe().expect("Should get subscription");

        assert_eq!(subscription.next().await, Some("first".to_string()));
        assert_eq!(subscription.next().await, Some("second".to_string()));

        buffer.complete();

        assert_eq!(subscription.next().await, None);
    }

    #[tokio::test]
    async fn subscribed_buffer_holds_exactly_its_size_and_delivers_every_element() {
        let mut buffer = FlumeLimitedBuffer::<u32>::drop_new(4);
        let mut subscription = buffer.subscribe().expect("Should get subscription");

        let mut accepted = Vec::new();
        for i in 0..10u32 {
            if buffer.push_next(i) {
                accepted.push(i);
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(accepted, vec![0, 1, 2, 3]);

        buffer.complete();
        let delivered: Vec<u32> = subscription.by_ref().collect().await;
        assert_eq!(delivered, accepted);
    }

    #[tokio::test]
    async fn second_subscription_is_refused() {
        let mut buffer = FlumeLimitedBuffer::<i32>::drop_new(2);

        assert!(buffer.subscribe().is_some());
        assert!(buffer.subscribe().is_none());
    }

    #[tokio::test]
    async fn close_and_drain_returns_undelivered_elements_and_refuses_pushes() {
        let mut buffer = FlumeLimitedBuffer::<i32>::drop_new(4);
        let mut subscription = buffer.subscribe().expect("Should get subscription");

        assert!(buffer.push_next(1));
        assert!(buffer.push_next(2));
        assert!(buffer.push_next(3));
        assert_eq!(subscription.next().await, Some(1));

        assert_eq!(buffer.close_and_drain(), vec![2, 3]);
        assert!(buffer.is_complete());
        assert!(!buffer.push_next(4));
        assert_eq!(subscription.next().await, None);
    }
}
