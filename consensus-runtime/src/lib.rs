use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::time::Duration;

use consensus_api::{
    AdapterContext, Capabilities, ConsensusAdapter, ConsensusCommand, ConsensusError,
    ConsensusStatus, NetworkPacket, ObjectId, PacketRequest, Phase, ProtocolDescriptor,
    RuntimeControl,
};
use futures::FutureExt;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::{JoinHandle, JoinSet};

#[derive(Clone, Debug)]
pub struct RuntimeConfig {
    pub command_capacity: usize,
    pub packet_capacity: usize,
    pub max_payload_bytes: usize,
    pub request_timeout: Duration,
    pub drain_timeout: Duration,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            command_capacity: 128,
            packet_capacity: 256,
            max_payload_bytes: 16 * 1024 * 1024,
            request_timeout: Duration::from_secs(300),
            drain_timeout: Duration::from_secs(30),
        }
    }
}

#[derive(Clone)]
pub struct ConsensusHandle {
    commands: mpsc::Sender<ConsensusCommand>,
    packets: mpsc::Sender<PacketRequest>,
    status: watch::Receiver<ConsensusStatus>,
    shutdown: watch::Sender<bool>,
    config: RuntimeConfig,
}

impl ConsensusHandle {
    pub fn status(&self) -> ConsensusStatus { self.status.borrow().clone() }

    pub fn subscribe(&self) -> watch::Receiver<ConsensusStatus> { self.status.clone() }

    pub fn request_shutdown(&self) { self.shutdown.send_replace(true); }

    fn check(
        &self,
        capability: Option<(Capabilities, &'static str)>,
    ) -> Result<(), ConsensusError> {
        let status = self.status();
        if *self.shutdown.borrow()
            || matches!(
                status.phase,
                Phase::Draining | Phase::Stopped | Phase::Failed
            )
        {
            return Err(ConsensusError::Stopped);
        }
        if let Some((required, name)) = capability {
            if !status.protocol.capabilities.contains(required) {
                return Err(ConsensusError::UnsupportedCapability(name));
            }
            if status.phase != Phase::Ready {
                return Err(ConsensusError::NotReady);
            }
        }
        Ok(())
    }

    fn check_payload(&self, payload: &[u8]) -> Result<(), ConsensusError> {
        if payload.len() > self.config.max_payload_bytes {
            return Err(ConsensusError::InvalidInput(
                "payload limit exceeded".into(),
            ));
        }
        Ok(())
    }

    async fn response<T>(
        &self,
        rx: oneshot::Receiver<Result<T, ConsensusError>>,
    ) -> Result<T, ConsensusError> {
        tokio::time::timeout(self.config.request_timeout, rx)
            .await
            .map_err(|_| ConsensusError::DeadlineExceeded)?
            .map_err(|_| ConsensusError::Stopped)?
    }

    pub async fn submit(&self, payload: Vec<u8>) -> Result<String, ConsensusError> {
        self.check(Some((Capabilities::SUBMIT, "submit")))?;
        self.check_payload(&payload)?;
        let (reply, rx) = oneshot::channel();
        self.commands
            .try_send(ConsensusCommand::Submit { payload, reply })
            .map_err(queue_error)?;
        self.response(rx).await
    }

    pub async fn propose(&self, is_async: bool) -> Result<String, ConsensusError> {
        self.check(Some((Capabilities::PROPOSE, "propose")))?;
        let (reply, rx) = oneshot::channel();
        self.commands
            .try_send(ConsensusCommand::Propose { is_async, reply })
            .map_err(queue_error)?;
        self.response(rx).await
    }

    pub async fn finalized(&self) -> Result<ObjectId, ConsensusError> {
        self.check(Some((
            Capabilities::FINALIZED_PROGRESS,
            "finalized progress",
        )))?;
        let (reply, rx) = oneshot::channel();
        self.commands
            .try_send(ConsensusCommand::Finalized { reply })
            .map_err(queue_error)?;
        self.response(rx).await
    }

    pub async fn handle_packet(&self, packet: NetworkPacket) -> Result<(), ConsensusError> {
        self.check(None)?;
        self.check_payload(&packet.payload)?;
        if packet.kind.len() > 256 || packet.peer.id.len() > 1024 || packet.peer.host.len() > 1024 {
            return Err(ConsensusError::InvalidInput(
                "packet metadata limit exceeded".into(),
            ));
        }
        let (reply, rx) = oneshot::channel();
        self.packets
            .try_send(PacketRequest { packet, reply })
            .map_err(queue_error)?;
        self.response(rx).await
    }

    pub async fn wait_ready(&self) -> Result<(), ConsensusError> {
        let mut status = self.subscribe();
        tokio::time::timeout(self.config.request_timeout, async {
            loop {
                let current = status.borrow_and_update().clone();
                match current.phase {
                    Phase::Ready => return Ok(()),
                    Phase::Failed => return Err(current.error.unwrap_or(ConsensusError::Stopped)),
                    Phase::Stopped | Phase::Draining => return Err(ConsensusError::Stopped),
                    _ => {}
                }
                status
                    .changed()
                    .await
                    .map_err(|_| ConsensusError::Stopped)?;
            }
        })
        .await
        .map_err(|_| ConsensusError::DeadlineExceeded)?
    }
}

fn queue_error<T>(error: mpsc::error::TrySendError<T>) -> ConsensusError {
    match error {
        mpsc::error::TrySendError::Full(_) => ConsensusError::QueueFull,
        mpsc::error::TrySendError::Closed(_) => ConsensusError::Stopped,
    }
}

pub struct RuntimeBuilder {
    handle: ConsensusHandle,
    context: AdapterContext,
    status: watch::Sender<ConsensusStatus>,
}

impl RuntimeBuilder {
    pub fn new(
        protocol: ProtocolDescriptor,
        config: RuntimeConfig,
    ) -> Result<Self, ConsensusError> {
        if config.command_capacity == 0
            || config.packet_capacity == 0
            || config.max_payload_bytes == 0
            || config.request_timeout.is_zero()
            || config.drain_timeout.is_zero()
        {
            return Err(ConsensusError::InvalidInput(
                "runtime limits must be positive".into(),
            ));
        }
        let (commands, command_rx) = mpsc::channel(config.command_capacity);
        let (packets, packet_rx) = mpsc::channel(config.packet_capacity);
        let (status, status_rx) = watch::channel(ConsensusStatus {
            protocol,
            phase: Phase::Prepared,
            error: None,
        });
        let (shutdown, shutdown_rx) = watch::channel(false);
        let context = AdapterContext {
            commands: command_rx,
            packets: packet_rx,
            control: RuntimeControl::new(status.clone(), shutdown_rx),
            drain_timeout: config.drain_timeout,
        };
        Ok(Self {
            handle: ConsensusHandle {
                commands,
                packets,
                status: status_rx,
                shutdown,
                config,
            },
            context,
            status,
        })
    }

    pub fn handle(&self) -> ConsensusHandle { self.handle.clone() }

    pub fn build(self, adapter: Box<dyn ConsensusAdapter>) -> PreparedConsensus {
        PreparedConsensus {
            builder: self,
            adapter,
        }
    }
}

pub struct PreparedConsensus {
    builder: RuntimeBuilder,
    adapter: Box<dyn ConsensusAdapter>,
}

impl PreparedConsensus {
    pub fn handle(&self) -> ConsensusHandle { self.builder.handle() }

    pub fn start(self) -> ConsensusRuntime {
        let RuntimeBuilder {
            handle,
            context,
            status,
        } = self.builder;
        let mut shutdown = handle.shutdown.subscribe();
        let shutdown_state = handle.shutdown.subscribe();
        let drain_timeout = handle.config.drain_timeout;
        status.send_modify(|status| status.phase = Phase::Bootstrapping);
        let terminal_status = status.clone();
        let supervisor = tokio::spawn(async move {
            let work = AssertUnwindSafe(self.adapter.run(context)).catch_unwind();
            tokio::pin!(work);
            let result = tokio::select! {
                result = &mut work => flatten_result(result, *shutdown_state.borrow()),
                _ = async { let _ = shutdown.wait_for(|stopping| *stopping).await; } => {
                    status.send_modify(|status| status.phase = Phase::Draining);
                    match tokio::time::timeout(drain_timeout, &mut work).await {
                        Ok(result) => flatten_result(result, true),
                        Err(_) => Err(ConsensusError::ShutdownTimeout),
                    }
                }
            };
            status.send_modify(|status| {
                status.phase = if result.is_ok() {
                    Phase::Stopped
                } else {
                    Phase::Failed
                };
                status.error = result.as_ref().err().cloned();
            });
            result
        });
        ConsensusRuntime {
            handle,
            supervisor,
            result: None,
            terminal_status,
        }
    }
}

fn flatten_result(
    result: Result<Result<(), ConsensusError>, Box<dyn std::any::Any + Send>>,
    stopping: bool,
) -> Result<(), ConsensusError> {
    match result {
        Ok(Ok(())) if stopping => Ok(()),
        Ok(Ok(())) => Err(ConsensusError::TaskFailed {
            task: "adapter".into(),
            reason: "completed unexpectedly".into(),
        }),
        Ok(Err(error)) => Err(error),
        Err(_) => Err(ConsensusError::TaskFailed {
            task: "adapter".into(),
            reason: "panicked".into(),
        }),
    }
}

pub struct ConsensusRuntime {
    handle: ConsensusHandle,
    supervisor: JoinHandle<Result<(), ConsensusError>>,
    result: Option<Result<(), ConsensusError>>,
    terminal_status: watch::Sender<ConsensusStatus>,
}

impl ConsensusRuntime {
    pub fn handle(&self) -> ConsensusHandle { self.handle.clone() }

    pub async fn wait(&mut self) -> Result<(), ConsensusError> {
        if let Some(result) = &self.result {
            return result.clone();
        }
        let result = (&mut self.supervisor)
            .await
            .map_err(|error| ConsensusError::TaskFailed {
                task: "supervisor".into(),
                reason: error.to_string(),
            })?;
        self.result = Some(result.clone());
        result
    }

    pub async fn shutdown(mut self) -> Result<(), ConsensusError> {
        self.handle.request_shutdown();
        self.wait().await
    }
}

impl Drop for ConsensusRuntime {
    fn drop(&mut self) {
        self.handle.request_shutdown();
        self.supervisor.abort();
        self.terminal_status.send_if_modified(|status| {
            if matches!(status.phase, Phase::Stopped | Phase::Failed) {
                return false;
            }
            status.phase = Phase::Failed;
            status.error = Some(ConsensusError::TaskFailed {
                task: "runtime".into(),
                reason: "dropped without completed shutdown".into(),
            });
            true
        });
    }
}

#[derive(Default)]
pub struct TaskGroup {
    tasks: JoinSet<(String, Result<(), ConsensusError>)>,
}

impl TaskGroup {
    pub fn spawn(
        &mut self,
        name: impl Into<String>,
        future: impl Future<Output = Result<(), ConsensusError>> + Send + 'static,
    ) {
        let name = name.into();
        self.tasks.spawn(async move { (name, future.await) });
    }

    pub fn is_empty(&self) -> bool { self.tasks.is_empty() }
    pub fn len(&self) -> usize { self.tasks.len() }

    pub async fn join_next(&mut self) -> Result<String, ConsensusError> {
        match self.tasks.join_next().await {
            Some(Ok((name, Ok(())))) => Ok(name),
            Some(Ok((task, Err(error)))) => Err(ConsensusError::TaskFailed {
                task,
                reason: error.to_string(),
            }),
            Some(Err(error)) => Err(ConsensusError::TaskFailed {
                task: "child".into(),
                reason: error.to_string(),
            }),
            None => std::future::pending().await,
        }
    }

    pub async fn shutdown(&mut self) { self.tasks.shutdown().await; }
}
pub struct TaskScope {
    tasks: std::sync::Mutex<JoinSet<(String, Result<(), ConsensusError>)>>,
    changed: tokio::sync::Notify,
}

impl Default for TaskScope {
    fn default() -> Self {
        Self {
            tasks: std::sync::Mutex::new(JoinSet::new()),
            changed: tokio::sync::Notify::new(),
        }
    }
}

impl TaskScope {
    pub fn spawn(
        &self,
        name: impl Into<String>,
        task: impl Future<Output = Result<(), ConsensusError>> + Send + 'static,
    ) {
        let name = name.into();
        self.tasks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .spawn(async move { (name, task.await) });
        self.changed.notify_one();
    }

    pub async fn join_next(&self) -> Result<String, ConsensusError> {
        loop {
            tokio::select! {
                _ = self.changed.notified() => {},
                outcome = futures::future::poll_fn(|cx| {
                    match self.tasks.lock().unwrap_or_else(|e| e.into_inner()).poll_join_next(cx) {
                        std::task::Poll::Ready(None) => std::task::Poll::Pending,
                        outcome => outcome,
                    }
                }) => return match outcome {
                    Some(Ok((name, Ok(())))) => Ok(name),
                    Some(Ok((task, Err(error)))) => Err(ConsensusError::TaskFailed { task, reason: error.to_string() }),
                    Some(Err(error)) => Err(ConsensusError::TaskFailed { task: "native child".into(), reason: error.to_string() }),
                    None => unreachable!(),
                },
            }
        }
    }

    pub async fn shutdown(&self) {
        let mut tasks = std::mem::take(&mut *self.tasks.lock().unwrap_or_else(|e| e.into_inner()));
        tasks.shutdown().await;
    }

    pub fn abort(&self) {
        self.tasks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .abort_all();
    }
}
