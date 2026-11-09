//! Added by G3 (DR-135, S3b): the unit tests of Rule H, a held process runs
//! for the sender of its value (design §13).
//!
//! - The held map, env positions and the datum holder h(d) (T2, T12, T13).
//! - H6: the caller of a system body is the single code holder.
//! - `code_free` agrees with a recursive oracle.
//! - H2: the walk of a stored body agrees with a recursive oracle, each held
//!   term holds only the uses of its holder, and the walk is stack-safe.
//! - H5: the classification of send data and the record of a send agree with
//!   an oracle of the table in the design.
//! - H4 and T6: a held term or marker with a Unit signature or price 0 is
//!   malformed state.

use std::collections::BTreeMap;

use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{
    Bundle, EList, ETuple, GPrivate, GUnforgeable, If, Match, MatchCase, New, Receive, ReceiveBind,
    Send,
};
use models::rust::rholang::dismantle::dismantle_term;
use proptest::prelude::*;

use super::*;
use crate::rust::interpreter::accounting::authority::priced_cost_region;

/// The depth of the deep tests.
const DEEP_LEVELS: usize = 100_000;

/// The stack of the threads that walk and drop a deep term.
const SMALL_STACK_BYTES: usize = 256 * 1024;

/// The env positions of the random tests.
const POSITIONS: usize = 6;

fn ground(byte: u8) -> CostSignature {
    CostSignature {
        value: Some(CostSignatureValue::Ground(vec![byte; 4])),
    }
}

fn unit() -> CostSignature {
    CostSignature {
        value: Some(CostSignatureValue::Unit(true)),
    }
}

fn holder(byte: u8, price: u64) -> RegionOwner {
    RegionOwner {
        signature: ground(byte),
        price,
    }
}

fn caller_of(owner: &RegionOwner) -> CostCaller {
    CostCaller {
        signature: Some(owner.signature.clone()),
        price: owner.price,
    }
}

fn expr_par(instance: ExprInstance) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(instance),
    }])
}

fn number(value: i64) -> Par { expr_par(ExprInstance::GInt(value)) }

fn var_expr(k: i32) -> Expr {
    Expr {
        expr_instance: Some(ExprInstance::EVarBody(EVar {
            v: Some(Var {
                var_instance: Some(VarInstance::BoundVar(k)),
            }),
        })),
    }
}

/// `BoundVar(k)` alone, with its bit in `locally_free`.
fn var(k: i32) -> Par {
    Par {
        exprs: vec![var_expr(k)],
        locally_free: models::create_bit_vector(&[usize::try_from(k).expect("a small index")]),
        ..Default::default()
    }
}

/// A process: a send on `tag` of `tag`.
fn code(tag: i64) -> Par {
    Par {
        sends: vec![Send {
            chan: Some(number(tag)),
            data: vec![number(tag)],
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn private_name(id: u8) -> Par {
    Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id: vec![id; 32] })),
    }])
}

/// The free variables of `ps`, as the normalizer joins them.
fn joined_locally_free(ps: &[Par]) -> Vec<u8> {
    ps.iter().fold(Vec::new(), |bits, par| {
        union(bits, par.locally_free.clone())
    })
}

fn list(ps: Vec<Par>) -> Par {
    let locally_free = joined_locally_free(&ps);
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps,
                locally_free: locally_free.clone(),
                ..Default::default()
            })),
        }],
        locally_free,
        ..Default::default()
    }
}

fn tuple(ps: Vec<Par>) -> Par {
    let locally_free = joined_locally_free(&ps);
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::ETupleBody(ETuple {
                ps,
                locally_free: locally_free.clone(),
                ..Default::default()
            })),
        }],
        locally_free,
        ..Default::default()
    }
}

fn env_of(values: Vec<Par>) -> Env<Par> {
    let mut env = Env::new();
    for value in values {
        env.push(value).expect("a small env");
    }
    env
}

fn held_of(spans: &[(u32, u32, RegionOwner)]) -> HeldMap {
    let mut held = HeldMap::default();
    for (start, count, owner) in spans {
        held.insert_span(*start, *count, owner.clone());
    }
    held
}

fn region(signature: &CostSignature, id: &[u8], price: u64) -> CostRegion {
    priced_cost_region(signature, id, 0, price).expect("a region")
}

/// A Unit residue region that records `caller`.
fn recording(id: u8, caller: Option<CostCaller>) -> CostRegion {
    CostRegion {
        instance_id: vec![id; 32],
        signature: Some(unit()),
        price: 0,
        caller,
    }
}

fn sealed(regions: Vec<CostRegion>) -> CostAuthority { CostAuthority { regions } }

fn datum(seal: Option<CostAuthority>, pars: Vec<Par>) -> ListParWithRandom {
    ListParWithRandom {
        pars,
        random_state: Vec::new(),
        cost_authority: seal,
        cost_stack: None,
    }
}

fn binding_without_clients() -> OwnerFundingBinding {
    fn no_purse(_: &CostSignature) -> Option<OwnerPurseSet> { None }
    OwnerFundingBinding::new(BTreeSet::new(), no_purse, OwnerTable::empty(0))
}

// ---- the held map, positions and holders

/// A position takes the holder of the span with the greatest start at or
/// below it, when it lies below the end of that span. A span of width 0, or
/// one past `u32`, is never added. A copy with one more span leaves the
/// original as it was.
#[test]
fn the_held_map_finds_the_span_of_a_position() {
    let (first, second, third) = (holder(1, 5), holder(2, 6), holder(3, 7));
    let held = held_of(&[
        (0, 2, first.clone()),
        (2, 1, second.clone()),
        (5, 2, third.clone()),
    ]);
    let found: Vec<Option<RegionOwner>> = (0..9)
        .map(|position| held.holder(position).cloned())
        .collect();
    assert_eq!(found, vec![
        Some(first.clone()),
        Some(first.clone()),
        Some(second),
        None,
        None,
        Some(third.clone()),
        Some(third),
        None,
        None,
    ]);
    let mut unchanged = held.clone();
    unchanged.insert_span(9, 0, first.clone());
    unchanged.insert_span(u32::MAX - 1, 5, first.clone());
    assert_eq!(unchanged, held, "no span of width 0 or past u32");
    let extended = held.with_span(3, 2, first.clone());
    assert_eq!(extended.holder(3), Some(&first));
    assert_eq!(held.holder(3), None, "the copy shares, the original stays");
    assert!(HeldMap::default().is_empty() && !held.is_empty());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// The env position of `BoundVar(k)` is the position that `Env::get`
    /// reads, and the env value is the value that it returns.
    #[test]
    fn held_positions_match_env_get(values in 0_usize..12, k in -3_i32..16) {
        let env = env_of((0..values).map(|value| number(value as i64)).collect());
        let got = env.get(&k);
        prop_assert_eq!(env_value(k, &env), got.as_ref());
        let level = i32::try_from(values).expect("a small level");
        let expected = level.checked_sub(k).and_then(|p| p.checked_sub(1)).filter(|p| *p >= 0);
        prop_assert_eq!(env_position(k, &env), expected.map(|p| p as u32));
    }
}

/// h(d): the first region that records a caller decides, a Unit sentinel
/// gives no holder and stops the search (T12), and without a recorded caller
/// the signer of the seal decides. A holder at price 0 is no holder (T13).
#[test]
fn a_datum_holder_reads_the_first_recorded_caller_then_the_signer() {
    let (first, second) = (holder(1, 5), holder(2, 6));
    let first_caller = sealed(vec![
        region(&ground(9), b"s", 4),
        recording(1, Some(caller_of(&first))),
        recording(2, Some(caller_of(&second))),
    ]);
    assert_eq!(
        datum_holder(&first_caller).expect("a seal"),
        Some(first.clone())
    );
    let sentinel_first = sealed(vec![
        recording(1, Some(unit_sentinel())),
        recording(2, Some(caller_of(&second))),
    ]);
    assert_eq!(
        datum_holder(&sentinel_first).expect("a seal"),
        None,
        "a Unit sentinel stops the search"
    );
    let signer = sealed(vec![
        region(&ground(1), b"a", 9),
        region(&ground(2), b"b", 4),
        region(&unit(), b"u", 0),
    ]);
    let compound = sort_signature(&CostSignature {
        value: Some(CostSignatureValue::Compound(CostSignatureCompound {
            elements: vec![ground(1), ground(2)],
        })),
    })
    .term;
    assert_eq!(
        datum_holder(&signer).expect("a seal"),
        Some(RegionOwner {
            signature: compound,
            price: 4
        }),
        "the signer at the minimum price"
    );
    let free = sealed(vec![region(&ground(1), b"a", 0)]);
    assert_eq!(datum_holder(&free).expect("a seal"), None, "price 0 (T13)");
    let free_caller = sealed(vec![recording(1, Some(caller_of(&holder(1, 0))))]);
    assert_eq!(
        datum_holder(&free_caller).expect("a seal"),
        None,
        "price 0 (T13)"
    );
    let system = sealed(vec![region(&unit(), b"u", 0), recording(1, None)]);
    assert_eq!(datum_holder(&system).expect("a seal"), None);
}

/// Holders agree when their signatures are equal, at the minimum of their
/// prices (T2). Two signatures, or none, give no single holder.
#[test]
fn the_single_holder_needs_one_signature_at_the_minimum_price() {
    assert_eq!(single_holder(Vec::new()), None);
    assert_eq!(
        single_holder(vec![holder(1, 9), holder(1, 4), holder(1, 6)]),
        Some(holder(1, 4))
    );
    assert_eq!(single_holder(vec![holder(1, 9), holder(2, 4)]), None);
}

// ---- H6: the caller of a system body

fn arb_owner() -> impl Strategy<Value = RegionOwner> {
    (1_u8..4, 1_u64..10).prop_map(|(byte, price)| holder(byte, price))
}

/// A random stored seal: no seal, a signer region, a region that records a
/// caller or the Unit sentinel, or a malformed-free mix of these.
fn arb_seal() -> impl Strategy<Value = Option<CostAuthority>> {
    let region_strategy = prop_oneof![
        (arb_owner(), any::<u8>()).prop_map(|(owner, id)| region(
            &owner.signature,
            &[id],
            owner.price
        )),
        (arb_owner(), any::<u8>()).prop_map(|(owner, id)| recording(id, Some(caller_of(&owner)))),
        any::<u8>().prop_map(|id| recording(id, Some(unit_sentinel()))),
        any::<u8>().prop_map(|id| recording(id, None)),
    ];
    prop::option::of(prop::collection::vec(region_strategy, 0..3).prop_map(sealed))
}

fn arb_bound() -> impl Strategy<Value = Par> {
    prop_oneof![
        (0_i64..5).prop_map(number),
        (0_i64..5).prop_map(code),
        any::<u8>().prop_map(private_name),
    ]
}

fn arb_data() -> impl Strategy<Value = Vec<ListParWithRandom>> {
    prop::collection::vec(
        (arb_seal(), prop::collection::vec(arb_bound(), 0..3))
            .prop_map(|(seal, pars)| datum(seal, pars)),
        0..4,
    )
}

/// H6 stated directly: K holds the first non-Unit caller that the
/// continuation seal records and h(d) for each datum with a value that
/// carries code; the caller is its one signature at the minimum price.
fn oracle_caller(seal: Option<&CostAuthority>, data: &[ListParWithRandom]) -> Option<RegionOwner> {
    let mut members: BTreeMap<Vec<u8>, RegionOwner> = BTreeMap::new();
    let mut add = |owner: RegionOwner| {
        let key = owner.signature.encode_to_vec();
        let entry = members.entry(key).or_insert_with(|| owner.clone());
        entry.price = entry.price.min(owner.price);
    };
    if let Some(seal) = seal {
        let recorded = seal.regions.iter().find_map(|region| {
            let caller = region.caller.as_ref()?;
            let signature = caller.signature.clone()?;
            (!is_unit_cost_signature(&signature)).then_some(RegionOwner {
                signature,
                price: caller.price,
            })
        });
        recorded.into_iter().for_each(&mut add);
    }
    for datum in data {
        if datum.pars.iter().any(|par| !code_free(par)) {
            if let Some(seal) = datum.cost_authority.as_ref() {
                datum_holder(seal)
                    .expect("a seal")
                    .into_iter()
                    .for_each(&mut add);
            }
        }
    }
    match members.len() {
        1 => members.into_values().next(),
        _ => None,
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// The caller of a fired system body is the single code holder (H6).
    #[test]
    fn a_system_body_caller_is_the_single_code_holder(seal in arb_seal(), data in arb_data()) {
        let binding = binding_without_clients();
        let context = body_context(Some(&binding), seal.as_ref(), &data, true)
            .expect("well-formed seals");
        prop_assert_eq!(context.caller, oracle_caller(seal.as_ref(), &data));
    }

    /// The context of a fired body depends only on the seal, the data and
    /// the binding: the random states of the data do not change it, and
    /// without a binding it is the dev context.
    #[test]
    fn held_contexts_are_a_function_of_data_and_binding(
        seal in arb_seal(),
        data in arb_data(),
        system in any::<bool>(),
        state in prop::collection::vec(any::<u8>(), 0..8),
    ) {
        let binding = binding_without_clients();
        let first = body_context(Some(&binding), seal.as_ref(), &data, system).expect("seals");
        let again = body_context(Some(&binding), seal.as_ref(), &data, system).expect("seals");
        let reseeded: Vec<ListParWithRandom> = data
            .iter()
            .cloned()
            .map(|datum| ListParWithRandom { random_state: state.clone(), ..datum })
            .collect();
        let other = body_context(Some(&binding), seal.as_ref(), &reseeded, system).expect("seals");
        prop_assert_eq!(&first, &again);
        prop_assert_eq!(&first, &other);
        let mut start = 0_u32;
        for datum in &data {
            let count = u32::try_from(datum.pars.len()).expect("a small datum");
            let expected = datum
                .cost_authority
                .as_ref()
                .and_then(|seal| datum_holder(seal).expect("a seal"))
                .filter(|_| count > 0);
            for position in start..start + count {
                prop_assert_eq!(first.held.holder(position).cloned(), expected.clone());
            }
            start += count;
        }
        prop_assert_eq!(
            body_context(None, seal.as_ref(), &data, system).expect("no binding"),
            BodyContext { system, ..BodyContext::default() }
        );
    }
}

// ---- code_free

/// `code_free` stated directly, as a recursive oracle.
fn code_free_oracle(par: &Par) -> bool {
    par.sends.is_empty()
        && par.receives.is_empty()
        && par.news.is_empty()
        && par.matches.is_empty()
        && par.connectives.is_empty()
        && par.conditionals.is_empty()
        && par.cost_signed_terms.is_empty()
        && par.cost_stacks.is_empty()
        && !par.connective_used
        && par
            .bundles
            .iter()
            .all(|bundle| bundle.body.as_ref().is_none_or(code_free_oracle))
        && par.exprs.iter().all(|expr| match &expr.expr_instance {
            Some(ExprInstance::EListBody(list)) => {
                !list.connective_used
                    && list.remainder.is_none()
                    && list.ps.iter().all(code_free_oracle)
            }
            Some(ExprInstance::ETupleBody(tuple)) => {
                !tuple.connective_used && tuple.ps.iter().all(code_free_oracle)
            }
            Some(ExprInstance::ESetBody(set)) => {
                !set.connective_used
                    && set.remainder.is_none()
                    && set.ps.iter().all(code_free_oracle)
            }
            Some(ExprInstance::EMapBody(map)) => {
                !map.connective_used
                    && map.remainder.is_none()
                    && map.kvs.iter().all(|pair| match (&pair.key, &pair.value) {
                        (Some(key), Some(value)) => {
                            code_free_oracle(key) && code_free_oracle(value)
                        }
                        _ => false,
                    })
            }
            Some(
                ExprInstance::GBool(_)
                | ExprInstance::GInt(_)
                | ExprInstance::GString(_)
                | ExprInstance::GUri(_)
                | ExprInstance::GByteArray(_)
                | ExprInstance::GDouble(_)
                | ExprInstance::GBigInt(_)
                | ExprInstance::GBigRat(_)
                | ExprInstance::GFixedPoint(_)
                | ExprInstance::GUint64(_)
                | ExprInstance::GFloat32(_)
                | ExprInstance::GInt32(_)
                | ExprInstance::GUint32(_)
                | ExprInstance::GUint16(_)
                | ExprInstance::GUint8(_),
            ) => true,
            _ => false,
        })
}

/// Names, bundles of data and collections of data are code-free. Processes,
/// variables, methods, signed terms and bundles of code are not.
#[test]
fn code_free_admits_names_bundles_and_collections_of_data() {
    let bundle_of = |body: Par| Par {
        bundles: vec![Bundle {
            body: Some(body),
            write_flag: true,
            read_flag: false,
        }],
        ..Default::default()
    };
    for (label, par) in [
        ("nil", Par::default()),
        ("a number", number(1)),
        ("a private name", private_name(1)),
        ("a bundle of a name", bundle_of(private_name(2))),
        ("a list of data", list(vec![number(1), private_name(3)])),
        ("a tuple of data", tuple(vec![number(1), list(Vec::new())])),
    ] {
        assert!(code_free(&par), "{label} is code-free");
    }
    assert!(
        !plain_data(&private_name(1)),
        "a name is code-free but not plain data"
    );
    assert!(!plain_data(&bundle_of(number(1))), "so is a bundle of data");
    for (label, par) in [
        ("a process", code(1)),
        ("a variable", var(0)),
        ("a bundle of code", bundle_of(code(2))),
        ("a list of code", list(vec![number(1), code(3)])),
        ("a signed term", Par {
            cost_signed_terms: vec![CostSignedTerm {
                body: Some(Par::default()),
                signature: Some(ground(1)),
                held_price: None,
            }],
            ..Default::default()
        }),
        (
            "a method",
            expr_par(ExprInstance::EMethodBody(EMethod {
                method_name: "nth".to_owned(),
                target: Some(list(vec![number(1)])),
                arguments: vec![number(0)],
                ..Default::default()
            })),
        ),
    ] {
        assert!(!code_free(&par), "{label} is not code-free");
    }
}

/// A random Par for `code_free`: data, names, bundles, processes,
/// variables and collections, with now and then a connective or a
/// remainder.
fn arb_value() -> impl Strategy<Value = Par> {
    let leaf = prop_oneof![
        4 => (0_i64..9).prop_map(number),
        2 => any::<u8>().prop_map(private_name),
        1 => (0_i64..3).prop_map(code),
        1 => (0_i32..3).prop_map(var),
        1 => Just(Par {
            connective_used: true,
            ..Default::default()
        }),
    ];
    leaf.prop_recursive(4, 40, 4, |inner| {
        let children = prop::collection::vec(inner.clone(), 0..4);
        prop_oneof![
            (children.clone(), prop::bool::weighted(0.1)).prop_map(|(ps, connective_used)| {
                expr_par(ExprInstance::EListBody(EList {
                    ps,
                    connective_used,
                    ..Default::default()
                }))
            }),
            children.clone().prop_map(tuple),
            (inner.clone(), any::<bool>()).prop_map(|(body, write_flag)| Par {
                bundles: vec![Bundle {
                    body: Some(body),
                    write_flag,
                    read_flag: !write_flag,
                }],
                ..Default::default()
            }),
            children.prop_map(|pars| {
                pars.into_iter().fold(Par::default(), |left, right| {
                    models::rust::rholang::implicits::concatenate_pars(left, right)
                })
            }),
        ]
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// `code_free` agrees with its recursive oracle.
    #[test]
    fn code_free_matches_an_oracle(value in arb_value()) {
        prop_assert_eq!(code_free(&value), code_free_oracle(&value));
    }
}

// ---- H2: the walk of a stored body

/// A binder of the oracle: a nested receive or new, or a match case with the
/// holder of its target and whether the target carries code.
#[derive(Clone)]
struct OracleFrame {
    count: i32,
    holder: Option<RegionOwner>,
    carries_code: bool,
}

/// What a variable of a stored body resolves to in the oracle.
enum OracleBinding {
    Frame(Option<RegionOwner>, bool),
    Receive,
    Captured(Option<RegionOwner>, Option<Par>),
}

struct OracleScope<'a> {
    held: &'a HeldMap,
    env: &'a Env<Par>,
    bind_count: i32,
    future: &'a HeldFuture,
}

impl OracleScope<'_> {
    fn resolve(&self, k: i32, frames: &[OracleFrame]) -> OracleBinding {
        if k < 0 {
            return OracleBinding::Receive;
        }
        let mut k = k;
        for frame in frames.iter().rev() {
            if k < frame.count {
                return OracleBinding::Frame(frame.holder.clone(), frame.carries_code);
            }
            k -= frame.count;
        }
        if k < self.bind_count {
            return OracleBinding::Receive;
        }
        let captured = k - self.bind_count;
        OracleBinding::Captured(
            self.held.var_holder(captured, self.env).cloned(),
            self.env.get(&captured),
        )
    }

    fn holder_of(binding: OracleBinding) -> Option<(RegionOwner, bool)> {
        match binding {
            OracleBinding::Frame(Some(holder), carries_code) => Some((holder, carries_code)),
            OracleBinding::Captured(Some(holder), value) => {
                Some((holder, !value.as_ref().is_some_and(code_free)))
            }
            _ => None,
        }
    }

    fn use_holder(&self, expr: &Expr, frames: &[OracleFrame]) -> Option<(RegionOwner, bool)> {
        match expr.expr_instance.as_ref()? {
            ExprInstance::EVarBody(EVar {
                v:
                    Some(Var {
                        var_instance: Some(VarInstance::BoundVar(k)),
                    }),
            }) => Self::holder_of(self.resolve(*k, frames)),
            ExprInstance::EMethodBody(method) => {
                let mut arguments = method.arguments.clone();
                let mut target = method.target.clone()?;
                loop {
                    let [expr] = target.exprs.as_slice() else {
                        return None;
                    };
                    match expr.expr_instance.as_ref()? {
                        ExprInstance::EMethodBody(inner) => {
                            arguments.extend(inner.arguments.iter().cloned());
                            target = inner.target.clone()?;
                        }
                        ExprInstance::EVarBody(EVar {
                            v:
                                Some(Var {
                                    var_instance: Some(VarInstance::BoundVar(k)),
                                }),
                        }) => {
                            let arguments_fit = arguments.iter().all(|argument| {
                                code_free(argument)
                                    || single_bound_var(argument).is_some_and(|j| {
                                        matches!(
                                            self.resolve(j, frames),
                                            OracleBinding::Captured(_, Some(value))
                                                if code_free(&value)
                                        )
                                    })
                            });
                            return arguments_fit
                                .then(|| Self::holder_of(self.resolve(*k, frames)))
                                .flatten();
                        }
                        _ => return None,
                    }
                }
            }
            _ => None,
        }
    }

    fn wraps(&self, held_use: Option<(RegionOwner, bool)>) -> Option<RegionOwner> {
        let (holder, carries_code) = held_use?;
        (carries_code && self.future.needs_holder(&holder)).then_some(holder)
    }

    /// The oracle of `wrap_stored_body`, by recursion over the body.
    fn wrap(&self, par: &Par, frames: &[OracleFrame]) -> Par {
        let mut out = par.clone();
        let mut kept = Vec::new();
        let mut groups: Vec<(RegionOwner, Vec<Expr>)> = Vec::new();
        for expr in &par.exprs {
            match self.wraps(self.use_holder(expr, frames)) {
                Some(holder) => match groups
                    .iter_mut()
                    .find(|(group, _)| group.signature == holder.signature)
                {
                    Some((group, uses)) => {
                        group.price = group.price.min(holder.price);
                        uses.push(expr.clone());
                    }
                    None => groups.push((holder, vec![expr.clone()])),
                },
                None => kept.push(expr.clone()),
            }
        }
        out.exprs = kept;
        for send in out.sends.iter_mut() {
            for datum in send.data.iter_mut() {
                let marked = single_bound_var(datum)
                    .and_then(|k| self.wraps(Self::holder_of(self.resolve(k, frames))));
                if let Some(holder) = marked {
                    let body = datum.clone();
                    *datum = Par {
                        locally_free: body.locally_free.clone(),
                        connective_used: body.connective_used,
                        cost_signed_terms: vec![held_term(body, holder)],
                        ..Default::default()
                    };
                }
            }
        }
        let with = |frame: OracleFrame| {
            let mut inner = frames.to_vec();
            inner.push(frame);
            inner
        };
        for receive in out.receives.iter_mut() {
            if let Some(body) = receive.body.as_ref() {
                let inner = with(OracleFrame {
                    count: receive.bind_count.max(0),
                    holder: None,
                    carries_code: false,
                });
                receive.body = Some(self.wrap(body, &inner));
            }
        }
        for new in out.news.iter_mut() {
            if let Some(body) = new.p.as_ref() {
                let inner = with(OracleFrame {
                    count: new.bind_count.max(0),
                    holder: None,
                    carries_code: false,
                });
                new.p = Some(self.wrap(body, &inner));
            }
        }
        for matched in out.matches.iter_mut() {
            let (holder, carries_code) = matched
                .target
                .as_ref()
                .and_then(single_bound_var)
                .and_then(|k| Self::holder_of(self.resolve(k, frames)))
                .map_or((None, false), |(holder, carries_code)| {
                    (Some(holder), carries_code)
                });
            for case in matched.cases.iter_mut() {
                if let Some(body) = case.source.as_ref() {
                    let inner = with(OracleFrame {
                        count: case.free_count.max(0),
                        holder: holder.clone(),
                        carries_code,
                    });
                    case.source = Some(self.wrap(body, &inner));
                }
            }
        }
        for conditional in out.conditionals.iter_mut() {
            conditional.if_true = conditional
                .if_true
                .as_ref()
                .map(|body| self.wrap(body, frames));
            conditional.if_false = conditional
                .if_false
                .as_ref()
                .map(|body| self.wrap(body, frames));
        }
        for bundle in out.bundles.iter_mut() {
            bundle.body = bundle.body.as_ref().map(|body| self.wrap(body, frames));
        }
        for signed in out.cost_signed_terms.iter_mut() {
            if signed.held_price.is_none() {
                signed.body = signed.body.as_ref().map(|body| self.wrap(body, frames));
            }
        }
        for (holder, uses) in groups {
            let locally_free = uses.iter().fold(Vec::new(), |bits, expr| {
                union(bits, expr_locally_free(expr))
            });
            out.cost_signed_terms.push(held_term(
                Par {
                    exprs: uses,
                    locally_free,
                    ..Default::default()
                },
                holder,
            ));
        }
        out
    }

    /// Checks that each held term in `par` holds only uses of its holder's
    /// signature, at the minimum price of their holders.
    fn check_held_terms(&self, par: &Par, frames: &[OracleFrame]) -> Result<(), String> {
        for term in &par.cost_signed_terms {
            let Some(price) = term.held_price else {
                if let Some(body) = term.body.as_ref() {
                    self.check_held_terms(body, frames)?;
                }
                continue;
            };
            let body = term.body.as_ref().ok_or("a held term without a body")?;
            let signature = term
                .signature
                .as_ref()
                .ok_or("a held term without a signature")?;
            let mut lowest = u64::MAX;
            for expr in &body.exprs {
                let (holder, _) = self
                    .use_holder(expr, frames)
                    .ok_or("a held use without a holder")?;
                if &holder.signature != signature {
                    return Err(format!(
                        "a use of {:?} in a held term of {signature:?}",
                        holder
                    ));
                }
                lowest = lowest.min(holder.price);
            }
            if !body.exprs.is_empty() && price != lowest {
                return Err(format!("held price {price}, lowest use price {lowest}"));
            }
        }
        let with = |count: i32, holder: Option<RegionOwner>, carries_code: bool| {
            let mut inner = frames.to_vec();
            inner.push(OracleFrame {
                count: count.max(0),
                holder,
                carries_code,
            });
            inner
        };
        for receive in &par.receives {
            if let Some(body) = receive.body.as_ref() {
                self.check_held_terms(body, &with(receive.bind_count, None, false))?;
            }
        }
        for new in &par.news {
            if let Some(body) = new.p.as_ref() {
                self.check_held_terms(body, &with(new.bind_count, None, false))?;
            }
        }
        for matched in &par.matches {
            let (holder, carries_code) = matched
                .target
                .as_ref()
                .and_then(single_bound_var)
                .and_then(|k| Self::holder_of(self.resolve(k, frames)))
                .map_or((None, false), |(holder, carries_code)| {
                    (Some(holder), carries_code)
                });
            for case in &matched.cases {
                if let Some(body) = case.source.as_ref() {
                    self.check_held_terms(
                        body,
                        &with(case.free_count, holder.clone(), carries_code),
                    )?;
                }
            }
        }
        for body in par
            .conditionals
            .iter()
            .flat_map(|conditional| [conditional.if_true.as_ref(), conditional.if_false.as_ref()])
            .flatten()
            .chain(par.bundles.iter().filter_map(|bundle| bundle.body.as_ref()))
        {
            self.check_held_terms(body, frames)?;
        }
        Ok(())
    }
}

fn method_on(target: Par, argument: Par) -> Par {
    let locally_free = union(target.locally_free.clone(), argument.locally_free.clone());
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EMethodBody(EMethod {
                method_name: "nth".to_owned(),
                target: Some(target),
                arguments: vec![argument],
                locally_free: locally_free.clone(),
                connective_used: false,
            })),
        }],
        locally_free,
        ..Default::default()
    }
}

/// A random stored body: uses of variables and methods in process position,
/// sends of variables and data, and nested receives, news, matches, ifs,
/// bundles and signed terms. With `held_terms`, some signed terms are held
/// terms already, which the walk must not visit.
fn arb_body_with(held_terms: bool) -> impl Strategy<Value = Par> {
    let index = 0_i32..(POSITIONS as i32 + 2);
    let leaf = prop_oneof![
        3 => index.clone().prop_map(var),
        1 => (index.clone(), prop_oneof![
            (0_i64..3).prop_map(number),
            (0_i32..(POSITIONS as i32 + 2)).prop_map(var),
            (0_i64..3).prop_map(code),
        ])
            .prop_map(|(k, argument)| method_on(var(k), argument)),
        2 => (prop::collection::vec(
            prop_oneof![index.clone().prop_map(var), (0_i64..3).prop_map(number)],
            0..3
        ))
            .prop_map(|data| Par {
                sends: vec![Send {
                    chan: Some(number(0)),
                    data,
                    ..Default::default()
                }],
                ..Default::default()
            }),
        1 => Just(Par::default()),
    ];
    leaf.prop_recursive(4, 32, 3, move |inner| {
        prop_oneof![
            (inner.clone(), 0_i32..3).prop_map(|(body, bind_count)| Par {
                receives: vec![Receive {
                    binds: vec![ReceiveBind {
                        patterns: vec![Par::default()],
                        source: Some(number(1)),
                        free_count: bind_count,
                        ..Default::default()
                    }],
                    body: Some(body),
                    bind_count,
                    ..Default::default()
                }],
                ..Default::default()
            }),
            (inner.clone(), 0_i32..3).prop_map(|(body, bind_count)| Par {
                news: vec![New {
                    bind_count,
                    p: Some(body),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            (
                prop_oneof![
                    (0_i32..(POSITIONS as i32 + 2)).prop_map(var),
                    Just(number(0))
                ],
                prop::collection::vec((inner.clone(), 0_i32..3), 1..3)
            )
                .prop_map(|(target, cases)| Par {
                    matches: vec![Match {
                        target: Some(target),
                        cases: cases
                            .into_iter()
                            .map(|(source, free_count)| MatchCase {
                                pattern: Some(Par::default()),
                                source: Some(source),
                                free_count,
                                guard: None,
                            })
                            .collect(),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
            (inner.clone(), inner.clone()).prop_map(|(if_true, if_false)| Par {
                conditionals: vec![If {
                    condition: Some(number(1)),
                    if_true: Some(if_true),
                    if_false: Some(if_false),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            inner.clone().prop_map(|body| Par {
                bundles: vec![Bundle {
                    body: Some(body),
                    write_flag: true,
                    read_flag: true,
                }],
                ..Default::default()
            }),
            (inner.clone(), any::<bool>()).prop_map(move |(body, held)| Par {
                cost_signed_terms: vec![CostSignedTerm {
                    body: Some(body),
                    signature: Some(ground(7)),
                    held_price: (held && held_terms).then_some(3),
                }],
                ..Default::default()
            }),
            prop::collection::vec(inner, 1..3).prop_map(|pars| {
                pars.into_iter().fold(Par::default(), |left, right| {
                    models::rust::rholang::implicits::concatenate_pars(left, right)
                })
            }),
        ]
    })
}

/// A random scope of a stored receive: the env values, the held map over
/// them, the bind count of the receive and the future context.
fn arb_scope() -> impl Strategy<Value = (Vec<Par>, Vec<Option<RegionOwner>>, i32, u8)> {
    (
        prop::collection::vec(
            prop_oneof![(0_i64..3).prop_map(number), (0_i64..3).prop_map(code)],
            POSITIONS,
        ),
        prop::collection::vec(
            prop::option::of(prop_oneof![
                Just(holder(1, 5)),
                Just(holder(1, 7)),
                Just(holder(2, 3))
            ]),
            POSITIONS,
        ),
        0_i32..3,
        0_u8..3,
    )
}

fn future_of(choice: u8) -> HeldFuture {
    match choice {
        0 => HeldFuture::System,
        1 => HeldFuture::Signer(ground(1)),
        _ => HeldFuture::Signer(ground(2)),
    }
}

fn held_from(holders: &[Option<RegionOwner>]) -> HeldMap {
    let mut held = HeldMap::default();
    for (position, owner) in holders.iter().enumerate() {
        if let Some(owner) = owner {
            held.insert_span(position as u32, 1, owner.clone());
        }
    }
    held
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// The iterative walk of a stored body agrees with the recursive oracle,
    /// on every field, `locally_free` included.
    #[test]
    fn wrap_held_uses_matches_a_recursive_oracle(body in arb_body_with(true), scope in arb_scope()) {
        let (values, holders, bind_count, choice) = scope;
        let (env, held, future) = (env_of(values), held_from(&holders), future_of(choice));
        let oracle = OracleScope { held: &held, env: &env, bind_count, future: &future };
        let expected = oracle.wrap(&body, &[]);
        let mut walked = body.clone();
        let changed = wrap_stored_body(&held, &mut walked, bind_count, &env, &future);
        prop_assert_eq!(format!("{walked:?}"), format!("{expected:?}"));
        prop_assert_eq!(changed, format!("{walked:?}") != format!("{body:?}"));
        prop_assert_eq!(
            stored_body_has_held_uses(&held, &body, bind_count, &env, &future),
            changed
        );
    }

    /// Each held term that the walk makes holds only uses of its holder's
    /// signature, at the minimum price of their holders (T2).
    #[test]
    fn a_held_term_holds_only_its_holders_values(body in arb_body_with(false), scope in arb_scope()) {
        let (values, holders, bind_count, choice) = scope;
        let (env, held, future) = (env_of(values), held_from(&holders), future_of(choice));
        let oracle = OracleScope { held: &held, env: &env, bind_count, future: &future };
        let mut walked = body;
        wrap_stored_body(&held, &mut walked, bind_count, &env, &future);
        prop_assert_eq!(oracle.check_held_terms(&walked, &[]), Ok(()));
    }
}

/// `depth` nested receives without binds, then a bundle, around one use of
/// the captured variable 0. The loop builds the body from the inside out.
fn deep_body(depth: usize) -> Par {
    let mut par = var(0);
    for level in 0..depth {
        par = match level % 2 {
            0 => Par {
                receives: vec![Receive {
                    body: Some(par),
                    ..Default::default()
                }],
                ..Default::default()
            },
            _ => Par {
                bundles: vec![Bundle {
                    body: Some(par),
                    write_flag: true,
                    read_flag: true,
                }],
                ..Default::default()
            },
        };
    }
    par
}

/// The walk of a stored body 100,000 levels deep wraps the use at the
/// bottom, and the walk, its frames, `code_free` and the teardown stay within
/// a small stack. The verdicts are taken before the teardown, and the asserts
/// come after it.
#[test]
fn wrap_held_uses_and_code_free_are_stack_safe() {
    std::thread::Builder::new()
        .name("rule-h-deep".to_owned())
        .stack_size(SMALL_STACK_BYTES)
        .spawn(|| {
            let env = env_of(vec![code(1)]);
            let held = held_of(&[(0, 1, holder(1, 5))]);
            let mut body = deep_body(DEEP_LEVELS);
            let present = stored_body_has_held_uses(&held, &body, 0, &env, &HeldFuture::System);
            let changed = wrap_stored_body(&held, &mut body, 0, &env, &HeldFuture::System);
            let free = code_free(&body);
            dismantle_term(body);
            let mut value = Par::default();
            for _ in 0..DEEP_LEVELS {
                value = list(vec![value]);
            }
            let value_free = code_free(&value);
            dismantle_term(value);
            assert!(present && changed, "the deep use is wrapped");
            assert!(!free, "a body with a receive carries code");
            assert!(value_free, "a deep list of data is code-free");
        })
        .expect("spawn the deep thread")
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
}

// ---- H5: the record of a send

fn arb_datum_par() -> impl Strategy<Value = Par> {
    let leaf = prop_oneof![
        3 => (0_i32..(POSITIONS as i32)).prop_map(var),
        2 => (0_i64..3).prop_map(number),
        1 => (0_i64..3).prop_map(code),
    ];
    leaf.prop_recursive(2, 12, 3, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..3).prop_map(list),
            prop::collection::vec(inner, 0..3).prop_map(tuple),
        ]
    })
}

/// The class of one data par, stated directly: (holders, mixed, carries code).
fn oracle_class(par: &Par, held: &HeldMap, env: &Env<Par>) -> (Vec<RegionOwner>, bool, bool) {
    if code_free(par) {
        return (Vec::new(), false, false);
    }
    if let Some(k) = single_bound_var(par) {
        let value = env.get(&k);
        if value.as_ref().is_some_and(code_free) {
            return (Vec::new(), false, false);
        }
        return match held.var_holder(k, env) {
            Some(owner) => (vec![owner.clone()], false, true),
            None => (Vec::new(), true, true),
        };
    }
    if let Some(elements) = collection_elements(par) {
        let mut class = (Vec::new(), false, false);
        for element in elements {
            let (holders, mixed, carries) = oracle_class(element, held, env);
            class.0.extend(holders);
            class.1 |= mixed;
            class.2 |= carries;
        }
        return class;
    }
    (Vec::new(), true, true)
}

/// The record of the table of H5, stated directly.
fn oracle_record(
    system: bool,
    holders: &[RegionOwner],
    mixed: bool,
    carries_code: bool,
    foreign_code: bool,
    context: &CostSignature,
) -> String {
    let agreed = match holders {
        [] => None,
        [first, rest @ ..] => rest
            .iter()
            .all(|other| other.signature == first.signature)
            .then(|| RegionOwner {
                signature: first.signature.clone(),
                price: holders
                    .iter()
                    .map(|owner| owner.price)
                    .min()
                    .expect("holders"),
            }),
    };
    match system {
        true if !carries_code => "unchanged".to_owned(),
        true => match (mixed, agreed) {
            (false, Some(owner)) => format!("residue {owner:?}"),
            _ => "residue none".to_owned(),
        },
        // Changed by G3 (DR-135, S3b review): foreign held code records the
        // Unit sentinel unless its holders reduce to one agreed foreign
        // holder.
        // false => match (mixed, agreed) {
        //     (false, Some(owner)) if owner.signature != *context => format!("caller {owner:?}"),
        //     (true, _) if foreign_code => "sentinel".to_owned(),
        //     _ => "unchanged".to_owned(),
        // },
        false => {
            let foreign = foreign_code || holders.iter().any(|owner| owner.signature != *context);
            match (foreign, mixed, agreed) {
                (false, _, _) => "unchanged".to_owned(),
                (true, false, Some(owner)) if owner.signature != *context => {
                    format!("caller {owner:?}")
                }
                (true, _, _) => "sentinel".to_owned(),
            }
        }
    }
}

fn record_text(record: &SendRecord) -> String {
    match record {
        SendRecord::Unchanged => "unchanged".to_owned(),
        SendRecord::Caller(caller) if caller == &unit_sentinel() => "sentinel".to_owned(),
        SendRecord::Caller(caller) => format!("caller {:?}", RegionOwner {
            signature: caller.signature.clone().expect("a caller signature"),
            price: caller.price,
        }),
        SendRecord::Residue(Some(owner)) => format!("residue {owner:?}"),
        SendRecord::Residue(None) => "residue none".to_owned(),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// A send records one holder only when every code that its datum
    /// carries has one agreed holder, and in an owner body only when that
    /// holder differs from the context. Mixed code with a foreign holder in
    /// an owner body records the Unit sentinel (T12).
    #[test]
    fn datum_holder_needs_one_holder_for_all_code(
        data in prop::collection::vec(arb_datum_par(), 1..4),
        markers in prop::collection::vec(prop::option::of(prop_oneof![
            Just(holder(1, 4)),
            Just(holder(2, 6))
        ]), 1..4),
        scope in arb_scope(),
        context_byte in 1_u8..4,
        system in any::<bool>(),
    ) {
        let (values, holders, _, _) = scope;
        let (env, held) = (env_of(values), held_from(&holders));
        let context = ground(context_byte);
        let markers: Vec<Option<RegionOwner>> =
            markers.into_iter().chain(std::iter::repeat(None)).take(data.len()).collect();
        let references: Vec<&Par> = data.iter().collect();
        let mut evaluate = |par: &Par| -> Result<Par, InterpreterError> { Ok(par.clone()) };
        let class = classify_data(&held, &references, &markers, &env, &context, &mut evaluate)
            .expect("a class");
        let mut expected = (Vec::new(), false, false, false);
        for (par, marker) in data.iter().zip(&markers) {
            match marker {
                Some(owner) => {
                    expected.0.push(owner.clone());
                    expected.2 = true;
                    expected.3 |= owner.signature != context;
                }
                None => {
                    let (holders, mixed, carries) = oracle_class(par, &held, &env);
                    expected.0.extend(holders);
                    expected.1 |= mixed;
                    expected.2 |= carries;
                }
            }
            for k in locally_free_indices(&par.locally_free) {
                let foreign = held.var_holder(k, &env).is_some_and(|owner| owner.signature != context)
                    && !env.get(&k).as_ref().is_some_and(code_free);
                expected.3 |= foreign;
            }
        }
        let record = send_record(system, class, !expected.2, &context);
        prop_assert_eq!(
            record_text(&record),
            oracle_record(system, &expected.0, expected.1, expected.2, expected.3, &context)
        );
    }
}

/// The table of H5 on fixed data: an owner body records an agreed foreign
/// holder through a tagged forward, records the Unit sentinel for mixed
/// foreign code, and records nothing for its own holder. A system body
/// records one agreed holder, no caller for mixed code, and keeps S3's seal
/// for data.
#[test]
fn send_record_follows_the_table_of_h5() {
    let env = env_of(vec![code(1), number(2), code(3)]);
    let (sender, owner) = (holder(1, 4), holder(2, 6));
    let held = held_of(&[(0, 1, sender.clone()), (2, 1, owner.clone())]);
    let record = |data: Vec<Par>, system: bool, context: &CostSignature| {
        let references: Vec<&Par> = data.iter().collect();
        let mut evaluate = |par: &Par| -> Result<Par, InterpreterError> { Ok(par.clone()) };
        let class =
            classify_data(&held, &references, &[], &env, context, &mut evaluate).expect("a class");
        let carries = data.iter().any(|par| oracle_class(par, &held, &env).2);
        record_text(&send_record(system, class, !carries, context))
    };
    // BoundVar(2) is position 0, the sender's code. BoundVar(0) is position
    // 2, the owner's code. BoundVar(1) is position 1, a number.
    let tagged = tuple(vec![number(0), var(2)]);
    assert_eq!(
        record(vec![tagged.clone()], false, &owner.signature),
        format!("caller {sender:?}")
    );
    assert_eq!(
        record(vec![tuple(vec![var(2), code(9)])], false, &owner.signature),
        "sentinel"
    );
    assert_eq!(record(vec![var(0)], false, &owner.signature), "unchanged");
    assert_eq!(record(vec![var(1)], false, &owner.signature), "unchanged");
    assert_eq!(
        record(vec![tagged], true, &owner.signature),
        format!("residue {sender:?}")
    );
    assert_eq!(
        record(vec![var(2), var(0)], true, &owner.signature),
        "residue none",
        "two holders"
    );
    assert_eq!(
        record(vec![var(1), number(3)], true, &owner.signature),
        "unchanged"
    );
    // Added by G3 (DR-135, S3b review): foreign held code of two holders
    // that disagree records the Unit sentinel in an owner body, also when
    // one of them is the context.
    assert_eq!(
        record(vec![var(2), var(0)], false, &owner.signature),
        "sentinel",
        "a foreign holder and the context"
    );
    let three = env_of(vec![code(1), code(2), number(3)]);
    let other = holder(3, 8);
    let two_senders = held_of(&[(0, 1, sender.clone()), (1, 1, other)]);
    let references = [var(2), var(1)];
    let references: Vec<&Par> = references.iter().collect();
    let mut evaluate = |par: &Par| -> Result<Par, InterpreterError> { Ok(par.clone()) };
    let class = classify_data(
        &two_senders,
        &references,
        &[],
        &three,
        &owner.signature,
        &mut evaluate,
    )
    .expect("a class");
    assert_eq!(
        record_text(&send_record(false, class, false, &owner.signature)),
        "sentinel",
        "two foreign holders that disagree"
    );
}

// ---- H4 and T6: the guards of held terms and markers

fn held_signed(signature: Option<CostSignature>, held_price: Option<u64>) -> CostSignedTerm {
    CostSignedTerm {
        body: Some(code(1)),
        signature,
        held_price,
    }
}

/// A held term needs a canonical signature that is not Unit and a held
/// price above 0 (T13, T14). A surface signed term has no holder. A held
/// marker passes the same guard when the send strips it (T6).
#[test]
fn a_held_term_with_a_unit_signature_or_price_zero_is_malformed() {
    let malformed = |result: Result<Option<RegionOwner>, InterpreterError>| matches!(result, Err(InterpreterError::ReduceError(message)) if message.contains("held term is malformed"));
    let unsorted = CostSignature {
        value: Some(CostSignatureValue::Compound(CostSignatureCompound {
            elements: vec![ground(2), ground(1)],
        })),
    };
    assert!(malformed(held_term_holder(&held_signed(
        Some(unit()),
        Some(5)
    ))));
    assert!(malformed(held_term_holder(&held_signed(
        Some(ground(1)),
        Some(0)
    ))));
    assert!(malformed(held_term_holder(&held_signed(
        Some(unsorted),
        Some(5)
    ))));
    assert!(malformed(held_term_holder(&held_signed(None, Some(5)))));
    assert_eq!(
        held_term_holder(&held_signed(Some(ground(1)), None)).expect("a surface term"),
        None
    );
    assert_eq!(
        held_term_holder(&held_signed(Some(ground(1)), Some(5))).expect("a held term"),
        Some(holder(1, 5))
    );
    let marker = |term: CostSignedTerm| Par {
        cost_signed_terms: vec![term],
        ..Default::default()
    };
    for bad in [
        held_signed(Some(unit()), Some(5)),
        held_signed(Some(ground(1)), Some(0)),
    ] {
        let data = vec![number(1), marker(bad)];
        assert!(matches!(
            strip_held_markers(&data),
            Err(InterpreterError::ReduceError(message)) if message.contains("held term is malformed")
        ));
    }
}

/// The strip gives the body of each held marker and its holder, leaves every
/// other par as it is, and gives no holders when no par is a marker. A
/// surface signed term in send data is not a marker.
#[test]
fn held_markers_are_stripped_with_their_holders() {
    let marker = Par {
        cost_signed_terms: vec![held_signed(Some(ground(1)), Some(5))],
        ..Default::default()
    };
    let surface = Par {
        cost_signed_terms: vec![held_signed(Some(ground(1)), None)],
        ..Default::default()
    };
    let data = vec![number(1), marker, surface.clone()];
    let (stripped, holders) = strip_held_markers(&data).expect("well-formed markers");
    assert_eq!(stripped, vec![&number(1), &code(1), &surface]);
    assert_eq!(holders, vec![None, Some(holder(1, 5)), None]);
    let plain = vec![number(1), surface];
    let (stripped, holders) = strip_held_markers(&plain).expect("no marker");
    assert_eq!(stripped, plain.iter().collect::<Vec<_>>());
    assert!(holders.is_empty());
}
