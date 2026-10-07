use std::collections::BTreeMap;

use proptest::prelude::*;
use proptest::test_runner::{Config, FileFailurePersistence};

use super::*;

async fn check_parallel_chains(chains: &[Vec<u8>], reverse_sends: bool, payload: i32) {
    let mut processes = Vec::new();
    let mut messages = Vec::new();
    let mut expected = BTreeMap::<[u8; 32], u64>::new();
    let mut expected_comms = 0;
    for (chain, payers) in chains.iter().enumerate() {
        let mut body = format!("@\"output{chain}\"!({payload})");
        for stage in (0..payers.len()).rev() {
            let channel = format!("chain{chain}stage{stage}");
            body = format!("for(_ <- @\"{channel}\"){{ {body} }}");
            messages.push(format!("@\"{channel}\"!(0)"));
            let payer = lane(&format!("payer{}", payers[stage]));
            *expected.entry(payer).or_default() += 1;
            expected_comms += 1;
        }
        let authorities = payers
            .iter()
            .map(|payer| format!("payer{payer}"))
            .collect::<Vec<_>>()
            .join(" -o ");
        processes.push(format!("{{% {body} %}}[ {authorities} ]"));
    }
    if reverse_sends {
        messages.reverse();
        messages.extend(processes);
        processes = messages;
    } else {
        processes.extend(messages);
    }
    let mut runtime = runtime().await;
    evaluate(&mut runtime, &processes.join(" | ")).await;
    assert_eq!(runtime.cost.authority_realized().0, expected);
    assert_eq!(comm_count(&runtime), expected_comms);
    let events = runtime.cost.authority_events();
    assert_eq!(events.len(), expected_comms);
    for event in events {
        event.verify_authority().unwrap();
        let demand = authority_demand(&event.authority).unwrap();
        assert_eq!(demand.0.len(), 1);
        assert_eq!(demand.0.values().sum::<u64>(), 1);
        assert!(demand.0.keys().all(|payer| expected.contains_key(payer)));
    }
    let byte_events = runtime.cost.authority_byte_events();
    for event in &byte_events {
        event.verify_authority().unwrap();
        let demand = authority_demand(&event.authority).unwrap();
        assert!(demand.0.keys().all(|payer| expected.contains_key(payer)));
    }
    assert_eq!(
        byte_events.iter().map(|event| event.amount).sum::<u64>(),
        runtime.cost.quantitative_byte_cost()
    );
    for chain in 0..chains.len() {
        let output = new_gstring_par(format!("output{chain}"), Vec::new(), false);
        let data = runtime.get_data(&output).await;
        assert_eq!(data.len(), 1);
        let expected_payload = Compiler::source_to_adt(&payload.to_string()).unwrap();
        assert_eq!(data[0].a.pars, vec![expected_payload]);
    }
}

async fn check_staged_chains(chains: &[Vec<u8>], priorities: &[u16], payload: i32) {
    let mut processes = Vec::new();
    let mut deliveries = Vec::new();
    for (chain, payers) in chains.iter().enumerate() {
        let mut body = format!("@\"output{chain}\"!({payload})");
        for stage in (0..payers.len()).rev() {
            body = format!("for(_ <- @\"chain{chain}stage{stage}\"){{ {body} }}");
        }
        for stage in 0..payers.len() {
            let priority = priorities
                .get(deliveries.len())
                .copied()
                .unwrap_or_default();
            deliveries.push((priority, chain, stage));
        }
        let authorities = payers
            .iter()
            .map(|payer| format!("payer{payer}"))
            .collect::<Vec<_>>()
            .join(" -o ");
        processes.push(format!("{{% {body} %}}[ {authorities} ]"));
    }
    deliveries.sort();
    let mut runtime = runtime().await;
    evaluate(&mut runtime, &processes.join(" | ")).await;
    assert!(runtime.cost.authority_events().is_empty());
    let mut arrived = chains
        .iter()
        .map(|chain| vec![false; chain.len()])
        .collect::<Vec<_>>();
    let mut positions = vec![0; chains.len()];
    let mut total = BTreeMap::<[u8; 32], u64>::new();
    for (_, chain, stage) in deliveries {
        arrived[chain][stage] = true;
        let mut expected = BTreeMap::<[u8; 32], u64>::new();
        while positions[chain] < chains[chain].len() && arrived[chain][positions[chain]] {
            let payer = lane(&format!("payer{}", chains[chain][positions[chain]]));
            *expected.entry(payer).or_default() += 1;
            *total.entry(payer).or_default() += 1;
            positions[chain] += 1;
        }
        evaluate(&mut runtime, &format!("@\"chain{chain}stage{stage}\"!(0)")).await;
        assert_eq!(runtime.cost.authority_realized().0, expected);
        let events = runtime.cost.authority_events();
        assert_eq!(events.len() as u64, expected.values().sum::<u64>());
        for event in events {
            event.verify_authority().unwrap();
            assert_eq!(event.debit.0.values().sum::<u64>(), 1);
        }
        for (index, payers) in chains.iter().enumerate() {
            let output = new_gstring_par(format!("output{index}"), Vec::new(), false);
            let data = runtime.get_data(&output).await;
            if positions[index] == payers.len() {
                assert_eq!(data.len(), 1);
                assert_eq!(data[0].a.pars, vec![Compiler::source_to_adt(
                    &payload.to_string()
                )
                .unwrap()]);
            } else {
                assert!(data.is_empty());
            }
        }
    }
    let mut expected_total = BTreeMap::<[u8; 32], u64>::new();
    for payer in chains.iter().flatten() {
        *expected_total
            .entry(lane(&format!("payer{payer}")))
            .or_default() += 1;
    }
    assert_eq!(total, expected_total);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queued_inner_messages_wait_for_each_outer_transfer() {
    check_staged_chains(
        &[vec![0, 1, 0, 2], vec![1, 1, 1]],
        &[6, 4, 2, 0, 5, 3, 1],
        42,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn parallel_transfer_chains_preserve_repeated_and_distinct_payers() {
    for reverse in [false, true] {
        check_parallel_chains(
            &[vec![0, 1, 2, 3, 4, 5, 6, 7], vec![0, 0, 0, 0], vec![
                7, 3, 7,
            ]],
            reverse,
            42,
        )
        .await;
    }
}

proptest! {
    #![proptest_config(Config {
        cases: 32,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/proptest-regressions/accounting/located-authority-transfer.txt",
        )))),
        ..Config::default()
    })]

    #[test]
    fn generated_transfer_arrivals_preserve_causal_authority(
        chains in prop::collection::vec(prop::collection::vec(0_u8..8, 2..9), 1..5),
        priorities in prop::collection::vec(any::<u16>(), 0..33),
        payload in any::<i32>(),
    ) {
        let executor = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2).enable_all().build().unwrap();
        executor.block_on(check_staged_chains(&chains, &priorities, payload));
    }

    #[test]
    fn generated_parallel_transfer_chains_preserve_authority(
        chains in prop::collection::vec(prop::collection::vec(0_u8..16, 2..9), 1..5),
        reverse in any::<bool>(), payload in any::<i32>(),
    ) {
        let executor = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        executor.block_on(check_parallel_chains(&chains, reverse, payload));
    }
}
