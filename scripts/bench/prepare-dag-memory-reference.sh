#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
revision=3b5daabb
out="$root/target/dag-memory-reference-$(date -u +%Y%m%d%H%M%S)-$$"
apply=0
while (($#)); do
    case "$1" in
        --apply) apply=1; shift ;;
        --base) revision="$2"; shift 2 ;;
        --out-dir) out="$2"; shift 2 ;;
        --help|-h)
            printf '%s\n' 'Usage: prepare-dag-memory-reference.sh [--apply] [--base REVISION] [--out-dir NEW_DIRECTORY]' 'Prepares an isolated git archive of the actual pre-change implementation with identical allocator and consensus workloads, small API adapters, the fixed-time test hook and a real legacy storage constructor in TestNode resources. Saves these test adaptations as reviewable patches. It does not switch branches, create a worktree, compile, or run benchmarks. Default is a plan. Generated commands.txt gives serial builds and three repeated allocator comparisons; archive, binaries and evidence stay under target/.'
            exit 0 ;;
        *) printf 'Unknown argument: %s\n' "$1" >&2; exit 2 ;;
    esac
done
revision="$(git -C "$root" rev-parse --verify "$revision^{commit}")"
printf 'Prepare reference %s in %s\n' "$revision" "$out"
((apply)) || exit 0
[[ ! -e "$out" ]] || { printf 'Refusing existing output directory.\n' >&2; exit 2; }
out="$(realpath -m "$out")"
case "$out/" in "$root/target/"*) ;; *) printf 'Archive must remain under this checkout target/.\n' >&2; exit 2 ;; esac
mkdir -p "$out/source"
out="$(cd "$out" && pwd)"
git -C "$root" archive "$revision" | tar -x -C "$out/source"
mkdir -p "$out/source/block-storage/examples/support"
cp "$root/block-storage/examples/dag_memory.rs" "$out/source/block-storage/examples/dag_memory.rs"
cp "$root/block-storage/examples/support/counting_fixture.rs" "$out/source/block-storage/examples/support/counting_fixture.rs"
cp "$root/block-storage/examples/support/backend.rs" "$out/candidate-backend.rs"
cat >>"$out/source/block-storage/Cargo.toml" <<'TOML'

[[example]]
name = "dag_memory"
required-features = ["test-internals"]
TOML
cat >"$out/source/block-storage/examples/support/backend.rs" <<'RUST'
use std::path::PathBuf;
use block_storage::rust::dag::block_dag_key_value_storage::{BlockDagKeyValueStorage, KeyValueDagRepresentation};
use models::rust::block_hash::BlockHash;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use shared::rust::store::key_value_store::KvStoreError;

pub async fn open(manager: &mut impl KeyValueStoreManager, _window: i64) -> Result<BlockDagKeyValueStorage, KvStoreError> {
    BlockDagKeyValueStorage::new(manager).await
}
pub fn resident(dag: &KeyValueDagRepresentation) -> (usize, Option<i64>, usize) {
    (dag.dag_set.len(), dag.block_number_map.values().copied().min(), dag.child_map.values().map(|children| children.len()).sum())
}
pub fn visible(dag: &KeyValueDagRepresentation, hash: &BlockHash) -> Result<bool, KvStoreError> { Ok(dag.contains(hash)) }
pub const WINDOWED: bool = false;
pub async fn remove_new_schema(_path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    Err("the reference predates the new schema; use normal mode for latency comparisons".into())
}
RUST
mkdir -p "$out/source/casper/tests/dag_consensus_bench"
cp "$root/casper/tests/dag_consensus_bench.rs" "$out/source/casper/tests/dag_consensus_bench.rs"
cat >"$out/source/casper/tests/dag_consensus_bench/backend.rs" <<'RUST'
use crate::helper::test_node::{ShardOverrides, TestNode};
use crate::util::genesis_builder::GenesisContext;

pub async fn network(genesis: GenesisContext, _window: i64, depth: i32) -> Vec<TestNode> {
    TestNode::create_network_with_overrides(genesis, 4, ShardOverrides {
        max_parent_depth: Some(depth), finalization_rate: Some(0), ..Default::default()
    }).await.expect("benchmark network")
}
pub fn resident_blocks(node: &TestNode) -> usize {
    node.block_dag_storage.get_representation().unwrap().dag_set.len()
}
pub fn bounded_backend() -> bool { false }
RUST
creator=casper/src/rust/blocks/proposer/block_creator.rs
cp "$out/source/$creator" "$out/block_creator.original.rs"
awk '/^pub async fn create_with_progress\(/ {copy=1} copy && /^    \/\/ Sequence numbers/ {exit} copy {print}' "$root/$creator" >"$out/fixed-time-section.rs"
[[ "$(awk '/^pub async fn create_at\(/ {count++} END {print count+0}' "$out/fixed-time-section.rs")" == 1 ]]
awk 'FILENAME==ARGV[1] {section=section $0 ORS;next} /^pub async fn create_with_progress\(/ {printf "%s",section;skip=1} skip && /^    \/\/ Sequence numbers/ {skip=0} !skip {print}' "$out/fixed-time-section.rs" "$out/source/$creator" >"$out/block_creator.updated.rs"
mv "$out/block_creator.updated.rs" "$out/source/$creator"
diff -u "$out/block_creator.original.rs" "$out/source/$creator" >"$out/fixed-time-hook.patch" || [[ $? == 1 ]]
resources=casper/tests/util/rholang/resources.rs
cp "$out/source/$resources" "$out/resources.original.rs"
cat >"$out/real-storage-constructor.rs" <<'RUST'
pub async fn block_dag_storage_from_dyn(
    kvm: &mut dyn KeyValueStoreManager,
) -> Result<block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage, shared::rust::store::key_value_store::KvStoreError> {
    struct Forward<'a>(&'a mut dyn KeyValueStoreManager);
    #[async_trait::async_trait]
    impl KeyValueStoreManager for Forward<'_> {
        async fn store(&mut self, name: String) -> Result<std::sync::Arc<dyn shared::rust::store::key_value_store::KeyValueStore>, heed::Error> { self.0.store(name).await }
        async fn shutdown(&mut self) -> Result<(), heed::Error> { self.0.shutdown().await }
    }
    block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage::new(&mut Forward(kvm)).await
}

RUST
awk 'FILENAME==ARGV[1] {section=section $0 ORS;next} /^pub async fn block_dag_storage_from_dyn\(/ {printf "%s",section;skip=1} skip && /^pub async fn key_value_deploy_storage_from_dyn\(/ {skip=0} !skip {print}' "$out/real-storage-constructor.rs" "$out/source/$resources" >"$out/resources.updated.rs"
mv "$out/resources.updated.rs" "$out/source/$resources"
diff -u "$out/resources.original.rs" "$out/source/$resources" >"$out/real-storage-constructor.patch" || [[ $? == 1 ]]
printf '%s\n' "$revision" >"$out/reference-revision.txt"
git -C "$root" rev-parse HEAD >"$out/candidate-head.txt"
git -C "$root" diff --stat >"$out/candidate-diff-stat.txt"
sha256sum "$root/block-storage/examples/dag_memory.rs" "$root/block-storage/examples/support/counting_fixture.rs" "$root/block-storage/examples/support/backend.rs" "$out/source/block-storage/examples/support/backend.rs" >"$out/workload-sha256.txt"
sha256sum "$root/casper/tests/dag_consensus_bench.rs" "$root/casper/tests/dag_consensus_bench/backend.rs" "$out/source/casper/tests/dag_consensus_bench/backend.rs" "$out/fixed-time-hook.patch" "$out/real-storage-constructor.patch" >>"$out/workload-sha256.txt"
{
    printf 'rtk cargo build --release --locked --manifest-path %q -p block-storage --features test-internals --example dag_memory --target-dir %q\n' "$out/source/Cargo.toml" "$out/build"
    printf 'rtk cargo build --release --locked --manifest-path %q -p block-storage --features test-internals --example dag_memory\n' "$root/Cargo.toml"
    printf 'rtk proxy sha256sum %q %q\n' "$out/build/release/examples/dag_memory" "$root/target/release/examples/dag_memory"
    printf 'rtk proxy bash %q --reference-bin %q --candidate-bin %q --reference-revision %q --out-dir %q --apply\n' "$root/scripts/bench/run-dag-memory-comparison.sh" "$out/build/release/examples/dag_memory" "$root/target/release/examples/dag_memory" "$revision" "$out/comparison"
    printf 'rtk proxy bash %q --reference-bin %q --candidate-bin %q --comparison-dir %q --out-dir %q --apply\n' "$root/scripts/bench/run-dag-restart-comparison.sh" "$out/build/release/examples/dag_memory" "$root/target/release/examples/dag_memory" "$out/comparison" "$out/restart-comparison"
    printf 'rtk cargo test --release --locked --manifest-path %q -p casper --test dag_consensus_bench --no-run --target-dir %q\n' "$out/source/Cargo.toml" "$out/build"
    printf 'rtk cargo test --release --locked --manifest-path %q -p casper --test dag_consensus_bench --no-run\n' "$root/Cargo.toml"
    printf 'rtk proxy bash %q %q %q\n' "$root/scripts/bench/run-dag-consensus-comparison.sh" "$out/source" "$out/consensus-comparison"
} >"$out/commands.txt"
printf 'Prepared reference; review serial build/run commands in %s/commands.txt\n' "$out"
