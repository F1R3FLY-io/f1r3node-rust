use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use casper::rust::errors::CasperError;
use casper::rust::finality::floor::{Floor, FloorOfView};
use casper::rust::finality::floor_runner::{Advance, Budget, FloorCursor};
use casper::rust::safety::clique_oracle::FtThreshold;
use shared::rust::dag::observation_work::NoopWork;

pub async fn run_cursor(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    current: &Floor,
    ftt: FtThreshold,
    budget: Budget,
) -> (Result<FloorOfView, CasperError>, u32) {
    let mut cursor = match FloorCursor::new(dag.clone(), current.clone(), ftt) {
        Ok(cursor) => cursor,
        Err(error) => return (Err(error), 0),
    };
    let mut resumptions = 0u32;
    loop {
        match cursor.advance(&NoopWork, block_store, budget).await {
            Advance::Ready(result) => return (result, resumptions),
            Advance::Yielded => resumptions += 1,
        }
    }
}

pub async fn floor_of_view(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    current: &Floor,
    ftt: FtThreshold,
) -> Result<FloorOfView, CasperError> {
    let reference =
        casper::rust::finality::floor::floor_of_view(dag, block_store, current, ftt).await;
    for budget in [Budget::Steps(1), Budget::Steps(4), Budget::Unlimited] {
        let (resumable, _) = run_cursor(dag, block_store, current, ftt, budget).await;
        assert_eq!(
            format!("{resumable:?}"),
            format!("{reference:?}"),
            "the resumable cursor must agree with the reference derivation at {budget:?}"
        );
    }
    reference
}
