use std::time::Duration;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use proptest::prelude::*;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

use super::*;
use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::compiler::compiler::Compiler;
use crate::rust::interpreter::execution_space::{ConsumeResult, ProduceResult};
use crate::rust::interpreter::matcher::r#match::Matcher;
use crate::rust::interpreter::reduce::ReducerCore;

struct RejectedJoinReads {
    inner: ExecutionSpace,
    reject: BTreeSet<Par>,
}

#[async_trait]
impl ExecutionBackend for RejectedJoinReads {
    async fn consume(
        &self,
        channels: Vec<Par>,
        patterns: Vec<BindPattern>,
        continuation: TaggedContinuation,
        persistent: bool,
        peeks: BTreeSet<i32>,
    ) -> Result<ConsumeResult, RSpaceError> {
        self.inner
            .consume(channels, patterns, continuation, persistent, peeks)
            .await
    }

    async fn produce(
        &self,
        channel: Par,
        data: ListParWithRandom,
        persistent: bool,
    ) -> Result<ProduceResult, RSpaceError> {
        self.inner.produce(channel, data, persistent).await
    }

    async fn get_joins(&self, channel: Par) -> Result<Vec<Vec<Par>>, RSpaceError> {
        if self.reject.contains(&channel) {
            Err(RSpaceError::HostWorkRejected)
        } else {
            self.inner.get_joins(channel).await
        }
    }

    async fn is_replay(&self) -> bool { self.inner.is_replay().await }

    async fn update_produce(
        &self,
        original: &Produce,
        updated: Produce,
    ) -> Result<(), RSpaceError> {
        self.inner.update_produce(original, updated).await
    }
}

async fn rejected_frontier(rejections: &[bool]) {
    let mut stores = InMemoryStoreManager::new();
    let (play, _) = RSpace::create_with_replay(
        stores.r_space_stores().await.unwrap(),
        Arc::new(Box::new(Matcher)),
    )
    .unwrap();
    let raw: RhoISpace = Arc::new(Box::new(play.clone()));
    let channel = |index: usize| models::rust::utils::new_gint_par(index as i64, Vec::new(), false);
    let space = scheduled_execution(ExecutionSpace::new(RejectedJoinReads {
        inner: raw.into(),
        reject: rejections
            .iter()
            .enumerate()
            .filter(|(_, reject)| **reject)
            .map(|(index, _)| channel(index))
            .collect(),
    }));
    let coordinator = ReductionCoordinator::default();
    let core = ReducerCore::new(
        space,
        Arc::new(HashMap::new()),
        Arc::new(RwLock::new(HashMap::new())),
        Arc::new(HashMap::new()),
        RuntimeBudget::new(Cost::unsafe_max()),
        coordinator.clone(),
    );
    let source = (0..rejections.len())
        .map(|index| format!("@{index}!(1)"))
        .collect::<Vec<_>>()
        .join(" | ");
    let parsed = Compiler::source_to_adt(&source).unwrap();
    let (result, failures) = tokio::time::timeout(
        Duration::from_secs(5),
        core.inj_with_observation(
            parsed,
            Blake2b512Random::create_from_bytes(b"rejected-frontier"),
            None,
        ),
    )
    .await
    .expect("every prepared intent must receive a completion");
    if rejections.iter().any(|reject| *reject) {
        assert_eq!(result, Err(InterpreterError::HostWorkRejected));
        assert!(!failures.permits_retained_charge());
    } else {
        result.unwrap();
    }
    for (index, rejected) in rejections.iter().enumerate() {
        assert_eq!(
            play.get_data(&channel(index)).await.len(),
            usize::from(!rejected)
        );
    }
    let boundary = tokio::time::timeout(Duration::from_secs(5), coordinator.enter_boundary())
        .await
        .expect("rejected preparation must release the evaluation boundary");
    drop(boundary);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejected_join_reads_release_mixed_and_fully_rejected_frontiers() {
    for rejected in [[false, false], [false, true], [true, false], [true, true]] {
        rejected_frontier(&rejected).await;
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn generated_rejected_frontiers_preserve_exact_successful_effects(rejected in prop::collection::vec(any::<bool>(), 2..10)) {
        tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap().block_on(rejected_frontier(&rejected));
    }
}
