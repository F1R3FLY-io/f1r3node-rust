//! # A value must equal itself — and the compiler, not this file, is what keeps it true
//!
//! `models/src/lib.rs` hand-writes 124 `PartialEq`/`Hash` impls. Five of the
//! `PartialEq`s — for the prost `oneof`s [`TaggedCont`], [`VarInstance`],
//! [`ExprInstance`], [`ConnectiveInstance`] and [`UnfInstance`] — used to end in
//! a `_ => false` catch-all, and a sixth lived in the sorter's [`Tree`]
//! (`_ => return false`).
//!
//! ## Why that was a live defect and not a style question
//!
//! A catch-all makes a match exhaustive **to the compiler**. These five enums
//! are generated from `models/src/main/protobuf/RhoTypes.proto`, so they gain
//! arms whenever the schema grows. With the catch-all in place a new arm
//! compiled, fell through to `_ => false`, and compared **unequal to itself**:
//!
//! ```text
//!     let x = ExprInstance::GBlake2b256Hash(h);
//!     x == x    // false
//! ```
//!
//! That breaks reflexivity, and reflexivity is not advisory here. The generated
//! declarations carry `#[derive(Eq, Ord, PartialOrd)]` over the **hand-written**
//! `PartialEq`, so `Eq` — whose entire content is the promise `∀x. x == x` — is
//! asserted for exactly the code that would stop honouring it. Live consumers of
//! that promise include `HashSet<Par>` (`models/src/rust/sorted_par_hash_set.rs`)
//! and `impl Eq for EPathMap` (`models/src/rust/rhoapi_ext.rs`), which reaches
//! `ExprInstance` through `EPathMap → Par → Expr`. A non-reflexive key makes a
//! hash lookup miss an entry that is present, a `dedup` keep duplicates, and a
//! `sort` produce any order it likes — with no error anywhere.
//!
//! ## What actually enforces it: exhaustiveness
//!
//! **This file is not the guard.** The guard is the absence of a catch-all: with
//! the residue arm spelled out per variant, a 37th variant leaves the pair
//! `(Z(_), _)` uncovered and the build stops with `E0004`. Measured, by adding a
//! third variant to [`Tree`] and compiling `models` both ways:
//!
//! | `Tree` has | `eq` ends in | `score_tree.rs` E0004 sites |
//! |---|---|---|
//! | 2 variants | `_ => return false` | — (compiles) |
//! | 2 variants | the enumerated residue | — (compiles) |
//! | **3** variants | `_ => return false` | `:72` (`Clone`), `:318` (`compare_score`) — **`eq` is silent** |
//! | **3** variants | the enumerated residue | `:72`, `:318`, **and `:112` — `eq`** |
//!
//! The same 2×2, run against the real 36-arm `ExprInstance` table lifted
//! verbatim out of `models/src/lib.rs` onto a stub enum: with the residue arm a
//! 37th variant is rejected as ``(&ExprInstance::GBlake2b256Hash(_), _)` not
//! covered`; with `_ => false` it compiles clean. The mutation was reverted; the
//! transcripts are the record.
//!
//! ## What this file adds, which the compiler cannot
//!
//! Three things.
//!
//! 1. [`every_oneof_variant_equals_itself`] and
//!    [`score_tree_values_equal_themselves`] exercise the property directly for
//!    every variant that exists today, with `Hash` agreement alongside — because
//!    `Eq` and `Hash` must agree, and a partial `hash` would be the same defect
//!    in the form `HashMap` corrupts fastest.
//!
//! 2. [`every_oneof_variant_has_a_same_variant_eq_arm_and_a_hash_arm`] closes
//!    the one gap exhaustiveness leaves open. A maintainer who answers `E0004`
//!    by extending the **residue** list alone gets a variant that still compares
//!    unequal to itself, and it compiles. This derives the authoritative variant
//!    names from the generated `bincode_schema_tables::*_VARIANTS` tables — the same
//!    tables the codec is driven from — and requires each to own a same-variant
//!    `eq` arm and a `hash` arm.
//!
//! 3. [`no_comparison_impl_carries_an_irrefutable_arm`] scans every `src/` tree
//!    in the workspace so the catch-all cannot come back anywhere, in any
//!    comparison trait, and records the two classified exceptions as data with
//!    reasons rather than as absences nobody looked for.
//!
//! ## ⚠ On guarded catch-alls
//!
//! A `(a, b) if <cond> => false` arm looks like an enumerating alternative and
//! is not one: rustc **ignores guarded arms when computing exhaustiveness**, so
//! such an arm cannot make a match exhaustive and cannot hide a new variant
//! either. The scanner therefore skips guarded arms rather than flagging them —
//! they are not this defect, and flagging them would be noise that erodes the
//! gate.
//!
//! ## Anti-vacuity
//!
//! Both scanners are perturbed in memory and required to reject the
//! perturbation, and required to accept the unperturbed text — so a scanner that
//! rejected everything, or one that had stopped matching the source at all,
//! fails here rather than looking strict.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::var::{VarInstance, WildcardMsg};
use models::rhoapi::{
    ConnectiveBody, EAnd, EDiv, EEq, EGt, EGte, EList, ELt, ELte, EMap, EMatches, EMethod, EMinus,
    EMinusMinus, EMod, EMult, ENeg, ENeq, ENot, EOr, EPathMap, EPercentPercent, EPlus, EPlusPlus,
    ESet, ETuple, EVar, EZipper, GBigRational, GDeployId, GDeployerId, GFixedPoint, GPrivate,
    GSysAuthToken, Par, ParWithRandom, VarRef,
};
use models::rust::rholang::bincode_schema_tables::{
    CONNECTIVE_INSTANCE_VARIANTS, EXPR_INSTANCE_VARIANTS, TAGGED_CONT_VARIANTS,
    UNF_INSTANCE_VARIANTS, VAR_INSTANCE_VARIANTS,
};
use models::rust::rholang::sorter::score_tree::Tree;

// ===========================================================================
// §1  The property, exercised on every variant that exists today
// ===========================================================================

fn hash_of<T: Hash>(v: &T) -> u64 {
    let mut h = DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

/// `PartialEq::eq`, named. Reflexivity is the instance where both arguments are
/// the same value: `compares_equal(v, v)`.
///
/// ⚠ Two parameters rather than `x == x` written inline, and NOT as a way to
/// silence a lint. `clippy::eq_op` rejects `x == x` on the sound general ground
/// that it is a tautology the author did not mean — everywhere except here,
/// where it is precisely what is meant. `#[allow(clippy::eq_op)]` would switch
/// the lint off for a whole scope including the places it would be right about,
/// which is a worse trade than naming the operation once. The call still runs
/// `<T as PartialEq>::eq(v, v)`; nothing is weakened.
fn compares_equal<T: PartialEq>(a: &T, b: &T) -> bool { a == b }

/// `x == x`, `x == x.clone()`, and `hash(x) == hash(x.clone())` for one value.
fn reflexive_and_hash_agreeing<T>(label: &str, v: &T)
where T: PartialEq + Clone + Hash + std::fmt::Debug {
    assert!(
        compares_equal(v, v),
        "★ REFLEXIVITY IS BROKEN for `{label}`: a value does not equal itself ({v:?}). Every \
         `HashSet`, `HashMap`, `dedup` and `sort` holding one is now silently wrong, and the `Eq` \
         derived over this `PartialEq` is an unsound claim."
    );
    let c = v.clone();
    assert_eq!(
        v, &c,
        "★ `{label}` does not equal its own clone, so equality is reading something `Clone` does \
         not carry"
    );
    assert_eq!(
        hash_of(v),
        hash_of(&c),
        "★ `Eq`/`Hash` DISAGREE for `{label}`: two values that compare equal hash differently. \
         `HashMap` will store both and find neither reliably."
    );
}

/// One value of every [`TaggedCont`] variant, paired with its variant name.
fn tagged_cont_values() -> Vec<(&'static str, TaggedCont)> {
    vec![
        ("ParBody", TaggedCont::ParBody(ParWithRandom::default())),
        ("ScalaBodyRef", TaggedCont::ScalaBodyRef(7)),
    ]
}

/// One value of every [`VarInstance`] variant.
fn var_instance_values() -> Vec<(&'static str, VarInstance)> {
    vec![
        ("BoundVar", VarInstance::BoundVar(3)),
        ("FreeVar", VarInstance::FreeVar(5)),
        ("Wildcard", VarInstance::Wildcard(WildcardMsg {})),
    ]
}

/// One value of every [`ExprInstance`] variant.
///
/// ⚠ Order follows `EXPR_INSTANCE_VARIANTS`, and
/// [`every_oneof_variant_equals_itself`] checks the names against that table, so
/// a schema change cannot leave this list quietly short.
fn expr_instance_values() -> Vec<(&'static str, ExprInstance)> {
    vec![
        ("GBool", ExprInstance::GBool(true)),
        ("GInt", ExprInstance::GInt(11)),
        ("GString", ExprInstance::GString("s".to_string())),
        ("GUri", ExprInstance::GUri("rho:io:stdout".to_string())),
        ("GByteArray", ExprInstance::GByteArray(vec![1, 2, 3])),
        ("ENotBody", ExprInstance::ENotBody(ENot::default())),
        ("ENegBody", ExprInstance::ENegBody(ENeg::default())),
        ("EMultBody", ExprInstance::EMultBody(EMult::default())),
        ("EDivBody", ExprInstance::EDivBody(EDiv::default())),
        ("EPlusBody", ExprInstance::EPlusBody(EPlus::default())),
        ("EMinusBody", ExprInstance::EMinusBody(EMinus::default())),
        ("ELtBody", ExprInstance::ELtBody(ELt::default())),
        ("ELteBody", ExprInstance::ELteBody(ELte::default())),
        ("EGtBody", ExprInstance::EGtBody(EGt::default())),
        ("EGteBody", ExprInstance::EGteBody(EGte::default())),
        ("EEqBody", ExprInstance::EEqBody(EEq::default())),
        ("ENeqBody", ExprInstance::ENeqBody(ENeq::default())),
        ("EAndBody", ExprInstance::EAndBody(EAnd::default())),
        ("EOrBody", ExprInstance::EOrBody(EOr::default())),
        ("EVarBody", ExprInstance::EVarBody(EVar::default())),
        ("EListBody", ExprInstance::EListBody(EList::default())),
        ("ETupleBody", ExprInstance::ETupleBody(ETuple::default())),
        ("ESetBody", ExprInstance::ESetBody(ESet::default())),
        ("EMapBody", ExprInstance::EMapBody(EMap::default())),
        ("EMethodBody", ExprInstance::EMethodBody(EMethod::default())),
        (
            "EPathmapBody",
            ExprInstance::EPathmapBody(EPathMap::default()),
        ),
        ("EZipperBody", ExprInstance::EZipperBody(EZipper::default())),
        (
            "EMatchesBody",
            ExprInstance::EMatchesBody(EMatches::default()),
        ),
        (
            "EPercentPercentBody",
            ExprInstance::EPercentPercentBody(EPercentPercent::default()),
        ),
        (
            "EPlusPlusBody",
            ExprInstance::EPlusPlusBody(EPlusPlus::default()),
        ),
        (
            "EMinusMinusBody",
            ExprInstance::EMinusMinusBody(EMinusMinus::default()),
        ),
        ("EModBody", ExprInstance::EModBody(EMod::default())),
        // ⚠ `GDouble`'s payload is the IEEE-754 BIT PATTERN as `u64`, not `f64`.
        // That is what keeps this variant reflexive: `f64::NAN != f64::NAN`, so
        // an `f64` payload would break `x == x` for a reason that has nothing to
        // do with the catch-all — and would break the derived `Eq` with it.
        ("GDouble", ExprInstance::GDouble(0x4009_21fb_5444_2d18)),
        ("GBigInt", ExprInstance::GBigInt(vec![9, 9])),
        ("GBigRat", ExprInstance::GBigRat(GBigRational::default())),
        (
            "GFixedPoint",
            ExprInstance::GFixedPoint(GFixedPoint::default()),
        ),
    ]
}

/// One value of every [`ConnectiveInstance`] variant.
fn connective_instance_values() -> Vec<(&'static str, ConnectiveInstance)> {
    vec![
        (
            "ConnAndBody",
            ConnectiveInstance::ConnAndBody(ConnectiveBody::default()),
        ),
        (
            "ConnOrBody",
            ConnectiveInstance::ConnOrBody(ConnectiveBody::default()),
        ),
        (
            "ConnNotBody",
            ConnectiveInstance::ConnNotBody(Par::default()),
        ),
        (
            "VarRefBody",
            ConnectiveInstance::VarRefBody(VarRef::default()),
        ),
        ("ConnBool", ConnectiveInstance::ConnBool(true)),
        ("ConnInt", ConnectiveInstance::ConnInt(true)),
        ("ConnString", ConnectiveInstance::ConnString(true)),
        ("ConnUri", ConnectiveInstance::ConnUri(true)),
        ("ConnByteArray", ConnectiveInstance::ConnByteArray(true)),
    ]
}

/// One value of every [`UnfInstance`] variant.
fn unf_instance_values() -> Vec<(&'static str, UnfInstance)> {
    vec![
        (
            "GPrivateBody",
            UnfInstance::GPrivateBody(GPrivate::default()),
        ),
        (
            "GDeployIdBody",
            UnfInstance::GDeployIdBody(GDeployId::default()),
        ),
        (
            "GDeployerIdBody",
            UnfInstance::GDeployerIdBody(GDeployerId::default()),
        ),
        (
            "GSysAuthTokenBody",
            UnfInstance::GSysAuthTokenBody(GSysAuthToken::default()),
        ),
    ]
}

/// Run the property over one enum's values, and run the CONTROL that stops the
/// property from being satisfiable by a comparator that says `true` to
/// everything.
///
/// The control is not incidental: it is the cross-variant residue arm itself.
/// Every ordered pair of *distinct* variants must compare unequal, which is
/// precisely what the arm that replaced `_ => false` claims.
fn check_enum<T>(enum_name: &str, table_names: Vec<&str>, values: Vec<(&'static str, T)>)
where T: PartialEq + Clone + Hash + std::fmt::Debug {
    assert_eq!(
        values.len(),
        table_names.len(),
        "★ `{enum_name}` has {} variants in the GENERATED `bincode_schema_tables` table but this test \
         constructs {}. The schema grew (or shrank) and the reflexivity check silently stopped \
         covering all of it — which is the exact failure mode this file exists to prevent.",
        table_names.len(),
        values.len()
    );
    let constructed: Vec<&str> = values.iter().map(|(n, _)| *n).collect();
    assert_eq!(
        constructed, table_names,
        "★ `{enum_name}`'s variant names no longer match the generated `bincode_schema_tables` table, so \
         this test is exercising a list that has drifted from the schema"
    );

    // (a) THE PROPERTY.
    for (name, v) in &values {
        reflexive_and_hash_agreeing(&format!("{enum_name}::{name}"), v);
    }

    // (b) THE CONTROL — must NOT discriminate the way (a) does. A comparator
    //     that returned `true` unconditionally would pass (a) and fail here.
    for (i, (ni, vi)) in values.iter().enumerate() {
        for (j, (nj, vj)) in values.iter().enumerate() {
            if i == j {
                continue;
            }
            assert_ne!(
                vi, vj,
                "★ `{enum_name}::{ni}` compares EQUAL to `{enum_name}::{nj}`. Distinct variants \
                 must be distinct, or the residue arm that replaced `_ => false` is not being \
                 reached."
            );
        }
    }
    println!(
        "{enum_name}: {} variants, each equal to itself and hash-agreeing; {} ordered \
         cross-variant pairs all unequal",
        values.len(),
        values.len() * (values.len() - 1)
    );
}

fn names_of(table: &[models::rust::rholang::bincode_schema::VariantProgram]) -> Vec<&str> {
    table.iter().map(|v| v.name).collect()
}

/// ★ Every variant of all five `oneof`s equals itself, agrees with `Hash`, and
/// is unequal to every other variant.
///
/// ⚠ This passes for the variants that exist today and would have passed before
/// the catch-alls were removed. It is the *contract* being exercised, not the
/// discriminator; the discriminator is `E0004`, which is a property of the code
/// shape and is transcribed in this file's module docs.
#[test]
fn every_oneof_variant_equals_itself() {
    check_enum(
        "TaggedCont",
        names_of(TAGGED_CONT_VARIANTS),
        tagged_cont_values(),
    );
    check_enum(
        "VarInstance",
        names_of(VAR_INSTANCE_VARIANTS),
        var_instance_values(),
    );
    check_enum(
        "ExprInstance",
        names_of(EXPR_INSTANCE_VARIANTS),
        expr_instance_values(),
    );
    check_enum(
        "ConnectiveInstance",
        names_of(CONNECTIVE_INSTANCE_VARIANTS),
        connective_instance_values(),
    );
    check_enum(
        "UnfInstance",
        names_of(UNF_INSTANCE_VARIANTS),
        unf_instance_values(),
    );
}

/// ★ The sixth instance: the sorter's score [`Tree`], whose `_ => return false`
/// was the same defect in a type the schema-driven audits structurally cannot
/// see (it is not a proto message).
#[test]
fn score_tree_values_equal_themselves() {
    let leaf: Tree<i32> = Tree::Leaf(4);
    let node: Tree<i32> = Tree::Node(vec![Tree::Leaf(4), Tree::Node(vec![Tree::Leaf(5)])]);

    // (a) THE PROPERTY. `Tree` has no `Hash` impl, so there is no `Eq`/`Hash`
    //     agreement cell here; reflexivity is the whole claim.
    assert!(
        compares_equal(&leaf, &leaf),
        "★ `Tree::Leaf` does not equal itself"
    );
    assert!(
        compares_equal(&node, &node),
        "★ `Tree::Node` does not equal itself"
    );
    assert_eq!(leaf, leaf.clone(), "★ `Tree::Leaf` != its own clone");
    assert_eq!(node, node.clone(), "★ `Tree::Node` != its own clone");

    // (b) THE CONTROL — the enumerated cross-variant arms must be reached.
    assert_ne!(
        leaf, node,
        "★ a `Leaf` compares equal to a `Node`; the cross-variant arms that replaced \
         `_ => return false` are not being reached"
    );
    assert_ne!(
        node, leaf,
        "★ a `Node` compares equal to a `Leaf` (the other order)"
    );
    // …and a same-variant inequality, so "everything is unequal" also fails.
    assert_ne!(
        leaf,
        Tree::Leaf(5),
        "★ two different leaves compare equal; equality is not reading the payload"
    );
    println!("Tree: 2 variants reflexive, both cross-variant orders unequal");
}

// ===========================================================================
// §2  The source scanners
// ===========================================================================

/// The five comparison traits whose impls must not disable exhaustiveness.
///
/// `Ord`/`PartialOrd`/`Eq` carry none today; they are scanned all the same,
/// because "there were none" is a fact worth re-establishing on every run
/// rather than a memory of a survey done once.
const COMPARISON_TRAITS: &[&str] = &["PartialEq", "Eq", "Hash", "Ord", "PartialOrd"];

/// A catch-all arm that has been examined and is **not** this defect.
///
/// Kept as data, with a reason per entry, so a second one cannot be inherited
/// without argument — and [`no_comparison_impl_carries_an_irrefutable_arm`]
/// fails when an entry stops applying, so the list cannot rot into a licence.
struct Allowance {
    /// Repository-relative path.
    path: &'static str,
    /// The type the impl is for, as it appears after `for`.
    type_name: &'static str,
    /// The arm's body, verbatim, so widening it is a visible edit.
    arm_body: &'static str,
    reason: &'static str,
}

const ALLOWED_CATCH_ALLS: &[Allowance] = &[Allowance {
    path: "casper/src/rust/util/comm/casper_packet_handler.rs",
    type_name: "DispatcherMessage",
    arm_body: "self.message == other.message",
    reason: "NOT the defect: this catch-all DELEGATES, it does not answer `false`. The impl \
             special-cases `BlockHashMessage` (compare block hashes only) and hands every other \
             shape to `CasperMessage`'s own derived `PartialEq`. A new `CasperMessage` variant \
             therefore keeps `x == x` true by construction — the fallback is the CORRECT default, \
             where `false` is the wrong one. `DispatcherMessage` also has no `Hash` impl, so \
             there is no `Eq`/`Hash` pair to keep in step. Enumerating here would buy nothing and \
             would couple the dispatcher to the message schema.",
}];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the `models` crate always has a parent directory")
        .to_path_buf()
}

/// Replace comments and string/char literals with spaces, preserving newlines,
/// so brace/paren counting and pattern extraction cannot be fooled by a `=>`
/// inside a doc comment or a `format!`.
///
/// ⚠ A backslash-newline inside a string literal is a LINE CONTINUATION and its
/// newline must survive, or every line number after it shifts. The same hazard
/// is documented at `rholang/tests/normalize_oracle_provenance.rs::blank_rust`,
/// where it was a real bug.
fn blank_rust(src: &str) -> String {
    let c: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0usize;
    let push = |out: &mut String, ch: char| out.push(if ch == '\n' { '\n' } else { ' ' });
    while i < c.len() {
        match c[i] {
            '/' if c.get(i + 1) == Some(&'/') => {
                while i < c.len() && c[i] != '\n' {
                    out.push(' ');
                    i += 1;
                }
            }
            '/' if c.get(i + 1) == Some(&'*') => {
                let mut depth = 0usize;
                while i < c.len() {
                    if c[i] == '/' && c.get(i + 1) == Some(&'*') {
                        depth += 1;
                        out.push_str("  ");
                        i += 2;
                    } else if c[i] == '*' && c.get(i + 1) == Some(&'/') {
                        depth -= 1;
                        out.push_str("  ");
                        i += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        push(&mut out, c[i]);
                        i += 1;
                    }
                }
            }
            // Raw strings, whose contents may hold anything at all.
            'r' if matches!(c.get(i + 1), Some(&'#') | Some(&'"')) => {
                let mut j = i + 1;
                let mut hashes = 0usize;
                while c.get(j) == Some(&'#') {
                    hashes += 1;
                    j += 1;
                }
                if c.get(j) != Some(&'"') {
                    out.push(c[i]);
                    i += 1;
                    continue;
                }
                let terminator: Vec<char> = std::iter::once('"')
                    .chain(std::iter::repeat_n('#', hashes))
                    .collect();
                for _ in i..=j {
                    out.push(' ');
                }
                i = j + 1;
                while i < c.len() {
                    if c[i..].starts_with(terminator.as_slice()) {
                        for _ in 0..terminator.len() {
                            out.push(' ');
                        }
                        i += terminator.len();
                        break;
                    }
                    push(&mut out, c[i]);
                    i += 1;
                }
            }
            '"' => {
                out.push(' ');
                i += 1;
                while i < c.len() {
                    if c[i] == '\\' {
                        out.push(' ');
                        push(&mut out, *c.get(i + 1).unwrap_or(&' '));
                        i += 2;
                    } else if c[i] == '"' {
                        out.push(' ');
                        i += 1;
                        break;
                    } else {
                        push(&mut out, c[i]);
                        i += 1;
                    }
                }
            }
            // A char literal, including `'\n'`; lifetimes have no closing quote
            // and fall through to the default arm.
            '\'' if c.get(i + 2) == Some(&'\'') => {
                out.push_str("   ");
                i += 3;
            }
            '\'' if c.get(i + 1) == Some(&'\\') => {
                let end = (i + 2..c.len().min(i + 8))
                    .find(|&k| c[k] == '\'')
                    .unwrap_or(i);
                if end > i {
                    for _ in i..=end {
                        out.push(' ');
                    }
                    i = end + 1;
                } else {
                    out.push(c[i]);
                    i += 1;
                }
            }
            ch => {
                out.push(ch);
                i += 1;
            }
        }
    }
    out
}

/// One `impl <Trait> for <Type> { … }` block found in a file.
struct ImplBlock {
    trait_name: String,
    type_name: String,
    /// Byte offset of the `impl` keyword, for line reporting.
    start: usize,
    /// Body, blanked, `{` exclusive to `}` exclusive.
    body: String,
    /// Offset of the body's first byte within the blanked file.
    body_start: usize,
}

/// Every `impl <Trait> for <Type>` block in `blanked`, with its body.
///
/// Header shape recognised: `impl` `[<generics>]` `Trait[<…>]` `for` `Type…` `{`.
/// Anything without a `for` is an inherent impl and is skipped.
fn impl_blocks(blanked: &str) -> Vec<ImplBlock> {
    let bytes = blanked.as_bytes();
    let mut out = Vec::new();
    let mut search = 0usize;
    while let Some(rel) = blanked[search..].find("impl") {
        let at = search + rel;
        search = at + 4;
        // `impl` must be a whole word.
        let before_ok = at == 0 || !is_ident_byte(bytes[at - 1]);
        let after_ok = bytes
            .get(at + 4)
            .is_none_or(|b| !is_ident_byte(*b) && *b != b'!');
        if !before_ok || !after_ok {
            continue;
        }
        // The header runs to the first `{` that is not inside `<…>` or `(…)`.
        let (mut angle, mut paren) = (0i32, 0i32);
        let mut brace = None;
        for (k, b) in bytes.iter().enumerate().skip(at + 4) {
            match b {
                b'<' => angle += 1,
                b'>' => angle -= 1,
                b'(' => paren += 1,
                b')' => paren -= 1,
                b';' if angle == 0 && paren == 0 => break,
                b'{' if angle == 0 && paren == 0 => {
                    brace = Some(k);
                    break;
                }
                _ => {}
            }
        }
        let Some(open) = brace else { continue };
        let header = &blanked[at + 4..open];
        let Some((trait_part, type_part)) = split_on_word(header, "for") else {
            continue;
        };
        // Drop a leading `<…>` generics list, then take the last path segment of
        // the trait, so `std::hash::Hash` and `Hash` are the same trait.
        let trait_name = trait_part
            .trim()
            .trim_start_matches(|_| false)
            .rsplit("::")
            .next()
            .unwrap_or("")
            .split('<')
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        let type_name = type_part
            .split_whitespace()
            .next()
            .unwrap_or("")
            .split('<')
            .next()
            .unwrap_or("")
            .rsplit("::")
            .next()
            .unwrap_or("")
            .to_string();
        // Brace-balance for the body.
        let mut depth = 0i32;
        let mut close = None;
        for (k, b) in bytes.iter().enumerate().skip(open) {
            match b {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(k);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(close) = close else { continue };
        out.push(ImplBlock {
            trait_name,
            type_name,
            start: at,
            body: blanked[open + 1..close].to_string(),
            body_start: open + 1,
        });
        search = close;
    }
    out
}

fn is_ident_byte(b: u8) -> bool { b.is_ascii_alphanumeric() || b == b'_' }

/// Split `s` at the first whole-word occurrence of `word`.
fn split_on_word<'a>(s: &'a str, word: &str) -> Option<(&'a str, &'a str)> {
    let b = s.as_bytes();
    let mut from = 0usize;
    while let Some(rel) = s[from..].find(word) {
        let at = from + rel;
        let before = at == 0 || !is_ident_byte(b[at - 1]);
        let after = b.get(at + word.len()).is_none_or(|c| !is_ident_byte(*c));
        if before && after {
            return Some((&s[..at], &s[at + word.len()..]));
        }
        from = at + word.len();
    }
    None
}

/// One match arm: its pattern text and whether it carries an `if` guard.
struct Arm {
    pattern: String,
    guarded: bool,
    /// Byte offset of the `=>`, within whatever text was scanned.
    at: usize,
}

/// Every match arm in `body`, recovered by a FORWARD scan.
///
/// ⚠ Forward, and that direction is the whole design. Scanning **backwards**
/// from each `=>` is the obvious implementation and it is wrong: an arm body may
/// be a block, so the walk meets the previous arm's closing `}` before it meets
/// any boundary, swallows that arm whole, and reports a pattern naming four
/// variants where there are two. That was a real bug in the first draft here,
/// and it silently emptied the same-variant arm table for
/// `TaggedCont::ScalaBodyRef` — a gate reading `0 arms` for an arm that is right
/// there in the file.
///
/// Forward, the boundary is unambiguous. A pattern begins after the last `{`,
/// `}`, `;`, or a `,` that is **not** inside `(…)` or `[…]` — commas inside a
/// tuple or slice pattern belong to the pattern — and ends at the `=>`. The arm
/// body that follows moves the boundary onward through its own delimiters, so
/// block-bodied and expression-bodied arms need no special case.
fn arms(body: &str) -> Vec<Arm> {
    let c: Vec<char> = body.chars().collect();
    let mut out = Vec::new();
    // The bracket stack, so a `,` can be classified as "in a pattern" or "an
    // arm separator".
    let mut stack: Vec<char> = Vec::with_capacity(32);
    let mut boundary = 0usize;
    let mut i = 0usize;
    while i < c.len() {
        match c[i] {
            '(' | '[' => stack.push(c[i]),
            '{' if matches!(stack.last(), Some('(') | Some('[') | Some('s')) => {
                stack.push('s');
            }
            '{' => {
                stack.push('{');
                boundary = i + 1;
            }
            ')' | ']' => {
                stack.pop();
            }
            '}' => {
                if stack.pop() != Some('s') {
                    boundary = i + 1;
                }
            }
            ';' => boundary = i + 1,
            ',' if !matches!(stack.last(), Some('(') | Some('[') | Some('s')) => boundary = i + 1,
            // `=>`, but not the tail of `>=`, `<=`, `==`, `!=`.
            '=' if c.get(i + 1) == Some(&'>')
                && !matches!(c.get(i.wrapping_sub(1)), Some('=' | '!' | '<' | '>')) =>
            {
                let raw: String = c[boundary..i].iter().collect();
                let (pattern, guarded) = match split_on_word(&raw, "if") {
                    Some((p, _)) => (p.trim().to_string(), true),
                    None => (raw.trim().to_string(), false),
                };
                out.push(Arm {
                    pattern,
                    guarded,
                    at: i,
                });
                boundary = i + 2;
                i += 2;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// `true` when a pattern is a bare wildcard shape — `_`, `(_, _)`,
/// `(_, _) | (_, _)` and friends — i.e. it matches regardless of which variants
/// exist and therefore makes the match exhaustive by fiat.
///
/// A pattern naming any identifier is not this: `(Self::A(_), _)` sits below
/// `(Self::A(a), Self::A(b))` and says "self is `A`, other is not", which stays
/// true however many variants there are and leaves `(Z(_), _)` uncovered.
fn is_irrefutable_wildcard(pattern: &str) -> bool {
    !pattern.is_empty()
        && pattern.contains('_')
        && pattern
            .chars()
            .all(|ch| matches!(ch, '_' | '(' | ')' | ',' | '|' | ' ' | '\n' | '\t' | '\r'))
}

fn line_of(text: &str, offset: usize) -> usize { text[..offset].matches('\n').count() + 1 }

/// Every `.rs` file under a `src/` directory anywhere in the workspace.
///
/// `target/`, the wipeable scratch areas and `demos/` are excluded: generated
/// and throwaway code is not what this gate is about, and including it would
/// make the gate's own result depend on which build artifacts happen to exist.
fn workspace_sources() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if p.is_dir() {
                if matches!(
                    name.as_str(),
                    ".git" | "target" | "node_modules" | "scratch" | "scratchpad" | "demos"
                ) {
                    continue;
                }
                walk(&p, out);
            } else if name.ends_with(".rs") && p.components().any(|c| c.as_os_str() == "src") {
                out.push(p);
            }
        }
    }
    let mut out = Vec::with_capacity(1024);
    walk(&repo_root(), &mut out);
    out.sort();
    out
}

/// A catch-all found by the scanner, in reportable form.
#[derive(Debug, PartialEq, Eq)]
struct Finding {
    path: String,
    trait_name: String,
    type_name: String,
    line: usize,
    pattern: String,
}

/// Scan one file's text for unguarded irrefutable arms inside comparison-trait
/// impls.
fn scan_text(rel_path: &str, src: &str) -> Vec<Finding> {
    let blanked = blank_rust(src);
    let mut out = Vec::new();
    for b in impl_blocks(&blanked) {
        if !COMPARISON_TRAITS.contains(&b.trait_name.as_str()) {
            continue;
        }
        for arm in arms(&b.body) {
            if arm.guarded || !is_irrefutable_wildcard(&arm.pattern) {
                continue;
            }
            out.push(Finding {
                path: rel_path.to_string(),
                trait_name: b.trait_name.clone(),
                type_name: b.type_name.clone(),
                line: line_of(&blanked, b.body_start + arm.at),
                pattern: arm.pattern.trim().to_string(),
            });
        }
        let _ = b.start;
    }
    out
}

/// ★ No `PartialEq`/`Eq`/`Hash`/`Ord`/`PartialOrd` impl in any `src/` tree may
/// carry an unguarded catch-all arm, except the classified entries in
/// [`ALLOWED_CATCH_ALLS`].
#[test]
fn no_comparison_impl_carries_an_irrefutable_arm() {
    let root = repo_root();
    let files = workspace_sources();
    assert!(
        files.len() > 500,
        "★ the scanner found only {} source files; it is not looking at the workspace and a \
         clean result would mean nothing",
        files.len()
    );

    let mut findings = Vec::new();
    let mut impls_scanned = 0usize;
    for path in &files {
        let Ok(src) = std::fs::read_to_string(path) else {
            continue;
        };
        if !src.contains("impl") {
            continue;
        }
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();
        impls_scanned += impl_blocks(&blank_rust(&src))
            .iter()
            .filter(|b| COMPARISON_TRAITS.contains(&b.trait_name.as_str()))
            .count();
        findings.extend(scan_text(&rel, &src));
    }
    assert!(
        impls_scanned > 100,
        "★ only {impls_scanned} comparison-trait impls were found across the workspace; \
         `models/src/lib.rs` alone hand-writes 124, so the impl parser has stopped matching"
    );

    // Split into allowed and not, and check the allowances are still needed.
    let mut unexplained = Vec::new();
    let mut matched = vec![false; ALLOWED_CATCH_ALLS.len()];
    for f in &findings {
        match ALLOWED_CATCH_ALLS
            .iter()
            .position(|a| a.path == f.path && a.type_name == f.type_name)
        {
            Some(idx) => matched[idx] = true,
            None => unexplained.push(f),
        }
    }

    assert!(
        unexplained.is_empty(),
        "\n★ A CATCH-ALL ARM HAS RETURNED TO A COMPARISON IMPL.\n\
         \n\
         `_ => …` makes the match exhaustive TO THE COMPILER, which disables the only check that \
         a new variant gets an answer. A variant added later falls into it, compares UNEQUAL TO \
         ITSELF, and every `HashSet`, `HashMap`, `dedup` and `sort` holding one is silently \
         wrong — with `Eq` derived over it asserting the opposite.\n\
         \n\
         Enumerate the residue instead: `(Self::A(_), _) | (Self::B(_), _) | … => false`. If the \
         arm is genuinely not this defect (it DELEGATES rather than answering `false`, say), add \
         it to ALLOWED_CATCH_ALLS with a reason.\n\
         \n\
         {} unexplained:\n{}\n",
        unexplained.len(),
        unexplained
            .iter()
            .map(|f| format!(
                "  {}:{}  impl {} for {}   pattern: `{}`",
                f.path, f.line, f.trait_name, f.type_name, f.pattern
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );

    for (i, a) in ALLOWED_CATCH_ALLS.iter().enumerate() {
        assert!(
            matched[i],
            "★ ALLOWED_CATCH_ALLS entry for `{}` in `{}` is STALE — the scanner no longer finds a \
             catch-all there. Either it was enumerated (delete the entry) or the scanner stopped \
             seeing it. An exemption outliving its reason quietly widens what this gate \
             tolerates.\nReason on file: {}",
            a.type_name, a.path, a.reason
        );
        let text = std::fs::read_to_string(root.join(a.path))
            .unwrap_or_else(|e| panic!("cannot read allowlisted file {}: {e}", a.path));
        assert!(
            text.contains(a.arm_body),
            "★ ALLOWED_CATCH_ALLS records the arm at `{}` as `{}`, which the file no longer \
             contains. The exemption was written for a DELEGATING arm; if the arm now answers \
             something else the reason no longer covers it.\nReason on file: {}",
            a.type_name,
            a.arm_body,
            a.reason
        );
        assert!(
            !a.reason.trim().is_empty(),
            "an exemption without a reason is an undeclared exemption with extra steps"
        );
    }

    println!(
        "{} comparison-trait impls across {} source files: {} catch-all(s), all {} classified",
        impls_scanned,
        files.len(),
        findings.len(),
        ALLOWED_CATCH_ALLS.len()
    );
}

/// ★ **The scanner must be able to go red**, and must not be red about
/// everything.
#[test]
fn the_catch_all_scanner_can_go_red() {
    let root = repo_root();
    let target = "models/src/lib.rs";
    let clean = std::fs::read_to_string(root.join(target)).expect("models/src/lib.rs is readable");

    // (a) THE CONTROL — unperturbed, the file is clean. A scanner that flagged
    //     everything would fail here rather than look strict.
    assert_eq!(
        scan_text(target, &clean),
        vec![],
        "★ the control arm does not hold: `models/src/lib.rs` is already flagged, so the red \
         below would prove nothing"
    );

    // (b) THE MUTATION — restore ONE catch-all, exactly as it stood.
    let anchor = "            // The residue: self is one variant and other is a different one.\n            \
                  (VarInstance::BoundVar(_), _)\n            \
                  | (VarInstance::FreeVar(_), _)\n            \
                  | (VarInstance::Wildcard(_), _) => false,";
    assert!(
        clean.contains(anchor),
        "★ the mutation's anchor is gone from `models/src/lib.rs`, so this check is no longer \
         mutating what it claims to mutate"
    );
    let mutated = clean.replace(anchor, "            _ => false,");
    assert_ne!(mutated, clean, "the mutation must actually change the text");

    let found = scan_text(target, &mutated);
    assert_eq!(
        found.len(),
        1,
        "★ THE SCANNER CANNOT GO RED. `models/src/lib.rs` was mutated to restore the \
         `VarInstance` catch-all and the scanner reported a different number of findings than \
         the 1 expected, so `no_comparison_impl_carries_an_irrefutable_arm` is certifying \
         nothing.\nfound: {found:?}"
    );
    assert_eq!(found[0].type_name, "VarInstance");
    assert_eq!(found[0].trait_name, "PartialEq");
    assert_eq!(found[0].pattern, "_");

    // (c) A SECOND MUTATION SHAPE — `(_, _)` is a catch-all too, and a scanner
    //     that only knew the literal string `_ =>` would miss it.
    let mutated2 = clean.replace(anchor, "            (_, _) => false,");
    let found2 = scan_text(target, &mutated2);
    assert_eq!(
        found2.len(),
        1,
        "★ a `(_, _)` catch-all was not detected; the scanner is matching a literal rather than \
         the property (a pattern that matches regardless of which variants exist)"
    );
    assert_eq!(found2[0].pattern, "(_, _)");

    // (d) A GUARDED catch-all must NOT be flagged: rustc ignores guarded arms
    //     when computing exhaustiveness, so such an arm cannot hide a variant.
    let guarded = clean.replace(anchor, "            _ if false => false,");
    assert_eq!(
        scan_text(target, &guarded),
        vec![],
        "★ a GUARDED catch-all was flagged. Guarded arms do not count toward exhaustiveness, so \
         flagging them is a false positive — and false positives are how a gate gets disabled."
    );

    let exhaustive_struct_residue = r#"
        impl PartialEq for Example {
            fn eq(&self, other: &Self) -> bool {
                match (self, other) {
                    (Self::Named { value: left }, Self::Named { value: right }) => left == right,
                    (Self::Unit, Self::Unit) => true,
                    (Self::Named { .. }, _) | (Self::Unit, _) => false,
                }
            }
        }
    "#;
    assert_eq!(
        scan_text("synthetic.rs", exhaustive_struct_residue),
        vec![],
        "★ an exhaustive residue containing a named-field variant was reduced to its trailing \
         wildcard. That false positive would force comparison PDAs back to catch-all arms."
    );
}

// ===========================================================================
// §3  The gap exhaustiveness leaves: answering E0004 the wrong way
// ===========================================================================

/// The five `oneof` types, their `bincode_schema_tables` variant tables, and how the impl
/// header spells the type in `models/src/lib.rs`.
fn oneof_registry() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        ("TaggedCont", names_of(TAGGED_CONT_VARIANTS)),
        ("VarInstance", names_of(VAR_INSTANCE_VARIANTS)),
        ("ExprInstance", names_of(EXPR_INSTANCE_VARIANTS)),
        ("ConnectiveInstance", names_of(CONNECTIVE_INSTANCE_VARIANTS)),
        ("UnfInstance", names_of(UNF_INSTANCE_VARIANTS)),
    ]
}

/// The variant names an arm pattern mentions, in order: every `::Name(`.
fn variants_named(pattern: &str) -> Vec<String> {
    let b = pattern.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while let Some(rel) = pattern[i..].find("::") {
        let at = i + rel + 2;
        let mut j = at;
        while j < b.len() && is_ident_byte(b[j]) {
            j += 1;
        }
        if j > at && b.get(j) == Some(&b'(') {
            out.push(pattern[at..j].to_string());
        }
        i = at.max(i + 1);
    }
    out
}

/// Given `models/src/lib.rs`, the same-variant `eq` arms and the `hash` arms per
/// oneof type.
fn eq_and_hash_arms(src: &str, type_name: &str) -> (Vec<String>, Vec<String>) {
    let blanked = blank_rust(src);
    let (mut eq_arms, mut hash_arms) = (Vec::new(), Vec::new());
    for b in impl_blocks(&blanked) {
        if b.type_name != type_name {
            continue;
        }
        for arm in arms(&b.body) {
            let named = variants_named(&arm.pattern);
            match b.trait_name.as_str() {
                // A same-variant arm names ONE variant, TWICE — `(T::V(a), T::V(b))`.
                "PartialEq" if named.len() == 2 && named[0] == named[1] => {
                    eq_arms.push(named[0].clone())
                }
                // A hash arm names exactly one variant — `T::V(a) => …`.
                "Hash" if named.len() == 1 => hash_arms.push(named[0].clone()),
                _ => {}
            }
        }
    }
    (eq_arms, hash_arms)
}

/// ★ Every variant in the GENERATED schema tables owns a same-variant `eq` arm
/// and a `hash` arm.
///
/// This is the check the compiler cannot make. `E0004` forces a maintainer to
/// touch the impl when a variant appears; it does not force them to touch it
/// *correctly*. Extending the residue list alone —
/// `| (ExprInstance::GBlake2b256Hash(_), _)` — silences `E0004` and leaves the
/// new variant unequal to itself, which is the original defect restored by the
/// very edit that was supposed to repair it. Here it is red instead.
#[test]
fn every_oneof_variant_has_a_same_variant_eq_arm_and_a_hash_arm() {
    let src = std::fs::read_to_string(repo_root().join("models/src/lib.rs"))
        .expect("models/src/lib.rs is readable");
    let mut problems = Vec::new();
    let mut checked = 0usize;
    for (ty, variants) in oneof_registry() {
        let (eq_arms, hash_arms) = eq_and_hash_arms(&src, ty);
        for v in &variants {
            checked += 1;
            let eq_count = eq_arms.iter().filter(|a| a.as_str() == *v).count();
            let hash_count = hash_arms.iter().filter(|a| a.as_str() == *v).count();
            if eq_count != 1 {
                problems.push(format!(
                    "  `{ty}::{v}` has {eq_count} same-variant `eq` arm(s); it must have exactly \
                     one, `({ty}::{v}(a), {ty}::{v}(b)) => a == b`"
                ));
            }
            if hash_count != 1 {
                problems.push(format!(
                    "  `{ty}::{v}` has {hash_count} `hash` arm(s); it must have exactly one, or \
                     `Eq` and `Hash` cannot agree"
                ));
            }
        }
    }
    assert!(
        problems.is_empty(),
        "\n★ A SCHEMA VARIANT HAS NO EQUALITY OF ITS OWN.\n\
         \n\
         The variant lists come from the GENERATED `bincode_schema_tables::*_VARIANTS` tables, so they are \
         the schema itself and cannot go stale. A variant without a same-variant `eq` arm falls \
         through to the residue and compares UNEQUAL TO ITSELF — which is what happens when \
         `E0004` is answered by extending the residue list alone.\n\
         \n{}\n",
        problems.join("\n")
    );
    assert!(
        checked >= 54,
        "★ only {checked} variants were checked; the five oneofs carried 54 when this gate was \
         written, so the registry has stopped seeing them"
    );
    println!("{checked} schema variants, each with its own `eq` arm and `hash` arm");
}

/// ★ **That gate must be able to go red**, and must not be red about
/// everything.
#[test]
fn the_missing_arm_gate_can_go_red() {
    let clean = std::fs::read_to_string(repo_root().join("models/src/lib.rs"))
        .expect("models/src/lib.rs is readable");

    // (a) THE CONTROL — unperturbed, every variant is accounted for.
    for (ty, variants) in oneof_registry() {
        let (eq_arms, hash_arms) = eq_and_hash_arms(&clean, ty);
        for v in &variants {
            assert_eq!(
                eq_arms.iter().filter(|a| a.as_str() == *v).count(),
                1,
                "control: `{ty}::{v}` should already have exactly one `eq` arm"
            );
            assert_eq!(
                hash_arms.iter().filter(|a| a.as_str() == *v).count(),
                1,
                "control: `{ty}::{v}` should already have exactly one `hash` arm"
            );
        }
    }

    // (b) THE MUTATION — the WRONG COMPLETION: the same-variant `eq` arm is
    //     deleted and the residue list keeps the variant, exactly as a hurried
    //     answer to `E0004` would leave it. The build would still be green.
    let arm = "            (UnfInstance::GSysAuthTokenBody(a), UnfInstance::GSysAuthTokenBody(b)) => a == b,\n";
    assert!(
        clean.contains(arm),
        "★ the mutation's anchor arm is gone, so this check no longer mutates what it claims to"
    );
    let mutated = clean.replace(arm, "");
    assert_ne!(mutated, clean, "the mutation must actually change the text");

    let (eq_arms, hash_arms) = eq_and_hash_arms(&mutated, "UnfInstance");
    assert_eq!(
        eq_arms
            .iter()
            .filter(|a| a.as_str() == "GSysAuthTokenBody")
            .count(),
        0,
        "★ THE MISSING-ARM GATE CANNOT GO RED: the `eq` arm was deleted and the gate still finds \
         it, so `every_oneof_variant_has_a_same_variant_eq_arm_and_a_hash_arm` is certifying \
         nothing."
    );
    // …and the CONTROL within the mutation: the other three variants, and the
    // `hash` arms, must be untouched. A parser that lost the whole impl would
    // also report zero and would pass the assertion above.
    assert_eq!(
        eq_arms.len(),
        3,
        "★ the mutation removed more than the one arm it names; the gate's red would not be \
         attributable"
    );
    assert_eq!(
        hash_arms.len(),
        4,
        "★ the `hash` arms moved under a mutation that only touched `eq`"
    );
}
