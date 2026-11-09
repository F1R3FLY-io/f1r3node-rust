// P4 source-level test of the `hashmap:queue` space: one Rholang program in the Reified syntax of
// the v3 parser runs end to end (plan phase P4, `space_queue_source_program`).
//
// The program creates a space through the factory URN, puts a new name into the space with the
// annotation `new box : queue`, and uses a `use queue { … }` block in which the untagged channel
// `@"jobs"` is a collection of the space. A handle call and a send in the use block fill the
// queue, and two receives in the use block take the values in arrival order. The results go to
// host names that the program makes outside the use block. The test checks the normalized
// program, the results, the state of the space, and the replay of the play log.
//
// Changed by decision 90 (C6, decision 69): spaces have one execution model, native funded
// execution. So the program plays on the native session of the shared harness
// (`support::session`), which checkpoints the deploy and certifies it by native replay from its
// pre-state. The legacy play runtime and the rigged replay of the play log are commented out
// below.

// Added by decision 90 (C6): the native session of the shared harness.
#[path = "support/mod.rs"]
mod support;

// Disabled by decision 90 (C6): the native session holds the runtime and plays the deploy.
// use std::collections::HashMap;
//
// use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, Expr, Par};
// Retired by decision 112: the restated test reads no marker and no prost message.
// ```
// use models::rust::reified::encoding::Marker;
// use prost::Message;
// ```
// Disabled by decision 90 (C6): the native session holds the runtime and plays the deploy.
// use rholang::rust::interpreter::accounting::costs::Cost;
// use rholang::rust::interpreter::accounting::Sig;
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::reified::route::{classify_channel, ChannelClass};
use rholang::rust::interpreter::reified::scope::{classify_space_ref, SpaceRef};
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::spaces::namespace::{self, CollectionKey, NativeSpace};
use rholang::rust::interpreter::spaces::record;
// Disabled by decision 90 (C6): the native session holds the runtime and plays the deploy.
// use rholang::rust::interpreter::test_utils::resources::create_runtimes;
// use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
// use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
// Added by decision 90 (C6): the native session of the shared harness.
use support::session::Session;

// Disabled by decision 90 (C6): the native session funds its own deploy signature and limit.
// const DEPLOY_SIGNATURE: &[u8] = b"rrs-p4-source-deploy";
// const FUNDING_SIGNATURE: &[u8] = b"rrs-p4-source-payer";
// const PHLO: i64 = 100_000_000;
const LABEL: &str = "rrs-p4-source-program";

const PROGRAM: &str = r#"
new factory(`rho:space:hashmap:queue`), results in {
  for (queue <- factory!?("default")) {
    @"rrs-handle"!(*queue) |
    new box : queue, plain in {
      @"rrs-box"!(*box) |
      for (_ <- queue!?("produce", "jobs", ["first"])) {
        use queue {
          @"jobs"!("second") |
          for (@x <- @"jobs") {
            for (@y <- @"jobs") {
              box!(x, y) |
              for (@p, @q <- box) { results!([p, q]) }
            }
          }
        }
      } |
      plain!(1) |
      for (@z <- plain) { @"rrs-plain"!(z) }
    } |
    for (@pair <- results) { @"rrs-source-out"!(pair) }
  }
}
"#;

fn string(value: &str) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GString(value.to_owned())),
    }])
}

fn int(value: i64) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(value)),
    }])
}

fn list(ps: Vec<Par>) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EListBody(EList {
            ps,
            ..EList::default()
        })),
    }])
}

// Retired by decision 112: the normalized program holds protobuf fields and no marker names, so
// the byte search has no subject. The new helper `holds_field` reads the fields. The text before:
// ```
// /// True when the bytes of `par` hold the marker name of `marker`.
// fn holds_marker(par: &Par, marker: Marker) -> bool {
//     let bytes = par.encode_to_vec();
//     let [tag, index] = marker.id();
//     let pattern = [0x0a, 0x02, tag, index];
//     bytes.windows(pattern.len()).any(|window| window == pattern)
// }
// ```

/// True when `par`, or a process in a new, a receive or a use block of `par`, satisfies `found`.
fn holds_field(par: &Par, found: &dyn Fn(&Par) -> bool) -> bool {
    let bodies = par
        .news
        .iter()
        .filter_map(|new| new.p.as_ref())
        .chain(
            par.receives
                .iter()
                .filter_map(|receive| receive.body.as_ref()),
        )
        .chain(
            par.use_blocks
                .iter()
                .filter_map(|block| block.body.as_ref()),
        );
    found(par) || bodies.into_iter().any(|body| holds_field(body, found))
}

async fn values(runtime: &RhoRuntimeImpl, channel: &str) -> Vec<Vec<Par>> {
    runtime
        .get_data(&string(channel))
        .await
        .into_iter()
        .map(|datum| datum.a.pars)
        .collect()
}

async fn sole(runtime: &RhoRuntimeImpl, channel: &str) -> Par {
    match <[Vec<Par>; 1]>::try_from(values(runtime, channel).await) {
        Ok([pars]) => match <[Par; 1]>::try_from(pars) {
            Ok([value]) => value,
            Err(pars) => panic!("@{channel:?} holds a datum of {} values", pars.len()),
        },
        Err(data) => panic!("@{channel:?} holds {} data instead of one", data.len()),
    }
}

fn collection(space: NativeSpace, channel: &Par) -> (Par, Par) {
    let key = CollectionKey::of_channel(channel).expect("a channel has a collection key");
    (
        namespace::gprivate(namespace::data_channel(space, &key)),
        namespace::gprivate(namespace::collection_counter(space, &key)),
    )
}

async fn counter(runtime: &RhoRuntimeImpl, channel: &Par) -> i64 {
    match runtime.get_data(channel).await.as_slice() {
        [datum] => record::counter_value(&datum.a).expect("a counter record decodes"),
        other => panic!("a counter channel holds {} records", other.len()),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_queue_source_program() {
    // Changed by decision 112: the Reified syntax normalizes to protobuf fields. The text before:
    // // The v3 parser and the node normalizer turn the Reified syntax into marker tuples.
    // The v3 parser and the node normalizer turn the Reified syntax into protobuf fields.
    let normalized = Compiler::source_to_adt(PROGRAM).expect("the program normalizes");
    // Changed by decision 112: the use block and the annotated new are fields of the Par. The
    // text before:
    // ```
    // assert!(
    //     holds_marker(&normalized, Marker::Use),
    //     "the use block is a USE tuple"
    // );
    // assert!(
    //     holds_marker(&normalized, Marker::NewSpace),
    //     "the annotated new is a NEWSPACE tuple"
    // );
    // ```
    assert!(
        holds_field(&normalized, &|par| !par.use_blocks.is_empty()),
        "the use block is a field of the Par"
    );
    assert!(
        holds_field(&normalized, &|par| par
            .news
            .iter()
            .any(|new| !new.space_types.is_empty())),
        "the annotated new holds its space annotations"
    );

    // Changed by decision 90 (C6): the native session plays the program with the random state of
    // `LABEL` and certifies it by native replay. `Session::run` checks that the play reports no
    // error. The legacy play runtime follows.
    // let mut manager = InMemoryStoreManager::new();
    // let stores = manager
    //     .r_space_stores()
    //     .await
    //     .expect("create the in-memory RSpace stores");
    // let (mut play, mut replay, _history) = create_runtimes(stores, false, &mut Vec::new()).await;
    // for runtime in [&play, &replay] {
    //     runtime
    //         .cost
    //         .set_deploy_signature_funded(DEPLOY_SIGNATURE, Sig::Ground(FUNDING_SIGNATURE.to_vec()));
    // }
    // let start = play.get_root().await;
    // let played = play
    //     .evaluate(
    //         PROGRAM,
    //         Cost::create(PHLO, LABEL),
    //         HashMap::new(),
    //         Blake2b512Random::create_from_bytes(LABEL.as_bytes()),
    //     )
    //     .await
    //     .expect("the play evaluation returns a result");
    // assert!(
    //     played.errors.is_empty(),
    //     "the play reported errors: {:?}",
    //     played.errors
    // );
    let mut session = Session::new().await;
    session.run(PROGRAM, LABEL).await;
    // `play` borrows the runtime of the session, so the readers below take it as it is.
    let play = &session.play;

    // The use block delivers in arrival order: the record of the handle call, then the record
    // of the send.
    assert_eq!(
        sole(play, "rrs-source-out").await,
        list(vec![string("first"), string("second")])
    );
    assert_eq!(
        sole(play, "rrs-plain").await,
        int(1),
        "a plain binder is a host name"
    );

    // The annotated binder is a name of the space, and both collections are empty again.
    let space = match classify_space_ref(&sole(play, "rrs-handle").await) {
        Ok(SpaceRef::Native(space)) => space,
        other => panic!("the factory reply is not a native space handle: {other:?}"),
    };
    let box_name = sole(play, "rrs-box").await;
    assert_eq!(classify_channel(&box_name), ChannelClass::SpaceName(space));
    let (jobs_data, jobs_counter) = collection(space, &string("jobs"));
    let (box_data, box_counter) = collection(space, &box_name);
    assert!(play.get_data(&jobs_data).await.is_empty());
    assert!(play
        .get_continuations(vec![jobs_data.clone()])
        .await
        .is_empty());
    assert!(play.get_data(&box_data).await.is_empty());
    assert_eq!(
        counter(play, &jobs_counter).await,
        4,
        "the handle call, the send and the two receives each bump the jobs counter"
    );
    assert_eq!(
        counter(play, &box_counter).await,
        2,
        "one send and one receive on box"
    );
    assert!(
        values(play, "jobs").await.is_empty(),
        "no host data on @\"jobs\""
    );

    // Changed by decision 90 (C6): `Session::run` already certified the deploy by native replay
    // of its own record from the pre-state: the replay reaches the root of the play with the same
    // errors, journal and usage. The legacy replay of the play log follows.
    // // The replay of the play log reaches the same root with the same cost.
    // let checkpoint = play.create_checkpoint().await;
    // replay
    //     .reset(&start)
    //     .await
    //     .expect("reset the replay runtime to the start root");
    // replay
    //     .rig(checkpoint.log.clone())
    //     .await
    //     .expect("rig the replay runtime with the play log");
    // let replayed = replay
    //     .evaluate(
    //         PROGRAM,
    //         Cost::create(PHLO, LABEL),
    //         HashMap::new(),
    //         Blake2b512Random::create_from_bytes(LABEL.as_bytes()),
    //     )
    //     .await
    //     .expect("the replay evaluation returns a result");
    // assert!(
    //     replayed.errors.is_empty(),
    //     "the replay reported errors: {:?}",
    //     replayed.errors
    // );
    // replay
    //     .check_replay_data()
    //     .await
    //     .expect("the replay consumes the complete play log");
    // assert_eq!(replay.create_checkpoint().await.root, checkpoint.root);
    // assert_eq!(replayed.cost.value, played.cost.value);
}
