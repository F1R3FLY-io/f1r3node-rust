use std::borrow::Cow;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::{Deserialize, Deserializer, Serializer};

pub fn serialize<S>(bytes: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
where S: Serializer {
    serializer.serialize_str(&STANDARD.encode(bytes))
}

pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
where D: Deserializer<'de> {
    let s = <Cow<'de, str>>::deserialize(deserializer)?;
    STANDARD
        .decode(s.as_ref())
        .map_err(serde::de::Error::custom)
}

pub mod list {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(list: &[Vec<u8>], serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer {
        list.iter()
            .map(|bytes| STANDARD.encode(bytes))
            .collect::<Vec<_>>()
            .serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<Vec<u8>>, D::Error>
    where D: Deserializer<'de> {
        Vec::<String>::deserialize(deserializer)?
            .iter()
            .map(|s| STANDARD.decode(s).map_err(serde::de::Error::custom))
            .collect()
    }
}
