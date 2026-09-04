pub mod dag;
pub mod env;
pub mod grpc;
pub mod hashable_set;
pub mod metrics_constants;
pub mod metrics_semaphore;
pub mod shared;
pub mod store;
pub mod tracing_init;

pub type ByteVector = Vec<u8>;
pub type ByteBuffer = Vec<u8>;
pub type Byte = u8;
pub type ByteString = Vec<u8>;
pub type BitSet = Vec<u8>;
pub type BitVector = Vec<u8>;

pub mod serde_bytes {
    use prost::bytes::Bytes;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(bytes: &Bytes, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer {
        // Convert Bytes to &[u8] and serialize with serde_bytes
        ::serde_bytes::serialize(bytes.as_ref(), serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Bytes, D::Error>
    where D: Deserializer<'de> {
        // Deserialize to Vec<u8> and then convert to Bytes
        let bytes: Vec<u8> = ::serde_bytes::deserialize(deserializer)?;
        Ok(Bytes::from(bytes))
    }
}

pub mod serde_vec_bytes {
    use prost::bytes::Bytes;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(bytes_vec: &Vec<Bytes>, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer {
        // Convert Vec<Bytes> to Vec<Vec<u8>> for serialization
        let vec_u8: Vec<Vec<u8>> = bytes_vec.iter().map(|b| b.to_vec()).collect();
        vec_u8.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<Bytes>, D::Error>
    where D: Deserializer<'de> {
        // Deserialize as Vec<Vec<u8>> and convert to Vec<Bytes>
        let vec_u8: Vec<Vec<u8>> = Vec::deserialize(deserializer)?;
        let bytes_vec = vec_u8.into_iter().map(Bytes::from).collect();
        Ok(bytes_vec)
    }
}

pub mod serde_btreemap_bytes_i64 {
    use std::collections::BTreeMap;
    use std::hash::Hash;

    use prost::bytes::Bytes;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    // Helper struct for serializing Bytes as keys
    #[derive(Eq, Ord, PartialOrd)]
    struct BytesKey(Bytes);

    impl Hash for BytesKey {
        fn hash<H: std::hash::Hasher>(&self, state: &mut H) { self.0.as_ref().hash(state); }
    }

    impl PartialEq for BytesKey {
        fn eq(&self, other: &Self) -> bool { self.0.as_ref() == other.0.as_ref() }
    }

    impl Serialize for BytesKey {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where S: Serializer {
            ::serde_bytes::serialize(self.0.as_ref(), serializer)
        }
    }

    impl<'de> Deserialize<'de> for BytesKey {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where D: Deserializer<'de> {
            let bytes: Vec<u8> = ::serde_bytes::deserialize(deserializer)?;
            Ok(BytesKey(Bytes::from(bytes)))
        }
    }

    pub fn serialize<S>(map: &BTreeMap<Bytes, i64>, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer {
        // Convert BTreeMap<Bytes, i64> to BTreeMap<BytesKey, i64> for serialization
        let transformed_map: BTreeMap<BytesKey, i64> =
            map.iter().map(|(k, v)| (BytesKey(k.clone()), *v)).collect();

        transformed_map.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<BTreeMap<Bytes, i64>, D::Error>
    where D: Deserializer<'de> {
        // Deserialize as BTreeMap<BytesKey, i64> and convert to BTreeMap<Bytes, i64>
        let transformed_map: BTreeMap<BytesKey, i64> = BTreeMap::deserialize(deserializer)?;
        let map = transformed_map.into_iter().map(|(k, v)| (k.0, v)).collect();

        Ok(map)
    }
}

pub mod serde_hex_bytes {
    use prost::bytes::Bytes;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(bytes: &Bytes, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer {
        // Convert bytes to hex string (like Scala's PrettyPrinter.buildStringNoLimit)
        let hex_string = hex::encode(bytes.as_ref());
        hex_string.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Bytes, D::Error>
    where D: Deserializer<'de> {
        let hex_string: String = String::deserialize(deserializer)?;
        let bytes = hex::decode(&hex_string)
            .map_err(|e| serde::de::Error::custom(format!("Invalid hex string: {}", e)))?;
        Ok(Bytes::from(bytes))
    }
}

pub mod serde_hex_vec_u8 {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(bytes: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer {
        // Convert bytes to hex string (like Scala's PrettyPrinter.buildStringNoLimit)
        let hex_string = hex::encode(bytes);
        hex_string.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where D: Deserializer<'de> {
        let hex_string: String = String::deserialize(deserializer)?;
        hex::decode(&hex_string)
            .map_err(|e| serde::de::Error::custom(format!("Invalid hex string: {}", e)))
    }
}

pub mod serde_always_equal_bitset {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(_: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer {
        // Serialize as unit (like Scala's AlwaysEqual encoder)
        serializer.serialize_unit()
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where D: Deserializer<'de> {
        // Deserialize unit and return empty BitSet
        let _: () = <()>::deserialize(deserializer)?;
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use prost::bytes::Bytes;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct BytesWrapper(#[serde(with = "super::serde_bytes")] Bytes);

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct VecBytesWrapper(#[serde(with = "super::serde_vec_bytes")] Vec<Bytes>);

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct MapWrapper(#[serde(with = "super::serde_btreemap_bytes_i64")] BTreeMap<Bytes, i64>);

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct HexBytesWrapper(#[serde(with = "super::serde_hex_bytes")] Bytes);

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct HexVecWrapper(#[serde(with = "super::serde_hex_vec_u8")] Vec<u8>);

    #[derive(Debug, Serialize, Deserialize)]
    struct BitsetWrapper(#[serde(with = "super::serde_always_equal_bitset")] Vec<u8>);

    #[test]
    fn serde_bytes_round_trips_through_bincode() {
        let original = BytesWrapper(Bytes::from(vec![0u8, 1, 254, 255]));
        let encoded = bincode::serialize(&original).unwrap();
        let decoded: BytesWrapper = bincode::deserialize(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn serde_vec_bytes_round_trips_through_bincode() {
        let original = VecBytesWrapper(vec![
            Bytes::from_static(b"first"),
            Bytes::new(),
            Bytes::from_static(b"third"),
        ]);
        let encoded = bincode::serialize(&original).unwrap();
        let decoded: VecBytesWrapper = bincode::deserialize(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn serde_btreemap_bytes_i64_round_trips_through_bincode() {
        let mut map = BTreeMap::new();
        map.insert(Bytes::from_static(b"alpha"), -1i64);
        map.insert(Bytes::from_static(b"beta"), i64::MAX);
        map.insert(Bytes::new(), 0);
        let original = MapWrapper(map);
        let encoded = bincode::serialize(&original).unwrap();
        let decoded: MapWrapper = bincode::deserialize(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn serde_hex_bytes_encodes_as_hex_string_and_round_trips() {
        let original = HexBytesWrapper(Bytes::from(vec![0xde, 0xad, 0xbe, 0xef]));
        let json = serde_json::to_string(&original).unwrap();
        assert_eq!(json, "\"deadbeef\"");
        let decoded: HexBytesWrapper = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn serde_hex_bytes_rejects_invalid_hex() {
        let result: Result<HexBytesWrapper, _> = serde_json::from_str("\"zzzz\"");
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid hex string"), "{err}");
    }

    #[test]
    fn serde_hex_vec_u8_encodes_as_hex_string_and_round_trips() {
        let original = HexVecWrapper(vec![0x00, 0xff, 0x10]);
        let json = serde_json::to_string(&original).unwrap();
        assert_eq!(json, "\"00ff10\"");
        let decoded: HexVecWrapper = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn serde_hex_vec_u8_rejects_invalid_hex() {
        let result: Result<HexVecWrapper, _> = serde_json::from_str("\"abc\"");
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid hex string"), "{err}");
    }

    #[test]
    fn serde_always_equal_bitset_erases_content() {
        let full = BitsetWrapper(vec![1, 2, 3]);
        let empty = BitsetWrapper(Vec::new());
        let encoded_full = bincode::serialize(&full).unwrap();
        let encoded_empty = bincode::serialize(&empty).unwrap();
        assert_eq!(encoded_full, encoded_empty);

        let decoded: BitsetWrapper = bincode::deserialize(&encoded_full).unwrap();
        assert!(decoded.0.is_empty());
    }
}
