use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ChainManifest {
    pub protocol: String,
    pub protocol_version: u32,
    pub schema_version: u32,
    pub network: String,
    pub shard: String,
    pub genesis: String,
}
