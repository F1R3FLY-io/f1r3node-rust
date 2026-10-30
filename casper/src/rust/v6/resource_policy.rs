use serde::{Deserialize, Serialize};

use super::validity::Limits;

pub const DEFAULT_BLOCK_RECORDS: usize = 64;
pub const DEFAULT_BLOCK_RECORD_BYTES: usize = 8 * 1024 * 1024;
pub const DEFAULT_MAX_ENVELOPES: usize = 256;
pub const DEFAULT_DEMAND_CAP: i64 = 50_000_000_000;
pub const PROTOCOL_V6: i64 = 6;
const ENCODING_VERSION: u8 = 1;
const ENCODED_LEN: usize = 33;

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

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(ENCODED_LEN);
        out.push(ENCODING_VERSION);
        out.extend_from_slice(&(self.block_records as u64).to_be_bytes());
        out.extend_from_slice(&(self.block_record_bytes as u64).to_be_bytes());
        out.extend_from_slice(&(self.max_envelopes as u64).to_be_bytes());
        out.extend_from_slice(&self.demand_cap.to_be_bytes());
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
            demand_cap: i64::MAX,
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
        wrong_version[0] = 2;
        assert!(ShardResourcePolicy::decode(&wrong_version).is_err());
        let zero = ShardResourcePolicy {
            block_records: 0,
            ..ShardResourcePolicy::default()
        };
        assert!(ShardResourcePolicy::decode(&zero.encode()).is_err());
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
