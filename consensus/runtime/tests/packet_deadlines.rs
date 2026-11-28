use std::time::Duration;

use consensus_api::{
    AdapterContext, AdmittedRequest, Capabilities, ConsensusAdapter, ConsensusError, NetworkPacket,
    PacketRequest, Peer, Phase, ProtocolDescriptor,
};
use consensus_runtime::{RuntimeBuilder, RuntimeConfig};
use tokio::sync::{mpsc, oneshot};
use tokio::time::{advance, Instant};

const TIMEOUT: Duration = Duration::from_secs(10);

fn packet() -> NetworkPacket {
    NetworkPacket {
        peer: Peer {
            id: vec![1],
            host: "localhost".into(),
            tcp_port: 1,
            udp_port: 2,
        },
        kind: "test".into(),
        payload: vec![1],
    }
}

fn builder() -> RuntimeBuilder {
    RuntimeBuilder::new(
        ProtocolDescriptor {
            id: "packet-test".into(),
            version: 1,
            capabilities: Capabilities::NONE,
        },
        RuntimeConfig {
            request_timeout: TIMEOUT,
            ..RuntimeConfig::default()
        },
    )
    .unwrap()
}

struct HoldPackets(mpsc::UnboundedSender<AdmittedRequest<PacketRequest>>);

#[async_trait::async_trait]
impl ConsensusAdapter for HoldPackets {
    async fn run(self: Box<Self>, mut context: AdapterContext) -> Result<(), ConsensusError> {
        context.control.ready();
        loop {
            tokio::select! {
                _ = context.control.cancelled() => return Ok(()),
                Some(request) = context.packets.recv() => self.0.send(request).unwrap(),
            }
        }
    }
}

#[tokio::test(start_paused = true)]
async fn packet_expiry_is_checked_before_the_caller_observes_timeout() {
    let builder = builder();
    let handle = builder.handle();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let runtime = builder.build(Box::new(HoldPackets(tx))).start();
    handle.wait_ready().await.unwrap();
    let expected = Instant::now() + TIMEOUT;
    let mut call = Box::pin(handle.handle_packet(packet()));
    assert!(futures::poll!(&mut call).is_pending());
    let (request, _budget) = rx.recv().await.unwrap().into_parts();
    assert_eq!(request.deadline, expected);
    advance(TIMEOUT).await;
    assert!(!request.reply.is_closed());
    assert!(request.try_start().is_none());
    assert_eq!(call.await, Err(ConsensusError::DeadlineExceeded));
    assert_eq!(handle.status().phase, Phase::Ready);
    runtime.shutdown().await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn abandoned_packet_is_skipped_and_live_packet_completes() {
    let builder = builder();
    let handle = builder.handle();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let runtime = builder.build(Box::new(HoldPackets(tx))).start();
    handle.wait_ready().await.unwrap();
    let mut abandoned = Box::pin(handle.handle_packet(packet()));
    assert!(futures::poll!(&mut abandoned).is_pending());
    let (request, _budget) = rx.recv().await.unwrap().into_parts();
    drop(abandoned);
    assert!(request.try_start().is_none());
    let mut live = Box::pin(handle.handle_packet(packet()));
    assert!(futures::poll!(&mut live).is_pending());
    let (request, _live_budget) = rx.recv().await.unwrap().into_parts();
    request.try_start().unwrap().reply.send(Ok(())).unwrap();
    assert_eq!(live.await, Ok(()));
    runtime.shutdown().await.unwrap();
}

struct StartedPacket {
    started: oneshot::Sender<()>,
    release: oneshot::Receiver<()>,
    finished: oneshot::Sender<bool>,
}

#[async_trait::async_trait]
impl ConsensusAdapter for StartedPacket {
    async fn run(self: Box<Self>, mut context: AdapterContext) -> Result<(), ConsensusError> {
        context.control.ready();
        let (request, _budget) = context.packets.recv().await.unwrap().into_parts();
        let request = request.try_start().unwrap();
        self.started.send(()).unwrap();
        self.release.await.unwrap();
        self.finished
            .send(request.reply.send(Ok(())).is_err())
            .unwrap();
        context.control.cancelled().await;
        Ok(())
    }
}

#[tokio::test(start_paused = true)]
async fn started_packet_can_complete_after_caller_timeout() {
    let builder = builder();
    let handle = builder.handle();
    let (started, ready) = oneshot::channel();
    let (release, released) = oneshot::channel();
    let (finished, done) = oneshot::channel();
    let runtime = builder
        .build(Box::new(StartedPacket {
            started,
            release: released,
            finished,
        }))
        .start();
    handle.wait_ready().await.unwrap();
    let mut call = Box::pin(handle.handle_packet(packet()));
    assert!(futures::poll!(&mut call).is_pending());
    ready.await.unwrap();
    assert_eq!(call.await, Err(ConsensusError::DeadlineExceeded));
    release.send(()).unwrap();
    assert!(done.await.unwrap());
    runtime.shutdown().await.unwrap();
}
