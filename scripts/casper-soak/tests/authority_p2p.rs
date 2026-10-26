#![cfg(all(feature = "p2p", target_os = "linux"))]

#[path = "../src/authority_p2p.rs"]
mod p2p;
#[path = "../src/authority_process.rs"]
#[allow(dead_code)]
mod process;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::time::Duration;

use casper_soak::{encoded, hash};
use comm::rust::peer_node::PeerNode;
use comm::rust::rp::rp_conf::RPConf;
use comm::rust::transport::communication_response::CommunicationResponse;
use comm::rust::transport::grpc_transport_server::{GrpcTransportServer, TransportLayerServer};
use crypto::rust::util::certificate_helper::{CertificateHelper, CertificatePrinter};
use models::casper::BlockMessageProto;
use prost::Message;
use serde_json::{json, Value};

fn cert(port: u16) -> (String, String, PeerNode) {
    let (private, public) = CertificateHelper::generate_key_pair();
    let der = CertificateHelper::generate_certificate(&private, &public).unwrap();
    let id = CertificateHelper::public_address(&public)
        .unwrap()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    (
        CertificatePrinter::print_certificate(&der),
        CertificatePrinter::print_private_key_from_secret(&private).unwrap(),
        PeerNode::from_address(&format!(
            "rnode://{id}@127.0.0.1?protocol={port}&discovery=40404"
        ))
        .unwrap(),
    )
}

fn save(root: &std::path::Path, name: &str, bytes: &[u8]) -> Value {
    fs::write(root.join(name), bytes).unwrap();
    json!({"path":name,"bytes":bytes.len(),"sha256":hash(bytes)})
}

#[tokio::test]
async fn production_transport_delivers_exact_blocks_and_rejects_a_foreign_network() {
    let root = tempfile::tempdir().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let (server_cert, server_key, remote) = cert(port);
    let received = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let messages = received.clone();
    let server = GrpcTransportServer::new(
        RPConf::new(
            remote.clone(),
            "soak-test".into(),
            None,
            Duration::from_secs(2),
            4,
            1,
        ),
        "soak-test".into(),
        port,
        server_cert,
        server_key,
        2_097_152,
        2_097_152,
        1,
    );
    let handle = server
        .handle_receive(
            Arc::new(move |protocol| {
                let messages = messages.clone();
                Box::pin(async move {
                    messages.lock().await.push(protocol);
                    Ok(CommunicationResponse::handled_without_message())
                })
            }),
            Arc::new(|_| Box::pin(async { Ok(()) })),
        )
        .await
        .unwrap();
    for _ in 0..100 {
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let (certificate, key, local) = cert(40400);
    let certificate = save(root.path(), "client.pem", certificate.as_bytes());
    fs::write(root.path().join("key.pem"), key).unwrap();
    fs::set_permissions(
        root.path().join("key.pem"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let block = BlockMessageProto {
        block_hash: vec![42; 32].into(),
        sig: vec![1, 2, 3].into(),
        ..Default::default()
    }
    .encode_to_vec();
    let block_ref = save(root.path(), "block.pb", &block);
    let mut fixture = json!({"schema_version":1,"kind":"signed-block-sequence-v1","members":{"bounded":{
        "local_peer":local.to_address(),"target_peer":remote.to_address(),"network_id":"soak-test","timeout_ms":2000,
        "certificate":certificate,"private_key_path":root.path().join("key.pem"),"operations":{"load_fixture":[block_ref]}}}});
    let fixture_ref = save(root.path(), "fixture.json", &encoded(&fixture).unwrap());
    let mut request = json!({"schema_version":1,"step":{"index":0,"operation":"load_fixture","member":{"member_id":"bounded"}},
        "input_root":root.path(),"inputs":{"fixture":fixture_ref},"deadline_monotonic_ns":(process::now().unwrap()+5_000_000_000).to_string()});
    let output = root.path().join("success");
    fs::create_dir(&output).unwrap();
    fs::write(root.path().join("request.json"), encoded(&request).unwrap()).unwrap();
    let status = tokio::process::Command::new(env!("CARGO_BIN_EXE_casper-authority-p2p"))
        .env(
            "CASPER_AUTHORITY_STEP_REQUEST",
            root.path().join("request.json"),
        )
        .env("CASPER_AUTHORITY_STEP_OUTPUT", &output)
        .status()
        .await
        .unwrap();
    assert!(status.success());
    let result = casper_soak::record(&output.join("result.json")).unwrap();
    assert_eq!(
        result["transport"]["deliveries"][0]["transport_acknowledged"],
        true
    );
    assert_eq!(result["status"], "unknown");
    for _ in 0..100 {
        if !received.lock().await.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let messages = received.lock().await;
    assert_eq!(messages.len(), 1);
    let models::routing::protocol::Message::Packet(packet) = messages[0].message.as_ref().unwrap()
    else {
        panic!("The packet is missing.")
    };
    assert_eq!(packet.type_id, "BlockMessage");
    assert_eq!(packet.content.as_ref(), block);
    drop(messages);
    fixture["members"]["bounded"]["network_id"] = "foreign".into();
    request["inputs"]["fixture"] = save(root.path(), "foreign.json", &encoded(&fixture).unwrap());
    let output = root.path().join("foreign");
    fs::create_dir(&output).unwrap();
    let result = p2p::execute(&encoded(&request).unwrap(), &output)
        .await
        .unwrap();
    assert_eq!(result["reason"], "transport_failed");
    assert_eq!(received.lock().await.len(), 1);
    fixture["members"]["bounded"]["operations"]["load_fixture"][0]["sha256"] =
        "0".repeat(64).into();
    request["inputs"]["fixture"] = save(root.path(), "changed.json", &encoded(&fixture).unwrap());
    assert!(p2p::prepare(&request).is_err());
    assert_eq!(received.lock().await.len(), 1);
    handle.abort();
}
