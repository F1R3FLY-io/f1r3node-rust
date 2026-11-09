//! Added by G3 (DR-135): every region that a native run creates records the
//! signed price of the run's contract (plan S2, R4). The price rides in the
//! stored seals, so a later owner charge reads the price of the context that
//! stored the process.

use std::collections::HashMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rust::utils::new_gstring_par;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

use super::*;
use crate::rust::interpreter::accounting::authority::is_unit_cost_signature;
use crate::rust::interpreter::rho_runtime::RhoRuntime;
use crate::rust::interpreter::test_utils::resources::create_runtimes;

/// The term covers the four places where a run creates a region: the region
/// that an unsigned send and receive open, the region of a signed term, the
/// region of a signed bind, and the seal of a datum that stays stored.
const TERM: &str = r#"
new x, y, z in {
  x!(1) | for (@n <- x) { Nil } |
  {% y!(2) | for (@m <- y) { Nil } %}[ signer ] |
  z!(3) | for ({% @k <- z %}[ binder ]) { Nil } |
  @"g3-region-price"!(4)
}
"#;

/// Plays `TERM` at `price` and returns the price of every non-Unit region of
/// every observation, and the price of every region of the stored datum.
async fn region_prices(price: u64) -> (Vec<u64>, Vec<u64>) {
    let mut manager = InMemoryStoreManager::new();
    let stores = manager.r_space_stores().await.expect("in-memory stores");
    let (mut runtime, _, _) = create_runtimes(stores, false, &mut Vec::new()).await;
    runtime
        .cost
        .set_deploy_signature_funded(b"g3-region-price", Sig::Ground(vec![9]));
    let evaluation = runtime
        .evaluate_with_native_phlo(
            TERM,
            HashMap::new(),
            Blake2b512Random::create_from_bytes(b"g3-region-price"),
            None,
            priced_config(1_000_000, [1; 4], price),
        )
        .await
        .expect("the native evaluation runs");
    assert!(evaluation.errors.is_empty(), "{:?}", evaluation.errors);
    let observed: Vec<u64> = evaluation
        .byte_observations
        .rows
        .iter()
        .flat_map(|row| row.authority.regions.iter())
        .filter(|region| {
            !is_unit_cost_signature(region.signature.as_ref().expect("a region signature"))
        })
        .map(|region| region.price)
        .collect();
    let stored = runtime
        .get_data(&new_gstring_par(
            "g3-region-price".to_string(),
            Vec::new(),
            false,
        ))
        .await;
    assert_eq!(stored.len(), 1, "the datum stays stored");
    let stored = stored[0]
        .a
        .cost_authority
        .as_ref()
        .expect("the stored datum has a seal")
        .regions
        .iter()
        .map(|region| region.price)
        .collect();
    (observed, stored)
}

#[tokio::test]
async fn a_region_records_the_price_of_its_creating_context() {
    for price in [0, 7, 1 << 40] {
        let (observed, stored) = region_prices(price).await;
        assert!(
            observed.len() >= 8,
            "the run observes the regions of every surface, got {}",
            observed.len()
        );
        assert!(
            observed.iter().all(|region| *region == price),
            "price {price}: observed region prices {observed:?}"
        );
        assert!(!stored.is_empty());
        assert!(
            stored.iter().all(|region| *region == price),
            "price {price}: stored region prices {stored:?}"
        );
    }
}
