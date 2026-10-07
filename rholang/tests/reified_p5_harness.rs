//! The shared harness of the P5 suites (P5 design v3, A.3.2), checked through the engine of S1.
//!
//! The P5 batches build their suites on `support/`. These tests check its parts before the
//! batches rely on them. The session runs a `hashmap:queue` space that the runtime agent, the
//! family agent, runs. The property runs the family agent directly in the in-memory agent
//! environment.

#[path = "support/mod.rs"]
mod support;

use models::rust::space_factory_urn::SpaceFactoryUrn;
use proptest::collection::vec;
use proptest::prelude::*;
use rholang::rust::interpreter::spaces::agent::SpaceOp;
use rholang::rust::interpreter::spaces::families::FamilyAgent;
use rholang::rust::interpreter::spaces::namespace::{Durability, NativeSpace, SID_LEN};
use rholang::rust::interpreter::spaces::types::qualifier::SpaceQualifier;
use support::agent::{
    any, budget, config_of, counter, counter_channel, create, creation, data_channel, env_over,
    fired, int, play, receive_on, records_on, run, send_on, string,
};
use support::props::{check, pure_config, tokio_runtime};
use support::session::{with_space, Session};

/// The session creates a space with its factory, and the deploys of a session reach it through
/// the family agent: the records take the orders of the counter, and a receive takes the head.
#[tokio::test]
async fn harness_session_runs_a_queue_space_under_the_family_agent() {
    let mut session = Session::new().await;
    let space = session
        .setup("rho:space:hashmap:queue", r#""default""#)
        .await;
    for value in [5, 3] {
        session
            .run(
                &with_space(&format!("a!({value})")),
                &format!("rrs-p5-harness-send-{value}"),
            )
            .await;
    }
    assert_eq!(session.records(space.space, &space.a).await, vec![
        (0, vec![int(5)], false),
        (1, vec![int(3)], false)
    ]);
    assert_eq!(session.counter(space.space, &space.a).await, 2);
    session
        .run(
            &with_space(r#"for (@v <- a) { @"rrs-p5-harness-got"!(v) }"#),
            "rrs-p5-harness-receive",
        )
        .await;
    assert_eq!(session.sole("rrs-p5-harness-got").await, int(5));
    assert_eq!(session.records(space.space, &space.a).await, vec![(
        1,
        vec![int(3)],
        false
    )]);
    assert_eq!(session.counter(space.space, &space.a).await, 3);
    assert!(session.parked(space.space, &space.b).await.is_empty());
}

/// Over the in-memory agent environment, the family agent delivers the sends of a queue to the
/// receives in send order, and each operation bumps the counter once.
#[test]
fn harness_agent_env_runs_the_family_agent_in_fifo_order() {
    check(pure_config(), vec(0i64..10, 1..6), |values| {
        tokio_runtime().block_on(async {
            let host = play().await;
            let budget = budget();
            let env = env_over(host.clone(), &budget);
            let space = NativeSpace {
                sid: [9; SID_LEN],
                durability: Durability::Durable,
            };
            let config = config_of(space, "hashmap", "queue", SpaceQualifier::Default);
            create(
                &FamilyAgent,
                &env,
                creation(space, config, SpaceFactoryUrn::HASHMAP_QUEUE),
            )
            .await;
            let channel = string("rrs-p5-harness");
            for value in &values {
                run(
                    &FamilyAgent,
                    &env,
                    SpaceOp::Produce(send_on(space, &channel, *value, false)),
                )
                .await
                .expect("the send commits");
            }
            let mut delivered = Vec::with_capacity(values.len());
            for index in 0..values.len() {
                let outcome = run(
                    &FamilyAgent,
                    &env,
                    SpaceOp::Consume(receive_on(space, &channel, &format!("w{index}"), any())),
                )
                .await
                .expect("the receive commits");
                delivered.extend(fired(&outcome).into_iter().flat_map(|(_, bound)| bound));
            }
            let sent: Vec<_> = values.iter().map(|value| int(*value)).collect();
            prop_assert_eq!(delivered, sent);
            prop_assert!(records_on(&host, &data_channel(space, &channel))
                .await
                .is_empty());
            let operations = i64::try_from(2 * values.len()).expect("few operations");
            prop_assert_eq!(
                counter(&host, &counter_channel(space, &channel)).await,
                vec![operations]
            );
            Ok(())
        })
    });
}
