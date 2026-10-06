use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use casper_soak::{
    array, artifact, encoded, exclusive, hash, number, parse, regular, text, MAX_BYTES,
};
use comm::rust::peer_node::PeerNode;
use comm::rust::rp::protocol_helper;
use comm::rust::transport::grpc_transport_client::GrpcTransportClient;
use comm::rust::transport::transport_layer::TransportLayer;
use eyre::{ensure, Result};
use models::casper::BlockMessageProto;
use models::routing::{Packet, Protocol};
use prost::Message;
use serde_json::{json, Value};
use tokio::sync::Mutex;

pub struct Prepared {
    peer: PeerNode,
    network: String,
    certificate: String,
    key: String,
    timeout: Duration,
    packets: Vec<(Protocol, Value)>,
}

pub fn prepare(request: &Value) -> Result<Prepared> {
    ensure!(request["schema_version"] == 1, "The step schema differs.");
    let operation = text(&request["step"]["operation"])?;
    ensure!(
        [
            "load_fixture",
            "replay_fixture",
            "load_fixture_with_missing_dependencies",
            "load_justification_fixture",
            "evaluate",
            "await_restart_receipt"
        ]
        .contains(&operation),
        "The workload operation is unsupported."
    );
    let root = Path::new(text(&request["input_root"])?);
    ensure!(root.is_absolute(), "The input root must be absolute.");
    let fixture = parse(&artifact(root, &request["inputs"]["fixture"])?)?;
    ensure!(
        fixture["schema_version"] == 1 && fixture["kind"] == "signed-block-sequence-v1",
        "The live fixture format differs."
    );
    let member = text(&request["step"]["member"]["member_id"])?;
    let config = &fixture["members"][member];
    let local = PeerNode::from_address(text(&config["local_peer"])?)?;
    let peer = PeerNode::from_address(text(&config["target_peer"])?)?;
    let network = text(&config["network_id"])?;
    ensure!(
        !network.is_empty() && network.len() <= 256,
        "The network identifier is invalid."
    );
    let timeout = number(&config["timeout_ms"])?;
    ensure!(
        (1..=30_000).contains(&timeout),
        "The transport timeout is invalid."
    );
    let references = array(&config["operations"][operation])?;
    if operation == "replay_fixture" {
        ensure!(
            config["operations"]["replay_fixture"] == config["operations"]["load_fixture"],
            "The replay block inventory differs from the loaded fixture."
        );
    }
    ensure!(
        references.len() <= 1024,
        "The block inventory exceeds its bound."
    );
    if ["evaluate", "await_restart_receipt"].contains(&operation) {
        ensure!(
            references.is_empty(),
            "A control operation cannot submit blocks."
        );
    } else {
        ensure!(!references.is_empty(), "The block inventory is empty.");
    }
    let mut total = 0usize;
    let mut packets = Vec::new();
    for reference in references {
        let content = artifact(root, reference)?;
        total += content.len();
        ensure!(
            total <= 32 * MAX_BYTES as usize,
            "The block inventory exceeds its byte bound."
        );
        let block = BlockMessageProto::decode(content.as_slice())?;
        ensure!(
            block.block_hash.len() == 32,
            "The block hash length differs."
        );
        let receipt = json!({"artifact":reference,"block_hash":block.block_hash.iter().map(|b|format!("{b:02x}")).collect::<String>()});
        let packet = protocol_helper::packet(&local, network, Packet {
            type_id: "BlockMessage".to_owned(),
            content: content.into(),
        });
        packets.push((packet, receipt));
    }
    let certificate = String::from_utf8(artifact(root, &config["certificate"])?)?;
    let key_path = Path::new(text(&config["private_key_path"])?);
    ensure!(
        key_path.is_absolute(),
        "The private key path must be absolute."
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::symlink_metadata(key_path)?;
        ensure!(
            metadata.mode() & 0o077 == 0 && metadata.uid() == unsafe { libc::geteuid() },
            "The private key permissions differ."
        );
    }
    let key = String::from_utf8(regular(key_path, 16 * 1024)?)?;
    Ok(Prepared {
        peer,
        network: network.to_owned(),
        certificate,
        key,
        timeout: Duration::from_millis(timeout),
        packets,
    })
}

pub async fn execute(bytes: &[u8], output: &Path) -> Result<Value> {
    let request = parse(bytes)?;
    ensure!(
        output.is_absolute() && output.is_dir() && !output.is_symlink(),
        "The driver output is invalid."
    );
    let deadline = casper_soak::manifest::decimal(&request["deadline_monotonic_ns"])?;
    ensure!(
        deadline > super::process::now()?,
        "The workload deadline expired."
    );
    let prepared = prepare(&request)?;
    let client = GrpcTransportClient::new(
        prepared.network,
        prepared.certificate,
        prepared.key,
        (2 * MAX_BYTES) as i32,
        64 * 1024,
        16,
        Arc::new(Mutex::new(Default::default())),
        prepared.timeout,
    )?;
    let mut deliveries = Vec::new();
    let mut failure = None;
    for (packet, mut receipt) in prepared.packets {
        let remaining = deadline
            .checked_sub(super::process::now()?)
            .filter(|n| *n > 0)
            .ok_or_else(|| eyre::eyre!("The workload deadline expired."))?;
        let result = tokio::time::timeout(
            prepared.timeout.min(Duration::from_nanos(remaining)),
            client.send(&prepared.peer, &packet),
        )
        .await;
        receipt["transport_acknowledged"] = json!(matches!(result, Ok(Ok(()))));
        if !matches!(result, Ok(Ok(()))) {
            failure = Some("The block transport failed or exceeded its deadline.");
        }
        deliveries.push(receipt);
        let transport = json!({"schema_version":1,"request_sha256":hash(bytes),"step":request["step"],"deliveries":deliveries,"failure":failure});
        let path = output.join(format!("delivery-{:04}.json", deliveries.len()));
        exclusive(&path, &encoded(&transport)?, true)?;
        if failure.is_some() {
            break;
        }
    }
    let result = json!({"schema_version":1,"request_sha256":hash(bytes),"step":request["step"],
        "status":"unknown","reason":if failure.is_some() {"transport_failed"} else {"node_input_exports_unavailable"},
        "transport":{"deliveries":deliveries,"failure":failure}});
    exclusive(&output.join("result.json"), &encoded(&result)?, true)?;
    Ok(result)
}
