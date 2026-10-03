//! REPL gRPC Service implementation
//!
//! This module provides a gRPC service for the REPL (Read-Eval-Print Loop) functionality,
//! allowing clients to execute Rholang code and receive formatted output.

use std::collections::HashMap;
use std::sync::Arc;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;

/// Protobuf message types for REPL service
pub mod repl {
    tonic::include_proto!("repl");
}

use itertools::Itertools;
use models::rhoapi::Par;
use repl::{CmdRequest, EvalRequest, ReplResponse};
#[cfg(any(test, not(feature = "mettail-frontend")))]
use rholang::rust::interpreter::accounting::costs::Cost;
#[cfg(not(feature = "mettail-frontend"))]
use rholang::rust::interpreter::compiler::compiler::Compiler;
#[cfg(not(feature = "mettail-frontend"))]
use rholang::rust::interpreter::frontend::PreparedProgram;
use rholang::rust::interpreter::interpreter::EvaluateResult;
use rholang::rust::interpreter::pretty_printer::PrettyPrinter;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};

use crate::rust::api::repl_grpc_service::repl::repl_server::Repl;

#[derive(Clone)]
pub struct ReplGrpcServiceImpl {
    runtime: Arc<tokio::sync::Mutex<RhoRuntimeImpl>>,
    #[cfg(feature = "mettail-frontend")]
    f1r3lang: Option<crate::rust::runtime::f1r3lang::F1r3langEvaluation>,
}

impl ReplGrpcServiceImpl {
    pub fn new(runtime: RhoRuntimeImpl) -> Self {
        Self {
            runtime: Arc::new(tokio::sync::Mutex::new(runtime)),
            #[cfg(feature = "mettail-frontend")]
            f1r3lang: None,
        }
    }

    #[cfg(feature = "mettail-frontend")]
    pub fn with_f1r3lang(
        runtime: RhoRuntimeImpl,
        composition: crate::rust::runtime::f1r3lang::F1r3langEvaluation,
    ) -> Self {
        Self {
            runtime: Arc::new(tokio::sync::Mutex::new(runtime)),
            f1r3lang: Some(composition),
        }
    }

    async fn execute_code(
        &self,
        source: &str,
        print_unmatched_sends_only: bool,
    ) -> eyre::Result<ReplResponse> {
        // TODO: maybe we should move this call to tokio::task::spawn_blocking if the execution will block the task for a long time
        use rholang::rust::interpreter::storage::storage_printer;

        // Serialize preparation, matcher ledger, checkpoint and execution together.
        // Separate requests must not reset one another's runtime funding/ledger.
        let mut runtime = self.runtime.lock().await;
        #[cfg(feature = "mettail-frontend")]
        let composition = match &self.f1r3lang {
            Some(composition) => composition,
            None => {
                return Ok(ReplResponse {
                    output: "Error: F1R3Lang public evaluation is not configured".into(),
                })
            }
        };

        #[cfg(feature = "mettail-frontend")]
        let preparation = {
            if source.len() > composition.max_source_bytes {
                return Ok(ReplResponse {
                    output: "Error: Rholang source byte limit exceeded".into(),
                });
            }
            let frontend = composition.frontend.clone();
            let source = source.to_owned();
            tokio::task::spawn_blocking(move || {
                rholang::rust::interpreter::frontend::prepare_program(
                    frontend.as_ref(),
                    &source,
                    HashMap::new(),
                )
            })
            .await?
        };
        #[cfg(not(feature = "mettail-frontend"))]
        let preparation = Compiler::source_to_adt_with_normalizer_env(source, HashMap::new())
            .map(PreparedProgram::from_normalized);

        // Match Scala behavior: catch compilation errors and return them as successful responses
        // with "Error: {error}" format, rather than propagating as gRPC errors
        let prepared = match preparation {
            Ok(p) => p,
            Err(e) => {
                // Return error as successful response, matching Scala's ReplGrpcService behavior
                // Scala: case _: InterpreterError => Sync[F].delay(s"Error: ${er.toString}")
                let error_msg = format!("Error: {}", e);
                return Ok(ReplResponse { output: error_msg });
            }
        };

        #[cfg(feature = "mettail-frontend")]
        if let Err(error) = composition.matcher.prepare_flt_patterns(prepared.as_par()) {
            return Ok(ReplResponse {
                output: format!("Error: {error}"),
            });
        }
        let prepared = tokio::task::spawn_blocking(move || {
            print_normalized_term(prepared.as_par());
            prepared
        })
        .await?;

        let rand = Blake2b512Random::create_from_length(10);
        #[cfg(feature = "mettail-frontend")]
        let funding = composition.funding.clone();
        #[cfg(not(feature = "mettail-frontend"))]
        let funding = Cost::unsafe_max();
        #[cfg(feature = "mettail-frontend")]
        composition.matcher.refusals().take();
        let checkpoint = runtime.create_soft_checkpoint().await;
        let evaluated = runtime.evaluate_prepared(prepared, funding, rand).await;
        let EvaluateResult {
            cost, mut errors, ..
        } = match evaluated {
            Ok(result) => result,
            Err(error) => {
                runtime.revert_to_soft_checkpoint(checkpoint).await;
                return Err(error.into());
            }
        };
        #[cfg(feature = "mettail-frontend")]
        if let Some(error) = composition.matcher.refusals().decider_gap_error() {
            errors.push(error);
        }
        if !errors.is_empty() {
            runtime.revert_to_soft_checkpoint(checkpoint).await;
        }

        let pretty_storage = if print_unmatched_sends_only {
            storage_printer::pretty_print_unmatched_sends_with_reasons(&*runtime).await
        } else {
            storage_printer::pretty_print(&*runtime).await
        };

        let error_str = if errors.is_empty() {
            String::new()
        } else {
            format!(
                "Errors received during evaluation:\n{}\n",
                errors.into_iter().map(|err| err.to_string()).join("\n")
            )
        };

        let output = format!(
            "Deployment cost: {cost:?}\n
        {error_str}Storage Contents:\n{pretty_storage}",
        );

        Ok(ReplResponse { output })
    }
}

fn print_normalized_term(normalized_term: &Par) {
    println!(
        "\nEvaluating:{}",
        PrettyPrinter::new().build_channel_string(normalized_term)
    );
}

pub fn create_repl_grpc_service(runtime: RhoRuntimeImpl) -> impl Repl {
    ReplGrpcServiceImpl::new(runtime)
}

#[cfg(all(test, feature = "mettail-frontend"))]
mod prepared_route_tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use mettail_rholang_runtime::guard_par_substrate::SubstrateGuardMatcher;
    use mettail_rholang_runtime::speculation::server::LookaheadEngine;
    use rholang::rust::interpreter::accounting::has_cost::HasCost;
    use rholang::rust::interpreter::external_services::ExternalServices;
    use rholang::rust::interpreter::frontend::{
        PreparationError, PreparedProgram, ProgramFrontend, PREPARED_PROGRAM_ABI_V1,
    };
    use rholang::rust::interpreter::rho_runtime::create_runtime_from_kv_store;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

    use super::*;

    struct CountingFrontend(Arc<AtomicUsize>);
    impl ProgramFrontend for CountingFrontend {
        fn abi_version(&self) -> u16 { PREPARED_PROGRAM_ABI_V1 }
        fn prepare(
            &self,
            source: &str,
            _: HashMap<String, Par>,
        ) -> Result<PreparedProgram, PreparationError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            if source == "refuse" {
                Err(PreparationError::new("explicit test refusal"))
            } else {
                Ok(PreparedProgram::from_normalized(Par::default()))
            }
        }
    }

    async fn runtime(matcher: SubstrateGuardMatcher) -> RhoRuntimeImpl {
        let mut stores = InMemoryStoreManager::new();
        let store = stores.r_space_stores().await.unwrap();
        create_runtime_from_kv_store(
            store,
            Arc::new(HashMap::new()),
            false,
            &mut Vec::new(),
            Arc::new(Box::new(matcher)),
            ExternalServices::noop(),
        )
        .await
    }

    #[tokio::test]
    async fn configured_route_prepares_once_and_never_reparses_the_source() {
        let matcher = SubstrateGuardMatcher::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let runtime = runtime(matcher.clone()).await;
        let lookahead = LookaheadEngine::new();
        assert!(lookahead.bind_host(runtime.cost().clone()));
        let service = ReplGrpcServiceImpl::with_f1r3lang(
            runtime,
            crate::rust::runtime::f1r3lang::F1r3langEvaluation {
                frontend: Arc::new(CountingFrontend(calls.clone())),
                max_source_bytes: 4096,
                funding: Cost::create(10_000, "test host grant"),
                matcher,
                lookahead,
            },
        );
        let result = service
            .execute_code("not valid legacy Rholang source", false)
            .await
            .unwrap();
        assert!(
            result.output.contains("Storage Contents"),
            "{}",
            result.output
        );
        assert!(
            !result.output.contains("Errors received"),
            "{}",
            result.output
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let failure = service.execute_code("refuse", false).await.unwrap();
        assert!(failure.output.contains("explicit test refusal"));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn unconfigured_route_refuses_before_any_legacy_parse() {
        let service = ReplGrpcServiceImpl::new(runtime(SubstrateGuardMatcher::new()).await);
        let result = service.execute_code("new (", false).await.unwrap();
        assert!(result.output.contains("not configured"));
    }

    async fn finite_public_service(funding: i64) -> ReplGrpcServiceImpl {
        use mettail_rholang_runtime::guard_discharge::LoweringOptions;
        use mettail_rholang_runtime::language_install::{
            EmptyRegistrySnapshot, LanguageInstallPolicy, LANGUAGE_CAPABILITY_ABI_CURRENT,
        };
        use mettail_rholang_runtime::rholang_ast::RholangPreparationPolicy;
        use mettail_rholang_runtime::{LanguageRights, RuntimePolicy};

        let mut composition = crate::rust::runtime::f1r3lang::F1r3langComposition::new(
            Arc::new(EmptyRegistrySnapshot),
            LanguageInstallPolicy::new(
                LanguageRights::native_flt_default(),
                RuntimePolicy::default(),
                LANGUAGE_CAPABILITY_ABI_CURRENT,
            ),
            RholangPreparationPolicy {
                max_source_bytes: 4096,
                max_import_entries: 16,
                max_import_nodes: 4096,
                max_import_payload_bytes: 65_536,
                max_preparation_work: 10_000_000,
                max_preparation_units: 10_000_000,
                lowering: LoweringOptions::NO_DISCHARGE,
            },
            funding,
        )
        .unwrap();
        let mut stores = InMemoryStoreManager::new();
        let runtime = create_runtime_from_kv_store(
            stores.r_space_stores().await.unwrap(),
            Arc::new(HashMap::new()),
            false,
            &mut composition.definitions,
            Arc::new(Box::new(composition.evaluation.matcher.clone())),
            ExternalServices::noop(),
        )
        .await;
        assert!(composition
            .evaluation
            .lookahead
            .bind_host(runtime.cost().clone()));
        ReplGrpcServiceImpl::with_f1r3lang(runtime, composition.evaluation)
    }

    async fn public_eval(service: &ReplGrpcServiceImpl, source: &str) -> ReplResponse {
        service
            .eval(tonic::Request::new(EvalRequest {
                program: source.into(),
                print_unmatched_sends_only: false,
                language: "rho".into(),
            }))
            .await
            .unwrap()
            .into_inner()
    }

    async fn public_values(service: &ReplGrpcServiceImpl, name: &str) -> Vec<Par> {
        let channel = models::rust::utils::new_gstring_par(name.into(), Vec::new(), false);
        service
            .runtime
            .lock()
            .await
            .get_data(&channel)
            .await
            .into_iter()
            .flat_map(|datum| datum.a.pars.clone())
            .collect()
    }

    #[tokio::test]
    async fn finite_public_funding_exhaustion_rolls_back_and_next_request_is_clean() {
        let service = finite_public_service(1).await;
        let seeded = public_eval(&service, "@\"public.keep\"!(7)").await;
        assert!(!seeded.output.contains("Error"), "{}", seeded.output);
        let expected = vec![models::rust::utils::new_gint_par(7, Vec::new(), false)];
        assert_eq!(public_values(&service, "public.keep").await, expected);

        let refused = public_eval(
            &service,
            "@\"public.partial.a\"!(1) | @\"public.partial.b\"!(2) | @\"public.partial.c\"!(3)",
        )
        .await;
        assert!(
            refused.output.contains("Errors received during evaluation"),
            "{}",
            refused.output
        );
        assert!(
            refused
                .output
                .contains("Computation ran out of phlogistons."),
            "{}",
            refused.output
        );
        {
            let runtime = service.runtime.lock().await;
            assert_eq!(runtime.cost.total_cost().value, 1);
            assert!(runtime.cost.last_oop_event().is_some());
        }
        assert_eq!(public_values(&service, "public.keep").await, expected);
        for name in ["public.partial.a", "public.partial.b", "public.partial.c"] {
            assert!(
                public_values(&service, name).await.is_empty(),
                "partial effect on {name}"
            );
        }

        let empty = public_eval(&service, "Nil").await;
        assert!(!empty.output.contains("Error"), "{}", empty.output);
        {
            let runtime = service.runtime.lock().await;
            assert_eq!(runtime.cost.total_cost().value, 0);
            assert!(runtime.cost.last_oop_event().is_none());
        }
        let valid = public_eval(&service, "@\"public.next\"!(9)").await;
        assert!(!valid.output.contains("Error"), "{}", valid.output);
        assert_eq!(public_values(&service, "public.next").await, vec![
            models::rust::utils::new_gint_par(9, Vec::new(), false),
        ]);
        assert_eq!(public_values(&service, "public.keep").await, expected);
        let runtime = service.runtime.lock().await;
        assert_eq!(runtime.cost.total_cost().value, 1);
        assert!(runtime.cost.last_oop_event().is_none());
        assert_eq!(service.f1r3lang.as_ref().unwrap().funding.value, 1);
    }

    #[tokio::test]
    async fn public_preparation_refusal_preserves_storage_and_next_request_runs() {
        let service = finite_public_service(1).await;
        let seeded = public_eval(&service, "@\"public.before\"!(11)").await;
        assert!(!seeded.output.contains("Error"), "{}", seeded.output);
        let before_cost = service.runtime.lock().await.cost.total_cost();
        let before_events = service.runtime.lock().await.get_cost_event_log();
        let expected = public_values(&service, "public.before").await;
        assert_eq!(expected, vec![models::rust::utils::new_gint_par(
            11,
            Vec::new(),
            false
        )]);

        let refused = public_eval(&service, "new (").await;
        assert!(refused.output.starts_with("Error:"), "{}", refused.output);
        assert!(
            !refused.output.contains("Deployment cost:"),
            "{}",
            refused.output
        );
        assert_eq!(public_values(&service, "public.before").await, expected);
        {
            let runtime = service.runtime.lock().await;
            assert_eq!(runtime.cost.total_cost(), before_cost);
            assert_eq!(runtime.get_cost_event_log(), before_events);
        }

        let valid = public_eval(&service, "@\"public.after\"!(12)").await;
        assert!(!valid.output.contains("Error"), "{}", valid.output);
        assert_eq!(public_values(&service, "public.before").await, expected);
        assert_eq!(public_values(&service, "public.after").await, vec![
            models::rust::utils::new_gint_par(12, Vec::new(), false),
        ]);
        let runtime = service.runtime.lock().await;
        assert_eq!(runtime.cost.total_cost().value, 1);
        assert!(runtime.cost.last_oop_event().is_none());
    }

    #[tokio::test]
    async fn actual_regex_application_uses_shared_provider_public_route() {
        use mettail_rholang_runtime::guard_discharge::LoweringOptions;
        use mettail_rholang_runtime::language_install::{
            decode_ddl_envelope, par_to_canonical_value, CanonicalValueLimits,
            EmptyRegistrySnapshot, LanguageInstallService,
        };
        use mettail_rholang_runtime::rholang_ast::RholangPreparationPolicy;
        use models::rhoapi::expr::ExprInstance;
        use models::rhoapi::Expr;
        use models::rust::epathmap_trie_codec::EPathMapMode;

        use crate::rust::runtime::f1r3lang::{inline_language_install_policy, F1r3langComposition};

        fn scalar(value: ExprInstance) -> Par {
            Par::default().with_exprs(vec![Expr {
                expr_instance: Some(value),
            }])
        }

        fn list(value: &Par) -> &[Par] {
            assert_eq!(
                value.exprs.len(),
                1,
                "expected one list expression: {value:?}"
            );
            match &value.exprs[0].expr_instance {
                Some(ExprInstance::EListBody(list)) => &list.ps,
                other => panic!("expected an observation list, got {other:?}"),
            }
        }

        async fn observation(runtime: &RhoRuntimeImpl, name: &str) -> Par {
            let channel = scalar(ExprInstance::GString(name.to_owned()));
            let data = runtime.get_data(&channel).await;
            assert_eq!(data.len(), 1, "expected exactly one observation on {name}");
            assert_eq!(data[0].a.pars.len(), 1, "expected one payload on {name}");
            data[0].a.pars[0].clone()
        }

        fn assert_observation(value: &Par, expected_prefix: &[Par]) {
            let fields = list(value);
            assert_eq!(fields.len(), expected_prefix.len() + 2);
            assert_eq!(&fields[..expected_prefix.len()], expected_prefix);
            assert_ne!(
                fields[expected_prefix.len()],
                Par::default(),
                "missing receipt"
            );
            assert_ne!(
                fields[expected_prefix.len() + 1],
                Par::default(),
                "missing usage"
            );
        }

        fn trace_pathmap(value: &Par) -> &models::rhoapi::EPathMap {
            assert_eq!(value.exprs.len(), 1, "expected one PathMap expression");
            match &value.exprs[0].expr_instance {
                Some(ExprInstance::EPathmapBody(map)) => map,
                other => panic!("expected a trace PathMap, got {other:?}"),
            }
        }

        fn reflected_text(owner: &str, text: &str) -> Par {
            let tag =
                models::rust::rholang::implicits::GPrivateBuilder::new_par_from_string(format!(
                    "{}{}.^dynamic-text:{}",
                    mettail_rholang_runtime::REFLECTED_TERM_ABI_PREFIX,
                    owner,
                    hex::encode(text.as_bytes()),
                ));
            models::rust::utils::new_elist_par(
                vec![tag],
                Vec::new(),
                false,
                None,
                Vec::new(),
                false,
            )
        }

        fn reflected_constructor(owner: &str, label: &str, children: Vec<Par>) -> Par {
            let tag = |label: &str| {
                models::rust::rholang::implicits::GPrivateBuilder::new_par_from_string(format!(
                    "{}{}.{}",
                    mettail_rholang_runtime::REFLECTED_TERM_ABI_PREFIX,
                    owner,
                    label,
                ))
            };
            let mut elements = vec![tag(label), tag("^gnd")];
            elements.extend(children);
            models::rust::utils::new_elist_par(elements, Vec::new(), false, None, Vec::new(), false)
        }

        let mut composition = F1r3langComposition::new(
            Arc::new(EmptyRegistrySnapshot),
            inline_language_install_policy(),
            RholangPreparationPolicy {
                max_source_bytes: 1_000_000,
                max_import_entries: 16,
                max_import_nodes: 4096,
                max_import_payload_bytes: 65_536,
                max_preparation_work: 100_000_000,
                max_preparation_units: 100_000_000,
                lowering: LoweringOptions::NO_DISCHARGE,
            },
            100_000_000,
        )
        .unwrap();
        let declaration_source = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../mettail-module-dev/mettail-rust/rholang-runtime/tests/fixtures/regex_gslt.rho"
        ));
        let declaration = composition
            .evaluation
            .frontend
            .prepare(declaration_source, HashMap::new())
            .expect("independent expected declaration uses the checked frontend");
        let declaration =
            par_to_canonical_value(declaration.as_par(), CanonicalValueLimits::default())
                .expect("independent expected declaration is a canonical value");
        let expected_installer = LanguageInstallService::new(
            Arc::new(EmptyRegistrySnapshot),
            inline_language_install_policy(),
        );
        let expected_installation = expected_installer
            .install_all(decode_ddl_envelope(declaration).expect("canonical declaration envelope"))
            .expect("independent expected declaration installs");
        assert_eq!(expected_installation.exports.len(), 1);
        assert_eq!(expected_installation.exports[0].name, "Regex");
        let expected_owner = format!(
            "mettail-grammar-core-v1:{}",
            hex::encode(expected_installation.exports[0].receipt.fingerprint),
        );
        let mut stores = InMemoryStoreManager::new();
        let store = stores.eval_stores().await.unwrap();
        let mut runtime = create_runtime_from_kv_store(
            store,
            Arc::new(HashMap::new()),
            false,
            &mut composition.definitions,
            Arc::new(Box::new(composition.evaluation.matcher.clone())),
            ExternalServices::noop(),
        )
        .await;
        crate::rust::runtime::f1r3lang::ensure_eval_registry(&mut runtime)
            .await
            .expect("the production eval registry installs");
        assert!(composition
            .evaluation
            .lookahead
            .bind_host(runtime.cost().clone()));
        let service = ReplGrpcServiceImpl::with_f1r3lang(runtime, composition.evaluation);
        let source = include_str!(concat!(env!("CARGO_MANIFEST_DIR"),
            "/../../mettail-module-dev/mettail-rust/rholang-runtime/tests/fixtures/regex_gslt_application.rho"));
        let response = service
            .eval(tonic::Request::new(EvalRequest {
                program: source.into(),
                print_unmatched_sends_only: false,
                language: "rho".into(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(
            response.output.contains("Storage Contents"),
            "{}",
            response.output
        );
        assert!(
            !response.output.contains("Errors received"),
            "{}",
            response.output
        );
        let runtime = service.runtime.lock().await;
        let nullable_channel = scalar(ExprInstance::GString("regex.nullable".into()));
        assert!(
            !runtime.get_data(&nullable_channel).await.is_empty(),
            "Regex application produced no nullable observation; storage:\n{}",
            response.output
        );
        for channel in ["regex.nullable", "regex.match"] {
            assert_observation(&observation(&runtime, channel).await, &[scalar(
                ExprInstance::GBool(true),
            )]);
        }
        let success = observation(&runtime, "regex.lookahead.success").await;
        let success = trace_pathmap(&success);
        assert_eq!(success.mode(), EPathMapMode::Set);
        assert_eq!(success.len(), 1, "nullable(a?) must have one success trace");
        let first_key = success.next_value_key(&[]).expect("success trace key");
        let trace = success
            .leaf_at_encoded_key(&first_key)
            .expect("success trace member");
        let trace = list(&trace);
        assert!(trace.len() >= 2, "lookahead must record a guest transition");
        let expected_terminal =
            reflected_constructor(&expected_owner, "DoneBool", vec![reflected_constructor(
                &expected_owner,
                "BTrue",
                Vec::new(),
            )]);
        assert_ne!(trace.first(), trace.last(), "nullable(a?) must reduce");
        assert_eq!(
            trace.last(),
            Some(&expected_terminal),
            "nullable(a?) must produce doneBool(yes)"
        );
        let failure = observation(&runtime, "regex.lookahead.failure").await;
        let failure = trace_pathmap(&failure);
        assert_eq!(failure.mode(), EPathMapMode::Empty);
        assert_eq!(failure.len(), 0);
        let choices = observation(&runtime, "regex.lookahead.choices.success").await;
        let choices = trace_pathmap(&choices);
        assert_eq!(choices.mode(), EPathMapMode::Set);
        assert_eq!(
            choices.len(),
            2,
            "both installed rewrite choices must survive"
        );
        let first_choice = choices.next_value_key(&[]).expect("first choice trace key");
        let second_choice = choices
            .next_value_key(&first_choice)
            .expect("second choice trace key");
        assert_ne!(
            first_choice, second_choice,
            "proof-distinct traces need distinct keys"
        );
        assert!(choices.next_value_key(&second_choice).is_none());
        for key in [&first_choice, &second_choice] {
            let trace = choices
                .leaf_at_encoded_key(key)
                .expect("choice trace member");
            let trace = list(&trace);
            assert!(trace.len() >= 2, "each choice must include a guest rewrite");
            assert_eq!(trace.last(), Some(&expected_terminal));
        }
        let choice_failure = observation(&runtime, "regex.lookahead.choices.failure").await;
        let choice_failure = trace_pathmap(&choice_failure);
        assert_eq!(choice_failure.mode(), EPathMapMode::Empty);
        assert_eq!(choice_failure.len(), 0);
        let twins = observation(&runtime, "regex.lookahead.twins.success").await;
        let twins = trace_pathmap(&twins);
        assert_eq!(twins.mode(), EPathMapMode::Set);
        assert_eq!(
            twins.len(),
            2,
            "equal-target rewrite proofs must not coalesce"
        );
        let first_twin_key = twins.next_value_key(&[]).expect("first twin proof key");
        let second_twin_key = twins
            .next_value_key(&first_twin_key)
            .expect("second twin proof key");
        assert_ne!(first_twin_key, second_twin_key);
        assert!(twins.next_value_key(&second_twin_key).is_none());
        let first_twin = twins
            .leaf_at_encoded_key(&first_twin_key)
            .expect("first twin proof trace");
        let second_twin = twins
            .leaf_at_encoded_key(&second_twin_key)
            .expect("second twin proof trace");
        let first_twin = list(&first_twin);
        let second_twin = list(&second_twin);
        assert_ne!(first_twin, second_twin, "proof links must remain distinct");
        assert_eq!(first_twin.first(), second_twin.first());
        assert_eq!(first_twin.last(), Some(&expected_terminal));
        assert_eq!(second_twin.last(), Some(&expected_terminal));
        let twin_failure = observation(&runtime, "regex.lookahead.twins.failure").await;
        assert_eq!(trace_pathmap(&twin_failure).len(), 0);
        let hole_success = observation(&runtime, "regex.lookahead.hole.success").await;
        let hole_success = trace_pathmap(&hole_success);
        assert_eq!(hole_success.mode(), EPathMapMode::Set);
        assert_eq!(hole_success.len(), 1, "received Pattern hole must reduce");
        let hole_key = hole_success
            .next_value_key(&[])
            .expect("received-hole success key");
        let hole_trace = hole_success
            .leaf_at_encoded_key(&hole_key)
            .expect("received-hole trace");
        let hole_trace = list(&hole_trace);
        assert!(hole_trace.len() >= 2);
        assert_eq!(hole_trace.last(), Some(&expected_terminal));
        let hole_failure = observation(&runtime, "regex.lookahead.hole.failure").await;
        assert_eq!(trace_pathmap(&hole_failure).len(), 0);
        let zero_success = observation(&runtime, "regex.lookahead.zero.success").await;
        let zero_success = trace_pathmap(&zero_success);
        assert_eq!(zero_success.mode(), EPathMapMode::Empty);
        assert_eq!(zero_success.len(), 0, "[0] cannot fire a guest rewrite");
        let zero_failure = observation(&runtime, "regex.lookahead.zero.failure").await;
        assert_eq!(trace_pathmap(&zero_failure).len(), 0);

        let one_success = observation(&runtime, "regex.lookahead.one.success").await;
        let one_success = trace_pathmap(&one_success);
        assert_eq!(one_success.mode(), EPathMapMode::Set);
        assert_eq!(one_success.len(), 1, "[1] completes only ForkYesDirect");
        let one_key = one_success
            .next_value_key(&[])
            .expect("one-step success key");
        let one_trace = one_success
            .leaf_at_encoded_key(&one_key)
            .expect("one-step success trace");
        let one_trace = list(&one_trace);
        assert!(one_trace.len() >= 2);
        assert_eq!(one_trace.last(), Some(&expected_terminal));
        let one_failure = observation(&runtime, "regex.lookahead.one.failure").await;
        assert_eq!(trace_pathmap(&one_failure).len(), 0);
        let truncated_channel = scalar(ExprInstance::GString(
            mettail_rholang_runtime::lookahead::SPEC_TRUNCATED_V2_CHANNEL.into(),
        ));
        let reports = runtime.get_data(&truncated_channel).await;
        assert_eq!(
            reports.len(),
            6,
            "each v2 lookahead request has a typed report"
        );
        let mut truncated_lengths = Vec::with_capacity(reports.len());
        for report in reports {
            assert_eq!(report.a.pars.len(), 1);
            let fields = list(&report.a.pars[0]);
            assert_eq!(fields.len(), 2);
            assert_eq!(fields[0], scalar(ExprInstance::GInt(2)));
            truncated_lengths.push(trace_pathmap(&fields[1]).len());
        }
        truncated_lengths.sort_unstable();
        assert_eq!(truncated_lengths, [0, 0, 0, 0, 1, 1]);
        let derivative = observation(&runtime, "regex.derivative").await;
        let derivative_fields = list(&derivative);
        assert_eq!(derivative_fields.len(), 3);
        assert!(derivative_fields
            .iter()
            .all(|field| field != &Par::default()));
        assert_observation(&observation(&runtime, "regex.search").await, &[
            scalar(ExprInstance::GInt(2)),
            scalar(ExprInstance::GInt(6)),
            reflected_text(&expected_owner, "λλ"),
        ]);
        for (channel, expected) in [("regex.replace-first", "xba"), ("regex.replace", "xbx")] {
            assert_observation(&observation(&runtime, channel).await, &[reflected_text(
                &expected_owner,
                expected,
            )]);
        }
        assert_eq!(
            observation(&runtime, "regex.guard").await,
            scalar(ExprInstance::GString("abcb".into())),
        );
        assert_eq!(
            observation(&runtime, "regex.guard.input").await,
            scalar(ExprInstance::GString("ax".into())),
        );
        assert_eq!(
            observation(&runtime, "regex.guard.conjunction").await,
            scalar(ExprInstance::GString("abcb".into())),
        );
        assert_eq!(
            observation(&runtime, "regex.guard.conjunction.input").await,
            scalar(ExprInstance::GString("ax".into())),
        );
        assert_eq!(
            observation(&runtime, "regex.guard.disjunction").await,
            scalar(ExprInstance::GString("ax".into())),
        );
    }

    #[test]
    fn nonactivated_public_source_routes_fail_closed() {
        assert!(crate::rust::runtime::require_legacy_source_route("Signed deploy").is_err());
        assert!(crate::rust::runtime::require_legacy_source_route("LSP validation").is_err());
    }
}

#[async_trait::async_trait]
impl Repl for ReplGrpcServiceImpl {
    async fn run(
        &self,
        request: tonic::Request<CmdRequest>,
    ) -> Result<tonic::Response<ReplResponse>, tonic::Status> {
        let cmd_request = request.into_inner();
        let response = self
            .execute_code(&cmd_request.line, false)
            .await
            .map_err(|e| tonic::Status::internal(e.to_string()))?;

        Ok(tonic::Response::new(response))
    }

    async fn eval(
        &self,
        request: tonic::Request<EvalRequest>,
    ) -> Result<tonic::Response<ReplResponse>, tonic::Status> {
        let eval_request = request.into_inner();
        #[cfg(feature = "mettail-frontend")]
        if !matches!(
            eval_request.language.as_str(),
            "" | "rho" | "rholang" | "f1r3lang"
        ) {
            return Err(tonic::Status::invalid_argument(
                "Unsupported evaluation language",
            ));
        }
        let response = self
            .execute_code(
                &eval_request.program,
                eval_request.print_unmatched_sends_only,
            )
            .await
            .map_err(|e| tonic::Status::internal(e.to_string()))?;
        Ok(tonic::Response::new(response))
    }
}

#[cfg(all(test, not(feature = "mettail-frontend")))]
mod tests {
    use std::sync::Arc;

    use rholang::rust::interpreter::external_services::ExternalServices;
    use rholang::rust::interpreter::matcher::r#match::Matcher;
    use rholang::rust::interpreter::rho_runtime::create_runtime_from_kv_store;
    use rholang::rust::interpreter::system_processes::test_framework_contracts;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

    use super::*;

    async fn create_test_runtime_with_stdout() -> RhoRuntimeImpl {
        let mut kvm = InMemoryStoreManager::new();
        let store = kvm.r_space_stores().await.unwrap();
        let runtime = create_runtime_from_kv_store(
            store,
            Arc::new(std::collections::HashMap::new()),
            true,
            &mut test_framework_contracts(),
            Arc::new(Box::new(Matcher)),
            ExternalServices::noop(),
        )
        .await;

        runtime
    }

    #[tokio::test]
    async fn test_repl_service_run() {
        let runtime = create_test_runtime_with_stdout().await;
        let service = ReplGrpcServiceImpl::new(runtime);
        let request = tonic::Request::new(CmdRequest {
            line: "1 + 1".to_string(),
        });

        let result = service.run(request).await;
        assert!(result.is_ok());
        let response = result.unwrap().into_inner();
        assert!(response.output.contains("Storage Contents"));
    }

    #[tokio::test]
    async fn test_repl_service_eval() {
        let runtime = create_test_runtime_with_stdout().await;
        let service = ReplGrpcServiceImpl::new(runtime);
        let request = tonic::Request::new(EvalRequest {
            program: "1 + 1".to_string(),
            print_unmatched_sends_only: true,
            language: "rho".to_string(),
        });

        let result = service.eval(request).await;
        assert!(result.is_ok());

        let response = result.unwrap().into_inner();
        assert!(response.output.contains("Storage Contents"));
        assert!(response.output.contains("Resting diagnostics:"));
    }
}
