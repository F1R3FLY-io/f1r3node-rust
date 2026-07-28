//! # The SCHEMA CONFORMANCE PROBE — the generated table against serde itself
//!
//! The byte differential proves the emitter and the derived `Serialize` agree.
//! It does not say *where* they would disagree, and a mistake in the generator
//! surfaces as "byte 4711 differs" on whichever fixture happens to reach it
//! first. This file closes that gap by comparing the generated table against
//! **serde's own derived behaviour**, per type and per field.
//!
//! ## How
//!
//! A `Serializer` that records `serialize_struct`'s name and arity and each
//! `serialize_field`'s key — and **does not serialize the values**, so it is
//! non-recursive, allocation-light, and works on `Default::default()` for every
//! type in the schema. What it observes is exactly what bincode's positional
//! layout depends on: *which fields, in which order*.
//!
//! ## ⚠ Why this is not paranoia
//!
//! prost's field order is **not** the intuitive one. It emits every plain
//! field first, in declaration order, and only then every oneof field
//! (`prost-build-0.14.3/src/code_generator.rs:270-291` — two separate loops).
//! `TaggedContinuation` declares `oneof tagged_cont { … }` *before* `guard`, so
//! a generator that placed each oneof at its first member's position produced a
//! 95-byte encoding with its two halves exchanged: **same length, same byte
//! multiset, different order**. No length check, no round-trip and no value
//! comparison can see that. The first version of this generator had exactly
//! that bug; the write differential caught it, and this probe is what makes the
//! next one fail with the type and field name attached.
//!
//! ## Coverage
//!
//! The registry is **generated in the same pass as the table**, so a new
//! message joins this test automatically. A hand-written list would omit
//! precisely the type nobody remembered.

use models::rust::rholang::wire::FieldKind;
use models::rust::rholang::wire_schema::CONFORMANCE_REGISTRY;
use serde::{Serialize, Serializer};

// ===========================================================================
// §A  The recording serializer
// ===========================================================================

#[derive(Debug)]
struct Observation {
    struct_name: String,
    arity: usize,
    field_names: Vec<&'static str>,
}

#[derive(Debug)]
struct ProbeError(String);

impl std::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ProbeError {}
impl serde::ser::Error for ProbeError {
    fn custom<T: std::fmt::Display>(msg: T) -> Self {
        ProbeError(msg.to_string())
    }
}

/// Records the shape of one `serialize_struct` call and stops.
struct Probe;

/// Everything the probe does not need is a hard error rather than a silent
/// `Ok`: a type whose `Serialize` is not a struct would otherwise be recorded
/// as a zero-field struct and pass vacuously.
macro_rules! unsupported {
    ($($name:ident ( $($arg:ty),* ) -> $ret:ty ;)*) => {$(
        fn $name(self, $(_: $arg),*) -> Result<$ret, Self::Error> {
            Err(ProbeError(format!(
                "wire_schema_conformance: the probe reached `{}`, but every schema message \
                 must serialize as a STRUCT. A non-struct `Serialize` would be recorded as a \
                 zero-field struct and pass vacuously.",
                stringify!($name)
            )))
        }
    )*};
}

impl Serializer for Probe {
    type Ok = Observation;
    type Error = ProbeError;
    type SerializeSeq = serde::ser::Impossible<Observation, ProbeError>;
    type SerializeTuple = serde::ser::Impossible<Observation, ProbeError>;
    type SerializeTupleStruct = serde::ser::Impossible<Observation, ProbeError>;
    type SerializeTupleVariant = serde::ser::Impossible<Observation, ProbeError>;
    type SerializeMap = serde::ser::Impossible<Observation, ProbeError>;
    type SerializeStruct = StructProbe;
    type SerializeStructVariant = serde::ser::Impossible<Observation, ProbeError>;

    fn serialize_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Ok(StructProbe {
            struct_name: name.to_string(),
            arity: len,
            field_names: Vec::with_capacity(len),
        })
    }

    unsupported! {
        serialize_bool(bool) -> Observation;
        serialize_i8(i8) -> Observation;
        serialize_i16(i16) -> Observation;
        serialize_i32(i32) -> Observation;
        serialize_i64(i64) -> Observation;
        serialize_u8(u8) -> Observation;
        serialize_u16(u16) -> Observation;
        serialize_u32(u32) -> Observation;
        serialize_u64(u64) -> Observation;
        serialize_f32(f32) -> Observation;
        serialize_f64(f64) -> Observation;
        serialize_char(char) -> Observation;
        serialize_str(&str) -> Observation;
        serialize_bytes(&[u8]) -> Observation;
        serialize_unit() -> Observation;
        serialize_unit_struct(&'static str) -> Observation;
        serialize_none() -> Observation;
        serialize_seq(Option<usize>) -> Self::SerializeSeq;
        serialize_tuple(usize) -> Self::SerializeTuple;
        serialize_map(Option<usize>) -> Self::SerializeMap;
    }

    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        Err(ProbeError("unit variant at the top level".into()))
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: &T,
    ) -> Result<Self::Ok, Self::Error> {
        Err(ProbeError("newtype struct at the top level".into()))
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T,
    ) -> Result<Self::Ok, Self::Error> {
        Err(ProbeError("newtype variant at the top level".into()))
    }
    fn serialize_some<T: ?Sized + Serialize>(self, _: &T) -> Result<Self::Ok, Self::Error> {
        Err(ProbeError("Some(..) at the top level".into()))
    }
    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Err(ProbeError("tuple struct at the top level".into()))
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Err(ProbeError("tuple variant at the top level".into()))
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Err(ProbeError("struct variant at the top level".into()))
    }
}

struct StructProbe {
    struct_name: String,
    arity: usize,
    field_names: Vec<&'static str>,
}

impl serde::ser::SerializeStruct for StructProbe {
    type Ok = Observation;
    type Error = ProbeError;

    /// ★ The value is deliberately **not** serialized. That makes the probe
    /// non-recursive (so it terminates on `Par`, which is cyclic through
    /// `Expr`) and independent of the value's contents, so `Default::default()`
    /// suffices for every type in the schema.
    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        _value: &T,
    ) -> Result<(), Self::Error> {
        self.field_names.push(key);
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(Observation {
            struct_name: self.struct_name,
            arity: self.arity,
            field_names: self.field_names,
        })
    }
}

// ===========================================================================
// §B  The gate
// ===========================================================================

/// Compare one type's derived `Serialize` against its generated program.
///
/// A pure function of one observation and one table row, so
/// [`the_conformance_probe_can_go_red`] can hand it a deliberately wrong row.
fn conformance_verdict(
    expected_type: &str,
    kinds: &[FieldKind],
    names: &[&str],
    observed: &Observation,
) -> Result<(), String> {
    if observed.struct_name != expected_type {
        return Err(format!(
            "CONFORMANCE FAILED for `{expected_type}`: serde reports the struct name as \
             `{}`. The generator and prost disagree about this type's identity.",
            observed.struct_name
        ));
    }
    if observed.arity != kinds.len() || observed.field_names.len() != kinds.len() {
        return Err(format!(
            "CONFORMANCE FAILED for `{expected_type}`: serde emits {} fields (declared arity \
             {}) but the generated program has {}. A field was added, removed, or `#[serde(skip)]`ed \
             without the table following.",
            observed.field_names.len(),
            observed.arity,
            kinds.len()
        ));
    }
    for (i, (observed_name, table_name)) in observed.field_names.iter().zip(names).enumerate() {
        if observed_name != table_name {
            return Err(format!(
                "CONFORMANCE FAILED for `{expected_type}` at position {i}: serde emits \
                 `{observed_name}` where the generated program declares `{table_name}`.\n\
                 ⚠ bincode is POSITIONAL — field names never reach the wire — so an order \
                 difference between two same-typed fields is a byte-identical-length, \
                 byte-multiset-identical CONSENSUS FORK that no round-trip can see.\n\
                 serde order: {:?}\n\
                 table order: {:?}",
                observed.field_names, names
            ));
        }
    }
    Ok(())
}

/// Every generated type, checked against serde's own derive.
///
/// The dispatch is a macro over the concrete types because a `Serialize` call
/// needs a value, and the registry carries only names. The macro's list is
/// checked against the generated registry below, so it cannot silently fall
/// behind.
macro_rules! probe_all {
    ($($ty:ty),* $(,)?) => {{
        let mut observed: Vec<(String, Observation)> = Vec::new();
        $(
            let value = <$ty>::default();
            let obs = value.serialize(Probe).unwrap_or_else(|e| panic!(
                "wire_schema_conformance: {} did not serialize as a struct: {e}",
                stringify!($ty)
            ));
            observed.push((obs.struct_name.clone(), obs));
        )*
        observed
    }};
}

#[test]
fn every_generated_type_matches_serdes_own_field_order() {
    use models::rhoapi::*;

    let observed = probe_all!(
        Par,
        TaggedContinuation,
        ParWithRandom,
        PCost,
        ListParWithRandom,
        Var,
        var::WildcardMsg,
        Bundle,
        Send,
        ReceiveBind,
        BindPattern,
        ListBindPatterns,
        Receive,
        New,
        MatchCase,
        Match,
        If,
        Expr,
        GBigRational,
        GFixedPoint,
        EList,
        ETuple,
        ESet,
        EMap,
        EZipper,
        EMethod,
        KeyValuePair,
        EVar,
        ENot,
        ENeg,
        EMult,
        EDiv,
        EMod,
        EPlus,
        EMinus,
        ELt,
        ELte,
        EGt,
        EGte,
        EEq,
        ENeq,
        EAnd,
        EOr,
        EMatches,
        EPercentPercent,
        EPlusPlus,
        EMinusMinus,
        Connective,
        VarRef,
        ConnectiveBody,
        DeployId,
        DeployerId,
        GUnforgeable,
        GPrivate,
        GDeployId,
        GDeployerId,
        GSysAuthToken,
    );

    // ★ Coverage against the GENERATED registry, not against this macro's list.
    // If the schema gains a message, the registry grows and this assertion
    // fails until the probe is extended — the omission cannot be silent.
    let probed: std::collections::BTreeSet<&str> =
        observed.iter().map(|(n, _)| n.as_str()).collect();
    let registered: std::collections::BTreeSet<&str> =
        CONFORMANCE_REGISTRY.iter().map(|(n, _, _)| *n).collect();
    assert_eq!(
        probed, registered,
        "the probe must cover EVERY generated type. Missing from the probe: {:?}; probed but \
         not generated: {:?}",
        registered.difference(&probed).collect::<Vec<_>>(),
        probed.difference(&registered).collect::<Vec<_>>()
    );
    assert!(
        registered.len() >= 55,
        "the generated registry collapsed to {} types — a conformance run over an empty \
         registry reports a comfortable pass",
        registered.len()
    );

    for (name, obs) in &observed {
        let (_, kinds, names) = CONFORMANCE_REGISTRY
            .iter()
            .find(|(t, _, _)| t == name)
            .unwrap_or_else(|| panic!("`{name}` is probed but not in the generated registry"));
        if let Err(why) = conformance_verdict(name, kinds, names, obs) {
            panic!("{why}");
        }
    }
}

/// ★★ ANTI-VACUITY. The verdict must REJECT a wrong table row, and must still
/// ACCEPT the right one in the same run.
#[test]
fn the_conformance_probe_can_go_red() {
    use models::rhoapi::TaggedContinuation;

    let observed = TaggedContinuation::default()
        .serialize(Probe)
        .expect("TaggedContinuation serializes as a struct");
    let (_, kinds, names) = CONFORMANCE_REGISTRY
        .iter()
        .find(|(t, _, _)| *t == "TaggedContinuation")
        .expect("TaggedContinuation is in the registry");

    // CONTROL — the real row.
    assert!(
        conformance_verdict("TaggedContinuation", kinds, names, &observed).is_ok(),
        "the CONTROL must pass, or every rejection below is a statement about the judge"
    );

    // MUTATION 1 — the exact historical bug: the two fields transposed.
    assert_eq!(
        names,
        &["guard", "tagged_cont"],
        "the fixture must be the type whose order actually surprised the generator"
    );
    let transposed = ["tagged_cont", "guard"];
    let why = conformance_verdict("TaggedContinuation", kinds, &transposed, &observed)
        .expect_err("a TRANSPOSED field order must be REJECTED");
    assert!(
        why.contains("at position 0"),
        "the transposition must be rejected at the first differing position, naming both \
         orders; got: {why}"
    );

    // MUTATION 2 — a dropped field.
    let short = ["guard"];
    let why = conformance_verdict("TaggedContinuation", &kinds[..1], &short, &observed)
        .expect_err("a SHORTENED program must be REJECTED");
    assert!(
        why.contains("fields"),
        "the arity mismatch must be rejected by the arity clause; got: {why}"
    );

    // MUTATION 3 — the wrong type name.
    let why = conformance_verdict("Send", kinds, names, &observed)
        .expect_err("a MISMATCHED type name must be REJECTED");
    assert!(why.contains("struct name"), "got: {why}");

    // And the control again, so a verdict that latched into rejecting fails.
    assert!(
        conformance_verdict("TaggedContinuation", kinds, names, &observed).is_ok(),
        "the verdict must still ACCEPT the truth after rejecting all three mutations"
    );
}

/// The registry rows must be internally consistent: one name per kind.
#[test]
fn the_registry_rows_are_well_formed() {
    for (ty, kinds, names) in CONFORMANCE_REGISTRY {
        assert_eq!(
            kinds.len(),
            names.len(),
            "`{ty}`: the generated program has {} kinds and {} names",
            kinds.len(),
            names.len()
        );
        for name in *names {
            assert!(!name.is_empty(), "`{ty}` has an empty field name");
            assert!(
                !name.starts_with("r#"),
                "`{ty}`: field `{name}` still carries prost's keyword escape; serde emits the \
                 bare identifier, so the probe would never match"
            );
        }
    }
}
