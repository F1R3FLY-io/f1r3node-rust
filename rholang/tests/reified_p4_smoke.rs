// The P4 smoke test of Reified RSpaces: the first working space, `hashmap:queue`, end to end.
//
// One deploy in ordinary Rholang syntax takes these steps. It uses no `use` block and no
// `new x : s` annotation, because the parser that reads them arrives with P3b.
//
// 1. It creates a `hashmap:queue` space through the factory URN with `!?`.
// 2. It calls the handle method `gensym`, which replies a name of the space.
// 3. It sends the values 2 and 1 on that name through the handle method `produce`.
// 4. It receives twice on the name. The name routes to its space (rule R3), so each receive is
//    a consume of the queue.
//
// The test requires the values in the order of the produce calls, and an empty queue
// afterwards. The play log must show two records on the data channel of the queue and two
// COMMs that remove them. The test then replays the play log and requires an equal root, an
// equal cost and a replay that consumes the complete log.
//
// Changed by decision 90 (C6, decision 69): spaces have one execution model, native funded
// execution. So the deploy plays on the native session of the shared harness
// (`support::session`), which checkpoints the deploy and certifies it by native replay of its own
// record from the pre-state: an equal root, equal errors, the complete journal and an equal usage.
// The legacy play runtime and the rigged replay of the play log are commented out below.

// Added by decision 90 (C6): the native session of the shared harness.
#[path = "support/mod.rs"]
mod support;

// Disabled by decision 90 (C6): the native session holds the runtime and plays the deploy.
// use std::collections::HashMap;
//
// use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::Par;
use models::rust::utils::{new_elist_par, new_gint_par, new_gstring_par};
// Disabled by decision 90 (C6): the native session holds the runtime and plays the deploy.
// use rholang::rust::interpreter::accounting::costs::Cost;
// use rholang::rust::interpreter::accounting::Sig;
use rholang::rust::interpreter::reified::route::{classify_channel, ChannelClass};
use rholang::rust::interpreter::reified::scope::{classify_space_ref, SpaceRef};
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::spaces::namespace::{self, CollectionKey};
// Disabled by decision 90 (C6): the native session holds the runtime and plays the deploy.
// use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::hash;
// Disabled by decision 90 (C6): the native session holds the runtime and plays the deploy.
// use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
// use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent};
// Added by decision 90 (C6): the native session of the shared harness.
use support::session::Session;

// Disabled by decision 90 (C6): the native session funds its own deploy signature and limit.
// const DEPLOY_SIGNATURE: &[u8] = b"rrs-p4-smoke-deploy";
// const FUNDING_SIGNATURE: &[u8] = b"rrs-p4-smoke-payer";
// const EVALUATION_PHLO: i64 = 100_000_000;
const LABEL: &str = "rrs-p4-smoke";
const HANDLE_OUT: &str = "rrs-p4-smoke-handle";
const NAME_OUT: &str = "rrs-p4-smoke-name";
const RECEIVED_OUT: &str = "rrs-p4-smoke-received";

// The values go into the queue as 2 and then 1, so the FIFO order differs from the order of
// the values.
const PROGRAM: &str = r#"
new factory(`rho:space:hashmap:queue`) in {
  for (queue <- factory!?("default")) {
    @"rrs-p4-smoke-handle"!(*queue) |
    for (box <- queue!?("gensym")) {
      @"rrs-p4-smoke-name"!(*box) |
      for (_ <- queue!?("produce", *box, [2])) {
        for (_ <- queue!?("produce", *box, [1])) {
          for (@first <- box) {
            for (@second <- box) {
              @"rrs-p4-smoke-received"!([first, second])
            }
          }
        }
      }
    }
  }
}
"#;

// Disabled by decision 90 (C6): the native session derives the random state from `LABEL`.
// fn seed() -> Blake2b512Random { Blake2b512Random::create_from_bytes(LABEL.as_bytes()) }

fn string_par(value: &str) -> Par { new_gstring_par(value.to_owned(), Vec::new(), false) }

fn int_par(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

fn list_par(ps: Vec<Par>) -> Par { new_elist_par(ps, Vec::new(), false, None, Vec::new(), false) }

// Disabled by decision 90 (C6): the native session replaces the legacy play and replay
// runtimes.
// /// A play runtime and a replay runtime over one in-memory store, with a funded deploy.
// async fn fresh_runtimes() -> (RhoRuntimeImpl, RhoRuntimeImpl) {
//     let mut manager = InMemoryStoreManager::new();
//     let stores = manager
//         .r_space_stores()
//         .await
//         .expect("create the in-memory RSpace stores");
//     let (play, replay, _history) = create_runtimes(stores, false, &mut Vec::new()).await;
//     for runtime in [&play, &replay] {
//         runtime
//             .cost
//             .set_deploy_signature_funded(DEPLOY_SIGNATURE, Sig::Ground(FUNDING_SIGNATURE.to_vec()));
//     }
//     (play, replay)
// }

/// The data of the only datum on the public channel `@channel`.
async fn sole_datum(runtime: &RhoRuntimeImpl, channel: &str) -> Vec<Par> {
    let data = runtime.get_data(&string_par(channel)).await;
    match <[_; 1]>::try_from(data) {
        Ok([datum]) => datum.a.pars,
        Err(data) => panic!(
            "@{channel:?} holds {} data instead of one: {data:?}",
            data.len()
        ),
    }
}

/// The only `Par` of the only datum on the public channel `@channel`.
async fn sole_value(runtime: &RhoRuntimeImpl, channel: &str) -> Par {
    match <[_; 1]>::try_from(sole_datum(runtime, channel).await) {
        Ok([value]) => value,
        Err(pars) => panic!(
            "@{channel:?} holds a datum of {} values: {pars:?}",
            pars.len()
        ),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_p4_smoke_hashmap_queue_play_and_replay() {
    // Changed by decision 90 (C6): the native session plays the deploy with the random state of
    // `LABEL`, checks that it reports no error and certifies it by native replay. The legacy play
    // follows.
    // let (mut play, mut replay) = fresh_runtimes().await;
    // let start_root = play.get_root().await;
    //
    // let played = play
    //     .evaluate(
    //         PROGRAM,
    //         Cost::create(EVALUATION_PHLO, LABEL),
    //         HashMap::new(),
    //         seed(),
    //     )
    //     .await
    //     .expect("the play evaluation returns a result");
    // assert!(
    //     played.errors.is_empty(),
    //     "the play reported errors: {:?}",
    //     played.errors
    // );
    let mut session = Session::new().await;
    let deploy = session.run(PROGRAM, LABEL).await;
    // `play` borrows the runtime of the session, so the readers below take it as it is.
    let play = &session.play;

    // FIFO: the two receives deliver the values in the order of the produce calls.
    assert_eq!(
        sole_value(play, RECEIVED_OUT).await,
        list_par(vec![int_par(2), int_par(1)]),
        "the queue delivers its records in FIFO order"
    );

    // The factory replied a writable handle of a native space, and `gensym` replied a name
    // of that space.
    let handle = sole_value(play, HANDLE_OUT).await;
    let space = match classify_space_ref(&handle) {
        Ok(SpaceRef::Native(space)) => space,
        other => panic!("the factory reply is not a native space handle: {other:?}"),
    };
    let name = sole_value(play, NAME_OUT).await;
    assert_eq!(
        classify_channel(&name),
        ChannelClass::SpaceName(space),
        "the gensym name routes to the space of the handle"
    );

    // The queue is empty afterwards: its data channel holds no record and no waiter.
    let key = CollectionKey::of_channel(&name).expect("the name has a collection key");
    let data_channel = namespace::gprivate(namespace::data_channel(space, &key));
    assert!(
        play.get_data(&data_channel).await.is_empty(),
        "the queue holds no record after the two receives"
    );
    assert!(
        play.get_continuations(vec![data_channel.clone()])
            .await
            .is_empty(),
        "the queue holds no waiter after the two receives"
    );

    // The emptiness is not vacuous. The play log stores the two records on the data channel of
    // the queue, and two COMMs on that channel remove them.
    // Changed by decision 90 (C6): the event log of the native play is the log of the deploy.
    // The legacy reader of the checkpoint log follows.
    // let play_checkpoint = play.create_checkpoint().await;
    // let data_hash = hash(&data_channel);
    // let stored = play_checkpoint
    //     .log
    //     .iter()
    let data_hash = hash(&data_channel);
    let stored = deploy
        .log
        .iter()
        .filter(|event| {
            matches!(event, Event::IoEvent(IOEvent::Produce(produce)) if produce.channel_hash == data_hash)
        })
        .count();
    // Changed by decision 90 (C6): the event log of the native play is the log of the deploy.
    // let fired = play_checkpoint
    //     .log
    //     .iter()
    let fired = deploy
        .log
        .iter()
        .filter(|event| {
            matches!(event, Event::Comm(comm) if comm.consume.channel_hashes == [data_hash.clone()])
        })
        .count();
    assert_eq!(
        (stored, fired),
        (2, 2),
        "the queue stores two records and two COMMs remove them"
    );

    // Changed by decision 90 (C6): `Session::run` already certified the deploy by native replay of
    // its own record from the pre-state: the replay reaches the play root with the same errors,
    // the complete journal and the same usage. The legacy replay of the play log follows.
    // replay
    //     .reset(&start_root)
    //     .await
    //     .expect("reset the replay runtime to the start root");
    // replay
    //     .rig(play_checkpoint.log.clone())
    //     .await
    //     .expect("rig the replay runtime with the play log");
    // let replayed = replay
    //     .evaluate(
    //         PROGRAM,
    //         Cost::create(EVALUATION_PHLO, LABEL),
    //         HashMap::new(),
    //         seed(),
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
    // let replay_checkpoint = replay.create_checkpoint().await;
    // assert_eq!(
    //     replay_checkpoint.root, play_checkpoint.root,
    //     "the replay reaches the play root"
    // );
    // assert_eq!(
    //     replayed.cost.value, played.cost.value,
    //     "the replay charges the play cost"
    // );
}
