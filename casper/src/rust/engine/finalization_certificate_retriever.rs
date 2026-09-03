use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use comm::rust::rp::connect::ConnectionsCell;
use comm::rust::rp::rp_conf::RPConf;
use comm::rust::transport::transport_layer::TransportLayer;
use models::casper::FinalizationCertificateRequestProto;
use models::rust::block_hash::{BlockHash, LENGTH};

use crate::rust::errors::CasperError;
use crate::rust::metrics_constants::{
    FINALIZATION_CERTIFICATE_REQUESTS_CAPACITY_DEFERRED_METRIC,
    FINALIZATION_CERTIFICATE_REQUESTS_RETRIES_METRIC,
    FINALIZATION_CERTIFICATE_REQUESTS_TOTAL_METRIC,
    FINALIZATION_CERTIFICATE_RETRIEVER_TRACKED_METRIC, FINALIZATION_METRICS_SOURCE,
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct CertificateRequestState {
    last_request: Instant,
    attempts: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CertificateRequestOutcome {
    Requested,
    Cooldown,
    Capacity,
}

#[derive(Clone, Debug)]
pub struct FinalizationCertificateRetriever<T: TransportLayer + Send + Sync> {
    requests: Arc<Mutex<HashMap<BlockHash, CertificateRequestState>>>,
    transport: Arc<T>,
    connections_cell: ConnectionsCell,
    conf: RPConf,
}

impl<T: TransportLayer + Send + Sync> FinalizationCertificateRetriever<T> {
    const MAX_TRACKED: usize = 256;
    const PEER_FANOUT: usize = 4;
    const BASE_RETRY_MS: u64 = 500;
    const MAX_RETRY_MS: u64 = 30_000;

    pub fn new(transport: Arc<T>, connections_cell: ConnectionsCell, conf: RPConf) -> Self {
        Self {
            requests: Arc::new(Mutex::new(HashMap::new())),
            transport,
            connections_cell,
            conf,
        }
    }

    fn validate_digest(digest: &BlockHash) -> Result<(), CasperError> {
        if digest.len() != LENGTH {
            return Err(CasperError::RuntimeError(format!(
                "finalization certificate digest must be {LENGTH} bytes"
            )));
        }
        Ok(())
    }

    fn retry_delay(attempts: u32) -> Duration {
        let multiplier = 1u64 << attempts.saturating_sub(1).min(6);
        Duration::from_millis(
            Self::BASE_RETRY_MS
                .saturating_mul(multiplier)
                .min(Self::MAX_RETRY_MS),
        )
    }

    fn update_metric(&self) -> Result<(), CasperError> {
        let count = self
            .requests
            .lock()
            .map_err(|_| {
                CasperError::RuntimeError(
                    "failed to acquire finalization certificate request tracker".to_string(),
                )
            })?
            .len();
        metrics::gauge!(FINALIZATION_CERTIFICATE_RETRIEVER_TRACKED_METRIC, "source" => FINALIZATION_METRICS_SOURCE)
            .set(count as f64);
        Ok(())
    }

    pub async fn request(
        &self,
        digest: BlockHash,
    ) -> Result<CertificateRequestOutcome, CasperError> {
        Self::validate_digest(&digest)?;
        let now = Instant::now();
        let outcome = {
            let mut requests = self.requests.lock().map_err(|_| {
                CasperError::RuntimeError(
                    "failed to acquire finalization certificate request tracker".to_string(),
                )
            })?;
            if let Some(state) = requests.get_mut(&digest) {
                if now.duration_since(state.last_request) < Self::retry_delay(state.attempts) {
                    CertificateRequestOutcome::Cooldown
                } else {
                    state.last_request = now;
                    state.attempts = state.attempts.saturating_add(1);
                    CertificateRequestOutcome::Requested
                }
            } else if requests.len() >= Self::MAX_TRACKED {
                CertificateRequestOutcome::Capacity
            } else {
                requests.insert(digest.clone(), CertificateRequestState {
                    last_request: now,
                    attempts: 1,
                });
                CertificateRequestOutcome::Requested
            }
        };

        match outcome {
            CertificateRequestOutcome::Requested => {
                let attempts = self.attempts(&digest)?;
                metrics::counter!(FINALIZATION_CERTIFICATE_REQUESTS_TOTAL_METRIC, "source" => FINALIZATION_METRICS_SOURCE)
                    .increment(1);
                if attempts > 1 {
                    metrics::counter!(FINALIZATION_CERTIFICATE_REQUESTS_RETRIES_METRIC, "source" => FINALIZATION_METRICS_SOURCE)
                        .increment(1);
                }
                self.transport
                    .send_message_to_peers(
                        &self.connections_cell,
                        &self.conf,
                        Arc::new(FinalizationCertificateRequestProto {
                            digest: digest.clone(),
                        }),
                        Some(Self::PEER_FANOUT),
                    )
                    .await?;
            }
            CertificateRequestOutcome::Capacity => {
                metrics::counter!(FINALIZATION_CERTIFICATE_REQUESTS_CAPACITY_DEFERRED_METRIC, "source" => FINALIZATION_METRICS_SOURCE)
                    .increment(1);
            }
            CertificateRequestOutcome::Cooldown => {}
        }
        self.update_metric()?;
        Ok(outcome)
    }

    fn attempts(&self, digest: &BlockHash) -> Result<u32, CasperError> {
        Ok(self
            .requests
            .lock()
            .map_err(|_| {
                CasperError::RuntimeError(
                    "failed to acquire finalization certificate request tracker".to_string(),
                )
            })?
            .get(digest)
            .map(|state| state.attempts)
            .unwrap_or(0))
    }

    pub async fn request_all(&self) -> Result<(), CasperError> {
        let digests: Vec<BlockHash> = self
            .requests
            .lock()
            .map_err(|_| {
                CasperError::RuntimeError(
                    "failed to acquire finalization certificate request tracker".to_string(),
                )
            })?
            .keys()
            .cloned()
            .collect();
        let mut first_error = None;
        for digest in digests {
            if let Err(error) = self.request(digest).await {
                first_error.get_or_insert(error);
            }
        }
        if let Some(error) = first_error {
            return Err(error);
        }
        Ok(())
    }

    pub fn response_is_expected(&self, digest: &BlockHash) -> Result<bool, CasperError> {
        Self::validate_digest(digest)?;
        Ok(self
            .requests
            .lock()
            .map_err(|_| {
                CasperError::RuntimeError(
                    "failed to acquire finalization certificate request tracker".to_string(),
                )
            })?
            .contains_key(digest))
    }

    pub fn complete(&self, digest: &BlockHash) -> Result<(), CasperError> {
        Self::validate_digest(digest)?;
        self.requests
            .lock()
            .map_err(|_| {
                CasperError::RuntimeError(
                    "failed to acquire finalization certificate request tracker".to_string(),
                )
            })?
            .remove(digest);
        self.update_metric()
    }

    pub fn retain_active(&self, active: &HashSet<BlockHash>) -> Result<(), CasperError> {
        let mut requests = self.requests.lock().map_err(|_| {
            CasperError::RuntimeError(
                "failed to acquire finalization certificate request tracker".to_string(),
            )
        })?;
        requests.retain(|digest, _| active.contains(digest));
        drop(requests);
        self.update_metric()
    }

    #[cfg(test)]
    pub fn tracked_count(&self) -> Result<usize, CasperError> {
        Ok(self
            .requests
            .lock()
            .map_err(|_| {
                CasperError::RuntimeError(
                    "failed to acquire finalization certificate request tracker".to_string(),
                )
            })?
            .len())
    }

    #[cfg(test)]
    pub(crate) fn make_retry_ready(&self, digest: &BlockHash) -> Result<(), CasperError> {
        let mut requests = self.requests.lock().map_err(|_| {
            CasperError::RuntimeError(
                "failed to acquire finalization certificate request tracker".to_string(),
            )
        })?;
        let state = requests.get_mut(digest).ok_or_else(|| {
            CasperError::RuntimeError("finalization certificate request is not tracked".to_string())
        })?;
        state.last_request = Instant::now()
            .checked_sub(Self::retry_delay(state.attempts))
            .ok_or_else(|| {
                CasperError::RuntimeError(
                    "finalization certificate retry clock cannot be adjusted".to_string(),
                )
            })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use comm::rust::errors::CommError;
    use comm::rust::peer_node::{Endpoint, NodeIdentifier, PeerNode};
    use comm::rust::rp::connect::{Connections, ConnectionsCell};
    use comm::rust::rp::protocol_helper;
    use comm::rust::test_instances::{create_rp_conf_ask, TransportLayerStub};
    use prost::bytes::Bytes;
    use prost::Message;

    use super::*;

    fn peer() -> PeerNode {
        PeerNode {
            id: NodeIdentifier {
                key: Bytes::from_static(b"certificate-peer"),
            },
            endpoint: Endpoint {
                host: "host".to_string(),
                tcp_port: 40400,
                udp_port: 40400,
            },
        }
    }

    fn retriever() -> (
        FinalizationCertificateRetriever<TransportLayerStub>,
        Arc<TransportLayerStub>,
    ) {
        let peer = peer();
        let transport = Arc::new(TransportLayerStub::new());
        let retriever = FinalizationCertificateRetriever::new(
            transport.clone(),
            ConnectionsCell {
                peers: Arc::new(Mutex::new(Connections::from_vec(vec![peer.clone()]))),
            },
            create_rp_conf_ask(peer, None, None),
        );
        (retriever, transport)
    }

    #[tokio::test]
    async fn request_is_bounded_deduplicated_and_content_addressed() {
        let (retriever, transport) = retriever();
        let digest = Bytes::from(vec![1; LENGTH]);

        assert_eq!(
            retriever.request(digest.clone()).await.unwrap(),
            CertificateRequestOutcome::Requested
        );
        assert_eq!(
            retriever.request(digest.clone()).await.unwrap(),
            CertificateRequestOutcome::Cooldown
        );
        assert_eq!(retriever.tracked_count().unwrap(), 1);
        assert!(retriever.response_is_expected(&digest).unwrap());
        assert_eq!(transport.request_count(), 1);
        let (_, protocol) = transport.get_request(0).expect("certificate request");
        let packet = protocol_helper::to_packet(&protocol).expect("request packet");
        assert_eq!(packet.type_id, "FinalizationCertificateRequest");
        let request = FinalizationCertificateRequestProto::decode(packet.content.as_ref())
            .expect("certificate request payload");
        assert_eq!(request.digest, digest);

        retriever.complete(&digest).unwrap();
        assert!(!retriever.response_is_expected(&digest).unwrap());
        assert_eq!(retriever.tracked_count().unwrap(), 0);
    }

    #[tokio::test]
    async fn tracker_capacity_defers_new_work_without_evicting_live_requests() {
        let peer = peer();
        let transport = Arc::new(TransportLayerStub::new());
        let retriever = FinalizationCertificateRetriever::new(
            transport,
            ConnectionsCell {
                peers: Arc::new(Mutex::new(Connections::from_vec(Vec::new()))),
            },
            create_rp_conf_ask(peer, None, None),
        );
        let mut active = HashSet::new();
        for index in 0..FinalizationCertificateRetriever::<TransportLayerStub>::MAX_TRACKED {
            let mut digest = vec![0; LENGTH];
            digest[LENGTH - std::mem::size_of::<usize>()..].copy_from_slice(&index.to_be_bytes());
            let digest = Bytes::from(digest);
            active.insert(digest.clone());
            assert_eq!(
                retriever.request(digest).await.unwrap(),
                CertificateRequestOutcome::Requested
            );
        }
        assert_eq!(
            retriever
                .request(Bytes::from(vec![0xff; LENGTH]))
                .await
                .unwrap(),
            CertificateRequestOutcome::Capacity
        );
        assert_eq!(
            retriever.tracked_count().unwrap(),
            FinalizationCertificateRetriever::<TransportLayerStub>::MAX_TRACKED
        );

        let survivor = active.iter().next().unwrap().clone();
        retriever
            .retain_active(&HashSet::from([survivor.clone()]))
            .unwrap();
        assert_eq!(retriever.tracked_count().unwrap(), 1);
        assert!(retriever.response_is_expected(&survivor).unwrap());
    }

    #[tokio::test]
    async fn malformed_digest_never_enters_the_tracker() {
        let (retriever, _) = retriever();
        assert!(retriever
            .request(Bytes::from_static(b"short"))
            .await
            .is_err());
        assert_eq!(retriever.tracked_count().unwrap(), 0);
    }

    #[tokio::test]
    async fn transport_failure_retains_the_live_request_for_retry() {
        let (retriever, transport) = retriever();
        let digest = Bytes::from(vec![7; LENGTH]);
        transport.set_responses(|_, _| Err(CommError::TimeOut));

        assert!(retriever.request(digest.clone()).await.is_err());
        assert!(retriever.response_is_expected(&digest).unwrap());
        assert_eq!(retriever.tracked_count().unwrap(), 1);

        retriever
            .requests
            .lock()
            .unwrap()
            .get_mut(&digest)
            .unwrap()
            .last_request = Instant::now().checked_sub(Duration::from_secs(1)).unwrap();
        transport.reset();
        assert_eq!(
            retriever.request(digest.clone()).await.unwrap(),
            CertificateRequestOutcome::Requested
        );
        assert!(retriever.response_is_expected(&digest).unwrap());
        assert_eq!(transport.request_count(), 1);
    }

    #[tokio::test]
    async fn request_all_attempts_every_live_digest_when_transport_fails() {
        let (retriever, transport) = retriever();
        let left = Bytes::from(vec![1; LENGTH]);
        let right = Bytes::from(vec![2; LENGTH]);
        retriever.request(left.clone()).await.unwrap();
        retriever.request(right.clone()).await.unwrap();
        let retry_ready = Instant::now().checked_sub(Duration::from_secs(1)).unwrap();
        for state in retriever.requests.lock().unwrap().values_mut() {
            state.last_request = retry_ready;
        }
        transport.reset();
        transport.set_responses(|_, _| Err(CommError::TimeOut));

        assert!(retriever.request_all().await.is_err());
        assert_eq!(transport.request_count(), 2);
        assert!(retriever.response_is_expected(&left).unwrap());
        assert!(retriever.response_is_expected(&right).unwrap());
    }
}
