// P4 integration tests of the space handle and of the handle process HANDLE_OP (P4 design C.8,
// A6, A16, A21, A22 and A23).
//
// A factory replies the handle capability `bundle+{H(sid)}`. Each call `queue!(ret, method,
// a…)` on it meets the persistent handle continuation on `H(sid)`, which forwards the call to
// HANDLE_OP with the bare handle `H(sid)` as `this`. Every test drives the handle through
// Rholang deploys on a play runtime and reads the replies from public channels.
//
// Some tests need a name that no program can make, for example a handle of a space that does
// not exist. A deploy receives such a name through a URI binder `new x(`rho:rrs:test:x`)` and
// the normalizer environment. This is how Casper passes the deployer id to a deploy.

#[path = "../../models/tests/support/reified_strategies.rs"]
mod reified_strategies;

use std::collections::{BTreeSet, HashMap};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::{ListParWithRandom, Par};
use reified_strategies::{gint, gstring};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::Sig;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::interpreter::EvaluateResult;
use rholang::rust::interpreter::reified::handle::typed_error;
use rholang::rust::interpreter::reified::route::{classify_channel, ChannelClass};
use rholang::rust::interpreter::reified::scope::{classify_space_ref, SpaceRef};
use rholang::rust::interpreter::reified::system_channels::HANDLE_OP_PROCESS;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::spaces::errors::SpaceReplyCode;
use rholang::rust::interpreter::spaces::namespace::{
    self, CollectionKey, Durability, Kind, NameTail, NativeSpace, ReifiedId, REIFIED_TAG, SID_LEN,
};
use rholang::rust::interpreter::spaces::record;
use rholang::rust::interpreter::spaces::types::config::SpaceConfig;
use rholang::rust::interpreter::spaces::types::qualifier::SpaceQualifier;
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::hash;
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent};

/// True when `event` is a COMM whose consume is on the channel with the hash `key`.
fn is_comm_on(event: &Event, key: &Blake2b256Hash) -> bool {
    match event {
        Event::Comm(comm) => comm.consume.channel_hashes.as_slice() == std::slice::from_ref(key),
        Event::IoEvent(_) => false,
    }
}

const DEPLOY_SIGNATURE: &[u8] = b"rrs-p4-handle-deploy";
const FUNDING_SIGNATURE: &[u8] = b"rrs-p4-handle-payer";
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

/// `body` with the handle of the setup space bound to `queue` and its names bound to `a` and
/// `b`. A later deploy reads them from the store with peeks, as a user program does.
fn with_space(body: &str) -> String {
    format!("for (queue <<- @\"rrs-handle\") {{ for (a, b <<- @\"rrs-names\") {{\n{body}\n}} }}")
}

/// The URI of an injected name.
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
    async fn new() -> Self {
        let mut manager = InMemoryStoreManager::new();
        let stores = manager
            .r_space_stores()
            .await
            .expect("create the in-memory RSpace stores");
        let (play, _replay, _history) = create_runtimes(stores, false, &mut Vec::new()).await;
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

    /// A deploy that must succeed.
    async fn run_with(&mut self, source: &str, label: &str, injections: &Injections) -> Deploy {
        let deploy = self.deploy_with(source, label, injections).await;
        assert!(
            deploy.result.errors.is_empty(),
            "{label}: the deploy fails: {:?}",
            deploy.result.errors
        );
        deploy
    }

    async fn run(&mut self, source: &str, label: &str) -> Deploy {
        self.run_with(source, label, &Injections::new()).await
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

    /// Runs `SETUP` and returns the space and its two names.
    async fn setup(&mut self) -> (NativeSpace, Vec<Par>) {
        self.run(SETUP, "rrs-p4-handle-setup").await;
        let space = native(&self.sole("rrs-handle").await);
        let names = match <[Vec<Par>; 1]>::try_from(self.values("rrs-names").await) {
            Ok([names]) => names,
            Err(data) => panic!("@\"rrs-names\" holds {} data", data.len()),
        };
        (space, names)
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

fn handle_channel(space: NativeSpace) -> Par {
    namespace::gprivate(namespace::handle_channel(space))
}

fn data_channel(space: NativeSpace, channel: &Par) -> Par {
    let key = CollectionKey::of_channel(channel).expect("a channel has a collection key");
    namespace::gprivate(namespace::data_channel(space, &key))
}

fn fabricated_space(fill: u8) -> NativeSpace {
    NativeSpace {
        sid: [fill; SID_LEN],
        durability: Durability::Durable,
    }
}

/// A 34-byte id of `kind` that no program can make.
fn fabricated_id(kind: Kind, fill: u8) -> Par {
    let mut id = vec![REIFIED_TAG, kind as u8];
    id.extend([fill; SID_LEN]);
    namespace::gprivate(id)
}

/// The typed error reply of the method `method` with the detail entries `detail`.
fn method_error(code: SpaceReplyCode, method: &str, detail: Vec<(&str, Par)>) -> Par {
    let mut entries = Vec::with_capacity(detail.len() + 1);
    entries.push(("method", gstring(method)));
    entries.extend(detail);
    typed_error(code, entries)
}

fn bad_argument(method: &str, argument: &str) -> Par {
    method_error(SpaceReplyCode::BadArgument, method, vec![(
        "argument",
        gstring(argument),
    )])
}

/// True when an event of `log` touches the channel `channel`.
fn touches(log: &[Event], channel: &Par) -> bool {
    let key = hash(channel);
    log.iter().any(|event| match event {
        Event::IoEvent(IOEvent::Produce(produce)) => produce.channel_hash == key,
        Event::IoEvent(IOEvent::Consume(consume)) => consume.channel_hashes.contains(&key),
        Event::Comm(comm) => comm.consume.channel_hashes.contains(&key),
    })
}

/// The number of COMMs of `log` on the HANDLE_OP channel: the calls that reached the process.
fn handle_op_calls(log: &[Event]) -> usize {
    let key = hash(&HANDLE_OP_PROCESS.channel());
    log.iter().filter(|event| is_comm_on(event, &key)).count()
}

/// The payloads and `ord` of the records on `channel`, in `ord` order.
async fn records(session: &Session, channel: &Par) -> Vec<(i64, Vec<Par>, bool)> {
    let mut records: Vec<(i64, Vec<Par>, bool)> = session
        .data(channel)
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_handle_methods_gensym() {
    let mut session = Session::new().await;
    let (space, _) = session.setup().await;
    let before = session.space_data_channels().await;
    let deploy = session
        .run(
            &with_space(
                r#"for (first <- queue!?("gensym")) { for (second <- queue!?("gensym")) { @"rrs-gensym"!(*first, *second) } }"#,
            ),
            "rrs-p4-gensym",
        )
        .await;
    let names = match <[Vec<Par>; 1]>::try_from(session.values("rrs-gensym").await) {
        Ok([names]) => names,
        Err(data) => panic!("@\"rrs-gensym\" holds {} data", data.len()),
    };
    let [first, second] = <[Par; 2]>::try_from(names).expect("two names");
    assert_ne!(first, second, "each call makes a new name");
    for name in [&first, &second] {
        assert_eq!(classify_channel(name), ChannelClass::SpaceName(space));
        assert!(
            matches!(
                namespace::reified_id_of(name),
                Some(ReifiedId::Name(found, NameTail::Random(_))) if found == space
            ),
            "a gensym name is N(sid‖tail)"
        );
    }
    // gensym touches no storage: no event on the config channel and no new space data.
    assert_eq!(
        handle_op_calls(&deploy.log),
        2,
        "both calls reach HANDLE_OP"
    );
    assert!(!touches(&deploy.log, &config_channel(space)));
    assert_eq!(session.space_data_channels().await, before);
    // The names route to the space: a send and a receive on a name use its collection.
    session
        .run(
            &with_space(
                r#"for (first, second <<- @"rrs-gensym") { first!(10) | for (@v <- second) { @"rrs-second"!(v) } }"#,
            ),
            "rrs-p4-gensym-use",
        )
        .await;
    assert_eq!(records(&session, &data_channel(space, &first)).await, vec![
        (0, vec![gint(10)], false)
    ]);
    assert_eq!(
        session
            .play
            .get_continuations(vec![data_channel(space, &second)])
            .await
            .len(),
        1,
        "the receive on the second name waits in the space"
    );
    assert!(session.values("rrs-second").await.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_handle_methods_produce() {
    let mut session = Session::new().await;
    let (space, names) = session.setup().await;
    let a = names.first().expect("the setup makes names").clone();
    session
        .run(
            &with_space(
                r#"
for (@first <- queue!?("produce", *a, [1, 2])) {
  for (@second <- queue!?("produce", *a, [3])) {
    for (@third <- queue!?("produce", "inbox", [4])) { @"rrs-replies"!(first, second, third) }
  }
}"#,
            ),
            "rrs-p4-produce",
        )
        .await;
    assert_eq!(
        session.values("rrs-replies").await,
        vec![vec![Par::default(), Par::default(), Par::default()]],
        "produce replies Nil after the operation"
    );
    assert_eq!(
        records(&session, &data_channel(space, &a)).await,
        vec![
            (0, vec![gint(1), gint(2)], false),
            (1, vec![gint(3)], false)
        ],
        "each call stores one record with the next ord"
    );
    // A forged channel is a key of the space too, and a receive in a use block reads it.
    let inbox = data_channel(space, &gstring("inbox"));
    assert_eq!(records(&session, &inbox).await, vec![(
        0,
        vec![gint(4)],
        false
    )]);
    session
        .run(
            &with_space(
                r#"
for (@x, @y <- a) { for (@z <- a) { @"rrs-a"!([x, y, z]) } } |
use queue { for (@w <- @"inbox") { new out in { out!(w) | for (@v <- out) { @"rrs-inbox"!(v) } } } }
"#,
            ),
            "rrs-p4-produce-receive",
        )
        .await;
    assert_eq!(session.values("rrs-a").await, vec![vec![
        reified_strategies::list(vec![gint(1), gint(2), gint(3)])
    ]]);
    assert!(records(&session, &data_channel(space, &a)).await.is_empty());
    assert!(records(&session, &inbox).await.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_handle_methods_qualifier() {
    let mut session = Session::new().await;
    let (space, _) = session.setup().await;
    let before = session.space_data_channels().await;
    let deploy = session
        .run(
            &with_space(r#"for (@q <- queue!?("qualifier")) { @"rrs-qualifier"!(q) }"#),
            "rrs-p4-qualifier",
        )
        .await;
    assert_eq!(session.sole("rrs-qualifier").await, gstring("default"));
    assert_eq!(handle_op_calls(&deploy.log), 1);
    assert!(
        !touches(&deploy.log, &config_channel(space)),
        "a read stages nothing"
    );
    assert_eq!(session.space_data_channels().await, before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_handle_methods_config() {
    let mut session = Session::new().await;
    let (space, _) = session.setup().await;
    let deploy = session
        .run(
            &with_space(
                r#"
for (@config <- queue!?("config")) {
  @"rrs-config"!(config) |
  @"rrs-fields"!(config.get("outer"), config.get("inner"), config.get("qualifier"), config.get("params"), config.get("theory"))
}"#,
            ),
            "rrs-p4-config",
        )
        .await;
    let config = session.sole("rrs-config").await;
    let expected = SpaceConfig::new(space.sid, "hashmap", "queue", SpaceQualifier::Default);
    assert_eq!(config, expected.to_par());
    assert_eq!(SpaceConfig::from_par(&config), Ok(expected));
    assert_eq!(session.values("rrs-fields").await, vec![vec![
        gstring("hashmap"),
        gstring("queue"),
        gstring("default"),
        reified_strategies::map(Vec::new()),
        Par::default(),
    ]]);
    let stored = session.data(&config_channel(space)).await;
    let stored = record::config_map(&stored.first().expect("a config record").a)
        .expect("the config record decodes")
        .clone();
    assert_eq!(config, stored, "config replies the stored map");
    assert!(
        !touches(&deploy.log, &config_channel(space)),
        "a read stages nothing"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_handle_methods_consume_rejects_foreign_matcher() {
    let mut session = Session::new().await;
    session.setup().await;
    let injections = Injections::from([
        (injected("body"), fabricated_id(Kind::Body, 0x0B)),
        (injected("matcher"), fabricated_id(Kind::Matcher, 0x0C)),
    ]);
    let cases = [
        (
            "arrow",
            r#""consume", [(*matcher, *a)], "<--", *body"#,
            method_error(SpaceReplyCode::InvalidArrow, "consume", Vec::new()),
        ),
        (
            "pairs",
            r#""consume", "pairs", "<-", *body"#,
            bad_argument("consume", "pairs"),
        ),
        (
            "empty-pairs",
            r#""consume", [], "<-", *body"#,
            bad_argument("consume", "pairs"),
        ),
        (
            "triple",
            r#""consume", [(*matcher, *a, 1)], "<-", *body"#,
            bad_argument("consume", "pairs"),
        ),
        (
            "plain-body",
            r#""consume", [(*matcher, *a)], "<=", *plain"#,
            bad_argument("consume", "body"),
        ),
        (
            "foreign-first",
            r#""consume", [(*plain, *a)], "<-", *body"#,
            method_error(SpaceReplyCode::ForeignMatcher, "consume", vec![(
                "index",
                gint(0),
            )]),
        ),
        (
            "foreign-second",
            r#""consume", [(*matcher, *a), (*plain, *b)], "<<-", bundle+{*body}"#,
            method_error(SpaceReplyCode::ForeignMatcher, "consume", vec![(
                "index",
                gint(1),
            )]),
        ),
    ];
    let mut source = format!(
        "new body(`{}`), matcher(`{}`), plain in {{\n",
        injected("body"),
        injected("matcher")
    );
    let mut body = String::new();
    for (label, arguments, _) in &cases {
        body.push_str(&format!(
            "for (@reply <- queue!?({arguments})) {{ @\"rrs-{label}\"!(reply) }} |\n"
        ));
    }
    body.push_str("Nil");
    source.push_str(&with_space(&body));
    source.push_str("\n}");
    session
        .run_with(&source, "rrs-p4-consume-shapes", &injections)
        .await;
    for (label, arguments, expected) in cases {
        assert_eq!(
            session.sole(&format!("rrs-{label}")).await,
            expected,
            "consume({arguments})"
        );
    }
    // When every matcher is a user-agent matcher, the call needs P8 and aborts.
    let source = format!(
        "new body(`{}`), matcher(`{}`) in {{\n{}\n}}",
        injected("body"),
        injected("matcher"),
        with_space(
            r#"for (@reply <- queue!?("consume", [(*matcher, *a)], "<-", *body)) { @"rrs-agent-only"!(reply) }"#
        )
    );
    session
        .fail_with(&source, "rrs-p4-consume-agent-only", &injections, &[
            "AgentSpacesUnavailable",
        ])
        .await;
    assert!(session.values("rrs-agent-only").await.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_handle_methods_persistent_kwarg() {
    let mut session = Session::new().await;
    let (space, names) = session.setup().await;
    let a = names.first().expect("the setup makes names").clone();
    let b = names.get(1).expect("the setup makes two names").clone();
    session
        .run(
            &with_space(
                r#"
for (_ <- queue!?("produce", *a, [7]; persistent=true)) {
  for (_ <- queue!?("produce", *b, [8]; persistent=false)) {
    for (@x <- a) { for (@y <- a) { @"rrs-persistent"!(x, y) } }
  }
}"#,
            ),
            "rrs-p4-persistent-kwarg",
        )
        .await;
    assert_eq!(
        session.values("rrs-persistent").await,
        vec![vec![gint(7), gint(7)]],
        "both receives read the persistent record"
    );
    assert_eq!(
        records(&session, &data_channel(space, &a)).await,
        vec![(0, vec![gint(7)], true)],
        "the persistent record stays"
    );
    assert_eq!(records(&session, &data_channel(space, &b)).await, vec![(
        0,
        vec![gint(8)],
        false
    )]);
    let cases = [
        (
            "not-a-bool",
            "persistent=1",
            bad_argument("produce", "persistent"),
        ),
        (
            "typo",
            "persistant=true",
            method_error(SpaceReplyCode::UnknownParam, "produce", vec![
                ("param", gstring("persistant")),
                ("suggestion", gstring("persistent")),
            ]),
        ),
        (
            "priority",
            "priority=1",
            method_error(SpaceReplyCode::UnknownParam, "produce", vec![
                ("param", gstring("priority")),
                ("suggestion", Par::default()),
            ]),
        ),
    ];
    let mut body = String::new();
    for (label, param, _) in &cases {
        body.push_str(&format!(
            "for (@reply <- queue!?(\"produce\", *b, [9]; {param})) {{ @\"rrs-{label}\"!(reply) }} |\n"
        ));
    }
    body.push_str("Nil");
    session
        .run(&with_space(&body), "rrs-p4-persistent-kwarg-errors")
        .await;
    for (label, param, expected) in cases {
        assert_eq!(
            session.sole(&format!("rrs-{label}")).await,
            expected,
            "{param}"
        );
    }
    assert_eq!(
        records(&session, &data_channel(space, &b)).await.len(),
        1,
        "a typed error stores nothing"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_handle_methods_bad_argument_typed() {
    let mut session = Session::new().await;
    let (space, names) = session.setup().await;
    let cases = [
        (
            "gensym-arity",
            r#""gensym", 1"#,
            bad_argument("gensym", "arguments"),
        ),
        (
            "config-arity",
            r#""config", "x""#,
            bad_argument("config", "arguments"),
        ),
        (
            "produce-arity",
            r#""produce", *a"#,
            bad_argument("produce", "arguments"),
        ),
        (
            "produce-msg",
            r#""produce", *a, 7"#,
            bad_argument("produce", "msg"),
        ),
        (
            "produce-read-only",
            r#""produce", bundle-{*a}, [1]"#,
            bad_argument("produce", "ch"),
        ),
        (
            "consume-arity",
            r#""consume", [], "<-""#,
            bad_argument("consume", "arguments"),
        ),
    ];
    let mut body = String::new();
    for (label, arguments, _) in &cases {
        body.push_str(&format!(
            "for (@reply <- queue!?({arguments})) {{ @\"rrs-{label}\"!(reply) }} |\n"
        ));
    }
    body.push_str("Nil");
    let deploy = session
        .run(&with_space(&body), "rrs-p4-bad-arguments")
        .await;
    for (label, arguments, expected) in cases {
        assert_eq!(
            session.sole(&format!("rrs-{label}")).await,
            expected,
            "queue!({arguments})"
        );
    }
    assert_eq!(handle_op_calls(&deploy.log), 6);
    for name in &names {
        assert!(
            records(&session, &data_channel(space, name))
                .await
                .is_empty(),
            "a typed error stores nothing"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_unknown_method_typed_reply() {
    let mut session = Session::new().await;
    let (space, _) = session.setup().await;
    let before = session.space_data_channels().await;
    let cases = [
        ("prodcue", gstring("produce")),
        ("Gensym", gstring("gensym")),
        ("confg", gstring("config")),
        ("frobnicate", Par::default()),
        ("", Par::default()),
    ];
    let mut body = String::new();
    for (index, (method, _)) in cases.iter().enumerate() {
        body.push_str(&format!(
            "for (@reply <- queue!?(\"{method}\")) {{ @\"rrs-method-{index}\"!(reply) }} |\n"
        ));
    }
    body.push_str("Nil");
    let deploy = session
        .run(&with_space(&body), "rrs-p4-unknown-method")
        .await;
    for (index, (method, suggestion)) in cases.into_iter().enumerate() {
        assert_eq!(
            session.sole(&format!("rrs-method-{index}")).await,
            method_error(SpaceReplyCode::UnknownMethod, method, vec![(
                "suggestion",
                suggestion
            )]),
            "{method:?}"
        );
    }
    assert!(!touches(&deploy.log, &config_channel(space)));
    assert_eq!(session.space_data_channels().await, before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_malformed_message_dropped() {
    let mut session = Session::new().await;
    let (space, _) = session.setup().await;
    let before = session.space_data_channels().await;
    // A message without a reply name or without a method string is dropped: no reply, no
    // abort, no space operation (A23).
    let cases = [
        ("empty", "queue!()"),
        ("ret-only", "queue!(*ret)"),
        ("int-ret", r#"queue!(1, "gensym")"#),
        ("string-ret", r#"queue!("ret", "gensym")"#),
        ("read-only-ret", r#"queue!(bundle-{*ret}, "gensym")"#),
        ("int-method", "queue!(*ret, 1)"),
        ("list-method", r#"queue!(*ret, ["gensym"])"#),
    ];
    for (label, call) in cases {
        let deploy = session
            .run(
                &format!(
                    "new ret in {{ {} | for (@reply <- ret) {{ @\"rrs-dropped-{label}\"!(reply) }} }}",
                    with_space(call)
                ),
                &format!("rrs-p4-dropped-{label}"),
            )
            .await;
        assert_eq!(
            handle_op_calls(&deploy.log),
            1,
            "{label}: the call reaches HANDLE_OP"
        );
        assert!(
            session
                .values(&format!("rrs-dropped-{label}"))
                .await
                .is_empty(),
            "{label}: the message is dropped"
        );
    }
    assert_eq!(session.space_data_channels().await, before);
    // The control: a message with a reply name and a method string gets its reply.
    session
        .run(
            &format!(
                "new ret in {{ {} | for (@reply <- ret) {{ @\"rrs-replied\"!(reply) }} }}",
                with_space(r#"queue!(*ret, "qualifier")"#)
            ),
            "rrs-p4-dropped-control",
        )
        .await;
    assert_eq!(session.sole("rrs-replied").await, gstring("default"));
    assert_eq!(session.data(&config_channel(space)).await.len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_internal_op_requires_handle_capability() {
    let mut session = Session::new().await;
    let (space, _) = session.setup().await;
    // HANDLE_OP is URN-bound, so a program can call it. It accepts only the bare handle as
    // `this`. Every other value is dropped with no reply and no abort (A6).
    let forged = [
        ("capability", "*queue"),
        ("rebundled", "bundle{*queue}"),
        ("no-flags", "bundle0{*queue}"),
        ("space-name", "*a"),
        ("plain-name", "*plain"),
        ("string", r#""this""#),
        ("nil", "Nil"),
        ("ops", "*ops"),
    ];
    for (label, this) in forged {
        let deploy = session
            .run(
                &format!(
                    "new ops(`rho:reified:internal:handleOp`), plain, ret in {{ {} | for (@reply <- ret) {{ @\"rrs-forged-{label}\"!(reply) }} }}",
                    with_space(&format!(r#"ops!({this}, [*ret, "config"])"#))
                ),
                &format!("rrs-p4-forged-{label}"),
            )
            .await;
        assert_eq!(
            handle_op_calls(&deploy.log),
            1,
            "{label}: the call reaches HANDLE_OP"
        );
        assert!(
            session
                .values(&format!("rrs-forged-{label}"))
                .await
                .is_empty(),
            "{label}: HANDLE_OP drops the call"
        );
    }
    // The bare handle is the capability: the handle continuation forwards it, and a deploy
    // that holds it (here through injection) gets the reply.
    let injections = Injections::from([(injected("this"), handle_channel(space))]);
    session
        .run_with(
            &format!(
                "new ops(`rho:reified:internal:handleOp`), this(`{}`), ret in {{ ops!(*this, [*ret, \"config\"]) | for (@reply <- ret) {{ @\"rrs-bare\"!(reply) }} }}",
                injected("this")
            ),
            "rrs-p4-bare-handle",
            &injections,
        )
        .await;
    let expected =
        SpaceConfig::new(space.sid, "hashmap", "queue", SpaceQualifier::Default).to_par();
    assert_eq!(session.sole("rrs-bare").await, expected);
    session
        .run(
            &format!(
                "new ret in {{ {} | for (@reply <- ret) {{ @\"rrs-via-handle\"!(reply) }} }}",
                with_space(r#"queue!(*ret, "config")"#)
            ),
            "rrs-p4-via-handle",
        )
        .await;
    assert_eq!(session.sole("rrs-via-handle").await, expected);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_handle_dangling_aborts() {
    let mut session = Session::new().await;
    let missing = fabricated_space(0x6D);
    let injections = Injections::from([(injected("handle"), handle_channel(missing))]);
    for (label, call) in [
        ("gensym", r#"ops!(*handle, [*ret, "gensym"])"#),
        ("config", r#"ops!(*handle, [*ret, "config"])"#),
        ("malformed", r#"ops!(*handle, [1])"#),
    ] {
        let source = format!(
            "new ops(`rho:reified:internal:handleOp`), handle(`{}`), ret in {{ {call} | for (@reply <- ret) {{ @\"rrs-reply\"!(reply) }} }}",
            injected("handle")
        );
        session
            .fail_with(
                &source,
                &format!("rrs-p4-dangling-{label}"),
                &injections,
                &["DanglingHandle"],
            )
            .await;
    }
    // A use block over the fabricated handle aborts at its first operation.
    session
        .fail_with(
            &format!(
                "new handle(`{}`) in {{ use handle {{ @\"x\"!(1) }} }}",
                injected("handle")
            ),
            "rrs-p4-dangling-use",
            &injections,
            &["DanglingHandle"],
        )
        .await;
    assert!(session.values("rrs-reply").await.is_empty());
    assert!(session.space_data_channels().await.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_handle_persistent_send_aborts() {
    let mut session = Session::new().await;
    let (space, _) = session.setup().await;
    let before = session.space_data_channels().await;
    let injections = Injections::from([(injected("bare"), handle_channel(space))]);
    for (label, call) in [
        ("capability", r#"queue!!(*ret, "gensym")"#),
        ("rebundled", r#"@{bundle+{*queue}}!!(*ret, "qualifier")"#),
        ("bare", r#"bare!!(*ret, "config")"#),
    ] {
        let source = format!(
            "new bare(`{}`), ret in {{ {} | for (@reply <- ret) {{ @\"rrs-{label}\"!(reply) }} }}",
            injected("bare"),
            with_space(call)
        );
        let deploy = session
            .fail_with(
                &source,
                &format!("rrs-p4-persistent-handle-{label}"),
                &injections,
                &["PersistentHandleCall"],
            )
            .await;
        assert_eq!(handle_op_calls(&deploy.log), 0, "{label}: no call runs");
        assert!(!touches(&deploy.log, &handle_channel(space)), "{label}");
        assert!(session.values(&format!("rrs-{label}")).await.is_empty());
    }
    assert_eq!(session.space_data_channels().await, before);
    // The single send of the same call replies.
    session
        .run(
            &format!(
                "new ret in {{ {} | for (@reply <- ret) {{ @\"rrs-single\"!(reply) }} }}",
                with_space(r#"queue!(*ret, "qualifier")"#)
            ),
            "rrs-p4-single-handle-call",
        )
        .await;
    assert_eq!(session.sole("rrs-single").await, gstring("default"));
}
