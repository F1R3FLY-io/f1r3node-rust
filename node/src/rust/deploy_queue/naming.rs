use std::time::Duration;

use super::config::DeployQueueConf;

/// `f1r3node.deploy.<shard>.<first 16 hex chars of validator pk>` unless the
/// operator named the queue.
pub fn queue_name(
    conf: &DeployQueueConf,
    shard_id: &str,
    validator_pk_hex: &Option<String>,
) -> String {
    if !conf.queue_name.is_empty() {
        return conf.queue_name.clone();
    }
    let prefix: String = validator_pk_hex
        .as_deref()
        .unwrap_or("")
        .trim_start_matches("0x")
        .chars()
        .take(16)
        .collect();
    format!("{}.{}.{}", conf.deploy_exchange, sanitize(shard_id), prefix)
}

/// Queue-side wall-clock TTL in milliseconds; `None` when disabled.
/// `ttl = deploy_lifespan * heartbeat.check-interval * ttl_factor`.
pub fn message_ttl_ms(
    ttl_factor: f64,
    deploy_lifespan: i64,
    check_interval: Duration,
) -> Option<u64> {
    if ttl_factor <= 0.0 || deploy_lifespan <= 0 || check_interval.is_zero() {
        return None;
    }
    let ms = (deploy_lifespan as f64) * (check_interval.as_millis() as f64) * ttl_factor;
    if !ms.is_finite() || ms < 1.0 {
        return None;
    }
    Some(ms.min(u32::MAX as f64) as u64)
}

/// Hide credentials when logging the broker URL.
pub fn redact_url(url: &str) -> String {
    match (url.find("://"), url.rfind('@')) {
        (Some(s), Some(at)) if at > s => format!("{}://***@{}", &url[..s], &url[at + 1..]),
        _ => url.to_string(),
    }
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_name_derives_from_shard_and_pk() {
        let conf = DeployQueueConf::default();
        let pk = Some("04abcdef0123456789abcdef0123456789".to_string());
        assert_eq!(
            queue_name(&conf, "asi-testnet", &pk),
            "f1r3node.deploy.asi-testnet.04abcdef01234567"
        );
    }

    #[test]
    fn queue_name_honours_override_and_sanitizes_shard() {
        let mut conf = DeployQueueConf::default();
        assert_eq!(
            queue_name(&conf, "root/shard a", &None),
            "f1r3node.deploy.root_shard_a."
        );
        conf.queue_name = "custom".into();
        assert_eq!(queue_name(&conf, "x", &None), "custom");
    }

    #[test]
    fn ttl_is_lifespan_times_cadence_times_factor() {
        assert_eq!(
            message_ttl_ms(1.0, 50, Duration::from_secs(5)),
            Some(250_000)
        );
        assert_eq!(
            message_ttl_ms(2.0, 50, Duration::from_secs(5)),
            Some(500_000)
        );
        assert_eq!(message_ttl_ms(0.0, 50, Duration::from_secs(5)), None);
        assert_eq!(message_ttl_ms(1.0, 0, Duration::from_secs(5)), None);
        assert_eq!(message_ttl_ms(1.0, 50, Duration::ZERO), None);
    }

    #[test]
    fn redacts_credentials() {
        assert_eq!(
            redact_url("amqps://u:p@host:5671/%2f"),
            "amqps://***@host:5671/%2f"
        );
        assert_eq!(redact_url("amqp://host:5672/%2f"), "amqp://host:5672/%2f");
    }
}
