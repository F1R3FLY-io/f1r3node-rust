// P4 integration tests of the space factory and of the Reified system processes (P4 design A.5,
// C.7 and C.9, plan decisions 7, 32 and 33, readiness review §4).
//
// Every test runs Rholang deploys on a play runtime over an in-memory store. A deploy runs as
// Casper runs a user deploy: a soft checkpoint, the evaluation, the event log, and a revert of
// the soft checkpoint when the deploy fails. The tests read the results from public channels
// and from the store, and they decode the space records with the public decoders.

#[path = "../../models/tests/support/reified_strategies.rs"]
mod reified_strategies;

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{BindPattern, Bundle, ListParWithRandom, Par, TaggedContinuation};
use models::rust::space_factory_urn::{SpaceFactoryUrn, HASHMAP_BAG_ALIAS, PATHMAP_BAG_ALIAS};
use models::rust::utils::new_bundle_par;
use prost::Message;
use reified_strategies::{gbool, gstring};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::Sig;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::interpreter::EvaluateResult;
use rholang::rust::interpreter::reified::factory::{
    handle_capability, handle_continuation, handle_pattern,
};
use rholang::rust::interpreter::reified::handle::typed_error;
use rholang::rust::interpreter::reified::route::{classify_channel, ChannelClass};
use rholang::rust::interpreter::reified::runtime::unavailable;
use rholang::rust::interpreter::reified::scope::{classify_space_ref, SpaceRef};
use rholang::rust::interpreter::reified::system_channels::{
    body_ref, SystemChannel, ASSERT_EQUAL_PROCESS, BODY_REF_BASE, GET_SPACE_AGENT_PROCESS,
    HANDLE_OP_PROCESS, HASHMAP_QUEUE_FACTORY, P4_SYSTEM_CHANNELS, P6_PROCESSES,
};
use rholang::rust::interpreter::reified::system_processes::definitions;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::spaces::errors::SpaceReplyCode;
use rholang::rust::interpreter::spaces::factory::urn::{
    factory_capability, registered_aliases, registered_channel, registered_p5_families,
    registered_p6_families,
};
use rholang::rust::interpreter::spaces::namespace::{
    self, CollectionKey, Durability, Kind, Marker, NameTail, NativeSpace, ReifiedId, AGENT_TAG,
    REIFIED_TAG, SID_LEN,
};
use rholang::rust::interpreter::spaces::record;
use rholang::rust::interpreter::spaces::types::config::SpaceConfig;
use rholang::rust::interpreter::spaces::types::qualifier::SpaceQualifier;
use rholang::rust::interpreter::system_processes::{
    test_framework_contracts, BodyRefs, Definition,
};
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::hash;
use rspace_plus_plus::rspace::internal::{Datum, WaitingContinuation};
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent};

const DEPLOY_SIGNATURE: &[u8] = b"rrs-p4-factory-deploy";
const FUNDING_SIGNATURE: &[u8] = b"rrs-p4-factory-payer";
const PHLO: i64 = 100_000_000;

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

/// The URI of an injected name. A test passes a name to a deploy as `new x(`rho:rrs:test:x`)`.
fn injected(name: &str) -> String { format!("rho:rrs:test:{name}") }

/// The names that a deploy receives through URI binders.
type Injections = HashMap<String, Par>;

/// One deploy: its result, its event log, and the space-scoped channels with data that it
/// left in the store before the revert of a failed deploy.
struct Deploy {
    result: EvaluateResult,
    log: Vec<Event>,
    residue: BTreeSet<Vec<u8>>,
}

impl Deploy {
    /// The codes of the errors: `<Code>` of `reified:<Code>: …`, or the debug form of any
    /// other error.
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

/// A play runtime over a fresh in-memory store, with a funded deploy.
struct Session {
    play: RhoRuntimeImpl,
}

impl Session {
    async fn new() -> Self { Self::with_processes(&mut Vec::new()).await }

    async fn with_processes(processes: &mut Vec<Definition>) -> Self {
        let mut manager = InMemoryStoreManager::new();
        let stores = manager
            .r_space_stores()
            .await
            .expect("create the in-memory RSpace stores");
        let (play, _replay, _history) = create_runtimes(stores, false, processes).await;
        play.cost
            .set_deploy_signature_funded(DEPLOY_SIGNATURE, Sig::Ground(FUNDING_SIGNATURE.to_vec()));
        Self { play }
    }

    async fn deploy_with(&mut self, source: &str, label: &str, injections: &Injections) -> Deploy {
        let fallback = self.play.create_soft_checkpoint().await;
        let before = self.space_data_channels().await;
        let result = self
            .play
            .evaluate(
                source,
                Cost::create(PHLO, label.to_owned()),
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

    async fn deploy(&mut self, source: &str, label: &str) -> Deploy {
        self.deploy_with(source, label, &Injections::new()).await
    }

    /// A deploy that must succeed.
    async fn run(&mut self, source: &str, label: &str) -> Deploy {
        let deploy = self.deploy(source, label).await;
        assert!(
            deploy.result.errors.is_empty(),
            "{label}: the deploy fails: {:?}",
            deploy.result.errors
        );
        deploy
    }

    /// A deploy that must fail with exactly the error codes `expected` and leave no space data.
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
            "{label}: a failed deploy wrote space data before it failed: {} channels",
            deploy.residue.len()
        );
        deploy
    }

    /// The data of every datum on `@channel`.
    async fn values(&self, channel: &str) -> Vec<Vec<Par>> {
        self.play
            .get_data(&gstring(channel))
            .await
            .into_iter()
            .map(|datum| datum.a.pars)
            .collect()
    }

    /// The only value of the only datum on `@channel`.
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

    /// The ids of the space-scoped `0xE0` channels that hold data in the store.
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

    /// Runs `SETUP` and returns the space, its handle and its two names.
    async fn setup(&mut self) -> (NativeSpace, Par, Vec<Par>) {
        self.run(SETUP, "rrs-p4-factory-setup").await;
        let handle = self.sole("rrs-handle").await;
        let names = match <[Vec<Par>; 1]>::try_from(self.values("rrs-names").await) {
            Ok([names]) => names,
            Err(data) => panic!("@\"rrs-names\" holds {} data", data.len()),
        };
        (native(&handle), handle, names)
    }
}

/// The native space of a handle capability.
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

fn handle_channel(space: NativeSpace) -> Par {
    namespace::gprivate(namespace::handle_channel(space))
}

fn collection_key(name: &Par) -> CollectionKey {
    CollectionKey::of_channel(name).expect("a name has a collection key")
}

/// A durable space that no factory created.
fn fabricated_space(fill: u8) -> NativeSpace {
    NativeSpace {
        sid: [fill; SID_LEN],
        durability: Durability::Durable,
    }
}

/// The capability `bundle+{channel}` that the URN map binds.
fn write_bundle(channel: Par) -> Par { new_bundle_par(channel, true, false) }

/// The id of the name under a capability: the body of a bundle, or the name itself.
fn capability_id(value: &Par) -> Option<Vec<u8>> {
    let name = match value.bundles.as_slice() {
        [Bundle {
            body: Some(body), ..
        }] => body,
        _ => value,
    };
    namespace::sole_gprivate_id(name).map(<[u8]>::to_vec)
}

/// The channel hashes that an event touches.
fn event_channels(event: &Event) -> Vec<Blake2b256Hash> {
    match event {
        Event::IoEvent(IOEvent::Produce(produce)) => vec![produce.channel_hash.clone()],
        Event::IoEvent(IOEvent::Consume(consume)) => consume.channel_hashes.clone(),
        Event::Comm(comm) => comm.consume.channel_hashes.clone(),
    }
}

/// `(kind, persistent)` of each IO event of `log` on `channel`, in log order. `P` is a produce,
/// `C` a consume and `M` a COMM.
fn events_on(log: &[Event], channel: &Par) -> Vec<(char, bool)> {
    let key = hash(channel);
    log.iter()
        .filter_map(|event| match event {
            Event::IoEvent(IOEvent::Produce(produce)) if produce.channel_hash == key => {
                Some(('P', produce.persistent))
            }
            Event::IoEvent(IOEvent::Consume(consume))
                if consume.channel_hashes == [key.clone()] =>
            {
                Some(('C', consume.persistent))
            }
            Event::Comm(comm) if comm.consume.channel_hashes == [key.clone()] => {
                Some(('M', comm.consume.persistent))
            }
            _ => None,
        })
        .collect()
}

/// The typed error reply `("rho:space:error", code, {})` of `getSpaceAgent`.
fn typed(code: SpaceReplyCode) -> Par { typed_error(code, Vec::new()) }

/// The system processes of P5 (P5 design v3, A.7, A.8 and A.10) as `(urn, op, arity, body
/// ref)`, in the order of the definitions: the families in family-index order, then the alias.
/// A row is defined only when it is registered, so the pins below keep the registered rows.
const P5_PINNED: [(&str, u8, i32, i64); 25] = [
    ("rho:space:hashmap:bag", 0x00, 2, 200),
    ("rho:space:hashmap:stack", 0x02, 2, 202),
    ("rho:space:hashmap:set", 0x03, 2, 203),
    ("rho:space:hashmap:cell", 0x04, 2, 204),
    ("rho:space:hashmap:priorityqueue", 0x05, 2, 205),
    ("rho:space:pathmap:bag", 0x07, 2, 207),
    ("rho:space:pathmap:queue", 0x08, 2, 208),
    ("rho:space:pathmap:stack", 0x09, 2, 209),
    ("rho:space:pathmap:set", 0x0A, 2, 210),
    ("rho:space:pathmap:cell", 0x0B, 2, 211),
    ("rho:space:pathmap:priorityqueue", 0x0C, 2, 212),
    ("rho:space:array:bag", 0x0D, 2, 213),
    ("rho:space:array:queue", 0x0E, 2, 214),
    ("rho:space:array:stack", 0x0F, 2, 215),
    ("rho:space:array:set", 0x10, 2, 216),
    ("rho:space:array:cell", 0x11, 2, 217),
    ("rho:space:array:priorityqueue", 0x12, 2, 218),
    ("rho:space:vector:bag", 0x13, 2, 219),
    ("rho:space:vector:queue", 0x14, 2, 220),
    ("rho:space:vector:stack", 0x15, 2, 221),
    ("rho:space:vector:set", 0x16, 2, 222),
    ("rho:space:vector:cell", 0x17, 2, 223),
    ("rho:space:vector:priorityqueue", 0x18, 2, 224),
    ("rho:space:HashMapBagSpace", 0x00, 2, 200),
    ("rho:space:PathMapSpace", 0x07, 2, 207),
];

/// The system channels of the registered families of P5 and of the registered aliases, in the
/// order of the definitions.
fn registered_p5_channels() -> Vec<SystemChannel> {
    registered_p5_families()
        .map(|(_, channel)| channel)
        .chain(registered_aliases().map(|(channel, _)| channel))
        .collect()
}

/// The rows of [`P5_PINNED`] that are registered.
fn registered_p5_pins() -> Vec<(&'static str, u8, i32, i64)> {
    let registered: BTreeSet<&str> = registered_p5_channels()
        .into_iter()
        .map(|channel| channel.urn)
        .collect();
    P5_PINNED
        .into_iter()
        .filter(|(urn, ..)| registered.contains(urn))
        .collect()
}

/// The system processes of P6 (design v3 4.2) as `(urn, op, arity, body ref)`, in the order of
/// the definitions: the two VectorDB families in family-index order, after the rows of P5.
const P6_PINNED: [(&str, u8, i32, i64); 2] = [
    ("rho:space:hashmap:vectordb", 0x06, 2, 206),
    ("rho:space:vector:vectordb", 0x19, 2, 225),
];

/// The system processes of P6 that are not factories (P6 step 6), as `(urn, op, arity, body
/// ref)`, after the VectorDB families: `rho:lang:vector`, whose calls the reducer packs into
/// `[ret, [op, x…]]` (I-18).
const P6_PROCESS_PINS: [(&str, u8, i32, i64); 1] = [("rho:lang:vector", 0x23, 2, 235)];

/// The system channels of the registered families of P6, in the order of the definitions.
fn registered_p6_channels() -> Vec<SystemChannel> {
    registered_p6_families()
        .map(|(_, channel)| channel)
        .collect()
}

/// The rows of [`P6_PINNED`] that are registered.
fn registered_p6_pins() -> Vec<(&'static str, u8, i32, i64)> {
    let registered: BTreeSet<&str> = registered_p6_channels()
        .into_iter()
        .map(|channel| channel.urn)
        .collect();
    P6_PINNED
        .into_iter()
        .filter(|(urn, ..)| registered.contains(urn))
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_private_channels_unique() {
    let mut session = Session::new().await;
    let urn_map = session.play.reducer.urn_map.clone();
    // Each P4 URN is bound to the write capability of its own two-byte op channel.
    for channel in P4_SYSTEM_CHANNELS {
        assert_eq!(
            urn_map.get(channel.urn),
            Some(&write_bundle(channel.channel())),
            "the URN map binds {}",
            channel.urn
        );
        assert_eq!(
            capability_id(&channel.channel()),
            Some(vec![REIFIED_TAG, channel.op]),
            "{}",
            channel.urn
        );
    }
    // Each registered URN of P5, a family or an alias, is bound to the write capability of the
    // op channel of its family (P5 design v3, A.7).
    let p5 = registered_p5_channels();
    let p6 = registered_p6_channels();
    for channel in p5.iter().chain(p6.iter()).chain(P6_PROCESSES.iter()) {
        assert_eq!(
            urn_map.get(channel.urn),
            Some(&write_bundle(channel.channel())),
            "the URN map binds {}",
            channel.urn
        );
    }
    // No two URNs share a name, and only the registered URNs hold an id with the Reified tag. A
    // FIPS alias shares the channel of its family by design, so it is exempt.
    let aliases: BTreeSet<&str> = registered_aliases()
        .map(|(channel, _)| channel.urn)
        .collect();
    let registered: BTreeSet<&str> = P4_SYSTEM_CHANNELS
        .iter()
        .chain(p5.iter())
        .chain(p6.iter())
        .chain(P6_PROCESSES.iter())
        .map(|channel| channel.urn)
        .collect();
    let mut owners: BTreeMap<Vec<u8>, &str> = BTreeMap::new();
    for (urn, value) in urn_map.iter() {
        if !aliases.contains(urn.as_str()) {
            if let Some(previous) = owners.insert(value.encode_to_vec(), urn) {
                panic!("{urn} and {previous} share one channel");
            }
        }
        let reified =
            capability_id(value).is_some_and(|id| namespace::decode_reified_id(&id).is_some());
        assert_eq!(
            reified,
            registered.contains(urn.as_str()),
            "{urn}: only the registered URNs hold Reified ids"
        );
    }
    for (alias, family) in registered_aliases() {
        assert_eq!(
            urn_map.get(alias.urn),
            urn_map.get(&family.to_string()),
            "{} binds the capability of {family}",
            alias.urn
        );
    }
    // The families and the FIPS aliases without a registration have no binding: they fail
    // closed.
    for family in SpaceFactoryUrn::all().filter(|family| registered_channel(*family).is_none()) {
        assert!(!urn_map.contains_key(&family.to_string()), "{family}");
    }
    for alias in [HASHMAP_BAG_ALIAS, PATHMAP_BAG_ALIAS] {
        assert_eq!(
            urn_map.contains_key(alias),
            aliases.contains(alias),
            "{alias}"
        );
    }
    // Two factory calls in one deploy make two spaces with disjoint space-scoped channels.
    session
        .run(
            r#"
new factory(`rho:space:hashmap:queue`) in {
  for (first <- factory!?("default")) { @"rrs-first"!(*first) } |
  for (second <- factory!?("default")) { @"rrs-second"!(*second) }
}
"#,
            "rrs-p4-unique-two-spaces",
        )
        .await;
    let first = native(&session.sole("rrs-first").await);
    let second = native(&session.sole("rrs-second").await);
    assert_ne!(first.sid, second.sid, "two calls make two space ids");
    let channels_of = |space: NativeSpace| -> BTreeSet<Vec<u8>> {
        BTreeSet::from([
            namespace::config_channel(space),
            namespace::space_counter(space),
            namespace::handle_channel(space),
        ])
    };
    assert!(
        channels_of(first).is_disjoint(&channels_of(second)),
        "two spaces share no private channel"
    );
    let stored = session.space_data_channels().await;
    for space in [first, second] {
        for id in [
            namespace::config_channel(space),
            namespace::space_counter(space),
        ] {
            assert!(stored.contains(&id), "each space stores its own records");
        }
    }
    assert_eq!(stored.len(), 4, "two config records and two space counters");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_private_channels_body_refs_disjoint() {
    let defined = definitions(&unavailable());
    let refs: Vec<i64> = defined
        .iter()
        .map(|definition| definition.body_ref)
        .collect();
    let mut expected = vec![201, 233, 234, 248];
    expected.extend(registered_p5_pins().into_iter().map(|(.., body)| body));
    expected.extend(registered_p6_pins().into_iter().map(|(.., body)| body));
    expected.extend(P6_PROCESS_PINS.into_iter().map(|(.., body)| body));
    assert_eq!(
        refs, expected,
        "the P4 body refs are 200 + op, and the registered P5 and P6 rows follow"
    );
    for (definition, channel) in defined.iter().zip(P4_SYSTEM_CHANNELS) {
        assert_eq!(definition.body_ref, body_ref(channel.op));
        assert!(definition.body_ref >= BODY_REF_BASE);
    }
    let head_refs = [
        BodyRefs::STDOUT,
        BodyRefs::STDOUT_ACK,
        BodyRefs::STDERR,
        BodyRefs::STDERR_ACK,
        BodyRefs::ED25519_VERIFY,
        BodyRefs::SHA256_HASH,
        BodyRefs::KECCAK256_HASH,
        BodyRefs::BLAKE2B256_HASH,
        BodyRefs::SECP256K1_VERIFY,
        BodyRefs::GET_BLOCK_DATA,
        BodyRefs::GET_INVALID_BLOCKS,
        BodyRefs::VAULT_ADDRESS,
        BodyRefs::DEPLOYER_ID_OPS,
        BodyRefs::REG_OPS,
        BodyRefs::SYS_AUTHTOKEN_OPS,
        BodyRefs::REG_OPS_V1,
        BodyRefs::GPT4,
        BodyRefs::DALLE3,
        BodyRefs::TEXT_TO_AUDIO,
        BodyRefs::GRPC_TELL,
        BodyRefs::DEV_NULL,
        BodyRefs::ABORT,
        BodyRefs::OLLAMA_CHAT,
        BodyRefs::OLLAMA_GENERATE,
        BodyRefs::OLLAMA_MODELS,
        BodyRefs::DEPLOY_DATA,
        BodyRefs::CHROMA_CREATE_COLLECTION,
        BodyRefs::CHROMA_GET_COLLECTION_META,
        BodyRefs::CHROMA_UPSERT_ENTRIES,
        BodyRefs::CHROMA_QUERY,
        BodyRefs::CHROMA_DELETE_DOCUMENTS,
        BodyRefs::REGISTRY_LOOKUP,
    ];
    let framework: Vec<i64> = test_framework_contracts()
        .iter()
        .map(|definition| definition.body_ref)
        .collect();
    for head in head_refs.iter().chain(&framework) {
        assert!(
            *head < BODY_REF_BASE,
            "the HEAD body ref {head} is below the Reified base"
        );
        assert!(
            !refs.contains(head),
            "the HEAD body ref {head} is a Reified body ref"
        );
    }
    // A runtime with the test framework contracts installed holds every body ref once, and each
    // Reified process still answers its own calls.
    let mut processes = test_framework_contracts();
    let mut session = Session::with_processes(&mut processes).await;
    let installed: BTreeSet<i64> = session
        .play
        .reducer
        .dispatcher
        ._dispatch_table
        .read()
        .await
        .keys()
        .copied()
        .collect();
    for body in refs.iter().chain(&framework) {
        assert!(installed.contains(body), "the dispatch table holds {body}");
    }
    session
        .run(
            r#"
new factory(`rho:space:hashmap:queue`), spaceAgent(`rho:space:getSpaceAgent`),
    equal(`rho:assert:assertEqual`) in {
  for (queue <- factory!?("default")) {
    for (@capability <- spaceAgent!?(*queue)) {
      for (@same <- equal!?(capability, *factory)) {
        for (name <- queue!?("gensym")) {
          @"rrs-out"!(same, *name)
        }
      }
    }
  }
}
"#,
            "rrs-p4-body-refs",
        )
        .await;
    let out = session.values("rrs-out").await;
    match out.as_slice() {
        [values] => match values.as_slice() {
            [same, name] => {
                assert_eq!(same, &gbool(true), "getSpaceAgent and assertEqual answer");
                assert!(
                    matches!(classify_channel(name), ChannelClass::SpaceName(_)),
                    "the handle process answers gensym"
                );
            }
            other => panic!("@\"rrs-out\" holds {} values", other.len()),
        },
        other => panic!("@\"rrs-out\" holds {} data", other.len()),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_private_channels_kind_ids_disjoint() {
    // The KIND bytes are distinct in both durabilities. M and T have no ephemeral form.
    let mut bytes = BTreeSet::new();
    for kind in Kind::ALL {
        for durability in [Durability::Durable, Durability::Ephemeral] {
            match kind.byte(durability) {
                Some(byte) => {
                    assert!(
                        bytes.insert(byte),
                        "{kind:?} repeats the KIND byte {byte:#04x}"
                    );
                    assert_eq!(Kind::from_byte(byte), Some((kind, durability)));
                }
                None => assert!(
                    matches!(kind, Kind::Engine | Kind::Theory),
                    "{kind:?} has an ephemeral form"
                ),
            }
        }
    }
    assert_eq!(bytes.len(), 9 * 2 + 2);
    // A real space and a real collection: each channel decodes to its own kind.
    let mut session = Session::new().await;
    let (space, handle, names) = session.setup().await;
    let name = names.first().expect("the setup makes names").clone();
    session
        .run(
            r#"for (a, b <<- @"rrs-names") { a!(1) }"#,
            "rrs-p4-kind-ids-enqueue",
        )
        .await;
    let key = collection_key(&name);
    let ids = [
        (namespace::config_channel(space), "config"),
        (namespace::space_counter(space), "space counter"),
        (
            namespace::collection_counter(space, &key),
            "collection counter",
        ),
        (namespace::data_channel(space, &key), "data"),
        (namespace::handle_channel(space), "handle"),
        (
            namespace::sole_gprivate_id(&name)
                .expect("a space name is a private name")
                .to_vec(),
            "name",
        ),
    ];
    let distinct: BTreeSet<&Vec<u8>> = ids.iter().map(|(id, _)| id).collect();
    assert_eq!(
        distinct.len(),
        ids.len(),
        "the channels of one space are distinct"
    );
    for (id, label) in &ids {
        let decoded = namespace::decode_reified_id(id)
            .unwrap_or_else(|| panic!("the {label} channel decodes"));
        let (expected_class, matches_kind) = match decoded {
            ReifiedId::Config(found) => (ChannelClass::DefaultObject, found == space),
            ReifiedId::Handle(found) => (ChannelClass::DefaultObject, found == space),
            ReifiedId::SpaceCounter(found) => (ChannelClass::Storage, found == space),
            ReifiedId::CollectionCounter(found, tail) => (
                ChannelClass::Storage,
                found == space && tail == key.as_bytes(),
            ),
            ReifiedId::Data(found, tail) => (
                ChannelClass::Storage,
                found == space && tail == key.as_bytes(),
            ),
            ReifiedId::Name(found, NameTail::Random(_)) => {
                (ChannelClass::SpaceName(space), found == space)
            }
            other => panic!("the {label} channel decodes to {other:?}"),
        };
        assert!(matches_kind, "the {label} channel names its space and key");
        assert_eq!(
            classify_channel(&namespace::gprivate(id.clone())),
            expected_class,
            "the {label} channel routes by its kind"
        );
    }
    assert_eq!(namespace::config_channel(space).len(), 2 + SID_LEN);
    assert_eq!(namespace::handle_channel(space).len(), 2 + SID_LEN);
    assert!(namespace::data_channel(space, &key).len() > 2 + SID_LEN);
    assert_eq!(
        ids[5].0.len(),
        2 + SID_LEN + 32,
        "a gensym name has a 32-byte tail"
    );
    // The store holds the records of the space on exactly these channels.
    let stored = session.space_data_channels().await;
    assert_eq!(
        stored,
        BTreeSet::from([
            namespace::config_channel(space),
            namespace::space_counter(space),
            namespace::collection_counter(space, &key),
            namespace::data_channel(space, &key),
        ])
    );
    // The names of `new` (32 bytes), the byte names (1 byte), the op ids, the markers and the
    // agent tag never decode as space-scoped ids.
    session
        .run(r#"new x in { @"rrs-plain"!(*x) }"#, "rrs-p4-kind-ids-plain")
        .await;
    let plain = session.sole("rrs-plain").await;
    let plain_id = namespace::sole_gprivate_id(&plain).expect("a new name is private");
    assert_eq!(plain_id.len(), 32);
    assert_eq!(
        namespace::decode_reified_id(plain_id),
        None,
        "a new name is never a Reified id"
    );
    assert_eq!(classify_channel(&plain), ChannelClass::Unforgeable);
    for op in [
        HASHMAP_QUEUE_FACTORY.op,
        GET_SPACE_AGENT_PROCESS.op,
        ASSERT_EQUAL_PROCESS.op,
        HANDLE_OP_PROCESS.op,
        AGENT_TAG,
    ]
    .into_iter()
    .chain(Marker::ALL.map(|marker| marker as u8))
    {
        let id = namespace::op_channel(op);
        assert_eq!(namespace::decode_reified_id(&id), Some(ReifiedId::Op(op)));
        assert_eq!(
            classify_channel(&namespace::gprivate(id)),
            ChannelClass::System
        );
    }
    for byte in 0..=u8::MAX {
        assert_eq!(
            namespace::decode_reified_id(&[byte]),
            None,
            "byte name {byte}"
        );
    }
    // The handle capability is the write bundle of the handle channel.
    assert_eq!(handle, handle_capability(space));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_factory_persistent_send_aborts_once() {
    let mut session = Session::new().await;
    let deploy = session
        .fail_with(
            r#"
new factory(`rho:space:hashmap:queue`), ret in {
  factory!!(*ret, "default") |
  for (@handle <- ret) { @"rrs-out"!(handle) }
}
"#,
            "rrs-p4-factory-persistent",
            &Injections::new(),
            &["PersistentFactoryCall"],
        )
        .await;
    let factory_channel = HASHMAP_QUEUE_FACTORY.channel();
    assert!(
        events_on(&deploy.log, &factory_channel).is_empty(),
        "the call never reaches the factory channel"
    );
    assert!(
        session.values("rrs-out").await.is_empty(),
        "no handle is replied"
    );
    // The same call with a single send creates exactly one space.
    let control = session
        .run(
            r#"
new factory(`rho:space:hashmap:queue`), ret in {
  factory!(*ret, "default") |
  for (@handle <- ret) { @"rrs-out"!(handle) }
}
"#,
            "rrs-p4-factory-single",
        )
        .await;
    assert_eq!(
        events_on(&control.log, &factory_channel),
        vec![('P', false), ('M', true)],
        "one call datum meets the persistent factory install once"
    );
    let space = native(&session.sole("rrs-out").await);
    assert_eq!(
        session.space_data_channels().await,
        BTreeSet::from([
            namespace::config_channel(space),
            namespace::space_counter(space)
        ])
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_factory_kwargs_fold() {
    let mut session = Session::new().await;
    // A named param is folded into the constructor list, so the factory checks it against the
    // schema of the family. The queue schema is empty.
    let unknown = session
        .fail_with(
            r#"new factory(`rho:space:hashmap:queue`) in { for (q <- factory!?("default"; capacity=4)) { Nil } }"#,
            "rrs-p4-kwargs-named",
            &Injections::new(),
            &["UnknownParam"],
        )
        .await;
    assert!(
        format!("{:?}", unknown.result.errors).contains("\\\"capacity\\\" is not a param"),
        "the error names the param: {:?}",
        unknown.result.errors
    );
    session
        .fail_with(
            r#"new factory(`rho:space:hashmap:queue`) in { for (q <- factory!?(; capacity=4)) { Nil } }"#,
            "rrs-p4-kwargs-only",
            &Injections::new(),
            &["UnknownParam"],
        )
        .await;
    // The hook rejects positional params before it packs the call.
    session
        .fail_with(
            r#"new factory(`rho:space:hashmap:queue`) in { for (q <- factory!?("default"; 4)) { Nil } }"#,
            "rrs-p4-kwargs-positional",
            &Injections::new(),
            &["PositionalParamsUnsupported"],
        )
        .await;
    // The params come last in the constructor list, so the earlier gates see them after the
    // qualifier and the theory.
    session
        .fail_with(
            r#"new factory(`rho:space:hashmap:queue`) in { for (q <- factory!?("seq"; capacity=4)) { Nil } }"#,
            "rrs-p4-kwargs-after-qualifier",
            &Injections::new(),
            &["QualifierUnavailable"],
        )
        .await;
    session
        .fail_with(
            r#"new factory(`rho:space:hashmap:queue`) in { for (q <- factory!?(free Nat(); capacity=4)) { Nil } }"#,
            "rrs-p4-kwargs-after-theory",
            &Injections::new(),
            &["TheoryUnavailable"],
        )
        .await;
    // An empty params section adds no PARAMS tuple: the call is the call without params.
    let mut folded = Session::new().await;
    folded
        .run(
            r#"new factory(`rho:space:hashmap:queue`) in { for (q <- factory!?("default";)) { @"rrs-handle"!(*q) } }"#,
            "rrs-p4-kwargs-empty",
        )
        .await;
    let mut plain = Session::new().await;
    plain
        .run(
            r#"new factory(`rho:space:hashmap:queue`) in { for (q <- factory!?("default")) { @"rrs-handle"!(*q) } }"#,
            "rrs-p4-kwargs-empty",
        )
        .await;
    let folded_handle = folded.sole("rrs-handle").await;
    assert_eq!(
        folded_handle,
        plain.sole("rrs-handle").await,
        "the same call random gives the same space"
    );
    let config = folded.data(&config_channel(native(&folded_handle))).await;
    let map =
        record::config_map(&config.first().expect("a config record").a).expect("a config record");
    assert_eq!(
        SpaceConfig::from_par(map).expect("a config map").params,
        BTreeMap::new(),
        "the space has no params"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_factory_creates_config_counter_handle() {
    let mut session = Session::new().await;
    let deploy = session
        .run(
            r#"new factory(`rho:space:hashmap:queue`) in { for (q <- factory!?("default")) { @"rrs-handle"!(*q) } }"#,
            "rrs-p4-factory-creates",
        )
        .await;
    let handle = session.sole("rrs-handle").await;
    let space = native(&handle);
    assert_eq!(
        handle,
        handle_capability(space),
        "the reply is bundle+{{H(sid)}}"
    );
    assert_eq!(space.durability, Durability::Durable);
    // C(sid): one persistent config record with the I-17 map and no cost authority.
    let config = session.data(&config_channel(space)).await;
    let [config] = <[Datum<ListParWithRandom>; 1]>::try_from(config)
        .unwrap_or_else(|data| panic!("C(sid) holds {} records", data.len()));
    assert!(config.persist, "the config record is persistent");
    assert_eq!(config.a.cost_authority, None);
    assert!(!config.a.random_state.is_empty());
    let map = record::config_map(&config.a).expect("the config record decodes");
    let decoded = SpaceConfig::from_par(map).expect("the config map decodes");
    assert_eq!(
        decoded,
        SpaceConfig::new(space.sid, "hashmap", "queue", SpaceQualifier::Default)
    );
    assert_eq!(map, &decoded.to_par(), "the stored map is canonical");
    // S(sid): one counter record with the value 0.
    let counters = session.data(&space_counter_channel(space)).await;
    let [counter] = <[Datum<ListParWithRandom>; 1]>::try_from(counters)
        .unwrap_or_else(|data| panic!("S(sid) holds {} records", data.len()));
    assert!(!counter.persist, "the space counter is not persistent");
    assert_eq!(record::counter_value(&counter.a), Ok(0));
    // H(sid): one persistent handle continuation that forwards each call to HANDLE_OP.
    let waiters = session.waiters(&handle_channel(space)).await;
    let [waiter] = <[WaitingContinuation<BindPattern, TaggedContinuation>; 1]>::try_from(waiters)
        .unwrap_or_else(|waiters| panic!("H(sid) holds {} continuations", waiters.len()));
    assert!(waiter.persist, "the handle continuation is persistent");
    assert!(waiter.peeks.is_empty());
    assert_eq!(waiter.patterns, vec![handle_pattern()]);
    let expected = handle_continuation(space, &Blake2b512Random::create_from_bytes(b"any"));
    let body_of = |continuation: &TaggedContinuation| match &continuation.tagged_cont {
        Some(TaggedCont::ParBody(body)) => body.body.clone(),
        other => panic!("the handle continuation is not a Par body: {other:?}"),
    };
    assert_eq!(body_of(&waiter.continuation), body_of(&expected));
    assert_eq!(waiter.continuation.guard, None);
    assert_eq!(waiter.continuation.cost_authority, None);
    // The log holds the three staged operations of the factory (P4 design B.6, row F).
    assert_eq!(events_on(&deploy.log, &config_channel(space)), vec![(
        'P', true
    )]);
    assert_eq!(events_on(&deploy.log, &space_counter_channel(space)), vec![
        ('P', false)
    ]);
    assert_eq!(events_on(&deploy.log, &handle_channel(space)), vec![(
        'C', true
    )]);
    // The records reach the history at the checkpoint and come back after a reset.
    let root = session.play.create_checkpoint().await.root;
    session
        .play
        .reset(&root)
        .await
        .expect("reset the play runtime to the checkpoint root");
    assert_eq!(session.data(&config_channel(space)).await.len(), 1);
    assert_eq!(session.data(&space_counter_channel(space)).await.len(), 1);
    assert_eq!(session.waiters(&handle_channel(space)).await.len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_factory_rejects_bad_constructor() {
    let mut session = Session::new().await;
    for (label, arguments, detail) in [
        ("int", "1", "the constructor is not [qualifier?]"),
        (
            "two-qualifiers",
            r#""default", "default""#,
            "the constructor is not",
        ),
        (
            "theory-first",
            r#"free Nat(), "default""#,
            "the constructor is not",
        ),
        (
            "unknown-qualifier",
            r#""bogus""#,
            "unknown space qualifier: bogus",
        ),
        (
            "uppercase-qualifier",
            r#""Default""#,
            "unknown space qualifier: Default",
        ),
        ("list", r#"["default"]"#, "the constructor is not"),
        (
            "map",
            r#"{"qualifier": "default"}"#,
            "the constructor is not",
        ),
    ] {
        let source = format!(
            "new factory(`rho:space:hashmap:queue`) in {{ for (q <- factory!?({arguments})) {{ @\"rrs-handle\"!(*q) }} }}"
        );
        let deploy = session
            .fail_with(
                &source,
                &format!("rrs-p4-bad-constructor-{label}"),
                &Injections::new(),
                &["BadConstructorArgument"],
            )
            .await;
        assert!(
            format!("{:?}", deploy.result.errors).contains(detail),
            "{label}: {:?}",
            deploy.result.errors
        );
    }
    assert!(
        session.values("rrs-handle").await.is_empty(),
        "no constructor made a space"
    );
    assert!(session.space_data_channels().await.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_factory_p4_gates_fail_closed() {
    let mut session = Session::new().await;
    for (label, arguments, expected) in [
        ("temp", r#""temp""#, "QualifierUnavailable"),
        ("seq", r#""seq""#, "QualifierUnavailable"),
        ("theory", "free Nat()", "TheoryUnavailable"),
        (
            "theory-with-args",
            "free Bounded(1, 2)",
            "TheoryUnavailable",
        ),
        (
            "qualifier-before-theory",
            r#""seq", free Nat()"#,
            "QualifierUnavailable",
        ),
    ] {
        let source = format!(
            "new factory(`rho:space:hashmap:queue`) in {{ for (q <- factory!?({arguments})) {{ Nil }} }}"
        );
        session
            .fail_with(
                &source,
                &format!("rrs-p4-gate-{label}"),
                &Injections::new(),
                &[expected],
            )
            .await;
    }
    // A family or a FIPS alias without a registration has no binding, so a program that names it
    // fails before any space operation. P5 and P6 register a family only when the family
    // dispatch wires it. Phase 2 of P5e wires the PathMap families, so their factories and the
    // PathMap alias are registered, and P6 wires the two VectorDB families.
    let registered: BTreeSet<&str> = registered_p5_channels()
        .into_iter()
        .chain(registered_p6_channels())
        .map(|channel| channel.urn)
        .collect();
    for wired in [
        "rho:space:pathmap:queue",
        PATHMAP_BAG_ALIAS,
        "rho:space:hashmap:vectordb",
        "rho:space:vector:vectordb",
    ] {
        assert!(registered.contains(wired), "{wired}");
    }
    // The PathMap and Array stores have no VectorDB family (design v3 4.2), so their factories
    // are never registered, and the loop always tests them.
    for never in ["rho:space:pathmap:vectordb", "rho:space:array:vectordb"] {
        assert!(!registered.contains(never), "{never}");
    }
    for urn in [
        "rho:space:hashmap:bag",
        "rho:space:hashmap:stack",
        "rho:space:pathmap:queue",
        "rho:space:pathmap:vectordb",
        "rho:space:array:vectordb",
        HASHMAP_BAG_ALIAS,
        PATHMAP_BAG_ALIAS,
    ]
    .into_iter()
    .filter(|urn| !registered.contains(urn))
    {
        let source =
            format!("new factory(`{urn}`) in {{ for (q <- factory!?(\"default\")) {{ Nil }} }}");
        let deploy = session.deploy(&source, &format!("rrs-p4-gate-{urn}")).await;
        let errors = format!("{:?}", deploy.result.errors);
        assert!(
            errors.contains(&format!("No value set for {urn}")),
            "{urn}: {errors}"
        );
        assert!(deploy.residue.is_empty(), "{urn}");
    }
    // A space that a user agent implements is not available until P8.
    session
        .fail_with(
            r#"use @"user agent" { @"x"!(1) }"#,
            "rrs-p4-gate-agent",
            &Injections::new(),
            &["AgentSpacesUnavailable"],
        )
        .await;
    assert!(session.space_data_channels().await.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_system_ops_assert_equal() {
    let mut session = Session::new().await;
    let cases = [
        ("1 | 2", "2 | 1", true),
        ("1", "1", true),
        ("1", "2", false),
        ("[1, 2]", "[2, 1]", false),
        (r#"{"a": 1, "b": 2}"#, r#"{"b": 2, "a": 1}"#, true),
        ("Set(1, 2)", "Set(2, 1)", true),
        ("Nil", "Nil", true),
        (r#""text""#, r#""text""#, true),
        ("(1, 2)", "(2, 1)", false),
    ];
    let mut source = String::from("new equal(`rho:assert:assertEqual`) in {\n");
    for (index, (left, right, _)) in cases.iter().enumerate() {
        source.push_str(&format!(
            "  for (@result <- equal!?({left}, {right})) {{ @\"rrs-equal-{index}\"!(result) }} |\n"
        ));
    }
    source.push_str("  Nil\n}");
    session.run(&source, "rrs-p4-assert-equal").await;
    for (index, (left, right, expected)) in cases.iter().enumerate() {
        assert_eq!(
            session.sole(&format!("rrs-equal-{index}")).await,
            gbool(*expected),
            "assertEqual({left}, {right})"
        );
    }
    // The process takes exactly three arguments. Another arity leaves the call datum unmatched.
    let deploy = session
        .run(
            r#"new equal(`rho:assert:assertEqual`), ret in { equal!(*ret, 1) | for (@r <- ret) { @"rrs-two"!(r) } }"#,
            "rrs-p4-assert-equal-arity",
        )
        .await;
    assert!(session.values("rrs-two").await.is_empty());
    assert_eq!(
        events_on(&deploy.log, &ASSERT_EQUAL_PROCESS.channel()),
        vec![('P', false)],
        "the call datum stays on the op channel"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_system_ops_get_space_agent_capability() {
    let mut session = Session::new().await;
    let (space, _, _) = session.setup().await;
    let deploy = session
        .run(
            r#"
new spaceAgent(`rho:space:getSpaceAgent`), factory(`rho:space:hashmap:queue`) in {
  for (queue <<- @"rrs-handle") {
    for (@capability <- spaceAgent!?(*queue)) {
      @"rrs-capability"!(capability) |
      @"rrs-same"!(capability == *factory) |
      for (second <- @capability!?("default")) { @"rrs-second"!(*second) }
    } |
    for (@again <- spaceAgent!?(bundle{*queue})) { @"rrs-again"!(again) }
  }
}
"#,
            "rrs-p4-get-space-agent",
        )
        .await;
    let capability = session.sole("rrs-capability").await;
    assert_eq!(
        Some(&capability),
        session.play.reducer.urn_map.get("rho:space:hashmap:queue"),
        "the reply is the URN map value of the family"
    );
    assert_eq!(
        Some(capability.clone()),
        factory_capability(SpaceFactoryUrn::HASHMAP_QUEUE)
    );
    assert_eq!(session.sole("rrs-same").await, gbool(true));
    assert_eq!(session.sole("rrs-again").await, capability);
    // The capability is a working factory.
    let second = native(&session.sole("rrs-second").await);
    assert_ne!(second.sid, space.sid, "the capability creates a new space");
    // getSpaceAgent reads the config record and writes nothing to the space.
    assert!(events_on(&deploy.log, &config_channel(space)).is_empty());
    assert!(events_on(&deploy.log, &space_counter_channel(space)).is_empty());
    assert_eq!(session.data(&config_channel(space)).await.len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_system_ops_get_space_agent_dangling_handle() {
    let mut session = Session::new().await;
    let missing = fabricated_space(0x5A);
    let injections = Injections::from([(injected("handle"), handle_channel(missing))]);
    let source = format!(
        "new spaceAgent(`rho:space:getSpaceAgent`), handle(`{}`) in {{ for (@reply <- spaceAgent!?(*handle)) {{ @\"rrs-out\"!(reply) }} }}",
        injected("handle")
    );
    let deploy = session
        .deploy_with(&source, "rrs-p4-get-space-agent-dangling", &injections)
        .await;
    assert!(
        deploy.result.errors.is_empty(),
        "a dangling handle is a typed reply, not an abort: {:?}",
        deploy.result.errors
    );
    assert_eq!(
        session.sole("rrs-out").await,
        typed(SpaceReplyCode::DanglingHandle)
    );
    // A handle of a space whose creation was reverted is dangling too.
    let reverted = session
        .deploy(
            r#"
new factory(`rho:space:hashmap:queue`) in {
  for (queue <- factory!?("default")) { @"rrs-reverted"!(*queue) | @"rrs-x"!!(1) | @"rrs-x"!!(1; k=1) }
}
"#,
            "rrs-p4-get-space-agent-reverted",
        )
        .await;
    assert_eq!(reverted.codes(), vec!["ParamsUnsupported"]);
    assert!(
        !reverted.residue.is_empty(),
        "the reverted deploy created its space before it failed"
    );
    assert!(session.values("rrs-reverted").await.is_empty());
    let created = reverted
        .residue
        .iter()
        .find_map(|id| match namespace::decode_reified_id(id) {
            Some(ReifiedId::Config(space)) => Some(space),
            _ => None,
        })
        .expect("the reverted deploy stored a config record");
    let injections = Injections::from([(injected("handle"), handle_channel(created))]);
    let source = source.replace("rrs-out", "rrs-out-reverted");
    session
        .deploy_with(
            &source,
            "rrs-p4-get-space-agent-reverted-handle",
            &injections,
        )
        .await;
    assert_eq!(
        session.sole("rrs-out-reverted").await,
        typed(SpaceReplyCode::DanglingHandle)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_system_ops_get_space_agent_not_native() {
    let mut session = Session::new().await;
    session.setup().await;
    let cases = [
        ("string", r#""user agent""#, SpaceReplyCode::NotANativeSpace),
        ("list", "[1, 2]", SpaceReplyCode::NotANativeSpace),
        ("plain-name", "*plain", SpaceReplyCode::NotANativeSpace),
        ("nil", "Nil", SpaceReplyCode::EmptySpaceRef),
        ("space-name", "*a", SpaceReplyCode::NotASpaceRef),
        ("factory", "*factory", SpaceReplyCode::NotASpaceRef),
        (
            "read-bundle",
            "bundle-{*queue}",
            SpaceReplyCode::HandleNotWritable,
        ),
        (
            "no-flags",
            "bundle0{*queue}",
            SpaceReplyCode::HandleNotWritable,
        ),
    ];
    let mut source = String::from(
        "new spaceAgent(`rho:space:getSpaceAgent`), factory(`rho:space:hashmap:queue`), plain in {\n  for (queue <<- @\"rrs-handle\") { for (a, b <<- @\"rrs-names\") {\n",
    );
    for (label, reference, _) in &cases {
        source.push_str(&format!(
            "    for (@reply <- spaceAgent!?({reference})) {{ @\"rrs-{label}\"!(reply) }} |\n"
        ));
    }
    source.push_str("    Nil\n  } }\n}");
    session
        .run(&source, "rrs-p4-get-space-agent-not-native")
        .await;
    for (label, reference, code) in cases {
        assert_eq!(
            session.sole(&format!("rrs-{label}")).await,
            typed(code),
            "getSpaceAgent({reference})"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_system_ops_arity_and_body_refs_stable() {
    // The four processes of P4 first, then the registered rows of P5 in the order of the table,
    // then the registered rows of P6, then the processes of P6.
    let mut pinned = vec![
        ("rho:space:hashmap:queue", 0x01_u8, 2, 201),
        ("rho:space:getSpaceAgent", 0x21, 2, 233),
        ("rho:assert:assertEqual", 0x22, 3, 234),
        ("rho:reified:internal:handleOp", 0x30, 2, 248),
    ];
    pinned.extend(registered_p5_pins());
    pinned.extend(registered_p6_pins());
    pinned.extend(P6_PROCESS_PINS);
    let defined = definitions(&unavailable());
    assert_eq!(defined.len(), pinned.len());
    for (definition, (urn, op, arity, body)) in defined.iter().zip(pinned) {
        assert_eq!(definition.urn, urn);
        assert_eq!(
            definition.fixed_channel,
            namespace::gprivate(vec![REIFIED_TAG, op])
        );
        assert_eq!(definition.arity, arity, "{urn}");
        assert_eq!(definition.body_ref, body, "{urn}");
        assert_eq!(definition.remainder, None, "{urn} has a fixed arity");
        assert_eq!(
            definition.to_urn_map(),
            (
                urn.to_owned(),
                write_bundle(definition.fixed_channel.clone())
            )
        );
    }
    // Through deploys: a call with another arity never reaches the process, and the factory
    // packs any number of constructor arguments into one list.
    let mut session = Session::new().await;
    let deploy = session
        .run(
            r#"
new spaceAgent(`rho:space:getSpaceAgent`), equal(`rho:assert:assertEqual`), r1, r2 in {
  spaceAgent!(*r1) | equal!(*r2, 1, 2, 3) |
  for (@x <- r1) { @"rrs-agent"!(x) } | for (@y <- r2) { @"rrs-equal"!(y) }
}
"#,
            "rrs-p4-arity",
        )
        .await;
    assert!(session.values("rrs-agent").await.is_empty());
    assert!(session.values("rrs-equal").await.is_empty());
    assert_eq!(
        events_on(&deploy.log, &GET_SPACE_AGENT_PROCESS.channel()),
        vec![('P', false)]
    );
    assert_eq!(
        events_on(&deploy.log, &ASSERT_EQUAL_PROCESS.channel()),
        vec![('P', false)]
    );
    session
        .fail_with(
            r#"new factory(`rho:space:hashmap:queue`) in { for (q <- factory!?("default", "seq", "temp")) { Nil } }"#,
            "rrs-p4-arity-factory",
            &Injections::new(),
            &["BadConstructorArgument"],
        )
        .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_system_ops_persistent_send_aborts() {
    let mut session = Session::new().await;
    session.setup().await;
    let cases = [
        (
            "factory",
            r#"new factory(`rho:space:hashmap:queue`), ret in { factory!!(*ret, "default") | for (@x <- ret) { @"rrs-out"!(x) } }"#,
            "PersistentFactoryCall",
            1,
        ),
        (
            "get-space-agent",
            r#"new spaceAgent(`rho:space:getSpaceAgent`), ret in { for (q <<- @"rrs-handle") { spaceAgent!!(*ret, *q) } | for (@x <- ret) { @"rrs-out"!(x) } }"#,
            "PersistentOpCall",
            1,
        ),
        (
            "assert-equal",
            r#"new equal(`rho:assert:assertEqual`), ret in { equal!!(*ret, 1, 1) | for (@x <- ret) { @"rrs-out"!(x) } }"#,
            "PersistentOpCall",
            1,
        ),
        (
            "handle-op",
            r#"new ops(`rho:reified:internal:handleOp`), ret in { for (q <<- @"rrs-handle") { ops!!(*q, [*ret, "gensym"]) } | for (@x <- ret) { @"rrs-out"!(x) } }"#,
            "PersistentOpCall",
            0,
        ),
        (
            "handle",
            r#"new ret in { for (q <<- @"rrs-handle") { q!!(*ret, "gensym") } | for (@x <- ret) { @"rrs-out"!(x) } }"#,
            "PersistentHandleCall",
            1,
        ),
    ];
    for (label, source, code, replies) in cases {
        let out = format!("rrs-out-{label}");
        let source = source.replace("rrs-out", &out);
        let deploy = session
            .fail_with(
                &source,
                &format!("rrs-p4-persistent-{label}"),
                &Injections::new(),
                &[code],
            )
            .await;
        assert!(
            deploy
                .log
                .iter()
                .flat_map(event_channels)
                .all(|channel| P4_SYSTEM_CHANNELS
                    .iter()
                    .all(|system| hash(&system.channel()) != channel)),
            "{label}: no event reaches a Reified op channel"
        );
        assert!(session.values(&out).await.is_empty(), "{label}: no reply");
        // The same call with a single send runs. A direct call of HANDLE_OP with the handle
        // capability is dropped, because only the bare handle is accepted (A6).
        let single = source.replacen("!!(", "!(", 1);
        session
            .run(&single, &format!("rrs-p4-single-{label}"))
            .await;
        assert_eq!(
            session.values(&out).await.len(),
            replies,
            "{label}: the replies of the single send"
        );
    }
}
