use casper::rust::api::block_api::DeployValidationError;
use casper::rust::casper::DeployError;

/// What the consumer does with a delivery, decided only by the outcome of
/// `BlockAPI::deploy`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Admitted to the pool: ack, publish `accepted`.
    Accepted,
    /// Already known (redelivery after a crash, or a client retry): ack,
    /// publish `accepted { duplicate: true }`.
    Duplicate,
    /// The same bytes will fail again: ack, publish `rejected`.
    Terminal(String),
    /// Retry the same delivery later; the node, not the deploy, is the cause.
    Transient(String),
    /// Transient, and an operator problem: retry, warn, raise the stalled gauge.
    TransientAlarm(String),
}

impl Verdict {
    pub fn label(&self) -> &'static str {
        match self {
            Verdict::Accepted => "accepted",
            Verdict::Duplicate => "duplicate",
            Verdict::Terminal(_) => "rejected",
            Verdict::Transient(_) | Verdict::TransientAlarm(_) => "deferred",
        }
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            Verdict::Accepted | Verdict::Duplicate => None,
            Verdict::Terminal(r) | Verdict::Transient(r) | Verdict::TransientAlarm(r) => Some(r),
        }
    }

    pub fn is_retry(&self) -> bool {
        matches!(self, Verdict::Transient(_) | Verdict::TransientAlarm(_))
    }
}

/// Classify a decode or signature failure that happened before
/// `BlockAPI::deploy` was reached.
pub fn classify_decode_failure(reason: impl Into<String>) -> Verdict {
    Verdict::Terminal(reason.into())
}

/// Classify the result of `BlockAPI::deploy`.
pub fn classify(result: &Result<String, eyre::Report>) -> Verdict {
    match result {
        Ok(_) => Verdict::Accepted,
        Err(report) => classify_report(report),
    }
}

fn classify_report(report: &eyre::Report) -> Verdict {
    for cause in report.chain() {
        if let Some(e) = cause.downcast_ref::<DeployError>() {
            return match e {
                DeployError::DuplicateDeploy(_) => Verdict::Duplicate,
                DeployError::ParsingError(d) => Verdict::Terminal(format!("parse error: {d}")),
                DeployError::MissingUser => Verdict::Terminal("missing user".into()),
                DeployError::UnknownSignatureAlgorithm(a) => {
                    Verdict::Terminal(format!("unknown signature algorithm {a}"))
                }
                DeployError::SignatureVerificationFailed => {
                    Verdict::Terminal("signature verification failed".into())
                }
            };
        }
        if let Some(e) = cause.downcast_ref::<DeployValidationError>() {
            return classify_validation_message(&e.message);
        }
    }
    classify_message(&report.to_string())
}

/// `DeployValidationError` carries only a message; the two operator-side
/// conditions are recognised by their fixed wording in `BlockAPI::deploy`.
fn classify_validation_message(message: &str) -> Verdict {
    let m = message.to_ascii_lowercase();
    if m.contains("read-only mode") {
        return Verdict::TransientAlarm(message.to_string());
    }
    if m.contains("not bonded") {
        return Verdict::TransientAlarm(message.to_string());
    }
    Verdict::Terminal(message.to_string())
}

/// Errors that are not typed: the engine-not-ready warning, a pool cap on
/// branches that have one (`PoolFull`), and storage errors.
fn classify_message(message: &str) -> Verdict {
    let m = message.to_ascii_lowercase();
    if m.contains("casper instance was not available") {
        return Verdict::Transient("casper engine not available yet".into());
    }
    if m.contains("poolfull") || (m.contains("pool") && m.contains("full")) {
        return Verdict::Transient(message.to_string());
    }
    if m.contains("not bonded") || m.contains("read-only mode") {
        return Verdict::TransientAlarm(message.to_string());
    }
    Verdict::Transient(message.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err<E: std::error::Error + Send + Sync + 'static>(e: E) -> Result<String, eyre::Report> {
        Err(eyre::Report::new(e))
    }

    #[test]
    fn ok_is_accepted() {
        assert_eq!(classify(&Ok("Success".into())), Verdict::Accepted);
    }

    #[test]
    fn duplicate_is_acked_as_duplicate() {
        let r = err(DeployError::DuplicateDeploy(vec![1u8, 2, 3]));
        assert_eq!(classify(&r), Verdict::Duplicate);
    }

    #[test]
    fn deploy_errors_are_terminal() {
        assert!(matches!(
            classify(&err(DeployError::ParsingError("x".into()))),
            Verdict::Terminal(_)
        ));
        assert!(matches!(
            classify(&err(DeployError::SignatureVerificationFailed)),
            Verdict::Terminal(_)
        ));
        assert!(matches!(
            classify(&err(DeployError::UnknownSignatureAlgorithm("y".into()))),
            Verdict::Terminal(_)
        ));
        assert!(matches!(
            classify(&err(DeployError::MissingUser)),
            Verdict::Terminal(_)
        ));
    }

    #[test]
    fn validation_errors_split_by_cause() {
        let shard = err(DeployValidationError {
            message: "Deploy shardId 'a' is not as expected network shard 'b'.".into(),
        });
        assert!(matches!(classify(&shard), Verdict::Terminal(_)));
        let window = err(DeployValidationError {
            message:
                "Deploy validAfterBlockNumber 1 has expired at block 90 with deploy lifespan 50."
                    .into(),
        });
        assert!(matches!(classify(&window), Verdict::Terminal(_)));
        let bonded = err(DeployValidationError {
            message: "Deploy was rejected because node's validator is not bonded.".into(),
        });
        assert!(matches!(classify(&bonded), Verdict::TransientAlarm(_)));
        let ro = err(DeployValidationError {
            message: "Deploy was rejected because node is running in read-only mode.".into(),
        });
        assert!(matches!(classify(&ro), Verdict::TransientAlarm(_)));
    }

    #[test]
    fn untyped_errors_are_transient() {
        let not_ready: Result<String, eyre::Report> = Err(eyre::eyre!(
            "Error: Could not deploy, casper instance was not available yet."
        ));
        assert!(matches!(classify(&not_ready), Verdict::Transient(_)));
        let pool: Result<String, eyre::Report> =
            Err(eyre::eyre!("PoolFull {{ pending: 8192, cap: 8192 }}"));
        assert!(matches!(classify(&pool), Verdict::Transient(_)));
        let other: Result<String, eyre::Report> = Err(eyre::eyre!("lmdb: MDB_MAP_FULL"));
        assert!(matches!(classify(&other), Verdict::Transient(_)));
    }

    #[test]
    fn wrapped_causes_are_found() {
        let inner = eyre::Report::new(DeployError::DuplicateDeploy(vec![9u8]));
        let wrapped: Result<String, eyre::Report> = Err(inner.wrap_err("while admitting"));
        assert_eq!(classify(&wrapped), Verdict::Duplicate);
    }
}
