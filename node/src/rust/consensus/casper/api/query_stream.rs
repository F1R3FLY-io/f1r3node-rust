use std::future::Future;
use std::pin::Pin;

use futures::{Stream, StreamExt};

pub type QueryStream<T> = Pin<Box<dyn Stream<Item = Result<T, tonic::Status>> + Send>>;

pub(super) fn query_stream<T: Send + 'static, U: Send + 'static>(
    query: impl Future<Output = Result<Vec<T>, tonic::Status>> + Send + 'static,
    mut convert: impl FnMut(T) -> U + Send + 'static,
) -> QueryStream<U> {
    Box::pin(
        futures::stream::once(query)
            .flat_map(|result| match result {
                Ok(items) => futures::stream::iter(items.into_iter().map(Ok)).left_stream(),
                Err(error) => futures::stream::once(std::future::ready(Err(error))).right_stream(),
            })
            .map(move |item| item.map(&mut convert)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stream_preserves_query_results_and_errors() {
        let items: Vec<_> = query_stream(async { Ok(vec![1, 2, 3]) }, |item| item * 2)
            .collect()
            .await;
        assert_eq!(
            items.into_iter().map(Result::unwrap).collect::<Vec<_>>(),
            vec![2, 4, 6]
        );
        let mut stream = query_stream::<u8, u8>(
            async { Err(tonic::Status::internal("query failed")) },
            |_| panic!("failed queries must not convert items"),
        );
        assert_eq!(
            stream.next().await.unwrap().unwrap_err().message(),
            "query failed"
        );
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn dropping_the_stream_cancels_the_in_flight_query() {
        let (owned, mut released) = tokio::sync::oneshot::channel::<()>();
        let mut stream = query_stream::<u8, u8>(
            async move {
                let _owned = owned;
                std::future::pending().await
            },
            std::convert::identity,
        );
        assert!(futures::poll!(stream.next()).is_pending());
        assert!(matches!(
            released.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        drop(stream);
        assert!(released.await.is_err());
    }

    #[tokio::test]
    async fn empty_query_ends_without_an_item() {
        let mut stream = query_stream::<u8, u8>(async { Ok(Vec::new()) }, std::convert::identity);
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn dropping_a_partially_read_stream_releases_remaining_items() {
        struct Item(tokio::sync::oneshot::Sender<()>);
        let (first, first_rx) = tokio::sync::oneshot::channel();
        let (second, second_rx) = tokio::sync::oneshot::channel();
        let mut stream = query_stream(
            async { Ok(vec![Item(first), Item(second)]) },
            std::convert::identity,
        );
        let item = stream.next().await.unwrap().unwrap();
        item.0.send(()).unwrap();
        first_rx.await.unwrap();
        drop(stream);
        assert!(second_rx.await.is_err());
    }

    #[tokio::test]
    async fn conversion_follows_demand_and_drop_releases_unconverted_items() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        struct Item(usize, Arc<AtomicUsize>);
        impl Drop for Item {
            fn drop(&mut self) { self.1.fetch_add(1, Ordering::SeqCst); }
        }

        let converted = Arc::new(AtomicUsize::new(0));
        let dropped = Arc::new(AtomicUsize::new(0));
        let items = (0..3).map(|i| Item(i, dropped.clone())).collect();
        let conversions = converted.clone();
        let mut stream = query_stream(async { Ok(items) }, move |item: Item| {
            conversions.fetch_add(1, Ordering::SeqCst);
            item.0
        });
        assert_eq!(converted.load(Ordering::SeqCst), 0);
        assert_eq!(stream.next().await.unwrap().unwrap(), 0);
        assert_eq!(converted.load(Ordering::SeqCst), 1);
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        drop(stream);
        assert_eq!(converted.load(Ordering::SeqCst), 1);
        assert_eq!(dropped.load(Ordering::SeqCst), 3);
    }
}
