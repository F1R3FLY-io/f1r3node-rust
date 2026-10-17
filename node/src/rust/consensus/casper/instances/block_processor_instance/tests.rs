use std::time::Duration;

use casper::rust::blocks::block_processor::InFlightBlockGuard;
use tokio::sync::{watch, Semaphore};
use tokio::time::timeout;

use super::*;

fn guarded_block(id: u8, blocks: &Arc<InFlightBlocks>) -> (u8, InFlightBlockGuard) {
    match mark_in_flight(blocks, vec![id].into()) {
        InFlightMark::Marked(guard) => (id, guard),
        other => panic!("could not mark block: {other:?}"),
    }
}

#[tokio::test]
async fn panic_stops_admission_and_releases_active_and_queued_blocks() {
    timeout(Duration::from_secs(5), async {
        let blocks = Arc::new(InFlightBlocks::new());
        let (tx, rx) = mpsc::channel(8);
        for id in 0..8 {
            tx.send(guarded_block(id, &blocks)).await.unwrap();
        }
        let (_shutdown_tx, shutdown_rx) = watch::channel(false);
        let (started_tx, mut started_rx) = mpsc::unbounded_channel();
        let panic_gate = Arc::new(Semaphore::new(0));
        let gate = panic_gate.clone();
        let processor = tokio::spawn(run_block_tasks(
            rx,
            shutdown_rx,
            move |(id, guard)| {
                let started_tx = started_tx.clone();
                let gate = gate.clone();
                async move {
                    let _guard = guard;
                    started_tx.send(id).unwrap();
                    if id == 1 {
                        let _permit = gate.acquire().await.unwrap();
                        panic!("block B panicked");
                    }
                    std::future::pending::<()>().await;
                }
            },
            |()| std::future::ready(Vec::new()),
        ));
        let mut started = vec![
            started_rx.recv().await.unwrap(),
            started_rx.recv().await.unwrap(),
        ];
        started.sort();
        assert_eq!(started, vec![0, 1]);
        panic_gate.add_permits(1);
        let error = processor.await.unwrap().unwrap_err();
        assert!(error.to_string().contains("block B panicked"), "{error}");
        assert!(tx.is_closed());
        assert!(
            blocks.is_empty(),
            "all in-flight guards must be released before return"
        );
        assert_eq!(started_rx.recv().await, None, "queued block must not start");
    })
    .await
    .expect("processor did not report the panic");
}

#[tokio::test]
async fn panic_while_draining_is_reported() {
    timeout(Duration::from_secs(5), async {
        let blocks = Arc::new(InFlightBlocks::new());
        let (tx, rx) = mpsc::channel(1);
        tx.send(guarded_block(0, &blocks)).await.unwrap();
        drop(tx);
        let (_shutdown_tx, shutdown_rx) = watch::channel(false);
        let panic_gate = Arc::new(Semaphore::new(0));
        let gate = panic_gate.clone();
        let mut processor = Box::pin(run_block_tasks(
            rx,
            shutdown_rx,
            move |(_, guard)| {
                let gate = gate.clone();
                async move {
                    let _guard = guard;
                    let _permit = gate.acquire().await.unwrap();
                    panic!("panic during drain");
                }
            },
            |()| std::future::ready(Vec::new()),
        ));
        assert!(futures::poll!(&mut processor).is_pending());
        panic_gate.add_permits(1);
        let error = processor.await.unwrap_err();
        assert!(error.to_string().contains("panic during drain"), "{error}");
        assert!(blocks.is_empty());
    })
    .await
    .expect("processor did not finish draining");
}

#[tokio::test]
async fn shutdown_drains_accepted_blocks_with_bounded_parallelism() {
    timeout(Duration::from_secs(5), async {
        let blocks = Arc::new(InFlightBlocks::new());
        let (tx, rx) = mpsc::channel(8);
        for id in 0..8 {
            tx.send(guarded_block(id, &blocks)).await.unwrap();
        }
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let (started_tx, mut started_rx) = mpsc::unbounded_channel();
        let gate = Arc::new(Semaphore::new(0));
        let task_gate = gate.clone();
        let mut processor = Box::pin(run_block_tasks(
            rx,
            shutdown_rx,
            move |(id, guard)| {
                let gate = task_gate.clone();
                let started_tx = started_tx.clone();
                async move {
                    let _guard = guard;
                    started_tx.send(id).unwrap();
                    gate.acquire().await.unwrap().forget();
                }
            },
            |()| std::future::ready(Vec::new()),
        ));
        assert!(futures::poll!(&mut processor).is_pending());
        let mut started = vec![
            started_rx.recv().await.unwrap(),
            started_rx.recv().await.unwrap(),
        ];
        assert!(started_rx.try_recv().is_err());
        shutdown_tx.send(true).unwrap();
        assert!(futures::poll!(&mut processor).is_pending());
        assert!(tx.is_closed());
        gate.add_permits(8);
        processor.await.unwrap();
        while let Some(id) = started_rx.recv().await {
            started.push(id);
        }
        started.sort();
        assert_eq!(started, (0..8).collect::<Vec<_>>());
        assert!(blocks.is_empty());
    })
    .await
    .expect("processor did not drain accepted blocks");
}

#[tokio::test]
async fn panic_during_release_scan_is_reported_and_releases_guards() {
    timeout(Duration::from_secs(5), async {
        let blocks = Arc::new(InFlightBlocks::new());
        let (tx, rx) = mpsc::channel(1);
        tx.send(guarded_block(0, &blocks)).await.unwrap();
        let (_shutdown_tx, shutdown_rx) = watch::channel(false);
        let error = run_block_tasks(
            rx,
            shutdown_rx,
            std::future::ready,
            |(_, guard)| async move {
                let _guard = guard;
                panic!("release scan panicked");
            },
        )
        .await
        .unwrap_err();
        assert!(
            error.to_string().contains("release scan panicked"),
            "{error}"
        );
        assert!(tx.is_closed());
        assert!(blocks.is_empty());
    })
    .await
    .expect("processor did not report the scan panic");
}

#[tokio::test]
async fn closed_input_drains_blocks_released_by_an_active_scan() {
    timeout(Duration::from_secs(5), async {
        let blocks = Arc::new(InFlightBlocks::new());
        let (tx, rx) = mpsc::channel(1);
        tx.send(guarded_block(0, &blocks)).await.unwrap();
        drop(tx);
        let (_shutdown_tx, shutdown_rx) = watch::channel(false);
        let (started_tx, mut started_rx) = mpsc::unbounded_channel();
        let gate = Arc::new(Semaphore::new(0));
        let scan_gate = gate.clone();
        let scan_blocks = blocks.clone();
        let processor = tokio::spawn(run_block_tasks(
            rx,
            shutdown_rx,
            move |(id, guard)| {
                started_tx.send(id).unwrap();
                drop(guard);
                std::future::ready(id)
            },
            move |id| {
                let blocks = scan_blocks.clone();
                let gate = scan_gate.clone();
                async move {
                    if id == 0 {
                        gate.acquire().await.unwrap().forget();
                        vec![guarded_block(1, &blocks)]
                    } else {
                        Vec::new()
                    }
                }
            },
        ));
        assert_eq!(started_rx.recv().await, Some(0));
        assert!(!processor.is_finished());
        gate.add_permits(1);
        processor.await.unwrap().unwrap();
        assert_eq!(started_rx.recv().await, Some(1));
        assert_eq!(started_rx.recv().await, None);
        assert!(blocks.is_empty());
    })
    .await
    .expect("processor did not drain released blocks");
}
