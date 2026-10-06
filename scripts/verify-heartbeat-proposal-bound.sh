#!/usr/bin/env bash
set -euo pipefail

artifact=${1:?artifact path is required}
[ "$artifact" = "node/src/rust/consensus/casper/instances/heartbeat_proposer.rs" ]

cargo test --release -p node heartbeat_proposer::tests::
