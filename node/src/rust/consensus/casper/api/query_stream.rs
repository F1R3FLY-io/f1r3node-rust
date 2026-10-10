use std::future::Future;
use std::pin::Pin;

use futures::{Stream, StreamExt};

pub type QueryStream<T> = Pin<Box<dyn Stream<Item = Result<T, tonic::Status>> + Send>>;

pub(super) fn query_stream<T: Send + 'static>(
    query: impl Future<Output = Result<Vec<T>, tonic::Status>> + Send + 'static,
) -> QueryStream<T> {
    Box::pin(futures::stream::once(query).flat_map(|result| {
        futures::stream::iter(match result {
            Ok(items) => items.into_iter().map(Ok).collect(),
            Err(error) => vec![Err(error)],
        })
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stream_preserves_query_results_and_errors() {
        let items: Vec<_> = query_stream(async { Ok(vec![1, 2, 3]) }).collect().await;
        assert_eq!(
            items.into_iter().map(Result::unwrap).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        let mut stream = query_stream::<u8>(async { Err(tonic::Status::internal("query failed")) });
        assert_eq!(
            stream.next().await.unwrap().unwrap_err().message(),
            "query failed"
        );
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn dropping_the_stream_cancels_the_in_flight_query() {
        let (owned, mut released) = tokio::sync::oneshot::channel::<()>();
        let mut stream = query_stream::<u8>(async move {
            let _owned = owned;
            std::future::pending().await
        });
        assert!(futures::poll!(stream.next()).is_pending());
        assert!(matches!(
            released.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        drop(stream);
        assert!(released.await.is_err());
    }
}
