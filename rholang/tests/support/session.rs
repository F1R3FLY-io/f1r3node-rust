//! A runtime session over a play runtime and a replay runtime, as the P4 queue suite runs
//! deploys (`reified_queue.rs`), for a space of any family.
//!
//! A deploy runs as Casper runs a user deploy: a soft checkpoint, the evaluation, the event log,
//! and a revert of the soft checkpoint when the deploy fails. The setup deploy creates a space
//! with a factory and two names of it, and later deploys read the handle and the names from the
//! store with peeks ([`with_space`]). The readers decode the records, the parked continuations
//! and the counters of the collection of a name in a HashMap-keyed space.

use std::collections::{BTreeSet, HashMap};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use models::rust::utils::new_gstring_par;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::Sig;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::interpreter::EvaluateResult;
use rholang::rust::interpreter::reified::scope::{classify_space_ref, SpaceRef};
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::spaces::namespace::{
    self, CollectionKey, NativeSpace, REIFIED_TAG,
};
use rholang::rust::interpreter::spaces::record;
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::hash;
use rspace_plus_plus::rspace::internal::{Datum, WaitingContinuation};
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent, Produce};

/// The signature of every deploy of a session.
pub const DEPLOY_SIGNATURE: &[u8] = b"rrs-p5-deploy";

/// The payer of every deploy of a session.
pub const FUNDING_SIGNATURE: &[u8] = b"rrs-p5-payer";

/// The phlo limit of a deploy.
pub const PHLO: i64 = 100_000_000;

/// The source of a deploy that creates a space with the factory `urn`, called with the
/// constructor arguments `ctor` (Rholang source, for example `"default"`), and two names of it.
/// The handle goes to `@"rrs-handle"` and the names go to `@"rrs-names"`.
pub fn setup_source(urn: &str, ctor: &str) -> String {
    format!(
        r#"
new factory(`{urn}`) in {{
  for (space <- factory!?({ctor})) {{
    @"rrs-handle"!(*space) |
    for (a <- space!?("gensym")) {{
      for (b <- space!?("gensym")) {{
        @"rrs-names"!(*a, *b)
      }}
    }}
  }}
}}
"#
    )
}

/// `body` with the handle of the setup space bound to `space` and its names bound to `a` and
/// `b`.
pub fn with_space(body: &str) -> String {
    format!("for (space <<- @\"rrs-handle\") {{ for (a, b <<- @\"rrs-names\") {{\n{body}\n}} }}")
}

/// The URI of an injected name.
pub fn injected(name: &str) -> String { format!("rho:rrs:test:{name}") }

/// The names that a deploy injects, by URI.
pub type Injections = HashMap<String, Par>;

/// One deploy: its result, its event log, and the space-scoped channels with data that it left
/// in the store before the revert of a failed deploy.
pub struct Deploy {
    pub result: EvaluateResult,
    pub log: Vec<Event>,
    pub residue: BTreeSet<Vec<u8>>,
}

impl Deploy {
    /// The error codes of the deploy, in error order.
    pub fn codes(&self) -> Vec<String> { error_codes(&self.result.errors) }
}

/// The codes of `errors`: the code of a Reified error, and the debug text of any other error.
pub fn error_codes(errors: &[InterpreterError]) -> Vec<String> {
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
pub struct Session {
    pub play: RhoRuntimeImpl,
    pub replay: RhoRuntimeImpl,
}

impl Session {
    pub async fn new() -> Self {
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

    /// Runs one deploy of `source` with the phlo limit `phlo`. A failed deploy is reverted.
    pub async fn deploy_full(
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

    /// Runs one deploy of `source` with the names `injections`.
    pub async fn deploy_with(
        &mut self,
        source: &str,
        label: &str,
        injections: &Injections,
    ) -> Deploy {
        self.deploy_full(source, label, injections, PHLO).await
    }

    /// Runs one deploy of `source` that must succeed.
    pub async fn run(&mut self, source: &str, label: &str) -> Deploy {
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
    pub async fn fail_with(
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

    /// The data on the string channel `channel`, as their pars.
    pub async fn values(&self, channel: &str) -> Vec<Vec<Par>> {
        self.play
            .get_data(&new_gstring_par(channel.to_string(), Vec::new(), false))
            .await
            .into_iter()
            .map(|datum| datum.a.pars)
            .collect()
    }

    /// The only value of the only datum on the string channel `channel`.
    pub async fn sole(&self, channel: &str) -> Par {
        match <[Vec<Par>; 1]>::try_from(self.values(channel).await) {
            Ok([pars]) => match <[Par; 1]>::try_from(pars) {
                Ok([value]) => value,
                Err(pars) => panic!("@{channel:?} holds a datum of {} values", pars.len()),
            },
            Err(data) => panic!("@{channel:?} holds {} data instead of one", data.len()),
        }
    }

    /// The data on `channel`.
    pub async fn data(&self, channel: &Par) -> Vec<Datum<ListParWithRandom>> {
        self.play.get_data(channel).await
    }

    /// The waiting continuations on `channel`.
    pub async fn waiters(
        &self,
        channel: &Par,
    ) -> Vec<WaitingContinuation<BindPattern, TaggedContinuation>> {
        self.play.get_continuations(vec![channel.clone()]).await
    }

    /// The ids of the space-scoped channels that hold data in the hot store.
    pub async fn space_data_channels(&self) -> BTreeSet<Vec<u8>> {
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

    /// Runs [`setup_source`] with the factory `urn` and the constructor arguments `ctor`, and
    /// returns the space and the channels of its two names.
    pub async fn setup(&mut self, urn: &str, ctor: &str) -> Space {
        self.run(&setup_source(urn, ctor), &format!("rrs-p5-setup {urn}"))
            .await;
        let space = native(&self.sole("rrs-handle").await);
        let names = match <[Vec<Par>; 1]>::try_from(self.values("rrs-names").await) {
            Ok([names]) => names,
            Err(data) => panic!("@\"rrs-names\" holds {} data", data.len()),
        };
        let [a, b] = <[Par; 2]>::try_from(names).expect("the setup makes two names");
        Space { space, a, b }
    }

    /// The records of the collection of `name`, in `ord` order: `(ord, payload, persistent)`.
    pub async fn records(&self, space: NativeSpace, name: &Par) -> Vec<(i64, Vec<Par>, bool)> {
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
    pub async fn parked(&self, space: NativeSpace, name: &Par) -> Vec<(i64, bool, bool)> {
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
    pub async fn counter(&self, space: NativeSpace, name: &Par) -> i64 {
        counter_of(&self.data(&counter_channel(space, name)).await)
    }

    /// The value of the space counter.
    pub async fn space_counter(&self, space: NativeSpace) -> i64 {
        counter_of(&self.data(&space_counter_channel(space)).await)
    }
}

/// The value of the only counter record of a counter channel.
pub fn counter_of(data: &[Datum<ListParWithRandom>]) -> i64 {
    match data {
        [datum] => {
            assert!(!datum.persist, "a counter record is never persistent");
            record::counter_value(&datum.a).expect("a counter record decodes")
        }
        other => panic!("a counter channel holds {} records", other.len()),
    }
}

/// The setup space and its two names.
pub struct Space {
    pub space: NativeSpace,
    pub a: Par,
    pub b: Par,
}

impl Space {
    /// The labels of the channels of the space for event signatures: `C`, `S`, `H`, and `S:x`,
    /// `D:x`, `N:x` for the name `x`.
    pub fn labels(&self) -> Labels {
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
pub struct Labels(pub Vec<(Blake2b256Hash, String)>);

impl Labels {
    /// The label of the channel with the hash `channel`.
    pub fn label(&self, channel: &Blake2b256Hash) -> Option<&str> {
        self.0
            .iter()
            .find(|(key, _)| key == channel)
            .map(|(_, label)| label.as_str())
    }

    /// The signature of each event of `log` on a labeled channel, in log order: `P` for a
    /// produce, `C` for a consume, `M` for a COMM, then the label. A persistent event ends with
    /// `!`, and a COMM with peeks ends with `peek`.
    pub fn signature(&self, log: &[Event]) -> Vec<String> {
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

/// The native space of a handle capability.
pub fn native(handle: &Par) -> NativeSpace {
    match classify_space_ref(handle) {
        Ok(SpaceRef::Native(space)) => space,
        other => panic!("the value is not a native space handle: {other:?}"),
    }
}

/// The config channel `C(sid)`.
pub fn config_channel(space: NativeSpace) -> Par {
    namespace::gprivate(namespace::config_channel(space))
}

/// The space counter `S(sid)`.
pub fn space_counter_channel(space: NativeSpace) -> Par {
    namespace::gprivate(namespace::space_counter(space))
}

/// The collection key of `channel`.
pub fn key(channel: &Par) -> CollectionKey {
    CollectionKey::of_channel(channel).expect("a channel has a collection key")
}

/// The counter channel `S(sid,k)` of the collection of `channel`.
pub fn counter_channel(space: NativeSpace, channel: &Par) -> Par {
    namespace::gprivate(namespace::collection_counter(space, &key(channel)))
}

/// The data channel `D(sid,k)` of the collection of `channel`.
pub fn data_channel(space: NativeSpace, channel: &Par) -> Par {
    namespace::gprivate(namespace::data_channel(space, &key(channel)))
}

/// The number of events of `log` that touch `channel`.
pub fn events_touching(log: &[Event], channel: &Par) -> usize {
    let key = hash(channel);
    log.iter()
        .filter(|event| match event {
            Event::IoEvent(IOEvent::Produce(produce)) => produce.channel_hash == key,
            Event::IoEvent(IOEvent::Consume(consume)) => consume.channel_hashes.contains(&key),
            Event::Comm(comm) => comm.consume.channel_hashes.contains(&key),
        })
        .count()
}

/// The number of COMMs of `log` whose consume is on `channel` alone.
pub fn comms_on(log: &[Event], channel: &Par) -> usize {
    let key = hash(channel);
    log.iter()
        .filter(|event| match event {
            Event::Comm(comm) => {
                comm.consume.channel_hashes.as_slice() == std::slice::from_ref(&key)
            }
            Event::IoEvent(_) => false,
        })
        .count()
}

/// The produce events of `log` on `channel`, in log order.
pub fn data_produces(log: &[Event], channel: &Par) -> Vec<Produce> {
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
