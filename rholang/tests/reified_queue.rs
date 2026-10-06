// P4 integration tests of the `hashmap:queue` space: the head-only FIFO discipline, the real
// events of each operation, the counters and the meta channels (P4 design B.2 to B.6, D.1, plan
// decisions 11, 30 and 31, A1, A2, A3 and A10).
//
// Each test runs Rholang deploys on a play runtime over an in-memory store. A deploy runs as
// Casper runs a user deploy: a soft checkpoint, the evaluation, the event log, and a revert of
// the soft checkpoint when the deploy fails. The setup deploy creates a space and two names of
// it, and later deploys read the handle and the names from the store with peeks.
//
// The event tables of P4 design B.6 name the staged operations of each queue operation. The
// tests compare the events of the log on the space channels with these tables: `C(sid)` is the
// config channel, `S(sid)` the space counter, `S(k)` and `D(k)` the counter and the data channel
// of the collection `k`.

#[path = "../../models/tests/support/reified_strategies.rs"]
mod reified_strategies;

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use reified_strategies::{gint, gstring, list, tuple};
use rholang::rust::interpreter::accounting::authority::AuthorityByteEventKind;
use rholang::rust::interpreter::accounting::byte_accounting::{
    consume_introduction_identity, produce_introduction_identity,
};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::Sig;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::interpreter::EvaluateResult;
use rholang::rust::interpreter::merging::mergeable_tags::non_negative_mergeable_tag_name;
use rholang::rust::interpreter::reified::costs::FIRE_ROUND_CAP;
use rholang::rust::interpreter::reified::scope::{classify_space_ref, SpaceRef};
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::spaces::namespace::{
    self, CollectionKey, Durability, NameTail, NativeSpace, REIFIED_TAG, SID_LEN,
};
use rholang::rust::interpreter::spaces::record::{self, RecordKind};
use rholang::rust::interpreter::spaces::types::config::SpaceConfig;
use rholang::rust::interpreter::spaces::types::qualifier::SpaceQualifier;
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::hash;
use rspace_plus_plus::rspace::internal::{Datum, WaitingContinuation};
use rspace_plus_plus::rspace::merger::merging_logic::MergeType;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent, Produce, COMM};

/// True when `event` is a COMM whose consume is on the channel with the hash `key`.
fn is_comm_on(event: &Event, key: &Blake2b256Hash) -> bool {
    match event {
        Event::Comm(comm) => comm.consume.channel_hashes.as_slice() == std::slice::from_ref(key),
        Event::IoEvent(_) => false,
    }
}

const DEPLOY_SIGNATURE: &[u8] = b"rrs-p4-queue-deploy";
const FUNDING_SIGNATURE: &[u8] = b"rrs-p4-queue-payer";
const PHLO: i64 = 100_000_000;

/// The phlo limit of a divergent pair. It stops the endless re-issue in about 100 ms.
const DIVERGENT_PHLO: i64 = 100_000;

/// A deploy that creates a space and two names of it. The handle goes to `@"rrs-handle"` and
/// the names go to `@"rrs-names"`.
const SETUP: &str = r#"
new factory(`rho:space:hashmap:queue`) in {
  for (queue <- factory!?("default")) {
    @"rrs-handle"!(*queue) |
    for (a <- queue!?("gensym")) {
      for (b <- queue!?("gensym")) {
        @"rrs-names"!(*a, *b)
      }
    }
  }
}
"#;

/// `body` with the handle of the setup space bound to `queue` and its names bound to `a` and
/// `b`.
fn with_space(body: &str) -> String {
    format!("for (queue <<- @\"rrs-handle\") {{ for (a, b <<- @\"rrs-names\") {{\n{body}\n}} }}")
}

/// The URI of an injected name.
fn injected(name: &str) -> String { format!("rho:rrs:test:{name}") }

type Injections = HashMap<String, Par>;

/// One deploy: its result, its event log, and the space-scoped channels with data that it
/// left in the store before the revert of a failed deploy.
struct Deploy {
    result: EvaluateResult,
    log: Vec<Event>,
    residue: BTreeSet<Vec<u8>>,
}

impl Deploy {
    fn codes(&self) -> Vec<String> { error_codes(&self.result.errors) }
}

fn error_codes(errors: &[InterpreterError]) -> Vec<String> {
    let mut codes = Vec::with_capacity(errors.len());
    for error in errors {
        push_codes(error, &mut codes);
    }
    codes
}

fn push_codes(error: &InterpreterError, codes: &mut Vec<String>) {
    match error {
        InterpreterError::AggregateError { interpreter_errors } => {
            for inner in interpreter_errors {
                push_codes(inner, codes);
            }
        }
        InterpreterError::ReduceError(message) => codes.push(
            message
                .strip_prefix("reified:")
                .and_then(|rest| rest.split(':').next())
                .map(str::to_owned)
                .unwrap_or_else(|| message.clone()),
        ),
        other => codes.push(format!("{other:?}")),
    }
}

/// A play runtime and a replay runtime over a fresh in-memory store, with a funded deploy.
struct Session {
    play: RhoRuntimeImpl,
    replay: RhoRuntimeImpl,
}

impl Session {
    async fn new() -> Self {
        let mut manager = InMemoryStoreManager::new();
        let stores = manager
            .r_space_stores()
            .await
            .expect("create the in-memory RSpace stores");
        let (play, replay, _history) = create_runtimes(stores, false, &mut Vec::new()).await;
        for runtime in [&play, &replay] {
            runtime.cost.set_deploy_signature_funded(
                DEPLOY_SIGNATURE,
                Sig::Ground(FUNDING_SIGNATURE.to_vec()),
            );
        }
        Self { play, replay }
    }

    async fn deploy_full(
        &mut self,
        source: &str,
        label: &str,
        injections: &Injections,
        phlo: i64,
    ) -> Deploy {
        let fallback = self.play.create_soft_checkpoint().await;
        let before = self.space_data_channels().await;
        let result = self
            .play
            .evaluate(
                source,
                Cost::create(phlo, label.to_owned()),
                injections.clone(),
                Blake2b512Random::create_from_bytes(label.as_bytes()),
            )
            .await
            .unwrap_or_else(|error| panic!("{label}: the evaluation returns no result: {error:?}"));
        let log = self.play.take_event_log().await;
        let residue = self
            .space_data_channels()
            .await
            .difference(&before)
            .cloned()
            .collect();
        if !result.errors.is_empty() {
            self.play.revert_to_soft_checkpoint(fallback).await;
        }
        Deploy {
            result,
            log,
            residue,
        }
    }

    async fn deploy_with(&mut self, source: &str, label: &str, injections: &Injections) -> Deploy {
        self.deploy_full(source, label, injections, PHLO).await
    }

    async fn run(&mut self, source: &str, label: &str) -> Deploy {
        let deploy = self.deploy_with(source, label, &Injections::new()).await;
        assert!(
            deploy.result.errors.is_empty(),
            "{label}: the deploy fails: {:?}",
            deploy.result.errors
        );
        deploy
    }

    /// A deploy that must fail with exactly the error codes `expected` and leave no new space
    /// data.
    async fn fail_with(
        &mut self,
        source: &str,
        label: &str,
        injections: &Injections,
        expected: &[&str],
    ) -> Deploy {
        let deploy = self.deploy_with(source, label, injections).await;
        assert_eq!(deploy.codes(), expected, "{label}: the error codes");
        assert!(
            deploy.residue.is_empty(),
            "{label}: a failed deploy wrote space data before it failed"
        );
        deploy
    }

    async fn values(&self, channel: &str) -> Vec<Vec<Par>> {
        self.play
            .get_data(&gstring(channel))
            .await
            .into_iter()
            .map(|datum| datum.a.pars)
            .collect()
    }

    async fn sole(&self, channel: &str) -> Par {
        match <[Vec<Par>; 1]>::try_from(self.values(channel).await) {
            Ok([pars]) => match <[Par; 1]>::try_from(pars) {
                Ok([value]) => value,
                Err(pars) => panic!("@{channel:?} holds a datum of {} values", pars.len()),
            },
            Err(data) => panic!("@{channel:?} holds {} data instead of one", data.len()),
        }
    }

    async fn data(&self, channel: &Par) -> Vec<Datum<ListParWithRandom>> {
        self.play.get_data(channel).await
    }

    async fn waiters(
        &self,
        channel: &Par,
    ) -> Vec<WaitingContinuation<BindPattern, TaggedContinuation>> {
        self.play.get_continuations(vec![channel.clone()]).await
    }

    async fn space_data_channels(&self) -> BTreeSet<Vec<u8>> {
        self.play
            .get_hot_changes()
            .await
            .into_iter()
            .filter(|(_, row)| !row.data.is_empty())
            .filter_map(|(channels, _)| match channels.as_slice() {
                [channel] => namespace::sole_gprivate_id(channel)
                    .filter(|id| id.first() == Some(&REIFIED_TAG) && id.len() > 2)
                    .map(<[u8]>::to_vec),
                _ => None,
            })
            .collect()
    }

    /// Runs `SETUP` and returns the space and the channels of its two names.
    async fn setup(&mut self) -> Space {
        self.run(SETUP, "rrs-p4-queue-setup").await;
        let space = native(&self.sole("rrs-handle").await);
        let names = match <[Vec<Par>; 1]>::try_from(self.values("rrs-names").await) {
            Ok([names]) => names,
            Err(data) => panic!("@\"rrs-names\" holds {} data", data.len()),
        };
        let [a, b] = <[Par; 2]>::try_from(names).expect("the setup makes two names");
        Space { space, a, b }
    }

    /// The records of the collection of `name`, in `ord` order: `(ord, payload, persistent)`.
    async fn records(&self, space: NativeSpace, name: &Par) -> Vec<(i64, Vec<Par>, bool)> {
        let mut records: Vec<(i64, Vec<Par>, bool)> = self
            .data(&data_channel(space, name))
            .await
            .into_iter()
            .map(|datum| {
                let decoded = record::unwrap_datum(&datum.a).expect("a datum record decodes");
                (decoded.ord, decoded.datum.pars, datum.persist)
            })
            .collect();
        records.sort_by_key(|(ord, _, _)| *ord);
        records
    }

    /// The parked continuations of the collection of `name`, in `ord` order: `(ord,
    /// persistent, peek)`.
    async fn parked(&self, space: NativeSpace, name: &Par) -> Vec<(i64, bool, bool)> {
        let mut parked: Vec<(i64, bool, bool)> = self
            .waiters(&data_channel(space, name))
            .await
            .into_iter()
            .map(|waiter| {
                let ord = record::continuation_ord(&waiter.continuation)
                    .expect("a parked continuation decodes");
                (ord, waiter.persist, !waiter.peeks.is_empty())
            })
            .collect();
        parked.sort_by_key(|(ord, _, _)| *ord);
        parked
    }

    /// The value of the counter of the collection of `name`.
    async fn counter(&self, space: NativeSpace, name: &Par) -> i64 {
        counter_of(&self.data(&counter_channel(space, name)).await)
    }

    /// The value of the space counter.
    async fn space_counter(&self, space: NativeSpace) -> i64 {
        counter_of(&self.data(&space_counter_channel(space)).await)
    }
}

/// The value of the only counter record of a counter channel.
fn counter_of(data: &[Datum<ListParWithRandom>]) -> i64 {
    match data {
        [datum] => {
            assert!(!datum.persist, "a counter record is never persistent");
            record::counter_value(&datum.a).expect("a counter record decodes")
        }
        other => panic!("a counter channel holds {} records", other.len()),
    }
}

/// The setup space and its two names.
struct Space {
    space: NativeSpace,
    a: Par,
    b: Par,
}

impl Space {
    /// The labels of the channels of the space for event signatures: `C`, `S`, `H`, and `S:x`,
    /// `D:x`, `N:x` for the name `x`.
    fn labels(&self) -> Labels {
        let mut labels = vec![
            (hash(&config_channel(self.space)), "C".to_owned()),
            (hash(&space_counter_channel(self.space)), "S".to_owned()),
            (
                hash(&namespace::gprivate(namespace::handle_channel(self.space))),
                "H".to_owned(),
            ),
        ];
        for (label, name) in [("a", &self.a), ("b", &self.b)] {
            labels.push((
                hash(&counter_channel(self.space, name)),
                format!("S:{label}"),
            ));
            labels.push((hash(&data_channel(self.space, name)), format!("D:{label}")));
            labels.push((hash(name), format!("N:{label}")));
        }
        Labels(labels)
    }
}

/// Channel labels for the event signatures of a log.
struct Labels(Vec<(Blake2b256Hash, String)>);

impl Labels {
    fn label(&self, channel: &Blake2b256Hash) -> Option<&str> {
        self.0
            .iter()
            .find(|(key, _)| key == channel)
            .map(|(_, label)| label.as_str())
    }

    /// The signature of each event of `log` on a labeled channel, in log order: `P` for a
    /// produce, `C` for a consume, `M` for a COMM, then the label. A persistent event ends with
    /// `!`, and a COMM with peeks ends with `peek`.
    fn signature(&self, log: &[Event]) -> Vec<String> {
        log.iter()
            .filter_map(|event| match event {
                Event::IoEvent(IOEvent::Produce(produce)) => self
                    .label(&produce.channel_hash)
                    .map(|label| format!("P {label}{}", bang(produce.persistent))),
                Event::IoEvent(IOEvent::Consume(consume)) => {
                    match consume.channel_hashes.as_slice() {
                        [channel] => self
                            .label(channel)
                            .map(|label| format!("C {label}{}", bang(consume.persistent))),
                        _ => None,
                    }
                }
                Event::Comm(comm) => match comm.consume.channel_hashes.as_slice() {
                    [channel] => self.label(channel).map(|label| {
                        format!(
                            "M {label}{}{}",
                            bang(comm.consume.persistent),
                            if comm.peeks.is_empty() { "" } else { " peek" }
                        )
                    }),
                    _ => None,
                },
            })
            .collect()
    }
}

fn bang(persistent: bool) -> &'static str {
    match persistent {
        true => "!",
        false => "",
    }
}

fn native(handle: &Par) -> NativeSpace {
    match classify_space_ref(handle) {
        Ok(SpaceRef::Native(space)) => space,
        other => panic!("the value is not a native space handle: {other:?}"),
    }
}

fn config_channel(space: NativeSpace) -> Par {
    namespace::gprivate(namespace::config_channel(space))
}

fn space_counter_channel(space: NativeSpace) -> Par {
    namespace::gprivate(namespace::space_counter(space))
}

fn key(channel: &Par) -> CollectionKey {
    CollectionKey::of_channel(channel).expect("a channel has a collection key")
}

fn counter_channel(space: NativeSpace, channel: &Par) -> Par {
    namespace::gprivate(namespace::collection_counter(space, &key(channel)))
}

fn data_channel(space: NativeSpace, channel: &Par) -> Par {
    namespace::gprivate(namespace::data_channel(space, &key(channel)))
}

/// The number of events of `log` that touch `channel`.
fn events_touching(log: &[Event], channel: &Par) -> usize {
    let key = hash(channel);
    log.iter()
        .filter(|event| match event {
            Event::IoEvent(IOEvent::Produce(produce)) => produce.channel_hash == key,
            Event::IoEvent(IOEvent::Consume(consume)) => consume.channel_hashes.contains(&key),
            Event::Comm(comm) => comm.consume.channel_hashes.contains(&key),
        })
        .count()
}

/// The number of COMMs of `log` whose consume is on `channel`.
fn comms_on(log: &[Event], channel: &Par) -> usize {
    let key = hash(channel);
    log.iter().filter(|event| is_comm_on(event, &key)).count()
}

/// The produce events of `log` on `channel`, in log order.
fn data_produces(log: &[Event], channel: &Par) -> Vec<Produce> {
    let key = hash(channel);
    log.iter()
        .filter_map(|event| match event {
            Event::IoEvent(IOEvent::Produce(produce)) if produce.channel_hash == key => {
                Some(produce.clone())
            }
            _ => None,
        })
        .collect()
}

mod space_queue_head_only {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_queue_head_only_fifo_delivery() {
        let mut session = Session::new().await;
        let space = session.setup().await;
        // The values arrive in an order that is neither sorted nor the order of their hashes.
        let values = [5, 3, 9, 1, 7];
        for (index, value) in values.into_iter().enumerate() {
            session
                .run(
                    &with_space(&format!("a!({value})")),
                    &format!("rrs-p4-fifo-enqueue-{index}"),
                )
                .await;
        }
        assert_eq!(
            session.records(space.space, &space.a).await,
            values
                .iter()
                .enumerate()
                .map(|(ord, value)| (ord as i64, vec![gint(*value)], false))
                .collect::<Vec<_>>(),
            "each record takes the next ord"
        );
        session
            .run(
                &with_space(
                    r#"for (@v1 <- a) { for (@v2 <- a) { for (@v3 <- a) { for (@v4 <- a) { for (@v5 <- a) {
  @"rrs-fifo"!([v1, v2, v3, v4, v5])
} } } } }"#,
                ),
                "rrs-p4-fifo-dequeue",
            )
            .await;
        assert_eq!(
            session.sole("rrs-fifo").await,
            list(values.iter().map(|value| gint(*value)).collect()),
            "the receives deliver the records in arrival order"
        );
        assert!(session.records(space.space, &space.a).await.is_empty());
        // Waiters are first in, first out too.
        for index in 1..=3 {
            session
                .run(
                    &with_space(&format!(r#"for (@v <- b) {{ @"rrs-waiter-{index}"!(v) }}"#)),
                    &format!("rrs-p4-fifo-park-{index}"),
                )
                .await;
        }
        assert_eq!(session.parked(space.space, &space.b).await, vec![
            (0, false, false),
            (1, false, false),
            (2, false, false)
        ]);
        for (index, value) in [30, 10, 20].into_iter().enumerate() {
            session
                .run(
                    &with_space(&format!("b!({value})")),
                    &format!("rrs-p4-fifo-send-{index}"),
                )
                .await;
        }
        for (index, value) in [(1, 30), (2, 10), (3, 20)] {
            assert_eq!(
                session.sole(&format!("rrs-waiter-{index}")).await,
                gint(value),
                "waiter {index} gets the value of send {index}"
            );
        }
        assert!(session.parked(space.space, &space.b).await.is_empty());
        // A persistent receive drains the records in arrival order in one operation and then
        // waits (P4 design B.6, E9).
        for (index, value) in [8, 6, 4].into_iter().enumerate() {
            session
                .run(
                    &with_space(&format!("a!({value})")),
                    &format!("rrs-p4-fifo-refill-{index}"),
                )
                .await;
        }
        let ords: HashMap<Blake2b256Hash, i64> = session
            .data(&data_channel(space.space, &space.a))
            .await
            .iter()
            .map(|datum| {
                (
                    datum.source.hash.clone(),
                    record::datum_ord(&datum.a).expect("a datum record decodes"),
                )
            })
            .collect();
        let drain = session
            .run(
                &with_space(r#"for (@v <= a) { @"rrs-drained"!(v) }"#),
                "rrs-p4-fifo-drain",
            )
            .await;
        let data_key = hash(&data_channel(space.space, &space.a));
        let drained: Vec<i64> = drain
            .log
            .iter()
            .filter_map(|event| match event {
                Event::Comm(comm) if comm.consume.channel_hashes == [data_key.clone()] => comm
                    .produces
                    .first()
                    .and_then(|produce| ords.get(&produce.hash).copied()),
                _ => None,
            })
            .collect();
        assert_eq!(
            drained,
            vec![10, 11, 12],
            "the drain takes the records in ord order"
        );
        assert_eq!(
            space.labels().signature(&drain.log),
            vec![
                "C S:a", "M S:a", "P S:a", "C D:a!", "M D:a!", "C D:a!", "M D:a!", "C D:a!",
                "M D:a!", "C D:a!"
            ],
            "one bump, three fires, then the park"
        );
        assert_eq!(session.values("rrs-drained").await.len(), 3);
        assert!(session.records(space.space, &space.a).await.is_empty());
        assert_eq!(session.parked(space.space, &space.a).await, vec![(
            13, true, false
        )]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_queue_head_only_data_head_blocks() {
        let mut session = Session::new().await;
        let space = session.setup().await;
        for (index, value) in [1, 2].into_iter().enumerate() {
            session
                .run(
                    &with_space(&format!("a!({value})")),
                    &format!("rrs-p4-data-head-{index}"),
                )
                .await;
        }
        // The receive matches only 2. The head record is 1, so the receive waits, although the
        // second record matches.
        let deploy = session
            .run(
                &with_space(r#"for (@2 <- a) { @"rrs-two"!(2) }"#),
                "rrs-p4-data-head-receive",
            )
            .await;
        assert!(
            session.values("rrs-two").await.is_empty(),
            "the head record blocks"
        );
        assert_eq!(session.records(space.space, &space.a).await, vec![
            (0, vec![gint(1)], false),
            (1, vec![gint(2)], false)
        ]);
        assert_eq!(session.parked(space.space, &space.a).await, vec![(
            2, false, false
        )]);
        assert_eq!(
            space.labels().signature(&deploy.log),
            vec!["C S:a", "M S:a", "P S:a", "C D:a"],
            "the receive bumps the counter and parks (P4 design B.6, E4)"
        );
        // The host scans every datum, so the same program on a host channel fires.
        session
            .run(
                r#"@"rrs-host"!(1) | @"rrs-host"!(2) | for (@2 <- @"rrs-host") { @"rrs-host-two"!(2) }"#,
                "rrs-p4-data-head-host",
            )
            .await;
        assert_eq!(session.sole("rrs-host-two").await, gint(2));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_queue_head_only_waiter_head_blocks() {
        let mut session = Session::new().await;
        let space = session.setup().await;
        for value in [1, 2] {
            session
                .run(
                    &with_space(&format!(
                        r#"for (@{value} <- b) {{ @"rrs-waiter-{value}"!({value}) }}"#
                    )),
                    &format!("rrs-p4-waiter-head-park-{value}"),
                )
                .await;
        }
        // The head waiter takes only 1. A send of 2 is stored, although the second waiter
        // takes 2.
        let deploy = session
            .run(&with_space("b!(2)"), "rrs-p4-waiter-head-send")
            .await;
        assert!(
            session.values("rrs-waiter-2").await.is_empty(),
            "the head waiter blocks"
        );
        assert_eq!(session.records(space.space, &space.b).await, vec![(
            2,
            vec![gint(2)],
            false
        )]);
        assert_eq!(session.parked(space.space, &space.b).await, vec![
            (0, false, false),
            (1, false, false)
        ]);
        assert_eq!(
            space.labels().signature(&deploy.log),
            vec!["C S:b", "M S:b", "P S:b", "P D:b"],
            "the send bumps the counter and stores (P4 design B.6, E1)"
        );
        // A later send of 1 goes behind the stored record, so the head waiter still waits.
        session
            .run(&with_space("b!(1)"), "rrs-p4-waiter-head-send-one")
            .await;
        assert!(session.values("rrs-waiter-1").await.is_empty());
        assert_eq!(session.records(space.space, &space.b).await.len(), 2);
        // The host matches each datum against every waiter.
        session
            .run(
                r#"for (@1 <- @"rrs-host") { @"rrs-host-1"!(1) } | for (@2 <- @"rrs-host") { @"rrs-host-2"!(2) }"#,
                "rrs-p4-waiter-head-host-park",
            )
            .await;
        session
            .run(r#"@"rrs-host"!(2)"#, "rrs-p4-waiter-head-host-send")
            .await;
        assert_eq!(session.sole("rrs-host-2").await, gint(2));
        // A guard of the head waiter blocks too: the pattern matches 3, the guard refuses it, and
        // the waiter behind it is never offered the datum.
        session
            .run(
                &with_space(r#"for (@x <- a where x > 5) { @"rrs-guarded"!(x) }"#),
                "rrs-p4-waiter-head-guard",
            )
            .await;
        session
            .run(
                &with_space(r#"for (@y <- a) { @"rrs-unguarded"!(y) }"#),
                "rrs-p4-waiter-head-unguarded",
            )
            .await;
        session
            .run(&with_space("a!(3)"), "rrs-p4-waiter-head-guard-send")
            .await;
        assert!(session.values("rrs-guarded").await.is_empty());
        assert!(
            session.values("rrs-unguarded").await.is_empty(),
            "the guarded head blocks"
        );
        assert_eq!(session.records(space.space, &space.a).await, vec![(
            2,
            vec![gint(3)],
            false
        )]);
        assert_eq!(session.parked(space.space, &space.a).await.len(), 2);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_queue_head_only_peek_keeps_head() {
        let mut session = Session::new().await;
        let space = session.setup().await;
        for (index, value) in [5, 6].into_iter().enumerate() {
            session
                .run(
                    &with_space(&format!("a!({value})")),
                    &format!("rrs-p4-peek-{index}"),
                )
                .await;
        }
        let before = session.counter(space.space, &space.a).await;
        let first = session
            .run(
                &with_space(r#"for (@x <<- a) { @"rrs-peek-1"!(x) }"#),
                "rrs-p4-peek-first",
            )
            .await;
        session
            .run(
                &with_space(r#"for (@y <<- a) { @"rrs-peek-2"!(y) }"#),
                "rrs-p4-peek-second",
            )
            .await;
        assert_eq!(session.sole("rrs-peek-1").await, gint(5));
        assert_eq!(session.sole("rrs-peek-2").await, gint(5), "the head stays");
        assert_eq!(
            session.records(space.space, &space.a).await,
            vec![(0, vec![gint(5)], false), (1, vec![gint(6)], false)],
            "a peek removes no record"
        );
        assert_eq!(
            session.counter(space.space, &space.a).await,
            before + 2,
            "each peek bumps the counter (A2)"
        );
        assert_eq!(
            space.labels().signature(&first.log),
            vec!["C S:a", "M S:a", "P S:a", "C D:a", "M D:a peek"],
            "a peek fires with peeks and keeps the record (P4 design B.6, E5)"
        );
        session
            .run(
                &with_space(r#"for (@z <- a) { for (@w <- a) { @"rrs-dequeued"!(z, w) } }"#),
                "rrs-p4-peek-dequeue",
            )
            .await;
        assert_eq!(session.values("rrs-dequeued").await, vec![vec![
            gint(5),
            gint(6)
        ]]);
    }
}

mod space_real_events {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_queue_events_namespaced() {
        let mut session = Session::new().await;
        let space = session.setup().await;
        session
            .run(&with_space("a!(0) | b!(1)"), "rrs-p4-namespaced-seed")
            .await;
        let deploy = session
            .run(
                &with_space(r#"a!(2) | for (@x <- b) { @"rrs-out"!(x) } | for (@y <<- a) { Nil }"#),
                "rrs-p4-namespaced",
            )
            .await;
        // The routed channel replaces the name in every identity (R6): no event names the
        // channels of the program.
        for name in [&space.a, &space.b] {
            assert_eq!(
                events_touching(&deploy.log, name),
                0,
                "no event is on a user name"
            );
        }
        let space_channels: BTreeSet<Blake2b256Hash> = [
            space_counter_channel(space.space),
            config_channel(space.space),
            data_channel(space.space, &space.a),
            data_channel(space.space, &space.b),
            counter_channel(space.space, &space.a),
            counter_channel(space.space, &space.b),
        ]
        .iter()
        .map(hash)
        .collect();
        let mut on_space = 0;
        for event in &deploy.log {
            let channels: Vec<&Blake2b256Hash> = match event {
                Event::IoEvent(IOEvent::Produce(produce)) => vec![&produce.channel_hash],
                Event::IoEvent(IOEvent::Consume(consume)) => {
                    consume.channel_hashes.iter().collect()
                }
                Event::Comm(comm) => comm
                    .consume
                    .channel_hashes
                    .iter()
                    .chain(comm.produces.iter().map(|produce| &produce.channel_hash))
                    .collect(),
            };
            if channels
                .iter()
                .any(|channel| space_channels.contains(*channel))
            {
                on_space += 1;
                assert!(
                    channels
                        .iter()
                        .all(|channel| space_channels.contains(*channel)),
                    "a space event mixes space and host channels"
                );
            }
        }
        assert_eq!(
            on_space,
            4 + 5 + 5,
            "an enqueue, a dequeue and a peek of the queues"
        );
        assert_eq!(
            space.labels().signature(&deploy.log).len(),
            on_space,
            "every space event is on a labeled space channel"
        );
        assert_eq!(session.values("rrs-out").await, vec![vec![gint(1)]]);
        // The same program on host names logs its events on those names.
        let host = session
            .run(
                r#"new a, b in { b!(1) | a!(2) | for (@x <- b) { Nil } | for (@y <<- a) { Nil } }"#,
                "rrs-p4-namespaced-host",
            )
            .await;
        assert!(host.log.iter().all(|event| match event {
            Event::IoEvent(IOEvent::Produce(produce)) =>
                !space_channels.contains(&produce.channel_hash),
            _ => true,
        }));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_queue_counter_bump_shape() {
        let mut session = Session::new().await;
        let space = session.setup().await;
        let labels = space.labels();
        assert_eq!(session.space_counter(space.space).await, 0);
        assert!(session
            .data(&counter_channel(space.space, &space.a))
            .await
            .is_empty());
        // The first operation on a collection opens its counter from the space counter (E1').
        let space_counter_source = session
            .data(&space_counter_channel(space.space))
            .await
            .first()
            .expect("the space counter has a record")
            .source
            .clone();
        let first = session.run(&with_space("a!(1)"), "rrs-p4-bump-first").await;
        assert_eq!(labels.signature(&first.log), vec![
            "C S", "M S", "P S", "P S:a", "P D:a"
        ]);
        assert!(
            first.log.iter().any(|event| matches!(
                event,
                Event::Comm(comm) if comm.produces == [space_counter_source.clone()]
            )),
            "the bump consumes the stored space counter record"
        );
        assert_eq!(session.space_counter(space.space).await, 1);
        assert_eq!(session.counter(space.space, &space.a).await, 1);
        // Every later operation consumes the collection counter and stores the next value (E1).
        for (index, (body, shape)) in [
            ("a!(2)", vec!["C S:a", "M S:a", "P S:a", "P D:a"]),
            ("for (@x <- a) { Nil }", vec![
                "C S:a", "M S:a", "P S:a", "C D:a", "M D:a",
            ]),
            ("for (@x <<- a) { Nil }", vec![
                "C S:a",
                "M S:a",
                "P S:a",
                "C D:a",
                "M D:a peek",
            ]),
            ("for (@3 <- a) { Nil }", vec![
                "C S:a", "M S:a", "P S:a", "C D:a",
            ]),
        ]
        .into_iter()
        .enumerate()
        {
            let counter_source = session
                .data(&counter_channel(space.space, &space.a))
                .await
                .first()
                .expect("the collection counter has a record")
                .source
                .clone();
            let value = session.counter(space.space, &space.a).await;
            let deploy = session
                .run(&with_space(body), &format!("rrs-p4-bump-{index}"))
                .await;
            assert_eq!(labels.signature(&deploy.log), shape, "{body}");
            assert!(
                deploy.log.iter().any(|event| matches!(
                    event,
                    Event::Comm(comm) if comm.produces == [counter_source.clone()]
                )),
                "{body}: the bump consumes the stored counter record"
            );
            assert_eq!(
                session.counter(space.space, &space.a).await,
                value + 1,
                "{body}: every operation bumps the counter once"
            );
        }
        assert_eq!(
            session.space_counter(space.space).await,
            1,
            "one collection was opened"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_queue_ord_envelope() {
        let mut session = Session::new().await;
        let space = session.setup().await;
        for (index, payload) in ["1", "2, 3", "[4]"].into_iter().enumerate() {
            session
                .run(
                    &with_space(&format!("a!({payload})")),
                    &format!("rrs-p4-ord-{index}"),
                )
                .await;
        }
        let stored = session.data(&data_channel(space.space, &space.a)).await;
        let mut ords = Vec::with_capacity(stored.len());
        for datum in &stored {
            let decoded = record::unwrap_datum(&datum.a).expect("a datum record decodes");
            assert_eq!(
                datum.a.pars.first(),
                Some(&record::datum_envelope(decoded.ord)),
                "the first par is the envelope [tag_D, ord, Nil, Nil]"
            );
            assert_eq!(&datum.a.pars[1..], decoded.datum.pars.as_slice());
            assert!(
                datum.a.pars.len() >= 2,
                "a record holds the envelope and the payload, so no datum is a single number"
            );
            ords.push((decoded.ord, decoded.datum.pars));
        }
        ords.sort_by_key(|(ord, _)| *ord);
        assert_eq!(ords, vec![
            (0, vec![gint(1)]),
            (1, vec![gint(2), gint(3)]),
            (2, vec![list(vec![gint(4)])]),
        ]);
        assert_eq!(
            RecordKind::Datum.tag(),
            reified_strategies::expr_par(
                models::rhoapi::expr::ExprInstance::GByteArray(vec![REIFIED_TAG, 0x52, b'D', 1]),
                Vec::new(),
                false
            ),
            "the tag is version 1 of the datum envelope"
        );
        // A parked receive carries the continuation envelope [tag_K, ord, Nil, body].
        for index in 0..2 {
            session
                .run(
                    &with_space(&format!(r#"for (@v <- b) {{ @"rrs-parked-{index}"!(v) }}"#)),
                    &format!("rrs-p4-ord-park-{index}"),
                )
                .await;
        }
        let waiters = session.waiters(&data_channel(space.space, &space.b)).await;
        let mut parked_ords: Vec<i64> = waiters
            .iter()
            .map(|waiter| {
                let decoded = record::unwrap_continuation(&waiter.continuation)
                    .expect("a parked continuation decodes");
                assert_eq!(
                    record::wrap_continuation(decoded.ord, &decoded.continuation),
                    Ok(waiter.continuation.clone()),
                    "the parked continuation is the envelope of the original body"
                );
                decoded.ord
            })
            .collect();
        parked_ords.sort_unstable();
        assert_eq!(parked_ords, vec![0, 1]);
        // The ord is the counter value before the operation, so it continues after a dequeue.
        session
            .run(&with_space("for (@x <- a) { Nil }"), "rrs-p4-ord-dequeue")
            .await;
        session.run(&with_space("a!(5)"), "rrs-p4-ord-after").await;
        assert_eq!(
            session
                .records(space.space, &space.a)
                .await
                .into_iter()
                .map(|(ord, _, _)| ord)
                .collect::<Vec<_>>(),
            vec![1, 2, 4]
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_queue_reissue_persistent_send() {
        let mut session = Session::new().await;
        let space = session.setup().await;
        let labels = space.labels();
        // Two waiters on b. A persistent send fires both in one transaction and then stays
        // (P4 design B.6, E7).
        for index in 1..=2 {
            session
                .run(
                    &with_space(&format!(r#"for (@v <- b) {{ @"rrs-fired-{index}"!(v) }}"#)),
                    &format!("rrs-p4-persistent-park-{index}"),
                )
                .await;
        }
        let deploy = session
            .run(&with_space("b!!(7)"), "rrs-p4-persistent-send")
            .await;
        assert_eq!(
            labels.signature(&deploy.log),
            vec!["C S:b", "M S:b", "P S:b", "P D:b!", "M D:b", "P D:b!", "M D:b", "P D:b!"],
            "one bump, two fires, then the store"
        );
        assert_eq!(session.sole("rrs-fired-1").await, gint(7));
        assert_eq!(session.sole("rrs-fired-2").await, gint(7));
        assert_eq!(
            session.records(space.space, &space.b).await,
            vec![(2, vec![gint(7)], true)],
            "the persistent record stays with the ord of the send"
        );
        // More waiters than FIRE_ROUND_CAP: the agent commits the first rounds and re-issues
        // the rest as a new operation, with its own bump (A10, I-21). The re-issued send keeps
        // the order of its first round (decision 43).
        let waiters = FIRE_ROUND_CAP + 2;
        let mut parks = String::new();
        for _ in 0..waiters {
            parks.push_str(r#"for (@v <- a) { @"rrs-capped"!(v) } | "#);
        }
        parks.push_str("Nil");
        session.run(&with_space(&parks), "rrs-p4-capped-park").await;
        assert_eq!(session.parked(space.space, &space.a).await.len(), waiters);
        let counter = session.counter(space.space, &space.a).await;
        let capped = session
            .run(&with_space("a!!(8)"), "rrs-p4-capped-send")
            .await;
        assert_eq!(
            comms_on(&capped.log, &data_channel(space.space, &space.a)),
            waiters,
            "every waiter fires once"
        );
        assert_eq!(
            comms_on(&capped.log, &counter_channel(space.space, &space.a)),
            2,
            "the send runs as two operations, so it bumps the counter twice"
        );
        assert_eq!(session.counter(space.space, &space.a).await, counter + 2);
        assert_eq!(session.values("rrs-capped").await.len(), waiters);
        assert_eq!(
            session.records(space.space, &space.a).await,
            vec![(counter, vec![gint(8)], true)],
            "the second operation stores the persistent record with the order of the first"
        );
        assert!(session.parked(space.space, &space.a).await.is_empty());
        let produces = data_produces(&capped.log, &data_channel(space.space, &space.a));
        assert_eq!(produces.len(), waiters + 1, "every fire and the store");
        assert!(
            produces.iter().all(|produce| *produce == produces[0]),
            "both operations stage one record, so one identity"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_queue_reissue_reenters_ahead_of_younger_records() {
        // Decision 43: a re-issued persistent send keeps the order of its first round. A body
        // that the first round fires sends a younger datum to the queue before the second round,
        // and the head waiter does not take it. The send re-enters ahead of that datum and fires
        // the head waiter.
        let mut session = Session::new().await;
        let space = session.setup().await;
        let mut parks = String::from("for (@v <- a) { a!(99) }");
        for _ in 1..FIRE_ROUND_CAP {
            parks.push_str(" | for (@v <- a) { Nil }");
        }
        session.run(&with_space(&parks), "rrs-d43-park").await;
        session
            .run(
                &with_space(r#"for (@1 <- a) { @"rrs-d43-one"!(1) }"#),
                "rrs-d43-park-one",
            )
            .await;
        let first = session.counter(space.space, &space.a).await;
        let deploy = session.run(&with_space("a!!(1)"), "rrs-d43-reenter").await;
        assert_eq!(
            comms_on(&deploy.log, &counter_channel(space.space, &space.a)),
            3,
            "the first round, the younger send and the second round each bump the counter"
        );
        assert_eq!(session.sole("rrs-d43-one").await, gint(1));
        assert_eq!(
            session.records(space.space, &space.a).await,
            vec![
                (first, vec![gint(1)], true),
                (first + 1, vec![gint(99)], false)
            ],
            "the send rests at its first order, ahead of the younger datum"
        );
        assert!(session.parked(space.space, &space.a).await.is_empty());
        let produces = data_produces(&deploy.log, &data_channel(space.space, &space.a));
        let send: Vec<&Produce> = produces
            .iter()
            .filter(|produce| produce.persistent)
            .collect();
        assert_eq!(
            send.len(),
            FIRE_ROUND_CAP + 2,
            "two rounds of fires and the store"
        );
        assert!(send.iter().all(|produce| *produce == send[0]));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_queue_reissue_keeps_one_produce_identity() {
        // Decision 43: every round of a re-issued persistent send stages the same record. The
        // deploy has one introduction row for its one identity, as the host has for a
        // persistent produce that it makes again (`persistent_introductions`).
        let mut session = Session::new().await;
        let space = session.setup().await;
        let mut parks = String::new();
        for _ in 0..=FIRE_ROUND_CAP {
            parks.push_str("for (@v <- a) { Nil } | ");
        }
        parks.push_str("Nil");
        session
            .run(&with_space(&parks), "rrs-d43-identity-park")
            .await;
        let deploy = session
            .run(&with_space("a!!(5)"), "rrs-d43-identity-send")
            .await;
        let produces = data_produces(&deploy.log, &data_channel(space.space, &space.a));
        assert_eq!(
            produces.len(),
            FIRE_ROUND_CAP + 2,
            "every fire and the store"
        );
        assert!(produces.iter().all(|produce| *produce == produces[0]));
        let identity = produce_introduction_identity(&produces[0]);
        let rows = deploy
            .result
            .byte_observations
            .rows
            .iter()
            .filter(|row| {
                row.kind == AuthorityByteEventKind::ProduceIntroduction && row.event_id == identity
            })
            .count();
        assert_eq!(
            rows, 1,
            "one introduction row for the one identity of the send"
        );
        check_rows(&deploy.result, &deploy.log, "rrs-d43-identity-send");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_queue_reissue_peek_produce() {
        let mut session = Session::new().await;
        let space = session.setup().await;
        let labels = space.labels();
        // A peek waiter at the head and a waiter behind it. The send fires the peek waiter and
        // offers the datum again to the next head in the same transaction (A1, decision 30).
        session
            .run(
                &with_space(r#"for (@x <<- b) { @"rrs-peeked"!(x) }"#),
                "rrs-p4-reoffer-peek",
            )
            .await;
        session
            .run(
                &with_space(r#"for (@y <- b) { @"rrs-taken"!(y) }"#),
                "rrs-p4-reoffer-take",
            )
            .await;
        let deploy = session
            .run(&with_space("b!(5)"), "rrs-p4-reoffer-send")
            .await;
        assert_eq!(
            labels.signature(&deploy.log),
            vec![
                "C S:b",
                "M S:b",
                "P S:b",
                "P D:b",
                "M D:b peek",
                "P D:b",
                "M D:b"
            ],
            "one bump, the peek fire, and the re-offer that the next waiter takes"
        );
        assert_eq!(session.sole("rrs-peeked").await, gint(5));
        assert_eq!(session.sole("rrs-taken").await, gint(5));
        assert!(
            session.records(space.space, &space.b).await.is_empty(),
            "the datum is spent"
        );
        assert!(session.parked(space.space, &space.b).await.is_empty());
        // With no waiter behind the peek waiter, the re-offer stores the datum (E6).
        session
            .run(
                &with_space(r#"for (@x <<- a) { @"rrs-peeked-a"!(x) }"#),
                "rrs-p4-reoffer-peek-a",
            )
            .await;
        let stored = session
            .run(&with_space("a!(6)"), "rrs-p4-reoffer-store")
            .await;
        assert_eq!(
            labels.signature(&stored.log),
            vec!["C S:a", "M S:a", "P S:a", "P D:a", "M D:a peek", "P D:a"],
            "one bump, the peek fire, then the store of the same record"
        );
        assert_eq!(session.sole("rrs-peeked-a").await, gint(6));
        assert_eq!(
            session.records(space.space, &space.a).await,
            vec![(1, vec![gint(6)], false)],
            "the datum stays after the peek"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_queue_divergent_pair_exhausts_phlo() {
        // A persistent receive and a persistent send on one collection: each round fires and
        // keeps both, so the operation re-issues as a new Intent::Space after every round
        // (A10). The rounds stop only when the phlo runs out.
        let mut outcomes = Vec::with_capacity(2);
        for attempt in 0..2 {
            let mut session = Session::new().await;
            let space = session.setup().await;
            let counter = session.counter_or_zero(space.space, &space.a).await;
            let deploy = session
                .deploy_full(
                    &with_space("for (@x <= a) { Nil } | a!!(1)"),
                    "rrs-p4-divergent",
                    &Injections::new(),
                    DIVERGENT_PHLO,
                )
                .await;
            assert_eq!(
                deploy.result.errors,
                vec![InterpreterError::OutOfPhlogistonsError],
                "attempt {attempt}: the pair runs until the phlo is spent"
            );
            assert_eq!(deploy.result.cost.value, DIVERGENT_PHLO);
            let bumps = comms_on(&deploy.log, &counter_channel(space.space, &space.a));
            let fires = comms_on(&deploy.log, &data_channel(space.space, &space.a));
            assert!(
                bumps >= 3,
                "attempt {attempt}: the pair runs several rounds: {bumps}"
            );
            assert!(
                bumps == fires || bumps == fires + 1,
                "attempt {attempt}: each round bumps once and fires once, and the phlo can run \
                 out between the two: {fires} fires, {bumps} bumps"
            );
            assert_eq!(
                session.counter_or_zero(space.space, &space.a).await,
                counter,
                "attempt {attempt}: the failed deploy is reverted"
            );
            // Decision 43: the element in flight keeps its record, so every round repeats one
            // produce on the data channel and one COMM, and only the bump of a round is new.
            let data = data_channel(space.space, &space.a);
            let produces = data_produces(&deploy.log, &data);
            assert!(
                produces.iter().all(|produce| *produce == produces[0]),
                "attempt {attempt}: every round produces one record"
            );
            let key = hash(&data);
            let comms: Vec<&COMM> = deploy
                .log
                .iter()
                .filter_map(|event| match event {
                    Event::Comm(comm) if is_comm_on(event, &key) => Some(comm),
                    _ => None,
                })
                .collect();
            assert!(
                comms.iter().all(|comm| *comm == comms[0]),
                "attempt {attempt}: every round repeats one COMM"
            );
            outcomes.push(deploy.log);
        }
        assert_eq!(outcomes[0], outcomes[1], "the rounds are deterministic");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_queue_observer_parity() {
        // The accounting observer sees each space event exactly as it sees a host event:
        // one introduction row for each non-persistent produce or consume, one row for each
        // identity of a persistent one, and one row for each COMM. The data-channel events of
        // each queue operation also equal the host events of the same program, except the
        // re-produce that the host reducer adds after a consume-side peek.
        let steps = [
            ("enqueue", "a!(1)"),
            ("dequeue", "for (@x <- a) { Nil }"),
            ("park", "for (@x <- b) { Nil }"),
            ("fire", "b!(5)"),
            ("enqueue-again", "a!(2)"),
            ("peek", "for (@x <<- a) { Nil }"),
            ("peek-wait", r#"for (@x <<- b) { @"rrs-seen"!(x) }"#),
            ("produce-on-peek", "b!(6)"),
            ("take", r#"for (@x <- b) { Nil } | for (@y <- b) { Nil }"#),
            ("persistent-send", "b!!(7)"),
        ];
        let mut session = Session::new().await;
        let space = session.setup().await;
        let labels = space.labels();
        let host_labels = Labels(vec![
            (hash(&gstring("rrs-host-a")), "D:a".to_owned()),
            (hash(&gstring("rrs-host-b")), "D:b".to_owned()),
        ]);
        for (label, body) in steps {
            let reified = session
                .run(&with_space(body), &format!("rrs-p4-parity-{label}"))
                .await;
            check_rows(&reified.result, &reified.log, label);
            let host_body = body
                .replace("<- a", "<- @\"rrs-host-a\"")
                .replace("<- b", "<- @\"rrs-host-b\"")
                .replace("a!", "@\"rrs-host-a\"!")
                .replace("b!", "@\"rrs-host-b\"!")
                .replace("rrs-seen", "rrs-host-seen");
            let host = session
                .run(&host_body, &format!("rrs-p4-parity-host-{label}"))
                .await;
            check_rows(&host.result, &host.log, &format!("host {label}"));
            let mut space_events: Vec<String> = labels
                .signature(&reified.log)
                .into_iter()
                .filter(|event| event.contains("D:"))
                .collect();
            let mut host_events = host_labels.signature(&host.log);
            if label == "peek" {
                assert_eq!(
                    host_events.pop().as_deref(),
                    Some("P D:a"),
                    "the host reducer produces the peeked datum again"
                );
            }
            space_events.sort();
            host_events.sort();
            assert_eq!(
                space_events, host_events,
                "{label}: the events equal the host events"
            );
        }
        assert_eq!(
            session.values("rrs-seen").await,
            session.values("rrs-host-seen").await
        );
    }
}

impl Session {
    /// The value of the counter of the collection of `name`, or 0 before its first operation.
    async fn counter_or_zero(&self, space: NativeSpace, name: &Par) -> i64 {
        let data = self.data(&counter_channel(space, name)).await;
        match data.is_empty() {
            true => 0,
            false => counter_of(&data),
        }
    }
}

/// Checks the observation rows of one deploy against its log (P4 design B.6 and I-12).
fn check_rows(result: &EvaluateResult, log: &[Event], label: &str) {
    let rows = &result.byte_observations.rows;
    let mut expected: BTreeMap<(u8, [u8; 32]), usize> = BTreeMap::new();
    let mut persistent_seen = BTreeSet::new();
    let mut comms = 0;
    for event in log {
        let (kind, identity, persistent) = match event {
            Event::IoEvent(IOEvent::Produce(produce)) => (
                AuthorityByteEventKind::ProduceIntroduction,
                produce_introduction_identity(produce),
                produce.persistent,
            ),
            Event::IoEvent(IOEvent::Consume(consume)) => (
                AuthorityByteEventKind::ConsumeIntroduction,
                consume_introduction_identity(consume),
                consume.persistent,
            ),
            Event::Comm(_) => {
                comms += 1;
                continue;
            }
        };
        if persistent && !persistent_seen.insert((kind.tag(), identity)) {
            continue;
        }
        *expected.entry((kind.tag(), identity)).or_default() += 1;
    }
    let mut observed: BTreeMap<(u8, [u8; 32]), usize> = BTreeMap::new();
    let mut comm_rows = 0;
    for row in rows {
        match row.kind {
            AuthorityByteEventKind::Comm => comm_rows += 1,
            kind => *observed.entry((kind.tag(), row.event_id)).or_default() += 1,
        }
    }
    assert_eq!(
        observed, expected,
        "{label}: one introduction row per observed event"
    );
    assert_eq!(comm_rows, comms, "{label}: one COMM row per COMM");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_queue_join_rejected() {
    let mut session = Session::new().await;
    let space = session.setup().await;
    for (label, body) in [
        (
            "two-names",
            r#"for (@x <- a & @y <- b) { @"rrs-join"!(x, y) }"#,
        ),
        (
            "repeated",
            r#"new dup in { dup!(*a, *a) | for (c, d <- dup) { for (@x <- c & @y <- d) { @"rrs-join"!(x, y) } } }"#,
        ),
        (
            "forged",
            r#"use queue { for (@x <- @"left" & @y <- @"right") { Nil } }"#,
        ),
        ("peek", r#"for (@x <<- a & @y <<- b) { Nil }"#),
        ("persistent", r#"for (@x <= a & @y <= b) { Nil }"#),
    ] {
        session
            .fail_with(
                &with_space(body),
                &format!("rrs-p4-join-{label}"),
                &Injections::new(),
                &["OrderedJoinUnsupported"],
            )
            .await;
    }
    assert!(session.records(space.space, &space.a).await.is_empty());
    assert!(session.parked(space.space, &space.a).await.is_empty());
    assert!(session.values("rrs-join").await.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_queue_cross_space_join_rejected() {
    let mut session = Session::new().await;
    let space = session.setup().await;
    session
        .run(
            r#"
new factory(`rho:space:hashmap:queue`) in {
  for (other <- factory!?("default")) { for (c <- other!?("gensym")) { @"rrs-other"!(*c) } }
}
"#,
            "rrs-p4-cross-other",
        )
        .await;
    for (label, body) in [
        (
            "two-spaces",
            r#"for (c <<- @"rrs-other") { for (@x <- a & @y <- c) { Nil } }"#,
        ),
        ("space-and-host", r#"for (@x <- a & @y <- @"host") { Nil }"#),
        (
            "host-and-space",
            r#"new h in { for (@y <- h & @x <- b) { Nil } }"#,
        ),
        (
            "use-and-other",
            r#"for (c <<- @"rrs-other") { use queue { for (@x <- @"left" & @y <- c) { Nil } } }"#,
        ),
    ] {
        session
            .fail_with(
                &with_space(body),
                &format!("rrs-p4-cross-{label}"),
                &Injections::new(),
                &["CrossSpaceJoin"],
            )
            .await;
    }
    assert!(session.parked(space.space, &space.a).await.is_empty());
    assert!(session.parked(space.space, &space.b).await.is_empty());
    // A join of host channels outside every use block is an ordinary join.
    session
        .run(
            r#"@"rrs-l"!(1) | @"rrs-r"!(2) | for (@x <- @"rrs-l" & @y <- @"rrs-r") { @"rrs-joined"!(x + y) }"#,
            "rrs-p4-cross-host-join",
        )
        .await;
    assert_eq!(session.sole("rrs-joined").await, gint(3));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_queue_query_rejected() {
    let mut session = Session::new().await;
    let space = session.setup().await;
    session.run(&with_space("a!(1)"), "rrs-p4-query-seed").await;
    for (label, body) in [
        ("value", r#"for (@x <- a ~ 1) { @"rrs-query"!(x) }"#),
        ("params", r#"for (@x <- a ~ (; k=1)) { @"rrs-query"!(x) }"#),
        (
            "forged",
            r#"use queue { for (@x <- @"docs" ~ ([0.5]; k=1)) { Nil } }"#,
        ),
    ] {
        session
            .fail_with(
                &with_space(body),
                &format!("rrs-p4-query-{label}"),
                &Injections::new(),
                &["QuerySpecUnsupported"],
            )
            .await;
    }
    assert!(session.values("rrs-query").await.is_empty());
    assert_eq!(
        session.records(space.space, &space.a).await,
        vec![(0, vec![gint(1)], false)],
        "a rejected query takes no record"
    );
    assert_eq!(
        session.counter(space.space, &space.a).await,
        1,
        "and bumps no counter"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_queue_send_params_rejected() {
    let mut session = Session::new().await;
    let space = session.setup().await;
    for (label, body, code) in [
        ("priority", "a!(1; priority=0)", "UnknownParam"),
        ("named", "a!(1; capacity=2)", "UnknownParam"),
        ("positional", "a!(1; 2)", "PositionalParamsUnsupported"),
        ("persistent", "a!!(1; priority=0)", "UnknownParam"),
        ("forged", r#"use queue { @"x"!(1; k=1) }"#, "UnknownParam"),
        ("host", r#"@"rrs-host"!(1; k=1)"#, "ParamsUnsupported"),
    ] {
        let deploy = session
            .fail_with(
                &with_space(body),
                &format!("rrs-p4-params-{label}"),
                &Injections::new(),
                &[code],
            )
            .await;
        if code == "UnknownParam" {
            assert!(
                format!("{:?}", deploy.result.errors).contains("is not a param of this channel"),
                "{label}"
            );
        }
    }
    assert!(session.records(space.space, &space.a).await.is_empty());
    assert!(session.values("rrs-host").await.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_queue_dangling_name_aborts() {
    let mut session = Session::new().await;
    let space = session.setup().await;
    let missing = NativeSpace {
        sid: [0x3D; SID_LEN],
        durability: Durability::Durable,
    };
    let injections = Injections::from([(
        injected("name"),
        namespace::gprivate(namespace::name_id(missing, NameTail::Random([0x11; 32]))),
    )]);
    for (label, body) in [
        ("send", "name!(1)"),
        ("receive", r#"for (@x <- name) { @"rrs-x"!(x) }"#),
        ("peek", "for (@x <<- name) { Nil }"),
    ] {
        session
            .fail_with(
                &format!("new name(`{}`) in {{ {body} }}", injected("name")),
                &format!("rrs-p4-dangling-{label}"),
                &injections,
                &["DanglingName"],
            )
            .await;
    }
    // A name of a space whose creating deploy was reverted is dangling too: the revert removes
    // the config record, so no record of the space stays behind.
    let reverted = session
        .deploy_with(
            r#"
new factory(`rho:space:hashmap:queue`) in {
  for (q <- factory!?("default")) { for (n <- q!?("gensym")) { @"rrs-lost"!(*n) | @"rrs-y"!(1; k=1) } }
}
"#,
            "rrs-p4-dangling-reverted",
            &Injections::new(),
        )
        .await;
    assert_eq!(reverted.codes(), vec!["ParamsUnsupported"]);
    assert!(
        reverted.log.iter().any(|event| matches!(
            event,
            Event::IoEvent(IOEvent::Produce(produce))
                if produce.channel_hash == hash(&gstring("rrs-lost"))
        )),
        "the reverted deploy sent its name before it failed"
    );
    assert!(session.values("rrs-lost").await.is_empty());
    let lost_space = reverted
        .residue
        .iter()
        .find_map(|id| match namespace::decode_reified_id(id) {
            Some(namespace::ReifiedId::Config(space)) => Some(space),
            _ => None,
        })
        .expect("the reverted deploy created its space before it failed");
    assert!(session.data(&config_channel(lost_space)).await.is_empty());
    let injections = Injections::from([(
        injected("lost"),
        namespace::gprivate(namespace::name_id(lost_space, NameTail::Random([0x22; 32]))),
    )]);
    session
        .fail_with(
            &format!("new lost(`{}`) in {{ lost!(1) }}", injected("lost")),
            "rrs-p4-dangling-lost",
            &injections,
            &["DanglingName"],
        )
        .await;
    // A name of the live space works.
    session
        .run(&with_space("a!(1)"), "rrs-p4-dangling-control")
        .await;
    assert_eq!(session.records(space.space, &space.a).await.len(), 1);
}

mod space_meta_channels {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_meta_channels_config_record() {
        let mut session = Session::new().await;
        let space = session.setup().await;
        let config = session.data(&config_channel(space.space)).await;
        let [config] = <[Datum<ListParWithRandom>; 1]>::try_from(config)
            .unwrap_or_else(|data| panic!("C(sid) holds {} records", data.len()));
        assert!(config.persist);
        assert_eq!(
            config.a.pars.first(),
            Some(&list(vec![RecordKind::Config.tag()])),
            "the envelope of the config record is [tag_C]"
        );
        let map = record::config_map(&config.a).expect("the config record decodes");
        assert_eq!(
            SpaceConfig::from_par(map),
            Ok(SpaceConfig::new(
                space.space.sid,
                "hashmap",
                "queue",
                SpaceQualifier::Default
            ))
        );
        // No operation and no handle call writes the config record again.
        let deploy = session
            .run(
                &with_space(
                    r#"
a!(1) | for (@x <- b) { Nil } | for (@y <<- a) { Nil } |
for (@q <- queue!?("qualifier")) { for (@c <- queue!?("config")) { for (n <- queue!?("gensym")) { n!(q, c) } } }
"#,
                ),
                "rrs-p4-config-untouched",
            )
            .await;
        assert_eq!(
            events_touching(&deploy.log, &config_channel(space.space)),
            0,
            "reads of the config record log no event"
        );
        let after = session.data(&config_channel(space.space)).await;
        assert_eq!(after.len(), 1);
        assert_eq!(
            after[0].source, config.source,
            "the config record is the same record"
        );
        assert_eq!(after[0].a, config.a);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_meta_channels_counters() {
        let mut session = Session::new().await;
        let space = session.setup().await;
        assert_eq!(
            session.space_counter(space.space).await,
            0,
            "no collection is open"
        );
        let operations = [
            ("a", "a!(1)"),
            ("a", "a!(2)"),
            ("a", "for (@x <<- a) { Nil }"),
            ("a", "for (@x <- a) { Nil }"),
            ("b", "for (@9 <- b) { Nil }"),
            ("b", "b!(8)"),
            ("a", "for (@7 <- a) { Nil }"),
        ];
        let mut expected: BTreeMap<&str, i64> = BTreeMap::new();
        for (index, (name, body)) in operations.into_iter().enumerate() {
            session
                .run(&with_space(body), &format!("rrs-p4-counters-{index}"))
                .await;
            *expected.entry(name).or_default() += 1;
            assert_eq!(
                session.counter(space.space, &space.a).await,
                expected.get("a").copied().unwrap_or_default(),
                "{body}: every operation on a bumps its counter"
            );
            if let Some(count) = expected.get("b") {
                assert_eq!(
                    session.counter(space.space, &space.b).await,
                    *count,
                    "{body}"
                );
            }
            assert_eq!(
                session.space_counter(space.space).await,
                i64::try_from(expected.len()).expect("a small count"),
                "{body}: the space counter counts the open collections"
            );
        }
        // Each counter channel holds one record, and it is never persistent.
        for channel in [
            space_counter_channel(space.space),
            counter_channel(space.space, &space.a),
            counter_channel(space.space, &space.b),
        ] {
            let data = session.data(&channel).await;
            assert_eq!(data.len(), 1);
            assert!(!data[0].persist);
            assert_eq!(
                data[0].a.pars.first(),
                Some(&list(vec![RecordKind::Counter.tag()]))
            );
        }
        // The queue state matches the operations: b's head waiter waits for 9, and b holds 8.
        assert_eq!(session.records(space.space, &space.b).await, vec![(
            1,
            vec![gint(8)],
            false
        )]);
        assert_eq!(session.parked(space.space, &space.b).await, vec![(
            0, false, false
        )]);
        assert_eq!(session.parked(space.space, &space.a).await, vec![(
            4, false, false
        )]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_meta_channels_no_second_root() {
        let mut session = Session::new().await;
        let empty_root = session.play.get_root().await;
        let space = session.setup().await;
        session
            .run(&with_space("a!(1) | a!(2)"), "rrs-p4-root-enqueue")
            .await;
        let root = session.play.create_checkpoint().await.root;
        assert_ne!(root, empty_root);
        // The checkpoint moves every space record into the one history trie: nothing stays
        // behind in the hot store.
        assert!(session.space_data_channels().await.is_empty());
        // A second runtime over the same store reads the space records at the root.
        let replay_root = {
            let replay = &mut session.replay;
            replay.reset(&root).await.expect("reset the replay runtime");
            replay.get_root().await
        };
        assert_eq!(replay_root, root);
        assert_eq!(
            session
                .replay
                .get_data(&data_channel(space.space, &space.a))
                .await
                .len(),
            2
        );
        assert_eq!(
            session
                .replay
                .get_data(&config_channel(space.space))
                .await
                .len(),
            1
        );
        // A read changes no state, so the root stays. A write changes the root.
        session
            .run(
                &with_space(r#"for (@q <- queue!?("qualifier")) { Nil }"#),
                "rrs-p4-root-read",
            )
            .await;
        assert_eq!(
            session.play.create_checkpoint().await.root,
            root,
            "a read keeps the root"
        );
        session.run(&with_space("a!(3)"), "rrs-p4-root-write").await;
        let written = session.play.create_checkpoint().await.root;
        assert_ne!(written, root, "a write changes the root");
        // At the root before the space, the space does not exist: nothing outside the trie
        // keeps it.
        session
            .play
            .reset(&empty_root)
            .await
            .expect("reset to the empty root");
        assert!(session.data(&config_channel(space.space)).await.is_empty());
        let injections = Injections::from([(injected("name"), space.a.clone())]);
        session
            .fail_with(
                &format!("new name(`{}`) in {{ name!(1) }}", injected("name")),
                "rrs-p4-root-gone",
                &injections,
                &["DanglingName"],
            )
            .await;
        session
            .play
            .reset(&written)
            .await
            .expect("reset to the written root");
        assert_eq!(session.records(space.space, &space.a).await.len(), 3);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_namespaced_channels_never_mergeable() {
    let mut session = Session::new().await;
    let space = session.setup().await;
    // The control sends on a tuple channel headed by the IntegerAdd tag outside every use
    // block. The space operations include the same kind of tuple channel inside a use block,
    // where it routes to the space.
    let deploy = session
        .run(
            &format!(
                "new tag(`rho:system:integerAddMergeableTag`) in {{ {} }}",
                with_space(
                    r#"
@(*tag, 1)!(5) |
use queue { @(*tag, 2)!(6) | for (@v <- @(*tag, 2)) { Nil } } |
a!(1) | for (@x <- a) { Nil } | for (@y <<- b) { Nil } |
for (n <- queue!?("gensym")) { n!(7) }
"#
                )
            ),
            "rrs-p4-mergeable",
        )
        .await;
    let tag = non_negative_mergeable_tag_name();
    let control = tuple(vec![tag.clone(), gint(1)]);
    let routed = tuple(vec![tag, gint(2)]);
    let mergeable = &deploy.result.mergeable;
    assert_eq!(
        mergeable.keys().cloned().collect::<Vec<_>>(),
        vec![control.clone()],
        "only the control channel is mergeable"
    );
    assert_eq!(mergeable.get(&control), Some(&MergeType::IntegerAdd));
    assert!(
        !mergeable.contains_key(&routed),
        "the routed tuple channel is a space channel"
    );
    for channel in mergeable.keys() {
        assert!(
            namespace::sole_gprivate_id(channel).is_none_or(|id| id.first() != Some(&REIFIED_TAG)),
            "no 0xE0 channel is mergeable"
        );
    }
    // The tuple channel inside the use block is a collection of the space.
    assert_eq!(
        comms_on(&deploy.log, &data_channel(space.space, &routed)),
        1,
        "the routed send and receive meet in the space"
    );
    assert_eq!(events_touching(&deploy.log, &routed), 0);
}
