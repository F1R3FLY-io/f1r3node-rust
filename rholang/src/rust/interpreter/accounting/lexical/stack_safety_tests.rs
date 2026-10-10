//! G1-2 (DR-67): parity, metering, linear work and stack safety of the funding
//! resolver.
//!
//! The resolver of `lexical.rs` is a specialized machine with one scope stack.
//! These tests compare it with the recursive resolver that it replaced
//! (`resolve_lexical_names_for_funding_recursive`). The two resolvers produce
//! different terms outside the cost signatures: the recursive one substituted
//! every bound name of a resolved `new` into the whole body, and the machine
//! rewrites only the signatures that the funding analysis reads. So the
//! parity check compares the signatures of every walked position, the
//! analyzer's results on both outputs, and the first error.

use std::collections::{BTreeMap, HashMap};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::cost_signature::Value as CostSignatureValue;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{
    CostSignature, CostSignedTerm, GPrivate, GUnforgeable, New, Par, ReceiveBind,
};
use models::rust::host_work::{
    HostWorkDimension, HostWorkLimit, HostWorkLimits, HostWorkReservationError,
};
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;
use proptest::prelude::*;

use super::*;
use crate::rust::interpreter::accounting::delta_sigma::{
    demand_bound, static_authority_plan, static_authority_signatures,
};
use crate::rust::interpreter::accounting::Sig;
use crate::rust::interpreter::compiler::compiler::Compiler;
use crate::rust::interpreter::host_work::HostWorkBudget;

/// The nesting depth of the deep tests.
const DEEP_NESTING: usize = 100_000;

/// The stack of the thread that runs the resolver on a deep term.
const SMALL_STACK_BYTES: usize = 256 * 1024;

fn private_name(tag: u8) -> Par {
    Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id: vec![tag; 32] })),
    }])
}

/// The URN map of the generated programs. `rho:test:2` is missing on purpose,
/// so a `new` that binds it fails, and the first failure decides the error.
fn test_urn_map() -> HashMap<String, Par> {
    HashMap::from([
        ("rho:test:0".to_string(), private_name(0xa0)),
        ("rho:test:1".to_string(), private_name(0xa1)),
    ])
}

fn unlimited_budget() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX)))
}

// ── A generator of well-formed programs ────────────────────────────────────

#[derive(Clone, Debug)]
enum SigShape {
    /// A name in scope, chosen by index (a ground signature when no name is in
    /// scope).
    Scoped(u8),
    /// An unbound identifier, which the normalizer makes a ground signature.
    Ground(u8),
    /// A compound of a name in scope and a ground signature.
    Pair(u8, u8),
}

#[derive(Clone, Debug)]
enum Shape {
    Nil,
    /// `@"ping"!(0)`.
    Ping,
    /// A send on a name in scope.
    SendOn(u8),
    /// `@"data"!(P)`: the process `P` travels as data.
    Data(Box<Shape>),
    Parallel(Box<Shape>, Box<Shape>),
    /// `new` with fresh names and URN names.
    New {
        names: u8,
        urns: Vec<u8>,
        body: Box<Shape>,
    },
    /// `for (x <- @"in") { P }`.
    Receive(Box<Shape>),
    /// `match 7 { y => P _ => Q }`: the first case binds a process variable.
    Match(Box<Shape>, Box<Shape>),
    If(Box<Shape>, Box<Shape>),
    Bundle(Box<Shape>),
    Signed(SigShape, Box<Shape>),
    Stack(Vec<SigShape>),
    /// `*x` for the innermost receive-bound name, if one is in scope.
    Dequote,
}

fn arb_sig() -> impl Strategy<Value = SigShape> {
    prop_oneof![
        4 => any::<u8>().prop_map(SigShape::Scoped),
        2 => (0u8..4).prop_map(SigShape::Ground),
        1 => (any::<u8>(), 0u8..4).prop_map(|(name, ground)| SigShape::Pair(name, ground)),
    ]
}

fn arb_shape() -> impl Strategy<Value = Shape> {
    let leaf = prop_oneof![
        Just(Shape::Nil),
        Just(Shape::Ping),
        any::<u8>().prop_map(Shape::SendOn),
        (arb_sig(), Just(Shape::Nil)).prop_map(|(sig, body)| Shape::Signed(sig, Box::new(body))),
        prop::collection::vec(arb_sig(), 1..3).prop_map(Shape::Stack),
        Just(Shape::Dequote),
    ];
    leaf.prop_recursive(5, 48, 4, |inner| {
        prop_oneof![
            inner.clone().prop_map(|body| Shape::Data(Box::new(body))),
            (inner.clone(), inner.clone())
                .prop_map(|(left, right)| Shape::Parallel(Box::new(left), Box::new(right))),
            (0u8..3, prop::collection::vec(0u8..3, 0..2), inner.clone()).prop_map(
                |(names, urns, body)| Shape::New {
                    names,
                    urns,
                    body: Box::new(body),
                }
            ),
            inner
                .clone()
                .prop_map(|body| Shape::Receive(Box::new(body))),
            (inner.clone(), inner.clone())
                .prop_map(|(left, right)| Shape::Match(Box::new(left), Box::new(right))),
            (inner.clone(), inner.clone())
                .prop_map(|(left, right)| Shape::If(Box::new(left), Box::new(right))),
            inner.clone().prop_map(|body| Shape::Bundle(Box::new(body))),
            (arb_sig(), inner).prop_map(|(sig, body)| Shape::Signed(sig, Box::new(body))),
        ]
    })
}

/// A name in scope and whether a receive binds it.
struct Scope {
    names: Vec<(String, bool)>,
    fresh: usize,
}

impl Scope {
    fn fresh(&mut self, prefix: &str) -> String {
        self.fresh += 1;
        format!("{prefix}{}", self.fresh)
    }

    fn pick(&self, index: u8) -> String {
        match self.names.len() {
            0 => "g0".to_string(),
            len => self.names[usize::from(index) % len].0.clone(),
        }
    }
}

fn render_sig(sig: &SigShape, scope: &Scope) -> String {
    match sig {
        SigShape::Scoped(index) => scope.pick(*index),
        SigShape::Ground(tag) => format!("g{tag}"),
        SigShape::Pair(index, tag) => format!("{} (*) g{tag}", scope.pick(*index)),
    }
}

fn render(shape: &Shape, scope: &mut Scope, out: &mut String) {
    match shape {
        Shape::Nil => out.push_str("Nil"),
        Shape::Ping => out.push_str("@\"ping\"!(0)"),
        Shape::SendOn(index) => match scope.names.len() {
            0 => out.push_str("@\"ping\"!(1)"),
            _ => {
                out.push_str(&scope.pick(*index));
                out.push_str("!(2)");
            }
        },
        Shape::Data(body) => {
            out.push_str("@\"data\"!(");
            render(body, scope, out);
            out.push(')');
        }
        Shape::Parallel(left, right) => {
            render(left, scope, out);
            out.push_str(" | ");
            render(right, scope, out);
        }
        Shape::New { names, urns, body } => {
            let simple = usize::from(*names).max(usize::from(urns.is_empty()));
            let mut binders = Vec::with_capacity(simple + urns.len());
            for _ in 0..simple {
                binders.push((scope.fresh("n"), None));
            }
            for urn in urns {
                binders.push((scope.fresh("u"), Some(*urn)));
            }
            out.push_str("new ");
            for (position, (name, urn)) in binders.iter().enumerate() {
                if position > 0 {
                    out.push_str(", ");
                }
                out.push_str(name);
                if let Some(urn) = urn {
                    out.push_str(&format!("(`rho:test:{urn}`)"));
                }
            }
            out.push_str(" in { ");
            let before = scope.names.len();
            scope
                .names
                .extend(binders.into_iter().map(|(name, _)| (name, false)));
            render(body, scope, out);
            scope.names.truncate(before);
            out.push_str(" }");
        }
        Shape::Receive(body) => {
            let name = scope.fresh("x");
            out.push_str(&format!("for ({name} <- @\"in\") {{ "));
            scope.names.push((name, true));
            render(body, scope, out);
            scope.names.pop();
            out.push_str(" }");
        }
        Shape::Match(left, right) => {
            let variable = scope.fresh("y");
            out.push_str(&format!("match 7 {{ {variable} => {{ "));
            render(left, scope, out);
            out.push_str(" } _ => { ");
            render(right, scope, out);
            out.push_str(" } }");
        }
        Shape::If(left, right) => {
            out.push_str("if (true) { ");
            render(left, scope, out);
            out.push_str(" } else { ");
            render(right, scope, out);
            out.push_str(" }");
        }
        Shape::Bundle(body) => {
            out.push_str("bundle+ { ");
            render(body, scope, out);
            out.push_str(" }");
        }
        Shape::Signed(sig, body) => {
            out.push_str("{% ");
            render(body, scope, out);
            out.push_str(&format!(" %}}[ {} ]", render_sig(sig, scope)));
        }
        Shape::Stack(cells) => {
            for cell in cells {
                out.push_str(&render_sig(cell, scope));
                out.push_str(" :: ");
            }
            out.push_str("()");
        }
        Shape::Dequote => match scope.names.iter().rev().find(|(_, received)| *received) {
            Some((name, _)) => {
                out.push('*');
                out.push_str(name);
            }
            None => out.push_str("Nil"),
        },
    }
}

fn source_of(shape: &Shape) -> String {
    let mut scope = Scope {
        names: Vec::new(),
        fresh: 0,
    };
    let mut out = String::new();
    render(shape, &mut scope, &mut out);
    out
}

// ── Projections of a resolved program ──────────────────────────────────────

/// The signatures of every position that the funding analysis reads, in a
/// fixed depth-first order. The walk uses a heap work list.
fn walked_signatures(par: &Par) -> Vec<CostSignature> {
    let mut signatures = Vec::new();
    let mut pending = vec![par];
    while let Some(par) = pending.pop() {
        let mut children = Vec::new();
        for send in &par.sends {
            children.extend(send.data.iter());
        }
        for receive in &par.receives {
            signatures.extend(
                receive
                    .binds
                    .iter()
                    .filter_map(|bind| bind.cost_signature.clone()),
            );
            children.extend(receive.body.iter());
        }
        for new in &par.news {
            children.extend(new.p.iter());
        }
        for matched in &par.matches {
            children.extend(matched.cases.iter().filter_map(|case| case.source.as_ref()));
        }
        for conditional in &par.conditionals {
            children.extend(conditional.if_true.iter());
            children.extend(conditional.if_false.iter());
        }
        for bundle in &par.bundles {
            children.extend(bundle.body.iter());
        }
        for signed in &par.cost_signed_terms {
            signatures.extend(signed.signature.clone());
            children.extend(signed.body.iter());
        }
        for stack in &par.cost_stacks {
            signatures.extend(stack.cells.iter().cloned());
        }
        pending.extend(children.into_iter().rev());
    }
    signatures
}

/// The outcome of one parity check.
#[derive(Debug, PartialEq, Eq)]
enum Parity {
    /// The two resolvers agree on the first error, or on the walked
    /// signatures and the analyzer's results.
    Same,
    /// The recursive resolver substituted the whole body of a resolved `new`
    /// and failed on a position that the analyzer does not read. The machine
    /// rewrites only signatures, so it accepted the program (DR-67
    /// implementation note 2).
    HeadSubstitutionFailure,
}

/// Compares the two resolvers on one program: the first error, or the walked
/// signatures and the analyzer's results. It also compares the metered result
/// with the unmetered one.
fn resolver_parity(
    program: &Par,
    rand: &Blake2b512Random,
    urn_map: &HashMap<String, Par>,
    label: &str,
) -> Parity {
    let machine = resolve_lexical_names_for_funding(program, rand.clone(), urn_map);
    let recursive = resolve_lexical_names_for_funding_recursive(program, rand.clone(), urn_map);
    let budget = unlimited_budget();
    let metered =
        resolve_lexical_names_for_funding_metered(program, rand.clone(), urn_map, &budget);
    assert_eq!(metered, machine, "metered and unmetered results of {label}");
    match (machine, recursive) {
        (Ok(machine), Ok(recursive)) => {
            assert_eq!(
                walked_signatures(&machine),
                walked_signatures(&recursive),
                "signatures of {label}"
            );
            let deploy = Sig::Ground(b"deployer".to_vec());
            assert_eq!(
                static_authority_plan(&machine, &deploy),
                static_authority_plan(&recursive, &deploy),
                "authority plan of {label}"
            );
            assert_eq!(
                demand_bound(&machine, &deploy),
                demand_bound(&recursive, &deploy),
                "demand bound of {label}"
            );
            assert_eq!(
                static_authority_signatures(&machine),
                static_authority_signatures(&recursive),
                "static signatures of {label}"
            );
            Parity::Same
        }
        (Ok(_), Err(InterpreterError::SubstituteError(message)))
            if message.starts_with("Illegal Substitution") =>
        {
            Parity::HeadSubstitutionFailure
        }
        (machine, recursive) => {
            assert_eq!(machine.err(), recursive.err(), "first error of {label}");
            Parity::Same
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn machine_resolver_matches_the_recursive_resolver(
        shape in arb_shape(),
        seed in prop::collection::vec(any::<u8>(), 1..16),
    ) {
        let source = source_of(&shape);
        let program = Compiler::source_to_adt(&source)
            .unwrap_or_else(|error| panic!("generated program {source} does not compile: {error}"));
        prop_assert_eq!(term_count(&program), evaluation_terms(&program).len());
        prop_assert_eq!(
            resolver_parity(
                &program,
                &Blake2b512Random::create_from_bytes(&seed),
                &test_urn_map(),
                &source,
            ),
            Parity::Same
        );
    }
}

/// The URNs that a program binds, collected with a heap work list.
fn bound_urns(program: &Par, urns: &mut BTreeMap<String, Par>) {
    let mut pending = vec![program];
    while let Some(par) = pending.pop() {
        for send in &par.sends {
            pending.extend(send.data.iter());
        }
        for receive in &par.receives {
            pending.extend(receive.body.iter());
        }
        for new in &par.news {
            for urn in &new.uri {
                let tag = u8::try_from(urns.len() % 251).expect("small tag");
                urns.entry(urn.clone()).or_insert_with(|| private_name(tag));
            }
            pending.extend(new.p.iter());
        }
        for matched in &par.matches {
            pending.extend(matched.cases.iter().filter_map(|case| case.source.as_ref()));
        }
        for conditional in &par.conditionals {
            pending.extend(conditional.if_true.iter());
            pending.extend(conditional.if_false.iter());
        }
        for bundle in &par.bundles {
            pending.extend(bundle.body.iter());
        }
        for signed in &par.cost_signed_terms {
            pending.extend(signed.body.iter());
        }
    }
}

/// Normalizes one corpus source. The external parser panics on syntax that it
/// does not implement (`select` in `examples/old/*/Cell2.rho`), so a panic
/// counts as a rejection, as an error does.
fn compile_corpus_source(source: &str) -> Option<Par> {
    std::panic::catch_unwind(|| Compiler::source_to_adt(source))
        .ok()
        .and_then(Result::ok)
}

/// Parity on every corpus program that the normalizer accepts, once with the
/// URNs of the corpus bound and once with an empty URN map, which makes each
/// URN binding fail. One corpus program shows the deliberate deviation of
/// `Parity::HeadSubstitutionFailure`: `test-matches.rho` sends `1 matches _`
/// inside a resolved `new`, and the recursive resolver failed to substitute
/// the wildcard pattern.
#[test]
fn machine_resolver_matches_the_recursive_resolver_on_the_corpus() {
    let crate_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut pending = vec![
        crate_root.join("../examples"),
        crate_root.join("examples"),
        crate_root.join("src/test/resources/tests"),
        crate_root.join("../casper/src/main/resources"),
    ];
    let mut programs = Vec::new();
    while let Some(path) = pending.pop() {
        if path.is_dir() {
            let mut entries = std::fs::read_dir(&path)
                .expect("read a corpus directory")
                .map(|entry| entry.expect("read a corpus entry").path())
                .collect::<Vec<_>>();
            entries.sort();
            pending.extend(entries);
        } else if path.extension().is_some_and(|extension| extension == "rho") {
            let source = std::fs::read_to_string(&path).expect("read a corpus source");
            if let Some(program) = compile_corpus_source(&source) {
                programs.push((path, program));
            }
        }
    }
    assert!(
        programs.len() >= 40,
        "only {} corpus programs compiled",
        programs.len()
    );
    let mut urns = BTreeMap::new();
    for (_, program) in &programs {
        bound_urns(program, &mut urns);
    }
    let urn_map = urns.into_iter().collect::<HashMap<_, _>>();
    let rand = Blake2b512Random::create_from_bytes(b"corpus resolver parity");
    let mut deviations = Vec::new();
    for (path, program) in &programs {
        let label = path
            .strip_prefix(crate_root)
            .unwrap_or(path)
            .display()
            .to_string();
        for urns in [&urn_map, &HashMap::new()] {
            match resolver_parity(program, &rand, urns, &label) {
                Parity::Same => {}
                Parity::HeadSubstitutionFailure => deviations.push(label.clone()),
            }
        }
    }
    assert_eq!(
        deviations,
        vec!["src/test/resources/tests/test-matches.rho".to_string()],
        "the deliberate deviation appears only where the recursive resolver failed"
    );
}

/// HEAD substituted the whole body of a resolved `new`. The wildcard pattern
/// of `1 matches _` in send data failed that substitution, although the
/// analyzer never reads it. The machine rewrites only signatures, so it
/// resolves the signature of `x` and accepts the program.
#[test]
fn machine_does_not_substitute_positions_that_the_analyzer_does_not_read() {
    let program = Compiler::source_to_adt(r#"new x in { @"data"!(1 matches _) | {% Nil %}[ x ] }"#)
        .expect("wildcard program");
    let rand = Blake2b512Random::create_from_bytes(b"wildcard pattern");
    let machine = resolve_lexical_names_for_funding(&program, rand.clone(), &HashMap::new())
        .expect("the machine resolves the program");
    let head = resolve_lexical_names_for_funding_recursive(&program, rand.clone(), &HashMap::new());
    assert!(
        matches!(
            &head,
            Err(InterpreterError::SubstituteError(message))
                if message.starts_with("Illegal Substitution")
        ),
        "the whole-body substitution of HEAD fails: {head:?}"
    );
    // The root has one term, so `new x` allocates x from the root randomness.
    let mut name_rand = rand;
    let x = private_name_from(name_rand.next());
    let signature = machine.news[0]
        .p
        .as_ref()
        .expect("body of x")
        .cost_signed_terms[0]
        .signature
        .as_ref()
        .expect("signature")
        .value
        .clone();
    assert_eq!(
        signature,
        Some(CostSignatureValue::Name(
            ParSortMatcher::sort_match(&x).term
        ))
    );
}

fn private_name_from(id: Vec<i8>) -> Par {
    Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
            id: id.into_iter().map(|byte| byte as u8).collect(),
        })),
    }])
}

/// A dequotation `*x` is a term of the reducer's schedule. The recursive
/// resolver substituted a dequotation of a resolved name away before it
/// counted the terms of the node, so it gave a sibling `new` another split of
/// the randomness than the reducer gives it. The machine counts the terms of
/// the input node, as the reducer does, and it still resolves the dequotation.
#[test]
fn machine_follows_the_reducer_schedule_beside_a_resolved_dequotation() {
    let program = Compiler::source_to_adt(r#"new x in { *x | new y in { {% Nil %}[ y ] } }"#)
        .expect("dequotation program");
    let rand = Blake2b512Random::create_from_bytes(b"dequoted sibling");
    // The root has one term, so `new x` allocates x from the root randomness.
    // Its body has two terms, `new y` (index 0) and `*x` (index 1).
    let mut body_rand = rand.clone();
    let _x = body_rand.next();
    let mut y_rand = body_rand.split_byte(0);
    let y = private_name_from(y_rand.next());
    let expected = Some(CostSignatureValue::Name(
        ParSortMatcher::sort_match(&y).term,
    ));
    let signature_of_y = |resolved: &Par| {
        resolved.news[0].p.as_ref().expect("body of x").news[0]
            .p
            .as_ref()
            .expect("body of y")
            .cost_signed_terms[0]
            .signature
            .as_ref()
            .expect("signature")
            .value
            .clone()
    };
    let machine = resolve_lexical_names_for_funding(&program, rand.clone(), &HashMap::new())
        .expect("the machine resolves the program");
    let recursive = resolve_lexical_names_for_funding_recursive(&program, rand, &HashMap::new())
        .expect("the recursive resolver resolves the program");
    assert_eq!(signature_of_y(&machine), expected);
    assert_ne!(
        signature_of_y(&recursive),
        expected,
        "the recursive resolver used the split of a one-term node"
    );
    let deploy = Sig::Ground(b"deployer".to_vec());
    assert_eq!(
        static_authority_plan(&machine, &deploy).is_ok(),
        static_authority_plan(&recursive, &deploy).is_ok(),
        "both resolve the dequotation of x"
    );
    assert!(static_authority_plan(&machine, &deploy).is_ok());
}

/// The linear helper binds the values of `util::allocate_new_bindings`, in the
/// same order, with the same errors.
#[test]
fn new_binding_values_match_allocate_new_bindings() {
    let urn_map = test_urn_map();
    let injected = Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id: vec![0xb0; 32] })),
    }]);
    let resolver = FundingResolver::new(&urn_map, None);
    for bind_count in -1i32..6 {
        for uri in [
            Vec::new(),
            vec!["rho:test:0".to_string()],
            vec!["rho:test:1".to_string(), "rho:test:injected".to_string()],
            vec!["rho:test:2".to_string()],
        ] {
            let new = New {
                bind_count,
                p: Some(Par::default()),
                uri,
                injections: BTreeMap::from([("rho:test:injected".to_string(), injected.clone())]),
                locally_free: Vec::new(),
            };
            let mut machine_rand = Blake2b512Random::create_from_bytes(b"binding values");
            let mut reducer_rand = machine_rand.clone();
            let machine = resolver.new_binding_values(&new, &mut machine_rand);
            let reducer = allocate_new_bindings(&new, &Env::new(), &mut reducer_rand, &urn_map);
            match (machine, reducer) {
                (Ok(values), Ok(env)) => {
                    let expected = (0..env.level)
                        .map(|position| env.env_map.get(&position).cloned())
                        .collect::<Vec<_>>();
                    assert_eq!(values.into_iter().map(Some).collect::<Vec<_>>(), expected);
                    assert_eq!(machine_rand, reducer_rand, "the same randomness remains");
                }
                (machine, reducer) => assert_eq!(machine.err(), reducer.err()),
            }
        }
    }
}

// ── Metering ───────────────────────────────────────────────────────────────

/// `new n0 in { {% Nil %}[ n0 ] | new n1 in { ... } }`, nested `depth` levels.
fn nested_new_source(depth: usize) -> String {
    let mut source = String::with_capacity(depth * 40);
    for level in 0..depth {
        source.push_str(&format!("new n{level} in {{ {{% Nil %}}[ n{level} ] | "));
    }
    source.push_str("Nil");
    for _ in 0..depth {
        source.push_str(" }");
    }
    source
}

fn usages_of(program: &Par) -> Vec<u64> {
    let budget = unlimited_budget();
    resolve_lexical_names_for_funding_metered(
        program,
        Blake2b512Random::create_from_bytes(b"linear work"),
        &HashMap::new(),
        &budget,
    )
    .expect("an unlimited budget resolves the program");
    HostWorkDimension::ALL
        .iter()
        .map(|dimension| budget.usage(*dimension).get())
        .collect()
}

/// Every host-work dimension grows by the same amount for each added level of
/// nesting, so the charged work is affine in the depth. The recursive resolver
/// substituted the whole remaining body at each level, a quadratic amount of
/// work for the same programs.
#[test]
fn charged_work_is_linear_in_the_nesting_depth() {
    let depths = [32usize, 64, 128];
    let usages = depths
        .iter()
        .map(|depth| {
            let program =
                Compiler::source_to_adt(&nested_new_source(*depth)).expect("nested program");
            usages_of(&program)
        })
        .collect::<Vec<_>>();
    for (position, dimension) in HostWorkDimension::ALL.iter().enumerate() {
        let first_step = usages[1][position] - usages[0][position];
        let second_step = usages[2][position] - usages[1][position];
        assert_eq!(
            second_step,
            2 * first_step,
            "{dimension} grows linearly: {:?}",
            usages
                .iter()
                .map(|usage| usage[position])
                .collect::<Vec<_>>()
        );
    }
    let items = HostWorkDimension::ALL
        .iter()
        .position(|dimension| *dimension == HostWorkDimension::StructuralItems)
        .expect("structural items");
    assert!(usages[0][items] > 0, "the resolver charges its steps");
}

/// A budget one unit below the work of the resolver in any charged dimension
/// rejects the program, and a budget equal to the work accepts it with the
/// same result.
#[test]
fn an_exhausted_budget_rejects_the_program() {
    let program = Compiler::source_to_adt(
        r#"new slot, other(`rho:test:0`) in { {% @"x"!(0) | for (y <- @"in") { {% Nil %}[ y ] } %}[ slot (*) g1 ] | slot :: other :: () | @"data"!({% Nil %}[ other ]) | match 7 { z => { {% Nil %}[ slot ] } _ => Nil } }"#,
    )
    .expect("metering program");
    let rand = Blake2b512Random::create_from_bytes(b"exhausted budget");
    let urn_map = test_urn_map();
    let expected = resolve_lexical_names_for_funding(&program, rand.clone(), &urn_map)
        .expect("the program resolves");
    let budget = unlimited_budget();
    resolve_lexical_names_for_funding_metered(&program, rand.clone(), &urn_map, &budget)
        .expect("an unlimited budget resolves the program");
    let mut charged = 0;
    for dimension in HostWorkDimension::ALL {
        let usage = budget.usage(dimension).get();
        if usage == 0 {
            continue;
        }
        charged += 1;
        let mut short = HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX));
        short.set(dimension, HostWorkLimit::new(usage - 1));
        let short_budget = HostWorkBudget::new(short);
        assert_eq!(
            resolve_lexical_names_for_funding_metered(
                &program,
                rand.clone(),
                &urn_map,
                &short_budget
            ),
            Err(InterpreterError::HostWorkRejected),
            "{dimension} one unit short"
        );
        assert!(matches!(
            short_budget.rejection(),
            Some(HostWorkReservationError::LimitExceeded { .. })
        ));

        let mut exact = HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX));
        exact.set(dimension, HostWorkLimit::new(usage));
        assert_eq!(
            resolve_lexical_names_for_funding_metered(
                &program,
                rand.clone(),
                &urn_map,
                &HostWorkBudget::new(exact)
            ),
            Ok(expected.clone()),
            "{dimension} exactly enough"
        );
    }
    assert!(
        charged >= 5,
        "the resolver charges steps, bindings and backing"
    );
}

// ── Stack safety ───────────────────────────────────────────────────────────

/// Drops a deep term on a small-stack thread with the resolver's iterative
/// teardown.
fn dismantle_on_small_stack(pars: Vec<Par>) {
    std::thread::Builder::new()
        .name("lexical-teardown".to_string())
        .stack_size(SMALL_STACK_BYTES)
        .spawn(move || dismantle(pars))
        .expect("spawn the teardown thread")
        .join()
        .expect("the iterative teardown stays within a small stack");
}

/// `depth` nested `new` terms. Each level binds one name and signs an empty
/// term with the name of the enclosing `new` (bound level 0). The loop builds
/// the term from the inside out, so construction never recurses.
fn deep_new_chain(depth: usize) -> Par {
    let mut par = Par::default();
    for _ in 0..depth {
        let mut outer = Par::default();
        outer.news.push(New {
            bind_count: 1,
            p: Some(par),
            uri: Vec::new(),
            injections: BTreeMap::new(),
            locally_free: Vec::new(),
        });
        outer.cost_signed_terms.push(CostSignedTerm {
            body: Some(Par::default()),
            signature: Some(CostSignature {
                value: Some(CostSignatureValue::BoundLevel(0)),
            }),
        });
        par = outer;
    }
    par
}

/// The resolver allocates the name of each of 100,000 nested `new` terms on a
/// 256 KiB stack. The expected names follow the reducer's rule: each level has
/// two terms, the `new` gets split 0 of the level's randomness, allocates one
/// name from it, and passes what remains to its body.
#[test]
fn resolver_allocates_one_hundred_thousand_nested_names_on_a_small_stack() {
    let rand = Blake2b512Random::create_from_bytes(b"deep resolver");
    let program = deep_new_chain(DEEP_NESTING);
    let budget = unlimited_budget();
    let resolved = std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("lexical-small-stack".to_string())
            .stack_size(SMALL_STACK_BYTES)
            .spawn_scoped(scope, || {
                resolve_lexical_names_for_funding_metered(
                    &program,
                    rand.clone(),
                    &HashMap::new(),
                    &budget,
                )
            })
            .expect("spawn the small-stack thread")
            .join()
    });
    let resolved = resolved
        .expect("the resolver stays within a small stack")
        .expect("the resolver accepts the chain");

    let mut expected = Vec::with_capacity(DEEP_NESTING);
    let mut level_rand = rand;
    for _ in 0..DEEP_NESTING {
        let mut term_rand = level_rand.split_byte(0);
        let id = term_rand
            .next()
            .into_iter()
            .map(|byte| byte as u8)
            .collect::<Vec<_>>();
        let name = Par::default().with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id })),
        }]);
        expected.push(ParSortMatcher::sort_match(&name).term);
        level_rand = term_rand;
    }

    let mut mismatches = 0usize;
    let mut levels = 0usize;
    let mut current = Some(&resolved);
    while let Some(par) = current {
        let signature = par
            .cost_signed_terms
            .first()
            .and_then(|signed| signed.signature.as_ref())
            .map(|signature| signature.value.clone());
        let want = match levels {
            0 => Some(Some(CostSignatureValue::BoundLevel(0))),
            level if level <= DEEP_NESTING => {
                Some(Some(CostSignatureValue::Name(expected[level - 1].clone())))
            }
            _ => None,
        };
        if signature != want && !(levels == DEEP_NESTING && signature.is_none()) {
            mismatches += 1;
        }
        levels += 1;
        current = par.news.first().and_then(|new| new.p.as_ref());
    }
    dismantle_on_small_stack(vec![program, resolved]);
    assert_eq!(levels, DEEP_NESTING + 1);
    assert_eq!(mismatches, 0, "every level carries the name of its `new`");
}

/// Compares two terms without recursion: every walked child pairwise, and
/// every other field by value. The other fields of the deep terms are shallow.
fn same_term(left: &Par, right: &Par) -> bool {
    let mut pending = vec![(left, right)];
    while let Some((left, right)) = pending.pop() {
        if left.sends.len() != right.sends.len()
            || left.receives.len() != right.receives.len()
            || left.news.len() != right.news.len()
            || left.matches.len() != right.matches.len()
            || left.conditionals.len() != right.conditionals.len()
            || left.bundles.len() != right.bundles.len()
            || left.cost_signed_terms.len() != right.cost_signed_terms.len()
            || left.exprs != right.exprs
            || left.unforgeables != right.unforgeables
            || left.connectives != right.connectives
            || left.locally_free != right.locally_free
            || left.connective_used != right.connective_used
            || left.cost_stacks != right.cost_stacks
        {
            return false;
        }
        for (left, right) in left.sends.iter().zip(&right.sends) {
            if left.chan != right.chan
                || left.persistent != right.persistent
                || left.data.len() != right.data.len()
            {
                return false;
            }
            pending.extend(left.data.iter().zip(&right.data));
        }
        for (left, right) in left.receives.iter().zip(&right.receives) {
            let binds = |binds: &[ReceiveBind]| {
                binds
                    .iter()
                    .map(|bind| {
                        (
                            bind.patterns.clone(),
                            bind.source.clone(),
                            bind.cost_signature.clone(),
                        )
                    })
                    .collect::<Vec<_>>()
            };
            if binds(&left.binds) != binds(&right.binds)
                || left.persistent != right.persistent
                || left.bind_count != right.bind_count
                || left.body.is_some() != right.body.is_some()
            {
                return false;
            }
            pending.extend(left.body.iter().zip(right.body.iter()));
        }
        for (left, right) in left.news.iter().zip(&right.news) {
            if left.bind_count != right.bind_count
                || left.uri != right.uri
                || left.p.is_some() != right.p.is_some()
            {
                return false;
            }
            pending.extend(left.p.iter().zip(right.p.iter()));
        }
        for (left, right) in left.matches.iter().zip(&right.matches) {
            if left.target != right.target || left.cases.len() != right.cases.len() {
                return false;
            }
            for (left, right) in left.cases.iter().zip(&right.cases) {
                if left.pattern != right.pattern
                    || left.free_count != right.free_count
                    || left.source.is_some() != right.source.is_some()
                {
                    return false;
                }
                pending.extend(left.source.iter().zip(right.source.iter()));
            }
        }
        for (left, right) in left.conditionals.iter().zip(&right.conditionals) {
            if left.condition != right.condition
                || left.if_true.is_some() != right.if_true.is_some()
                || left.if_false.is_some() != right.if_false.is_some()
            {
                return false;
            }
            pending.extend(left.if_true.iter().zip(right.if_true.iter()));
            pending.extend(left.if_false.iter().zip(right.if_false.iter()));
        }
        for (left, right) in left.bundles.iter().zip(&right.bundles) {
            if left.write_flag != right.write_flag
                || left.read_flag != right.read_flag
                || left.body.is_some() != right.body.is_some()
            {
                return false;
            }
            pending.extend(left.body.iter().zip(right.body.iter()));
        }
        for (left, right) in left.cost_signed_terms.iter().zip(&right.cost_signed_terms) {
            if left.signature != right.signature || left.body.is_some() != right.body.is_some() {
                return false;
            }
            pending.extend(left.body.iter().zip(right.body.iter()));
        }
    }
    true
}

/// A term nested 100,000 levels deep through every construct that the
/// resolver walks: send data, receive bodies, `new` in data position, match
/// cases, `if` branches, bundles and signed terms. Its signatures are ground,
/// so the resolved term equals the input.
fn deep_mixed_term(depth: usize) -> Par {
    let ground = || CostSignature {
        value: Some(CostSignatureValue::Ground(vec![7])),
    };
    let mut par = Par::default();
    for level in 0..depth {
        let mut outer = Par::default();
        match level % 6 {
            0 => outer.sends.push(models::rhoapi::Send {
                chan: Some(private_name(1)),
                data: vec![par],
                persistent: false,
                locally_free: Vec::new(),
                connective_used: false,
            }),
            1 => outer.receives.push(models::rhoapi::Receive {
                binds: vec![ReceiveBind {
                    patterns: Vec::new(),
                    source: Some(private_name(2)),
                    remainder: None,
                    free_count: 1,
                    cost_signature: Some(ground()),
                }],
                body: Some(par),
                persistent: false,
                peek: false,
                bind_count: 1,
                locally_free: Vec::new(),
                connective_used: false,
                condition: None,
            }),
            2 => outer.news.push(New {
                bind_count: 2,
                p: Some(par),
                uri: Vec::new(),
                injections: BTreeMap::new(),
                locally_free: Vec::new(),
            }),
            3 => outer.matches.push(models::rhoapi::Match {
                target: Some(Par::default()),
                cases: vec![models::rhoapi::MatchCase {
                    pattern: Some(Par::default()),
                    source: Some(par),
                    free_count: 1,
                    guard: None,
                }],
                locally_free: Vec::new(),
                connective_used: false,
            }),
            4 => outer.conditionals.push(models::rhoapi::If {
                condition: Some(Par::default()),
                if_true: Some(par),
                if_false: Some(Par::default()),
                locally_free: Vec::new(),
                connective_used: false,
            }),
            _ => outer.cost_signed_terms.push(CostSignedTerm {
                body: Some(par),
                signature: Some(ground()),
            }),
        }
        outer.bundles.push(models::rhoapi::Bundle {
            body: Some(Par::default()),
            write_flag: true,
            read_flag: true,
        });
        par = outer;
    }
    par
}

#[test]
fn resolver_walks_a_term_nested_one_hundred_thousand_deep_on_a_small_stack() {
    let program = deep_mixed_term(DEEP_NESTING);
    let budget = unlimited_budget();
    let resolved = std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("lexical-mixed-small-stack".to_string())
            .stack_size(SMALL_STACK_BYTES)
            .spawn_scoped(scope, || {
                let resolved = resolve_lexical_names_for_funding_metered(
                    &program,
                    Blake2b512Random::create_from_bytes(b"deep mixed resolver"),
                    &test_urn_map(),
                    &budget,
                );
                let same = resolved
                    .as_ref()
                    .ok()
                    .map(|resolved| same_term(&program, resolved));
                (resolved, same)
            })
            .expect("spawn the small-stack thread")
            .join()
    });
    let (resolved, same) = resolved.expect("the resolver stays within a small stack");
    let resolved = resolved.expect("the resolver accepts the term");
    dismantle_on_small_stack(vec![program, resolved]);
    assert_eq!(same, Some(true), "ground signatures resolve to themselves");
}
