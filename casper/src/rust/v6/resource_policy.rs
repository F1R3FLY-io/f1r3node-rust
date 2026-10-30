use serde::{Deserialize, Serialize};

use super::validity::Limits;

pub const DEFAULT_BLOCK_RECORDS: usize = 64;
pub const DEFAULT_BLOCK_RECORD_BYTES: usize = 8 * 1024 * 1024;
pub const DEFAULT_MAX_ENVELOPES: usize = 256;
pub const DEFAULT_DEMAND_CAP: i64 = 50_000_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct ShardResourcePolicy {
    pub block_records: usize,
    pub block_record_bytes: usize,
    pub max_envelopes: usize,
    pub demand_cap: i64,
}

impl Default for ShardResourcePolicy {
    fn default() -> Self {
        ShardResourcePolicy {
            block_records: DEFAULT_BLOCK_RECORDS,
            block_record_bytes: DEFAULT_BLOCK_RECORD_BYTES,
            max_envelopes: DEFAULT_MAX_ENVELOPES,
            demand_cap: DEFAULT_DEMAND_CAP,
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
        Ok(())
    }

    pub fn limits(&self) -> Limits {
        Limits {
            demand_cap: self.demand_cap,
            max_envelopes: self.max_envelopes,
            max_records: self.block_records,
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
        };
        let limits = policy.limits();
        assert_eq!(limits.max_records, 7);
        assert_eq!(limits.max_envelopes, 9);
        assert_eq!(limits.demand_cap, 11);
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
