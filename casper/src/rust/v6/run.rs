use models::rust::casper::protocol::casper_message::Event;
use prost::bytes::Bytes;

use super::body::StateHash;

#[derive(Clone, Debug, PartialEq)]
pub struct EnvelopeRun {
    pub envelope_sig: Bytes,
    pub comm_phlo: u64,
    pub compute_phlo: u64,
    pub failed: bool,
    pub user_log: Vec<Event>,
    pub post_root: StateHash,
}

impl EnvelopeRun {
    pub fn phlo_used(&self) -> u64 { self.comm_phlo.saturating_add(self.compute_phlo) }

    pub fn recorded(&self) -> RecordedRun<'_> {
        RecordedRun {
            comm_phlo: self.comm_phlo,
            compute_phlo: self.compute_phlo,
            failed: self.failed,
            user_log: &self.user_log,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RecordedRun<'a> {
    pub comm_phlo: u64,
    pub compute_phlo: u64,
    pub failed: bool,
    pub user_log: &'a [Event],
}

impl RecordedRun<'_> {
    pub fn mismatch(&self, replayed: &EnvelopeRun) -> Option<String> {
        if self.comm_phlo != replayed.comm_phlo {
            return Some(format!(
                "comm phlo differs: recorded {}, replayed {}",
                self.comm_phlo, replayed.comm_phlo
            ));
        }
        if self.compute_phlo != replayed.compute_phlo {
            return Some(format!(
                "compute phlo differs: recorded {}, replayed {}",
                self.compute_phlo, replayed.compute_phlo
            ));
        }
        if self.failed != replayed.failed {
            return Some(format!(
                "outcome differs: recorded failed={}, replayed failed={}",
                self.failed, replayed.failed
            ));
        }
        if self.user_log != replayed.user_log.as_slice() {
            return Some("user logs differ".to_string());
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(comm: u64, compute: u64) -> EnvelopeRun {
        EnvelopeRun {
            envelope_sig: Bytes::from_static(b"sig"),
            comm_phlo: comm,
            compute_phlo: compute,
            failed: false,
            user_log: Vec::new(),
            post_root: Bytes::from_static(b"root"),
        }
    }

    #[test]
    fn phlo_used_is_comm_plus_compute() {
        assert_eq!(run(3, 40).phlo_used(), 43);
        assert_eq!(run(u64::MAX, 1).phlo_used(), u64::MAX);
    }

    #[test]
    fn a_run_matches_its_own_recording() {
        let r = run(3, 40);
        assert_eq!(r.recorded().mismatch(&r), None);
    }

    #[test]
    fn replay_reports_each_kind_of_difference() {
        let recorded = run(3, 40);
        let rec = recorded.recorded();
        assert!(rec.mismatch(&run(4, 40)).unwrap().contains("comm phlo"));
        assert!(rec.mismatch(&run(3, 41)).unwrap().contains("compute phlo"));
        let mut failed = run(3, 40);
        failed.failed = true;
        assert!(rec.mismatch(&failed).unwrap().contains("outcome"));
    }
}
