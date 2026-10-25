#!/usr/bin/env bash
set -euo pipefail

artifact=${1:?artifact path is required}
case "$artifact" in
    node/src/rust/instances/heartbeat_proposer.rs)
        cargo test --release -p node heartbeat_proposer
        ;;
    casper/src/rust/blocks/proposer/proposer.rs)
        cargo test --release -p casper --test mod add_block::h1_proposer_spec
        ;;
    *)
        printf 'Unsupported heartbeat verification artifact: %s\n' "$artifact" >&2
        exit 2
        ;;
esac
