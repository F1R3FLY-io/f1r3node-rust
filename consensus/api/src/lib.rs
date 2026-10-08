use std::time::Duration;

use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::Instant;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectId(pub Vec<u8>);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Capabilities(u64);

impl Capabilities {
    pub const NONE: Self = Self(0);
    pub const SUBMIT: Self = Self(1);
    pub const PROPOSE: Self = Self(2);
    pub const FINALIZED_PROGRESS: Self = Self(4);

    pub const fn union(self, other: Self) -> Self { Self(self.0 | other.0) }

    pub const fn contains(self, other: Self) -> bool { self.0 & other.0 == other.0 }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolDescriptor {
    pub id: String,
    pub version: u32,
    pub capabilities: Capabilities,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Prepared,
    Bootstrapping,
    Ready,
    Draining,
    Stopped,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsensusStatus {
    pub protocol: ProtocolDescriptor,
    pub phase: Phase,
    pub error: Option<ConsensusError>,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ConsensusError {
    #[error("Unsupported consensus capability: {0}")]
    UnsupportedCapability(&'static str),
    #[error("Consensus is not ready")]
    NotReady,
    #[error("Consensus has stopped accepting work")]
    Stopped,
    #[error("Consensus queue is full")]
    QueueFull,
    #[error("Consensus request deadline expired")]
    DeadlineExceeded,
    #[error("Invalid consensus input: {0}")]
    InvalidInput(String),
    #[error("{0}")]
    Protocol(String),
    #[error("{message}")]
    Rejected {
        code: &'static str,
        message: String,
        detail: Vec<u8>,
    },
    #[error("Consensus task {task} failed: {reason}")]
    TaskFailed { task: String, reason: String },
    #[error("Consensus shutdown deadline expired")]
    ShutdownTimeout,
}

#[derive(Clone, Debug)]
pub struct Peer {
    pub id: Vec<u8>,
    pub host: String,
    pub tcp_port: u32,
    pub udp_port: u32,
}

#[derive(Clone, Debug)]
pub struct NetworkPacket {
    pub peer: Peer,
    pub kind: String,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdmissionOutcome {
    Accepted(ObjectId),
    Duplicate(ObjectId),
    Deferred {
        object: ObjectId,
        missing: Vec<ObjectId>,
    },
    Rejected {
        object: Option<ObjectId>,
        reason: String,
    },
}

pub enum ConsensusCommand {
    Submit {
        payload: Vec<u8>,
        deadline: Instant,
        reply: oneshot::Sender<Result<String, ConsensusError>>,
    },
    Propose {
        is_async: bool,
        deadline: Instant,
        reply: oneshot::Sender<Result<String, ConsensusError>>,
    },
    Finalized {
        deadline: Instant,
        reply: oneshot::Sender<Result<ObjectId, ConsensusError>>,
    },
}

impl ConsensusCommand {
    #[must_use]
    pub fn try_start(self) -> Option<Self> {
        let (deadline, closed) = match &self {
            Self::Submit {
                deadline, reply, ..
            }
            | Self::Propose {
                deadline, reply, ..
            } => (*deadline, reply.is_closed()),
            Self::Finalized { deadline, reply } => (*deadline, reply.is_closed()),
        };
        if closed {
            return None;
        }
        if Instant::now() >= deadline {
            match self {
                Self::Submit { reply, .. } | Self::Propose { reply, .. } => {
                    let _ = reply.send(Err(ConsensusError::DeadlineExceeded));
                }
                Self::Finalized { reply, .. } => {
                    let _ = reply.send(Err(ConsensusError::DeadlineExceeded));
                }
            }
            return None;
        }
        Some(self)
    }
}

pub struct PacketRequest {
    pub packet: NetworkPacket,
    pub reply: oneshot::Sender<Result<(), ConsensusError>>,
}

#[derive(Clone)]
pub struct RuntimeControl {
    status: watch::Sender<ConsensusStatus>,
    shutdown: watch::Receiver<bool>,
}

impl RuntimeControl {
    pub fn new(status: watch::Sender<ConsensusStatus>, shutdown: watch::Receiver<bool>) -> Self {
        Self { status, shutdown }
    }

    pub fn status(&self) -> ConsensusStatus { self.status.borrow().clone() }

    pub fn ready(&self) {
        self.status.send_if_modified(|status| {
            if status.phase == Phase::Bootstrapping && !*self.shutdown.borrow() {
                status.phase = Phase::Ready;
                true
            } else {
                false
            }
        });
    }

    pub fn draining(&self) {
        self.status.send_if_modified(|status| {
            if matches!(status.phase, Phase::Bootstrapping | Phase::Ready) {
                status.phase = Phase::Draining;
                true
            } else {
                false
            }
        });
    }

    pub async fn cancelled(&mut self) {
        let _ = self.shutdown.wait_for(|stopping| *stopping).await;
    }

    pub fn is_cancelled(&self) -> bool { *self.shutdown.borrow() }
}

pub struct AdapterContext {
    pub commands: mpsc::Receiver<ConsensusCommand>,
    pub packets: mpsc::Receiver<PacketRequest>,
    pub control: RuntimeControl,
    pub drain_timeout: Duration,
}

#[async_trait::async_trait]
pub trait ConsensusAdapter: Send + 'static {
    async fn run(self: Box<Self>, context: AdapterContext) -> Result<(), ConsensusError>;
}
