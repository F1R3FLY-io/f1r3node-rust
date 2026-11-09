//! Added by decision 127 (kind registry): a kind that only this test registers plugs into
//! f1r3node with trait implementations and one registration: a new outer store (`plugmap`), a
//! new discipline (`plugqueue`) with its own param (`depth`), a recorded system process and an
//! installer of its services.
//!
//! Changed by decision 128 (kind routing): the store takes the routed layout of `OuterStore`, so
//! it declares only its name and its qualifiers.

#[path = "support/mod.rs"]
mod support;

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

use linkme::distributed_slice;
use models::rhoapi::{ListParWithRandom, Par};
use models::rust::space_factory_urn::{SpaceFactoryUrn, SpaceInner, SpaceOuter, SpaceQualifier};
use models::rust::utils::{new_gbool_par, new_gint_par};
use rholang::rust::interpreter::contract_call::ContractCall;
use rholang::rust::interpreter::errors::{illegal_argument_error, InterpreterError};
use rholang::rust::interpreter::reified::errors::ReifiedReduceError;
use rholang::rust::interpreter::reified::runtime::{unavailable, ReifiedRuntime};
use rholang::rust::interpreter::reified::system_channels::{
    check_persistent_send, is_call_op, is_factory_op, is_recorded_process, packs_arguments,
    SystemChannel,
};
use rholang::rust::interpreter::reified::system_processes::{definitions, SpaceProcess};
use rholang::rust::interpreter::reified::vectordb_services::{uninstall, EmbedFailure};
use rholang::rust::interpreter::spaces::disciplines::queue::QueueDiscipline;
use rholang::rust::interpreter::spaces::errors::SpaceError;
use rholang::rust::interpreter::spaces::factory::defaults::{ParamCheck, ParamRule};
use rholang::rust::interpreter::spaces::families::{
    check_space_kinds, install_space_services, Family, SpaceKind, SPACE_KINDS,
};
use rholang::rust::interpreter::spaces::generic::{
    Discipline, GenericSpace, Ordered, OuterStore, Side,
};
use rholang::rust::interpreter::spaces::outer::hashmap::HashMapOuter;
use rholang::rust::interpreter::system_processes::{
    is_native_recorded_op, Definition, ProcessContext,
};
use serde::Serialize;
use support::session::{Injections, Session};

/// The test store: the routed layout (the defaults of `OuterStore`), with the `default`
/// qualifier only.
#[derive(Clone, Copy, Debug, Default)]
struct PlugMap;

impl OuterStore for PlugMap {
    const NAME: &'static str = "plugmap";
    const QUALIFIERS: &'static [SpaceQualifier] = &[SpaceQualifier::Default];
}

/// The test discipline: the queue, with the constructor param `depth`.
#[derive(Clone, Copy, Debug, Default)]
struct PlugQueue;

impl Discipline for PlugQueue {
    const NAME: &'static str = "plugqueue";
    const USES_COUNTER: bool = QueueDiscipline::USES_COUNTER;
    const SEND_PARAMS: &'static [&'static str] = QueueDiscipline::SEND_PARAMS;

    fn param_rule(key: &str) -> Option<ParamCheck> {
        match key {
            "depth" => Some(depth_rule),
            _ => None,
        }
    }

    fn head<T: Serialize>(
        &self,
        side: Side,
        entries: &[Ordered<'_, T>],
    ) -> Result<Option<usize>, SpaceError> {
        QueueDiscipline.head(side, entries)
    }

    fn reaches_head(&self, side: Side, queued: usize) -> bool {
        QueueDiscipline.reaches_head(side, queued)
    }

    fn precedes(&self, side: Side, ord: i64, head: i64) -> Result<bool, SpaceError> {
        QueueDiscipline.precedes(side, ord, head)
    }
}

/// The rule of `depth`: the rule of `size` (F7).
fn depth_rule(value: &Par) -> Option<Par> { ParamRule::Size.canonical(value) }

const PLUGMAP_QUEUE_FACTORY: SystemChannel = SystemChannel {
    urn: "rho:space:plugmap:queue",
    op: 0x40,
    arity: 2,
};
const PLUG_ECHO: SystemChannel = SystemChannel {
    urn: "rho:plug:echo",
    op: 0x41,
    arity: 2,
};
const HASHMAP_PLUGQUEUE_FACTORY: SystemChannel = SystemChannel {
    urn: "rho:space:hashmap:plugqueue",
    op: 0x42,
    arity: 2,
};

static PLUGMAP_QUEUE: GenericSpace<PlugMap, QueueDiscipline> =
    GenericSpace::new(PlugMap, QueueDiscipline);
static HASHMAP_PLUGQUEUE: GenericSpace<HashMapOuter, PlugQueue> =
    GenericSpace::new(HashMapOuter, PlugQueue);

/// 0 before an install, 1 after an observer install, 2 after a validator install.
static INSTALLED: AtomicU8 = AtomicU8::new(0);

fn install_plug_services(is_validator: bool) {
    INSTALLED.store(1 + u8::from(is_validator), Ordering::SeqCst);
}

/// The definition of `rho:plug:echo`, a recorded process, in the form of the reified handlers
/// (`definition_with_remainder` of `reified::system_processes`). The DR-114 reply helpers are
/// pub(crate), so the test replies with `produce` directly.
fn plug_echo(_runtime: &Arc<ReifiedRuntime>) -> Definition {
    Definition {
        urn: PLUG_ECHO.urn.to_string(),
        fixed_channel: PLUG_ECHO.channel(),
        arity: PLUG_ECHO.arity,
        body_ref: PLUG_ECHO.body_ref(),
        handler: Box::new(|context: ProcessContext| {
            let space = context.space;
            let dispatcher = context.dispatcher;
            Box::new(move |contract_args| {
                let contract = ContractCall {
                    space: space.clone(),
                    dispatcher: dispatcher.clone(),
                };
                Box::pin(async move { echo(&contract, contract_args).await })
            })
        }),
        remainder: None,
    }
}

/// `echo!(ret, value)`: play replies `[value]`, and replay replies the recorded output.
async fn echo(
    contract: &ContractCall,
    contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
) -> Result<Vec<Par>, InterpreterError> {
    let Some((produce, is_replay, previous_output, args)) = contract.unapply(contract_args) else {
        return Err(illegal_argument_error(PLUG_ECHO.urn));
    };
    let [ret, value] = args.as_slice() else {
        return Err(illegal_argument_error(PLUG_ECHO.urn));
    };
    let reply = match is_replay {
        true => previous_output,
        false => vec![value.clone()],
    };
    produce(&reply, ret).await?;
    Ok(reply)
}

#[distributed_slice(SPACE_KINDS)]
static PLUG_KIND: SpaceKind = SpaceKind {
    families: &[
        Family::new(&PLUGMAP_QUEUE, PLUGMAP_QUEUE_FACTORY, &[], &[]),
        Family::new(
            &HASHMAP_PLUGQUEUE,
            HASHMAP_PLUGQUEUE_FACTORY,
            &["depth"],
            &[],
        ),
    ],
    processes: &[SpaceProcess {
        channel: PLUG_ECHO,
        recorded: true,
        definition: plug_echo,
    }],
    services: Some(install_plug_services),
};

#[test]
fn a_kind_registered_in_a_test_plugs_into_the_registry() {
    assert_eq!(SPACE_KINDS.len(), 2);
    assert_eq!(check_space_kinds(), Ok(()));
    let families: Vec<Family> = Family::registered().copied().collect();
    let urns: BTreeSet<SpaceFactoryUrn> = families.iter().map(|family| family.urn()).collect();
    let ops: BTreeSet<u8> = families.iter().map(|family| family.factory().op).collect();
    let bodies: BTreeSet<i64> = families
        .iter()
        .map(|family| family.factory().body_ref())
        .chain([PLUG_ECHO.body_ref()])
        .collect();
    assert_eq!(
        (families.len(), urns.len(), ops.len(), bodies.len()),
        (34, 34, 34, 35)
    );
    for family in &families {
        assert_eq!(family.factory().urn, family.urn().to_string(), "{family:?}");
    }
    let plugmap = SpaceFactoryUrn {
        outer: SpaceOuter::named("plugmap"),
        inner: SpaceInner::Queue,
    };
    let plugqueue = SpaceFactoryUrn {
        outer: SpaceOuter::HashMap,
        inner: SpaceInner::named("plugqueue"),
    };
    assert_eq!(
        Family::of_urn(plugmap).map(Family::factory),
        Some(PLUGMAP_QUEUE_FACTORY)
    );
    assert_eq!(
        Family::of_urn(plugqueue).map(Family::factory),
        Some(HASHMAP_PLUGQUEUE_FACTORY)
    );
    for factory in [PLUGMAP_QUEUE_FACTORY, HASHMAP_PLUGQUEUE_FACTORY] {
        assert!(is_factory_op(factory.op) && is_call_op(factory.op) && packs_arguments(factory.op));
        assert_eq!(
            check_persistent_send(&factory.channel(), true),
            Err(ReifiedReduceError::PersistentFactoryCall)
        );
    }
    assert!(!is_factory_op(PLUG_ECHO.op) && !packs_arguments(PLUG_ECHO.op));
    assert!(is_call_op(PLUG_ECHO.op));
    assert!(is_recorded_process(PLUG_ECHO.body_ref()));
    assert!(is_native_recorded_op(PLUG_ECHO.body_ref()));
    assert_eq!(check_persistent_send(&PLUG_ECHO.channel(), true), Ok(()));
    let defined = definitions(&unavailable());
    let tail: Vec<(&str, i64)> = defined[defined.len() - 3..]
        .iter()
        .map(|definition| (definition.urn.as_str(), definition.body_ref))
        .collect();
    assert_eq!(tail, [
        (PLUGMAP_QUEUE_FACTORY.urn, PLUGMAP_QUEUE_FACTORY.body_ref()),
        (
            HASHMAP_PLUGQUEUE_FACTORY.urn,
            HASHMAP_PLUGQUEUE_FACTORY.body_ref()
        ),
        (PLUG_ECHO.urn, PLUG_ECHO.body_ref()),
    ]);
}

const PROGRAM: &str = r#"new plugmap(`rho:space:plugmap:queue`), plugqueue(`rho:space:hashmap:plugqueue`),
  echo(`rho:plug:echo`), gsa(`rho:space:getSpaceAgent`), equal(`rho:assert:assertEqual`) in {
  for (@e <- echo!?(9)) { @"rrs-127-echo"!(e) } |
  for (m <- plugmap!?("default")) {
    for (@capability <- gsa!?(*m)) {
      for (@same <- equal!?(capability, *plugmap)) { @"rrs-127-agent-plugmap"!(same) }
    } |
    for (a <- m!?("gensym")) { a!(7) | for (@x <- a) { @"rrs-127-plugmap"!(x) } }
  } |
  for (q <- plugqueue!?("default"; depth=2)) {
    for (@capability <- gsa!?(*q)) {
      for (@same <- equal!?(capability, *plugqueue)) { @"rrs-127-agent-plugqueue"!(same) }
    } |
    for (b <- q!?("gensym")) { b!(8) | for (@y <- b) { @"rrs-127-plugqueue"!(y) } }
  }
}"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_kind_registered_in_a_test_runs_its_spaces() {
    let mut session = Session::new().await;
    session.run(PROGRAM, "rrs-127 plug-in kind").await;
    assert_eq!(
        session.sole("rrs-127-echo").await,
        new_gint_par(9, Vec::new(), false)
    );
    assert_eq!(
        session.sole("rrs-127-plugmap").await,
        new_gint_par(7, Vec::new(), false)
    );
    assert_eq!(
        session.sole("rrs-127-plugqueue").await,
        new_gint_par(8, Vec::new(), false)
    );
    for channel in ["rrs-127-agent-plugmap", "rrs-127-agent-plugqueue"] {
        assert_eq!(
            session.sole(channel).await,
            new_gbool_par(true, Vec::new(), false),
            "{channel}"
        );
    }
    session
        .fail_with(
            "new plugmap(`rho:space:plugmap:queue`) in { for (m <- plugmap!?(\"temp\")) { Nil } }",
            "rrs-127 plug-in qualifier",
            &Injections::new(),
            &["BadConstructorArgument"],
        )
        .await;
    session
        .fail_with(
            "new p(`rho:space:hashmap:plugqueue`) in { for (q <- p!?(\"default\"; depth=0)) { Nil } }",
            "rrs-127 plug-in param",
            &Injections::new(),
            &["InvalidParam"],
        )
        .await;
}

/// Added by decision 127 (kind registry, ruling D7): the node install runs the installer of
/// each kind. An observer installs the unavailable VectorDB services. No VectorDB test runs in
/// this binary, so no other test writes the services.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_kind_registered_in_a_test_installs_its_services() {
    install_space_services(false);
    assert_eq!(INSTALLED.load(Ordering::SeqCst), 1);
    let installed = uninstall().expect("the VectorDB kind installs its services");
    assert!(installed.writer.is_none());
    assert_eq!(
        installed.embedder.embed(&[String::from("plug")]).await,
        Err(EmbedFailure::Unavailable)
    );
}
