use std::sync::atomic::{AtomicU8, Ordering};

use serde::de::Deserializer;
use serde::ser::{Error, Serializer};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordEncoding {
    Legacy,
    Cost,
}

static ENCODING: AtomicU8 = AtomicU8::new(0);

pub fn set_record_encoding(encoding: RecordEncoding) {
    let value = match encoding {
        RecordEncoding::Legacy => 0,
        RecordEncoding::Cost => 1,
    };
    ENCODING.store(value, Ordering::Relaxed);
}

pub fn record_encoding() -> RecordEncoding {
    match ENCODING.load(Ordering::Relaxed) {
        0 => RecordEncoding::Legacy,
        _ => RecordEncoding::Cost,
    }
}

pub trait CostField {
    fn is_absent(&self) -> bool;
}

impl<T> CostField for Vec<T> {
    fn is_absent(&self) -> bool { self.is_empty() }
}

impl<T> CostField for Option<T> {
    fn is_absent(&self) -> bool { self.is_none() }
}

pub fn serialize_cost_field<T, S>(value: &T, serializer: S) -> Result<S::Ok, S::Error>
where
    T: Serialize + CostField,
    S: Serializer,
{
    match record_encoding() {
        RecordEncoding::Cost => value.serialize(serializer),
        RecordEncoding::Legacy if value.is_absent() => ().serialize(serializer),
        RecordEncoding::Legacy => Err(S::Error::custom(format!(
            "a cost-accounting field cannot be written in the legacy record encoding: {}",
            std::any::type_name::<T>()
        ))),
    }
}

pub fn deserialize_cost_field<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    T: Deserialize<'de> + Default,
    D: Deserializer<'de>,
{
    match record_encoding() {
        RecordEncoding::Cost => T::deserialize(deserializer),
        RecordEncoding::Legacy => {
            <()>::deserialize(deserializer)?;
            Ok(T::default())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
    struct Record {
        head: u32,
        #[serde(
            serialize_with = "serialize_cost_field",
            deserialize_with = "deserialize_cost_field",
            default
        )]
        cost: Vec<u8>,
        tail: u32,
    }

    #[test]
    fn each_encoding_round_trips_and_only_the_cost_encoding_writes_the_field() {
        let empty = Record {
            head: 1,
            cost: Vec::new(),
            tail: 2,
        };
        let full = Record {
            head: 1,
            cost: vec![7],
            tail: 2,
        };

        set_record_encoding(RecordEncoding::Legacy);
        let legacy = bincode::serialize(&empty).unwrap();
        assert_eq!(legacy, bincode::serialize(&(1u32, 2u32)).unwrap());
        assert_eq!(bincode::deserialize::<Record>(&legacy).unwrap(), empty);
        assert!(bincode::serialize(&full).is_err());

        set_record_encoding(RecordEncoding::Cost);
        let cost = bincode::serialize(&full).unwrap();
        assert_eq!(cost, bincode::serialize(&(1u32, vec![7u8], 2u32)).unwrap());
        assert_eq!(bincode::deserialize::<Record>(&cost).unwrap(), full);

        set_record_encoding(RecordEncoding::Legacy);
    }
}
