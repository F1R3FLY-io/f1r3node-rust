#[test]
fn host_and_shared_runtime_do_not_import_native_consensus_state() {
    let sources = [
        (
            "node runtime",
            include_str!("../src/rust/runtime/node_runtime.rs"),
        ),
        (
            "application provider",
            include_str!("../src/rust/runtime/application.rs"),
        ),
        (
            "server host",
            include_str!("../src/rust/runtime/servers_instances.rs"),
        ),
        ("node setup", include_str!("../src/rust/runtime/setup.rs")),
        (
            "packet ingress",
            include_str!("../src/rust/consensus/ingress.rs"),
        ),
        (
            "manifest schema",
            include_str!("../src/rust/consensus/manifest.rs"),
        ),
        (
            "consensus API",
            include_str!("../../consensus/api/src/lib.rs"),
        ),
        (
            "consensus runtime",
            include_str!("../../consensus/runtime/src/lib.rs"),
        ),
    ];
    for (name, source) in sources {
        for forbidden in [
            "casper::",
            "EngineCell",
            "CasperLoop",
            "BlockMessage",
            "MultiParentCasper",
            "casper_launch",
            "BlockRetriever",
        ] {
            assert!(
                !source.contains(forbidden),
                "{name} still contains {forbidden}"
            );
        }
    }
}
