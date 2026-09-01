// See casper/src/test/scala/coop/rchain/casper/helper/RhoSpec.scala

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use casper::rust::genesis::genesis::Genesis;
use casper::rust::helper::test_result_collector::{
    RhoTestAssertion, TestResult, TestResultCollector,
};
use casper::rust::util::rholang::tools::Tools;
use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signed::Signed;
use models::rhoapi::{BindPattern, ListParWithRandom, TaggedContinuation};
use models::rust::casper::protocol::casper_message::DeployData;
use rholang::rust::build::compile_rholang_source::CompiledRholangSource;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::pretty_printer::PrettyPrinter;
use rholang::rust::interpreter::rho_runtime::{create_runtime_from_kv_store, RhoRuntime};
use rholang::rust::interpreter::system_processes::{byte_name, Definition};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::r#match::Match;

use crate::genesis::contracts::test_util::TestUtil;
use crate::helper::{
    block_data_contract, casper_invalid_blocks_contract, deployer_id_contract, rho_logger_contract,
    secp256k1_sign_contract, sys_auth_token_contract,
};
use crate::util::genesis_builder::{GenesisBuilder, GenesisParameters};
use crate::util::rholang::resources::mk_test_rnode_store_manager_shared;

const SHARD_ID: &str = "root-shard";
const RHO_SPEC_PRIVATE_KEY: &str =
    "abaa20c1d578612b568a7c3d9b16e81c68d73b931af92cf79727e02011c558c6";
const RHO_SPEC_TIMESTAMP: i64 = 1559158671800;

pub struct RhoSpec {
    pub test_object: CompiledRholangSource,
    pub extra_non_genesis_deploys: Vec<Signed<DeployData>>,
    pub execution_timeout: Duration,
    pub genesis_parameters: GenesisParameters,
}

impl RhoSpec {
    pub fn new(
        test_object: CompiledRholangSource,
        extra_non_genesis_deploys: Vec<Signed<DeployData>>,
        execution_timeout: Duration,
    ) -> Self {
        Self {
            test_object,
            extra_non_genesis_deploys,
            execution_timeout,
            genesis_parameters: GenesisBuilder::build_genesis_parameters_with_defaults(None, None),
        }
    }

    pub fn new_with_genesis_parameters(
        test_object: CompiledRholangSource,
        extra_non_genesis_deploys: Vec<Signed<DeployData>>,
        execution_timeout: Duration,
        genesis_parameters: GenesisParameters,
    ) -> Self {
        Self {
            test_object,
            extra_non_genesis_deploys,
            execution_timeout,
            genesis_parameters,
        }
    }

    fn printer() -> PrettyPrinter { PrettyPrinter::new() }

    pub fn mk_test(&self, _test_name: &str, test_attempts: &HashMap<i64, Vec<RhoTestAssertion>>) {
        assert!(
            !test_attempts.is_empty(),
            "It doesn't make sense to have less than one attempt"
        );

        let (attempt, assertions) = test_attempts
            .iter()
            .find(|(_, assertions)| Self::has_failures(assertions))
            .map(|(k, v)| (*k, v.clone()))
            .unwrap_or_else(|| {
                let first = test_attempts.iter().next().unwrap();
                (*first.0, first.1.clone())
            });

        let clue_msg = |clue: &str| format!("{} (test attempt: {})", clue, attempt);

        let mut printer = Self::printer();

        for assertion in assertions {
            match assertion {
                RhoTestAssertion::RhoAssertEquals {
                    expected,
                    actual,
                    clue,
                    ..
                } => {
                    assert_eq!(
                        printer.build_string_from_message(&expected),
                        printer.build_string_from_message(&actual),
                        "{}",
                        clue_msg(&clue)
                    );
                    assert_eq!(expected, actual, "{}", clue_msg(&clue));
                }
                RhoTestAssertion::RhoAssertNotEquals {
                    unexpected,
                    actual,
                    clue,
                    ..
                } => {
                    assert_ne!(
                        printer.build_string_from_message(&unexpected),
                        printer.build_string_from_message(&actual),
                        "{}",
                        clue_msg(&clue)
                    );
                    assert_ne!(unexpected, actual, "{}", clue_msg(&clue));
                }
                RhoTestAssertion::RhoAssertTrue {
                    is_success, clue, ..
                } => {
                    assert!(is_success, "{}", clue_msg(&clue));
                }
            }
        }
    }

    // Note: Original Scala code has a bug here - it checks `_.isSuccess` instead of `!_.isSuccess`.
    // This was fixed in Rust to correctly return true when there are failures (not successes).
    pub fn has_failures(assertions: &[RhoTestAssertion]) -> bool {
        assertions.iter().any(|a| !a.is_success())
    }

    /// Runs the tests by executing get_results and iterating through assertions
    /// Original Scala:
    /// ```scala
    /// val result = getResults(testObject, extraNonGenesisDeploys, executionTimeout).runSyncUnsafe(Duration.Inf)
    /// result.assertions.foreach(mkTest)
    /// ```
    pub async fn run_tests(&self) -> Result<TestResult, InterpreterError> {
        let test_result_collector = Arc::new(TestResultCollector::new());

        let result = get_results(
            &self.test_object,
            &self.extra_non_genesis_deploys,
            self.execution_timeout,
            self.genesis_parameters.clone(),
            test_result_collector,
        )
        .await?;

        // Anti-vacuity guard: a suite that never reported completion or collected zero
        // assertions is a HARNESS failure, not a pass — it must never read as green. (Only the
        // framework path goes through `run_tests`; the low-level `failing_result_collector` /
        // `timeout` specs call `get_results` directly and legitimately inspect `has_finished`.)
        if !result.has_finished || result.assertions.is_empty() {
            return Err(InterpreterError::BugFoundError(format!(
                "RhoSpec suite '{}' did not complete (has_finished={}) or collected no assertions \
                 (count={}). The test runtime is likely missing genesis/registry state.",
                self.test_object.path,
                result.has_finished,
                result.assertions.len(),
            )));
        }

        // Run mkTest for each assertion
        for (test_name, test_attempts) in &result.assertions {
            self.mk_test(test_name, test_attempts);
        }

        Ok(result)
    }
}

pub fn test_framework_contracts(
    test_result_collector: Arc<TestResultCollector>,
) -> Vec<Definition> {
    vec![
        Definition {
            urn: "rho:test:assertAck".to_string(),
            fixed_channel: byte_name(101),
            arity: 5,
            body_ref: 101,
            handler: {
                let trc = test_result_collector.clone();
                Box::new(move |ctx| {
                    let trc = trc.clone();
                    Box::new(move |args| {
                        let trc = trc.clone();
                        let ctx = ctx.clone();
                        Box::pin(async move {
                            trc.handle_message(ctx, args).await;
                            Ok(vec![])
                        })
                    })
                })
            },
            remainder: None,
        },
        Definition {
            urn: "rho:test:testSuiteCompleted".to_string(),
            fixed_channel: byte_name(102),
            arity: 1,
            body_ref: 102,
            handler: {
                let trc = test_result_collector.clone();
                Box::new(move |ctx| {
                    let trc = trc.clone();
                    Box::new(move |args| {
                        let trc = trc.clone();
                        let ctx = ctx.clone();
                        Box::pin(async move {
                            trc.handle_message(ctx, args).await;
                            Ok(vec![])
                        })
                    })
                })
            },
            remainder: None,
        },
        Definition {
            urn: "rho:io:stdlog".to_string(),
            fixed_channel: byte_name(103),
            arity: 2,
            body_ref: 103,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(async move { rho_logger_contract::handle_message(ctx, args).await })
                })
            }),
            remainder: None,
        },
        Definition {
            urn: "rho:test:deployerId:make".to_string(),
            fixed_channel: byte_name(104),
            arity: 3,
            body_ref: 104,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(async move { deployer_id_contract::get(ctx, args).await })
                })
            }),
            remainder: None,
        },
        Definition {
            urn: "rho:test:crypto:secp256k1Sign".to_string(),
            fixed_channel: byte_name(105),
            arity: 3,
            body_ref: 105,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(async move { secp256k1_sign_contract::get(ctx, args).await })
                })
            }),
            remainder: None,
        },
        Definition {
            urn: "sys:test:authToken:make".to_string(),
            fixed_channel: byte_name(106),
            arity: 1,
            body_ref: 106,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(async move { sys_auth_token_contract::get(ctx, args).await })
                })
            }),
            remainder: None,
        },
        Definition {
            urn: "rho:test:block:data:set".to_string(),
            fixed_channel: byte_name(107),
            arity: 3,
            body_ref: 107,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(async move { block_data_contract::set(ctx, args).await })
                })
            }),
            remainder: None,
        },
        Definition {
            urn: "rho:test:casper:invalidBlocks:set".to_string(),
            fixed_channel: byte_name(108),
            arity: 2,
            body_ref: 108,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(async move { casper_invalid_blocks_contract::set(ctx, args).await })
                })
            }),
            remainder: None,
        },
    ]
}

pub async fn get_results(
    test_object: &CompiledRholangSource,
    other_libs: &[Signed<DeployData>],
    execution_timeout: Duration,
    genesis_parameters: GenesisParameters,
    test_result_collector: Arc<TestResultCollector>,
) -> Result<TestResult, InterpreterError> {
    let mut genesis_builder = GenesisBuilder::new();
    let genesis = genesis_builder
        .build_genesis_with_parameters(Some(genesis_parameters.clone()))
        .await
        .map_err(|e| {
            InterpreterError::BugFoundError(format!("Failed to build genesis: {:?}", e))
        })?;

    // Run the suite against the GENESIS post-state (registry, ListOps, PoS, vaults, …), not a
    // fresh empty scope. Otherwise the RhoSpec framework — whose `testSuite` contracts are gated
    // on `rho:lang:listOps` (RhoSpecContract.rho) — never installs, no assertions are collected,
    // and every genesis spec passes VACUOUSLY. Open the same shared RSpace scope genesis was
    // written into and reset the runtime to the genesis root below.
    let mut kvs_manager = mk_test_rnode_store_manager_shared(genesis.rspace_scope_id.clone());
    let r_store = kvs_manager.r_space_stores().await.map_err(|e| {
        InterpreterError::BugFoundError(format!("Failed to create RSpaceStore: {}", e))
    })?;

    // NOTE: In Scala, RSpacePlusPlus_RhoTypes.create() calls Rust via JNA, where Matcher
    // is created automatically (see rspace++/libs/rspace_rhotypes/src/lib.rs).
    // In pure Rust code (without JNA), we must create the Matcher explicitly here,
    // as RSpace::create(stores, matcher) requires it as a parameter.
    let matcher =
        Arc::new(Box::new(Matcher)
            as Box<
                dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>,
            >);

    let mut additional_system_processes = test_framework_contracts(test_result_collector.clone());

    let mut runtime = create_runtime_from_kv_store(
        r_store,
        std::sync::Arc::new(Genesis::default_mergeable_tags()),
        true,
        &mut additional_system_processes,
        matcher,
        rholang::rust::interpreter::external_services::ExternalServices::noop(),
    )
    .await;

    // Position the runtime at the genesis post-state so the standard library / registry
    // (rho:lang:listOps, rho:system:pos, rho:vault:*, …) resolve for the test suite.
    runtime
        .reset(&Blake2b256Hash::from_bytes_prost(
            &genesis.genesis_block.body.state.post_state_hash,
        ))
        .await?;

    // Shape A (Phase 0.5, 2026-08-31): project every Consensus
    // bundle entry into a per-suite staging directory whose on-disk
    // layout matches the emitter's bundle-relative canonRoot shape
    // (`/@bundle/<logical_name>`).  Registers `/@bundle/<...> →
    // <staging>/<...>` remaps on the runtime's fs_handles.
    //
    // The projection step is load-bearing: Shape A uses
    // `logical_name` (not the operator's `canon_path` filename) for
    // the emitted `rel`, so opening `<canonRoot>/<rel>` at syscall
    // time expects an on-disk file at `<resolved_root>/<leaf(
    // logical_name)>`.  Operators are free to name their staging
    // file anything (the two failing pre-Shape-A specs pair
    // `logical_name="consensus-cap"` with `canon_path=".../
    // consensus.dat"`), so we must copy the bytes to a location
    // whose filename equals the logical name.
    //
    // Oracular entries are skipped — the emitter leaves their
    // canonRoot as the absolute `canon_path` (unchanged from
    // pre-Shape-A), so fall-through in the resolver returns the
    // same path and the syscall works as before.
    let _staging_dir: Option<tempfile::TempDir> = {
        use casper::rust::genesis::contracts::fs_genesis::{
            BundleConsensusMode, BundleEntryKind, BUNDLE_ROOT_PREFIX,
        };
        use rholang::rust::interpreter::io::path::capture_root_identity;

        use crate::helper::test_node::copy_dir_recursive;
        let consensus_entries: Vec<_> = genesis_parameters
            .2
            .fs_bundle
            .iter()
            .filter(|e| e.consensus_mode == BundleConsensusMode::Consensus)
            .collect();
        if consensus_entries.is_empty() {
            None
        } else {
            let td = tempfile::Builder::new()
                .prefix("rho-spec-shape-a-staging-")
                .tempdir()
                .map_err(|e| {
                    InterpreterError::BugFoundError(format!(
                        "Failed to create Shape A staging dir: {e}"
                    ))
                })?;
            let staging_root = td.path().to_path_buf();
            for entry in consensus_entries {
                let on_disk_target = staging_root.join(&entry.logical_name);
                let logical_key = match entry.kind {
                    BundleEntryKind::File => {
                        if let Some(parent) = on_disk_target.parent() {
                            std::fs::create_dir_all(parent).map_err(|e| {
                                InterpreterError::BugFoundError(format!(
                                    "Failed to mkdir parent for {on_disk_target:?}: {e}"
                                ))
                            })?;
                        }
                        std::fs::copy(&entry.canon_path, &on_disk_target).map_err(|e| {
                            InterpreterError::BugFoundError(format!(
                                "Failed to project bundle entry '{}' from {:?} to {:?}: {e}",
                                entry.logical_name, entry.canon_path, on_disk_target
                            ))
                        })?;
                        // File-split canonRoot mirrors the emitter:
                        // parent-of-logical-name (or `/@bundle` bare
                        // for flat logical names) is what the deploy
                        // sees; the resolver joins to the parent
                        // on-disk.
                        let logical_path = std::path::Path::new(&entry.logical_name);
                        let parent_rel =
                            logical_path.parent().and_then(|p| p.to_str()).unwrap_or("");
                        if parent_rel.is_empty() {
                            std::path::PathBuf::from(BUNDLE_ROOT_PREFIX)
                        } else {
                            std::path::PathBuf::from(format!("{BUNDLE_ROOT_PREFIX}/{parent_rel}"))
                        }
                    }
                    BundleEntryKind::Dir => {
                        // Deep-copy the directory tree so the caller's
                        // Dir.openFile("child") lands on a real file
                        // under the staging tree.
                        copy_dir_recursive(&entry.canon_path, &on_disk_target).map_err(|e| {
                            InterpreterError::BugFoundError(format!(
                                "Failed to project Dir bundle entry '{}' from {:?} to {:?}: {e}",
                                entry.logical_name, entry.canon_path, on_disk_target
                            ))
                        })?;
                        std::path::PathBuf::from(format!(
                            "{BUNDLE_ROOT_PREFIX}/{}",
                            entry.logical_name
                        ))
                    }
                };
                // Register `logical_key → <staging_parent-or-dir>`
                // matching the (canonRoot, rel) split the emitter
                // produces.  For File the resolver target is the
                // parent of the on-disk file (so `safe_descend`'s
                // one-leaf walk lands on the file); for Dir the
                // target IS the dir.
                let on_disk_resolved = match entry.kind {
                    BundleEntryKind::File => on_disk_target
                        .parent()
                        .expect("staging file has parent")
                        .to_path_buf(),
                    BundleEntryKind::Dir => on_disk_target.clone(),
                };
                match capture_root_identity(&on_disk_resolved) {
                    Ok(id) => runtime.fs_handles.root_registry.register_with_remap(
                        logical_key,
                        on_disk_resolved,
                        id,
                    ),
                    Err(e) => tracing::warn!(
                        target: "f1r3fly.rho_spec.shape_a",
                        logical_key = ?entry.logical_name,
                        on_disk = ?on_disk_resolved,
                        error = %e,
                        "RhoSpec Shape A wiring: capture_root_identity failed"
                    ),
                }
            }
            Some(td)
        }
    };

    println!("Starting tests from {}", test_object.path);

    let runtime = setup_runtime(runtime, other_libs).await?;

    let rand = Blake2b512Random::create_from_length(128).split_short(1);

    // Note: tokio::time::timeout similar to Scala's `monix.eval.Task.timeout()`.
    // If test execution takes longer than `execution_timeout`, it returns a timeout error.
    match tokio::time::timeout(
        execution_timeout,
        TestUtil::eval_source(test_object, &runtime, rand),
    )
    .await
    {
        Ok(result) => result?,
        Err(_) => {
            return Err(InterpreterError::BugFoundError(format!(
                "Timeout of {:?} expired while executing test from {}",
                execution_timeout, test_object.path
            )))
        }
    }

    Ok(test_result_collector.get_result())
}

async fn setup_runtime<R: RhoRuntime>(
    runtime: R,
    extra_libs: &[Signed<DeployData>],
) -> Result<R, InterpreterError> {
    eval_deploy(&rho_spec_deploy(), &runtime).await?;

    for deploy in extra_libs {
        eval_deploy(deploy, &runtime).await?;
    }

    Ok(runtime)
}

async fn eval_deploy(
    deploy: &Signed<DeployData>,
    runtime: &impl RhoRuntime,
) -> Result<(), InterpreterError> {
    use models::rust::normalizer_env::normalizer_env_from_deploy;
    use rholang::rust::interpreter::system_processes::DeployData as SystemProcessDeployData;

    let rand = Tools::unforgeable_name_rng(&deploy.pk, deploy.data.time_stamp);
    let deploy_data = SystemProcessDeployData::from_deploy(deploy);

    runtime.set_deploy_data(deploy_data).await;

    TestUtil::eval(
        &deploy.data.term,
        runtime,
        normalizer_env_from_deploy(deploy),
        rand,
    )
    .await?;

    Ok(())
}

fn rho_spec_deploy() -> Signed<DeployData> {
    let sk_bytes = hex::decode(RHO_SPEC_PRIVATE_KEY).expect("Invalid RHO_SPEC_PRIVATE_KEY hex");
    let sk = PrivateKey::from_bytes(&sk_bytes);

    let code = crate::util::rholang::test_rho_loader::load_test_rho("RhoSpecContract.rho")
        .expect("Failed to load RhoSpecContract.rho");

    let compiled =
        CompiledRholangSource::new(code, HashMap::new(), "RhoSpecContract.rho".to_string())
            .expect("Failed to compile RhoSpecContract.rho");

    let deploy_data = DeployData {
        term: compiled.code,
        time_stamp: RHO_SPEC_TIMESTAMP,
        valid_after_block_number: 0,
        shard_id: SHARD_ID.to_string(),
        expiration_timestamp: None,
        authority_presentations: Vec::new(),
    };

    Signed::create(deploy_data, Box::new(Secp256k1), sk).expect("Failed to sign RhoSpec deploy")
}
