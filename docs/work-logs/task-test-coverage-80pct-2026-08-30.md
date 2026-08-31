---
task: raise unit-test line coverage above 80%
branch: feature/test-coverage
status: in_progress
claimed_by: claude-session-325b268b
claimed_at: 2026-08-30T00:00:00Z
handoff_status: paused
---

# Test coverage: raise line coverage above 80%

## Baseline (CI run 33277347504, 2026-08-29)

| Crate | Lines | Covered | Line coverage |
| --- | ---: | ---: | ---: |
| rspace_plus_plus | 9021 | 6436 | 71.3% |
| rholang | 21247 | 14993 | 70.6% |
| shared | 1793 | 1069 | 59.6% |
| node | 11460 | 5848 | 51.0% |
| models | 5527 | 3167 | 57.3% |
| crypto | 1641 | 1282 | 78.1% |
| block-storage | 2999 | 1834 | 61.2% |
| comm | 6012 | 4414 | 73.4% |
| graphz | 263 | 194 | 73.8% |
| casper | 39996 | 30239 | 75.6% |
| **All** | 99959 | 69476 | 69.5% |

To reach 80% overall, about 10,500 more lines must be covered.

## Method

The per-crate lcov artifacts from the CI run identify the files with the most
uncovered lines. Work proceeds crate by crate, weakest first, with in-file
`#[cfg(test)] mod tests` unit tests. Production code is not changed.

## Session 2026-08-30 targets (phase 1)

- crypto: signed.rs (0%), signatures_alg.rs (0%), public_key.rs, sha_256.rs, certificate_helper.rs gaps
- shared: key_value_typed_store_impl.rs (0%), mod.rs serde helpers (0%), hashable_set.rs (0%), env.rs (0%), lmdb_key_value_store.rs gaps
- models: block_metadata.rs (0%), pathmap_zipper.rs (0%), casper/pretty_printer.rs (0%), utils.rs gaps, casper_message.rs round-trips
- block-storage: finality stores (0%), key_value_rejected_deploy_buffer.rs (0%), dag storage gaps

## Phase 1 progress

- shared DONE: 59.6% -> 92.2% (2248/2437), 56 tests added, `cargo test -p shared` green (103 passed). Only production-adjacent change: two existing test helpers in lmdb_key_value_store.rs made `pub(super)` for reuse. Skipped: grpc_server.rs, printer.rs, `LmdbKeyValueStore::size_bytes` (`todo!()`).
- crypto DONE: 78.1% -> 92.6% (1851/1998), 48 tests added, `cargo test -p crypto` green (93 passed). Remaining misses are fault-injection-only branches (openssl/rcgen failure arms, u64 counter overflow).
- block-storage DONE: 46 tests added, `cargo test -p block-storage` green (64 lib + 35 integration + 8 atomic-buffer + 3 loom, 0 failed). ~600-700 previously-missed lines exercised: finality stores 0% -> ~full, rejected-deploy buffer 0% -> full, metadata/equivocation stores, key_value_block_store error paths, and ~150-200 of block_dag_key_value_storage's 349 missed lines (representation navigation, insert guards, genesis register, deploy-terminal write-once, record_directly_finalized paths). Crate estimate ~85%.
- models DONE: 105 tests added, `cargo test -p models` green (120 lib passed). ~1,200-1,400 previously-missed lines exercised: the four 0% files (block_metadata, pathmap_zipper, casper/pretty_printer, most of par_to_sexpr) near end-to-end, most uncovered utils.rs constructors, several hundred casper_message.rs round-trip lines. Only non-test change: `concatenate_pars` moved verbatim above the new test mod in rholang/implicits.rs.
- rholang phase-A DONE: 85 tests added, `cargo test --release -p rholang` green (277 lib passed, was 192). ~1,350-1,450 lines: errors, rho_type, pretty_printer (exact-output over direct ASTs), substitute (both sort paths), has_locally_free, ground_normalize_matcher, openai/ollama non-network paths. reduce.rs and system_processes.rs deliberately deferred (need interpreter harness).
- rspace_plus_plus DONE: 58 tests added, measured 71.3% -> 83.2% (8234/9891 via scripts/coverage.sh). state_change 12.3% -> 87.0%, merging_logic 89.9%, event_log_index 87.4%, state_change_merger 78.2%, hot_store 90.4%, history repository/reader ~90/80%. New tests/merger/state_change_tests.rs registered; fixture helpers made pub.
- comm+graphz DONE and verified green on 72bc91a43: ~66 tests (11 graphz, ~55 comm). `cargo test -p comm` 163 lib + 218 integration + 8 more passed; `cargo test -p graphz` 22 passed. ~370 comm lines newly exercised (hostname trust manager, transport receiver circuit breaker, rp_conf/connect/protocol_helper, upnp pure helpers, ssl interceptor with real generated certs) and ~60 of graphz's 69 uncovered lines. Skipped: live-infra paths (kademlia, gateway, live gRPC channels) and rustls-private constructors.

## Phase 3 progress (2026-08-31)

- rholang deep pass DONE and measured: crate now 85.7% (19478/22737 via scripts/coverage.sh). reduce.rs 60.3% -> 71.9% (~620 lines), system_processes.rs 38.5% -> 84.3% (~590 lines; remaining misses are the network-gated AI/grpcTell bodies). 36 tests in two new integration specs: rholang/tests/reduce_deep_spec.rs (21) and rholang/tests/system_processes_deep_spec.rs (15). Full `cargo test --release -p rholang` green (277 lib + 303 integration + new binaries). Pinned semantics worth knowing: match with no matching case is a no-op, and evaluation errors roll back tuplespace effects.
- casper DONE: 27 tests, `cargo test --release -p casper` green (351 lib + 932 integration passed, 11 pre-existing ignores). ~600-700 lines newly exercised (~+1.6-1.9pp on 79.4%): block_retriever admit_hash branches + caps/eviction (12 in-file tests), running_spec message fall-throughs + floor cache (9), 17 no-casper BlockAPI error arms + preview_private_names (2), NEW tests/api/block_report_api_test.rs covering block_report_api.rs from 0% (4). Skipped with reasons: initializing/block_processor full graphs, casper_launch wiring, dag_merger/block_creator edges (need merge harnesses — candidate for a dedicated later pass).
- node DONE and measured: 51.0% -> 65.0% (8443/12990 via scripts/coverage.sh). 63 tests, ~1,030 newly-exercised production lines: web_api_routes 34.8% -> 91.6% (AppState proved stub-constructible; tower-oneshot tests over the real router), shared_handlers -> 97.1% (exhaustive error classification), deploy_grpc_service_v1 0% -> 77.7%, web_api 51.7% -> 84.6% (pure conversion layer), system_deploy_info -> 99.5%. Remaining misses are engine-bound success arms plus the skipped bootstrap/wiring files. One existing test helper made pub(super). Verified green under the CI-equivalent gate: `cargo nextest run -p node --release` = 246 passed, 0 skipped. Caveat: debug-mode `cargo test -p node --lib` shows 3 pre-existing environmental failures in rust::diagnostics::sigar_reporter timing tests on this macOS machine (fail at untouched baseline too; pass in release/nextest, the CI configuration).

## CI status note (2026-08-31)

- PR #371 (coverage tooling) MERGED to dev at 05ef40c67, BEFORE commit 72bc91a43. The branch has no open PR, so 72bc91a43 (the new tests) received no CI run; the coverage matrix runs only on pull_request events. A fresh PR feature/test-coverage -> dev is needed once the casper/node/rholang wave lands.
- The dev push run for the #371 merge (33284889953) failed on five integration legs with "self-hosted runner lost communication" — known OCI ephemeral-runner loss, not a code failure; the identical content was fully green on the PR run 33277347504. Remedy: re-run the WHOLE workflow (partial re-run skips Launch Ephemeral Runners and queues forever).

## Follow-up bugs found (not fixed here) — continued

- rholang substitute.rs (~line 707): the SORTED `SubstituteTrait<Expr>` arm for `EMinusBody` rebuilds the expression as `EPlusBody` (copy-paste); the no-sort arm is correct. Tests deliberately skip pinning the wrong output.
- rholang pretty_printer.rs (~line 827): the Match arm passes `&m.target` (`&Option<Par>`) to the Any-based printer, so every match target renders as an `<unprintable>` placeholder.

Estimated overall after shared+crypto+scaffolding exclusion: ~71.8%.

## Follow-up bugs found (not fixed here)

- `Secp256k1Eth::name()` returns `"secp256k1:eth"` but `SignaturesAlgFactory::apply` and the serde `Deserialize` impl only match `"secp256k1-eth"`; a serialized `Box<dyn SignaturesAlg>` holding `Secp256k1Eth` therefore round-trips to an error. Behavior pinned by crypto tests (`deserialize_accepts_eth_alias`); needs its own issue/fix branch.

## Remaining after phase 1 (largest gaps, for later sessions)

- casper (~9.8k uncovered): block_api.rs, block_retriever.rs, initializing.rs, running.rs, block_processor.rs; test_utils/* at 0% suggests TestNode-based tests do not run under the coverage matrix — investigate before writing new tests there
- rholang (~6.3k): reduce.rs (2131 miss), system_processes.rs, pretty_printer.rs, substitute.rs
- node (~5.6k): web_api.rs, web_api_routes.rs, shared_handlers.rs are the testable part; runtime/main/server bootstrap files (~2.3k) are likely not unit-testable
- rspace_plus_plus (~2.6k): merger/state_change.rs (12.3%), merging_logic.rs, state_change_merger.rs, event_log_index.rs
- comm (~1.6k): upnp.rs (15%), rp_conf.rs, transport files

## Decisions taken 2026-08-30 (user-directed)

1. **Scaffolding excluded from the denominator.** `scripts/coverage.sh` and the
   ci.yml Measure coverage step now pass
   `--ignore-filename-regex '(/test_utils/|block-storage/src/rust/test/)'`
   (identical in both places). Rationale: src-shipped test scaffolding is
   compiled for cross-crate test consumers, so per-crate isolation can never
   exercise it in its own crate's run. Effect on the baseline, holding tests
   constant: casper 75.6% -> 79.4%, block-storage 61.2% -> 63.3%, rholang
   70.6% -> 69.9% (its scaffolding was well covered and no longer pads the
   number), overall 69.5% -> ~70.8%.
2. **Dead casper test_utils tree deleted (dedup).** `casper/src/rust/test_utils/`
   (3,816 lines) duplicated `casper/tests/helper/` and had diverged
   (test_node.rs 614 differing lines); nothing anywhere imported
   `casper::rust::test_utils` — it was compiled via the self-dev-dep but never
   executed. The actively maintained copy in `casper/tests/helper/` survives.
   The `test-utils` feature stays, slimmed to `[]`: it still gates the
   well-known DEFAULT_SEC/DEFAULT_PUB deploy keys and the default-key fallback
   in `casper/src/rust/util/construct_deploy.rs` out of production builds, and
   node's dev-dependency still enables it (node/tests/rho_trie_traverser_test.rs
   passes `sec: None`). Its `tempfile` and `block-storage/test-internals`
   transitive enables were removed — both are direct casper dev-dependencies.
   Verified: `cargo check -p casper --tests` and `cargo check -p node --tests`
   both green.

## Notes

- Coverage measurement: `just coverage <crate...>` (scripts/coverage.sh, cargo-llvm-cov + nextest, release profile)
- The report never gates; thresholds remain a separate ratified decision
