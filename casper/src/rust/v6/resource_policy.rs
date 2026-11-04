use serde::{Deserialize, Serialize};

use super::validity::Limits;

pub const DEFAULT_BLOCK_RECORDS: usize = 64;
pub const DEFAULT_BLOCK_RECORD_BYTES: usize = 8 * 1024 * 1024;
pub const DEFAULT_MAX_ENVELOPES: usize = 256;
pub const DEFAULT_LOG_MILLIBYTES_PER_PHLO: u64 = 1_250;
pub const DEFAULT_ENTRY_OVERHEAD_BYTES: u64 = 4_096;
pub const DEFAULT_BLOCK_OVERHEAD_BYTES: u64 = 32_768;
pub const DEFAULT_DEMAND_CAP: i64 = ((DEFAULT_BLOCK_RECORD_BYTES as u64
    - DEFAULT_MAX_ENVELOPES as u64 * DEFAULT_ENTRY_OVERHEAD_BYTES
    - DEFAULT_BLOCK_OVERHEAD_BYTES)
    * 1_000
    / DEFAULT_LOG_MILLIBYTES_PER_PHLO) as i64;
pub const PROTOCOL_V6: i64 = 6;
const ENCODING_VERSION: u8 = 2;
const ENCODED_LEN: usize = 57;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct ShardResourcePolicy {
    pub block_records: usize,
    pub block_record_bytes: usize,
    pub max_envelopes: usize,
    pub demand_cap: i64,
    pub log_millibytes_per_phlo: u64,
    pub entry_overhead_bytes: u64,
    pub block_overhead_bytes: u64,
}

impl Default for ShardResourcePolicy {
    fn default() -> Self {
        ShardResourcePolicy {
            block_records: DEFAULT_BLOCK_RECORDS,
            block_record_bytes: DEFAULT_BLOCK_RECORD_BYTES,
            max_envelopes: DEFAULT_MAX_ENVELOPES,
            demand_cap: DEFAULT_DEMAND_CAP,
            log_millibytes_per_phlo: DEFAULT_LOG_MILLIBYTES_PER_PHLO,
            entry_overhead_bytes: DEFAULT_ENTRY_OVERHEAD_BYTES,
            block_overhead_bytes: DEFAULT_BLOCK_OVERHEAD_BYTES,
        }
    }
}

impl ShardResourcePolicy {
    pub fn check(&self) -> Result<(), String> {
        if self.block_records == 0 {
            return Err("block-records must be positive".to_string());
        }
        if self.block_record_bytes == 0 {
            return Err("block-record-bytes must be positive".to_string());
        }
        if self.max_envelopes == 0 {
            return Err("max-envelopes must be positive".to_string());
        }
        if self.demand_cap <= 0 {
            return Err("demand-cap must be positive".to_string());
        }
        if self.log_millibytes_per_phlo == 0 {
            return Err("log-millibytes-per-phlo must be positive".to_string());
        }
        let bound = self.record_bound(self.demand_cap as u64, self.max_envelopes as u64);
        if bound.is_none_or(|b| b > self.block_record_bytes as u128) {
            return Err(format!(
                "demand-cap {} does not fit the record bytes: {} x {} millibytes per phlo plus {} envelopes x {} bytes plus {} bytes exceeds block-record-bytes {}",
                self.demand_cap,
                self.demand_cap,
                self.log_millibytes_per_phlo,
                self.max_envelopes,
                self.entry_overhead_bytes,
                self.block_overhead_bytes,
                self.block_record_bytes
            ));
        }
        Ok(())
    }

    pub fn record_bound(&self, demand: u64, envelopes: u64) -> Option<u128> {
        let log = (demand as u128).checked_mul(self.log_millibytes_per_phlo as u128)?;
        let log = log.div_ceil(1_000);
        log.checked_add((envelopes as u128).checked_mul(self.entry_overhead_bytes as u128)?)?
            .checked_add(self.block_overhead_bytes as u128)
    }

    pub fn largest_demand_cap(&self) -> i64 {
        let fixed = self.max_envelopes as u128 * self.entry_overhead_bytes as u128
            + self.block_overhead_bytes as u128;
        let room = (self.block_record_bytes as u128).saturating_sub(fixed);
        (room * 1_000 / self.log_millibytes_per_phlo.max(1) as u128).min(i64::MAX as u128) as i64
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(ENCODED_LEN);
        out.push(ENCODING_VERSION);
        out.extend_from_slice(&(self.block_records as u64).to_be_bytes());
        out.extend_from_slice(&(self.block_record_bytes as u64).to_be_bytes());
        out.extend_from_slice(&(self.max_envelopes as u64).to_be_bytes());
        out.extend_from_slice(&self.demand_cap.to_be_bytes());
        out.extend_from_slice(&self.log_millibytes_per_phlo.to_be_bytes());
        out.extend_from_slice(&self.entry_overhead_bytes.to_be_bytes());
        out.extend_from_slice(&self.block_overhead_bytes.to_be_bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() != ENCODED_LEN {
            return Err(format!(
                "a resource policy is {ENCODED_LEN} bytes, not {}",
                bytes.len()
            ));
        }
        if bytes[0] != ENCODING_VERSION {
            return Err(format!("unknown resource policy encoding {}", bytes[0]));
        }
        let word = |i: usize| {
            let mut w = [0u8; 8];
            w.copy_from_slice(&bytes[1 + 8 * i..9 + 8 * i]);
            w
        };
        let size = |i: usize| {
            usize::try_from(u64::from_be_bytes(word(i)))
                .map_err(|_| "a resource policy cap does not fit this platform".to_string())
        };
        let policy = ShardResourcePolicy {
            block_records: size(0)?,
            block_record_bytes: size(1)?,
            max_envelopes: size(2)?,
            demand_cap: i64::from_be_bytes(word(3)),
            log_millibytes_per_phlo: u64::from_be_bytes(word(4)),
            entry_overhead_bytes: u64::from_be_bytes(word(5)),
            block_overhead_bytes: u64::from_be_bytes(word(6)),
        };
        policy.check()?;
        Ok(policy)
    }

    pub fn limits(&self) -> Limits {
        Limits {
            demand_cap: self.demand_cap,
            max_envelopes: self.max_envelopes,
            max_records: self.block_records,
            max_record_bytes: self.block_record_bytes,
            ordered_only: true,
            min_phlo_price: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_policy_sets_the_structural_limits() {
        let policy = ShardResourcePolicy {
            block_records: 7,
            block_record_bytes: 1024,
            max_envelopes: 9,
            demand_cap: 11,
            ..ShardResourcePolicy::default()
        };
        let limits = policy.limits();
        assert_eq!(limits.max_records, 7);
        assert_eq!(limits.max_envelopes, 9);
        assert_eq!(limits.demand_cap, 11);
        assert_eq!(limits.max_record_bytes, 1024);
        assert_eq!(Limits::default().max_records, DEFAULT_BLOCK_RECORDS);
    }

    #[test]
    fn missing_fields_take_their_defaults() {
        let policy: ShardResourcePolicy = serde_json::from_str(r#"{"block-records": 8}"#).unwrap();
        assert_eq!(policy.block_records, 8);
        assert_eq!(policy.block_record_bytes, DEFAULT_BLOCK_RECORD_BYTES);
        assert!(policy.check().is_ok());
    }

    #[test]
    fn the_encoding_round_trips() {
        let policy = ShardResourcePolicy {
            block_records: 7,
            block_record_bytes: 1 << 40,
            max_envelopes: 9,
            demand_cap: 1_000_000,
            log_millibytes_per_phlo: 1_100,
            entry_overhead_bytes: 2_048,
            block_overhead_bytes: 1_024,
        };
        let bytes = policy.encode();
        assert_eq!(bytes.len(), ENCODED_LEN);
        assert_eq!(ShardResourcePolicy::decode(&bytes), Ok(policy));
    }

    #[test]
    fn malformed_encodings_are_refused() {
        let bytes = ShardResourcePolicy::default().encode();
        assert!(ShardResourcePolicy::decode(&bytes[..ENCODED_LEN - 1]).is_err());
        let mut wrong_version = bytes.clone();
        wrong_version[0] = 1;
        assert!(ShardResourcePolicy::decode(&wrong_version).is_err());
        let zero = ShardResourcePolicy {
            block_records: 0,
            ..ShardResourcePolicy::default()
        };
        assert!(ShardResourcePolicy::decode(&zero.encode()).is_err());
    }

    #[test]
    fn the_demand_cap_fits_the_record_bytes() {
        let policy = ShardResourcePolicy::default();
        assert!(policy.check().is_ok());
        assert_eq!(policy.demand_cap, policy.largest_demand_cap());
        let bound = policy
            .record_bound(policy.demand_cap as u64, policy.max_envelopes as u64)
            .unwrap();
        assert!(bound <= policy.block_record_bytes as u128);
        let over = ShardResourcePolicy {
            demand_cap: policy.largest_demand_cap() + 1,
            ..policy
        };
        assert!(over
            .check()
            .unwrap_err()
            .contains("does not fit the record bytes"));
        let small = ShardResourcePolicy {
            block_record_bytes: 1 << 20,
            max_envelopes: 32,
            ..policy
        };
        assert_eq!(
            small.largest_demand_cap(),
            ((1 << 20) - 32 * 4_096 - 32_768) * 1_000 / 1_250
        );
        assert!(ShardResourcePolicy {
            log_millibytes_per_phlo: 0,
            ..policy
        }
        .check()
        .is_err());
    }

    #[test]
    fn zero_caps_are_refused() {
        let policy = ShardResourcePolicy {
            block_records: 0,
            ..ShardResourcePolicy::default()
        };
        assert!(policy.check().is_err());
        let policy = ShardResourcePolicy {
            demand_cap: 0,
            ..ShardResourcePolicy::default()
        };
        assert!(policy.check().is_err());
    }
}
