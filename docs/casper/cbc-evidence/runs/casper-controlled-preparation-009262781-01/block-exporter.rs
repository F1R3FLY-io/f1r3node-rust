use std::collections::BTreeMap;
use std::path::PathBuf;

use casper::rust::storage::rnode_key_value_store_manager::rnode_db_mapping;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::shared::lmdb_dir_store_manager::LmdbDirStoreManager;
use sha2::{Digest, Sha256};
use prost::Message;
use models::casper::BlockMessageProto;

fn decompress(stored: &[u8]) -> Vec<u8> {
    let mut cursor = stored;
    let len = prost::encoding::decode_varint(&mut cursor).expect("varint length") as usize;
    lz4_flex::decompress(cursor, len).expect("lz4 block")
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let data = PathBuf::from(&args[1]);
    let out = PathBuf::from(&args[2]);
    std::fs::create_dir_all(&out).unwrap();
    let mapping = rnode_db_mapping(None).into_iter().collect();
    let mut manager = LmdbDirStoreManager::new(data, mapping);
    let store = manager.store("blocks".to_string()).await.expect("blocks store");
    let map = store.to_map().expect("block map");
    let mut manifest = BTreeMap::new();
    for (key, value) in map {
        let raw = decompress(&value);
        let hash = hex::encode(&key);
        let digest = hex::encode(Sha256::digest(&raw));
        let name = format!("{hash}.pb");
        std::fs::write(out.join(&name), &raw).unwrap();
        let proto = BlockMessageProto::decode(raw.as_slice()).expect("block proto");
        let header = proto.header.as_ref();
        let body = proto.body.as_ref();
        let number = body.and_then(|b| b.state.as_ref()).map(|s| s.block_number).unwrap_or(-1);
        let parents: Vec<String> = header.map(|h| h.parents_hash_list.iter().map(hex::encode).collect()).unwrap_or_default();
        let deploys = body.map(|b| b.deploys.len()).unwrap_or(0);
        manifest.insert(hash, serde_json::json!({"path": name, "bytes": raw.len(), "sha256": digest, "block_number": number, "seq_num": proto.seq_num, "sender": hex::encode(&proto.sender), "parents": parents, "deploys": deploys}));
    }
    std::fs::write(out.join("blocks.json"), serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
    println!("exported {} blocks", manifest.len());
    manager.shutdown().await.expect("shutdown");
}
