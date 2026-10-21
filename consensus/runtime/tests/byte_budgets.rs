use std::time::Duration;

use consensus_api::{
    AdapterContext, AdmittedRequest, Capabilities, ConsensusAdapter, ConsensusCommand,
    ConsensusError, NetworkPacket, PacketRequest, Peer, ProtocolDescriptor,
};
use consensus_runtime::{ConsensusRuntime, RuntimeBuilder, RuntimeConfig};
use tokio::sync::mpsc;

type Command = AdmittedRequest<ConsensusCommand>;
type Packet = AdmittedRequest<PacketRequest>;

struct Hold {
    commands: mpsc::UnboundedSender<Command>,
    packets: mpsc::UnboundedSender<Packet>,
}

#[async_trait::async_trait]
impl ConsensusAdapter for Hold {
    async fn run(self: Box<Self>, mut context: AdapterContext) -> Result<(), ConsensusError> {
        context.control.ready();
        loop {
            tokio::select! {
                _ = context.control.cancelled() => return Ok(()),
                Some(request) = context.commands.recv() => self.commands.send(request).unwrap(),
                Some(request) = context.packets.recv() => self.packets.send(request).unwrap(),
            }
        }
    }
}

fn descriptor() -> ProtocolDescriptor {
    ProtocolDescriptor {
        id: "budget-test".into(),
        version: 1,
        capabilities: Capabilities::SUBMIT.union(Capabilities::PROPOSE),
    }
}

fn config() -> RuntimeConfig {
    RuntimeConfig {
        command_byte_capacity: 8,
        packet_byte_capacity: 8,
        max_payload_bytes: 8,
        request_timeout: Duration::from_secs(10),
        ..RuntimeConfig::default()
    }
}

async fn setup(
    config: RuntimeConfig,
) -> (
    ConsensusRuntime,
    mpsc::UnboundedReceiver<Command>,
    mpsc::UnboundedReceiver<Packet>,
) {
    let (commands, command_rx) = mpsc::unbounded_channel();
    let (packets, packet_rx) = mpsc::unbounded_channel();
    let runtime = RuntimeBuilder::new(descriptor(), config)
        .unwrap()
        .build(Box::new(Hold { commands, packets }))
        .start();
    runtime.handle().wait_ready().await.unwrap();
    (runtime, command_rx, packet_rx)
}

fn packet(payload: Vec<u8>) -> NetworkPacket {
    NetworkPacket {
        peer: Peer {
            id: vec![],
            host: String::new(),
            tcp_port: 1,
            udp_port: 2,
        },
        kind: String::new(),
        payload,
    }
}

#[tokio::test(start_paused = true)]
async fn active_requests_retain_separate_budgets_after_caller_timeout() {
    let (runtime, mut commands, mut packets) = setup(config()).await;
    let handle = runtime.handle();
    let clone = handle.clone();
    let mut command_call = Box::pin(handle.submit(vec![0; 8]));
    assert!(futures::poll!(&mut command_call).is_pending());
    let (command, command_permit) = commands.recv().await.unwrap().into_parts();
    let command = command.try_start().unwrap();
    assert_eq!(clone.submit(vec![0]).await, Err(ConsensusError::QueueFull));
    let mut packet_call = Box::pin(handle.handle_packet(packet(vec![0; 8])));
    assert!(futures::poll!(&mut packet_call).is_pending());
    let (request, packet_permit) = packets.recv().await.unwrap().into_parts();
    let request = request.try_start().unwrap();
    assert_eq!(
        clone.handle_packet(packet(vec![0])).await,
        Err(ConsensusError::QueueFull)
    );
    assert_eq!(command_call.await, Err(ConsensusError::DeadlineExceeded));
    assert_eq!(packet_call.await, Err(ConsensusError::DeadlineExceeded));
    assert_eq!(clone.submit(vec![0]).await, Err(ConsensusError::QueueFull));
    assert_eq!(
        clone.handle_packet(packet(vec![0])).await,
        Err(ConsensusError::QueueFull)
    );
    drop((command, command_permit, request, packet_permit));
    let mut next = Box::pin(handle.submit(vec![0; 8]));
    assert!(futures::poll!(&mut next).is_pending());
    let (command, permit) = commands.recv().await.unwrap().into_parts();
    if let ConsensusCommand::Submit { reply, .. } = command {
        reply.send(Ok("accepted".into())).unwrap();
    } else {
        panic!("expected submit")
    }
    drop(permit);
    assert_eq!(next.await, Ok("accepted".into()));
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn count_rejection_returns_the_byte_reservation() {
    let (runtime, mut commands, _) = setup(RuntimeConfig {
        command_capacity: 1,
        ..config()
    })
    .await;
    let handle = runtime.handle();
    let mut first = Box::pin(handle.submit(vec![0]));
    assert!(futures::poll!(&mut first).is_pending());
    assert_eq!(
        handle.submit(vec![0; 7]).await,
        Err(ConsensusError::QueueFull)
    );
    let held = commands.recv().await.unwrap();
    let mut second = Box::pin(handle.submit(vec![0; 7]));
    assert!(futures::poll!(&mut second).is_pending());
    drop(commands.recv().await.unwrap());
    drop(held);
    drop((first, second));
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn buffer_capacity_and_packet_metadata_are_charged() {
    let (runtime, _, mut packets) = setup(config()).await;
    let handle = runtime.handle();
    let mut oversized_allocation = Vec::with_capacity(9);
    oversized_allocation.push(1);
    assert_eq!(
        handle.submit(oversized_allocation).await,
        Err(ConsensusError::QueueFull)
    );
    let mut request = packet(vec![0; 8]);
    request.peer.host = "x".into();
    assert_eq!(
        handle.handle_packet(request).await,
        Err(ConsensusError::QueueFull)
    );
    let mut exact = packet(vec![0; 5]);
    exact.peer.id = vec![1];
    exact.peer.host = "x".into();
    exact.kind = "x".into();
    let mut call = Box::pin(handle.handle_packet(exact));
    assert!(futures::poll!(&mut call).is_pending());
    drop(packets.recv().await.unwrap());
    drop(call);
    runtime.shutdown().await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn expired_abandoned_panicking_and_aborted_requests_release_permits() {
    let (runtime, mut commands, _) = setup(config()).await;
    let handle = runtime.handle();
    for mode in 0..4 {
        let mut call = Box::pin(handle.submit(vec![0; 8]));
        assert!(futures::poll!(&mut call).is_pending());
        let admitted = commands.recv().await.unwrap();
        match mode {
            0 => {
                tokio::time::advance(Duration::from_secs(10)).await;
                let (command, permit) = admitted.into_parts();
                assert!(command.try_start().is_none());
                drop(permit);
            }
            1 => {
                drop(call);
                drop(admitted);
                continue;
            }
            2 => {
                let task = tokio::spawn(async move {
                    let (_command, _permit) = admitted.into_parts();
                    panic!("request panic");
                });
                assert!(task.await.unwrap_err().is_panic());
            }
            _ => {
                let (started, ready) = tokio::sync::oneshot::channel();
                let task = tokio::spawn(async move {
                    let (_command, _permit) = admitted.into_parts();
                    started.send(()).unwrap();
                    std::future::pending::<()>().await;
                });
                ready.await.unwrap();
                task.abort();
                assert!(task.await.unwrap_err().is_cancelled());
            }
        }
        drop(call);
    }
    let mut final_call = Box::pin(handle.submit(vec![0; 8]));
    assert!(futures::poll!(&mut final_call).is_pending());
    drop(commands.recv().await.unwrap());
    drop(final_call);
    runtime.shutdown().await.unwrap();
}

#[test]
fn byte_budgets_are_validated() {
    let defaults = RuntimeConfig::default();
    assert_eq!(defaults.command_byte_capacity, 64 * 1024 * 1024);
    assert_eq!(defaults.packet_byte_capacity, 256 * 1024 * 1024);
    for (commands, packets) in [
        (0, 8),
        (8, 0),
        (7, 8),
        (8, 7),
        (usize::MAX, 8),
        (8, usize::MAX),
    ] {
        let result = RuntimeBuilder::new(descriptor(), RuntimeConfig {
            command_byte_capacity: commands,
            packet_byte_capacity: packets,
            ..config()
        });
        assert!(matches!(result, Err(ConsensusError::InvalidInput(_))));
    }
}
