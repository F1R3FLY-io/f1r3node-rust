use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use comm::rust::errors::CommError;
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
use crate::rust::recovery_budget::{RecoveryDispatch, RecoveryRegistration, RecoveryWindow};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CertificateRequestOutcome {
    Requested,
    Cooldown,
    Capacity,
}

#[derive(Debug)]
enum CertificateSendError {
    Transport(CasperError),
    Timeout,
}

enum CertificateRequestDecision {
    Dispatch(RecoveryDispatch<BlockHash>),
    Cooldown,
    Capacity,
}

impl CertificateSendError {
    fn is_timeout(&self) -> bool {
        matches!(
            self,
            Self::Timeout | Self::Transport(CasperError::CommError(CommError::TimeOut))
        )
    }

    fn into_casper(self) -> CasperError {
        match self {
            Self::Transport(error) => error,
            Self::Timeout => CasperError::CommError(CommError::TimeOut),
        }
    }
}

#[derive(Clone, Debug)]
pub struct FinalizationCertificateRetriever<T: TransportLayer + Send + Sync> {
    requests: Arc<Mutex<RecoveryWindow<BlockHash>>>,
    transport: Arc<T>,
    connections_cell: ConnectionsCell,
    conf: RPConf,
}

impl<T: TransportLayer + Send + Sync> FinalizationCertificateRetriever<T> {
    const MAX_TRACKED: usize = 256;
    const PEER_FANOUT: usize = 4;
    const BASE_RETRY_MS: u64 = 500;
    const MAX_RETRY_MS: u64 = 30_000;
    const MAX_DISPATCH_PER_TICK: usize = 16;

    pub fn new(transport: Arc<T>, connections_cell: ConnectionsCell, conf: RPConf) -> Self {
        let dispatch_timeout = conf.default_timeout;
        Self {
            requests: Arc::new(Mutex::new(RecoveryWindow::new(
                Self::MAX_TRACKED,
                Duration::from_millis(Self::BASE_RETRY_MS),
                Duration::from_millis(Self::MAX_RETRY_MS),
                dispatch_timeout,
            ))),
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
        let decision = {
            let mut requests = self.requests.lock().map_err(|_| {
                CasperError::RuntimeError(
                    "failed to acquire finalization certificate request tracker".to_string(),
                )
            })?;
            match requests.register(digest.clone(), now) {
                RecoveryRegistration::Capacity => CertificateRequestDecision::Capacity,
                RecoveryRegistration::Inserted | RecoveryRegistration::Present => {
                    match requests.take_ready_key(&digest, now) {
                        Some(dispatch) => CertificateRequestDecision::Dispatch(dispatch),
                        None => CertificateRequestDecision::Cooldown,
                    }
                }
            }
        };
        let outcome = match decision {
            CertificateRequestDecision::Dispatch(dispatch) => {
                let result = self.send(&dispatch).await;
                let timed_out = result.as_ref().is_err_and(CertificateSendError::is_timeout);
                self.finish(dispatch, timed_out)?;
                result.map_err(CertificateSendError::into_casper)?;
                CertificateRequestOutcome::Requested
            }
            CertificateRequestDecision::Cooldown => CertificateRequestOutcome::Cooldown,
            CertificateRequestDecision::Capacity => {
                metrics::counter!(FINALIZATION_CERTIFICATE_REQUESTS_CAPACITY_DEFERRED_METRIC, "source" => FINALIZATION_METRICS_SOURCE)
                    .increment(1);
                CertificateRequestOutcome::Capacity
            }
        };
        self.update_metric()?;
        Ok(outcome)
    }

    pub fn track(&self, digest: BlockHash) -> Result<(), CasperError> {
        Self::validate_digest(&digest)?;
        let registration = self
            .requests
            .lock()
            .map_err(|_| {
                CasperError::RuntimeError(
                    "failed to acquire finalization certificate request tracker".to_string(),
                )
            })?
            .register(digest, Instant::now());
        if registration == RecoveryRegistration::Capacity {
            metrics::counter!(FINALIZATION_CERTIFICATE_REQUESTS_CAPACITY_DEFERRED_METRIC, "source" => FINALIZATION_METRICS_SOURCE)
                .increment(1);
        }
        self.update_metric()
    }

    async fn send(
        &self,
        dispatch: &RecoveryDispatch<BlockHash>,
    ) -> Result<(), CertificateSendError> {
        metrics::counter!(FINALIZATION_CERTIFICATE_REQUESTS_TOTAL_METRIC, "source" => FINALIZATION_METRICS_SOURCE)
            .increment(1);
        if dispatch.attempt() > 1 {
            metrics::counter!(FINALIZATION_CERTIFICATE_REQUESTS_RETRIES_METRIC, "source" => FINALIZATION_METRICS_SOURCE)
                .increment(1);
        }
        let send = self.transport.send_message_to_peers(
            &self.connections_cell,
            &self.conf,
            Arc::new(FinalizationCertificateRequestProto {
                digest: dispatch.key().clone(),
            }),
            Some(Self::PEER_FANOUT),
        );
        match tokio::time::timeout(self.conf.default_timeout, send).await {
            Ok(result) => result
                .map_err(CasperError::from)
                .map_err(CertificateSendError::Transport),
            Err(_) => Err(CertificateSendError::Timeout),
        }
    }

    fn finish(
        &self,
        dispatch: RecoveryDispatch<BlockHash>,
        timed_out: bool,
    ) -> Result<(), CasperError> {
        let mut requests = self.requests.lock().map_err(|_| {
            CasperError::RuntimeError(
                "failed to acquire finalization certificate request tracker".to_string(),
            )
        })?;
        if timed_out {
            requests.expire_dispatch(dispatch, Instant::now());
        } else {
            requests.finish_dispatch(dispatch, Instant::now());
        }
        Ok(())
    }

    fn contains(&self, digest: &BlockHash) -> Result<bool, CasperError> {
        Ok(self
            .requests
            .lock()
            .map_err(|_| {
                CasperError::RuntimeError(
                    "failed to acquire finalization certificate request tracker".to_string(),
                )
            })?
            .contains(digest))
    }

    pub async fn request_all(&self) -> Result<(), CasperError> {
        let dispatches = {
            let mut requests = self.requests.lock().map_err(|_| {
                CasperError::RuntimeError(
                    "failed to acquire finalization certificate request tracker".to_string(),
                )
            })?;
            requests.take_ready_batch(Instant::now(), Self::MAX_DISPATCH_PER_TICK)
        };
        let results =
            futures::future::join_all(dispatches.iter().map(|dispatch| self.send(dispatch))).await;
        let mut first_error = None;
        for (dispatch, result) in dispatches.into_iter().zip(results) {
            let timed_out = result.as_ref().is_err_and(CertificateSendError::is_timeout);
            self.finish(dispatch, timed_out)?;
            if let Err(error) = result {
                first_error.get_or_insert_with(|| error.into_casper());
            }
        }
        self.update_metric()?;
        if let Some(error) = first_error {
            return Err(error);
        }
        Ok(())
    }

    pub fn response_is_expected(&self, digest: &BlockHash) -> Result<bool, CasperError> {
        Self::validate_digest(digest)?;
        self.contains(digest)
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
            .resolve(digest);
        self.update_metric()
    }

    pub fn retain_active(&self, active: &HashSet<BlockHash>) -> Result<(), CasperError> {
        let mut requests = self.requests.lock().map_err(|_| {
            CasperError::RuntimeError(
                "failed to acquire finalization certificate request tracker".to_string(),
            )
        })?;
        requests.retain_active(active);
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
        if !requests.make_ready(digest, Instant::now()) {
            return Err(CasperError::RuntimeError(
                "finalization certificate request is not tracked".to_string(),
            ));
        }
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
    async fn tracking_defers_transport_to_the_bounded_batch() {
        let (retriever, transport) = retriever();
        let digest = Bytes::from(vec![3; LENGTH]);

        retriever.track(digest.clone()).unwrap();
        assert_eq!(transport.request_count(), 0);
        assert!(retriever.response_is_expected(&digest).unwrap());

        retriever.request_all().await.unwrap();
        assert_eq!(transport.request_count(), 1);
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
        assert_eq!(
            retriever.request(survivor.clone()).await.unwrap(),
            CertificateRequestOutcome::Cooldown
        );
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

        retriever.make_retry_ready(&digest).unwrap();
        transport.reset();
        assert_eq!(
            retriever.request(digest.clone()).await.unwrap(),
            CertificateRequestOutcome::Requested
        );
        assert!(retriever.response_is_expected(&digest).unwrap());
        assert_eq!(transport.request_count(), 1);
    }

    #[tokio::test]
    async fn transport_timeout_releases_the_dispatch_with_maximum_backoff() {
        let (retriever, transport) = retriever();
        let digest = Bytes::from(vec![8; LENGTH]);
        transport.set_response_delay(Duration::from_secs(5));

        let error = tokio::time::timeout(Duration::from_secs(1), retriever.request(digest.clone()))
            .await
            .expect("the request has a bounded transport deadline")
            .unwrap_err();
        assert_eq!(error, CasperError::CommError(CommError::TimeOut));
        assert!(retriever.response_is_expected(&digest).unwrap());

        transport.reset();
        retriever.make_retry_ready(&digest).unwrap();
        assert_eq!(
            retriever.request(digest).await.unwrap(),
            CertificateRequestOutcome::Requested
        );
    }

    #[tokio::test]
    async fn cancelled_request_recovers_after_its_lease_and_maximum_backoff() {
        let (mut retriever, transport) = retriever();
        retriever.conf.default_timeout = Duration::from_secs(1);
        retriever.requests = Arc::new(Mutex::new(RecoveryWindow::new(
            1,
            Duration::from_millis(1),
            Duration::from_millis(10),
            Duration::from_millis(10),
        )));
        let digest = Bytes::from(vec![9; LENGTH]);
        transport.set_response_delay(Duration::from_secs(5));

        let request = {
            let retriever = retriever.clone();
            let digest = digest.clone();
            tokio::spawn(async move { retriever.request(digest).await })
        };
        tokio::time::sleep(Duration::from_millis(2)).await;
        request.abort();
        assert!(request.await.unwrap_err().is_cancelled());
        assert_eq!(retriever.tracked_count().unwrap(), 1);

        tokio::time::sleep(Duration::from_millis(25)).await;
        transport.reset();
        assert_eq!(
            retriever.request(digest).await.unwrap(),
            CertificateRequestOutcome::Requested
        );
    }

    #[tokio::test]
    async fn request_all_attempts_every_live_digest_when_transport_fails() {
        let (retriever, transport) = retriever();
        let left = Bytes::from(vec![1; LENGTH]);
        let right = Bytes::from(vec![2; LENGTH]);
        retriever.request(left.clone()).await.unwrap();
        retriever.request(right.clone()).await.unwrap();
        retriever.make_retry_ready(&left).unwrap();
        retriever.make_retry_ready(&right).unwrap();
        transport.reset();
        transport.set_responses(|_, _| Err(CommError::TimeOut));

        assert!(retriever.request_all().await.is_err());
        assert_eq!(transport.request_count(), 2);
        assert!(retriever.response_is_expected(&left).unwrap());
        assert!(retriever.response_is_expected(&right).unwrap());
    }

    #[tokio::test]
    async fn request_all_times_out_the_bounded_batch_concurrently() {
        let peer = peer();
        let transport = Arc::new(TransportLayerStub::new());
        let retriever = FinalizationCertificateRetriever::new(
            transport.clone(),
            ConnectionsCell {
                peers: Arc::new(Mutex::new(Connections::from_vec(vec![peer.clone()]))),
            },
            create_rp_conf_ask(peer, Some(Duration::from_millis(100)), None),
        );
        let now = Instant::now();
        {
            let mut requests = retriever.requests.lock().unwrap();
            for index in
                0..FinalizationCertificateRetriever::<TransportLayerStub>::MAX_DISPATCH_PER_TICK
            {
                let mut digest = vec![0; LENGTH];
                digest[LENGTH - std::mem::size_of::<usize>()..]
                    .copy_from_slice(&index.to_be_bytes());
                assert_eq!(
                    requests.register(Bytes::from(digest), now),
                    RecoveryRegistration::Inserted
                );
            }
        }
        transport.set_response_delay(Duration::from_secs(5));

        let error = tokio::time::timeout(Duration::from_secs(1), retriever.request_all())
            .await
            .expect("the batch shares one bounded dispatch interval")
            .unwrap_err();
        assert_eq!(error, CasperError::CommError(CommError::TimeOut));
        assert_eq!(
            retriever.tracked_count().unwrap(),
            FinalizationCertificateRetriever::<TransportLayerStub>::MAX_DISPATCH_PER_TICK
        );
    }

    #[tokio::test]
    async fn request_all_rotates_fairly_across_bounded_batches() {
        let (retriever, transport) = retriever();
        let count =
            FinalizationCertificateRetriever::<TransportLayerStub>::MAX_DISPATCH_PER_TICK + 4;
        let mut digests = Vec::with_capacity(count);
        for index in 0..count {
            let mut digest = vec![0; LENGTH];
            digest[LENGTH - std::mem::size_of::<usize>()..].copy_from_slice(&index.to_be_bytes());
            let digest = Bytes::from(digest);
            retriever.request(digest.clone()).await.unwrap();
            retriever.make_retry_ready(&digest).unwrap();
            digests.push(digest);
        }
        transport.reset();

        retriever.request_all().await.unwrap();
        assert_eq!(
            transport.request_count(),
            FinalizationCertificateRetriever::<TransportLayerStub>::MAX_DISPATCH_PER_TICK
        );
        retriever.request_all().await.unwrap();
        assert_eq!(transport.request_count(), count);

        let requested = (0..count)
            .map(|index| {
                let (_, protocol) = transport.get_request(index).unwrap();
                let packet = protocol_helper::to_packet(&protocol).unwrap();
                FinalizationCertificateRequestProto::decode(packet.content.as_ref())
                    .unwrap()
                    .digest
            })
            .collect::<HashSet<_>>();
        assert_eq!(requested, digests.into_iter().collect());
    }
}
