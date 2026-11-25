use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::io::Cursor;
use std::time::Instant;

use bincode::Options;
use serde::{Deserialize, Serialize};

#[allow(dead_code, clippy::large_enum_variant, clippy::enum_variant_names)]
pub mod rhoapi {
    include!(concat!(env!("OUT_DIR"), "/rhoapi.rs"));
}
type Blake2b256Hash = Vec<u8>;
include!(concat!(env!("OUT_DIR"), "/leaf_records.rs"));

pub const MAX_RECORD_BYTES: usize = 16 * 1024 * 1024;

pub fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, Box<dyn Error>> {
    if bytes.len() > MAX_RECORD_BYTES {
        return Err("encoded record exceeds 16 MiB decoder limit".into());
    }
    let mut input = Cursor::new(bytes);
    let options = bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_limit(MAX_RECORD_BYTES as u64);
    let mut decoder = bincode::Deserializer::with_reader(&mut input, options);
    let value = T::deserialize(serde_stacker::Deserializer::new(&mut decoder))
        .map_err(|_| "record does not match source-derived bincode schema")?;
    if input.position() != bytes.len() as u64 {
        return Err("trailing bytes after record".into());
    }
    Ok(value)
}

pub fn borrowed_bytes(bytes: &[u8]) -> Result<&[u8], Box<dyn Error>> {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_limit(MAX_RECORD_BYTES as u64)
        .reject_trailing_bytes()
        .deserialize(bytes)
        .map_err(|_| "invalid byte-vector wrapper".into())
}

#[derive(Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Distribution {
    pub count: u64,
    pub sum: u64,
    pub frequencies: BTreeMap<u64, u64>,
}

impl Distribution {
    pub fn validate(&self) -> Result<(), Box<dyn Error>> {
        let (mut count, mut sum) = (0u64, 0u64);
        for (value, n) in &self.frequencies {
            if *n == 0 {
                return Err("empty histogram bucket".into());
            }
            count = count.checked_add(*n).ok_or("histogram count overflow")?;
            sum = sum
                .checked_add(value.checked_mul(*n).ok_or("histogram product overflow")?)
                .ok_or("histogram sum overflow")?;
        }
        if count != self.count || sum != self.sum {
            return Err("histogram does not reconcile".into());
        }
        Ok(())
    }

    pub fn add(&mut self, value: u64) {
        self.count += 1;
        self.sum += value;
        *self.frequencies.entry(value).or_default() += 1;
    }
    pub fn merge(&mut self, other: &Self) {
        self.count += other.count;
        self.sum += other.sum;
        for (value, n) in &other.frequencies {
            *self.frequencies.entry(*value).or_default() += n;
        }
    }
    fn quantile(&self, percent: u64) -> Option<u64> {
        if self.count == 0 {
            return None;
        }
        let rank = (self.count * percent).div_ceil(100).max(1);
        let mut n = 0;
        self.frequencies.iter().find_map(|(v, count)| {
            n += count;
            (n >= rank).then_some(*v)
        })
    }
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"count":self.count,"sum":self.sum,
            "min":self.frequencies.first_key_value().map(|(v,_)|v),
            "mean":(self.count>0).then(||self.sum as f64/self.count as f64),
            "p50":self.quantile(50),"p95":self.quantile(95),
            "max":self.frequencies.last_key_value().map(|(v,_)|v)})
    }
}

#[derive(Default)]
pub struct Nested {
    pub par_sizes: Distribution,
    pub outer_par_bytes: u64,
    pub outer_par_count: u64,
    pub max_type_depth: usize,
}

pub struct WalkContext {
    pub nested: Nested,
    pub visits: usize,
    pub deadline: Instant,
}

impl WalkContext {
    fn visit(&mut self, depth: usize) -> Result<(), Box<dyn Error>> {
        self.visits += 1;
        if depth > 256 || self.visits > 5_000_000 {
            return Err("nested value traversal bound exceeded".into());
        }
        if self.visits.is_multiple_of(1024) && Instant::now() > self.deadline {
            return Err("decode time budget exceeded".into());
        }
        self.nested.max_type_depth = self.nested.max_type_depth.max(depth);
        Ok(())
    }
    fn record_par(&mut self, value: &rhoapi::Par, par_depth: usize) -> Result<(), Box<dyn Error>> {
        let bytes = bincode::serialized_size(value)?;
        self.nested.par_sizes.add(bytes);
        if par_depth == 0 {
            self.nested.outer_par_count += 1;
            self.nested.outer_par_bytes += bytes;
        }
        Ok(())
    }
}

trait Walk {
    const HAS_PAR: bool;
    fn walk(
        &self,
        ctx: &mut WalkContext,
        depth: usize,
        par_depth: usize,
    ) -> Result<(), Box<dyn Error>>;
}

macro_rules! primitive {
    ($($t:ty),*) => { $(impl Walk for $t {
        const HAS_PAR: bool = false;
        fn walk(&self, _: &mut WalkContext, _: usize, _: usize) -> Result<(), Box<dyn Error>> { Ok(()) }
    })* };
}
primitive!(u8, u32, u64, i32, i64, bool, f32, f64, String);

impl<T: Walk> Walk for Vec<T> {
    const HAS_PAR: bool = T::HAS_PAR;
    fn walk(&self, ctx: &mut WalkContext, depth: usize, pd: usize) -> Result<(), Box<dyn Error>> {
        if T::HAS_PAR {
            for v in self {
                v.walk(ctx, depth, pd)?;
            }
        }
        Ok(())
    }
}
impl<T: Walk> Walk for Option<T> {
    const HAS_PAR: bool = T::HAS_PAR;
    fn walk(&self, ctx: &mut WalkContext, depth: usize, pd: usize) -> Result<(), Box<dyn Error>> {
        if let Some(v) = self {
            v.walk(ctx, depth, pd)?;
        }
        Ok(())
    }
}
impl<T: Walk> Walk for Box<T> {
    const HAS_PAR: bool = T::HAS_PAR;
    fn walk(&self, ctx: &mut WalkContext, depth: usize, pd: usize) -> Result<(), Box<dyn Error>> {
        (**self).walk(ctx, depth, pd)
    }
}
impl<K: Walk, V: Walk> Walk for BTreeMap<K, V> {
    const HAS_PAR: bool = K::HAS_PAR || V::HAS_PAR;
    fn walk(&self, ctx: &mut WalkContext, depth: usize, pd: usize) -> Result<(), Box<dyn Error>> {
        for (k, v) in self {
            k.walk(ctx, depth, pd)?;
            v.walk(ctx, depth, pd)?;
        }
        Ok(())
    }
}
include!(concat!(env!("OUT_DIR"), "/leaf_walks.rs"));

#[derive(Default)]
pub struct Elements {
    pub sizes: Distribution,
    pub pars_per_element: Distribution,
    pub groups_or_values: Distribution,
    pub nested: Nested,
    pub persistent: u64,
}

pub fn analyze(kind: &str, payload: &[u8], deadline: Instant) -> Result<Elements, Box<dyn Error>> {
    let entries: Vec<&[u8]> = bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_limit(MAX_RECORD_BYTES as u64)
        .reject_trailing_bytes()
        .deserialize(payload)
        .map_err(|_| "invalid encoded leaf element list")?;
    let mut result = Elements::default();
    let mut ctx = WalkContext {
        nested: Nested::default(),
        visits: 0,
        deadline,
    };
    for entry in entries {
        if Instant::now() > deadline {
            return Err("decode time budget exceeded".into());
        }
        result.sizes.add(entry.len() as u64);
        let before = ctx.nested.par_sizes.count;
        let encoded_size = match kind {
            "DataLeaf" => {
                let datum: Datum<rhoapi::ListParWithRandom> = decode(entry)?;
                result.persistent += u64::from(datum.persist);
                result.groups_or_values.add(datum.a.pars.len() as u64);
                datum.a.walk(&mut ctx, 0, 0)?;
                bincode::serialized_size(&datum)?
            }
            "ContinuationsLeaf" => {
                let wk: WaitingContinuation<rhoapi::BindPattern, rhoapi::TaggedContinuation> =
                    decode(entry)?;
                result.persistent += u64::from(wk.persist);
                result.groups_or_values.add(wk.patterns.len() as u64);
                wk.patterns.walk(&mut ctx, 0, 0)?;
                wk.continuation.walk(&mut ctx, 0, 0)?;
                bincode::serialized_size(&wk)?
            }
            "JoinsLeaf" => {
                let channels: Vec<rhoapi::Par> = decode(entry)?;
                result.groups_or_values.add(channels.len() as u64);
                channels.walk(&mut ctx, 0, 0)?;
                bincode::serialized_size(&channels)?
            }
            _ => return Err("unknown leaf variant".into()),
        };
        if encoded_size != entry.len() as u64 {
            return Err("decoded record size does not match source encoding".into());
        }
        result
            .pars_per_element
            .add(ctx.nested.par_sizes.count - before);
    }
    result.nested = ctx.nested;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn nested_pars_count_empty_messages_and_separate_overlapping_sizes() {
        let child = rhoapi::Par::default();
        let parent = rhoapi::Par {
            sends: vec![rhoapi::Send {
                data: vec![child.clone()],
                ..Default::default()
            }],
            ..Default::default()
        };
        let payload =
            bincode::serialize(&vec![bincode::serialize(&vec![parent.clone()]).unwrap()]).unwrap();
        let result = analyze(
            "JoinsLeaf",
            &payload,
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();
        assert_eq!(result.nested.par_sizes.count, 2);
        assert_eq!(result.nested.outer_par_count, 1);
        assert_eq!(
            result.nested.outer_par_bytes,
            bincode::serialized_size(&parent).unwrap()
        );
        assert_eq!(
            result.nested.par_sizes.sum,
            bincode::serialized_size(&parent).unwrap() + bincode::serialized_size(&child).unwrap()
        );
        assert_eq!(result.groups_or_values.sum, 1);
    }

    #[test]
    fn strict_decoder_rejects_trailing_truncated_and_oversized_records() {
        assert!(decode::<u64>(&[0; 7]).is_err());
        assert!(decode::<u64>(&[0; 9]).is_err());
        assert!(decode::<u64>(&vec![0; MAX_RECORD_BYTES + 1]).is_err());
    }

    #[test]
    fn distributions_use_nearest_rank_and_merge_without_sampling() {
        let mut a = Distribution::default();
        for n in 1..=100 {
            a.add(n);
        }
        assert_eq!(a.quantile(50), Some(50));
        assert_eq!(a.quantile(95), Some(95));
        let mut b = Distribution::default();
        b.add(1000);
        a.merge(&b);
        assert_eq!(a.count, 101);
        assert_eq!(a.sum, 6050);
        a.validate().unwrap();
        a.sum += 1;
        assert!(a.validate().is_err());
        assert!(Distribution::default().quantile(95).is_none());
    }

    #[test]
    fn continuation_walk_includes_patterns_oneof_body_and_guard() {
        let wk = WaitingContinuation {
            patterns: vec![rhoapi::BindPattern {
                patterns: vec![rhoapi::Par::default()],
                ..Default::default()
            }],
            continuation: rhoapi::TaggedContinuation {
                tagged_cont: Some(rhoapi::tagged_continuation::TaggedCont::ParBody(
                    rhoapi::ParWithRandom {
                        body: Some(rhoapi::Par::default()),
                        random_state: vec![1, 2, 3],
                    },
                )),
                guard: Some(rhoapi::Par::default()),
            },
            persist: true,
            peeks: BTreeSet::from([0]),
            source: Consume {
                channel_hashes: vec![vec![0; 32]],
                hash: vec![1; 32],
                persistent: true,
            },
        };
        let payload = bincode::serialize(&vec![bincode::serialize(&wk).unwrap()]).unwrap();
        let result = analyze(
            "ContinuationsLeaf",
            &payload,
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();
        assert_eq!(result.nested.par_sizes.count, 3);
        assert_eq!(result.nested.outer_par_count, 3);
        assert_eq!(result.persistent, 1);
        assert_eq!(result.groups_or_values.sum, 1);
        assert_eq!(result.pars_per_element.sum, 3);
    }
}
