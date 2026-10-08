use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use consensus_api::{
    AdapterContext, Capabilities, ConsensusAdapter, ConsensusCommand, ConsensusError,
    NetworkPacket, Peer, Phase, ProtocolDescriptor,
};
use consensus_runtime::{RuntimeBuilder, RuntimeConfig, TaskGroup};

fn builder(capabilities: Capabilities) -> RuntimeBuilder {
    RuntimeBuilder::new(
        ProtocolDescriptor {
            id: "test".into(),
            version: 1,
            capabilities,
        },
        RuntimeConfig {
            command_capacity: 1,
            packet_capacity: 1,
            max_payload_bytes: 8,
            request_timeout: Duration::from_millis(100),
            drain_timeout: Duration::from_millis(100),
            cleanup_timeout: Duration::from_millis(100),
        },
    )
    .unwrap()
}

struct Echo;

#[async_trait::async_trait]
impl ConsensusAdapter for Echo {
    async fn run(self: Box<Self>, mut context: AdapterContext) -> Result<(), ConsensusError> {
        context.control.ready();
        loop {
            tokio::select! {
                _ = context.control.cancelled() => return Ok(()),
                Some(command) = context.commands.recv() => match command.try_start() {
                    Some(ConsensusCommand::Submit { payload, reply, .. }) => { let _ = reply.send(Ok(String::from_utf8(payload).unwrap())); }
                    Some(ConsensusCommand::Propose { reply, .. }) => { let _ = reply.send(Ok("proposal".into())); }
                    Some(ConsensusCommand::Finalized { reply, .. }) => { let _ = reply.send(Ok(consensus_api::ObjectId(vec![1]))); }
                    None => {}
                },
                Some(request) = context.packets.recv() => { let _ = request.reply.send(Ok(())); }
            }
        }
    }
}

#[tokio::test]
async fn requests_cross_the_adapter_and_shutdown_closes_admission() {
    let builder = builder(
        Capabilities::SUBMIT
            .union(Capabilities::PROPOSE)
            .union(Capabilities::FINALIZED_PROGRESS),
    );
    let handle = builder.handle();
    assert_eq!(handle.status().phase, Phase::Prepared);
    assert_eq!(handle.propose(false).await, Err(ConsensusError::NotReady));
    let runtime = builder.build(Box::new(Echo)).start();
    handle.wait_ready().await.unwrap();
    assert_eq!(handle.submit(b"deploy".to_vec()).await.unwrap(), "deploy");
    assert_eq!(handle.propose(false).await.unwrap(), "proposal");
    assert_eq!(
        handle.finalized().await.unwrap(),
        consensus_api::ObjectId(vec![1])
    );
    runtime.shutdown().await.unwrap();
    assert_eq!(handle.status().phase, Phase::Stopped);
    assert_eq!(handle.submit(vec![]).await, Err(ConsensusError::Stopped));
}

#[tokio::test]
async fn unsupported_operations_never_enter_the_adapter() {
    let builder = builder(Capabilities::NONE);
    let handle = builder.handle();
    let runtime = builder.build(Box::new(Echo)).start();
    handle.wait_ready().await.unwrap();
    assert_eq!(
        handle.propose(false).await,
        Err(ConsensusError::UnsupportedCapability("propose"))
    );
    assert_eq!(
        handle.finalized().await,
        Err(ConsensusError::UnsupportedCapability("finalized progress"))
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn oversized_payload_is_rejected() {
    let builder = builder(Capabilities::SUBMIT);
    let handle = builder.handle();
    let runtime = builder.build(Box::new(Echo)).start();
    handle.wait_ready().await.unwrap();
    assert!(matches!(
        handle.submit(vec![0; 9]).await,
        Err(ConsensusError::InvalidInput(_))
    ));
    runtime.shutdown().await.unwrap();
}

struct Blocked;

#[async_trait::async_trait]
impl ConsensusAdapter for Blocked {
    async fn run(self: Box<Self>, mut context: AdapterContext) -> Result<(), ConsensusError> {
        context.control.ready();
        context.control.cancelled().await;
        Ok(())
    }
}

#[tokio::test]
async fn full_command_queue_does_not_block_shutdown() {
    let builder = builder(Capabilities::SUBMIT);
    let handle = builder.handle();
    let runtime = builder.build(Box::new(Blocked)).start();
    handle.wait_ready().await.unwrap();
    let request = handle.submit(vec![1]);
    tokio::pin!(request);
    assert!(futures::poll!(&mut request).is_pending());
    assert_eq!(handle.submit(vec![2]).await, Err(ConsensusError::QueueFull));
    runtime.shutdown().await.unwrap();
    assert_eq!(request.await, Err(ConsensusError::Stopped));
}

#[tokio::test]
async fn unresponsive_request_has_a_deadline() {
    let builder = builder(Capabilities::SUBMIT);
    let handle = builder.handle();
    let runtime = builder.build(Box::new(Blocked)).start();
    handle.wait_ready().await.unwrap();
    assert_eq!(
        handle.submit(vec![]).await,
        Err(ConsensusError::DeadlineExceeded)
    );
    runtime.shutdown().await.unwrap();
}

struct BootstrapFromPacket;

#[async_trait::async_trait]
impl ConsensusAdapter for BootstrapFromPacket {
    async fn run(self: Box<Self>, mut context: AdapterContext) -> Result<(), ConsensusError> {
        let request = context.packets.recv().await.unwrap();
        context.control.ready();
        let _ = request.reply.send(Ok(()));
        context.control.cancelled().await;
        Ok(())
    }
}

fn packet() -> NetworkPacket {
    NetworkPacket {
        peer: Peer {
            id: vec![1],
            host: "localhost".into(),
            tcp_port: 1,
            udp_port: 2,
        },
        kind: "join".into(),
        payload: vec![],
    }
}

#[tokio::test]
async fn bootstrap_receives_packets_before_readiness() {
    let builder = builder(Capabilities::NONE);
    let handle = builder.handle();
    let runtime = builder.build(Box::new(BootstrapFromPacket)).start();
    handle.handle_packet(packet()).await.unwrap();
    handle.wait_ready().await.unwrap();
    runtime.shutdown().await.unwrap();
}

struct PanicAdapter;

#[async_trait::async_trait]
impl ConsensusAdapter for PanicAdapter {
    async fn run(self: Box<Self>, _: AdapterContext) -> Result<(), ConsensusError> {
        panic!("initialization failed")
    }
}

#[tokio::test]
async fn panic_is_reported_as_failed_health() {
    let builder = builder(Capabilities::NONE);
    let handle = builder.handle();
    let mut runtime = builder.build(Box::new(PanicAdapter)).start();
    assert!(runtime.wait().await.is_err());
    assert_eq!(handle.status().phase, Phase::Failed);
    assert!(handle.wait_ready().await.is_err());
}

struct UnexpectedExit;

#[async_trait::async_trait]
impl ConsensusAdapter for UnexpectedExit {
    async fn run(self: Box<Self>, _: AdapterContext) -> Result<(), ConsensusError> { Ok(()) }
}

#[tokio::test]
async fn unexpected_completion_is_a_failure() {
    let mut runtime = builder(Capabilities::NONE)
        .build(Box::new(UnexpectedExit))
        .start();
    assert!(matches!(
        runtime.wait().await,
        Err(ConsensusError::TaskFailed { .. })
    ));
}

struct DropFlag(Arc<AtomicBool>);
impl Drop for DropFlag {
    fn drop(&mut self) { self.0.store(true, Ordering::SeqCst); }
}

struct StuckAdapter(Arc<AtomicBool>);

#[async_trait::async_trait]
impl ConsensusAdapter for StuckAdapter {
    async fn run(self: Box<Self>, context: AdapterContext) -> Result<(), ConsensusError> {
        let _guard = DropFlag(self.0.clone());
        context.control.ready();
        std::future::pending().await
    }
}

#[tokio::test]
async fn shutdown_deadline_cancels_stuck_adapter() {
    let dropped = Arc::new(AtomicBool::new(false));
    let runtime = builder(Capabilities::NONE)
        .build(Box::new(StuckAdapter(dropped.clone())))
        .start();
    runtime.handle().wait_ready().await.unwrap();
    assert_eq!(
        runtime.shutdown().await,
        Err(ConsensusError::ShutdownTimeout)
    );
    assert!(dropped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn task_group_reports_child_failure_and_aborts_siblings() {
    let mut group = TaskGroup::default();
    let dropped = Arc::new(AtomicBool::new(false));
    let guard = DropFlag(dropped.clone());
    group.spawn("sibling", async move {
        let _guard = guard;
        std::future::pending().await
    });
    group.spawn("failure", async {
        Err(ConsensusError::Protocol("disk failed".into()))
    });
    assert!(
        matches!(group.join_next().await, Err(ConsensusError::TaskFailed { task, .. }) if task == "failure")
    );
    group.shutdown().await;
    assert!(group.is_empty());
    assert!(dropped.load(Ordering::SeqCst));
}

#[test]
fn invalid_runtime_limits_return_an_error() {
    let config = RuntimeConfig {
        command_capacity: 0,
        ..RuntimeConfig::default()
    };
    assert!(RuntimeBuilder::new(
        ProtocolDescriptor {
            id: "test".into(),
            version: 1,
            capabilities: Capabilities::NONE
        },
        config
    )
    .is_err());
}
#[tokio::test]
async fn dropping_runtime_revokes_ready_health_and_stops_work() {
    let dropped = Arc::new(AtomicBool::new(false));
    let runtime = builder(Capabilities::NONE)
        .build(Box::new(StuckAdapter(dropped.clone())))
        .start();
    let handle = runtime.handle();
    handle.wait_ready().await.unwrap();
    drop(runtime);
    assert_eq!(handle.status().phase, Phase::Failed);
    assert_eq!(
        handle.handle_packet(packet()).await,
        Err(ConsensusError::Stopped)
    );
    tokio::task::yield_now().await;
    assert!(dropped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn dynamic_tasks_are_supervised_and_drained() {
    let scope = Arc::new(consensus_runtime::TaskScope::default());
    let waiting = scope.join_next();
    tokio::pin!(waiting);
    assert!(futures::poll!(&mut waiting).is_pending());
    scope.spawn("late task", async { Ok(()) });
    assert_eq!(waiting.await.unwrap(), "late task");
    let dropped = Arc::new(AtomicBool::new(false));
    let guard = DropFlag(dropped.clone());
    scope.spawn("pending", async move {
        let _guard = guard;
        std::future::pending().await
    });
    scope.spawn("failed", async {
        Err(ConsensusError::Protocol("failed".into()))
    });
    assert!(
        matches!(scope.join_next().await, Err(ConsensusError::TaskFailed { task, .. }) if task == "failed")
    );
    scope.shutdown().await;
    assert!(dropped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn stopped_scopes_reject_tasks_from_retained_spawners() {
    for abort in [false, true] {
        let scope = Arc::new(consensus_runtime::TaskScope::default());
        let retained = scope.clone();
        if abort {
            scope.abort();
        } else {
            scope.shutdown().await;
        }
        let dropped = Arc::new(AtomicBool::new(false));
        let guard = DropFlag(dropped.clone());
        retained.spawn("late child", async move {
            let _guard = guard;
            std::future::pending().await
        });
        assert!(dropped.load(Ordering::SeqCst));
        scope.shutdown().await;
    }
}

#[tokio::test]
async fn full_packet_queue_does_not_block_shutdown() {
    let builder = builder(Capabilities::NONE);
    let handle = builder.handle();
    let runtime = builder.build(Box::new(Blocked)).start();
    handle.wait_ready().await.unwrap();
    let request = handle.handle_packet(packet());
    tokio::pin!(request);
    assert!(futures::poll!(&mut request).is_pending());
    assert_eq!(
        handle.handle_packet(packet()).await,
        Err(ConsensusError::QueueFull)
    );
    runtime.shutdown().await.unwrap();
    assert_eq!(request.await, Err(ConsensusError::Stopped));
}

#[tokio::test]
async fn wait_is_repeatable_after_completion() {
    let mut runtime = builder(Capabilities::NONE)
        .build(Box::new(UnexpectedExit))
        .start();
    let first = runtime.wait().await;
    assert_eq!(runtime.wait().await, first);
    assert_eq!(runtime.shutdown().await, first);
}
