//! The PathMap spaces of the native spaces (P5e, Phase 2 of wave W3; P5 design v2 3.4 with the
//! changes of v3 A.12; joins plan section 9; decisions 45, 50 and 51).
//!
//! The family agent dispatches each operation of a `pathmap:<inner>` space to the agent
//! `GenericSpace<PathMapOuter, D>`, which runs it on the join engine through the prefix source
//! (`spaces::pathmap::join`). The tests drive the family agent over an in-memory play space and
//! its replay space. A path `p` is a list of elements. A send on `p` stores its record on
//! `D(p)` and one marker on the index channel `I(q)` of each prefix `q ⪯ p`. A receive on `q`
//! parks on `I(q)`.
//!
//! | Test | What it shows |
//! |---|---|
//! | `pathmap_fips_example_reroot` | the FIPS example: re-rooted PathMaps, the list rule and the unit rule |
//! | `pathmap_non_pathmap_payload_list` | a value that is not a PathMap arrives as `[suffix…, value]`, one list for each argument |
//! | `pathmap_empty_unit_rule` | `{||}` under a suffix that is not empty is `{|suffix|}`, and `{||}` on its own path stays empty |
//! | `pathmap_most_specific_waiter_first` | a send meets the waiters of its path first, then of each shorter prefix |
//! | `pathmap_queue_head_blocks` | the head waiter of a prefix blocks its later waiters, and a refusal moves to the next prefix |
//! | `pathmap_peek_restores_marker` | Rule E: a peek at each depth restores the marker with its bytes and its produce identity, for each authority |
//! | `pathmap_consume_removes_all_markers` | a receive takes the record and every marker of the datum |
//! | `pathmap_merge_prefix_contact_conflicts` | merge check #2 sees a datum and a waiter on a prefix of its path on the shared index channel |
//! | `pathmap_epathmap_element_rejected` | a PathMap or a zipper in a path aborts with `UnsupportedPathElement` and stages nothing |
//! | `pathmap_legacy_gint_suffix_absent` | the suffix arrives as its elements, never as the bytes of a key |
//! | `pathmap_segment_codec_matches_epm1_vectors` | the data and index channels carry the EPM1 keys of the paths |
//! | `pathmap_path_too_deep_aborts` | a path of 65 elements aborts with `PathTooDeep`, and 64 elements are accepted |
//! | `pathmap_gensym_roots_private_subtree` | a name of the space is the path `[n]`, and only its holder reaches the paths under it |
//! | `pathmap_channel_equals_singleton_list` | `@c` and `@[c]` are one path |
//! | `pathmap_marker_vehicle_comm_shape` | every COMM is single-channel, and the vehicle is never stored |
//! | `pathmap_token_bumped_every_state_change` | the token rule of decisions 31 and 51 |
//! | `pathmap_vehicle_introduction_covers_payload` | the vehicle is the marker followed by the payload (decision 40, Q3) |
//! | `pathmap_stack_settle` | a stack space over prefixes: tops only, and the cascade after a pop |
//! | `pathmap_pq_levels` | a priority-queue space over prefixes: the level order and the lanes |
//! | `pathmap_set_class_retired` | a set space retires the other members of a class on its path |
//! | `pathmap_cell_occupied` | a cell space holds one record on each path and one waiter on each prefix |
//! | `pathmap_joins_over_prefixes` | a join over prefixes of one space, on both sides, with the window of each path |
//! | `pathmap_join_window_takes_need_entries_of_each_path` | the window of `need` binds on nested prefixes (open point 4) |
//! | `pathmap_cap_restores_owed_markers` | a transaction at the round cap restores each owed marker with its bytes, and the settle job carries the offer (O-2) |
//! | `pathmap_cap_drops_a_stale_carried_offer` | a carried offer whose datum another job spent stores nothing (O-2) |
//! | `pathmap_cap_reissues_a_persistent_send_at_its_place` | decision 43 at the cap on a queue and a stack: the resumed send keeps its first order |
//! | `pathmap_replay_equal` | replay reproduces play: the results, the legacy logs, the costs and the root |
//! | `pathmap_restart` | the records, the markers and the waiters survive a checkpoint and a reset |
//! | `pathmap_legacy_charges_equal_host` | Option P: each staged operation charges what `ChargingRSpace` charges for its host call |
//! | `pathmap_staging_stays_in_the_lock_keys` | one escalation, and every staged channel lies in the lock keys of the prefix scope |
//!
//! | `reified_pathmapspace_alias_equal` | the FIPS alias `PathMapSpace` names `pathmap:bag`, and a program in `use space` runs the FIPS example through the reducer |
//! | `pathmap_rholang_names_root_private_subtrees` | a name of the space roots a private subtree through the reducer, and `@c ≡ @[c]` |
//!
//! The properties of the invariant map (P5 rows 114 to 119) follow the examples.

#[path = "support/mod.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::cost_signature::Value as CostSignatureValue;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{
    BindPattern, CostAuthority, CostRegion, CostSignature, EPathMap, EZipper, Expr,
    ListParWithRandom, Par, TaggedContinuation,
};
use models::rust::space_factory_urn::{SpaceFactoryUrn, SpaceInner, SpaceOuter};
use models::rust::utils::{new_elist_par, new_freevar_par, new_gbool_par};
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::RuntimeBudget;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::reified::costs::{ReissueCause, FIRE_ROUND_CAP};
use rholang::rust::interpreter::reified::legacy_charges::{
    consume_charges, produce_charges, ParSpaceConsume, ParSpaceProduce,
};
use rholang::rust::interpreter::rho_runtime::RhoISpace;
use rholang::rust::interpreter::spaces::agent::{
    ConsumeOp, JoinOp, ProduceOp, QueryParams, SpaceEnv, SpaceOp, SpaceOutcome,
};
use rholang::rust::interpreter::spaces::disciplines::priority_queue::PRIORITY_PARAM;
use rholang::rust::interpreter::spaces::errors::SpaceError;
use rholang::rust::interpreter::spaces::families::FamilyAgent;
use rholang::rust::interpreter::spaces::namespace::{
    self, Durability, NameTail, NativeSpace, SID_LEN,
};
use rholang::rust::interpreter::spaces::outer::pathmap::PathMapOuter;
use rholang::rust::interpreter::spaces::pathmap::markers::{
    decode_marker, decode_vehicle, marker_random, marker_record,
};
use rholang::rust::interpreter::spaces::pathmap::{encode_path, path_key, PATH_DEPTH_CAP};
use rholang::rust::interpreter::spaces::record;
use rholang::rust::interpreter::spaces::types::config::SpaceConfig;
use rholang::rust::interpreter::spaces::types::qualifier::SpaceQualifier;
use rholang::rust::interpreter::storage::charging_rspace::ChargingRSpace;
use rspace_plus_plus::rspace::checkpoint::Checkpoint;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::hash;
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex;
use rspace_plus_plus::rspace::merger::merging_logic::{
    conflict_reason, conflicts, NumberChannelsDiff,
};
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::space_store_handle::{ConsumeOutcome, ProduceOutcome};
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent, Produce};
use support::agent::{
    any, budget, config_of, create, creation, env_over, exactly, fired, int, join_on, play,
    play_and_replay, receive_on, run, send_on, string, HostRecord, ParRSpace, RecordingHost,
    StagedCopy,
};
use support::legacy::{host_consume, host_produce, split};
use support::props::{check, pure_config, tokio_runtime};
use support::session::{native, with_space, Session};

// ─── The rig ─────────────────────────────────────────────────────────────────────────────────

/// The space of every rig.
const SPACE: NativeSpace = NativeSpace {
    sid: [57; SID_LEN],
    durability: Durability::Durable,
};

/// The family `pathmap:<inner>`.
fn family(inner: SpaceInner) -> SpaceFactoryUrn {
    SpaceFactoryUrn {
        outer: SpaceOuter::PathMap,
        inner,
    }
}

/// The plain list `[ps…]`.
fn list(ps: Vec<Par>) -> Par { new_elist_par(ps, Vec::new(), false, None, Vec::new(), false) }

/// The elements of the path of the numbers `values`.
fn elements(values: &[i64]) -> Vec<Par> { values.iter().map(|value| int(*value)).collect() }

/// The channel of the path of the numbers `values`: the plain list of them.
fn path(values: &[i64]) -> Par { list(elements(values)) }

/// The list of the numbers `values`.
fn ints(values: &[i64]) -> Par { list(elements(values)) }

/// The PathMap value `{| ps |}`.
fn pathmap(ps: Vec<Par>) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EPathmapBody(EPathMap {
            ps,
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        })),
    }])
}

/// The data channel `D(sid, path_key(p))` of the path with the elements `path`.
fn data_at(path: &[Par]) -> Par {
    let key = path_key(path).expect("a path key");
    PathMapOuter::path_data(SPACE, &key)
}

/// The index channel `I(sid, prefix_key(q))` of the prefix with the elements `prefix`.
fn index_at(prefix: &[Par]) -> Par {
    let encoded = encode_path(prefix).expect("a path");
    let key = encoded.prefix_key(encoded.depth()).expect("a prefix key");
    PathMapOuter::index(SPACE, key)
}

/// The token `S(sid)`.
fn token() -> Par { PathMapOuter::token(SPACE) }

/// The random state of a send of `value` on `channel` with the salt `salt`.
fn send_random(channel: &Par, value: &Par, salt: u64) -> Vec<u8> {
    Blake2b512Random::create_from_bytes(
        format!("rrs-pathmap-send {channel:?} {value:?} {salt}").as_bytes(),
    )
    .to_bytes()
}

/// A send of the values `values` on `channel` with the salt `salt`.
fn send_values(channel: &Par, values: Vec<Par>, salt: u64) -> ProduceOp {
    let mut op = send_on(SPACE, channel, 0, false);
    op.data = ListParWithRandom {
        random_state: send_random(channel, &list(values.clone()), salt),
        pars: values,
        cost_authority: None,
        cost_stack: None,
    };
    op
}

/// A send of `value` on the path of the numbers `at`.
fn put(at: &[i64], value: i64) -> SpaceOp { put_salted(at, value, 0) }

/// A send of `value` on the path of the numbers `at` with the salt `salt`.
fn put_salted(at: &[i64], value: i64, salt: u64) -> SpaceOp {
    SpaceOp::Produce(send_values(&path(at), vec![int(value)], salt))
}

/// A persistent send (`!!`) of `value` on the path of the numbers `at`.
fn put_forever(at: &[i64], value: i64) -> SpaceOp {
    SpaceOp::Produce(ProduceOp {
        persistent: true,
        ..send_values(&path(at), vec![int(value)], 1_000_000)
    })
}

/// A send of the payload `payload` on `channel`.
fn put_on(channel: &Par, payload: Par) -> SpaceOp {
    SpaceOp::Produce(send_values(channel, vec![payload], 0))
}

/// The named params of a send or of a query with the level `level` of a priority queue.
fn priority(level: i64) -> BTreeMap<String, Par> {
    BTreeMap::from([(PRIORITY_PARAM.to_string(), int(level))])
}

/// A send of `value` on the path of the numbers `at` of a priority queue at the level `level`.
fn put_level(at: &[i64], value: i64, level: Option<i64>) -> SpaceOp {
    SpaceOp::Produce(ProduceOp {
        params: level.map(priority).unwrap_or_default(),
        ..send_values(&path(at), vec![int(value)], 7)
    })
}

/// A receive on `channel` whose body is `name`.
fn receive(channel: &Par, name: &str, pattern: BindPattern) -> ConsumeOp {
    receive_on(SPACE, channel, name, pattern)
}

/// A receive on the prefix of the numbers `at` whose body is `name`.
fn take(at: &[i64], name: &str, pattern: BindPattern) -> SpaceOp {
    SpaceOp::Consume(receive(&path(at), name, pattern))
}

/// A peek (`<<-`) on the prefix of the numbers `at` whose body is `name`.
fn peek(at: &[i64], name: &str, pattern: BindPattern) -> SpaceOp {
    SpaceOp::Consume(ConsumeOp {
        peek: true,
        ..receive(&path(at), name, pattern)
    })
}

/// A persistent receive (`<=`) on the prefix of the numbers `at` whose body is `name`.
fn take_forever(at: &[i64], name: &str, pattern: BindPattern) -> SpaceOp {
    SpaceOp::Consume(ConsumeOp {
        persistent: true,
        ..receive(&path(at), name, pattern)
    })
}

/// A receive on the prefix of the numbers `at` of a priority queue in the lane `lane`.
fn take_in(at: &[i64], name: &str, pattern: BindPattern, lane: Option<i64>) -> SpaceOp {
    SpaceOp::Consume(ConsumeOp {
        query: lane.map(|level| QueryParams {
            payload: Vec::new(),
            named: priority(level),
        }),
        ..receive(&path(at), name, pattern)
    })
}

/// A join over the prefixes of the numbers of each bind, in bind order, whose body is `name`.
fn join(binds: &[(&[i64], BindPattern)], name: &str) -> JoinOp {
    let channels: Vec<Par> = binds.iter().map(|(at, _)| path(at)).collect();
    let pairs: Vec<(&Par, BindPattern)> = channels
        .iter()
        .zip(binds)
        .map(|(channel, (_, pattern))| (channel, pattern.clone()))
        .collect();
    join_on(SPACE, &pairs, name)
}

/// `op` as a space operation.
fn joined(op: JoinOp) -> SpaceOp { SpaceOp::Join(op) }

/// The pattern that matches the list `[prefix…, x]` and binds `x`. The list holds a free
/// variable, so it is marked as a pattern (`connective_used`), as the normalizer marks it.
fn under(prefix: &[i64]) -> BindPattern {
    let mut ps = elements(prefix);
    ps.push(new_freevar_par(0, Vec::new()));
    BindPattern {
        patterns: vec![new_elist_par(ps, Vec::new(), true, None, Vec::new(), true)],
        remainder: None,
        free_count: 1,
    }
}

/// The pattern that binds two values.
fn any_two() -> BindPattern {
    BindPattern {
        patterns: vec![
            new_freevar_par(0, Vec::new()),
            new_freevar_par(1, Vec::new()),
        ],
        remainder: None,
        free_count: 2,
    }
}

/// The guard that refuses every match.
fn refusing() -> Par { new_gbool_par(false, Vec::new(), false) }

/// A fire of the body `name` with the bound values `values`, as `fired` reports it.
fn delivery(name: &str, values: Vec<Par>) -> (String, Vec<Par>) { (name.to_string(), values) }

/// A cost authority with one region of `byte`.
fn region(byte: u8) -> CostAuthority {
    CostAuthority {
        regions: vec![CostRegion {
            instance_id: vec![byte; 32],
            signature: Some(CostSignature {
                value: Some(CostSignatureValue::Ground(vec![byte])),
            }),
        }],
    }
}

/// The prefixes of the path `elements`, the empty prefix first.
fn prefixes_of(elements: &[Par]) -> Vec<Vec<Par>> {
    (0..=elements.len())
        .map(|depth| elements[..depth].to_vec())
        .collect()
}

/// The markers of each prefix, by the debug bytes of the prefix: the source hash of the datum of
/// each marker and its suffix.
type MarkerRows = BTreeMap<Vec<u8>, Vec<(Vec<u8>, Vec<Par>)>>;

/// A PathMap space of one family over an in-memory play space, run by the family agent.
struct Rig {
    host: ParRSpace,
    env: SpaceEnv,
}

impl Rig {
    /// A rig of `pathmap:<inner>` over a fresh play space.
    async fn new(inner: SpaceInner) -> Rig { Rig::over(play().await, inner).await }

    /// A rig of `pathmap:<inner>` over `host`, a fresh space.
    async fn over(host: ParRSpace, inner: SpaceInner) -> Rig {
        let env = env_over(host.clone(), &budget());
        let config = config_of(
            SPACE,
            SpaceOuter::PathMap.as_str(),
            inner.as_str(),
            SpaceQualifier::Default,
        );
        create(&FamilyAgent, &env, creation(SPACE, config, family(inner))).await;
        host.take_event_log().await;
        Rig { host, env }
    }

    /// Runs `op` in one job.
    async fn run(&self, op: SpaceOp) -> Result<SpaceOutcome, SpaceError> {
        run(&FamilyAgent, &self.env, op).await
    }

    /// Runs `op`, which must commit.
    async fn ok(&self, op: SpaceOp) -> SpaceOutcome {
        let label = format!("{op:?}");
        self.run(op)
            .await
            .unwrap_or_else(|error| panic!("the operation commits: {error:?}: {label}"))
    }

    /// Runs `op` and then every re-issue that follows it, as the jobs of one deploy do, and
    /// returns the outcome of each job.
    async fn chain(&self, op: SpaceOp) -> Vec<SpaceOutcome> {
        let mut outcomes = Vec::new();
        let mut next = Some(op);
        while let Some(op) = next.take() {
            let outcome = self.ok(op).await;
            next = outcome.reissue.as_ref().map(|reissue| reissue.op.clone());
            outcomes.push(outcome);
        }
        outcomes
    }

    /// The events since the last call.
    async fn log(&self) -> Vec<Event> { self.host.take_event_log().await }

    /// The root of a checkpoint of the play space. The checkpoint takes the event log.
    async fn root(&self) -> Blake2b256Hash {
        self.host
            .create_checkpoint()
            .await
            .expect("a checkpoint")
            .root
    }

    /// The stored datums of the path with the elements `at`.
    async fn datums(&self, at: &[Par]) -> Vec<Datum<ListParWithRandom>> {
        self.host.get_data(&data_at(at)).await
    }

    /// The payloads of the records of the path of the numbers `at`, in record order.
    async fn records(&self, at: &[i64]) -> Vec<Vec<Par>> {
        let mut records: Vec<(i64, Vec<Par>)> = self
            .datums(&elements(at))
            .await
            .into_iter()
            .map(|datum| {
                let record = record::unwrap_pq_datum(&datum.a).expect("a datum record");
                (record.ord, record.datum.pars)
            })
            .collect();
        records.sort_by_key(|(ord, _)| *ord);
        records.into_iter().map(|(_, pars)| pars).collect()
    }

    /// The markers of the prefix with the elements `at`, as stored.
    async fn marker_datums(&self, at: &[Par]) -> Vec<Datum<ListParWithRandom>> {
        self.host.get_data(&index_at(at)).await
    }

    /// The suffixes of the markers of the prefix of the numbers `at`, as sorted lists.
    async fn markers(&self, at: &[i64]) -> Vec<Par> {
        let mut suffixes: Vec<Par> = self
            .marker_datums(&elements(at))
            .await
            .into_iter()
            .map(|datum| list(decode_marker(&datum.a).expect("a marker").suffix.to_vec()))
            .collect();
        suffixes.sort_by_key(|suffix| format!("{suffix:?}"));
        suffixes
    }

    /// The number of single-bind waiters on the prefix of the numbers `at`.
    async fn waiters(&self, at: &[i64]) -> usize {
        self.host
            .get_waiting_continuations(vec![index_at(&elements(at))])
            .await
            .len()
    }

    /// The join groups of the prefix of the numbers `at`.
    async fn joins(&self, at: &[i64]) -> Vec<Vec<Par>> {
        self.host.get_joins(index_at(&elements(at))).await
    }

    /// The join waiters of the group of the prefixes `prefixes`, in canonical group order: the
    /// index channel of each prefix, ascending by its stable hash, then by bind index (FR-1').
    async fn join_waiters(&self, prefixes: &[&[i64]]) -> Vec<Vec<u8>> {
        let mut keyed: Vec<(Blake2b256Hash, usize)> = prefixes
            .iter()
            .enumerate()
            .map(|(position, at)| (hash(&index_at(&elements(at))), position))
            .collect();
        keyed.sort();
        let group: Vec<Par> = keyed
            .into_iter()
            .map(|(_, position)| index_at(&elements(prefixes[position])))
            .collect();
        self.host
            .get_waiting_continuations(group)
            .await
            .iter()
            .map(|waiter| bincode::serialize(waiter).expect("a waiter serializes"))
            .collect()
    }

    /// The value of the token `S(sid)`.
    async fn token(&self) -> i64 {
        match self.host.get_data(&token()).await.as_slice() {
            [datum] => record::counter_value(&datum.a).expect("a counter record"),
            other => panic!("the token holds {} records", other.len()),
        }
    }

    /// Checks the marker invariant of `PathMapPrefix.v` (`marker_invariant_preserved`) over the
    /// paths `paths`: a stored datum on `p` has exactly one marker on `I(q)` for each prefix `q`
    /// of `p`, with the suffix `p∖q`, and no other marker exists on the prefixes of these paths.
    async fn assert_marker_invariant(&self, paths: &[Vec<Par>], label: &str) {
        let mut prefixes: BTreeSet<Vec<u8>> = BTreeSet::new();
        let mut by_prefix: BTreeMap<Vec<u8>, Vec<Par>> = BTreeMap::new();
        for path in paths {
            for prefix in prefixes_of(path) {
                let key = format!("{prefix:?}").into_bytes();
                if prefixes.insert(key.clone()) {
                    by_prefix.insert(key, prefix);
                }
            }
        }
        let mut expected: MarkerRows = BTreeMap::new();
        let mut seen_paths: BTreeSet<Vec<u8>> = BTreeSet::new();
        for path in paths {
            if !seen_paths.insert(format!("{path:?}").into_bytes()) {
                continue;
            }
            for datum in self.datums(path).await {
                for prefix in prefixes_of(path) {
                    expected
                        .entry(format!("{prefix:?}").into_bytes())
                        .or_default()
                        .push((datum.source.hash.0.clone(), path[prefix.len()..].to_vec()));
                }
            }
        }
        for (key, prefix) in by_prefix {
            let mut found: Vec<(Vec<u8>, Vec<Par>)> = self
                .marker_datums(&prefix)
                .await
                .iter()
                .map(|datum| {
                    let view = decode_marker(&datum.a).expect("a marker decodes");
                    (view.source.to_vec(), view.suffix.to_vec())
                })
                .collect();
            let mut wanted = expected.remove(&key).unwrap_or_default();
            found.sort_by_key(|entry| format!("{entry:?}"));
            wanted.sort_by_key(|entry| format!("{entry:?}"));
            assert_eq!(
                found, wanted,
                "{label}: the markers of the prefix {prefix:?}"
            );
        }
    }
}

/// The fires of every outcome of `outcomes`, in job order.
fn all_fires(outcomes: &[SpaceOutcome]) -> Vec<(String, Vec<Par>)> {
    outcomes.iter().flat_map(fired).collect()
}

// ─── Examples: re-rooting ────────────────────────────────────────────────────────────────────

/// The FIPS example (`PathMapPrefix.v`, `fips_example`). `@[0,1,2]!({|"hi"|})`,
/// `@[0,1,2]!({|"hello"|})` and `@[0,1,3]!({|"there"|})` received on `@[0,1]` bind
/// `{|[2,"hi"]|}`, `{|[2,"hello"]|}` and `{|[3,"there"]|}`, the exact path first.
/// `@[0,1,2]!(42)` binds `[2, 42]`, and `@[0,1,2,"hi"]!({||})` binds `{|[2,"hi"]|}`.
#[tokio::test]
async fn pathmap_fips_example_reroot() {
    let rig = Rig::new(SpaceInner::Bag).await;
    let deep = |last: Par| list(vec![int(0), int(1), last]);
    rig.ok(put_on(&deep(int(2)), pathmap(vec![string("hi")])))
        .await;
    rig.ok(put_on(&deep(int(2)), pathmap(vec![string("hello")])))
        .await;
    rig.ok(put_on(&deep(int(3)), pathmap(vec![string("there")])))
        .await;
    let mut bound = Vec::new();
    for name in ["k1", "k2", "k3"] {
        let outcome = rig.ok(take(&[0, 1], name, any())).await;
        let [(body, values)] = fired(&outcome).try_into().expect("one fire");
        assert_eq!(body, name);
        bound.push(values);
    }
    let first_two: BTreeSet<String> = bound[..2]
        .iter()
        .map(|values| format!("{values:?}"))
        .collect();
    let expected: BTreeSet<String> = [vec![pathmap(vec![list(vec![int(2), string("hi")])])], vec![
        pathmap(vec![list(vec![int(2), string("hello")])]),
    ]]
    .iter()
    .map(|values| format!("{values:?}"))
    .collect();
    assert_eq!(first_two, expected, "the path [0,1,2] comes first");
    assert_eq!(bound[2], vec![pathmap(vec![list(vec![
        int(3),
        string("there")
    ])])]);

    rig.ok(take(&[0, 1], "n", any())).await;
    let outcome = rig.ok(put(&[0, 1, 2], 42)).await;
    assert_eq!(fired(&outcome), vec![delivery("n", vec![ints(&[2, 42])])]);

    rig.ok(take(&[0, 1], "u", any())).await;
    let unit = list(vec![int(0), int(1), int(2), string("hi")]);
    let outcome = rig.ok(put_on(&unit, pathmap(Vec::new()))).await;
    assert_eq!(fired(&outcome), vec![delivery("u", vec![pathmap(vec![
        list(vec![int(2), string("hi")])
    ])])]);
    for at in [&[0, 1, 2][..], &[0, 1, 3]] {
        assert!(rig.records(at).await.is_empty(), "{at:?}");
    }
    assert!(rig.markers(&[]).await.is_empty());
}

/// A value that is not a PathMap arrives as `[suffix…, value]` under a suffix that is not empty,
/// and as itself on its own path (`R(ε, v) = v`). A list payload is one element, so it is not
/// flattened, and each argument of a send is re-rooted on its own.
#[tokio::test]
async fn pathmap_non_pathmap_payload_list() {
    let rig = Rig::new(SpaceInner::Bag).await;
    for (at, payload, expected) in [
        (&[0, 1][..], int(42), ints(&[2, 42])),
        (&[0, 1], ints(&[5, 6]), list(vec![int(2), ints(&[5, 6])])),
        (&[0], string("s"), list(vec![int(1), int(2), string("s")])),
        (
            &[],
            string("t"),
            list(vec![int(0), int(1), int(2), string("t")]),
        ),
        (&[0, 1, 2], int(7), int(7)),
    ] {
        rig.ok(take(at, "k", any())).await;
        let outcome = rig.ok(put_on(&path(&[0, 1, 2]), payload)).await;
        assert_eq!(
            fired(&outcome),
            vec![delivery("k", vec![expected])],
            "{at:?}"
        );
    }
    rig.ok(SpaceOp::Consume(receive(&path(&[0]), "two", any_two())))
        .await;
    let outcome = rig
        .ok(SpaceOp::Produce(send_values(
            &path(&[0, 1]),
            vec![int(7), int(8)],
            0,
        )))
        .await;
    assert_eq!(fired(&outcome), vec![delivery("two", vec![
        ints(&[1, 7]),
        ints(&[1, 8])
    ])]);
    rig.assert_marker_invariant(&[elements(&[0, 1, 2]), elements(&[0, 1])], "the list rule")
        .await;
}

/// The unit rule of the FIPS: `{||}` under a suffix that is not empty is `{|suffix|}`. An entry
/// of one element that is not a list is the element itself (`canonical_entry`), so the suffix
/// `["hi"]` gives `{|"hi"|}`. On its own path the suffix is empty, so `{||}` stays empty
/// (Rholang's `{||}` is the empty map).
#[tokio::test]
async fn pathmap_empty_unit_rule() {
    let rig = Rig::new(SpaceInner::Bag).await;
    let channel = list(vec![int(0), int(1), int(2), string("hi")]);
    for (at, expected) in [
        (
            vec![int(0), int(1)],
            pathmap(vec![list(vec![int(2), string("hi")])]),
        ),
        (vec![int(0), int(1), int(2)], pathmap(vec![string("hi")])),
        (
            vec![int(0), int(1), int(2), string("hi")],
            pathmap(Vec::new()),
        ),
    ] {
        rig.ok(SpaceOp::Consume(receive(&list(at.clone()), "k", any())))
            .await;
        let outcome = rig.ok(put_on(&channel, pathmap(Vec::new()))).await;
        assert_eq!(
            fired(&outcome),
            vec![delivery("k", vec![expected])],
            "{at:?}"
        );
    }
    // A stored unit datum re-roots when a receive takes it.
    rig.ok(put_on(&channel, pathmap(Vec::new()))).await;
    let outcome = rig.ok(take(&[0], "late", any())).await;
    assert_eq!(fired(&outcome), vec![delivery("late", vec![pathmap(
        vec![list(vec![int(1), int(2), string("hi")])]
    )])]);
}

/// The suffix arrives as its elements, never as the bytes of a key. The legacy store decoded a
/// suffix that was not a tagged path as one `GInt` for each byte
/// (`L/spaces/types/pathmap.rs`, `path_to_par`).
#[tokio::test]
async fn pathmap_legacy_gint_suffix_absent() {
    let rig = Rig::new(SpaceInner::Bag).await;
    let channel = list(vec![string("ab"), int(300), string("xyz")]);
    rig.ok(SpaceOp::Consume(receive(
        &list(vec![string("ab")]),
        "k",
        any(),
    )))
    .await;
    let outcome = rig.ok(put_on(&channel, int(1))).await;
    let expected = list(vec![int(300), string("xyz"), int(1)]);
    assert_eq!(fired(&outcome), vec![delivery("k", vec![expected])]);
    let [(_, values)] = fired(&outcome).try_into().expect("one fire");
    let ExprInstance::EListBody(delivered) =
        values[0].exprs[0].expr_instance.clone().expect("a list")
    else {
        panic!("the delivery is a list");
    };
    assert_eq!(delivered.ps.len(), 3, "two suffix elements and the value");
    assert!(
        rig.records(&[]).await.is_empty() && rig.markers(&[]).await.is_empty(),
        "the delivery stored nothing"
    );
}

// ─── Examples: candidates, waiters and markers ───────────────────────────────────────────────

/// PRODUCE-WAITERS: a send on `[0,1,2]` meets the waiters of `[0,1,2]`, then `[0,1]`, `[0]` and
/// `[]`, whatever the order of the parks. A refusal moves on to the next prefix.
#[tokio::test]
async fn pathmap_most_specific_waiter_first() {
    for order in [[&[][..], &[0], &[0, 1]], [&[0, 1][..], &[], &[0]]] {
        let rig = Rig::new(SpaceInner::Bag).await;
        for at in order {
            rig.ok(take(at, &format!("w{}", at.len()), any())).await;
        }
        let mut names = Vec::new();
        for value in 1..=3 {
            let outcome = rig.ok(put(&[0, 1, 2], value)).await;
            let [(name, _)] = fired(&outcome).try_into().expect("one fire");
            names.push(name);
        }
        assert_eq!(names, vec!["w2", "w1", "w0"], "{order:?}");
    }
    let rig = Rig::new(SpaceInner::Bag).await;
    rig.ok(take(&[0, 1], "picky", exactly(9))).await;
    rig.ok(take(&[0], "open", any())).await;
    let outcome = rig.ok(put(&[0, 1], 5)).await;
    assert_eq!(fired(&outcome), vec![delivery("open", vec![ints(&[1, 5])])]);
    assert_eq!(rig.waiters(&[0, 1]).await, 1, "the refusing waiter stays");
    // CONSUME-CANDIDATES: the exact path first, then the longer paths in path-key order.
    let rig = Rig::new(SpaceInner::Bag).await;
    rig.ok(put(&[0, 2], 2)).await;
    rig.ok(put(&[0, 1], 1)).await;
    rig.ok(put(&[0], 0)).await;
    let mut taken = Vec::new();
    for _ in 0..3 {
        let outcome = rig.ok(take(&[0], "k", any())).await;
        let [(_, values)] = fired(&outcome).try_into().expect("one fire");
        taken.push(values);
    }
    assert_eq!(taken, vec![vec![int(0)], vec![ints(&[1, 1])], vec![ints(
        &[2, 2]
    )]]);
}

/// The head of each prefix blocks its own later waiters on a queue space, and a refusal moves
/// to the head waiter of the next prefix (seams design S-15). A receive on a prefix sees only
/// the head of each path.
#[tokio::test]
async fn pathmap_queue_head_blocks() {
    let rig = Rig::new(SpaceInner::Queue).await;
    rig.ok(take(&[0], "head", exactly(9))).await;
    rig.ok(take(&[0], "later", any())).await;
    let outcome = rig.ok(put(&[0, 1], 7)).await;
    assert!(
        outcome.fires.is_empty(),
        "the refusing head blocks the prefix"
    );
    assert_eq!(rig.records(&[0, 1]).await, vec![vec![int(7)]]);
    assert_eq!(rig.waiters(&[0]).await, 2);

    let rig = Rig::new(SpaceInner::Queue).await;
    rig.ok(take(&[0], "head", exactly(9))).await;
    rig.ok(take(&[0], "later", any())).await;
    rig.ok(take(&[], "root", any())).await;
    let outcome = rig.ok(put(&[0, 1], 7)).await;
    assert_eq!(
        fired(&outcome),
        vec![delivery("root", vec![ints(&[0, 1, 7])])],
        "the refusal moves to the head of the next prefix"
    );
    assert_eq!(rig.waiters(&[0]).await, 2);

    let rig = Rig::new(SpaceInner::Queue).await;
    rig.ok(put(&[0, 1], 1)).await;
    rig.ok(put(&[0, 1], 2)).await;
    let outcome = rig.ok(take(&[0], "second", exactly_list(&[1, 2]))).await;
    assert!(
        outcome.fires.is_empty(),
        "a receive sees only the head of each path, so it parks"
    );
    assert_eq!(rig.waiters(&[0]).await, 1);
    let outcome = rig.ok(take(&[0, 1], "first", any())).await;
    assert_eq!(
        fired(&outcome),
        vec![
            delivery("first", vec![int(1)]),
            delivery("second", Vec::new())
        ],
        "the pop exposes the record 2, so the parked receive fires in the cascade"
    );
    assert_eq!(rig.waiters(&[0]).await, 0);
    assert!(rig.records(&[0, 1]).await.is_empty());
}

/// The pattern that matches exactly the list of the numbers `values`.
fn exactly_list(values: &[i64]) -> BindPattern {
    BindPattern {
        patterns: vec![ints(values)],
        remainder: None,
        free_count: 0,
    }
}

/// A receive takes the record and every marker of the datum (RETIRE-PLAN), at each depth.
#[tokio::test]
async fn pathmap_consume_removes_all_markers() {
    for depth in 0..=3 {
        let rig = Rig::new(SpaceInner::Bag).await;
        rig.ok(put(&[0, 1, 2], 7)).await;
        rig.ok(put(&[0, 5], 8)).await;
        assert_eq!(rig.records(&[0, 1, 2]).await, vec![vec![int(7)]]);
        assert_eq!(rig.markers(&[0, 1]).await, vec![ints(&[2])]);
        assert_eq!(rig.markers(&[0, 1, 2]).await, vec![list(Vec::new())]);
        let prefix = &[0, 1, 2][..depth];
        // On the exact path the value arrives as itself (`R(ε, v) = v`).
        let pattern = match depth {
            3 => any(),
            _ => under(&[0, 1, 2][depth..]),
        };
        let outcome = rig.ok(take(prefix, "k", pattern)).await;
        assert_eq!(
            fired(&outcome),
            vec![delivery("k", vec![int(7)])],
            "{depth}"
        );
        assert!(rig.records(&[0, 1, 2]).await.is_empty());
        for at in [&[][..], &[0], &[0, 1], &[0, 1, 2]] {
            let others: usize = match at.len() {
                0 | 1 => 1,
                _ => 0,
            };
            assert_eq!(rig.markers(at).await.len(), others, "{depth}: {at:?}");
        }
        assert_eq!(rig.waiters(prefix).await, 0);
        rig.assert_marker_invariant(&[elements(&[0, 1, 2]), elements(&[0, 5])], "consume")
            .await;
    }
}

/// The marker records of the datum `datum` on the path `at`, one for each prefix, depth first:
/// the source hash of the datum, the suffix, the random state `PathMarker.derive(rs).split_short(k)`
/// and the authority of the record of the datum.
fn expected_markers(at: &[Par], datum: &Datum<ListParWithRandom>) -> Vec<ListParWithRandom> {
    let random = Blake2b512Random::from_bytes(&datum.a.random_state);
    (0..=at.len())
        .map(|depth| {
            marker_record(
                &datum.source.hash,
                at[depth..].to_vec(),
                &marker_random(&random, depth).expect("a marker random state"),
                datum.a.cost_authority.clone(),
            )
        })
        .collect()
}

/// Rule E on a PathMap space (decision 45, `peek_restores_delivered_marker`). A peek at each
/// depth removes the marker of the datum on its prefix with the COMM and restores it with the
/// same bytes and the same produce identity. The record and the other markers stay. The
/// marker authority is the authority of the record of the datum: `None`, an empty authority and
/// a region give equal bytes. A pure read bumps no token (decision 51).
#[tokio::test]
async fn pathmap_peek_restores_marker() {
    let at = elements(&[0, 1, 2]);
    for authority in [None, Some(CostAuthority::default()), Some(region(3))] {
        for depth in 0..=at.len() {
            let rig = Rig::new(SpaceInner::Bag).await;
            let mut send = send_values(&path(&[0, 1, 2]), vec![int(7)], 0);
            send.data.cost_authority = authority.clone();
            rig.ok(SpaceOp::Produce(send)).await;
            let [datum] = <[Datum<ListParWithRandom>; 1]>::try_from(rig.datums(&at).await)
                .expect("one datum");
            assert_eq!(datum.a.cost_authority, authority);
            let stored: Vec<Vec<Datum<ListParWithRandom>>> = {
                let mut stored = Vec::new();
                for prefix in prefixes_of(&at) {
                    stored.push(rig.marker_datums(&prefix).await);
                }
                stored
            };
            for (prefix_depth, markers) in stored.iter().enumerate() {
                let [marker] = markers.as_slice() else {
                    panic!("one marker on each prefix");
                };
                assert_eq!(
                    marker.a,
                    expected_markers(&at, &datum)[prefix_depth],
                    "STORE-MARKERS with the record authority"
                );
                assert_eq!(marker.a.cost_authority, authority);
            }
            let before = rig.token().await;
            rig.log().await;
            let prefix: Vec<i64> = [0, 1, 2][..depth].to_vec();
            let outcome = rig.ok(peek(&prefix, "p", any())).await;
            assert_eq!(fired(&outcome).len(), 1, "{authority:?} {depth}");
            let log = rig.log().await;
            for (prefix_depth, prefix) in prefixes_of(&at).iter().enumerate() {
                assert_eq!(
                    rig.marker_datums(prefix).await,
                    stored[prefix_depth],
                    "{authority:?} {depth}: the marker of depth {prefix_depth}"
                );
            }
            assert_eq!(rig.datums(&at).await, vec![datum.clone()]);
            let index = index_at(&at[..depth]);
            let restored: Vec<&Produce> = log
                .iter()
                .filter_map(|event| match event {
                    Event::IoEvent(IOEvent::Produce(produce))
                        if produce.channel_hash == hash(&index) =>
                    {
                        Some(produce)
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(
                restored,
                vec![&stored[depth][0].source],
                "{authority:?} {depth}: the restore keeps the produce identity"
            );
            assert_eq!(rig.token().await, before, "a pure read bumps no token");
            rig.assert_marker_invariant(std::slice::from_ref(&at), "a peek")
                .await;
        }
    }
}

/// Every COMM of a PathMap space is single-channel (`delivery_vehicle_single_channel`, F4). The
/// delivery COMM is on the index channel of the waiter with the vehicle as its produce, and each
/// retire is a single-channel `K_retire`. The vehicle is never stored.
#[tokio::test]
async fn pathmap_marker_vehicle_comm_shape() {
    let rig = Rig::new(SpaceInner::Bag).await;
    rig.ok(put(&[0, 1], 7)).await;
    rig.log().await;
    rig.ok(take(&[0], "k", any())).await;
    let log = rig.log().await;
    let mut comms = 0;
    for event in &log {
        if let Event::Comm(comm) = event {
            comms += 1;
            assert_eq!(
                comm.consume.channel_hashes.len(),
                1,
                "a single-channel COMM"
            );
            for produce in &comm.produces {
                assert_eq!(
                    produce.channel_hash, comm.consume.channel_hashes[0],
                    "the produce of a COMM is on its channel"
                );
            }
            assert!(comm.peeks.is_empty());
        }
    }
    // One delivery on I([0]), the retire of the record on D([0,1]), the retires of the markers
    // on I([]) and I([0,1]), and the token bump of the bag that changed the state (decision 51).
    assert_eq!(comms, 5);
    let bumps = log
        .iter()
        .filter(|event| matches!(event, Event::Comm(comm) if comm.consume.channel_hashes[0] == hash(&token())))
        .count();
    assert_eq!(bumps, 1, "one token bump");
    let delivered = log
        .iter()
        .filter(|event| matches!(event, Event::Comm(comm) if comm.consume.channel_hashes[0] == hash(&index_at(&elements(&[0])))))
        .count();
    assert_eq!(delivered, 1);

    // A produce-side delivery: the vehicle is the produce of the COMM on I(q), and no store
    // keeps it.
    rig.ok(take(&[0], "v", any())).await;
    rig.log().await;
    let outcome = rig.ok(put(&[0, 3], 9)).await;
    assert_eq!(fired(&outcome), vec![delivery("v", vec![ints(&[3, 9])])]);
    let log = rig.log().await;
    let index = hash(&index_at(&elements(&[0])));
    let comm = log
        .iter()
        .find_map(|event| match event {
            Event::Comm(comm) if comm.consume.channel_hashes == vec![index.clone()] => Some(comm),
            _ => None,
        })
        .expect("the delivery COMM on I([0])");
    assert_eq!(comm.produces.len(), 1);
    for prefix in prefixes_of(&elements(&[0, 3])) {
        assert!(rig.marker_datums(&prefix).await.is_empty(), "{prefix:?}");
    }
    assert!(rig.datums(&elements(&[0, 3])).await.is_empty());
}

/// The vehicle of a datum on a prefix is its marker followed by its payload
/// (`vehicle_carries_payload`, decision 40 Q3). So the introduction bytes of a delivered send
/// cover the payload, as the equal host produce does, and the bound values are the payload.
#[tokio::test]
async fn pathmap_vehicle_introduction_covers_payload() {
    let rig = Rig::new(SpaceInner::Bag).await;
    rig.ok(take(&[0], "v", any())).await;
    let mut send = send_values(&path(&[0, 4]), vec![string("payload")], 0);
    send.data.cost_authority = Some(region(5));
    let data = send.data.clone();
    rig.log().await;
    let outcome = rig.ok(SpaceOp::Produce(send)).await;
    assert_eq!(fired(&outcome), vec![delivery("v", vec![list(vec![
        int(4),
        string("payload")
    ])])]);
    let [fire] = outcome.fires.as_slice() else {
        panic!("one fire");
    };
    let [result] = fire.results.as_slice() else {
        panic!("one bound value");
    };
    let vehicle = decode_vehicle(&result.removed_datum).expect("the vehicle decodes");
    assert_eq!(vehicle.marker.suffix, elements(&[4]).as_slice());
    assert_eq!(vehicle.payload, data.pars.as_slice());
    assert_eq!(result.removed_datum.cost_authority, data.cost_authority);
}

// ─── Examples: path rules ────────────────────────────────────────────────────────────────────

/// A PathMap or a zipper anywhere in a path element aborts with `UnsupportedPathElement`
/// (`path_element_rules`), on a send, a receive and a join, and the job stages nothing. A
/// PathMap as a payload is re-rooted.
#[tokio::test]
async fn pathmap_epathmap_element_rejected() {
    let rig = Rig::new(SpaceInner::Bag).await;
    let zipper = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EZipperBody(EZipper {
            pathmap: Some(EPathMap {
                ps: vec![int(1)],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            }),
            current_path: Vec::new(),
            is_write_zipper: false,
            locally_free: Vec::new(),
            connective_used: false,
        })),
    }]);
    let nested = list(vec![int(0), list(vec![string("a"), pathmap(vec![int(1)])])]);
    for channel in [pathmap(Vec::new()), zipper.clone(), nested.clone()] {
        let root = rig.root().await;
        assert_eq!(
            rig.run(put_on(&channel, int(1))).await,
            Err(SpaceError::UnsupportedPathElement)
        );
        assert_eq!(
            rig.run(SpaceOp::Consume(receive(&channel, "k", any())))
                .await,
            Err(SpaceError::UnsupportedPathElement)
        );
        let mut binds = join(&[(&[0], any()), (&[1], any())], "j");
        binds.binds[1].channel = channel.clone();
        assert_eq!(
            rig.run(joined(binds)).await,
            Err(SpaceError::UnsupportedPathElement)
        );
        assert!(rig.log().await.is_empty());
        assert_eq!(rig.root().await, root);
    }
    rig.ok(take(&[0], "k", any())).await;
    let outcome = rig.ok(put_on(&path(&[0, 1]), pathmap(vec![int(5)]))).await;
    assert_eq!(fired(&outcome), vec![delivery("k", vec![pathmap(vec![
        ints(&[1, 5])
    ])])]);
}

/// A path of 65 elements aborts with `PathTooDeep` before any staging (F9), and a path of 64
/// elements is accepted with 65 markers.
#[tokio::test]
async fn pathmap_path_too_deep_aborts() {
    let rig = Rig::new(SpaceInner::Bag).await;
    let too_deep: Vec<i64> = (0..=PATH_DEPTH_CAP as i64).collect();
    let root = rig.root().await;
    assert_eq!(
        rig.run(put(&too_deep, 1)).await,
        Err(SpaceError::PathTooDeep)
    );
    assert_eq!(
        rig.run(take(&too_deep, "k", any())).await,
        Err(SpaceError::PathTooDeep)
    );
    assert!(rig.log().await.is_empty());
    assert_eq!(rig.root().await, root);
    let deepest: Vec<i64> = (0..PATH_DEPTH_CAP as i64).collect();
    rig.ok(put(&deepest, 1)).await;
    let mut markers = 0;
    for prefix in prefixes_of(&elements(&deepest)) {
        markers += rig.marker_datums(&prefix).await.len();
    }
    assert_eq!(markers, PATH_DEPTH_CAP + 1);
    let outcome = rig.ok(take(&deepest[..63], "k", any())).await;
    assert_eq!(fired(&outcome), vec![delivery("k", vec![ints(&[63, 1])])]);
    rig.assert_marker_invariant(&[elements(&deepest)], "the deepest path")
        .await;
}

/// The keys of the store are the EPM1 keys of the paths (`canonical_path.rs` at `11c92e7cc`,
/// D5 G4). The record of `@["a","b"]!(1)` lies on `D(sid, 04 01 61 04 01 62 00)`, and its
/// markers on `I(sid, ε)`, `I(sid, 04 01 61)` and `I(sid, 04 01 61 04 01 62)`.
#[tokio::test]
async fn pathmap_segment_codec_matches_epm1_vectors() {
    let rig = Rig::new(SpaceInner::Bag).await;
    let channel = list(vec![string("a"), string("b")]);
    rig.ok(put_on(&channel, int(1))).await;
    let data = namespace::gprivate(namespace::path_data_channel(SPACE, &[
        0x04, 0x01, b'a', 0x04, 0x01, b'b', 0x00,
    ]));
    assert_eq!(rig.host.get_data(&data).await.len(), 1);
    for key in [&[][..], &[0x04, 0x01, b'a'], &[
        0x04, 0x01, b'a', 0x04, 0x01, b'b',
    ]] {
        let index = namespace::gprivate(namespace::path_index_channel(SPACE, key));
        assert_eq!(rig.host.get_data(&index).await.len(), 1, "{key:?}");
    }
    // The pins of `per_arm_byte_pins`: `[5]` is `03 0A 00`, and `[[]]` is `0B 00 00`.
    for (channel, key) in [
        (ints(&[5]), vec![0x03, 0x0A, 0x00]),
        (list(vec![list(Vec::new())]), vec![0x0B, 0x00, 0x00]),
        (list(vec![new_gbool_par(true, Vec::new(), false)]), vec![
            0x02, 0x00,
        ]),
    ] {
        rig.ok(put_on(&channel, int(2))).await;
        let data = namespace::gprivate(namespace::path_data_channel(SPACE, &key));
        assert_eq!(rig.host.get_data(&data).await.len(), 1, "{key:?}");
    }
}

/// A name of the space is the path `[n]` (`@c ≡ @[c]`), so a name roots a private subtree. A
/// waiter on `n` receives the sends on `[n, …]`, and a send under another name never reaches
/// it.
#[tokio::test]
async fn pathmap_gensym_roots_private_subtree() {
    let rig = Rig::new(SpaceInner::Bag).await;
    let name =
        |byte: u8| namespace::gprivate(namespace::name_id(SPACE, NameTail::Random([byte; 32])));
    let (mine, other) = (name(1), name(2));
    rig.ok(SpaceOp::Consume(receive(&mine, "mine", any())))
        .await;
    let outcome = rig
        .ok(put_on(&list(vec![other.clone(), int(1)]), int(5)))
        .await;
    assert!(outcome.fires.is_empty(), "another name is another subtree");
    let outcome = rig
        .ok(put_on(&list(vec![mine.clone(), int(1)]), int(6)))
        .await;
    assert_eq!(fired(&outcome), vec![delivery("mine", vec![ints(&[1, 6])])]);
    let outcome = rig
        .ok(SpaceOp::Consume(receive(&other, "other", any())))
        .await;
    assert_eq!(fired(&outcome), vec![delivery("other", vec![ints(&[
        1, 5
    ])])]);
}

/// `@c` and `@[c]` are one path (`channel_path`): a send on `c` reaches a waiter on `[c]` with
/// the value itself, and the record of a send on `[c]` lies on the data channel of `c`.
#[tokio::test]
async fn pathmap_channel_equals_singleton_list() {
    let rig = Rig::new(SpaceInner::Bag).await;
    let channel = string("c");
    rig.ok(SpaceOp::Consume(receive(
        &list(vec![channel.clone()]),
        "k",
        any(),
    )))
    .await;
    let outcome = rig.ok(put_on(&channel, int(3))).await;
    assert_eq!(fired(&outcome), vec![delivery("k", vec![int(3)])]);
    rig.ok(put_on(&list(vec![channel.clone()]), int(4))).await;
    assert_eq!(
        rig.host
            .get_data(&data_at(std::slice::from_ref(&channel)))
            .await
            .len(),
        1
    );
    let outcome = rig
        .ok(SpaceOp::Consume(receive(&channel, "c", any())))
        .await;
    assert_eq!(fired(&outcome), vec![delivery("c", vec![int(4)])]);
    let outcome = rig
        .ok(put_on(&list(vec![channel.clone(), int(9)]), int(5)))
        .await;
    assert!(outcome.fires.is_empty());
    let outcome = rig
        .ok(SpaceOp::Consume(receive(&channel, "deep", any())))
        .await;
    assert_eq!(fired(&outcome), vec![delivery("deep", vec![ints(&[9, 5])])]);
}

// ─── Examples: the token ─────────────────────────────────────────────────────────────────────

/// The token `S(sid)` (decisions 31 and 51). A bag bumps it once for each transaction that
/// changes the state: a store, a fire that spends a datum or a waiter, and a park. A peek whose
/// marker is restored changes nothing and bumps nothing (F16). A queue bumps it once for every
/// operation, a peek included.
#[tokio::test]
async fn pathmap_token_bumped_every_state_change() {
    let rig = Rig::new(SpaceInner::Bag).await;
    let mut expected = rig.token().await;
    for (op, bumps) in [
        (put(&[0, 1], 1), true),
        (peek(&[0], "p", any()), false),
        (peek(&[0, 1], "p", exactly(1)), false),
        (take(&[5], "w", any()), true),
        (take(&[0], "k", any()), true),
        (put(&[5, 1], 2), true),
        (put_forever(&[6], 3), true),
        (peek(&[6], "p", any()), false),
        (take(&[6], "t", any()), false),
    ] {
        let label = format!("{op:?}");
        rig.ok(op).await;
        if bumps {
            expected += 1;
        }
        assert_eq!(rig.token().await, expected, "bag: {label}");
    }
    let rig = Rig::new(SpaceInner::Queue).await;
    let mut expected = rig.token().await;
    for op in [
        put(&[0, 1], 1),
        peek(&[0], "p", any()),
        take(&[0], "k", any()),
        take(&[7], "w", any()),
    ] {
        let label = format!("{op:?}");
        rig.ok(op).await;
        expected += 1;
        assert_eq!(rig.token().await, expected, "queue: {label}");
    }
}

// ─── Examples: the disciplines over prefixes ─────────────────────────────────────────────────

/// A stack space over prefixes (P5b on paths): a receive sees the top of each path, and a pop
/// that exposes a new top fires a parked waiter in the cascade.
#[tokio::test]
async fn pathmap_stack_settle() {
    let rig = Rig::new(SpaceInner::Stack).await;
    rig.ok(put(&[0, 1], 1)).await;
    rig.ok(put(&[0, 1], 2)).await;
    let outcome = rig.ok(take(&[0], "below", exactly_list(&[1, 1]))).await;
    assert!(
        outcome.fires.is_empty(),
        "the top is 2, so the receive parks"
    );
    let outcome = rig.ok(take(&[0, 1], "pop", any())).await;
    assert_eq!(
        all_fires(std::slice::from_ref(&outcome)),
        vec![delivery("pop", vec![int(2)]), delivery("below", Vec::new())],
        "the pop exposes 1, and the parked receive fires in the cascade"
    );
    assert!(rig.records(&[0, 1]).await.is_empty());
    assert_eq!(rig.waiters(&[0]).await, 0);
    rig.assert_marker_invariant(&[elements(&[0, 1])], "the stack")
        .await;

    // A fresh receive becomes the top waiter of its prefix.
    let rig = Rig::new(SpaceInner::Stack).await;
    rig.ok(take(&[0], "old", any())).await;
    rig.ok(take(&[0], "new", any())).await;
    let outcome = rig.ok(put(&[0, 3], 5)).await;
    assert_eq!(fired(&outcome), vec![delivery("new", vec![ints(&[3, 5])])]);
}

/// A priority-queue space over prefixes: on each path the lower level comes first, a level is
/// FIFO, and a lane sees only its level.
#[tokio::test]
async fn pathmap_pq_levels() {
    let rig = Rig::new(SpaceInner::PriorityQueue).await;
    rig.ok(put_level(&[0, 1], 10, Some(1))).await;
    rig.ok(put_level(&[0, 1], 20, Some(0))).await;
    rig.ok(put_level(&[0, 1], 21, Some(0))).await;
    let mut taken = Vec::new();
    for _ in 0..3 {
        let outcome = rig.ok(take(&[0], "k", any())).await;
        let [(_, values)] = fired(&outcome).try_into().expect("one fire");
        taken.push(values);
    }
    assert_eq!(taken, vec![
        vec![ints(&[1, 20])],
        vec![ints(&[1, 21])],
        vec![ints(&[1, 10])]
    ]);
    let rig = Rig::new(SpaceInner::PriorityQueue).await;
    rig.ok(put_level(&[0, 2], 5, Some(1))).await;
    let outcome = rig.ok(take_in(&[0], "lane0", any(), Some(0))).await;
    assert!(
        outcome.fires.is_empty(),
        "the lane of level 0 does not see level 1"
    );
    let outcome = rig.ok(take_in(&[0], "lane1", any(), Some(1))).await;
    assert_eq!(fired(&outcome), vec![delivery("lane1", vec![ints(&[
        2, 5
    ])])]);
    let outcome = rig.ok(put_level(&[0, 3], 6, Some(0))).await;
    assert_eq!(fired(&outcome), vec![delivery("lane0", vec![ints(&[
        3, 6
    ])])]);
    rig.assert_marker_invariant(&[elements(&[0, 2]), elements(&[0, 3])], "the PQ")
        .await;
}

/// A set space over prefixes (F3, F24): a receive that binds a class retires the other members
/// of that class on its path with `K_retire`. A member of the class on another path stays, and a
/// peek retires nothing.
#[tokio::test]
async fn pathmap_set_class_retired() {
    let rig = Rig::new(SpaceInner::Set).await;
    rig.ok(put_salted(&[0, 1], 4, 1)).await;
    rig.ok(put_salted(&[0, 1], 4, 2)).await;
    rig.ok(put_salted(&[0, 2], 4, 3)).await;
    assert_eq!(rig.records(&[0, 1]).await.len(), 2);
    let outcome = rig.ok(peek(&[0], "p", any())).await;
    assert_eq!(fired(&outcome).len(), 1);
    assert_eq!(
        rig.records(&[0, 1]).await.len(),
        2,
        "a peek retires nothing"
    );
    rig.assert_marker_invariant(&[elements(&[0, 1]), elements(&[0, 2])], "the peek")
        .await;
    let outcome = rig.ok(take(&[0], "k", any())).await;
    let fires = fired(&outcome);
    assert_eq!(fires.len(), 1);
    assert_eq!(fires[0].1[0], ints(&[1, 4]), "the path [0,1] comes first");
    assert!(
        rig.records(&[0, 1]).await.is_empty(),
        "the class leaves its path"
    );
    assert_eq!(
        rig.records(&[0, 2]).await.len(),
        1,
        "another path keeps its class"
    );
    rig.assert_marker_invariant(&[elements(&[0, 1]), elements(&[0, 2])], "the class fire")
        .await;
    // A produce-side join fire spends the stored representative of a class and retires the
    // other member of that class on its path (decision 50).
    let rig = Rig::new(SpaceInner::Set).await;
    rig.ok(joined(join(&[(&[3], any()), (&[4], any())], "j")))
        .await;
    rig.ok(put_salted(&[3], 8, 1)).await;
    rig.ok(put_salted(&[3], 8, 2)).await;
    assert_eq!(rig.records(&[3]).await.len(), 2, "the join waits for [4]");
    let outcome = rig.ok(put(&[4], 5)).await;
    assert_eq!(fired(&outcome), vec![delivery("j", vec![int(8), int(5)])]);
    assert!(
        rig.records(&[3]).await.is_empty(),
        "the class leaves its path"
    );
    rig.assert_marker_invariant(&[elements(&[3]), elements(&[4])], "the join class fire")
        .await;
}

/// A cell space over prefixes (F15): one record on each path and one waiter on each prefix. A
/// second store fails with `CellOccupied`, and the job stages nothing.
#[tokio::test]
async fn pathmap_cell_occupied() {
    let rig = Rig::new(SpaceInner::Cell).await;
    rig.ok(put(&[0, 1], 1)).await;
    rig.ok(put(&[0, 2], 2)).await;
    let root = rig.root().await;
    assert_eq!(
        rig.run(put(&[0, 1], 3)).await,
        Err(SpaceError::CellOccupied)
    );
    assert!(rig.log().await.is_empty());
    assert_eq!(rig.root().await, root);
    rig.ok(take(&[9], "w", any())).await;
    assert_eq!(
        rig.run(take(&[9], "w2", any())).await,
        Err(SpaceError::CellOccupied)
    );
    let outcome = rig.ok(take(&[0], "k", any())).await;
    assert_eq!(fired(&outcome), vec![delivery("k", vec![ints(&[1, 1])])]);
    rig.ok(put(&[0, 1], 4)).await;
    assert_eq!(rig.records(&[0, 1]).await, vec![vec![int(4)]]);
    rig.assert_marker_invariant(&[elements(&[0, 1]), elements(&[0, 2])], "the cell")
        .await;
}

// ─── Examples: joins over prefixes (wave W4) ─────────────────────────────────────────────────

/// A join over prefixes of one space (joins plan section 9): it parks on the index channels of
/// its prefixes, a send under one prefix waits for the other, and the second send fires it with
/// the re-rooted values in bind order. A consume-side join fires from stored data at once, and a
/// join takes two distinct datums on one path.
#[tokio::test]
async fn pathmap_joins_over_prefixes() {
    let rig = Rig::new(SpaceInner::Bag).await;
    rig.ok(joined(join(&[(&[0], any()), (&[1], any())], "j")))
        .await;
    assert_eq!(rig.joins(&[0]).await.len(), 1);
    let outcome = rig.ok(put(&[0, 5], 1)).await;
    assert!(outcome.fires.is_empty());
    let outcome = rig.ok(put(&[1, 6], 2)).await;
    assert_eq!(fired(&outcome), vec![delivery("j", vec![
        ints(&[5, 1]),
        ints(&[6, 2])
    ])]);
    assert!(rig.join_waiters(&[&[0], &[1]]).await.is_empty());
    rig.assert_marker_invariant(
        &[elements(&[0, 5]), elements(&[1, 6])],
        "a produce-side join",
    )
    .await;

    let rig = Rig::new(SpaceInner::Bag).await;
    rig.ok(put(&[0, 5], 1)).await;
    rig.ok(put(&[1], 2)).await;
    let outcome = rig
        .ok(joined(join(&[(&[1], any()), (&[0], any())], "c")))
        .await;
    assert_eq!(fired(&outcome), vec![delivery("c", vec![
        int(2),
        ints(&[5, 1])
    ])]);
    for at in [&[0, 5][..], &[1]] {
        assert!(rig.records(at).await.is_empty(), "{at:?}");
    }
    assert!(rig.markers(&[]).await.is_empty());

    // Two binds on one prefix bind two distinct datums.
    let rig = Rig::new(SpaceInner::Bag).await;
    rig.ok(put(&[2, 1], 1)).await;
    let outcome = rig
        .ok(joined(join(&[(&[2], any()), (&[2], any())], "twice")))
        .await;
    assert!(
        outcome.fires.is_empty(),
        "one datum cannot bind two positions"
    );
    let outcome = rig.ok(put(&[2, 1], 3)).await;
    let fires = fired(&outcome);
    assert_eq!(fires.len(), 1);
    let bound: BTreeSet<String> = fires[0]
        .1
        .iter()
        .map(|value| format!("{value:?}"))
        .collect();
    let expected: BTreeSet<String> = [ints(&[1, 1]), ints(&[1, 3])]
        .iter()
        .map(|value| format!("{value:?}"))
        .collect();
    assert_eq!(bound, expected);

    // A peek join restores each removed marker (Rule E).
    let rig = Rig::new(SpaceInner::Bag).await;
    rig.ok(put(&[0, 1], 1)).await;
    rig.ok(put(&[4], 2)).await;
    let before: Vec<Vec<Datum<ListParWithRandom>>> = {
        let mut before = Vec::new();
        for prefix in [
            elements(&[]),
            elements(&[0]),
            elements(&[0, 1]),
            elements(&[4]),
        ] {
            before.push(rig.marker_datums(&prefix).await);
        }
        before
    };
    let outcome = rig
        .ok(joined(JoinOp {
            peek: true,
            ..join(&[(&[0], any()), (&[4], any())], "pj")
        }))
        .await;
    assert_eq!(fired(&outcome), vec![delivery("pj", vec![
        ints(&[1, 1]),
        int(2)
    ])]);
    let mut after = Vec::new();
    for prefix in [
        elements(&[]),
        elements(&[0]),
        elements(&[0, 1]),
        elements(&[4]),
    ] {
        after.push(rig.marker_datums(&prefix).await);
    }
    for (before, after) in before.iter().zip(&after) {
        let mut before: Vec<Vec<u8>> = before
            .iter()
            .map(|datum| datum.source.hash.0.clone())
            .collect();
        let mut after: Vec<Vec<u8>> = after
            .iter()
            .map(|datum| datum.source.hash.0.clone())
            .collect();
        before.sort();
        after.sort();
        assert_eq!(before, after, "the peek join restores every marker");
    }
    rig.assert_marker_invariant(&[elements(&[0, 1]), elements(&[4])], "a peek join")
        .await;
}

/// The window of `need` binds on a prefix is the first `need` entries of each path under it
/// (open point 4). On a queue, binds on the nested prefixes `[0]` and `[0,1]` see the head of
/// each path. Two binds on `[0]` see the first two entries of each path, so they bind the two
/// records of `[0,2]` although `[0,1]` comes first.
#[tokio::test]
async fn pathmap_join_window_takes_need_entries_of_each_path() {
    let rig = Rig::new(SpaceInner::Queue).await;
    for (at, value) in [(&[0, 1][..], 1), (&[0, 1], 2), (&[0, 2], 3)] {
        rig.ok(put(at, value)).await;
    }
    let outcome = rig
        .ok(joined(join(
            &[(&[0], under(&[2])), (&[0, 1], any())],
            "nested",
        )))
        .await;
    assert_eq!(fired(&outcome), vec![delivery("nested", vec![
        int(3),
        int(1)
    ])]);
    assert_eq!(rig.records(&[0, 1]).await, vec![vec![int(2)]]);

    let rig = Rig::new(SpaceInner::Queue).await;
    for (at, value) in [(&[0, 1][..], 1), (&[0, 1], 2), (&[0, 1], 9)] {
        rig.ok(put(at, value)).await;
    }
    let outcome = rig
        .ok(joined(join(
            &[(&[0], exactly_list(&[1, 2])), (&[0, 1], exactly(1))],
            "heads",
        )))
        .await;
    assert!(
        outcome.fires.is_empty(),
        "the bind on [0] sees only the head 1 of [0,1], so the join parks"
    );

    let rig = Rig::new(SpaceInner::Queue).await;
    for (at, value) in [(&[0, 1][..], 1), (&[0, 1], 2), (&[0, 2], 3), (&[0, 2], 4)] {
        rig.ok(put(at, value)).await;
    }
    let outcome = rig
        .ok(joined(join(
            &[(&[0], under(&[2])), (&[0], under(&[2]))],
            "pair",
        )))
        .await;
    assert_eq!(fired(&outcome), vec![delivery("pair", vec![
        int(3),
        int(4)
    ])]);
    assert_eq!(rig.records(&[0, 1]).await, vec![vec![int(1)], vec![int(2)]]);
    rig.assert_marker_invariant(&[elements(&[0, 1]), elements(&[0, 2])], "the window")
        .await;
}

// ─── Examples: the cap (O-2) ─────────────────────────────────────────────────────────────────

/// A transaction that stops at `FIRE_ROUND_CAP` with a second send in flight restores the owed
/// marker before it commits (open item O-2). So every job of the deploy sees the full marker
/// invariant. The settle job carries the offer of the datum, and the chain fires every waiter
/// once.
#[tokio::test]
async fn pathmap_cap_restores_owed_markers() {
    let rig = Rig::new(SpaceInner::Bag).await;
    let waiters = FIRE_ROUND_CAP + 6;
    for index in 0..waiters {
        rig.ok(joined(JoinOp {
            peek: true,
            ..join(&[(&[0], any()), (&[5], any())], &format!("w{index}"))
        }))
        .await;
    }
    rig.ok(put(&[0, 1], 1)).await;
    let at = elements(&[0, 1]);
    let mut stored = Vec::new();
    for prefix in prefixes_of(&at) {
        stored.push(rig.marker_datums(&prefix).await);
    }
    let first = rig.ok(put(&[5], 2)).await;
    assert_eq!(
        first.fires.len(),
        FIRE_ROUND_CAP,
        "one fire count for the transaction"
    );
    for (depth, prefix) in prefixes_of(&at).iter().enumerate() {
        assert_eq!(
            rig.marker_datums(prefix).await,
            stored[depth],
            "the capped transaction restores the marker of depth {depth} with its bytes"
        );
    }
    let reissue = first.reissue.clone().expect("the cap re-issues");
    assert_eq!(reissue.cause, ReissueCause::RoundCap);
    let SpaceOp::Settle(settle) = &reissue.op else {
        panic!(
            "a cap with owed work re-issues a settle job: {:?}",
            reissue.op
        );
    };
    let cascade = settle
        .cascade
        .as_ref()
        .expect("the settle carries the cascade");
    assert!(
        cascade
            .pending
            .iter()
            .any(|pending| pending.restore.is_some()),
        "the settle carries the offer of the peeked datum"
    );
    rig.assert_marker_invariant(&[elements(&[0, 1]), elements(&[5])], "after the capped job")
        .await;
    let rest = rig.chain(reissue.op).await;
    let total = FIRE_ROUND_CAP + all_fires(&rest).len();
    assert_eq!(total, waiters, "every waiter fires once");
    assert_eq!(rig.records(&[0, 1]).await, vec![vec![int(1)]]);
    assert_eq!(rig.records(&[5]).await, vec![vec![int(2)]]);
    rig.assert_marker_invariant(&[elements(&[0, 1]), elements(&[5])], "after the chain")
        .await;
    // The markers of the datum of [0,1] keep their bytes. The root also holds the marker of the
    // record of [5], which the chain stored.
    let source = rig.datums(&at).await[0].source.hash.clone();
    for (depth, prefix) in prefixes_of(&at).iter().enumerate() {
        let markers: Vec<Datum<ListParWithRandom>> = rig
            .marker_datums(prefix)
            .await
            .into_iter()
            .filter(|datum| decode_marker(&datum.a).expect("a marker").is_of(&source))
            .collect();
        assert_eq!(markers, stored[depth], "{depth}");
    }
}

/// Decision 43 at the cap on head-only disciplines: a persistent send that fires more waiters
/// than `FIRE_ROUND_CAP` stops, and the settle job of its cascade carries the send as its resume
/// with the order of its first round. The re-issued send fires the waiters that are left and
/// then stores its record at that order, below the token. Every waiter fires once.
#[tokio::test]
async fn pathmap_cap_reissues_a_persistent_send_at_its_place() {
    for inner in [SpaceInner::Queue, SpaceInner::Stack] {
        let rig = Rig::new(inner).await;
        let waiters = FIRE_ROUND_CAP + 6;
        for index in 0..waiters {
            rig.ok(take(&[0], &format!("w{index}"), any())).await;
        }
        let first_ord = rig.token().await;
        let outcomes = rig.chain(put_forever(&[0, 1], 5)).await;
        assert_eq!(
            outcomes[0].fires.len(),
            FIRE_ROUND_CAP,
            "{inner:?}: the cap"
        );
        let reissue = outcomes[0].reissue.as_ref().expect("the cap re-issues");
        assert_eq!(reissue.cause, ReissueCause::RoundCap);
        let resumed = match &reissue.op {
            SpaceOp::Settle(settle) => settle
                .cascade
                .as_ref()
                .and_then(|cascade| cascade.resume.as_ref())
                .map(|resume| resume.op.clone()),
            other => Some(other.clone()),
        };
        let Some(SpaceOp::Produce(resumed)) = resumed else {
            panic!("{inner:?}: the send resumes after the cap");
        };
        assert_eq!(
            resumed.reissued_ord,
            Some(first_ord),
            "{inner:?}: the order of the first round"
        );
        assert!(outcomes
            .iter()
            .all(|outcome| outcome.fires.len() <= FIRE_ROUND_CAP));
        assert_eq!(
            all_fires(&outcomes).len(),
            waiters,
            "{inner:?}: every waiter fires once"
        );
        let stored: Vec<i64> = rig
            .datums(&elements(&[0, 1]))
            .await
            .iter()
            .map(|datum| {
                record::unwrap_pq_datum(&datum.a)
                    .expect("a datum record")
                    .ord
            })
            .collect();
        assert_eq!(
            stored,
            vec![first_ord],
            "{inner:?}: the record keeps its first order"
        );
        assert!(rig.token().await > first_ord);
        assert_eq!(rig.waiters(&[0]).await, 0);
        rig.assert_marker_invariant(&[elements(&[0, 1])], &format!("{inner:?}"))
            .await;
    }
}

/// O-2, the stale offer: a settle job carries the offer of a datum that another job of the deploy
/// spent first. The offer meets no waiter and stores nothing, so the datum stays spent, none of
/// its markers comes back, and the marker invariant holds.
#[tokio::test]
async fn pathmap_cap_drops_a_stale_carried_offer() {
    let rig = Rig::new(SpaceInner::Bag).await;
    for index in 0..FIRE_ROUND_CAP + 2 {
        rig.ok(joined(JoinOp {
            peek: true,
            ..join(&[(&[0], any()), (&[5], any())], &format!("w{index}"))
        }))
        .await;
    }
    rig.ok(put(&[0, 1], 1)).await;
    let first = rig.ok(put(&[5], 2)).await;
    assert_eq!(first.fires.len(), FIRE_ROUND_CAP);
    let reissue = first.reissue.clone().expect("the cap re-issues");
    let spent = rig.ok(take(&[0, 1], "spend", any())).await;
    assert_eq!(fired(&spent), vec![delivery("spend", vec![int(1)])]);
    rig.assert_marker_invariant(&[elements(&[0, 1]), elements(&[5])], "after the spend")
        .await;
    let rest = rig.chain(reissue.op).await;
    assert!(
        all_fires(&rest).is_empty(),
        "no datum under [0] is left to fire a join"
    );
    assert!(
        rig.records(&[0, 1]).await.is_empty(),
        "the stale offer stores nothing"
    );
    assert_eq!(rig.records(&[5]).await, vec![vec![int(2)]]);
    rig.assert_marker_invariant(&[elements(&[0, 1]), elements(&[5])], "after the chain")
        .await;
}

// ─── Merges ──────────────────────────────────────────────────────────────────────────────────

/// The merge index of the branch log `log` over the pre-state `base` of `host`, with the
/// pre-state closures of `block_index.rs::create_event_log_index`.
fn merge_index(host: &ParRSpace, base: &Blake2b256Hash, log: &[Event]) -> EventLogIndex {
    let reader = host
        .get_history_repository()
        .get_history_reader(base)
        .expect("read the history at the pre-state root");
    let produce_exists_in_pre_state = |produce: &Produce| {
        reader
            .get_data(&produce.channel_hash)
            .is_ok_and(|data| data.iter().any(|datum| datum.source == *produce))
    };
    let produce_touches_pre_state_join = |produce: &Produce| {
        reader
            .get_joins(&produce.channel_hash)
            .is_ok_and(|joins| joins.iter().any(|join| join.len() > 1))
    };
    EventLogIndex::new(
        log.to_vec(),
        produce_exists_in_pre_state,
        produce_touches_pre_state_join,
        NumberChannelsDiff::new(),
    )
}

/// `log` without the events on the channel with the hash `channel`.
fn without_channel(log: &[Event], channel: &Blake2b256Hash) -> Vec<Event> {
    log.iter()
        .filter(|event| match event {
            Event::IoEvent(IOEvent::Produce(produce)) => produce.channel_hash != *channel,
            Event::IoEvent(IOEvent::Consume(consume)) => !consume.channel_hashes.contains(channel),
            Event::Comm(comm) => !comm.consume.channel_hashes.contains(channel),
        })
        .cloned()
        .collect()
}

/// The two branch logs of `left` and `right` from the checkpoint `base` of `rig`: each branch
/// resets to the base and runs its operations.
async fn branches(
    rig: &Rig,
    base: &Blake2b256Hash,
    left: Vec<SpaceOp>,
    right: Vec<SpaceOp>,
) -> (Vec<Event>, Vec<Event>) {
    let mut logs = Vec::with_capacity(2);
    for ops in [left, right] {
        rig.host.reset(base).await.expect("the reset to the base");
        rig.log().await;
        for op in ops {
            rig.ok(op).await;
        }
        logs.push(rig.log().await);
    }
    let right = logs.pop().expect("the right log");
    let left = logs.pop().expect("the left log");
    (left, right)
}

/// The conflicting channels of the branch logs `left` and `right` over `base`.
fn conflicting(
    rig: &Rig,
    base: &Blake2b256Hash,
    left: &[Event],
    right: &[Event],
) -> BTreeSet<Blake2b256Hash> {
    conflicts(
        &merge_index(&rig.host, base, left),
        &merge_index(&rig.host, base, right),
    )
    .0
    .into_iter()
    .collect()
}

/// Merge check #2 sees a prefix contact on the shared index channel (`prefix_contact_anchored`,
/// MergeDetectsPrefixContact). A datum on `[0,1,2]` in one branch and a waiter on `[0,1]` in the
/// other meet on `I([0,1])`, in both directions. Both branches change the space, so they also race
/// on the token in check #1 (decision 51). Without the token events the contact still conflicts
/// on `I([0,1])` alone. A waiter on `[0,5]`, which is not a prefix of the path, merges.
#[tokio::test]
async fn pathmap_merge_prefix_contact_conflicts() {
    let rig = Rig::new(SpaceInner::Bag).await;
    rig.ok(put(&[9], 0)).await;
    let base = rig.root().await;
    let index = hash(&index_at(&elements(&[0, 1])));
    let token = hash(&token());
    for (left, right) in [
        (vec![put(&[0, 1, 2], 1)], vec![take(&[0, 1], "w", any())]),
        (vec![take(&[0, 1], "w", any())], vec![put(&[0, 1, 2], 1)]),
    ] {
        let (left, right) = branches(&rig, &base, left, right).await;
        assert_eq!(
            conflicting(&rig, &base, &left, &right),
            BTreeSet::from([index.clone(), token.clone()])
        );
        let (left, right) = (
            without_channel(&left, &token),
            without_channel(&right, &token),
        );
        assert_eq!(
            conflicting(&rig, &base, &left, &right),
            BTreeSet::from([index.clone()]),
            "the contact conflicts without the token"
        );
        assert!(conflict_reason(
            &merge_index(&rig.host, &base, &left),
            &merge_index(&rig.host, &base, &right)
        )
        .is_some_and(|reason| reason.starts_with("potentialCOMMs")));
    }
    let (left, right) = branches(&rig, &base, vec![put(&[0, 1, 2], 1)], vec![take(
        &[0, 5],
        "w",
        any(),
    )])
    .await;
    assert_eq!(
        conflicting(&rig, &base, &left, &right),
        BTreeSet::from([token.clone()])
    );
    assert!(conflicting(
        &rig,
        &base,
        &without_channel(&left, &token),
        &without_channel(&right, &token)
    )
    .is_empty());
}

// ─── Determinism: replay, restart and the lock keys ──────────────────────────────────────────

/// The disciplines of the replay and restart tests.
const INNERS: [SpaceInner; 6] = [
    SpaceInner::Bag,
    SpaceInner::Set,
    SpaceInner::Cell,
    SpaceInner::Queue,
    SpaceInner::Stack,
    SpaceInner::PriorityQueue,
];

/// The script of the replay and restart tests: waiters that park on prefixes, sends that fire
/// them and store, peeks, a join over prefixes on both sides, persistent sends, a refused guard
/// and the root prefix. A cell fails some operations with `CellOccupied`, and a failed operation
/// replays as the same failure.
fn pathmap_script() -> Vec<SpaceOp> {
    vec![
        take(&[0], "w0", any()),
        put(&[0, 1], 1),
        put(&[0, 1], 2),
        put(&[0, 2], 3),
        peek(&[0], "p", any()),
        joined(join(&[(&[0], any()), (&[5], any())], "j1")),
        put(&[5, 1], 4),
        take(&[0, 2], "t", any()),
        put_forever(&[6], 7),
        take(&[6], "f", any()),
        joined(JoinOp {
            guard: Some(refusing()),
            ..join(&[(&[0], any()), (&[6], any())], "g")
        }),
        put(&[0, 3], 8),
        joined(join(&[(&[0, 1], any()), (&[0], any())], "nested")),
        put(&[], 9),
        take(&[], "r", any()),
        take_forever(&[7], "forever", any()),
        put(&[7, 1], 10),
        put(&[7, 2], 11),
    ]
}

/// The paths that `script` names: the path of the channel of each operation and of each bind.
fn script_paths(script: &[SpaceOp]) -> Vec<Vec<Par>> {
    let mut paths: Vec<Vec<Par>> = Vec::new();
    let mut add = |channel: &Par| {
        let elements = match &channel.exprs.as_slice() {
            [Expr {
                expr_instance: Some(ExprInstance::EListBody(list)),
            }] => list.ps.clone(),
            _ => vec![channel.clone()],
        };
        if !paths.contains(&elements) {
            paths.push(elements);
        }
    };
    for op in script {
        match op {
            SpaceOp::Produce(op) => add(&op.channel),
            SpaceOp::Consume(op) => add(&op.channel),
            SpaceOp::Settle(op) => add(&op.channel),
            SpaceOp::Join(op) => op.binds.iter().for_each(|bind| add(&bind.channel)),
        }
    }
    paths
}

/// What one job of a replay script shows: its result, its legacy log and its phlo.
type Metered = (Result<SpaceOutcome, SpaceError>, Vec<Cost>, i64);

/// The phlo of a legacy budget: far above every script of the suite.
const LEGACY_PHLO: i64 = 1_000_000_000_000;

/// Runs one job of `op` over the host of `env` with a legacy budget of its own (decision 45,
/// Option P).
async fn run_job(env: &SpaceEnv, op: SpaceOp) -> Metered {
    let budget = RuntimeBudget::new(Cost::create(LEGACY_PHLO, "rrs pathmap replay step"));
    let _scope = budget.enter_legacy_scope();
    let env = SpaceEnv {
        budget: budget.clone(),
        ..env.clone()
    };
    let before = budget.get().value;
    let result = run(&FamilyAgent, &env, op).await;
    (result, budget.get_log(), before - budget.get().value)
}

/// Runs each operation of `script` and every re-issue that follows it, as the jobs of a deploy,
/// and returns what each job shows and the operation of each job.
async fn run_jobs(env: &SpaceEnv, script: &[SpaceOp]) -> (Vec<Metered>, Vec<SpaceOp>) {
    let mut steps = Vec::with_capacity(script.len());
    let mut jobs = Vec::with_capacity(script.len());
    for op in script {
        let mut next = Some(op.clone());
        while let Some(op) = next.take() {
            jobs.push(op.clone());
            let step = run_job(env, op).await;
            next = match &step.0 {
                Ok(outcome) => outcome.reissue.as_ref().map(|reissue| reissue.op.clone()),
                Err(_) => None,
            };
            steps.push(step);
        }
    }
    (steps, jobs)
}

/// One case of `pathmap_replay_equal`: play runs `script` with its re-issues, and replay runs
/// the same jobs with the checkpoint log of the undrained play run.
async fn replay_case(inner: SpaceInner, script: Vec<SpaceOp>, label: &str) {
    let (play_space, replay_space) = play_and_replay().await;
    let play_rig = Rig::over(play_space.clone(), inner).await;
    let start = play_space
        .create_checkpoint()
        .await
        .expect("a checkpoint after the creation")
        .root;
    let (played, jobs) = run_jobs(&play_rig.env, &script).await;
    assert!(
        played.iter().any(|(result, _, _)| result
            .as_ref()
            .is_ok_and(|outcome| !outcome.fires.is_empty())),
        "{label}: the script fires"
    );
    let Checkpoint { root, log } = play_space.create_checkpoint().await.expect("a checkpoint");
    replay_space
        .rig_and_reset(start.clone(), log)
        .await
        .expect("the replay rigs the play log");
    let replay_env = env_over(replay_space.clone(), &budget());
    let mut replayed = Vec::with_capacity(jobs.len());
    for op in &jobs {
        replayed.push(run_job(&replay_env, op.clone()).await);
    }
    assert_eq!(
        replayed, played,
        "{label}: the results, the legacy logs and the costs"
    );
    replay_space.take_event_log().await;
    replay_space
        .check_replay_data()
        .await
        .expect("every recorded COMM is paired");
    let replay_root = replay_space
        .create_checkpoint()
        .await
        .expect("a checkpoint")
        .root;
    assert_eq!(replay_root, root, "{label}: the root");

    replay_space
        .rig_and_reset(start, Vec::new())
        .await
        .expect("the replay rigs an empty log");
    let mut diverged = false;
    for op in &jobs {
        if let Err(SpaceError::ReplayDivergence(_)) =
            run(&FamilyAgent, &replay_env, op.clone()).await
        {
            diverged = true;
            break;
        }
    }
    assert!(diverged, "{label}: a replay without the log diverges");
}

/// Replay stages the transactions of play (decision 36). For each discipline the PathMap script
/// replays with the checkpoint log of an undrained play run: the results, the legacy logs, the
/// phlo and the root are equal, and every recorded COMM is paired. The capped chain of
/// `pathmap_cap_restores_owed_markers` replays too, with its settle job and the restore of its
/// offer. A replay without the log diverges at the first COMM.
#[tokio::test]
async fn pathmap_replay_equal() {
    for inner in INNERS {
        replay_case(inner, pathmap_script(), &format!("{inner:?}")).await;
    }
    let mut capped = Vec::new();
    for index in 0..FIRE_ROUND_CAP + 3 {
        capped.push(joined(JoinOp {
            peek: true,
            ..join(&[(&[0], any()), (&[5], any())], &format!("w{index}"))
        }));
    }
    capped.push(put(&[0, 1], 1));
    capped.push(put(&[5], 2));
    replay_case(SpaceInner::Bag, capped, "the capped chain").await;
}

/// The rows of `host` on the channels of the paths `paths`: for each prefix its markers, its
/// single-bind waiters, its join waiters and its join groups, and for each path its records,
/// all as sorted bytes, and the token.
async fn entry_set(host: &ParRSpace, paths: &[Vec<Par>]) -> Vec<Vec<Vec<u8>>> {
    let mut channels: Vec<Par> = vec![token()];
    for path in paths {
        for prefix in prefixes_of(path) {
            let index = index_at(&prefix);
            if !channels.contains(&index) {
                channels.push(index);
            }
        }
        channels.push(data_at(path));
    }
    let mut rows = Vec::with_capacity(channels.len() * 3);
    for channel in &channels {
        let mut data: Vec<Vec<u8>> = host
            .get_data(channel)
            .await
            .iter()
            .map(|datum| bincode::serialize(datum).expect("a datum serializes"))
            .collect();
        data.sort();
        rows.push(data);
        let mut joins = host.get_joins(channel.clone()).await;
        joins.sort_by_key(|group| bincode::serialize(group).expect("a group serializes"));
        let mut waiters: Vec<Vec<u8>> = Vec::new();
        for group in &joins {
            for waiter in host.get_waiting_continuations(group.clone()).await {
                waiters.push(bincode::serialize(&waiter).expect("a waiter serializes"));
            }
        }
        waiters.sort();
        rows.push(waiters);
        rows.push(
            joins
                .iter()
                .map(|group| bincode::serialize(group).expect("a group serializes"))
                .collect(),
        );
    }
    rows
}

/// Runs `script` in `rig` and returns the result of each operation.
async fn run_script(rig: &Rig, script: &[SpaceOp]) -> Vec<Result<SpaceOutcome, SpaceError>> {
    let mut results = Vec::with_capacity(script.len());
    for op in script {
        results.push(rig.run(op.clone()).await);
    }
    results
}

/// The records, the markers, the waiters and the join groups of a PathMap space survive a
/// checkpoint and a reset. After the reset the store holds the entries of a twin store that did
/// not reset, and the next operations give the same results and the same root.
#[tokio::test]
async fn pathmap_restart() {
    let script = pathmap_script();
    let paths = script_paths(&script);
    for inner in INNERS {
        let (built, next) = script.split_at(10);
        let restarted = Rig::new(inner).await;
        let twin = Rig::new(inner).await;
        for rig in [&restarted, &twin] {
            run_script(rig, built).await;
        }
        let root = restarted.root().await;
        assert_eq!(twin.root().await, root, "{inner:?}: one state");
        let before = entry_set(&restarted.host, &paths).await;
        assert!(
            before.iter().any(|row| !row.is_empty()),
            "{inner:?}: the state holds entries"
        );
        restarted
            .host
            .reset(&root)
            .await
            .expect("the reset to the checkpoint");
        assert_eq!(
            entry_set(&restarted.host, &paths).await,
            before,
            "{inner:?}: the reset keeps every entry"
        );
        let after = run_script(&restarted, next).await;
        assert_eq!(
            after,
            run_script(&twin, next).await,
            "{inner:?}: the results"
        );
        assert_eq!(
            restarted.root().await,
            twin.root().await,
            "{inner:?}: the root"
        );
        restarted
            .assert_marker_invariant(&paths, &format!("{inner:?} after the restart"))
            .await;
    }
}

/// The escalation of a PathMap operation (joins plan section 9): the first transaction reads the
/// config and stages nothing, and the second transaction locks the keys of the prefix scope.
/// Every channel that a job stages lies in the lock keys of its transaction, for every
/// discipline and every operation of the script. One transaction commits for each job.
#[tokio::test]
async fn pathmap_staging_stays_in_the_lock_keys() {
    for inner in INNERS {
        let host = play().await;
        let (recording, record) = RecordingHost::new(host.clone());
        let env = env_over(recording, &budget());
        let config = config_of(
            SPACE,
            SpaceOuter::PathMap.as_str(),
            inner.as_str(),
            SpaceQualifier::Default,
        );
        create(&FamilyAgent, &env, creation(SPACE, config, family(inner))).await;
        for op in pathmap_script() {
            *record.lock().expect("the record lock") = HostRecord::default();
            let label = format!("{inner:?} {op:?}");
            let result = run(&FamilyAgent, &env, op).await;
            let seen = std::mem::take(&mut *record.lock().expect("the record lock"));
            assert_eq!(seen.begins.len(), 2, "{label}: one escalation");
            assert!(
                seen.staged.iter().all(|(txn, _)| *txn == 1),
                "{label}: the first transaction stages nothing"
            );
            for (txn, staged) in &seen.staged {
                for channel in staged.channels() {
                    assert!(
                        seen.begins[*txn].contains(&channel),
                        "{label}: a staged channel lies in the lock keys"
                    );
                }
            }
            match result {
                Ok(_) => assert_eq!(seen.commits, vec![1], "{label}: one commit"),
                Err(_) => assert!(
                    seen.commits.is_empty(),
                    "{label}: a failure commits nothing"
                ),
            }
        }
    }
}

// ─── Option P ────────────────────────────────────────────────────────────────────────────────

/// The legacy log entries of the charges of the staged operation `staged` (decision 45, Option
/// P): the pre-charge, then the post-charges.
fn charges_of(staged: &StagedCopy) -> Vec<Cost> {
    split(match staged {
        StagedCopy::Produce(op) => produce_charges(op).expect("the charges of a staged produce"),
        StagedCopy::Consume(op) => consume_charges(op).expect("the charges of a staged consume"),
    })
}

/// The records that a [`StagedBindingMatcher`] binds, each with the values that the staged
/// operation bound from it, or `None` before the pin opens.
type Bindings = Arc<Mutex<Vec<(ListParWithRandom, ListParWithRandom)>>>;

/// The matcher of a lone host call of a PathMap space. A PathMap waiter binds the re-rooted
/// payload of the datum of a marker, and a vehicle carries the marker before the payload, so a
/// matcher of the record alone cannot bind them (`support::legacy::PinnedPayloadMatcher` matches
/// the payload of a datum envelope). This matcher binds exactly the records of the staged
/// operation, each with the values that the agent bound from it. The guard check is the check of
/// the Rholang matcher.
#[derive(Clone)]
struct StagedBindingMatcher {
    bindings: Bindings,
}

impl Match<BindPattern, ListParWithRandom, TaggedContinuation> for StagedBindingMatcher {
    fn get(&self, _pattern: &BindPattern, data: &ListParWithRandom) -> Option<ListParWithRandom> {
        self.bindings
            .lock()
            .expect("the bindings lock")
            .iter()
            .find(|(record, _)| record == data)
            .map(|(_, matched)| matched.clone())
    }

    fn check_commit(
        &self,
        continuation: &TaggedContinuation,
        matched: &[ListParWithRandom],
    ) -> bool {
        Matcher.check_commit(continuation, matched)
    }
}

/// A `ChargingRSpace` over a fresh in-memory space with the matcher of `bindings`, which charges
/// `budget`.
async fn binding_host(
    budget: &RuntimeBudget,
    bindings: &Bindings,
) -> impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation> {
    let mut stores = InMemoryStoreManager::new();
    let space = ParRSpace::create(
        stores.r_space_stores().await.expect("in-memory stores"),
        Arc::new(Box::new(StagedBindingMatcher {
            bindings: Arc::clone(bindings),
        })),
    )
    .expect("an in-memory space");
    let host: RhoISpace = Arc::new(Box::new(space));
    ChargingRSpace::charging_rspace(host, budget.clone())
}

/// The phlo of the budget of a lone host call: far above the charges of one operation.
const LONE_PHLO: i64 = 1_000_000_000_000;

/// Stores the record `candidate` of a fire on its channel of a lone host. The bindings are
/// empty, so the store fires nothing.
async fn store_candidate(
    host: &impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    channel: &Par,
    record: &ListParWithRandom,
    persist: bool,
) {
    let fired = host
        .produce(channel.clone(), record.clone(), persist)
        .await
        .expect("a lone host stores a candidate record");
    assert!(fired.is_none(), "empty bindings fire nothing");
}

/// The legacy log entries of the staged produce `op` when a fresh host runs it alone, as
/// `support::legacy::lone_host_produce` runs it, with the bindings of the staged fire.
async fn lone_pathmap_produce(op: &ParSpaceProduce) -> Vec<Cost> {
    let budget = RuntimeBudget::new(Cost::create(LONE_PHLO, "rrs lone pathmap produce"));
    let _scope = budget.enter_legacy_scope();
    let bindings: Bindings = Arc::new(Mutex::new(Vec::new()));
    let host = binding_host(&budget, &bindings).await;
    let mut bound = Vec::new();
    let fires = match &op.outcome {
        ProduceOutcome::Store => false,
        ProduceOutcome::Fire {
            channels,
            continuation,
            data,
        } => {
            for candidate in data {
                match candidate.datum_index >= 0 {
                    true => {
                        store_candidate(
                            &host,
                            &candidate.channel,
                            &candidate.removed_datum,
                            candidate.datum.persist,
                        )
                        .await
                    }
                    false => assert_eq!(candidate.removed_datum, op.data, "the incoming record"),
                }
                bound.push((candidate.removed_datum.clone(), candidate.datum.a.clone()));
            }
            let parked = host
                .consume(
                    channels.clone(),
                    continuation.patterns.clone(),
                    continuation.continuation.clone(),
                    continuation.persist,
                    continuation.peeks.clone(),
                )
                .await
                .expect("a lone host parks the waiter of a fire");
            assert!(parked.is_none(), "empty bindings park the waiter");
            true
        }
    };
    *bindings.lock().expect("the bindings lock") = bound;
    let run = host_produce(
        &host,
        &budget,
        op.channel.clone(),
        op.data.clone(),
        op.persist,
    )
    .await;
    assert_eq!(
        run.fired.is_some(),
        fires,
        "the lone produce fires as the staged one"
    );
    run.entries
}

/// The legacy log entries of the staged consume `op` when a fresh host runs it alone, as
/// `support::legacy::lone_host_consume` runs it, with the bindings of the staged fire.
async fn lone_pathmap_consume(op: &ParSpaceConsume) -> Vec<Cost> {
    let budget = RuntimeBudget::new(Cost::create(LONE_PHLO, "rrs lone pathmap consume"));
    let _scope = budget.enter_legacy_scope();
    let bindings: Bindings = Arc::new(Mutex::new(Vec::new()));
    let host = binding_host(&budget, &bindings).await;
    let mut bound = Vec::new();
    let fires = match &op.outcome {
        ConsumeOutcome::Park => false,
        ConsumeOutcome::Fire { data } => {
            for candidate in data {
                store_candidate(
                    &host,
                    &candidate.channel,
                    &candidate.removed_datum,
                    candidate.datum.persist,
                )
                .await;
                bound.push((candidate.removed_datum.clone(), candidate.datum.a.clone()));
            }
            true
        }
    };
    *bindings.lock().expect("the bindings lock") = bound;
    let run = host_consume(
        &host,
        &budget,
        op.channels.clone(),
        op.patterns.clone(),
        op.continuation.clone(),
        op.persist,
        !op.peeks.is_empty(),
    )
    .await;
    assert_eq!(
        run.fired.is_some(),
        fires,
        "the lone consume fires as the staged one"
    );
    run.entries
}

/// The shape of a staged operation of the Option P differential.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Shape {
    /// A take or a put of the token.
    Token,
    /// A store of a record or of a marker, or the restore of a marker.
    Store,
    /// A single-bind waiter that parks on a prefix.
    Park,
    /// A join waiter that parks on the index channels of its prefixes.
    JoinPark,
    /// A consume over two or more index channels: a join fire or a class fire `[I(q); n]`.
    GroupConsume,
    /// A produce-side fire of a join waiter with the vehicle.
    GroupFire,
    /// A produce-side fire of a single-bind waiter with the vehicle.
    SingleFire,
    /// A single-bind consume fire: a receive, a `K_retire` or a `K_lift`.
    Removal,
}

/// The shape of the staged operation `staged`.
fn shape_of(staged: &StagedCopy) -> Shape {
    let token = token();
    match staged {
        StagedCopy::Produce(op) if op.channel == token => Shape::Token,
        StagedCopy::Consume(op) if op.channels.contains(&token) => Shape::Token,
        StagedCopy::Produce(op) => match &op.outcome {
            ProduceOutcome::Store => Shape::Store,
            ProduceOutcome::Fire { channels, .. } if channels.len() >= 2 => Shape::GroupFire,
            ProduceOutcome::Fire { .. } => Shape::SingleFire,
        },
        StagedCopy::Consume(op) => match (&op.outcome, op.channels.len() >= 2) {
            (ConsumeOutcome::Park, true) => Shape::JoinPark,
            (ConsumeOutcome::Park, false) => Shape::Park,
            (ConsumeOutcome::Fire { .. }, true) => Shape::GroupConsume,
            (ConsumeOutcome::Fire { .. }, false) => Shape::Removal,
        },
    }
}

/// Runs the operations `ops` on a fresh recording rig of `inner` and checks Option P (decision
/// 45) for each job: its legacy log is the concatenation of the charges of its committed staged
/// operations, in staging order, and a fresh host that runs one staged operation alone logs
/// exactly its charges. It returns the shapes of all staged operations.
async fn option_p_scenario(inner: SpaceInner, ops: Vec<SpaceOp>) -> BTreeSet<Shape> {
    let host = play().await;
    let (recording, record) = RecordingHost::new(host.clone());
    let env = env_over(recording, &budget());
    let config = config_of(
        SPACE,
        SpaceOuter::PathMap.as_str(),
        inner.as_str(),
        SpaceQualifier::Default,
    );
    create(&FamilyAgent, &env, creation(SPACE, config, family(inner))).await;
    *record.lock().expect("the record lock") = HostRecord::default();
    let mut shapes = BTreeSet::new();
    for op in ops {
        let budget = RuntimeBudget::new(Cost::create(LEGACY_PHLO, "rrs pathmap option p"));
        let _scope = budget.enter_legacy_scope();
        let env = SpaceEnv {
            budget: budget.clone(),
            ..env.clone()
        };
        let label = format!("{op:?}");
        run(&FamilyAgent, &env, op)
            .await
            .unwrap_or_else(|error| panic!("the operation commits: {error:?}: {label}"));
        let seen = std::mem::take(&mut *record.lock().expect("the record lock"));
        let staged: Vec<&StagedCopy> = seen
            .staged
            .iter()
            .filter(|(txn, _)| seen.commits.contains(txn))
            .map(|(_, staged)| staged)
            .collect();
        let expected: Vec<Cost> = staged
            .iter()
            .flat_map(|staged| charges_of(staged))
            .collect();
        assert_eq!(budget.get_log(), expected, "the legacy log of {label}");
        for staged in staged {
            let lone = match staged {
                StagedCopy::Produce(op) => lone_pathmap_produce(op).await,
                StagedCopy::Consume(op) => lone_pathmap_consume(op).await,
            };
            assert_eq!(lone, charges_of(staged), "a lone host call of {label}");
            shapes.insert(shape_of(staged));
        }
    }
    shapes
}

/// Option P for PathMap spaces (decision 45). Every operation that the prefix source stages
/// charges what `ChargingRSpace` charges for the equal host call: the stores of records and
/// markers, the restore of a peeked marker, the parks on prefixes, the deliveries with the
/// vehicle, the `K_retire` of a spent datum, the class fire of a set, a join over prefixes, a
/// cascade with its `K_lift`, and the token.
#[tokio::test]
async fn pathmap_legacy_charges_equal_host() {
    let bag = option_p_scenario(SpaceInner::Bag, vec![
        put(&[0, 1], 1),
        peek(&[0], "p", any()),
        take(&[0], "k", any()),
        take(&[3], "w", any()),
        put(&[3, 1], 2),
        joined(join(&[(&[4], any()), (&[5], any())], "j")),
        put(&[4], 3),
        put(&[5, 2], 4),
    ])
    .await;
    for shape in [
        Shape::Token,
        Shape::Store,
        Shape::Park,
        Shape::JoinPark,
        Shape::GroupFire,
        Shape::SingleFire,
        Shape::Removal,
    ] {
        assert!(bag.contains(&shape), "the bag stages {shape:?}: {bag:?}");
    }
    let set = option_p_scenario(SpaceInner::Set, vec![
        put_salted(&[0, 1], 1, 1),
        put_salted(&[0, 1], 1, 2),
        take(&[0], "k", any()),
    ])
    .await;
    assert!(
        set.contains(&Shape::GroupConsume),
        "the set stages a class fire: {set:?}"
    );
    let queue = option_p_scenario(SpaceInner::Queue, vec![
        put(&[0, 1], 1),
        put(&[0, 1], 2),
        take(&[0], "second", exactly_list(&[1, 2])),
        take(&[0, 1], "first", any()),
        joined(join(&[(&[7], any()), (&[8], any())], "j")),
        put(&[7], 5),
        put(&[8], 6),
    ])
    .await;
    for shape in [
        Shape::Token,
        Shape::Removal,
        Shape::SingleFire,
        Shape::GroupFire,
    ] {
        assert!(
            queue.contains(&shape),
            "the queue stages {shape:?}: {queue:?}"
        );
    }
}

// ─── Properties (invariant map, P5 rows 114 to 119) ──────────────────────────────────────────

/// The pattern of a generated receive.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Pat {
    /// One free variable: every delivered value.
    Any,
    /// Exactly the value `R(s, v)` that a datum of the value `v` delivers under the suffix `s`.
    Exact(Vec<i64>, i64),
}

impl Pat {
    fn pattern(&self) -> BindPattern {
        match self {
            Pat::Any => any(),
            Pat::Exact(suffix, value) => BindPattern {
                patterns: vec![delivered(&elements(suffix), *value)],
                remainder: None,
                free_count: 0,
            },
        }
    }
}

/// The value that a datum of the number `value` delivers under the suffix `suffix`:
/// `R(ε, v) = v`, and `R(s, v) = [s…, v]` otherwise.
fn delivered(suffix: &[Par], value: i64) -> Par {
    match suffix.is_empty() {
        true => int(value),
        false => {
            let mut ps = suffix.to_vec();
            ps.push(int(value));
            list(ps)
        }
    }
}

/// A generated operation of the properties.
#[derive(Clone, Debug)]
enum PathOp {
    /// A send of `value` on the path `at`.
    Send {
        at: Vec<i64>,
        value: i64,
        persistent: bool,
    },
    /// A single-bind receive on the prefix `at`.
    Receive {
        at: Vec<i64>,
        pat: Pat,
        peek: bool,
        persistent: bool,
    },
}

impl PathOp {
    /// The space operation of the generated operation at the position `index` of its script.
    /// The position salts the random state of a send and names the body of a receive, so every
    /// record and every waiter of a script has an identity of its own.
    fn op(&self, index: usize) -> SpaceOp {
        let salt = u64::try_from(index).expect("a small index");
        match self {
            PathOp::Send {
                at,
                value,
                persistent,
            } => SpaceOp::Produce(ProduceOp {
                persistent: *persistent,
                ..send_values(&path(at), vec![int(*value)], salt)
            }),
            PathOp::Receive {
                at,
                pat,
                peek,
                persistent,
            } => SpaceOp::Consume(ConsumeOp {
                peek: *peek,
                persistent: *persistent,
                ..receive(&path(at), &format!("w{index}"), pat.pattern())
            }),
        }
    }

    /// The paths that the operation names.
    fn paths(&self) -> Vec<Vec<i64>> {
        match self {
            PathOp::Send { at, .. } | PathOp::Receive { at, .. } => vec![at.clone()],
        }
    }
}

/// A path of at most `depth` elements over the two values 0 and 1, so prefixes meet often.
fn arb_path(depth: usize) -> BoxedStrategy<Vec<i64>> { vec(0i64..2, 0..=depth).boxed() }

/// A pattern: any value, or the exact value of a short suffix and a small value.
fn arb_pat() -> BoxedStrategy<Pat> {
    prop_oneof![
        2 => Just(Pat::Any),
        3 => (vec(0i64..2, 0..=2), 0i64..3).prop_map(|(suffix, value)| Pat::Exact(suffix, value)),
    ]
    .boxed()
}

/// A send: persistent with the weight `persistent` out of 8.
fn arb_send(persistent: u32) -> BoxedStrategy<PathOp> {
    (arb_path(3), 0i64..3, 0u32..8)
        .prop_map(move |(at, value, roll)| PathOp::Send {
            at,
            value,
            persistent: roll < persistent,
        })
        .boxed()
}

/// A single-bind receive: a peek with the weight `peek` and persistent with the weight
/// `persistent`, each out of 8.
fn arb_receive(peek: u32, persistent: u32) -> BoxedStrategy<PathOp> {
    (arb_path(2), arb_pat(), 0u32..8, 0u32..8)
        .prop_map(
            move |(at, pat, peek_roll, persistent_roll)| PathOp::Receive {
                at,
                pat,
                peek: peek_roll < peek,
                persistent: persistent_roll < persistent && peek_roll >= peek,
            },
        )
        .boxed()
}

/// A script of single-bind operations: sends and receives, with peeks and persistent sends.
fn arb_single_script(length: std::ops::Range<usize>) -> BoxedStrategy<Vec<PathOp>> {
    vec(prop_oneof![arb_send(2), arb_receive(2, 0)], length).boxed()
}

/// The disciplines of a property.
fn arb_inner(inners: &'static [SpaceInner]) -> BoxedStrategy<SpaceInner> {
    proptest::sample::select(inners.to_vec()).boxed()
}

/// Every path that the script `script` names, with its prefixes.
fn universe(script: &[PathOp]) -> Vec<Vec<Par>> {
    let mut paths: Vec<Vec<Par>> = Vec::new();
    for op in script {
        for at in op.paths() {
            for prefix in prefixes_of(&elements(&at)) {
                if !paths.contains(&prefix) {
                    paths.push(prefix);
                }
            }
        }
    }
    paths
}

/// Runs the operation of `script` at `index` and its re-issues, at most `jobs` jobs, and returns
/// the outcomes. A job that fails (a cell that is occupied) ends the chain.
async fn run_bounded(rig: &Rig, op: SpaceOp, jobs: usize) -> Vec<Result<SpaceOutcome, SpaceError>> {
    let mut outcomes = Vec::new();
    let mut next = Some(op);
    while let Some(op) = next.take() {
        if outcomes.len() == jobs {
            break;
        }
        let result = rig.run(op).await;
        next = match &result {
            Ok(outcome) => outcome.reissue.as_ref().map(|reissue| reissue.op.clone()),
            Err(_) => None,
        };
        outcomes.push(result);
    }
    outcomes
}

/// True when `pattern` matches the delivered value `value`, by the Rholang matcher.
fn matches(pattern: &BindPattern, value: &Par) -> bool {
    Matcher
        .get(pattern, &ListParWithRandom {
            pars: vec![value.clone()],
            ..ListParWithRandom::default()
        })
        .is_some()
}

/// The stored datums under the prefix `prefix` of the universe `paths`, as `(suffix, ord,
/// value)` of each record, path by path.
async fn datums_under(rig: &Rig, paths: &[Vec<Par>], prefix: &[Par]) -> Vec<(Vec<Par>, i64, i64)> {
    let mut found = Vec::new();
    for path in paths {
        if path.len() < prefix.len() || path[..prefix.len()] != *prefix {
            continue;
        }
        for datum in rig.datums(path).await {
            let record = record::unwrap_pq_datum(&datum.a).expect("a datum record");
            let [value] = record.datum.pars.as_slice() else {
                panic!("a generated datum holds one value");
            };
            let ExprInstance::GInt(number) =
                value.exprs[0].expr_instance.clone().expect("a number")
            else {
                panic!("a generated datum is a number");
            };
            found.push((path[prefix.len()..].to_vec(), record.ord, number));
        }
    }
    found
}

/// `delivery_vehicle_single_channel` (R100). Every COMM of a PathMap space is single-channel: its
/// consume is on one channel (the class fire of a set repeats it), and each produce of a COMM is
/// on that channel. No vehicle is ever stored: every record of an index channel is a strict
/// marker, and every record of a data channel is a datum record.
#[test]
fn prop_pathmap_delivery_is_single_channel() {
    const INNERS: &[SpaceInner] = &[
        SpaceInner::Bag,
        SpaceInner::Set,
        SpaceInner::Cell,
        SpaceInner::Queue,
        SpaceInner::Stack,
        SpaceInner::PriorityQueue,
    ];
    check(
        pure_config(),
        (arb_inner(INNERS), arb_single_script(1..10)),
        |(inner, script)| tokio_runtime().block_on(single_channel_case(inner, script)),
    );
}

async fn single_channel_case(inner: SpaceInner, script: Vec<PathOp>) -> Result<(), TestCaseError> {
    let rig = Rig::new(inner).await;
    let paths = universe(&script);
    for (index, op) in script.iter().enumerate() {
        rig.log().await;
        for result in run_bounded(&rig, op.op(index), 4).await {
            if let Err(error) = result {
                prop_assert_eq!(error, SpaceError::CellOccupied, "only a cell fails");
            }
        }
        for event in rig.log().await {
            let Event::Comm(comm) = event else {
                continue;
            };
            let channel = &comm.consume.channel_hashes[0];
            prop_assert!(
                comm.consume
                    .channel_hashes
                    .iter()
                    .all(|other| other == channel),
                "a COMM consumes one channel"
            );
            prop_assert!(
                comm.produces
                    .iter()
                    .all(|produce| produce.channel_hash == *channel),
                "the produces of a COMM lie on its channel"
            );
        }
    }
    for prefix in &paths {
        for datum in rig.marker_datums(prefix).await {
            prop_assert!(decode_marker(&datum.a).is_ok(), "a stored marker is strict");
        }
        for datum in rig.datums(prefix).await {
            prop_assert!(record::unwrap_pq_datum(&datum.a).is_ok(), "a datum record");
        }
    }
    Ok(())
}

/// `no_match_remains` (R103, V23 NoMatchRemains). On a bag and a set, at every quiescent state of
/// any script of sends, receives and peeks, no parked waiter without a guard matches a datum
/// stored under its prefix.
#[test]
fn prop_pathmap_no_match_remains_any_schedule() {
    const INNERS: &[SpaceInner] = &[SpaceInner::Bag, SpaceInner::Set];
    check(
        pure_config(),
        (arb_inner(INNERS), arb_single_script(1..12)),
        |(inner, script)| tokio_runtime().block_on(no_match_case(inner, script)),
    );
}

async fn no_match_case(inner: SpaceInner, script: Vec<PathOp>) -> Result<(), TestCaseError> {
    let rig = Rig::new(inner).await;
    let paths = universe(&script);
    for (index, op) in script.iter().enumerate() {
        let outcomes = run_bounded(&rig, op.op(index), 8).await;
        prop_assert!(
            outcomes
                .last()
                .is_some_and(|last| last.as_ref().is_ok_and(|outcome| outcome.reissue.is_none())),
            "the chain of a bag or a set operation ends"
        );
        for prefix in &paths {
            let waiters = rig
                .host
                .get_waiting_continuations(vec![index_at(prefix)])
                .await;
            let stored = datums_under(&rig, &paths, prefix).await;
            for waiter in &waiters {
                prop_assert!(waiter.continuation.guard.is_none());
                for (suffix, _, value) in &stored {
                    prop_assert!(
                        !matches(&waiter.patterns[0], &delivered(suffix, *value)),
                        "a parked waiter on {:?} matches a datum under it",
                        prefix
                    );
                }
            }
        }
    }
    Ok(())
}

/// `heads_no_match` (R105). On a queue and a stack over prefixes, every operation ends with no
/// fireable head: the head waiter of each prefix matches no head datum of a path under it. The
/// head of a queue is the least order, and the head of a stack the greatest.
#[test]
fn prop_pathmap_head_cascade_leaves_no_fireable_head() {
    const INNERS: &[SpaceInner] = &[SpaceInner::Queue, SpaceInner::Stack];
    check(
        pure_config(),
        (arb_inner(INNERS), arb_single_script(1..12)),
        |(inner, script)| tokio_runtime().block_on(heads_case(inner, script)),
    );
}

/// The head of the orders `ords` on `inner`: the least for a queue, the greatest for a stack.
fn head_of(inner: SpaceInner, ords: impl Iterator<Item = i64>) -> Option<i64> {
    match inner {
        SpaceInner::Stack => ords.max(),
        _ => ords.min(),
    }
}

async fn heads_case(inner: SpaceInner, script: Vec<PathOp>) -> Result<(), TestCaseError> {
    let rig = Rig::new(inner).await;
    let paths = universe(&script);
    for (index, op) in script.iter().enumerate() {
        let outcomes = run_bounded(&rig, op.op(index), 8).await;
        prop_assert!(outcomes
            .last()
            .is_some_and(|last| last.as_ref().is_ok_and(|outcome| outcome.reissue.is_none())));
        for prefix in &paths {
            let waiters = rig
                .host
                .get_waiting_continuations(vec![index_at(prefix)])
                .await;
            let Some(head_ord) = head_of(
                inner,
                waiters.iter().map(|waiter| {
                    record::continuation_ord(&waiter.continuation).expect("a waiter record")
                }),
            ) else {
                continue;
            };
            let head = waiters
                .iter()
                .find(|waiter| {
                    record::continuation_ord(&waiter.continuation).expect("a waiter record")
                        == head_ord
                })
                .expect("the head waiter");
            for path in &paths {
                if path.len() < prefix.len() || path[..prefix.len()] != **prefix {
                    continue;
                }
                let stored = datums_under(&rig, std::slice::from_ref(path), path).await;
                let Some(head_datum) = head_of(inner, stored.iter().map(|(_, ord, _)| *ord)) else {
                    continue;
                };
                let (_, _, value) = stored
                    .iter()
                    .find(|(_, ord, _)| *ord == head_datum)
                    .expect("the head datum");
                prop_assert!(
                    !matches(&head.patterns[0], &delivered(&path[prefix.len()..], *value)),
                    "the head waiter of {:?} matches the head datum of {:?}",
                    prefix,
                    path
                );
            }
        }
    }
    Ok(())
}

/// `token_ord_reentry` (R106, decisions 31 and 43). On a queue and a stack the token gives every
/// record and every waiter of the space a distinct order below the token. A persistent operation
/// that re-issues carries the order of its first round, which is the token before its first job,
/// and the transaction meets no tie.
#[test]
fn prop_pathmap_token_orders_reenter_at_their_place() {
    const INNERS: &[SpaceInner] = &[SpaceInner::Queue, SpaceInner::Stack];
    let script = vec(prop_oneof![arb_send(3), arb_receive(0, 3)], 1..12);
    check(
        pure_config(),
        (arb_inner(INNERS), script),
        |(inner, script)| tokio_runtime().block_on(token_case(inner, script)),
    );
}

async fn token_case(inner: SpaceInner, script: Vec<PathOp>) -> Result<(), TestCaseError> {
    let rig = Rig::new(inner).await;
    let paths = universe(&script);
    for (index, op) in script.iter().enumerate() {
        let first_ord = rig.token().await;
        for result in run_bounded(&rig, op.op(index), 3).await {
            let outcome = match result {
                Ok(outcome) => outcome,
                Err(error) => {
                    return Err(TestCaseError::fail(format!("a job fails: {error:?}")));
                }
            };
            if let Some(reissue) = &outcome.reissue {
                let carried = match &reissue.op {
                    SpaceOp::Produce(op) => (op.persistent, op.reissued_ord),
                    SpaceOp::Consume(op) => (op.persistent, op.reissued_ord),
                    SpaceOp::Settle(_) | SpaceOp::Join(_) => (false, None),
                };
                match carried {
                    (true, ord) => prop_assert_eq!(ord, Some(first_ord), "the first-round order"),
                    (false, ord) => prop_assert_eq!(ord, None),
                }
            }
            let token = rig.token().await;
            let mut ords = Vec::new();
            for prefix in &paths {
                for waiter in rig
                    .host
                    .get_waiting_continuations(vec![index_at(prefix)])
                    .await
                {
                    ords.push(record::continuation_ord(&waiter.continuation).expect("a waiter"));
                }
                for datum in rig.datums(prefix).await {
                    ords.push(
                        record::unwrap_pq_datum(&datum.a)
                            .expect("a datum record")
                            .ord,
                    );
                }
            }
            let distinct: BTreeSet<i64> = ords.iter().copied().collect();
            prop_assert_eq!(distinct.len(), ords.len(), "the orders are distinct");
            prop_assert!(
                ords.iter().all(|ord| *ord < token),
                "every order lies below the token"
            );
        }
    }
    Ok(())
}

/// MergeDetectsPrefixContact (V25). A branch that stores a datum on a path `p` and a branch that
/// parks a waiter on prefixes meet in merge check #2 exactly on the index channels of the
/// prefixes of the waiter that are prefixes of `p`. Without the token events, no other channel
/// conflicts, and a waiter on no prefix of `p` merges.
#[test]
fn prop_pathmap_merge_detects_prefix_contact() {
    const INNERS: &[SpaceInner] = &[SpaceInner::Bag, SpaceInner::Queue];
    let waiter = prop_oneof![arb_path(3).prop_map(|at| vec![at]), vec(arb_path(2), 2..=2),];
    check(
        pure_config(),
        (arb_inner(INNERS), arb_path(3), waiter),
        |(inner, at, prefixes)| tokio_runtime().block_on(contact_case(inner, at, prefixes)),
    );
}

async fn contact_case(
    inner: SpaceInner,
    at: Vec<i64>,
    prefixes: Vec<Vec<i64>>,
) -> Result<(), TestCaseError> {
    // The pre-state holds no datum, so the waiter of the right branch parks on every prefix.
    let rig = Rig::new(inner).await;
    let base = rig.root().await;
    let waiter = match prefixes.as_slice() {
        [prefix] => take(prefix, "w", any()),
        _ => {
            let binds: Vec<(&[i64], BindPattern)> = prefixes
                .iter()
                .map(|prefix| (prefix.as_slice(), any()))
                .collect();
            joined(join(&binds, "j"))
        }
    };
    let (left, right) = branches(&rig, &base, vec![put(&at, 1)], vec![waiter]).await;
    let token = hash(&token());
    let expected: BTreeSet<Blake2b256Hash> = prefixes
        .iter()
        .filter(|prefix| prefix.len() <= at.len() && at[..prefix.len()] == ***prefix)
        .map(|prefix| hash(&index_at(&elements(prefix))))
        .collect();
    let found = conflicting(
        &rig,
        &base,
        &without_channel(&left, &token),
        &without_channel(&right, &token),
    );
    prop_assert_eq!(found, expected, "the contact channels");
    let with_token = conflicting(&rig, &base, &left, &right);
    prop_assert!(
        with_token.contains(&token),
        "both branches change the space"
    );
    Ok(())
}

// ─── Rholang programs ────────────────────────────────────────────────────────────────────────

/// The config of the space `space` that a session stored.
async fn stored_family(session: &Session, space: NativeSpace) -> SpaceFactoryUrn {
    let data = session
        .data(&namespace::gprivate(namespace::config_channel(space)))
        .await;
    let [config] = data.as_slice() else {
        panic!("C(sid) holds {} records", data.len());
    };
    let map = record::config_map(&config.a).expect("the config record decodes");
    let config = SpaceConfig::from_par(map).expect("the config map decodes");
    SpaceFactoryUrn::parse(&format!("rho:space:{}:{}", config.outer, config.inner))
        .expect("a family of the config")
}

/// The values on the public channel `channel` of a session, as sorted debug strings.
async fn outputs(session: &Session, channel: &str) -> Vec<String> {
    let mut values: Vec<String> = session
        .values(channel)
        .await
        .into_iter()
        .map(|pars| format!("{pars:?}"))
        .collect();
    values.sort();
    values
}

/// The FIPS alias `rho:space:PathMapSpace` names the factory of `pathmap:bag` (FIPS:297, family
/// index 7, body ref 207). `getSpaceAgent` of a space that the alias creates equals the alias and
/// the family URN (decision 33), and the config of the space names `pathmap:bag`. A program in
/// `use space` then runs the FIPS example through the reducer: the sends on `@[0,1,2]` reach the
/// receives on `@[0,1]` with the re-rooted values.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_pathmapspace_alias_equal() {
    let mut session = Session::new().await;
    session
        .run(
            r#"
new alias(`rho:space:PathMapSpace`), family(`rho:space:pathmap:bag`),
    gsa(`rho:space:getSpaceAgent`), equal(`rho:assert:assertEqual`), out in {
  for (space <- alias!?("default")) {
    for (@capability <- gsa!?(*space)) {
      for (@sameAlias <- equal!?(capability, *alias)) {
        for (@sameFamily <- equal!?(capability, *family)) {
          @"rrs-pathmap-alias"!(*space, sameAlias, sameFamily) |
          use space {
            @[0, 1, 2]!({|"hi"|}) |
            @[0, 1, 2]!(42) |
            for (@x <- @[0, 1]) { out!(x) } |
            for (@y <- @[0, 1]) { out!(y) }
          }
        }
      }
    }
  } |
  for (@value <= out) { @"rrs-pathmap-out"!(value) }
}
"#,
            "rrs-pathmap-alias-equal",
        )
        .await;
    let values = session.values("rrs-pathmap-alias").await;
    let [reply] = values.as_slice() else {
        panic!("the alias replied {} times", values.len());
    };
    let [handle, same_alias, same_family] = reply.as_slice() else {
        panic!("the reply holds {} values", reply.len());
    };
    assert_eq!(same_alias, &new_gbool_par(true, Vec::new(), false));
    assert_eq!(same_family, &new_gbool_par(true, Vec::new(), false));
    assert_eq!(
        stored_family(&session, native(handle)).await,
        family(SpaceInner::Bag)
    );
    let mut expected = vec![
        format!("{:?}", vec![pathmap(vec![list(vec![
            int(2),
            string("hi")
        ])])]),
        format!("{:?}", vec![ints(&[2, 42])]),
    ];
    expected.sort();
    assert_eq!(outputs(&session, "rrs-pathmap-out").await, expected);
}

/// A name of a PathMap space roots a private subtree through the reducer: a send on `@[*n, 1]`
/// in `use space` reaches a receive on the name `n` outside `use`, which routes to its space by
/// itself, with the suffix `[1, v]`. A send on the singleton list `@[*m]` reaches a receive on
/// `m` with the value itself (`@c ≡ @[c]`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pathmap_rholang_names_root_private_subtrees() {
    let mut session = Session::new().await;
    session.setup("rho:space:pathmap:bag", "\"default\"").await;
    session
        .run(
            &with_space(
                r#"new out in {
  use space { @[*a, 1]!(5) | @[*b]!(6) } |
  for (@v <- a) { out!(v) } |
  for (@w <- b) { out!(w) } |
  for (@x <= out) { @"rrs-pathmap-names"!(x) }
}"#,
            ),
            "rrs-pathmap-names",
        )
        .await;
    let mut expected = vec![
        format!("{:?}", vec![ints(&[1, 5])]),
        format!("{:?}", vec![int(6)]),
    ];
    expected.sort();
    assert_eq!(outputs(&session, "rrs-pathmap-names").await, expected);
}
