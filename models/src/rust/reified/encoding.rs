//! The marker-tuple wire encoding of the Reified RSpaces syntax (plan decision 21).
//!
//! | Source                          | Encoding                                          |
//! |---------------------------------|---------------------------------------------------|
//! | `use s { P }`                   | `(USE, s, P)` in `Par.exprs`                      |
//! | `new x : s, y in P`             | `(NEWSPACE, [[s], []], Par{news: [New]})`         |
//! | `x!(d…; p…, k=v…)`              | trailing data element `(PARAMS, [p…], {k: v…})`  |
//! | `for (y <- ch ~ (q…; k=v…)) P`  | bind source `(QUERY, ch, [q…], {k: v…})`          |
//! | `free Th(a…)`                   | value `(FREE, "Th", [a…])`                        |
//!
//! A decoder returns `Ok(None)` for a term without the marker of its position, and `Err` for
//! a term with that marker that is not a canonical encoding. Each decoder ends with the
//! canonicality check `encode(decoded) == input` under `Par` equality, which ignores
//! `locally_free`. The marker heads of a term never depend on its `connective_used` flags;
//! the canonicality check compares those flags.

use std::collections::BTreeMap;
use std::fmt;

use super::validation::{is_param_key, is_theory_name, IdentifierPosition};
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::g_unforgeable::UnfInstance;
use crate::rhoapi::{EList, EMap, ETuple, Expr, GPrivate, GUnforgeable, KeyValuePair, New, Par};
use crate::rust::par_map::ParMap;
use crate::rust::par_map_type_mapper::ParMapTypeMapper;
use crate::rust::utils::union;

/// The namespace tag of the Reified RSpaces names (plan decision 7).
pub const REIFIED_TAG: u8 = 0xE0;

/// The head of a marker tuple: the name `GPrivate([REIFIED_TAG, m])`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum Marker {
    /// `use s { P }`.
    Use = 0xD0,
    /// `new` with at least one space annotation.
    NewSpace = 0xD1,
    /// The params of a send.
    Params = 0xD2,
    /// The query of a receive bind.
    Query = 0xD3,
    /// `free Th(a…)`.
    Free = 0xD4,
}

impl Marker {
    /// Every marker, in id order.
    pub const ALL: [Marker; 5] = [
        Marker::Use,
        Marker::NewSpace,
        Marker::Params,
        Marker::Query,
        Marker::Free,
    ];

    /// The 2-byte id `[0xE0, m]` of the marker name.
    pub const fn id(self) -> [u8; 2] { [REIFIED_TAG, self as u8] }

    /// The number of tuple elements, the head included.
    pub const fn tuple_arity(self) -> usize {
        match self {
            Marker::Use | Marker::NewSpace | Marker::Params | Marker::Free => 3,
            Marker::Query => 4,
        }
    }

    /// The marker name: a `Par` with exactly one `GPrivate` that has the marker id.
    pub fn par(self) -> Par {
        Par {
            unforgeables: vec![GUnforgeable {
                unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
                    id: self.id().to_vec(),
                })),
            }],
            ..Par::default()
        }
    }

    /// The marker with the id `id`. Only a 2-byte id with the Reified tag can match.
    pub fn from_id(id: &[u8]) -> Option<Marker> {
        match id {
            [REIFIED_TAG, index] => Marker::ALL
                .into_iter()
                .find(|marker| *marker as u8 == *index),
            _ => None,
        }
    }

    /// The marker name in diagnostics.
    pub const fn name(self) -> &'static str {
        match self {
            Marker::Use => "USE",
            Marker::NewSpace => "NEWSPACE",
            Marker::Params => "PARAMS",
            Marker::Query => "QUERY",
            Marker::Free => "FREE",
        }
    }
}

impl fmt::Display for Marker {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

/// `use s { P }`: `space` is the default space of the untagged names in `body`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UseBlock {
    pub space: Par,
    pub body: Par,
}

/// `new x_1[: s_1], … in P` with at least one annotation. Entry `i` of `annotations`
/// annotates plain binder `i` of `new`, in binding order; the URI binders have no entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewSpace {
    pub annotations: Vec<Option<Par>>,
    pub new: New,
}

/// The params of a send: positional params in source order and named params by key.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SendParams {
    pub positional: Vec<Par>,
    pub named: BTreeMap<String, Par>,
}

/// `ch ~ (q…; k=v…)`: the channel, the payload and the named params of a receive bind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuerySpec {
    pub channel: Par,
    pub payload: Vec<Par>,
    pub named: BTreeMap<String, Par>,
}

/// `free Th(a…)`: a theory name and its arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FreeTheory {
    pub name: String,
    pub args: Vec<Par>,
}

/// A Reified construct of any marker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReifiedConstruct {
    Use(UseBlock),
    NewSpace(NewSpace),
    Params(SendParams),
    Query(QuerySpec),
    Free(FreeTheory),
}

impl ReifiedConstruct {
    /// The marker of the construct.
    pub fn marker(&self) -> Marker {
        match self {
            ReifiedConstruct::Use(_) => Marker::Use,
            ReifiedConstruct::NewSpace(_) => Marker::NewSpace,
            ReifiedConstruct::Params(_) => Marker::Params,
            ReifiedConstruct::Query(_) => Marker::Query,
            ReifiedConstruct::Free(_) => Marker::Free,
        }
    }
}

/// A construct that the reducer recognizes in process position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProcessConstruct {
    Use(UseBlock),
    NewSpace(NewSpace),
}

impl ProcessConstruct {
    /// The marker of the construct.
    pub fn marker(&self) -> Marker {
        match self {
            ProcessConstruct::Use(_) => Marker::Use,
            ProcessConstruct::NewSpace(_) => Marker::NewSpace,
        }
    }
}

impl From<ProcessConstruct> for ReifiedConstruct {
    fn from(construct: ProcessConstruct) -> Self {
        match construct {
            ProcessConstruct::Use(block) => ReifiedConstruct::Use(block),
            ProcessConstruct::NewSpace(new_space) => ReifiedConstruct::NewSpace(new_space),
        }
    }
}

/// A construct without a canonical encoding.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ReifiedEncodeError {
    #[error("PARAMS needs a positional or a named param")]
    EmptyParams,
    #[error("QUERY needs a payload element or a named param")]
    EmptyQuery,
    #[error("NEWSPACE needs an annotated binder")]
    NoAnnotation,
    #[error(
        "NEWSPACE has {annotations} annotations for a New with {bind_count} binders and {uris} \
         URIs"
    )]
    AnnotationCount {
        annotations: usize,
        bind_count: i32,
        uris: usize,
    },
    #[error("{name:?} is not a valid {position}")]
    InvalidIdentifier {
        name: String,
        position: IdentifierPosition,
    },
}

/// The element kind that a decoder expects at a tuple index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExpectedElement {
    /// The USE or the NEWSPACE marker name.
    ProcessMarker,
    /// A `Par` that is exactly one list.
    List,
    /// A `Par` that is exactly one map.
    Map,
    /// A `Par` that is exactly one string.
    String,
}

impl fmt::Display for ExpectedElement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            ExpectedElement::ProcessMarker => "the USE or the NEWSPACE marker",
            ExpectedElement::List => "a list",
            ExpectedElement::Map => "a map",
            ExpectedElement::String => "a string",
        })
    }
}

/// A term with a marker head that is not the canonical encoding of a construct.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ReifiedDecodeError {
    #[error("the {marker} tuple has {actual} elements instead of {expected}")]
    WrongArity {
        marker: Marker,
        expected: usize,
        actual: usize,
    },
    #[error("tuple element {index} is not {expected}")]
    ElementKind {
        index: usize,
        expected: ExpectedElement,
    },
    #[error("tuple element {index} has a remainder")]
    Remainder { index: usize },
    #[error("a named param has a key that is not a string")]
    NonStringKey,
    #[error("{name:?} is not a valid {position}")]
    InvalidIdentifier {
        name: String,
        position: IdentifierPosition,
    },
    #[error("annotation {binder} is neither [] nor [s]")]
    AnnotationShape { binder: usize },
    #[error(
        "NEWSPACE has {annotations} annotations for a New with {bind_count} binders and {uris} \
         URIs"
    )]
    AnnotationCount {
        annotations: usize,
        bind_count: i32,
        uris: usize,
    },
    #[error("the {marker} tuple holds an erased empty form")]
    EmptyCanonicalForm { marker: Marker },
    #[error("NEWSPACE element 2 is not a Par with exactly one New")]
    NotSoleNew,
    #[error("the {marker} tuple is not the canonical encoding of its construct")]
    NonCanonical { marker: Marker },
}

/// The one component of a `Par` that holds nothing else. The bit sets do not count.
enum SoleComponent<'a> {
    Expr(&'a ExprInstance),
    Unforgeable(&'a GUnforgeable),
    New(&'a New),
    Other,
}

fn sole_component(par: &Par) -> SoleComponent<'_> {
    let Par {
        sends,
        receives,
        news,
        exprs,
        matches,
        unforgeables,
        bundles,
        connectives,
        conditionals,
        locally_free: _,
        connective_used: _,
        cost_signed_terms,
        cost_stacks,
    } = par;
    let structural = sends.is_empty()
        && receives.is_empty()
        && matches.is_empty()
        && bundles.is_empty()
        && connectives.is_empty()
        && conditionals.is_empty()
        && cost_signed_terms.is_empty()
        && cost_stacks.is_empty();
    match (
        structural,
        exprs.as_slice(),
        unforgeables.as_slice(),
        news.as_slice(),
    ) {
        (
            true,
            [Expr {
                expr_instance: Some(instance),
            }],
            [],
            [],
        ) => SoleComponent::Expr(instance),
        (true, [], [unforgeable], []) => SoleComponent::Unforgeable(unforgeable),
        (true, [], [], [new]) => SoleComponent::New(new),
        _ => SoleComponent::Other,
    }
}

/// The expression of a `Par` that holds exactly one expression and nothing else.
pub(crate) fn sole_expr(par: &Par) -> Option<&ExprInstance> {
    match sole_component(par) {
        SoleComponent::Expr(instance) => Some(instance),
        SoleComponent::Unforgeable(_) | SoleComponent::New(_) | SoleComponent::Other => None,
    }
}

/// The marker of `par` when `par` equals a marker name. The check does not allocate.
pub fn marker_of(par: &Par) -> Option<Marker> {
    match sole_component(par) {
        SoleComponent::Unforgeable(GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id })),
        }) if !par.connective_used => Marker::from_id(id),
        SoleComponent::Unforgeable(_)
        | SoleComponent::Expr(_)
        | SoleComponent::New(_)
        | SoleComponent::Other => None,
    }
}

fn head_marker(tuple: &ETuple) -> Option<Marker> { tuple.ps.first().and_then(marker_of) }

/// True when the head of `tuple` is the USE or the NEWSPACE marker (hook H1).
pub fn is_process_marker_tuple(tuple: &ETuple) -> bool {
    match head_marker(tuple) {
        Some(Marker::Use | Marker::NewSpace) => true,
        Some(Marker::Params | Marker::Query | Marker::Free) | None => false,
    }
}

/// The marker and the tuple when `par` is exactly one tuple with a marker head.
pub fn sole_marker_tuple(par: &Par) -> Option<(Marker, &ETuple)> {
    match sole_expr(par) {
        Some(ExprInstance::ETupleBody(tuple)) => head_marker(tuple).map(|marker| (marker, tuple)),
        _ => None,
    }
}

/// True when `name` is a token of the grammar rule `var`:
/// `[a-zA-Z][a-zA-Z0-9_']*` or `_[a-zA-Z0-9_']+`.
pub fn is_identifier(name: &str) -> bool {
    match name.as_bytes() {
        [first, rest @ ..] if first.is_ascii_alphabetic() => {
            rest.iter().all(|byte| is_identifier_tail(*byte))
        }
        [b'_', rest @ ..] => !rest.is_empty() && rest.iter().all(|byte| is_identifier_tail(*byte)),
        _ => false,
    }
}

fn is_identifier_tail(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'\''
}

/// The `locally_free` union and the `connective_used` disjunction of `pars`, as the
/// collection normalizer computes them.
fn bits_of(pars: &[Par]) -> (Vec<u8>, bool) {
    pars.iter().fold(
        (Vec::new(), false),
        |(locally_free, connective_used), par| {
            (
                union(locally_free, par.locally_free.clone()),
                connective_used || par.connective_used,
            )
        },
    )
}

/// A `Par` that holds exactly one expression, as `prepend_expr` builds it.
fn expr_par(instance: ExprInstance, locally_free: Vec<u8>, connective_used: bool) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(instance),
        }],
        locally_free,
        connective_used,
        ..Par::default()
    }
}

fn list_par(ps: Vec<Par>) -> Par {
    let (locally_free, connective_used) = bits_of(&ps);
    expr_par(
        ExprInstance::EListBody(EList {
            ps,
            locally_free: locally_free.clone(),
            connective_used,
            remainder: None,
        }),
        locally_free,
        connective_used,
    )
}

fn string_par(value: &str) -> Par {
    expr_par(ExprInstance::GString(value.to_owned()), Vec::new(), false)
}

fn map_par(named: &BTreeMap<String, Par>) -> Par {
    let pairs = named
        .iter()
        .map(|(key, value)| (string_par(key), value.clone()))
        .collect();
    let map = ParMapTypeMapper::par_map_to_emap(ParMap::create_from_vec(pairs));
    let (locally_free, connective_used) = (map.locally_free.clone(), map.connective_used);
    expr_par(ExprInstance::EMapBody(map), locally_free, connective_used)
}

/// `Par{news: [new]}`, as `prepend_new` builds it.
fn new_par(new: New) -> Par {
    let locally_free = new.locally_free.clone();
    let connective_used = new.p.as_ref().is_some_and(|body| body.connective_used);
    Par {
        news: vec![new],
        locally_free,
        connective_used,
        ..Par::default()
    }
}

fn annotation_par(annotation: &Option<Par>) -> Par {
    list_par(annotation.iter().cloned().collect())
}

fn marker_tuple<const N: usize>(marker: Marker, fields: [Par; N]) -> ETuple {
    let mut ps = Vec::with_capacity(N + 1);
    ps.push(marker.par());
    ps.extend(fields);
    let (locally_free, connective_used) = bits_of(&ps);
    ETuple {
        ps,
        locally_free,
        connective_used,
    }
}

/// The `Par` that holds exactly the tuple `tuple`.
pub(crate) fn tuple_par(tuple: ETuple) -> Par {
    let (locally_free, connective_used) = (tuple.locally_free.clone(), tuple.connective_used);
    expr_par(
        ExprInstance::ETupleBody(tuple),
        locally_free,
        connective_used,
    )
}

fn tuple_expr(tuple: ETuple) -> Expr {
    Expr {
        expr_instance: Some(ExprInstance::ETupleBody(tuple)),
    }
}

fn annotation_count_matches(annotations: usize, new: &New) -> bool {
    match (
        usize::try_from(new.bind_count),
        annotations.checked_add(new.uri.len()),
    ) {
        (Ok(binders), Some(slots)) => binders == slots,
        (Ok(_), None) | (Err(_), Some(_) | None) => false,
    }
}

fn check_keys(named: &BTreeMap<String, Par>) -> Result<(), ReifiedEncodeError> {
    match named.keys().find(|key| !is_param_key(key)) {
        Some(key) => Err(ReifiedEncodeError::InvalidIdentifier {
            name: key.clone(),
            position: IdentifierPosition::ParamKey,
        }),
        None => Ok(()),
    }
}

pub(crate) fn use_tuple(space: Par, body: Par) -> ETuple {
    marker_tuple(Marker::Use, [space, body])
}

pub(crate) fn new_space_tuple(
    annotations: &[Option<Par>],
    new: New,
) -> Result<ETuple, ReifiedEncodeError> {
    if !annotation_count_matches(annotations.len(), &new) {
        return Err(ReifiedEncodeError::AnnotationCount {
            annotations: annotations.len(),
            bind_count: new.bind_count,
            uris: new.uri.len(),
        });
    }
    if !annotations.iter().any(Option::is_some) {
        return Err(ReifiedEncodeError::NoAnnotation);
    }
    let annotations = list_par(annotations.iter().map(annotation_par).collect());
    Ok(marker_tuple(Marker::NewSpace, [annotations, new_par(new)]))
}

pub(crate) fn params_tuple(
    positional: Vec<Par>,
    named: &BTreeMap<String, Par>,
) -> Result<ETuple, ReifiedEncodeError> {
    if positional.is_empty() && named.is_empty() {
        return Err(ReifiedEncodeError::EmptyParams);
    }
    check_keys(named)?;
    Ok(marker_tuple(Marker::Params, [
        list_par(positional),
        map_par(named),
    ]))
}

pub(crate) fn query_tuple(
    channel: Par,
    payload: Vec<Par>,
    named: &BTreeMap<String, Par>,
) -> Result<ETuple, ReifiedEncodeError> {
    if payload.is_empty() && named.is_empty() {
        return Err(ReifiedEncodeError::EmptyQuery);
    }
    check_keys(named)?;
    Ok(marker_tuple(Marker::Query, [
        channel,
        list_par(payload),
        map_par(named),
    ]))
}

pub(crate) fn free_tuple(name: &str, args: Vec<Par>) -> Result<ETuple, ReifiedEncodeError> {
    if !is_theory_name(name) {
        return Err(ReifiedEncodeError::InvalidIdentifier {
            name: name.to_owned(),
            position: IdentifierPosition::TheoryName,
        });
    }
    Ok(marker_tuple(Marker::Free, [
        string_par(name),
        list_par(args),
    ]))
}

/// The marker tuple of a construct, as an expression.
pub fn encode(construct: &ReifiedConstruct) -> Result<Expr, ReifiedEncodeError> {
    match construct {
        ReifiedConstruct::Use(block) => Ok(encode_use(block)),
        ReifiedConstruct::NewSpace(new_space) => encode_new_space(new_space),
        ReifiedConstruct::Params(params) => {
            params_tuple(params.positional.clone(), &params.named).map(tuple_expr)
        }
        ReifiedConstruct::Query(query) => {
            query_tuple(query.channel.clone(), query.payload.clone(), &query.named).map(tuple_expr)
        }
        ReifiedConstruct::Free(theory) => {
            free_tuple(&theory.name, theory.args.clone()).map(tuple_expr)
        }
    }
}

/// The USE tuple of `block`, an element of `Par.exprs`.
pub fn encode_use(block: &UseBlock) -> Expr {
    tuple_expr(use_tuple(block.space.clone(), block.body.clone()))
}

/// The NEWSPACE tuple of `new_space`, an element of `Par.exprs`.
pub fn encode_new_space(new_space: &NewSpace) -> Result<Expr, ReifiedEncodeError> {
    new_space_tuple(&new_space.annotations, new_space.new.clone()).map(tuple_expr)
}

/// The trailing data element of a send. A send without params has none.
pub fn encode_params(params: &SendParams) -> Result<Option<Par>, ReifiedEncodeError> {
    if params.positional.is_empty() && params.named.is_empty() {
        return Ok(None);
    }
    params_tuple(params.positional.clone(), &params.named).map(|tuple| Some(tuple_par(tuple)))
}

/// The bind source of a query.
pub fn encode_query(query: &QuerySpec) -> Result<Par, ReifiedEncodeError> {
    query_tuple(query.channel.clone(), query.payload.clone(), &query.named).map(tuple_par)
}

/// The value of `free Th(a…)`.
pub fn encode_free(theory: &FreeTheory) -> Result<Par, ReifiedEncodeError> {
    free_tuple(&theory.name, theory.args.clone()).map(tuple_par)
}

fn elements<const N: usize>(
    tuple: &ETuple,
    marker: Marker,
) -> Result<&[Par; N], ReifiedDecodeError> {
    <&[Par; N]>::try_from(tuple.ps.as_slice()).map_err(|_| ReifiedDecodeError::WrongArity {
        marker,
        expected: N,
        actual: tuple.ps.len(),
    })
}

fn sole_list(par: &Par, index: usize) -> Result<&EList, ReifiedDecodeError> {
    match sole_expr(par) {
        Some(ExprInstance::EListBody(list)) => match list.remainder {
            None => Ok(list),
            Some(_) => Err(ReifiedDecodeError::Remainder { index }),
        },
        _ => Err(ReifiedDecodeError::ElementKind {
            index,
            expected: ExpectedElement::List,
        }),
    }
}

fn sole_map(par: &Par, index: usize) -> Result<&EMap, ReifiedDecodeError> {
    match sole_expr(par) {
        Some(ExprInstance::EMapBody(map)) => match map.remainder {
            None => Ok(map),
            Some(_) => Err(ReifiedDecodeError::Remainder { index }),
        },
        _ => Err(ReifiedDecodeError::ElementKind {
            index,
            expected: ExpectedElement::Map,
        }),
    }
}

fn sole_string(par: &Par) -> Option<&str> {
    match sole_expr(par) {
        Some(ExprInstance::GString(value)) => Some(value),
        _ => None,
    }
}

fn sole_new(par: &Par) -> Option<&New> {
    match sole_component(par) {
        SoleComponent::New(new) => Some(new),
        SoleComponent::Expr(_) | SoleComponent::Unforgeable(_) | SoleComponent::Other => None,
    }
}

fn decode_named(map: &EMap, marker: Marker) -> Result<BTreeMap<String, Par>, ReifiedDecodeError> {
    let mut named = BTreeMap::new();
    for KeyValuePair { key, value } in &map.kvs {
        let name = key
            .as_ref()
            .and_then(sole_string)
            .ok_or(ReifiedDecodeError::NonStringKey)?;
        if !is_param_key(name) {
            return Err(ReifiedDecodeError::InvalidIdentifier {
                name: name.to_owned(),
                position: IdentifierPosition::ParamKey,
            });
        }
        let value = value
            .as_ref()
            .ok_or(ReifiedDecodeError::NonCanonical { marker })?;
        named.insert(name.to_owned(), value.clone());
    }
    Ok(named)
}

fn decode_annotation(annotation: &Par, binder: usize) -> Result<Option<Par>, ReifiedDecodeError> {
    match sole_expr(annotation) {
        Some(ExprInstance::EListBody(EList {
            ps,
            remainder: None,
            ..
        })) => match ps.as_slice() {
            [] => Ok(None),
            [space] => Ok(Some(space.clone())),
            _ => Err(ReifiedDecodeError::AnnotationShape { binder }),
        },
        _ => Err(ReifiedDecodeError::AnnotationShape { binder }),
    }
}

fn decode_use(tuple: &ETuple) -> Result<UseBlock, ReifiedDecodeError> {
    let [_, space, body] = elements::<3>(tuple, Marker::Use)?;
    if use_tuple(space.clone(), body.clone()) != *tuple {
        return Err(ReifiedDecodeError::NonCanonical {
            marker: Marker::Use,
        });
    }
    Ok(UseBlock {
        space: space.clone(),
        body: body.clone(),
    })
}

fn decode_new_space(tuple: &ETuple) -> Result<NewSpace, ReifiedDecodeError> {
    let [_, annotations, new] = elements::<3>(tuple, Marker::NewSpace)?;
    let annotations = sole_list(annotations, 1)?
        .ps
        .iter()
        .enumerate()
        .map(|(binder, annotation)| decode_annotation(annotation, binder))
        .collect::<Result<Vec<_>, _>>()?;
    let new = sole_new(new).ok_or(ReifiedDecodeError::NotSoleNew)?;
    if !annotation_count_matches(annotations.len(), new) {
        return Err(ReifiedDecodeError::AnnotationCount {
            annotations: annotations.len(),
            bind_count: new.bind_count,
            uris: new.uri.len(),
        });
    }
    if !annotations.iter().any(Option::is_some) {
        return Err(ReifiedDecodeError::EmptyCanonicalForm {
            marker: Marker::NewSpace,
        });
    }
    match new_space_tuple(&annotations, new.clone()) {
        Ok(canonical) if canonical == *tuple => Ok(NewSpace {
            annotations,
            new: new.clone(),
        }),
        Ok(_) | Err(_) => Err(ReifiedDecodeError::NonCanonical {
            marker: Marker::NewSpace,
        }),
    }
}

fn decode_params(tuple: &ETuple, input: &Par) -> Result<SendParams, ReifiedDecodeError> {
    let [_, positional, named] = elements::<3>(tuple, Marker::Params)?;
    let params = SendParams {
        positional: sole_list(positional, 1)?.ps.clone(),
        named: decode_named(sole_map(named, 2)?, Marker::Params)?,
    };
    if params.positional.is_empty() && params.named.is_empty() {
        return Err(ReifiedDecodeError::EmptyCanonicalForm {
            marker: Marker::Params,
        });
    }
    match params_tuple(params.positional.clone(), &params.named).map(tuple_par) {
        Ok(canonical) if canonical == *input => Ok(params),
        Ok(_) | Err(_) => Err(ReifiedDecodeError::NonCanonical {
            marker: Marker::Params,
        }),
    }
}

fn decode_query(tuple: &ETuple, input: &Par) -> Result<QuerySpec, ReifiedDecodeError> {
    let [_, channel, payload, named] = elements::<4>(tuple, Marker::Query)?;
    let query = QuerySpec {
        channel: channel.clone(),
        payload: sole_list(payload, 2)?.ps.clone(),
        named: decode_named(sole_map(named, 3)?, Marker::Query)?,
    };
    if query.payload.is_empty() && query.named.is_empty() {
        return Err(ReifiedDecodeError::EmptyCanonicalForm {
            marker: Marker::Query,
        });
    }
    match query_tuple(query.channel.clone(), query.payload.clone(), &query.named).map(tuple_par) {
        Ok(canonical) if canonical == *input => Ok(query),
        Ok(_) | Err(_) => Err(ReifiedDecodeError::NonCanonical {
            marker: Marker::Query,
        }),
    }
}

fn decode_free(tuple: &ETuple, input: &Par) -> Result<FreeTheory, ReifiedDecodeError> {
    let [_, name, args] = elements::<3>(tuple, Marker::Free)?;
    let name = sole_string(name).ok_or(ReifiedDecodeError::ElementKind {
        index: 1,
        expected: ExpectedElement::String,
    })?;
    if !is_theory_name(name) {
        return Err(ReifiedDecodeError::InvalidIdentifier {
            name: name.to_owned(),
            position: IdentifierPosition::TheoryName,
        });
    }
    let theory = FreeTheory {
        name: name.to_owned(),
        args: sole_list(args, 2)?.ps.clone(),
    };
    match free_tuple(&theory.name, theory.args.clone()).map(tuple_par) {
        Ok(canonical) if canonical == *input => Ok(theory),
        Ok(_) | Err(_) => Err(ReifiedDecodeError::NonCanonical {
            marker: Marker::Free,
        }),
    }
}

/// Decodes a USE or NEWSPACE tuple of `Par.exprs` (hook H2).
pub fn decode_process_tuple(tuple: &ETuple) -> Result<ProcessConstruct, ReifiedDecodeError> {
    match head_marker(tuple) {
        Some(Marker::Use) => decode_use(tuple).map(ProcessConstruct::Use),
        Some(Marker::NewSpace) => decode_new_space(tuple).map(ProcessConstruct::NewSpace),
        Some(Marker::Params | Marker::Query | Marker::Free) | None => {
            Err(ReifiedDecodeError::ElementKind {
                index: 0,
                expected: ExpectedElement::ProcessMarker,
            })
        }
    }
}

/// Decodes the params of a send from its last data element (hook H3).
pub fn split_trailing_params(data: &[Par]) -> Result<Option<SendParams>, ReifiedDecodeError> {
    let Some(last) = data.last() else {
        return Ok(None);
    };
    match sole_marker_tuple(last) {
        Some((Marker::Params, tuple)) => decode_params(tuple, last).map(Some),
        Some((Marker::Use | Marker::NewSpace | Marker::Query | Marker::Free, _)) | None => Ok(None),
    }
}

/// Decodes the query of a receive bind from its evaluated source (hook H4).
pub fn decode_query_source(source: &Par) -> Result<Option<QuerySpec>, ReifiedDecodeError> {
    match sole_marker_tuple(source) {
        Some((Marker::Query, tuple)) => decode_query(tuple, source).map(Some),
        Some((Marker::Use | Marker::NewSpace | Marker::Params | Marker::Free, _)) | None => {
            Ok(None)
        }
    }
}

/// Decodes the value of `free Th(a…)`, which the P4 factories read.
pub fn decode_free_value(value: &Par) -> Result<Option<FreeTheory>, ReifiedDecodeError> {
    match sole_marker_tuple(value) {
        Some((Marker::Free, tuple)) => decode_free(tuple, value).map(Some),
        Some((Marker::Use | Marker::NewSpace | Marker::Params | Marker::Query, _)) | None => {
            Ok(None)
        }
    }
}
