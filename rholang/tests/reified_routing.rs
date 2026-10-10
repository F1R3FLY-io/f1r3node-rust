// P4 integration tests of the routing of channels to spaces and of the space references of
// `use` blocks and `new` annotations (P4 design C.1 to C.6, rules R1 to R7, A6, A18 and A24).
//
// Each test runs Rholang deploys on a play runtime over an in-memory store. The source uses
// the Reified syntax of the v3 parser: `use s { P }` and `new x : s in { P }`. The setup deploy
// creates a space and two names of it, and later deploys read the handle and the names from the
// store with peeks.
//
// Routing rules (P4 design C.1): system channels, configs, handles and other unforgeable names
// route to the default space (R1). Storage ids route to the default space (R2). A space name
// routes to its own space in every context (R3). An agent-tagged name needs P8 (R4). Every other
// channel routes to the space of the use block, or to the default space outside a use block
// (R5). System process replies route without a context (R7).

#[path = "../../models/tests/support/reified_strategies.rs"]
mod reified_strategies;

use std::collections::{BTreeSet, HashMap};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::{ListParWithRandom, Par};
use models::rust::space_factory_urn::SpaceFactoryUrn;
use models::rust::utils::new_bundle_par;
use reified_strategies::{gbool, gint, gstring, list};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::Sig;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::interpreter::EvaluateResult;
use rholang::rust::interpreter::reified::route::{classify_channel, ChannelClass};
use rholang::rust::interpreter::reified::scope::{classify_space_ref, SpaceRef};
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::spaces::factory::urn::factory_capability;
use rholang::rust::interpreter::spaces::namespace::{
    self, CollectionKey, Durability, Kind, NameTail, NativeSpace, ReifiedId, AGENT_TAG,
    REIFIED_TAG, SID_LEN,
};
use rholang::rust::interpreter::spaces::record;
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::hash;
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::event::Event;

/// True when `event` is a COMM whose consume is on the channel with the hash `key`.
fn is_comm_on(event: &Event, key: &Blake2b256Hash) -> bool {
    match event {
        Event::Comm(comm) => comm.consume.channel_hashes.as_slice() == std::slice::from_ref(key),
        Event::IoEvent(_) => false,
    }
}

const DEPLOY_SIGNATURE: &[u8] = b"rrs-p4-routing-deploy";
const FUNDING_SIGNATURE: &[u8] = b"rrs-p4-routing-payer";
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

/// A deploy that creates a second space. Its handle goes to `@"rrs-other"`.
const OTHER: &str = r#"
new factory(`rho:space:hashmap:queue`) in {
  for (other <- factory!?("default")) { @"rrs-other"!(*other) }
}
"#;

/// `body` with the handle of the setup space bound to `queue` and its names bound to `a` and
/// `b`.
fn with_space(body: &str) -> String {
    format!("for (queue <<- @\"rrs-handle\") {{ for (a, b <<- @\"rrs-names\") {{\n{body}\n}} }}")
}

/// `body` with the setup space as above and the second space bound to `other`.
fn with_spaces(body: &str) -> String {
    with_space(&format!("for (other <<- @\"rrs-other\") {{\n{body}\n}}"))
}

fn injected(name: &str) -> String { format!("rho:rrs:test:{name}") }

type Injections = HashMap<String, Par>;

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
    async fn setup(&mut self) -> (NativeSpace, Par, Par) {
        self.run(SETUP, "rrs-p4-routing-setup").await;
        let space = native(&self.sole("rrs-handle").await);
        let names = match <[Vec<Par>; 1]>::try_from(self.values("rrs-names").await) {
            Ok([names]) => names,
            Err(data) => panic!("@\"rrs-names\" holds {} data", data.len()),
        };
        let [a, b] = <[Par; 2]>::try_from(names).expect("the setup makes two names");
        (space, a, b)
    }

    /// Runs `OTHER` and returns the second space.
    async fn other(&mut self) -> NativeSpace {
        self.run(OTHER, "rrs-p4-routing-other").await;
        native(&self.sole("rrs-other").await)
    }

    /// The payloads of the records on the collection of `channel` in `space`, in `ord` order.
    async fn records(&self, space: NativeSpace, channel: &Par) -> Vec<Vec<Par>> {
        let mut records: Vec<(i64, Vec<Par>)> = self
            .data(&data_channel(space, channel))
            .await
            .into_iter()
            .map(|datum| {
                let decoded = record::unwrap_datum(&datum.a).expect("a datum record decodes");
                (decoded.ord, decoded.datum.pars)
            })
            .collect();
        records.sort_by_key(|(ord, _)| *ord);
        records.into_iter().map(|(_, pars)| pars).collect()
    }

    /// The value of the space counter: the number of collections that the space opened.
    async fn opened(&self, space: NativeSpace) -> i64 {
        let data = self
            .data(&namespace::gprivate(namespace::space_counter(space)))
            .await;
        match data.as_slice() {
            [datum] => record::counter_value(&datum.a).expect("a counter record decodes"),
            other => panic!("the space counter holds {} records", other.len()),
        }
    }
}

fn native(handle: &Par) -> NativeSpace {
    match classify_space_ref(handle) {
        Ok(SpaceRef::Native(space)) => space,
        other => panic!("the value is not a native space handle: {other:?}"),
    }
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

/// The number of COMMs of `log` whose consume is on `channel`.
fn comms_on(log: &[Event], channel: &Par) -> usize {
    let key = hash(channel);
    log.iter().filter(|event| is_comm_on(event, &key)).count()
}

mod reducer_space_routing {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reducer_space_routing_space_name_intrinsic() {
        let mut session = Session::new().await;
        let (space, a, _) = session.setup().await;
        let other = session.other().await;
        // In the use block of the other space, a name of the first space still routes to the
        // first space (R3).
        session
            .run(
                &with_spaces("use other { a!(1) } | a!(2)"),
                "rrs-p4-intrinsic",
            )
            .await;
        let mut records = session.records(space, &a).await;
        records.sort_by_key(|pars| format!("{pars:?}"));
        assert_eq!(records, vec![vec![gint(1)], vec![gint(2)]]);
        assert!(session.records(other, &a).await.is_empty());
        assert_eq!(
            session.opened(other).await,
            0,
            "the other space opened no collection"
        );
        // A receive in the use block of the other space reads the first space.
        let receive = session
            .run(
                &with_spaces(r#"use other { for (@x <- a) { for (@y <- a) { Nil } } }"#),
                "rrs-p4-intrinsic-receive",
            )
            .await;
        assert_eq!(comms_on(&receive.log, &data_channel(space, &a)), 2);
        assert!(session.records(space, &a).await.is_empty());
        assert_eq!(session.opened(other).await, 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reducer_space_routing_default_classes() {
        let mut session = Session::new().await;
        let (space, _, _) = session.setup().await;
        let opened = session.opened(space).await;
        let foreign = fabricated_space(0x44);
        let foreign_key =
            CollectionKey::of_channel(&gstring("k")).expect("a channel has a collection key");
        let injections = Injections::from([
            (
                injected("storage"),
                namespace::gprivate(namespace::data_channel(foreign, &foreign_key)),
            ),
            (
                injected("config"),
                namespace::gprivate(namespace::config_channel(foreign)),
            ),
        ]);
        // In a use block, an unforgeable name (R1), a system channel (R1), the handle (R1), a
        // config id (R1) and a storage id (R2) route to the default space.
        let source = format!(
            "new storage(`{}`), config(`{}`), out(`rho:io:stdout`), plain in {{ {} }}",
            injected("storage"),
            injected("config"),
            with_space(
                r#"
new ret in {
  use queue { plain!(1) | out!("routing") | storage!(2) | config!(3) | queue!(*ret, "qualifier") } |
  for (@k <- ret) { plain!(k) } |
  for (@x <- plain) { for (@y <- plain) { @"rrs-plain"!([x, y]) } }
}
"#
            )
        );
        let deploy = session
            .run_with(&source, "rrs-p4-default-classes", &injections)
            .await;
        let plain: Vec<Par> = session
            .values("rrs-plain")
            .await
            .into_iter()
            .flatten()
            .collect();
        assert_eq!(plain.len(), 1);
        assert!(
            plain[0] == list(vec![gint(1), gstring("default")])
                || plain[0] == list(vec![gstring("default"), gint(1)]),
            "the plain name is a host channel, and the handle call works: {plain:?}"
        );
        assert_eq!(
            session
                .data(&namespace::gprivate(namespace::data_channel(
                    foreign,
                    &foreign_key
                )))
                .await
                .len(),
            1,
            "a storage id is a host channel"
        );
        assert_eq!(
            session
                .data(&namespace::gprivate(namespace::config_channel(foreign)))
                .await
                .len(),
            1,
            "a config id is a host channel"
        );
        assert_eq!(
            session.opened(space).await,
            opened,
            "no collection was opened"
        );
        assert_eq!(
            deploy.residue,
            BTreeSet::from([
                namespace::data_channel(foreign, &foreign_key),
                namespace::config_channel(foreign),
            ]),
            "the only new 0xE0 data are the host data on the two injected ids"
        );
        // The classes of these channels.
        for (label, channel, class) in [
            (
                "storage",
                namespace::gprivate(namespace::data_channel(foreign, &foreign_key)),
                ChannelClass::Storage,
            ),
            (
                "config",
                namespace::gprivate(namespace::config_channel(foreign)),
                ChannelClass::DefaultObject,
            ),
            (
                "handle",
                namespace::gprivate(namespace::handle_channel(space)),
                ChannelClass::DefaultObject,
            ),
            (
                "byte name",
                namespace::gprivate(vec![0]),
                ChannelClass::System,
            ),
        ] {
            assert_eq!(classify_channel(&channel), class, "{label}");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reducer_space_routing_use_forged() {
        let mut session = Session::new().await;
        let (space, _, _) = session.setup().await;
        session
            .run(
                &with_space(
                    r#"use queue { @"x"!(1) | @[1, 2]!(2) | @[2, 1]!(3) | @{"x" | 4}!(4) } | @"x"!(5)"#,
                ),
                "rrs-p4-forged",
            )
            .await;
        // In the use block, each forged channel is the collection of its canonical bytes (R5).
        assert_eq!(session.records(space, &gstring("x")).await, vec![vec![
            gint(1)
        ]]);
        assert_eq!(
            session.records(space, &list(vec![gint(1), gint(2)])).await,
            vec![vec![gint(2)]]
        );
        assert_eq!(
            session.records(space, &list(vec![gint(2), gint(1)])).await,
            vec![vec![gint(3)]],
            "a list keeps its order in its key"
        );
        // Outside the use block, the same channel is a host channel.
        assert_eq!(session.values("x").await, vec![vec![gint(5)]]);
        // A parallel composition is one key whatever its order (A5).
        session
            .run(
                &with_space(r#"use queue { for (@v <- @{4 | "x"}) { Nil } }"#),
                "rrs-p4-forged-par-order",
            )
            .await;
        let mut sorted_par = gstring("x");
        sorted_par.exprs.extend(gint(4).exprs);
        assert!(
            session.records(space, &sorted_par).await.is_empty(),
            "the receive on the reordered channel took the record"
        );
        assert_eq!(
            session.opened(space).await,
            4,
            "four collections were opened"
        );
        // In nested use blocks, the innermost block names the space.
        let other = session.other().await;
        session
            .run(
                &with_spaces(r#"use queue { use other { @"nested"!(1) } | @"outer"!(2) }"#),
                "rrs-p4-forged-nested",
            )
            .await;
        assert_eq!(session.records(other, &gstring("nested")).await, vec![
            vec![gint(1)]
        ]);
        assert!(session.records(space, &gstring("nested")).await.is_empty());
        assert_eq!(session.records(space, &gstring("outer")).await, vec![vec![
            gint(2)
        ]]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reducer_space_routing_use_new_mints_space_names() {
        let mut session = Session::new().await;
        let (space, _, _) = session.setup().await;
        session
            .run(
                &with_space(
                    r#"
new out in {
  use queue { new n, stdout(`rho:io:stdout`) in { out!(*n, *stdout) | n!(1) } } |
  for (@n, @stdout <- out) { @"rrs-minted"!(n, stdout) }
}
"#,
                ),
                "rrs-p4-minted",
            )
            .await;
        let minted = session.values("rrs-minted").await;
        let [[name, stdout]] = <[[Par; 2]; 1]>::try_from(
            minted
                .into_iter()
                .map(|pars| <[Par; 2]>::try_from(pars).expect("two values"))
                .collect::<Vec<_>>(),
        )
        .expect("one minted datum");
        assert_eq!(classify_channel(&name), ChannelClass::SpaceName(space));
        assert!(matches!(
            namespace::reified_id_of(&name),
            Some(ReifiedId::Name(found, NameTail::Random(_))) if found == space
        ));
        assert_eq!(
            stdout,
            new_bundle_par(namespace::gprivate(vec![0]), true, false),
            "a URI binder keeps its value in a use block"
        );
        assert_eq!(session.records(space, &name).await, vec![vec![gint(1)]]);
        // The new name keeps its space outside the use block.
        session
            .run(
                &with_space(r#"for (@n, @s <<- @"rrs-minted") { @n!(2) }"#),
                "rrs-p4-minted-outside",
            )
            .await;
        assert_eq!(session.records(space, &name).await, vec![
            vec![gint(1)],
            vec![gint(2)]
        ]);
        // The use block draws the same random state as the plain new: the tail of the space
        // name is the id of the plain name.
        session
            .run(
                &with_space(
                    r#"new out in { new n in { out!(*n) } | for (@n <- out) { @"rrs-plain-new"!(n) } }"#,
                ),
                "rrs-p4-minted-draws",
            )
            .await;
        session
            .run(
                &with_space(
                    r#"new out in { use queue { new n in { out!(*n) } } | for (@n <- out) { @"rrs-use-new"!(n) } }"#,
                ),
                "rrs-p4-minted-draws",
            )
            .await;
        let plain = session.sole("rrs-plain-new").await;
        let minted = session.sole("rrs-use-new").await;
        let tail = match namespace::reified_id_of(&minted) {
            Some(ReifiedId::Name(found, NameTail::Random(tail))) if found == space => tail,
            other => panic!("the use block made {other:?}"),
        };
        assert_eq!(
            namespace::sole_gprivate_id(&plain),
            Some(&tail[..]),
            "the space name wraps the draw of the plain name"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reducer_space_routing_newspace_annotation() {
        let mut session = Session::new().await;
        let (space, _, _) = session.setup().await;
        let other = session.other().await;
        session
            .run(
                &with_spaces(
                    r#"
new out in {
  new x : queue, y in { out!(*x, *y) | x!(1) | y!(2) } |
  use other { new u : queue, v in { out!(*u, *v) } } |
  for (@x, @y <- out) { for (@u, @v <- out) { @"rrs-annotated"!([x, y], [u, v]) } }
}
"#,
                ),
                "rrs-p4-annotation",
            )
            .await;
        let annotated = session.values("rrs-annotated").await;
        let pairs: Vec<Par> = annotated.into_iter().flatten().collect();
        assert_eq!(pairs.len(), 2);
        let mut classes = Vec::with_capacity(4);
        for pair in &pairs {
            let elements = match pair
                .exprs
                .first()
                .and_then(|expr| expr.expr_instance.as_ref())
            {
                Some(models::rhoapi::expr::ExprInstance::EListBody(list)) => list.ps.clone(),
                other => panic!("an annotated pair is a list: {other:?}"),
            };
            classes.extend(elements.iter().map(classify_channel));
        }
        classes.sort_by_key(|class| format!("{class:?}"));
        let mut expected = vec![
            ChannelClass::SpaceName(space),
            ChannelClass::SpaceName(space),
            ChannelClass::SpaceName(other),
            ChannelClass::Unforgeable,
        ];
        expected.sort_by_key(|class| format!("{class:?}"));
        assert_eq!(
            classes, expected,
            "an annotated binder joins its space, a plain binder joins the context or stays plain"
        );
        // The annotation is evaluated in the environment outside the new, so `q` names the
        // handle and not the new binder `q`.
        session
            .run(
                r#"for (q <<- @"rrs-handle") { new q, x : q in { @"rrs-outer"!(*x) } }"#,
                "rrs-p4-annotation-outer",
            )
            .await;
        assert_eq!(
            classify_channel(&session.sole("rrs-outer").await),
            ChannelClass::SpaceName(space)
        );
        // Annotations that name no native space fail closed.
        for (label, source, code) in [
            (
                "agent",
                r#"new x : @"agent" in { Nil }"#,
                "AgentSpacesUnavailable",
            ),
            ("name", "new x : a in { Nil }", "NotASpaceRef"),
            ("nil", "new x : @Nil in { Nil }", "EmptySpaceRef"),
        ] {
            session
                .fail_with(
                    &with_space(source),
                    &format!("rrs-p4-annotation-{label}"),
                    &Injections::new(),
                    &[code],
                )
                .await;
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reducer_space_routing_use_body_closure() {
        let mut session = Session::new().await;
        let (space, _, _) = session.setup().await;
        // A receive in a use block stores its body in the USE tuple of its context. When it
        // fires in a later deploy, outside every use block, the body runs in the context.
        session
            .run(
                &with_space(r#"use queue { for (@v <- @"inbox") { @"outbox"!(v) } }"#),
                "rrs-p4-closure-park",
            )
            .await;
        session
            .run(
                &with_space(r#"use queue { @"inbox"!(5) }"#),
                "rrs-p4-closure-fire",
            )
            .await;
        assert_eq!(session.records(space, &gstring("outbox")).await, vec![
            vec![gint(5)]
        ]);
        assert!(
            session.values("outbox").await.is_empty(),
            "the body did not run on the host"
        );
        // A host receive in a use block keeps the context of its body too.
        session
            .run(
                &with_space(
                    r#"new h in { use queue { for (@v <- h) { @"outbox2"!(v) } } | h!(6) }"#,
                ),
                "rrs-p4-closure-host",
            )
            .await;
        assert_eq!(session.records(space, &gstring("outbox2")).await, vec![
            vec![gint(6)]
        ]);
        // Without a use block, the same program writes to the host.
        session
            .run(
                &with_space(r#"new h in { for (@v <- h) { @"outbox3"!(v) } | h!(7) }"#),
                "rrs-p4-closure-control",
            )
            .await;
        assert_eq!(session.values("outbox3").await, vec![vec![gint(7)]]);
        assert!(session.records(space, &gstring("outbox3")).await.is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reducer_space_routing_replies_route_r7() {
        let mut session = Session::new().await;
        let (space, a, b) = session.setup().await;
        // System processes reply on the raw return channel. A space name routes the reply into
        // its space (R7).
        session
            .run(
                &with_space(
                    r#"
new spaceAgent(`rho:space:getSpaceAgent`), equal(`rho:assert:assertEqual`) in {
  spaceAgent!(*a, *queue) | equal!(*b, 1, 1) | queue!(*a, "qualifier")
}
"#,
                ),
                "rrs-p4-r7",
            )
            .await;
        let mut replies = session.records(space, &a).await;
        replies.sort_by_key(|pars| format!("{pars:?}"));
        let capability = factory_capability(SpaceFactoryUrn::HASHMAP_QUEUE)
            .expect("the family has a capability");
        let mut expected = vec![vec![capability], vec![gstring("default")]];
        expected.sort_by_key(|pars| format!("{pars:?}"));
        assert_eq!(
            replies, expected,
            "both replies are records of the space name"
        );
        assert_eq!(session.records(space, &b).await, vec![vec![gbool(true)]]);
        // R7 ignores the use context: a forged return channel is a host channel even in a use
        // block (A18).
        session
            .run(
                &with_space(
                    r#"new equal(`rho:assert:assertEqual`) in { use queue { equal!("rrs-r7-forged", 2, 2) } }"#,
                ),
                "rrs-p4-r7-forged",
            )
            .await;
        assert_eq!(session.values("rrs-r7-forged").await, vec![vec![gbool(
            true
        )]]);
        assert!(session
            .records(space, &gstring("rrs-r7-forged"))
            .await
            .is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reducer_space_routing_survives_reset() {
        let mut session = Session::new().await;
        let empty = session.play.get_root().await;
        let (space, a, _) = session.setup().await;
        session
            .run(&with_space("a!(1)"), "rrs-p4-reset-enqueue")
            .await;
        let root = session.play.create_checkpoint().await.root;
        // The runtime holds no space state of its own: after a reset to the root, routing reads
        // the trie.
        session.play.reset(&root).await.expect("reset to the root");
        session
            .run(&with_space("a!(2)"), "rrs-p4-reset-after")
            .await;
        assert_eq!(session.records(space, &a).await, vec![vec![gint(1)], vec![
            gint(2)
        ]]);
        // Another runtime over the store routes the same way at the root.
        session
            .replay
            .reset(&root)
            .await
            .expect("reset the replay runtime to the root");
        assert_eq!(
            session
                .replay
                .get_data(&data_channel(space, &a))
                .await
                .len(),
            1,
            "the replay runtime sees the record at the root"
        );
        // Before the space existed, the same name fails closed.
        session
            .play
            .reset(&empty)
            .await
            .expect("reset to the empty root");
        let injections = Injections::from([(injected("name"), a.clone())]);
        session
            .fail_with(
                &format!("new name(`{}`) in {{ name!(3) }}", injected("name")),
                "rrs-p4-reset-dangling",
                &injections,
                &["DanglingName"],
            )
            .await;
        session
            .play
            .reset(&root)
            .await
            .expect("reset to the root again");
        assert_eq!(session.records(space, &a).await, vec![vec![gint(1)]]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reducer_space_routing_agent_gate() {
        let mut session = Session::new().await;
        session.setup().await;
        let injections = Injections::from([(
            injected("tag"),
            namespace::gprivate(vec![REIFIED_TAG, AGENT_TAG]),
        )]);
        for (label, body) in [
            ("use-string", r#"use @"user agent" { @"x"!(1) }"#),
            ("use-list", "use @[1, 2] { Nil }"),
            ("use-plain", "new u in { use u { Nil } }"),
            ("annotation", r#"new x : @"user agent" in { x!(1) }"#),
            ("tagged-send", "@(*tag, 1, 2)!(3)"),
            ("tagged-receive", "for (@v <- @(*tag, 1, 2)) { Nil }"),
            (
                "tagged-join",
                r#"for (@v <- @(*tag, 1, 2) & @w <- @"host") { Nil }"#,
            ),
        ] {
            session
                .fail_with(
                    &format!(
                        "new tag(`{}`) in {{ {} }}",
                        injected("tag"),
                        with_space(body)
                    ),
                    &format!("rrs-p4-agent-{label}"),
                    &injections,
                    &["AgentSpacesUnavailable"],
                )
                .await;
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reducer_space_routing_ordinary_unchanged() {
        // An ordinary program logs the same events and costs the same, with or without spaces
        // in the store.
        let ordinary = r#"
new a, b, c, out(`rho:io:stdout`), tag(`rho:system:integerAddMergeableTag`) in {
  a!(1) | a!!(2) | b!(3, "x") | @"rrs-public"!([1, 2], {"k": 3}) |
  for (@x <- a & @y <- b) { c!(x) } |
  for (@z <<- @"rrs-public") { out!(z) } |
  for (@w <= c) { Nil } |
  @(*tag, "balance")!(10) |
  contract @"rrs-contract"(ret, @n) = { ret!(n + 1) } |
  for (@r <- @"rrs-contract"!?(41)) { @"rrs-result"!(r) }
}
"#;
        let mut without = Session::new().await;
        let plain = without.run(ordinary, "rrs-p4-ordinary").await;
        let mut with = Session::new().await;
        with.setup().await;
        with.run(
            &with_space("a!(1) | for (@x <- b) { Nil }"),
            "rrs-p4-ordinary-spaces",
        )
        .await;
        with.play.create_checkpoint().await;
        let spaced = with.run(ordinary, "rrs-p4-ordinary").await;
        assert_eq!(plain.log, spaced.log, "the ordinary events do not change");
        assert_eq!(plain.result.cost.value, spaced.result.cost.value);
        assert_eq!(plain.result.mergeable, spaced.result.mergeable);
        assert_eq!(with.sole("rrs-result").await, gint(42));
        assert!(!plain.log.is_empty(), "the ordinary program logs events");
        assert!(
            spaced.residue.is_empty(),
            "the ordinary program writes no space data"
        );
    }
}

mod reducer_handle_unwrap {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reducer_handle_unwrap_write_flag() {
        let mut session = Session::new().await;
        let (space, _, _) = session.setup().await;
        for (label, body) in [
            ("read-only", r#"use @{bundle-{*queue}} { @"x"!(1) }"#),
            ("no-flags", r#"use @{bundle0{*queue}} { @"x"!(1) }"#),
            ("annotation", r#"new x : @{bundle-{*queue}} in { x!(1) }"#),
        ] {
            session
                .fail_with(
                    &with_space(body),
                    &format!("rrs-p4-write-flag-{label}"),
                    &Injections::new(),
                    &["HandleNotWritable"],
                )
                .await;
        }
        assert!(session.records(space, &gstring("x")).await.is_empty());
        // A bundle with the write flag is the handle.
        session
            .run(
                &with_space(
                    r#"use @{bundle+{*queue}} { @"x"!(1) } | use @{bundle{*queue}} { @"x"!(2) }"#,
                ),
                "rrs-p4-write-flag-writable",
            )
            .await;
        assert_eq!(session.records(space, &gstring("x")).await.len(), 2);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reducer_handle_unwrap_not_a_space_ref() {
        let mut session = Session::new().await;
        let (space, _, _) = session.setup().await;
        let injections = Injections::from([(
            injected("config"),
            namespace::gprivate(namespace::config_channel(space)),
        )]);
        for (label, body) in [
            ("space-name", r#"use a { @"x"!(1) }"#),
            (
                "factory",
                r#"new factory(`rho:space:hashmap:queue`) in { use factory { @"x"!(1) } }"#,
            ),
            ("config", r#"use config { @"x"!(1) }"#),
            ("annotation", "new x : b in { x!(1) }"),
        ] {
            session
                .fail_with(
                    &format!(
                        "new config(`{}`) in {{ {} }}",
                        injected("config"),
                        with_space(body)
                    ),
                    &format!("rrs-p4-not-a-ref-{label}"),
                    &injections,
                    &["NotASpaceRef"],
                )
                .await;
        }
        assert_eq!(
            classify_space_ref(&namespace::gprivate(namespace::config_channel(space))),
            Err(rholang::rust::interpreter::reified::scope::SpaceRefError::NotASpaceRef)
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reducer_handle_unwrap_empty_space_ref() {
        let mut session = Session::new().await;
        session.setup().await;
        for (label, body) in [
            ("use", r#"use @Nil { @"x"!(1) }"#),
            ("annotation", "new x : @Nil in { x!(1) }"),
            ("annotation-second", "new y, x : @Nil in { x!(1) }"),
        ] {
            session
                .fail_with(
                    &with_space(body),
                    &format!("rrs-p4-empty-ref-{label}"),
                    &Injections::new(),
                    &["EmptySpaceRef"],
                )
                .await;
        }
        assert!(session.values("x").await.is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reducer_handle_unwrap_agent_classification() {
        let mut session = Session::new().await;
        session.setup().await;
        // A value that is not a Reified id is a space of a user agent: P8 implements it, so P4
        // fails closed. getSpaceAgent classifies the same values the same way.
        let references = [
            ("string", r#"@"user agent""#, r#""user agent""#),
            ("list", "@[1]", "[1]"),
            ("par", "@{1 | 2}", "1 | 2"),
            ("plain", "plain", "*plain"),
        ];
        for (label, name, _) in references {
            session
                .fail_with(
                    &with_space(&format!("new plain in {{ use {name} {{ @\"x\"!(1) }} }}")),
                    &format!("rrs-p4-agent-class-{label}"),
                    &Injections::new(),
                    &["AgentSpacesUnavailable"],
                )
                .await;
        }
        let mut body = String::from("new plain, spaceAgent(`rho:space:getSpaceAgent`) in {\n");
        for (label, _, value) in references {
            body.push_str(&format!(
                "for (@r <- spaceAgent!?({value})) {{ @\"rrs-class-{label}\"!(r) }} |\n"
            ));
        }
        body.push_str("Nil\n}");
        session
            .run(&with_space(&body), "rrs-p4-agent-class-reply")
            .await;
        for (label, _, _) in references {
            let reply = session.sole(&format!("rrs-class-{label}")).await;
            assert_eq!(
                reply,
                rholang::rust::interpreter::reified::handle::typed_error(
                    rholang::rust::interpreter::spaces::errors::SpaceReplyCode::NotANativeSpace,
                    Vec::new()
                ),
                "{label}"
            );
        }
        assert_eq!(
            classify_space_ref(&gstring("user agent")),
            Ok(SpaceRef::Agent(gstring("user agent")))
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reducer_handle_unwrap_native_handle() {
        let mut session = Session::new().await;
        let (space, _, _) = session.setup().await;
        let injections = Injections::from([(
            injected("bare"),
            namespace::gprivate(namespace::handle_channel(space)),
        )]);
        // The capability, a rebundled capability and the bare handle all name one space.
        session
            .run_with(
                &format!(
                    "new bare(`{}`) in {{ {} }}",
                    injected("bare"),
                    with_space(
                        r#"
use queue { @"x"!(1) } |
use @{bundle+{*queue}} { @"x"!(2) } |
use bare { @"x"!(3) } |
new n : bare in { n!(4) | @"rrs-n"!(*n) }
"#
                    )
                ),
                "rrs-p4-native-handle",
                &injections,
            )
            .await;
        let mut values: Vec<Vec<Par>> = session.records(space, &gstring("x")).await;
        values.sort_by_key(|pars| format!("{pars:?}"));
        assert_eq!(values, vec![vec![gint(1)], vec![gint(2)], vec![gint(3)]]);
        let name = session.sole("rrs-n").await;
        assert_eq!(classify_channel(&name), ChannelClass::SpaceName(space));
        assert_eq!(session.records(space, &name).await, vec![vec![gint(4)]]);
        assert_eq!(
            classify_space_ref(&namespace::gprivate(namespace::handle_channel(space))),
            Ok(SpaceRef::Native(space))
        );
        // An id of another kind with the same space id is no handle.
        let mut id = namespace::handle_channel(space);
        id[1] = Kind::Body as u8;
        assert_eq!(
            classify_space_ref(&namespace::gprivate(id)),
            Err(rholang::rust::interpreter::reified::scope::SpaceRefError::NotASpaceRef)
        );
    }

    /// P8 step 1 (design v2 AG-4): a call on a user agent is an ordinary send, so the reference
    /// of an agent follows the bundle rule of `eval_send`. `use` and an annotation of `new`
    /// reject a reference under a bundle without the write flag with `HandleNotWritable`, the
    /// code of P4 that P8 generalizes. A reference with a call channel passes the rule. Its
    /// `use` block and its annotation then fail closed with `AgentSpacesUnavailable` until the
    /// translation of `new` lands (P8 step 4).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reducer_agent_reference_write_rule() {
        let mut session = Session::new().await;
        session.setup().await;
        for (label, reference, code) in [
            (
                "read-only",
                r#"@{bundle-{"user agent"}}"#,
                "HandleNotWritable",
            ),
            (
                "no-flags",
                r#"@{bundle0{"user agent"}}"#,
                "HandleNotWritable",
            ),
            (
                "write-only",
                r#"@{bundle+{"user agent"}}"#,
                "AgentSpacesUnavailable",
            ),
            (
                "read-write",
                r#"@{bundle{"user agent"}}"#,
                "AgentSpacesUnavailable",
            ),
            ("plain", r#"@"user agent""#, "AgentSpacesUnavailable"),
        ] {
            for (form, body) in [
                ("use", format!("use {reference} {{ Nil }}")),
                ("annotation", format!("new x : {reference} in {{ Nil }}")),
            ] {
                session
                    .fail_with(
                        &with_space(&body),
                        &format!("rrs-p8-agent-write-{label}-{form}"),
                        &Injections::new(),
                        &[code],
                    )
                    .await;
            }
        }
    }
}
