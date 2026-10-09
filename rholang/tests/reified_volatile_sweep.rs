//! The scope of the deploy-end volatile sweep of P7 (consensus checklist item 20, design v3 VR-5).
//!
//! The sweep removes only the volatile channels of the deploy. Ordinary channels keep every record
//! and every waiting continuation, so a deploy with a `temp` space reaches the post-state root of
//! the same deploy without the space. Every deploy plays natively at the v6 limits and certifies
//! through its committed evidence (`support::native`).
//!
//! | Test | Property |
//! |---|---|
//! | `reified_volatile_sweep_skips_ordinary_channels` | A deploy with ordinary records, a persistent record, a persistent waiter and a `temp` space with records and a waiter reaches the post-state root of the same deploy without the space. The control with `default` leaves durable space state, so its root differs. |

#[path = "support/mod.rs"]
mod support;

use support::native::{assert_reproduces, certify, native_play, Played, AMPLE_LIMIT};

/// The ordinary part of each deploy: a record, a persistent record and a persistent waiter on
/// public channels. The terms of the part come first in the evaluation order of the deploy, so
/// their random states do not depend on the rest of the deploy.
const ORDINARY: &str = r#"@"rrs-p7-ordinary"!(1) | @"rrs-p7-ordinary"!!(2) |
for (@x <= @"rrs-p7-ordinary-wait") { Nil }"#;

/// A deploy of the ordinary part and a space of the qualifier `qualifier` whose collection keeps
/// two records and a waiter at the end of the deploy.
fn with_space(qualifier: &str) -> String {
    format!(
        r#"{ORDINARY} |
new factory(`rho:space:hashmap:bag`) in {{
  for (space <- factory!?("{qualifier}")) {{
    use space {{ new x in {{ x!(1) | x!(2) | for (@"never" <- x) {{ Nil }} }} }}
  }}
}}"#
    )
}

/// The same deploy without the space: the factory binder stays, so the ordinary terms keep
/// their positions in the evaluation order.
fn without_space() -> String { format!("{ORDINARY} |\nnew factory in {{ Nil }}") }

/// Plays `term` natively and certifies it.
async fn play_and_certify(label: &str, term: &str) -> Played {
    let played = native_play(term, AMPLE_LIMIT).await;
    assert!(played.result.errors.is_empty(), "{label}: {:?}", played.result.errors);
    assert!(
        played.residue.is_empty(),
        "{label}: the sweep leaves no volatile channel: {:?}",
        played.residue
    );
    let certification = certify(&played, term, AMPLE_LIMIT).await;
    assert_reproduces(label, &played, &certification);
    played
}

/// The `temp` deploy reaches the root of the deploy without the space, and the `default` control
/// keeps durable space state and reaches another root.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_volatile_sweep_skips_ordinary_channels() {
    let plain = play_and_certify("without a space", &without_space()).await;
    let ephemeral = play_and_certify("temp space", &with_space("temp")).await;
    assert!(ephemeral.durable.is_empty(), "{:?}", ephemeral.durable);
    assert_eq!(
        ephemeral.root, plain.root,
        "the sweep leaves the ordinary channels of the deploy without the space"
    );
    let durable = play_and_certify("default space", &with_space("default")).await;
    assert!(!durable.durable.is_empty(), "the control keeps durable space state");
    assert_ne!(durable.root, plain.root, "the control reaches another root");
}
