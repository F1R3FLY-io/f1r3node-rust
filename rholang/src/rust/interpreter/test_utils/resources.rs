// See rholang/src/test/scala/coop/rchain/rholang/Resources.scala

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use rspace_plus_plus::rspace::history::history_repository::HistoryRepository;
use rspace_plus_plus::rspace::rspace::{RSpace, RSpaceStore};
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::shared::lmdb_dir_store_manager::MB;
use rspace_plus_plus::rspace::shared::rspace_store_manager::mk_rspace_store_manager;
use rspace_plus_plus::rspace::trace::Log;
use tempfile::Builder;

use crate::rust::interpreter::accounting::costs::{search_cost, Cost};
use crate::rust::interpreter::errors::InterpreterError;
use crate::rust::interpreter::external_services::ExternalServices;
use crate::rust::interpreter::interpreter::EvaluateResult;
use crate::rust::interpreter::matcher::r#match::Matcher;
use crate::rust::interpreter::merging::mergeable_tags::default_mergeable_tags;
use crate::rust::interpreter::rho_runtime;
use crate::rust::interpreter::rho_runtime::{
    create_replay_rho_runtime, create_rho_runtime, RhoRuntime, RhoRuntimeImpl,
};
use crate::rust::interpreter::system_processes::Definition;
#[cfg(feature = "chromadb")]
use crate::rust::interpreter::{ollama_service::OllamaConfig, openai_service::OpenAIConfig};

pub fn mk_temp_dir(prefix: &str) -> PathBuf {
    let temp_dir = Builder::new()
        .prefix(prefix)
        .tempdir()
        .expect("Failed to create temp dir");
    temp_dir.keep()
}

pub fn with_temp_dir<F, R>(prefix: &str, f: F) -> R
where F: FnOnce(&Path) -> R {
    let temp_dir = Builder::new()
        .prefix(prefix)
        .tempdir()
        .expect("Failed to create temp dir");

    // Run the function with the temp_dir path
    let result = f(temp_dir.path());

    // TempDir will be dropped here and automatically cleaned up
    // unless we've decided to manually persist it
    result
}

pub async fn with_runtime<F, Fut, R>(prefix: &str, f: F) -> R
where
    F: FnOnce(RhoRuntimeImpl) -> Fut,
    Fut: Future<Output = R>,
{
    let temp_dir = Builder::new()
        .prefix(prefix)
        .tempdir()
        .expect("Failed to create temp dir");

    let mut store_manager = mk_rspace_store_manager(temp_dir.path().to_path_buf(), 100 * MB);
    let rspace_store = store_manager.r_space_stores().await.unwrap();

    // `ExternalServices::for_validator` constructs a `ChromaDBClient`,
    // which builds `SBERTEmbeddings::new()`, which forces lazy-init of
    // `rust_bert::common::resources::remote::CACHE`. That cache builds a
    // `reqwest::blocking::Client`, which internally creates+drops a
    // fresh current-thread `tokio::runtime::Runtime` inside
    // `reqwest::blocking::wait::enter`. Doing that from inside our
    // outer `#[tokio::test]` async context trips tokio's
    // "Cannot drop a runtime in a context where blocking is not allowed"
    // guard at `tokio::runtime::blocking::shutdown::Receiver::wait` and
    // panics the test. `spawn_blocking` runs the construction on a
    // blocking-pool worker so the inner Runtime never touches the outer
    // async context. The expensive part (model download + reqwest
    // client build) runs unchanged; the only cost is the microsecond
    // hand-off to a blocking-pool thread, and the `lazy_static! CACHE`
    // is warm for every subsequent test in the same process.
    #[cfg(feature = "chromadb")]
    let external_services = tokio::task::spawn_blocking(|| {
        ExternalServices::for_validator(&OpenAIConfig::disabled(), &OllamaConfig::disabled())
    })
    .await
    .expect("ExternalServices::for_validator panicked during test setup");
    #[cfg(not(feature = "chromadb"))]
    let external_services = ExternalServices::noop();

    let runtime = rho_runtime::create_runtime_from_kv_store(
        rspace_store,
        Arc::new(default_mergeable_tags()),
        false,
        &mut Vec::new(),
        Arc::new(Box::new(Matcher)),
        external_services,
    )
    .await
    .unwrap();

    f(runtime).await
}

/// Replays `term` against play's `log` as casper does: evaluates it from the
/// replay runtime's current state, then adds the charge for the searches
/// play's operations paid for, recomputed from the log.
pub async fn replay_charged<R: RhoRuntime>(
    replay: &mut R,
    log: Log,
    term: &str,
    initial_phlo: Cost,
    normalizer_env: HashMap<String, Par>,
    rand: Blake2b512Random,
) -> Result<EvaluateResult, InterpreterError> {
    let term_log = 0..log.len();
    replay.rig(log).await?;
    let pre_state = replay.create_soft_checkpoint().await;
    let mut result = replay
        .evaluate(term, initial_phlo.clone(), normalizer_env, rand)
        .await?;
    if result.pays_for_search() {
        let candidates = replay.search_candidates(&pre_state, term_log).await?;
        result.charge_search(search_cost(candidates), initial_phlo.value);
    }
    Ok(result)
}

pub async fn create_runtimes(
    stores: RSpaceStore,
    init_registry: bool,
    additional_system_processes: &mut Vec<Definition>,
) -> (
    RhoRuntimeImpl,
    RhoRuntimeImpl,
    Arc<
        Box<
            dyn HistoryRepository<Par, BindPattern, ListParWithRandom, TaggedContinuation>
                + Send
                + Sync
                + 'static,
        >,
    >,
) {
    create_runtimes_with_services(
        stores,
        init_registry,
        additional_system_processes,
        ExternalServices::noop(),
    )
    .await
}

/// Create runtimes with custom external services for testing
pub async fn create_runtimes_with_services(
    stores: RSpaceStore,
    init_registry: bool,
    additional_system_processes: &mut Vec<Definition>,
    external_services: ExternalServices,
) -> (
    RhoRuntimeImpl,
    RhoRuntimeImpl,
    Arc<
        Box<
            dyn HistoryRepository<Par, BindPattern, ListParWithRandom, TaggedContinuation>
                + Send
                + Sync
                + 'static,
        >,
    >,
) {
    let hrstores =
        RSpace::<Par, BindPattern, ListParWithRandom, TaggedContinuation>::create_with_replay(
            stores,
            Arc::new(Box::new(Matcher)),
        )
        .unwrap();

    let (space, replay) = hrstores;

    let rho_runtime = create_rho_runtime(
        space.clone(),
        Arc::new(default_mergeable_tags()),
        init_registry,
        additional_system_processes,
        external_services.clone(),
    )
    .await
    .unwrap();

    let replay_rho_runtime = create_replay_rho_runtime(
        replay,
        Arc::new(default_mergeable_tags()),
        init_registry,
        additional_system_processes,
        external_services,
    )
    .await
    .unwrap();
    (
        rho_runtime,
        replay_rho_runtime,
        space.get_history_repository(),
    )
}
