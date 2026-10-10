use std::time::Duration;

use consensus_api::{
    AdapterContext, Capabilities, ConsensusAdapter, ConsensusCommand, ConsensusError, Phase,
    ProtocolDescriptor,
};
use consensus_runtime::{RuntimeBuilder, RuntimeConfig};
use tokio::sync::{mpsc, oneshot};
use tokio::time::{advance, Instant};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

fn builder() -> RuntimeBuilder {
    RuntimeBuilder::new(
        ProtocolDescriptor {
            id: "deadline-test".into(),
            version: 1,
            capabilities: Capabilities::SUBMIT
                .union(Capabilities::PROPOSE)
                .union(Capabilities::FINALIZED_PROGRESS),
        },
        RuntimeConfig {
            command_capacity: 3,
            request_timeout: REQUEST_TIMEOUT,
            ..RuntimeConfig::default()
        },
    )
    .unwrap()
}

struct HoldCommands(mpsc::UnboundedSender<ConsensusCommand>);

#[async_trait::async_trait]
impl ConsensusAdapter for HoldCommands {
    async fn run(self: Box<Self>, mut context: AdapterContext) -> Result<(), ConsensusError> {
        context.control.ready();
        loop {
            tokio::select! {
                _ = context.control.cancelled() => return Ok(()),
                Some(command) = context.commands.recv() => self.0.send(command).unwrap(),
            }
        }
    }
}

#[tokio::test(start_paused = true)]
async fn all_commands_expire_even_before_the_caller_observes_its_timeout() {
    let builder = builder();
    let handle = builder.handle();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let runtime = builder.build(Box::new(HoldCommands(tx))).start();
    handle.wait_ready().await.unwrap();
    let expected_deadline = Instant::now() + REQUEST_TIMEOUT;
    let mut submit = Box::pin(handle.submit(vec![1]));
    let mut propose = Box::pin(handle.propose(false));
    let mut finalized = Box::pin(handle.finalized());
    assert!(futures::poll!(&mut submit).is_pending());
    assert!(futures::poll!(&mut propose).is_pending());
    assert!(futures::poll!(&mut finalized).is_pending());
    let commands = [
        rx.recv().await.unwrap(),
        rx.recv().await.unwrap(),
        rx.recv().await.unwrap(),
    ];
    advance(REQUEST_TIMEOUT).await;
    for command in commands {
        let (deadline, closed) = match &command {
            ConsensusCommand::Submit {
                deadline, reply, ..
            }
            | ConsensusCommand::Propose {
                deadline, reply, ..
            } => (*deadline, reply.is_closed()),
            ConsensusCommand::Finalized { deadline, reply } => (*deadline, reply.is_closed()),
        };
        assert_eq!(deadline, expected_deadline);
        assert!(!closed, "the caller has not polled its timeout yet");
        assert!(command.try_start().is_none());
    }
    assert_eq!(submit.await, Err(ConsensusError::DeadlineExceeded));
    assert_eq!(propose.await, Err(ConsensusError::DeadlineExceeded));
    assert_eq!(finalized.await, Err(ConsensusError::DeadlineExceeded));
    assert_eq!(handle.status().phase, Phase::Ready);
    runtime.shutdown().await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn abandoned_commands_are_skipped_before_the_deadline() {
    let builder = builder();
    let handle = builder.handle();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let runtime = builder.build(Box::new(HoldCommands(tx))).start();
    handle.wait_ready().await.unwrap();
    let started_at = Instant::now();
    let mut submit = Box::pin(handle.submit(vec![1]));
    let mut propose = Box::pin(handle.propose(false));
    let mut finalized = Box::pin(handle.finalized());
    assert!(futures::poll!(&mut submit).is_pending());
    assert!(futures::poll!(&mut propose).is_pending());
    assert!(futures::poll!(&mut finalized).is_pending());
    let commands = [
        rx.recv().await.unwrap(),
        rx.recv().await.unwrap(),
        rx.recv().await.unwrap(),
    ];
    drop((submit, propose, finalized));
    assert_eq!(Instant::now(), started_at);
    for command in commands {
        assert!(command.try_start().is_none());
    }
    assert_eq!(handle.status().phase, Phase::Ready);
    runtime.shutdown().await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn expired_commands_reply_with_deadline_errors() {
    let deadline = Instant::now();
    let (reply, rx) = oneshot::channel();
    assert!(ConsensusCommand::Submit {
        payload: vec![1],
        deadline,
        reply
    }
    .try_start()
    .is_none());
    assert_eq!(rx.await.unwrap(), Err(ConsensusError::DeadlineExceeded));
    let (reply, rx) = oneshot::channel();
    assert!(ConsensusCommand::Propose {
        is_async: false,
        deadline,
        reply
    }
    .try_start()
    .is_none());
    assert_eq!(rx.await.unwrap(), Err(ConsensusError::DeadlineExceeded));
    let (reply, rx) = oneshot::channel();
    assert!(ConsensusCommand::Finalized { deadline, reply }
        .try_start()
        .is_none());
    assert_eq!(rx.await.unwrap(), Err(ConsensusError::DeadlineExceeded));
}

struct StartedWrite {
    started: oneshot::Sender<()>,
    release: oneshot::Receiver<()>,
    completed: oneshot::Sender<bool>,
}

#[async_trait::async_trait]
impl ConsensusAdapter for StartedWrite {
    async fn run(self: Box<Self>, mut context: AdapterContext) -> Result<(), ConsensusError> {
        let Self {
            started,
            release,
            completed,
        } = *self;
        context.control.ready();
        let command = context.commands.recv().await.unwrap().try_start().unwrap();
        started.send(()).unwrap();
        release.await.unwrap();
        let reply = match command {
            ConsensusCommand::Submit { reply, .. } | ConsensusCommand::Propose { reply, .. } => {
                reply
            }
            ConsensusCommand::Finalized { .. } => panic!("expected a write command"),
        };
        completed
            .send(reply.send(Ok("write completed".into())).is_err())
            .unwrap();
        context.control.cancelled().await;
        Ok(())
    }
}

#[tokio::test(start_paused = true)]
async fn writes_started_before_the_deadline_can_complete_after_caller_timeout() {
    for submit in [true, false] {
        let builder = builder();
        let handle = builder.handle();
        let (started, start_rx) = oneshot::channel();
        let (release_tx, release) = oneshot::channel();
        let (completed, completion_rx) = oneshot::channel();
        let runtime = builder
            .build(Box::new(StartedWrite {
                started,
                release,
                completed,
            }))
            .start();
        handle.wait_ready().await.unwrap();
        let request = async {
            if submit {
                handle.submit(vec![1]).await
            } else {
                handle.propose(false).await
            }
        };
        tokio::pin!(request);
        assert!(futures::poll!(&mut request).is_pending());
        start_rx.await.unwrap();
        assert_eq!(request.await, Err(ConsensusError::DeadlineExceeded));
        release_tx.send(()).unwrap();
        assert!(
            completion_rx.await.unwrap(),
            "the write completed without a reply receiver"
        );
        assert_eq!(handle.status().phase, Phase::Ready);
        runtime.shutdown().await.unwrap();
    }
}
