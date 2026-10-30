use std::future::Future;

use tokio::runtime::{Handle, RuntimeFlavor};

pub async fn off_async_workers<F: Future>(work: F) -> F::Output {
    match Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(move || handle.block_on(work))
        }
        _ => work.await,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use super::*;

    #[test]
    fn heavy_work_leaves_the_worker_pool_free_for_other_tasks() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let released = Arc::new(AtomicBool::new(false));
            let observed = released.clone();
            let heavy = tokio::spawn(async move {
                off_async_workers(async move {
                    let deadline = std::time::Instant::now() + Duration::from_secs(5);
                    while !observed.load(Ordering::SeqCst) {
                        assert!(
                            std::time::Instant::now() < deadline,
                            "the heartbeat never ran"
                        );
                        std::thread::sleep(Duration::from_millis(5));
                    }
                })
                .await
            });
            tokio::time::sleep(Duration::from_millis(20)).await;
            let heartbeat = tokio::spawn(async move { released.store(true, Ordering::SeqCst) });
            heartbeat.await.unwrap();
            heavy.await.unwrap();
        });
    }

    #[test]
    fn nested_offloads_and_spawned_subtasks_complete() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let total = runtime.block_on(async {
            tokio::spawn(async {
                off_async_workers(async {
                    let inner =
                        off_async_workers(async { tokio::spawn(async { 20 }).await.unwrap() })
                            .await;
                    tokio::time::sleep(Duration::from_millis(1)).await;
                    inner + 1
                })
                .await
            })
            .await
            .unwrap()
        });
        assert_eq!(total, 21);
    }

    #[tokio::test]
    async fn a_current_thread_runtime_runs_the_work_inline() {
        assert_eq!(off_async_workers(async { 7 }).await, 7);
    }
}
