use node::rust::configuration::model::ConsensusProtocol;
use node::rust::consensus::factory::ensure_available;

#[test]
fn default_protocol_matches_the_compiled_factory() {
    let result = ensure_available(ConsensusProtocol::default());
    assert_eq!(result.is_ok(), cfg!(feature = "cbc-casper"));
    if let Err(error) = result {
        assert_eq!(error.protocol, "cbc-casper");
        assert_eq!(error.feature, "cbc-casper");
    }
}

#[cfg(not(feature = "cbc-casper"))]
#[tokio::test]
async fn unavailable_protocol_fails_before_creating_node_files() {
    use comm::rust::peer_node::NodeIdentifier;
    use node::rust::configuration::NodeConf;
    use node::rust::consensus::factory::UnavailableProtocol;
    use node::rust::runtime::node_runtime::{start, NodeRuntime};

    let directory = tempfile::tempdir().unwrap();
    let mut conf: NodeConf = hocon::HoconLoader::new()
        .load_str(include_str!("../src/main/resources/defaults.conf"))
        .unwrap()
        .resolve()
        .unwrap();
    conf.storage.data_dir = directory.path().join("data");
    conf.tls.certificate_path = directory.path().join("node.crt");
    conf.tls.key_path = directory.path().join("node.key");
    let error = start(conf.clone()).await.unwrap_err();
    assert!(error.downcast_ref::<UnavailableProtocol>().is_some());
    let error = NodeRuntime::new(conf, NodeIdentifier {
        key: vec![1].into(),
    })
    .main()
    .await
    .unwrap_err();
    assert!(error.downcast_ref::<UnavailableProtocol>().is_some());
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
fn omitted_protocol_defaults_to_casper_and_unknown_protocol_is_rejected() {
    use node::rust::configuration::model::{ConsensusConf, ConsensusProtocol};
    let conf: ConsensusConf = serde_json::from_str("{}").unwrap();
    assert_eq!(conf.protocol, ConsensusProtocol::CbcCasper);
    assert!(serde_json::from_str::<ConsensusConf>(r#"{"protocol":"unknown"}"#).is_err());
}
