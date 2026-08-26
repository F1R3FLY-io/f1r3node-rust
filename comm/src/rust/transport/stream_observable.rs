use std::sync::Arc;

use tokio::sync::{mpsc, oneshot};

use crate::rust::errors::CommError;
use crate::rust::peer_node::PeerNode;
use crate::rust::transport::activity_gate::{ActivityGate, ActivityGuard};
use crate::rust::transport::payload_budget::PayloadReservation;
use crate::rust::transport::transport_layer::Blob;

pub struct OutboundPayload {
    blob: Blob,
    reservation: PayloadReservation,
}

impl OutboundPayload {
    pub fn new(blob: Blob, reservation: PayloadReservation) -> Self { Self { blob, reservation } }

    pub fn blob(&self) -> &Blob { &self.blob }

    pub fn reserved_bytes(&self) -> usize { self.reservation.bytes() }
}

pub struct StreamObservable {
    peer: PeerNode,
    sender: mpsc::Sender<OutboundDelivery>,
    activity: Arc<ActivityGate>,
    buffer_size: usize,
}

pub struct OutboundDelivery {
    payload: Arc<OutboundPayload>,
    _activity: ActivityGuard,
    completion: Option<oneshot::Sender<Result<(), CommError>>>,
}

impl OutboundDelivery {
    fn new(
        payload: Arc<OutboundPayload>,
        activity: &Arc<ActivityGate>,
    ) -> Result<(Self, oneshot::Receiver<Result<(), CommError>>), CommError> {
        let activity = activity.try_enter().ok_or_else(|| {
            CommError::ResourceExhausted("client stream queue is retiring".to_string())
        })?;
        let (completion, receiver) = oneshot::channel();
        Ok((
            Self {
                payload,
                _activity: activity,
                completion: Some(completion),
            },
            receiver,
        ))
    }

    pub fn payload(&self) -> &Arc<OutboundPayload> { &self.payload }

    pub fn complete(mut self, result: Result<(), CommError>) {
        if let Some(completion) = self.completion.take() {
            let _ = completion.send(result);
        }
    }
}

impl std::fmt::Debug for StreamObservable {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StreamObservable")
            .field("peer", &self.peer)
            .field("buffer_size", &self.buffer_size)
            .field("available_capacity", &self.sender.capacity())
            .field("closed", &self.sender.is_closed())
            .finish()
    }
}

impl StreamObservable {
    pub fn new(
        peer: PeerNode,
        buffer_size: usize,
    ) -> Result<(Self, mpsc::Receiver<OutboundDelivery>), CommError> {
        if buffer_size == 0 {
            return Err(CommError::ConfigError(
                "client stream queue capacity must be positive".to_string(),
            ));
        }
        let (sender, receiver) = mpsc::channel(buffer_size);
        let activity = ActivityGate::new();
        Ok((
            Self {
                peer,
                sender,
                activity,
                buffer_size,
            },
            receiver,
        ))
    }

    pub fn enqueue(
        &self,
        payload: Arc<OutboundPayload>,
    ) -> Result<oneshot::Receiver<Result<(), CommError>>, CommError> {
        let (delivery, completion) = OutboundDelivery::new(payload, &self.activity)?;
        self.sender
            .try_send(delivery)
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => CommError::ResourceExhausted(format!(
                    "client stream queue for {} is full at {} items",
                    self.peer.endpoint.host, self.buffer_size
                )),
                mpsc::error::TrySendError::Closed(_) => {
                    CommError::InternalCommunicationError(format!(
                        "client stream queue for {} is closed",
                        self.peer.endpoint.host
                    ))
                }
            })?;
        Ok(completion)
    }

    pub fn peer(&self) -> &PeerNode { &self.peer }

    pub fn buffer_size(&self) -> usize { self.buffer_size }

    pub fn available_capacity(&self) -> usize { self.sender.capacity() }

    pub fn resident_deliveries(&self) -> usize { self.activity.active() }

    pub(crate) fn activity(&self) -> Arc<ActivityGate> { self.activity.clone() }

    pub fn is_active(&self) -> bool { !self.sender.is_closed() }
}

#[cfg(test)]
mod tests {
    use models::routing::Packet;
    use prost::bytes::Bytes;

    use super::*;
    use crate::rust::peer_node::{Endpoint, NodeIdentifier};
    use crate::rust::transport::payload_budget::PayloadBudget;

    fn peer(name: &'static str) -> PeerNode {
        PeerNode {
            id: NodeIdentifier {
                key: Bytes::from_static(name.as_bytes()),
            },
            endpoint: Endpoint::new("127.0.0.1".to_string(), 8080, 8080),
        }
    }

    fn payload(budget: &Arc<PayloadBudget>, bytes: usize) -> Arc<OutboundPayload> {
        Arc::new(OutboundPayload::new(
            Blob {
                sender: peer("sender"),
                packet: Packet {
                    type_id: "TestPacket".to_string(),
                    content: Bytes::from(vec![1; bytes]),
                },
            },
            budget.try_reserve(bytes).unwrap(),
        ))
    }

    #[test]
    fn queue_capacity_failure_releases_rejected_unique_payload() {
        let budget = PayloadBudget::new("test", 32, 2).unwrap();
        let (queue, mut receiver) = StreamObservable::new(peer("remote"), 1).unwrap();
        let _first_completion = queue.enqueue(payload(&budget, 7)).unwrap();
        let rejected = payload(&budget, 11);
        assert!(matches!(
            queue.enqueue(rejected),
            Err(CommError::ResourceExhausted(_))
        ));
        assert_eq!(queue.resident_deliveries(), 1);
        assert_eq!(budget.used_bytes(), 7);
        drop(receiver.try_recv().unwrap());
        assert_eq!(queue.resident_deliveries(), 0);
        assert_eq!(budget.used_bytes(), 0);
    }

    #[test]
    fn receiver_drop_releases_every_queued_payload() {
        let budget = PayloadBudget::new("test", 32, 2).unwrap();
        let (queue, receiver) = StreamObservable::new(peer("remote"), 2).unwrap();
        let _first_completion = queue.enqueue(payload(&budget, 7)).unwrap();
        let _second_completion = queue.enqueue(payload(&budget, 11)).unwrap();
        assert_eq!(queue.resident_deliveries(), 2);
        assert_eq!(budget.used_bytes(), 18);
        drop(receiver);
        assert_eq!(queue.resident_deliveries(), 0);
        assert_eq!(budget.used_bytes(), 0);
        assert!(!queue.is_active());
    }

    #[test]
    fn shared_fanout_payload_holds_one_reservation() {
        let budget = PayloadBudget::new("test", 32, 1).unwrap();
        let payload = payload(&budget, 13);
        let (first, mut first_rx) = StreamObservable::new(peer("first"), 1).unwrap();
        let (second, mut second_rx) = StreamObservable::new(peer("second"), 1).unwrap();
        let _first_completion = first.enqueue(payload.clone()).unwrap();
        let _second_completion = second.enqueue(payload.clone()).unwrap();
        drop(payload);
        assert_eq!(budget.used_bytes(), 13);
        drop(first_rx.try_recv().unwrap());
        assert_eq!(budget.used_bytes(), 13);
        drop(second_rx.try_recv().unwrap());
        assert_eq!(budget.used_bytes(), 0);
    }

    #[tokio::test]
    async fn completion_remains_pending_until_delivery_reports_remote_result() {
        let budget = PayloadBudget::new("test", 32, 1).unwrap();
        let (queue, mut receiver) = StreamObservable::new(peer("remote"), 1).unwrap();
        let mut completion = queue.enqueue(payload(&budget, 7)).unwrap();
        assert!(matches!(
            completion.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        receiver.try_recv().unwrap().complete(Ok(()));
        assert!(completion.await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn worker_termination_cannot_be_reported_as_delivery_success() {
        let budget = PayloadBudget::new("test", 32, 1).unwrap();
        let (queue, mut receiver) = StreamObservable::new(peer("remote"), 1).unwrap();
        let completion = queue.enqueue(payload(&budget, 7)).unwrap();
        drop(receiver.try_recv().unwrap());
        assert!(completion.await.is_err());
    }
}
