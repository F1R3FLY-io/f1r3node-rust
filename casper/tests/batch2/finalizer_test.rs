// See casper/src/test/scala/coop/rchain/casper/batch2/FinalizerTest.scala

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Instant;

use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use block_storage::rust::test::indexed_block_dag_storage::IndexedBlockDagStorage;
use casper::rust::casper_conf::FinalizerConf;
use casper::rust::finality::finalizer::Finalizer;
use casper::rust::safety::clique_oracle::{CliqueOracle, FtThreshold};
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{BlockMessage, Bond, StateEffectId};
use models::rust::validator::Validator;
use shared::rust::store::key_value_store::KvStoreError;

use crate::helper::block_dag_storage_fixture::with_storage;
use crate::helper::block_generator::{create_block, create_genesis_block};
use crate::helper::block_util::generate_validator;

fn set_state_effect_provenance(
    dag: &block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation,
    block_hash: &BlockHash,
    successful_indices: &[u32],
    rejected_effects: &[StateEffectId],
) {
    let mut metadata = dag.lookup_unsafe(block_hash).expect("block metadata");
    metadata.successful_state_effect_indices = successful_indices.iter().copied().collect();
    metadata.rejected_state_effects = rejected_effects.iter().cloned().collect();
    dag.block_metadata_index
        .write()
        .add(metadata)
        .expect("replace block state-effect provenance");
}

async fn highest_exact_state_certified_candidate(
    dag: &block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation,
    latest_messages: &std::collections::BTreeMap<Validator, BlockHash>,
    current_lfb: &BlockHash,
    current_lfb_height: i64,
    threshold: FtThreshold,
) -> Result<Option<BlockHash>, Box<dyn std::error::Error + Send + Sync>> {
    let mut eligible = Vec::new();
    for candidate in &dag.dag_set {
        let block_number = dag.block_number_unsafe(candidate)?;
        if candidate == current_lfb || block_number <= current_lfb_height {
            continue;
        }
        if CliqueOracle::ft_witnessed_exact(candidate, dag, latest_messages, threshold).await?
            && casper::rust::finality::floor::is_state_preserved(dag, current_lfb, candidate)?
            && casper::rust::finality::floor::state_witnessed_exact(
                dag,
                candidate,
                latest_messages,
                threshold,
            )
            .await?
        {
            eligible.push((block_number, candidate.clone()));
        }
    }
    Ok(eligible
        .into_iter()
        .max_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)))
        .map(|(_, hash)| hash))
}

fn create_block_creator<'a>(
    bonds: &'a [Bond],
    genesis: &'a BlockMessage,
    creator: &'a Validator,
) -> impl Fn(
    &mut KeyValueBlockStore,
    &mut IndexedBlockDagStorage,
    Vec<&BlockMessage>,
    &HashMap<&Validator, &BlockMessage>,
) -> BlockMessage
       + 'a {
    move |block_store, block_dag_storage, parents, justifications| {
        let parent_hashes: Vec<BlockHash> = parents
            .iter()
            .map(|parent| parent.block_hash.clone())
            .collect();

        let justifications: HashMap<Validator, BlockHash> = justifications
            .iter()
            .map(|(validator, block_message)| {
                ((*validator).clone(), block_message.block_hash.clone())
            })
            .collect();

        create_block(
            block_store,
            block_dag_storage,
            parent_hashes,
            genesis,
            Some(creator.clone()),
            Some(bonds.to_vec()),
            Some(justifications),
            None,
            None,
            None,
            None,
            None,
            None,
        )
    }
}

#[test]
fn cannot_be_orphaned_should_return_false_for_non_positive_agreeing_stake() {
    let v1 = generate_validator(Some("v1"));
    let v2 = generate_validator(Some("v2"));

    let message_weight_map = HashMap::from([(v1.clone(), 10_i64), (v2.clone(), 10_i64)]);
    let agreeing_weight_map = HashMap::from([(v1.clone(), 10_i64), (v2.clone(), 0_i64)]);

    assert!(!Finalizer::cannot_be_orphaned(
        &message_weight_map,
        &agreeing_weight_map
    ));
}

#[test]
fn cannot_be_orphaned_should_return_false_on_stake_sum_overflow() {
    let v1 = generate_validator(Some("v1"));
    let v2 = generate_validator(Some("v2"));

    let message_weight_map = HashMap::from([(v1.clone(), i64::MAX), (v2.clone(), 1_i64)]);
    let agreeing_weight_map = HashMap::from([(v1.clone(), i64::MAX)]);

    assert!(!Finalizer::cannot_be_orphaned(
        &message_weight_map,
        &agreeing_weight_map
    ));
}

//   *  *            b8 b9
//   *               b7         <- should not yet be LFB
//   *  *  *  *  *   b2 b3 b4 b5 b6
//   *               b1         <- should be LFB
//   v1 v2 v3 v4 v5
#[tokio::test]
async fn finalizer_advances_to_highest_eligible_causal_descendant_and_invokes_effects() {
    with_storage(|mut store, mut dag_store| async move {
        let validators = [
            generate_validator(Some("Validator 1")),
            generate_validator(Some("Validator 2")),
            generate_validator(Some("Validator 3")),
            generate_validator(Some("Validator 4")),
            generate_validator(Some("Validator 5")),
        ];
        let bonds: Vec<Bond> = validators
            .iter()
            .map(|v| Bond {
                validator: v.clone(),
                stake: 3,
            })
            .collect();

        let lfb_store = Rc::new(RefCell::new(BlockHash::default()));
        let lfb_effect_invocations = Rc::new(RefCell::new(0_usize));

        let genesis = create_genesis_block(
            &mut store,
            &mut dag_store,
            None,
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            None,
        );

        let creator1 = create_block_creator(&bonds, &genesis, &validators[0]);
        let creator2 = create_block_creator(&bonds, &genesis, &validators[1]);
        let creator3 = create_block_creator(&bonds, &genesis, &validators[2]);
        let creator4 = create_block_creator(&bonds, &genesis, &validators[3]);
        let creator5 = create_block_creator(&bonds, &genesis, &validators[4]);

        let genesis_justification = HashMap::from([
            (&validators[0], &genesis),
            (&validators[1], &genesis),
            (&validators[2], &genesis),
            (&validators[3], &genesis),
            (&validators[4], &genesis),
        ]);
        let finalised_store = Rc::new(RefCell::new(HashSet::<BlockHash>::new()));

        let b1 = creator1(
            &mut store,
            &mut dag_store,
            vec![&genesis],
            &genesis_justification,
        );

        let b2 = creator1(
            &mut store,
            &mut dag_store,
            vec![&b1],
            &genesis_justification,
        );
        let b3 = creator2(
            &mut store,
            &mut dag_store,
            vec![&b1],
            &genesis_justification,
        );
        let b4 = creator3(
            &mut store,
            &mut dag_store,
            vec![&b1],
            &genesis_justification,
        );
        let b5 = creator4(
            &mut store,
            &mut dag_store,
            vec![&b1],
            &genesis_justification,
        );
        let b6 = creator5(
            &mut store,
            &mut dag_store,
            vec![&b1],
            &genesis_justification,
        );

        let dag = dag_store.get_representation().expect("dag representation");
        let _lms: Vec<(Validator, BlockHash)> = dag
            .latest_messages()
            .unwrap()
            .into_iter()
            .map(|(v, m)| (v, m.block_hash))
            .collect();
        let lfb = {
            let lfb_store = lfb_store.clone();
            Finalizer::run(
                &dag,
                FtThreshold::from_f32_lossy(-1.0),
                &genesis.block_hash,
                0,
                move |(m, _ft)| {
                    let lfb_store = lfb_store.clone();
                    async move {
                        *lfb_store.borrow_mut() = m;
                        Ok(())
                    }
                },
                &FinalizerConf::default(),
            )
            .await
            .unwrap()
        };

        // check output
        assert_eq!(lfb.as_ref().map(|(h, _)| h), Some(&b1.block_hash));
        // check if new LFB effect is invoked
        assert_eq!(*lfb_store.borrow(), b1.block_hash);

        let finalized_height = dag.lookup_unsafe(&lfb.unwrap().0).unwrap().block_number;

        /* next layer */
        let b7 = creator1(
            &mut store,
            &mut dag_store,
            vec![&b2, &b3, &b4, &b5, &b6],
            &HashMap::from([
                (&validators[0], &b2),
                (&validators[1], &b3),
                (&validators[2], &b4),
                (&validators[3], &b4),
                (&validators[4], &b5),
            ]),
        );

        // add 2 children, this is not sufficient to finalize b7
        creator1(
            &mut store,
            &mut dag_store,
            vec![&b7],
            &HashMap::from([
                (&validators[0], &b7),
                (&validators[1], &b3),
                (&validators[2], &b4),
                (&validators[3], &b5),
                (&validators[4], &b6),
            ]),
        );
        creator2(
            &mut store,
            &mut dag_store,
            vec![&b7],
            &HashMap::from([
                (&validators[0], &b7),
                (&validators[1], &b3),
                (&validators[2], &b4),
                (&validators[3], &b5),
                (&validators[4], &b6),
            ]),
        );

        let dag = dag_store.get_representation().expect("dag representation");
        let lfb = {
            let lfb_effect_invocations = lfb_effect_invocations.clone();
            Finalizer::run(
                &dag,
                FtThreshold::from_f32_lossy(-1.0),
                &b1.block_hash,
                finalized_height,
                move |(_m, _ft)| {
                    let lfb_effect_invocations = lfb_effect_invocations.clone();
                    async move {
                        *lfb_effect_invocations.borrow_mut() += 1;
                        Ok(())
                    }
                },
                &FinalizerConf::default(),
            )
            .await
            .unwrap()
        };

        let secondary = lfb.expect("a majority-certified merge parent must finalize");
        let latest_messages = dag
            .latest_message_hashes()
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>();
        let threshold = FtThreshold::from_f32_lossy(-1.0);
        let expected_secondary = highest_exact_state_certified_candidate(
            &dag,
            &latest_messages,
            &b1.block_hash,
            finalized_height,
            threshold,
        )
        .await
        .expect("exact candidate search")
        .expect("eligible secondary parent");
        assert_eq!(secondary.0, expected_secondary);
        assert_eq!(*lfb_effect_invocations.borrow(), 1);
        assert!(
            CliqueOracle::ft_witnessed_exact(&secondary.0, &dag, &latest_messages, threshold,)
                .await
                .expect("selected secondary causal certificate")
        );
        assert!(casper::rust::finality::floor::state_witnessed_exact(
            &dag,
            &secondary.0,
            &latest_messages,
            threshold,
        )
        .await
        .expect("selected secondary state certificate"));
        assert!(casper::rust::finality::floor::is_state_preserved(
            &dag,
            &b1.block_hash,
            &secondary.0,
        )
        .expect("selected secondary preserves the current floor"));
        assert!(!CliqueOracle::ft_witnessed_exact(
            &b7.block_hash,
            &dag,
            &latest_messages,
            threshold,
        )
        .await
        .expect("merge causal certificate"));

        // add more 3 children - finalization should advance
        creator3(
            &mut store,
            &mut dag_store,
            vec![&b7],
            &HashMap::from([
                (&validators[0], &b7),
                (&validators[1], &b3),
                (&validators[2], &b4),
                (&validators[3], &b5),
                (&validators[4], &b6),
            ]),
        );
        creator4(
            &mut store,
            &mut dag_store,
            vec![&b7],
            &HashMap::from([
                (&validators[0], &b7),
                (&validators[1], &b3),
                (&validators[2], &b4),
                (&validators[3], &b5),
                (&validators[4], &b6),
            ]),
        );
        creator5(
            &mut store,
            &mut dag_store,
            vec![&b7],
            &HashMap::from([
                (&validators[0], &b7),
                (&validators[1], &b3),
                (&validators[2], &b4),
                (&validators[3], &b5),
                (&validators[4], &b6),
            ]),
        );

        let dag = dag_store.get_representation().expect("dag representation");
        let lfb = {
            let lfb_store = lfb_store.clone();
            let finalised_store = finalised_store.clone();
            Finalizer::run(
                &dag,
                FtThreshold::from_f32_lossy(-1.0),
                &secondary.0,
                dag.lookup_unsafe(&secondary.0)
                    .expect("secondary metadata")
                    .block_number,
                move |(m, _ft)| {
                    let lfb_store = lfb_store.clone();
                    let finalised_store = finalised_store.clone();
                    async move {
                        *lfb_store.borrow_mut() = m.clone();
                        finalised_store.borrow_mut().insert(m);
                        Ok(())
                    }
                },
                &FinalizerConf::default(),
            )
            .await
            .unwrap()
        };

        // check output
        assert_eq!(lfb.as_ref().map(|(h, _)| h), Some(&b7.block_hash));
        // check if new LFB effect is invoked
        assert_eq!(*lfb_store.borrow(), b7.block_hash);

        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await
    .expect("Test should complete successfully");
}

#[tokio::test]
async fn finalizer_invokes_effect_for_finalized_candidate_ahead_of_lfb() {
    with_storage(|mut store, mut dag_store| async move {
        let validators = [
            generate_validator(Some("Finalized Candidate Validator 1")),
            generate_validator(Some("Finalized Candidate Validator 2")),
            generate_validator(Some("Finalized Candidate Validator 3")),
            generate_validator(Some("Finalized Candidate Validator 4")),
            generate_validator(Some("Finalized Candidate Validator 5")),
        ];
        let bonds: Vec<Bond> = validators
            .iter()
            .map(|validator| Bond {
                validator: validator.clone(),
                stake: 3,
            })
            .collect();
        let genesis = create_genesis_block(
            &mut store,
            &mut dag_store,
            None,
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            None,
        );
        let creators = [
            create_block_creator(&bonds, &genesis, &validators[0]),
            create_block_creator(&bonds, &genesis, &validators[1]),
            create_block_creator(&bonds, &genesis, &validators[2]),
            create_block_creator(&bonds, &genesis, &validators[3]),
            create_block_creator(&bonds, &genesis, &validators[4]),
        ];
        let genesis_justifications = HashMap::from([
            (&validators[0], &genesis),
            (&validators[1], &genesis),
            (&validators[2], &genesis),
            (&validators[3], &genesis),
            (&validators[4], &genesis),
        ]);
        let candidate = creators[0](
            &mut store,
            &mut dag_store,
            vec![&genesis],
            &genesis_justifications,
        );
        for creator in &creators {
            creator(
                &mut store,
                &mut dag_store,
                vec![&candidate],
                &genesis_justifications,
            );
        }
        dag_store
            .record_directly_finalized(candidate.block_hash.clone(), 1.0, |_| async { Ok(()) })
            .await
            .expect("record candidate finalized");
        let dag = dag_store.get_representation().expect("dag representation");
        let effect_invoked = Rc::new(RefCell::new(false));
        let selected = Rc::new(RefCell::new(BlockHash::default()));
        let result = {
            let effect_invoked = effect_invoked.clone();
            let selected = selected.clone();
            Finalizer::run(
                &dag,
                FtThreshold::from_f32_lossy(-1.0),
                &genesis.block_hash,
                0,
                move |(hash, _)| {
                    let effect_invoked = effect_invoked.clone();
                    let selected = selected.clone();
                    async move {
                        *effect_invoked.borrow_mut() = true;
                        *selected.borrow_mut() = hash;
                        Ok(())
                    }
                },
                &FinalizerConf::default(),
            )
            .await
            .expect("finalizer run")
        };

        assert_eq!(
            result.as_ref().map(|(hash, _)| hash),
            Some(&candidate.block_hash)
        );
        assert_eq!(*selected.borrow(), candidate.block_hash);
        assert!(*effect_invoked.borrow());

        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await
    .expect("validation fixture");
}

#[tokio::test]
async fn finalizer_never_moves_to_a_sibling_of_the_exact_lfb() {
    with_storage(|mut store, mut dag_store| async move {
        let validators = [
            generate_validator(Some("Exact LFB Validator 1")),
            generate_validator(Some("Exact LFB Validator 2")),
            generate_validator(Some("Exact LFB Validator 3")),
            generate_validator(Some("Exact LFB Validator 4")),
            generate_validator(Some("Exact LFB Validator 5")),
        ];
        let bonds: Vec<Bond> = validators
            .iter()
            .map(|validator| Bond {
                validator: validator.clone(),
                stake: 3,
            })
            .collect();
        let genesis = create_genesis_block(
            &mut store,
            &mut dag_store,
            None,
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            None,
        );
        let creators = [
            create_block_creator(&bonds, &genesis, &validators[0]),
            create_block_creator(&bonds, &genesis, &validators[1]),
            create_block_creator(&bonds, &genesis, &validators[2]),
            create_block_creator(&bonds, &genesis, &validators[3]),
            create_block_creator(&bonds, &genesis, &validators[4]),
        ];
        let genesis_justifications = HashMap::from([
            (&validators[0], &genesis),
            (&validators[1], &genesis),
            (&validators[2], &genesis),
            (&validators[3], &genesis),
            (&validators[4], &genesis),
        ]);
        let current_lfb = creators[0](
            &mut store,
            &mut dag_store,
            vec![&genesis],
            &genesis_justifications,
        );
        let sibling = creators[1](
            &mut store,
            &mut dag_store,
            vec![&genesis],
            &genesis_justifications,
        );
        let sibling_justifications = HashMap::from([
            (&validators[0], &sibling),
            (&validators[1], &sibling),
            (&validators[2], &sibling),
            (&validators[3], &sibling),
            (&validators[4], &sibling),
        ]);
        for creator in &creators {
            creator(
                &mut store,
                &mut dag_store,
                vec![&sibling],
                &sibling_justifications,
            );
        }

        let dag = dag_store.get_representation().expect("dag representation");
        let effect_invoked = Rc::new(RefCell::new(false));
        let result = {
            let effect_invoked = effect_invoked.clone();
            Finalizer::run(
                &dag,
                FtThreshold::from_f32_lossy(-1.0),
                &current_lfb.block_hash,
                current_lfb.body.state.block_number,
                move |_| {
                    let effect_invoked = effect_invoked.clone();
                    async move {
                        *effect_invoked.borrow_mut() = true;
                        Ok(())
                    }
                },
                &FinalizerConf::default(),
            )
            .await
            .expect("finalizer run")
        };

        assert!(result.is_none());
        assert!(!*effect_invoked.borrow());

        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await
    .expect("validation fixture");
}

#[tokio::test]
async fn finalizer_advances_to_state_descendant_when_lfb_is_a_secondary_parent() {
    with_storage(|mut store, mut dag_store| async move {
        let validators = [
            generate_validator(Some("Asymmetric Validator 1")),
            generate_validator(Some("Asymmetric Validator 2")),
            generate_validator(Some("Asymmetric Validator 3")),
        ];
        let bonds = vec![
            Bond {
                validator: validators[0].clone(),
                stake: 60,
            },
            Bond {
                validator: validators[1].clone(),
                stake: 20,
            },
            Bond {
                validator: validators[2].clone(),
                stake: 15,
            },
        ];
        let genesis = create_genesis_block(
            &mut store,
            &mut dag_store,
            None,
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            None,
        );
        let creators: Vec<_> = validators
            .iter()
            .map(|validator| create_block_creator(&bonds, &genesis, validator))
            .collect();
        let genesis_justifications: HashMap<&Validator, &BlockMessage> = validators
            .iter()
            .map(|validator| (validator, &genesis))
            .collect();
        let current_lfb = creators[0](
            &mut store,
            &mut dag_store,
            vec![&genesis],
            &genesis_justifications,
        );
        let sibling = creators[1](
            &mut store,
            &mut dag_store,
            vec![&genesis],
            &genesis_justifications,
        );
        dag_store
            .record_directly_finalized(current_lfb.block_hash.clone(), 1.0, |_| async { Ok(()) })
            .await
            .expect("record current LFB");

        let merge_justifications = HashMap::from([
            (&validators[0], &current_lfb),
            (&validators[1], &sibling),
            (&validators[2], &sibling),
        ]);
        let merge = creators[0](
            &mut store,
            &mut dag_store,
            vec![&sibling, &current_lfb],
            &merge_justifications,
        );
        let support_justifications: HashMap<&Validator, &BlockMessage> = validators
            .iter()
            .map(|validator| (validator, &merge))
            .collect();
        let supports: Vec<BlockMessage> = creators
            .iter()
            .map(|creator| {
                creator(
                    &mut store,
                    &mut dag_store,
                    vec![&merge],
                    &support_justifications,
                )
            })
            .collect();

        let dag = dag_store.get_representation().expect("dag representation");
        dag.put_cached_floor(genesis.block_hash.clone(), genesis.block_hash.clone())
            .expect("genesis floor");
        dag.put_cached_floor(current_lfb.block_hash.clone(), genesis.block_hash.clone())
            .expect("current LFB floor");
        dag.put_cached_floor(sibling.block_hash.clone(), genesis.block_hash.clone())
            .expect("sibling floor");
        dag.put_cached_floor(merge.block_hash.clone(), current_lfb.block_hash.clone())
            .expect("merge floor");
        for support in &supports {
            dag.put_cached_floor(support.block_hash.clone(), merge.block_hash.clone())
                .expect("support floor");
        }
        set_state_effect_provenance(&dag, &current_lfb.block_hash, &[0], &[]);

        assert!(dag
            .is_dag_ancestor(&current_lfb.block_hash, &merge.block_hash)
            .expect("DAG ancestry"));
        assert!(!dag
            .is_in_main_chain(&current_lfb.block_hash, &merge.block_hash)
            .expect("main-chain ancestry"));
        assert!(casper::rust::finality::floor::is_state_preserved(
            &dag,
            &current_lfb.block_hash,
            &merge.block_hash,
        )
        .expect("state ancestry"));

        let result = Finalizer::run(
            &dag,
            FtThreshold::from_f32_lossy(0.1),
            &current_lfb.block_hash,
            current_lfb.body.state.block_number,
            |(_m, _ft)| async { Ok::<(), KvStoreError>(()) },
            &FinalizerConf::default(),
        )
        .await
        .expect("finalizer run");

        let (selected, _) = result.expect("state-preserving off-main merge must advance");
        assert!(casper::rust::finality::floor::is_state_preserved(
            &dag,
            &current_lfb.block_hash,
            &selected,
        )
        .expect("selected state ancestry"));
        assert!(!dag
            .is_in_main_chain(&current_lfb.block_hash, &selected)
            .expect("selected main-chain ancestry"));

        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await
    .expect("validation fixture");
}

#[tokio::test]
async fn finalizer_discovers_state_certified_secondary_parent_through_all_parent_coverage() {
    with_storage(|mut store, mut dag_store| async move {
        let validators = [
            generate_validator(Some("All Parent Validator 1")),
            generate_validator(Some("All Parent Validator 2")),
            generate_validator(Some("All Parent Validator 3")),
            generate_validator(Some("All Parent Validator 4")),
        ];
        let bonds = validators
            .iter()
            .zip([1, 3, 5, 7])
            .map(|(validator, stake)| Bond {
                validator: validator.clone(),
                stake,
            })
            .collect::<Vec<_>>();
        let genesis = create_genesis_block(
            &mut store,
            &mut dag_store,
            None,
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            None,
        );
        let creators = validators
            .iter()
            .map(|validator| create_block_creator(&bonds, &genesis, validator))
            .collect::<Vec<_>>();
        let genesis_justifications = validators
            .iter()
            .map(|validator| (validator, &genesis))
            .collect::<HashMap<_, _>>();
        let siblings = creators[..3]
            .iter()
            .map(|creator| {
                creator(
                    &mut store,
                    &mut dag_store,
                    vec![&genesis],
                    &genesis_justifications,
                )
            })
            .collect::<Vec<_>>();
        let merge_justifications = HashMap::from([
            (&validators[0], &siblings[0]),
            (&validators[1], &siblings[1]),
            (&validators[2], &siblings[2]),
            (&validators[3], &genesis),
        ]);
        let merge = creators[3](
            &mut store,
            &mut dag_store,
            vec![&siblings[0], &siblings[1], &siblings[2]],
            &merge_justifications,
        );
        let dag = dag_store.get_representation().expect("dag representation");
        for block in &siblings {
            set_state_effect_provenance(&dag, &block.block_hash, &[0], &[]);
        }
        let rejected = siblings[..2]
            .iter()
            .map(|block| StateEffectId {
                source_block_hash: block.block_hash.clone(),
                execution_index: 0,
            })
            .collect::<Vec<_>>();
        set_state_effect_provenance(&dag, &merge.block_hash, &[], &rejected);

        assert_eq!(merge.header.parents_hash_list[0], siblings[0].block_hash);
        assert!(!dag
            .is_in_main_chain(&siblings[2].block_hash, &merge.block_hash)
            .expect("secondary-parent main-chain relation"));
        let latest_messages = dag
            .latest_message_hashes()
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>();
        let threshold = FtThreshold::from_f32_lossy(0.0);
        for latest in latest_messages.values() {
            casper::rust::finality::floor::floor_of_block(&dag, latest, threshold)
                .await
                .expect("latest-message floor provenance");
        }
        assert!(!CliqueOracle::ft_witnessed_exact(
            &siblings[0].block_hash,
            &dag,
            &latest_messages,
            threshold,
        )
        .await
        .expect("strict half-support verdict"));
        assert!(!casper::rust::finality::floor::state_witnessed_exact(
            &dag,
            &siblings[1].block_hash,
            &latest_messages,
            threshold,
        )
        .await
        .expect("state-rejected sibling verdict"));
        assert!(casper::rust::finality::floor::state_witnessed_exact(
            &dag,
            &siblings[2].block_hash,
            &latest_messages,
            threshold,
        )
        .await
        .expect("surviving secondary sibling verdict"));

        let result = Finalizer::run(
            &dag,
            threshold,
            &genesis.block_hash,
            genesis.body.state.block_number,
            |(_block, _ft)| async { Ok::<(), KvStoreError>(()) },
            &FinalizerConf::default(),
        )
        .await
        .expect("finalizer run")
        .expect("secondary-parent candidate");

        assert_eq!(result.0, siblings[2].block_hash);

        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await
    .expect("validation fixture");
}

#[tokio::test]
async fn finalizer_rejects_dag_descendant_without_state_lineage() {
    with_storage(|mut store, mut dag_store| async move {
        let validators = [
            generate_validator(Some("DAG Finality Validator 1")),
            generate_validator(Some("DAG Finality Validator 2")),
            generate_validator(Some("DAG Finality Validator 3")),
            generate_validator(Some("DAG Finality Validator 4")),
            generate_validator(Some("DAG Finality Validator 5")),
        ];
        let bonds: Vec<Bond> = validators
            .iter()
            .map(|validator| Bond {
                validator: validator.clone(),
                stake: 3,
            })
            .collect();
        let genesis = create_genesis_block(
            &mut store,
            &mut dag_store,
            None,
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            None,
        );
        let creators: Vec<_> = validators
            .iter()
            .map(|validator| create_block_creator(&bonds, &genesis, validator))
            .collect();
        let genesis_justifications: HashMap<&Validator, &BlockMessage> = validators
            .iter()
            .map(|validator| (validator, &genesis))
            .collect();
        let current_lfb = creators[0](
            &mut store,
            &mut dag_store,
            vec![&genesis],
            &genesis_justifications,
        );
        let sibling = creators[1](
            &mut store,
            &mut dag_store,
            vec![&genesis],
            &genesis_justifications,
        );
        let merge_justifications = HashMap::from([
            (&validators[0], &current_lfb),
            (&validators[1], &sibling),
            (&validators[2], &genesis),
            (&validators[3], &genesis),
            (&validators[4], &genesis),
        ]);
        let merge = creators[2](
            &mut store,
            &mut dag_store,
            vec![&current_lfb, &sibling],
            &merge_justifications,
        );
        let merge_justifications: HashMap<&Validator, &BlockMessage> = validators
            .iter()
            .map(|validator| (validator, &merge))
            .collect();
        for creator in &creators {
            creator(
                &mut store,
                &mut dag_store,
                vec![&merge],
                &merge_justifications,
            );
        }

        let dag = dag_store.get_representation().expect("dag representation");
        assert!(dag
            .is_dag_ancestor(&current_lfb.block_hash, &merge.block_hash)
            .expect("DAG ancestry"));
        assert!(dag
            .is_in_main_chain(&current_lfb.block_hash, &merge.block_hash)
            .expect("main-chain ancestry"));

        let latest_messages = dag
            .latest_message_hashes()
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>();
        let current_lfb_effect = StateEffectId {
            source_block_hash: current_lfb.block_hash.clone(),
            execution_index: 0,
        };
        set_state_effect_provenance(&dag, &current_lfb.block_hash, &[0], &[]);
        set_state_effect_provenance(
            &dag,
            &merge.block_hash,
            &[],
            std::slice::from_ref(&current_lfb_effect),
        );
        for latest in latest_messages.values() {
            casper::rust::finality::floor::floor_of_block(
                &dag,
                latest,
                FtThreshold::from_f32_lossy(-1.0),
            )
            .await
            .expect("latest-message state lineage");
        }
        assert!(CliqueOracle::ft_witnessed_exact(
            &merge.block_hash,
            &dag,
            &latest_messages,
            FtThreshold::from_f32_lossy(-1.0),
        )
        .await
        .expect("exact clique decision"));
        assert!(casper::rust::finality::floor::state_witnessed_exact(
            &dag,
            &merge.block_hash,
            &latest_messages,
            FtThreshold::from_f32_lossy(-1.0),
        )
        .await
        .expect("state certificate"));

        let result = Finalizer::run(
            &dag,
            FtThreshold::from_f32_lossy(-1.0),
            &current_lfb.block_hash,
            current_lfb.body.state.block_number,
            |(_m, _ft)| async { Ok::<(), KvStoreError>(()) },
            &FinalizerConf::default(),
        )
        .await
        .expect("finalizer run");

        assert!(result.is_none());

        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await
    .expect("validation fixture");
}

#[tokio::test]
async fn finalizer_rejects_causal_certificate_without_state_support() {
    with_storage(|mut store, mut dag_store| async move {
        let validators = [
            generate_validator(Some("State Support Heavy Validator")),
            generate_validator(Some("State Support Source Validator")),
            generate_validator(Some("State Support Other Validator 1")),
            generate_validator(Some("State Support Other Validator 2")),
        ];
        let bonds = vec![
            Bond {
                validator: validators[0].clone(),
                stake: 7,
            },
            Bond {
                validator: validators[1].clone(),
                stake: 3,
            },
            Bond {
                validator: validators[2].clone(),
                stake: 3,
            },
            Bond {
                validator: validators[3].clone(),
                stake: 3,
            },
        ];
        let genesis = create_genesis_block(
            &mut store,
            &mut dag_store,
            None,
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            None,
        );
        let creators: Vec<_> = validators
            .iter()
            .map(|validator| create_block_creator(&bonds, &genesis, validator))
            .collect();
        let genesis_justifications: HashMap<&Validator, &BlockMessage> = validators
            .iter()
            .map(|validator| (validator, &genesis))
            .collect();
        let rejected_parent = creators[1](
            &mut store,
            &mut dag_store,
            vec![&genesis],
            &genesis_justifications,
        );
        let sibling = creators[2](
            &mut store,
            &mut dag_store,
            vec![&genesis],
            &genesis_justifications,
        );
        let merge_justifications = HashMap::from([
            (&validators[0], &genesis),
            (&validators[1], &rejected_parent),
            (&validators[2], &sibling),
            (&validators[3], &genesis),
        ]);
        let merge = creators[0](
            &mut store,
            &mut dag_store,
            vec![&rejected_parent, &sibling],
            &merge_justifications,
        );
        let sibling_justifications = HashMap::from([
            (&validators[0], &merge),
            (&validators[1], &rejected_parent),
            (&validators[2], &sibling),
            (&validators[3], &sibling),
        ]);
        let sibling_support = creators[3](
            &mut store,
            &mut dag_store,
            vec![&sibling],
            &sibling_justifications,
        );

        let dag = dag_store.get_representation().expect("dag representation");
        for (block, floor) in [
            (&genesis, &genesis),
            (&rejected_parent, &genesis),
            (&sibling, &genesis),
            (&merge, &genesis),
            (&sibling_support, &genesis),
        ] {
            dag.put_cached_floor(block.block_hash.clone(), floor.block_hash.clone())
                .expect("state floor");
        }
        let rejected_effect = StateEffectId {
            source_block_hash: rejected_parent.block_hash.clone(),
            execution_index: 0,
        };
        set_state_effect_provenance(&dag, &rejected_parent.block_hash, &[0], &[]);
        set_state_effect_provenance(
            &dag,
            &merge.block_hash,
            &[],
            std::slice::from_ref(&rejected_effect),
        );

        let latest_messages = dag
            .latest_message_hashes()
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>();
        let threshold = FtThreshold::from_f32_lossy(0.1);
        assert!(CliqueOracle::ft_witnessed_exact(
            &rejected_parent.block_hash,
            &dag,
            &latest_messages,
            threshold,
        )
        .await
        .expect("causal certificate"));
        assert!(!casper::rust::finality::floor::state_witnessed_exact(
            &dag,
            &rejected_parent.block_hash,
            &latest_messages,
            threshold,
        )
        .await
        .expect("state certificate"));

        let promoted = Rc::new(RefCell::new(Vec::new()));
        let result = {
            let promoted = promoted.clone();
            Finalizer::run(
                &dag,
                threshold,
                &genesis.block_hash,
                genesis.body.state.block_number,
                move |(hash, _ft)| {
                    promoted.borrow_mut().push(hash);
                    async { Ok::<(), KvStoreError>(()) }
                },
                &FinalizerConf::default(),
            )
            .await
            .expect("finalizer run")
        };

        assert_ne!(
            result.map(|(hash, _)| hash),
            Some(rejected_parent.block_hash.clone())
        );
        assert!(!promoted.borrow().contains(&rejected_parent.block_hash));

        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await
    .expect("validation fixture");
}

#[tokio::test]
async fn finalizer_examines_a_complete_frozen_candidate_set_beyond_the_old_prefix() {
    with_storage(|mut store, mut dag_store| async move {
        let validators = [
            generate_validator(Some("Complete Scan Validator 1")),
            generate_validator(Some("Complete Scan Validator 2")),
            generate_validator(Some("Complete Scan Validator 3")),
            generate_validator(Some("Complete Scan Validator 4")),
            generate_validator(Some("Complete Scan Validator 5")),
        ];
        let bonds: Vec<Bond> = validators
            .iter()
            .map(|validator| Bond {
                validator: validator.clone(),
                stake: 3,
            })
            .collect();
        let genesis = create_genesis_block(
            &mut store,
            &mut dag_store,
            None,
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            None,
        );
        let creators: Vec<_> = validators
            .iter()
            .map(|validator| create_block_creator(&bonds, &genesis, validator))
            .collect();
        let genesis_justifications: HashMap<&Validator, &BlockMessage> = validators
            .iter()
            .map(|validator| (validator, &genesis))
            .collect();
        let finalizable = creators[0](
            &mut store,
            &mut dag_store,
            vec![&genesis],
            &genesis_justifications,
        );
        let mut chain_tip = finalizable.clone();
        for _ in 0..132 {
            chain_tip = creators[0](
                &mut store,
                &mut dag_store,
                vec![&chain_tip],
                &genesis_justifications,
            );
        }

        let validator1_tip = creators[1](
            &mut store,
            &mut dag_store,
            vec![&chain_tip],
            &genesis_justifications,
        );
        let validator2_tip = creators[2](
            &mut store,
            &mut dag_store,
            vec![&chain_tip],
            &genesis_justifications,
        );
        let validator3_tip = creators[3](
            &mut store,
            &mut dag_store,
            vec![&finalizable],
            &genesis_justifications,
        );
        let validator4_tip = creators[4](
            &mut store,
            &mut dag_store,
            vec![&finalizable],
            &genesis_justifications,
        );
        let mut latest = [
            chain_tip,
            validator1_tip,
            validator2_tip,
            validator3_tip,
            validator4_tip,
        ];

        for _ in 0..2 {
            for creator_index in 0..validators.len() {
                let parent = latest[creator_index].clone();
                let next = {
                    let justifications: HashMap<&Validator, &BlockMessage> =
                        validators.iter().zip(latest.iter()).collect();
                    creators[creator_index](
                        &mut store,
                        &mut dag_store,
                        vec![&parent],
                        &justifications,
                    )
                };
                latest[creator_index] = next;
            }
        }

        let dag = dag_store.get_representation().expect("dag representation");
        let selected = Finalizer::run(
            &dag,
            FtThreshold::from_ppm(300_000),
            &genesis.block_hash,
            0,
            |(_hash, _ft)| async { Ok::<(), KvStoreError>(()) },
            &FinalizerConf::default(),
        )
        .await
        .expect("complete finalizer run")
        .expect("older candidate should finalize");

        assert_eq!(selected.0, finalizable.block_hash);

        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await
    .expect("validation fixture");
}

#[tokio::test]
async fn finalizer_recognizes_all_parent_convergence_in_a_reconvergent_dag() {
    with_storage(|mut store, mut dag_store| async move {
        let validators = [
            generate_validator(Some("Reconvergent Validator 1")),
            generate_validator(Some("Reconvergent Validator 2")),
            generate_validator(Some("Reconvergent Validator 3")),
        ];
        let bonds: Vec<Bond> = validators
            .iter()
            .map(|validator| Bond {
                validator: validator.clone(),
                stake: 10,
            })
            .collect();
        let genesis = create_genesis_block(
            &mut store,
            &mut dag_store,
            None,
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            None,
        );
        let creators: Vec<_> = validators
            .iter()
            .map(|validator| create_block_creator(&bonds, &genesis, validator))
            .collect();
        let genesis_justifications: HashMap<&Validator, &BlockMessage> = validators
            .iter()
            .map(|validator| (validator, &genesis))
            .collect();
        let mut left = creators[0](
            &mut store,
            &mut dag_store,
            vec![&genesis],
            &genesis_justifications,
        );
        let mut right = creators[1](
            &mut store,
            &mut dag_store,
            vec![&genesis],
            &genesis_justifications,
        );

        for _ in 0..24 {
            let justifications = HashMap::from([
                (&validators[0], &left),
                (&validators[1], &right),
                (&validators[2], &genesis),
            ]);
            let next_left = creators[0](
                &mut store,
                &mut dag_store,
                vec![&left, &right],
                &justifications,
            );
            let next_right = creators[1](
                &mut store,
                &mut dag_store,
                vec![&right, &left],
                &justifications,
            );
            left = next_left;
            right = next_right;
        }

        let dag = dag_store.get_representation().expect("dag representation");
        let expected_common_parent = left
            .header
            .parents_hash_list
            .iter()
            .max_by_key(|hash| {
                (
                    dag.block_number_unsafe(hash)
                        .expect("common-parent metadata"),
                    (*hash).clone(),
                )
            })
            .cloned()
            .expect("immediate common parent");
        let split_result = Finalizer::run(
            &dag,
            FtThreshold::from_f32_lossy(-1.0),
            &genesis.block_hash,
            0,
            |(_hash, _ft)| async { Ok::<(), KvStoreError>(()) },
            &FinalizerConf::default(),
        )
        .await
        .expect("split finalizer run")
        .expect("all-parent convergence must expose a certified common ancestor");
        let split_latest_messages = dag
            .latest_message_hashes()
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>();
        let exact_split = highest_exact_state_certified_candidate(
            &dag,
            &split_latest_messages,
            &genesis.block_hash,
            0,
            FtThreshold::from_f32_lossy(-1.0),
        )
        .await
        .expect("exact split candidate search")
        .expect("exact split candidate");
        assert_eq!(split_result.0, exact_split);
        assert_eq!(split_result.0, expected_common_parent);

        let production_threshold = FtThreshold::from_ppm(100_000);
        assert!(!CliqueOracle::ft_witnessed_exact(
            &left.block_hash,
            &dag,
            &split_latest_messages,
            production_threshold,
        )
        .await
        .expect("left split-tip certificate"));
        assert!(!CliqueOracle::ft_witnessed_exact(
            &right.block_hash,
            &dag,
            &split_latest_messages,
            production_threshold,
        )
        .await
        .expect("right split-tip certificate"));
        let production_split_result = Finalizer::run(
            &dag,
            production_threshold,
            &genesis.block_hash,
            0,
            |(_hash, _ft)| async { Ok::<(), KvStoreError>(()) },
            &FinalizerConf::default(),
        )
        .await
        .expect("production-threshold split finalizer run")
        .expect("production threshold must certify a common ancestor");
        let exact_production_split = highest_exact_state_certified_candidate(
            &dag,
            &split_latest_messages,
            &genesis.block_hash,
            0,
            production_threshold,
        )
        .await
        .expect("exact production candidate search")
        .expect("exact production candidate");
        assert_eq!(production_split_result.0, exact_production_split);

        let strict_split_result = Finalizer::run(
            &dag,
            FtThreshold::from_ppm(500_000),
            &genesis.block_hash,
            0,
            |(_hash, _ft)| async { Ok::<(), KvStoreError>(()) },
            &FinalizerConf::default(),
        )
        .await
        .expect("strict-threshold split finalizer run");
        let exact_strict_split = highest_exact_state_certified_candidate(
            &dag,
            &split_latest_messages,
            &genesis.block_hash,
            0,
            FtThreshold::from_ppm(500_000),
        )
        .await
        .expect("exact strict candidate search");
        assert_eq!(
            strict_split_result.as_ref().map(|(hash, _)| hash),
            exact_strict_split.as_ref()
        );
        assert!(strict_split_result.is_none());

        let split_justifications = HashMap::from([
            (&validators[0], &left),
            (&validators[1], &right),
            (&validators[2], &genesis),
        ]);
        let converged = creators[2](
            &mut store,
            &mut dag_store,
            vec![&left, &right],
            &split_justifications,
        );
        let converged_justifications: HashMap<&Validator, &BlockMessage> = validators
            .iter()
            .map(|validator| (validator, &converged))
            .collect();
        for creator in &creators {
            creator(
                &mut store,
                &mut dag_store,
                vec![&converged],
                &converged_justifications,
            );
        }

        let dag = dag_store.get_representation().expect("dag representation");
        let selected = Finalizer::run(
            &dag,
            FtThreshold::from_ppm(500_000),
            &genesis.block_hash,
            0,
            |(_hash, _ft)| async { Ok::<(), KvStoreError>(()) },
            &FinalizerConf::default(),
        )
        .await
        .expect("converged finalizer run")
        .expect("shared main-parent vote should finalize");

        assert_eq!(selected.0, converged.block_hash);

        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await
    .expect("validation fixture");
}

#[tokio::test]
#[ignore = "diagnostic: run manually for fast finalizer growth feedback"]
async fn finalizer_growth_feedback_loop_stale_justification_chain() {
    with_storage(|mut store, mut dag_store| async move {
        let validators = [
            generate_validator(Some("Growth Validator 1")),
            generate_validator(Some("Growth Validator 2")),
            generate_validator(Some("Growth Validator 3")),
        ];
        let bonds: Vec<Bond> = validators
            .iter()
            .map(|v| Bond {
                validator: v.clone(),
                stake: 10,
            })
            .collect();

        let genesis = create_genesis_block(
            &mut store,
            &mut dag_store,
            None,
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            None,
        );

        let creator1 = create_block_creator(&bonds, &genesis, &validators[0]);
        let creator2 = create_block_creator(&bonds, &genesis, &validators[1]);
        let creator3 = create_block_creator(&bonds, &genesis, &validators[2]);

        let checkpoints = [24usize, 48usize, 96usize];
        let mut timing_samples: Vec<(usize, u128)> = Vec::with_capacity(checkpoints.len());
        let mut latest_by_validator = [genesis.clone(), genesis.clone(), genesis.clone()];

        for height in 1..=checkpoints[checkpoints.len() - 1] {
            let creator_index = (height - 1) % validators.len();

            let mut justifications: HashMap<&Validator, &BlockMessage> = HashMap::new();
            for (idx, validator) in validators.iter().enumerate() {
                let justification = if idx == creator_index {
                    &latest_by_validator[idx]
                } else {
                    &genesis
                };
                justifications.insert(validator, justification);
            }

            let parent = &latest_by_validator[creator_index];
            let next_block = match creator_index {
                0 => creator1(&mut store, &mut dag_store, vec![parent], &justifications),
                1 => creator2(&mut store, &mut dag_store, vec![parent], &justifications),
                2 => creator3(&mut store, &mut dag_store, vec![parent], &justifications),
                _ => unreachable!("creator_index should be in [0, 2]"),
            };
            latest_by_validator[creator_index] = next_block;

            if checkpoints.contains(&height) {
                let dag = dag_store.get_representation().expect("dag representation");
                let started = Instant::now();
                let _ = Finalizer::run(
                    &dag,
                    FtThreshold::from_f32_lossy(-1.0),
                    &genesis.block_hash,
                    0,
                    |(_m, _ft)| async { Ok::<(), KvStoreError>(()) },
                    &FinalizerConf::default(),
                )
                .await
                .expect("Finalizer run should succeed");
                timing_samples.push((height, started.elapsed().as_millis()));
            }
        }

        assert_eq!(timing_samples.len(), checkpoints.len());
        eprintln!("finalizer growth feedback (stale-justification chain):");
        for (height, elapsed_ms) in timing_samples {
            eprintln!("  height={height:>3} finalizer_run_ms={elapsed_ms}");
        }

        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await
    .expect("growth feedback test should complete successfully");
}
