use casper::rust::util::{construct_deploy, proto_util};
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::Par;
use num_bigint::BigInt;
use rspace_plus_plus::rspace::merger::merging_logic::{
    apply_mergeable_value, MergeType, INTEGER_ADD_BITS,
};

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

fn integer_add_max() -> BigInt { (BigInt::from(1) << INTEGER_ADD_BITS) - 1 }

fn with_non_negative_number(body: &str) -> String {
    format!(
        r#"
new return, rl(`rho:registry:lookup`), nnCh, ch, firstCh, secondCh in {{
  rl!(`rho:lang:nonNegativeNumber`, *nnCh) |
  for (@(_, NonNegativeNumber) <- nnCh) {{
    {body}
  }}
}}
"#
    )
}

async fn run(node: &TestNode, term: String) -> Result<Vec<Par>, String> {
    let deploy = construct_deploy::source_deploy_now_full(
        term,
        Some(5_000_000),
        None,
        Some(construct_deploy::DEFAULT_SEC.clone()),
        None,
        Some(node.genesis.shard_id.clone()),
    )
    .unwrap();
    node.runtime_manager
        .capture_results(&proto_util::post_state_hash(&node.genesis), &deploy)
        .await
        .map_err(|e| format!("{e:?}"))
}

fn bools(results: &[Par]) -> Vec<bool> {
    let Some(ExprInstance::ETupleBody(tuple)) = &results[0].exprs[0].expr_instance else {
        panic!("expected a tuple result, got {results:?}");
    };
    tuple
        .ps
        .iter()
        .map(|p| match p.exprs[0].expr_instance {
            Some(ExprInstance::GBool(b)) => b,
            ref other => panic!("expected a bool, got {other:?}"),
        })
        .collect()
}

/// `NonNegativeNumber.rho` and `INTEGER_ADD_BITS` encode one consensus bound
/// independently. Both must accept 2^N - 1 and reject 2^N, with N taken from
/// the Rust constant.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn non_negative_number_and_merge_agree_on_the_integer_add_bound() {
    let max = integer_add_max();
    let past_max = &max + 1;
    assert!(apply_mergeable_value(&BigInt::from(0), &max, MergeType::IntegerAdd).is_some());
    assert!(apply_mergeable_value(&max, &BigInt::from(1), MergeType::IntegerAdd).is_none());

    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .unwrap();
    let node = TestNode::standalone(genesis).await.unwrap();

    let adds = run(
        &node,
        with_non_negative_number(&format!(
            r#"@NonNegativeNumber!(0, *ch) |
    for (nn <- ch) {{
      nn!("add", {max}n, *firstCh) |
      for (@atMax <- firstCh) {{
        nn!("add", 1, *secondCh) |
        for (@pastMax <- secondCh) {{ return!((atMax, pastMax)) }}
      }}
    }}"#
        )),
    )
    .await
    .unwrap();
    assert_eq!(
        bools(&adds),
        vec![true, false],
        "add up to 2^N - 1 succeeds, past it fails"
    );

    let init_at_max = run(
        &node,
        with_non_negative_number(&format!(
            r#"@NonNegativeNumber!({max}n, *ch) |
    for (nn <- ch) {{
      nn!("value", *firstCh) |
      for (@v <- firstCh) {{ return!((v == {max}n, true)) }}
    }}"#
        )),
    )
    .await
    .unwrap();
    assert_eq!(
        bools(&init_at_max),
        vec![true, true],
        "init at 2^N - 1 is kept"
    );

    let init_past_max = run(
        &node,
        with_non_negative_number(&format!(
            r#"@NonNegativeNumber!({past_max}n, *ch) |
    for (_ <- ch) {{ return!((true, true)) }}"#
        )),
    )
    .await;
    assert!(
        init_past_max
            .as_ref()
            .is_err_and(|e| e.contains("UserAbortError")),
        "init at 2^N must abort the deploy, got {init_past_max:?}"
    );
}
