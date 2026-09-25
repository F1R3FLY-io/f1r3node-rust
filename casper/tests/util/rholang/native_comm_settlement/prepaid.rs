use casper::rust::util::rholang::costacc::prepaid_receipts::{
    NativePrepaidCell, NativePrepaidContribution, NativePrepaidOrigin, OrderedPrepaidCells,
    PrepaidReceiptBucket, PrepaidReceiptChange,
};
use casper::rust::util::rholang::costacc::vault_payer::VaultPayer;
use models::rust::phlo_resource::PhloResourceLimits;
use models::rust::phlo_wire::PhloWireLimits;

use super::*;

pub(super) struct Seed {
    pub root: StateHash,
    pub stacks: Vec<supply::PurseStack>,
    tail: Vec<u8>,
}

pub(super) async fn seed(
    manager: &RuntimeManager,
    policy: &AdoptedResourcePolicy,
    root: &StateHash,
    treasury: &VaultAddress,
    payers: &[VaultPayer],
    resource: PhloResource<'_>,
    limits: (PhloWireLimits, PhloExecutionLimits),
) -> Seed {
    let (wire, execution) = limits;
    let mut runtime = RuntimeOps::new(manager.spawn_runtime().await);
    let mut paid = root.clone();
    for (index, payer) in payers.iter().enumerate() {
        paid = successful_system_state(
            runtime
                .play_system_deploy(
                    &paid,
                    &mut transfer(
                        &payer.address,
                        treasury,
                        18,
                        0xa0 + u8::try_from(index).unwrap(),
                    ),
                )
                .await
                .unwrap(),
        );
    }
    runtime
        .runtime
        .reset(&Blake2b256Hash::from_bytes_prost(&paid))
        .await
        .unwrap();
    let signature = accounting::authority::sig_to_cost_signature(resource.authority).unwrap();
    let channel = supply::supply_channel(resource.authority);
    runtime
        .runtime
        .reducer
        .space
        .produce(
            channel.clone(),
            ListParWithRandom {
                pars: Vec::new(),
                random_state: vec![0x77],
                cost_authority: None,
                cost_stack: Some(CostStack {
                    cells: vec![signature.clone(); 2],
                }),
            },
            false,
        )
        .await
        .unwrap();
    let stacks =
        supply::decode_purse_inventory(&runtime.get_data_datums(&channel).await, &signature)
            .unwrap()
            .stacks;
    assert_eq!(stacks.len(), 1);
    let source = stacks[0].source_hash;
    let encoded = resource
        .wire_key(execution)
        .unwrap()
        .encode(PhloResourceLimits {
            wire,
            authority_nodes: execution.authority_nodes,
        })
        .unwrap();
    let mut contributions: Vec<_> = payers
        .iter()
        .map(|payer| NativePrepaidContribution {
            custody: payer.custody_key,
            amount: 9,
        })
        .collect();
    contributions.sort_by_key(|source| source.custody);
    let cells: Vec<_> = (0..2)
        .map(|index| {
            NativePrepaidCell::encode(
                policy,
                NativePrepaidOrigin {
                    genesis_root: policy.genesis().genesis_root().as_ref().try_into().unwrap(),
                    pre_state_root: root.as_ref().try_into().unwrap(),
                    deploy_id: [0x78; 32],
                    birth_source: source,
                    cell_index: index,
                },
                &encoded,
                &contributions,
                NativePrepaidCellLimits {
                    wire,
                    authority_nodes: execution.authority_nodes,
                    sources: payers.len(),
                },
            )
            .unwrap()
        })
        .collect();
    let ordered = OrderedPrepaidCells::encode(
        &cells.iter().map(Vec::as_slice).collect::<Vec<_>>(),
        PrepaidCellLimits { cells: 2, wire },
    )
    .unwrap();
    let limits = PrepaidReceiptBucketLimits {
        occurrences: 1,
        wire,
    };
    let bucket = PrepaidReceiptBucket::new(source, &[&ordered], limits)
        .unwrap()
        .encode(limits)
        .unwrap();
    runtime
        .replace_prepaid_receipts(
            &[PrepaidReceiptChange {
                receipt_id: PrepaidReceiptBucket::key_for_source(&source),
                expected: None,
                replacement: Some(&bucket),
            }],
            PrepaidReceiptLimits {
                entries: 1,
                value_bytes: wire.total_bytes,
                batch_bytes: wire.total_bytes,
            },
        )
        .await
        .unwrap();
    Seed {
        root: runtime
            .runtime
            .create_checkpoint()
            .await
            .root
            .to_bytes_prost(),
        stacks,
        tail: cells[1].clone(),
    }
}

pub(super) async fn verify_remaining(seed: &Seed, runtime: &RuntimeOps, wire: PhloWireLimits) {
    let before = &seed.stacks[0];
    let data = runtime.get_data_datums(&before.channel).await;
    let stacks = supply::decode_purse_inventory(&data, &before.stack.cells[0])
        .unwrap()
        .stacks;
    assert_eq!(stacks.len(), 1);
    assert_eq!(stacks[0].stack.cells, before.stack.cells[1..]);
    let key = PrepaidReceiptBucket::key_for_source(&stacks[0].source_hash);
    let bytes = runtime
        .read_prepaid_receipt(&key, wire.total_bytes)
        .await
        .unwrap()
        .unwrap();
    let bucket = PrepaidReceiptBucket::decode(&bytes, PrepaidReceiptBucketLimits {
        occurrences: 1,
        wire,
    })
    .unwrap();
    bucket.check_occurrences(&stacks[0].source_hash, 1).unwrap();
    let ordered =
        OrderedPrepaidCells::decode(bucket.receipts()[0], PrepaidCellLimits { cells: 2, wire })
            .unwrap();
    assert_eq!(ordered.cells(), &[seed.tail.as_slice()]);
    assert!(runtime
        .read_prepaid_receipt(
            &PrepaidReceiptBucket::key_for_source(&before.source_hash),
            wire.total_bytes,
        )
        .await
        .unwrap()
        .is_none());
}
