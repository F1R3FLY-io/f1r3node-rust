use std::cmp::Ordering;
use std::collections::VecDeque;

use crypto::rust::signatures::signed::Signed;
use models::rust::casper::protocol::casper_message::DeployData;

pub const DEPLOY_QUEUE_BLOCKS: usize = 3;
const SERVICE_SAMPLE_BLOCKS: usize = 8;
const INITIAL_DEPLOYS_PER_BLOCK: usize = 8;

#[derive(Default)]
struct ServiceSamples {
    inclusions: VecDeque<usize>,
    last_sequence: Option<i32>,
}

#[derive(Default)]
pub struct DeployAdmission {
    samples: parking_lot::Mutex<ServiceSamples>,
}

impl DeployAdmission {
    pub(crate) fn observe(&self, sequence: i32, included: usize, had_demand: bool) {
        let mut samples = self.samples.lock();
        if samples.last_sequence.is_some_and(|last| sequence <= last) {
            return;
        }
        samples.last_sequence = Some(sequence);
        if !had_demand {
            return;
        }
        if samples.inclusions.len() == SERVICE_SAMPLE_BLOCKS {
            samples.inclusions.pop_front();
        }
        samples.inclusions.push_back(included);
        metrics::histogram!(
            crate::rust::metrics_constants::DEPLOY_ADMISSION_INCLUDED_METRIC,
            "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
        )
        .record(included as f64);
    }

    pub(crate) fn capacity(&self, shard_cap: usize, pending: usize) -> usize {
        if shard_cap == 0 {
            return 0;
        }
        let samples = self.samples.lock();
        if samples.inclusions.is_empty() {
            return INITIAL_DEPLOYS_PER_BLOCK
                .min(shard_cap)
                .saturating_mul(DEPLOY_QUEUE_BLOCKS);
        }
        let total: u128 = samples.inclusions.iter().map(|count| *count as u128).sum();
        let capacity = (total * DEPLOY_QUEUE_BLOCKS as u128 / samples.inclusions.len() as u128)
            .min(shard_cap.saturating_mul(DEPLOY_QUEUE_BLOCKS) as u128)
            as usize;
        if capacity == 0 && pending == 0 {
            1
        } else {
            capacity
        }
    }
}

pub(crate) fn compare_deploys(a: &Signed<DeployData>, b: &Signed<DeployData>) -> Ordering {
    a.data
        .valid_after_block_number
        .cmp(&b.data.valid_after_block_number)
        .then_with(|| a.data.time_stamp.cmp(&b.data.time_stamp))
        .then_with(|| a.sig.cmp(&b.sig))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_order_uses_height_then_timestamp_then_signature() {
        let deploy = |height, timestamp, term: &str| {
            crate::rust::util::construct_deploy::source_deploy(
                term.to_string(),
                timestamp,
                None,
                None,
                None,
                Some(height),
                Some("test".to_string()),
            )
            .unwrap()
        };
        let older_height = deploy(1, 100, "Nil");
        let newer_height = deploy(2, 1, "Nil");
        assert!(compare_deploys(&older_height, &newer_height).is_lt());
        let older_time = deploy(1, 99, "Nil");
        assert!(compare_deploys(&older_time, &older_height).is_lt());
        let same_time = deploy(1, 100, "@0!(0)");
        assert_eq!(
            compare_deploys(&older_height, &same_time),
            older_height.sig.cmp(&same_time.sig)
        );
        assert_eq!(
            compare_deploys(&older_height, &older_height),
            Ordering::Equal
        );
    }

    #[test]
    fn eight_inclusions_allow_twenty_four_positions() {
        let admission = DeployAdmission::default();
        admission.observe(1, 8, true);
        let capacity = admission.capacity(128, 24);
        assert!(24 <= capacity);
        assert!(25 > capacity);
    }

    #[test]
    fn cold_start_and_shard_cap_bound_capacity() {
        let admission = DeployAdmission::default();
        assert_eq!(admission.capacity(128, 0), 24);
        assert_eq!(admission.capacity(4, 0), 12);
        assert_eq!(admission.capacity(0, 0), 0);
        admission.observe(1, 128, true);
        assert_eq!(admission.capacity(4, 100), 12);
    }

    #[test]
    fn idle_and_repeated_blocks_do_not_change_the_estimate() {
        let admission = DeployAdmission::default();
        admission.observe(1, 8, true);
        admission.observe(2, 0, false);
        admission.observe(2, 100, true);
        admission.observe(1, 100, true);
        assert_eq!(admission.capacity(128, 1), 24);
        admission.observe(3, 0, true);
        assert_eq!(admission.capacity(128, 1), 12);
    }

    #[test]
    fn recent_samples_replace_old_capacity_and_allow_recovery() {
        let admission = DeployAdmission::default();
        admission.observe(0, 8, true);
        for sequence in 1..=8 {
            admission.observe(sequence, 0, true);
        }
        assert_eq!(admission.capacity(128, 1), 0);
        assert_eq!(admission.capacity(128, 0), 1);
        assert_eq!(admission.capacity(0, 0), 0);
        admission.observe(9, 8, true);
        assert_eq!(admission.capacity(128, 1), 3);
    }

    #[test]
    fn fractional_service_uses_the_whole_three_block_budget() {
        let admission = DeployAdmission::default();
        admission.observe(1, 1, true);
        admission.observe(2, 0, true);
        assert_eq!(admission.capacity(128, 1), 1);
    }
}
