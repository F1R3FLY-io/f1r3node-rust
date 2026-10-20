//! G1-4a (DR-122): the linear pre-execution cost estimate of an offered
//! deploy, as `docs/casper/theory/cost-accounting-impl/linear-cost-estimate.md`
//! specifies it. The word *linear* names the linear resource proof of the
//! cost-accounting papers, which the estimate supplies (design §1).
//!
//! The estimate runs four walks, each on a heap work stack (DR-67):
//!
//! 1. The pre-pass computes the message height, a length bound and the
//!    variable count of every subterm once, bottom-up, and the peak binder
//!    count that sizes the environment of pass 1.
//! 2. `static_authority_plan` decides rule X1 and gives the compute row.
//! 3. Pass 1 gives every de Bruijn level a binder kind, checks rules X2 to
//!    X8, sizes every send and receive with the reducer's own evaluation,
//!    substitution and measurement functions, and emits the post-order
//!    program of the vector fold.
//! 4. Pass 2 runs that program on a value stack. Parallel parts add, and
//!    branches take the componentwise maximum.
//!
//! The work of these walks grows with the term plus its region links, merged
//! map entries and sized bytes (design §5.8). Data are evaluated only when
//! every form in them has a known work limit
//! (arbitration H2), and only values whose message height fits the sizing
//! bound reach a recursive function of the reducer.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::cost_signature::Value as CostSignatureValue;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    BindPattern, Bundle, CostSignature, CostSignedTerm, EPathMap, EVar, Expr, GPrivate,
    GUnforgeable, If, Match, MatchCase, New, Par, Receive, ReceiveBind, Send, Var,
};
use models::rust::canonical_path::decode_trie_path;
use models::rust::epathmap_trie_codec::EPathMapMode;
use models::rust::phlo_schedule::PhloScheduleV1;
use models::rust::rholang::implicits::single_bundle;
use prost::Message;
use typed_arena::Arena;

use super::{static_authority_plan, static_authority_signatures, SigKey};
use crate::rust::interpreter::accounting::authority::{
    canonical_cost_signature, cost_region, cost_signature_to_sig, sig_to_cost_signature,
    UnprovableDemand,
};
use crate::rust::interpreter::accounting::byte_accounting::{
    comm_trace_bytes, consume_introduction_bytes, continuation_bytes, datum_bytes,
    message_field_bytes, par_with_random_bytes, produce_introduction_bytes, ByteAccountingError,
};
use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::accounting::lexical::resolve_lexical_names_for_funding;
// Added by dedup (task 11415, D8): the leaf walk of the native meter.
use crate::rust::interpreter::accounting::native_phlo_rules::{
    signature_leaf_units, SignatureUnitsMeter,
};
use crate::rust::interpreter::accounting::native_phlo_rules::{
    NativePhloDimension, NativePhloRules,
};
use crate::rust::interpreter::accounting::{RuntimeBudget, Sig};
use crate::rust::interpreter::deterministic_reduction;
use crate::rust::interpreter::env::Env;
use crate::rust::interpreter::errors::InterpreterError;
use crate::rust::interpreter::interpreter::InterpreterImpl;
use crate::rust::interpreter::metering::MeteredMachine;
use crate::rust::interpreter::reduce::ReducerCore;
use crate::rust::interpreter::rho_type::{RhoExpression, RhoUnforgeable};
use crate::rust::interpreter::substitute::Substitute;

/// The first completeness rule that an estimate fails (design §5.3). The
/// estimate is complete only when every rule holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IncompleteReason {
    /// X1: `static_authority_plan` rejects the term.
    StaticPlan(UnprovableDemand),
    /// A signed source is an on-chain private-name payer. The check of step
    /// G1-4b sets this reason.
    OnChainPayer,
    /// X2: a send or receive channel is not a level of kind fresh or fresh
    /// hole.
    NonFreshChannel,
    /// X3: a fresh level occurs outside a channel and a cost signature.
    EscapingName,
    /// X4: send data have free variables.
    OpenData,
    /// X5: a receive pattern uses an outer level.
    OpenPattern,
    /// X6: an introduced value depends on a received or matched value.
    ReceivedValueInIntroduction,
    /// X7: a peek bind or a cost stack.
    PeekOrStack,
    /// X8: a method call among the top-level expressions of an execution
    /// position.
    MethodInProcessPosition,
    /// X4 as DR-122 narrows it: a datum uses a form without a known work
    /// limit, or its evaluation fails.
    DataEvaluation,
    /// A value is nested above the sizing bound, or a size has no static
    /// value.
    UnsizedValue,
    /// Added by G1-6 (DR-124): a lane outside the client lanes has known
    /// demand. The check holds only the client's demand (P1:2191-2200), so
    /// that demand leaves the estimate. The check of step G1-6 sets this
    /// reason. Since the G1-6 fix, a client lane is a lane at whose purse a
    /// source is located, or that a source permits for at least one class.
    ForeignLane,
}

/// Whether the estimate covers all cost of the deploy (design §5.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Completeness {
    Complete,
    Incomplete(IncompleteReason),
}

/// The raw counts `V(s,k)`, the estimate `B` in phlo, the static signature of
/// every lane and the completeness of the estimate (design §5.1). The class
/// index of a quantity is the schedule's class of its native dimension.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CostEstimate {
    pub quantities: BTreeMap<(SigKey, usize), u64>,
    pub bound_phlo: u64,
    pub signatures: BTreeMap<SigKey, CostSignature>,
    pub completeness: Completeness,
}

impl CostEstimate {
    /// Added by G1-6 (DR-124): the estimate of the client lanes alone. P1's
    /// acceptance protocol checks the client's demand only (P1:2191-2200),
    /// and a layer signed by another signature adds nothing to that demand
    /// (P1:2002-2027). The check gives the client lanes: the deploy lane, the
    /// lanes at whose purses the sources are located, and, since the G1-6
    /// fix, the lanes that the sources permit. Every other lane
    /// leaves the estimate, and B counts the kept lanes with the formula of
    /// the analysis: weight times units times count, with an overflow that
    /// gives `u64::MAX`. An estimate that loses a counted lane is incomplete
    /// with the reason `ForeignLane`, unless an earlier rule already made it
    /// incomplete.
    pub fn retain_client_lanes(
        self,
        client: &BTreeSet<SigKey>,
        schedule: &PhloScheduleV1<'_>,
    ) -> Result<CostEstimate, InterpreterError> {
        let CostEstimate {
            quantities,
            mut signatures,
            completeness,
            ..
        } = self;
        signatures.retain(|lane, _| client.contains(lane));
        let mut units = BTreeMap::new();
        for (lane, signature) in &signatures {
            units.insert(*lane, signature_units(signature));
        }
        // Changed by dedup (task 11415, D8): the B loop moved to `bound_over`,
        // which `assemble` shares. The loop below keeps the client quantities.
        // `bound_over` then checks each kept quantity in the same order as
        // before, the units of its lane first and then its class.
        // let mut kept = BTreeMap::new();
        // let mut foreign = false;
        // let mut bound_phlo = Some(0u64);
        // for ((lane, class), count) in quantities {
        //     let lane_units = match (client.contains(&lane), units.get(&lane)) {
        //         (false, _) => {
        //             foreign = true;
        //             continue;
        //         }
        //         (true, None) => {
        //             return Err(InterpreterError::ReduceError(
        //                 "the estimate counted a lane without a static signature".to_string(),
        //             ))
        //         }
        //         (true, Some(lane_units)) => lane_units,
        //     };
        //     let weight = schedule
        //         .classes
        //         .get(class)
        //         .map(|class| class.weight)
        //         .ok_or_else(|| {
        //             InterpreterError::ReduceError(
        //                 "the schedule has no class for a counted quantity".to_string(),
        //             )
        //         })?;
        //     bound_phlo = bound_phlo.and_then(|bound| {
        //         weight
        //             .checked_mul(*lane_units)
        //             .and_then(|amount| amount.checked_mul(count))
        //             .and_then(|amount| bound.checked_add(amount))
        //     });
        //     kept.insert((lane, class), count);
        // }
        let mut kept = BTreeMap::new();
        let mut foreign = false;
        for ((lane, class), count) in quantities {
            if !client.contains(&lane) {
                foreign = true;
                continue;
            }
            kept.insert((lane, class), count);
        }
        let bound_phlo = bound_over(
            &kept,
            &units,
            schedule,
            "the schedule has no class for a counted quantity",
        )?;
        let completeness = match (completeness, foreign) {
            (Completeness::Complete, true) => {
                Completeness::Incomplete(IncompleteReason::ForeignLane)
            }
            (completeness, _) => completeness,
        };
        Ok(CostEstimate {
            quantities: kept,
            bound_phlo: bound_phlo.unwrap_or(u64::MAX),
            signatures,
            completeness,
        })
    }
}

/// Added by the G1-6 fix (DR-124): the lane key of a supply location. A lane
/// key is the digest of the encoded supply channel of the lane's signature
/// under the lane domain (`Sig::lane_hash`). A resource permission names its
/// lane by that encoded channel, so the lane key of a permission is the
/// digest of its location. The check uses it to find the lanes that a source
/// permits.
pub fn location_lane_key(location: &[u8]) -> SigKey {
    // Moved by dedup (task 11415) to `lane_digest` in
    // `rholang/src/rust/interpreter/accounting/mod.rs`.
    // let digest = crypto::rust::hash::blake2b256::Blake2b256::hash_parts([domain, location]);
    // digest
    //     .as_slice()
    //     .try_into()
    //     .map_err(|_| InterpreterError::ReduceError("a lane digest is not 32 bytes".to_string()))
    crate::rust::interpreter::accounting::lane_digest(location)
}

/// The per-message length bound of the pre-pass: a key, a length prefix and
/// every fixed scalar field of one message fit in it.
const NODE_BYTES: u64 = 48;
/// The height bound of a `Par` with leaf content: `Par`, its item, a variant
/// message, a `Var` and its wildcard message.
const PAR_LEAF_HEIGHT: u32 = 5;
const SIGNATURE_LEAF_HEIGHT: u32 = 3;
/// The largest message height of a value that reaches a recursive function
/// of the reducer: substitution, evaluation, sorting, measurement and drop.
/// It was the depth bound of `SorterMeter::enter`, which DR-131 S7 deleted.
pub(crate) const SIZING_HEIGHT_LIMIT: u32 = 256;
/// Added by the merge of feature/cost-accounted-rho (finding R1-16): the
/// member nesting at which the walks stop decoding pathmap members.
/// F1R3Lang's `EPathMap` keeps each member as a canonical trie key. A walk
/// decodes a member before it visits it. It decodes the members of that
/// member again from the bytes of their owner, so a chain of `n` levels
/// decodes about `n` copies of its bytes. Each level of members lies at least
/// three messages below its owner. A value with this many levels is therefore
/// taller than `SIZING_HEIGHT_LIMIT`, and each check of its height finds it
/// unsized whether or not the walk decodes the deeper levels.
const MEMBER_NESTING_LIMIT: u32 = SIZING_HEIGHT_LIMIT / 3 + 1;
/// Added by the same fix: the height that the pre-pass gives a `Par` whose
/// pathmap members lie at `MEMBER_NESTING_LIMIT`.
const UNSIZED_HEIGHT: u32 = SIZING_HEIGHT_LIMIT + 1;

/// The wrapper messages around a sized value: the datum, the continuation,
/// the random state and the seal.
const WRAPPER_MARGIN: u32 = 8;
/// A serialized `Blake2b512Random` always has 536 bytes.
pub(crate) const RANDOM_STATE_BYTES: usize = 536;
static RANDOM_STATE: [u8; RANDOM_STATE_BYTES] = [0; RANDOM_STATE_BYTES];
/// The explicit stack segment of pass 1. The walks use a heap work stack, so
/// only the reducer's recursion inside one sizing call uses this segment, and
/// the sizing bound limits that recursion (arbitration M4).
const SIZING_STACK_BYTES: usize = 64 * 1024 * 1024;
/// A fresh name stands for a private name with a 32-byte identity, and the
/// markers have identities of other lengths, so no deploy name equals one.
const FRESH_ID_BYTES: usize = 32;
const POISON_ID_BYTES: usize = 33;
const UNKNOWN_ID_BYTES: usize = 34;

/// The index of the compute row in a lane's counts, which follow the order of
/// `NativePhloDimension::ALL`: compute, introduction, transfer and trace.
const COMPUTE: usize = 0;

/// The estimate of one resolved term, as design §5 specifies it.
///
/// `resolved` is the output of the funding resolver, `funding` is the funding
/// signature, and `evaluator` is a reducer whose URN map the offered runtime
/// uses. The analysis evaluates with a copy of `evaluator` whose two meters
/// are fresh, so the runtime's budget stays untouched (arbitration M1). An
/// error means that an input is malformed, and the check rejects the deploy
/// without a charge.
pub fn linear_cost_estimate(
    resolved: &Par,
    funding: &Sig,
    schedule: &PhloScheduleV1<'_>,
    evaluator: &ReducerCore,
) -> Result<CostEstimate, InterpreterError> {
    debug_assert!(
        deterministic_reduction::current().is_none(),
        "the pre-execution estimate runs outside a reduction session"
    );
    let rules = NativePhloRules::resolve(schedule)
        .map_err(|error| InterpreterError::ReduceError(error.to_string()))?;
    let core = isolated_evaluator(evaluator);
    stacker::grow(SIZING_STACK_BYTES, || {
        let mut estimator = Estimator::new(funding, &core)?;
        estimator.measure(resolved)?;
        // Changed by the G1-4a fix (review M-2): the plan and the static
        // signature list canonicalize every signature recursively, so they
        // run only when no signature is taller than the sizing bound.
        // let plan = static_authority_plan(resolved, funding);
        let shallow = estimator.signatures_fit();
        let plan = match shallow {
            true => Some(static_authority_plan(resolved, funding)),
            false => {
                estimator.violate(IncompleteReason::UnsizedValue);
                None
            }
        };
        estimator.walk(resolved)?;
        let tables = estimator.tables()?;
        let vector = estimator.run(&tables)?;
        // let static_signatures = static_authority_signatures(resolved).ok();
        let static_signatures = match shallow {
            true => static_authority_signatures(resolved).ok(),
            false => None,
        };
        estimator.assemble(plan, vector, static_signatures, &rules, schedule)
    })
}

/// The estimate of an offered term: it parses the term, resolves its names
/// with the funding resolver, estimates the resolved term, and drops both
/// terms without recursion.
pub fn estimate_offered_term(
    term: &str,
    normalizer_env: HashMap<String, Par>,
    rand: Blake2b512Random,
    funding: &Sig,
    schedule: &PhloScheduleV1<'_>,
    evaluator: &ReducerCore,
) -> Result<CostEstimate, InterpreterError> {
    let parsed = InterpreterImpl::parse_source(term, normalizer_env)?;
    estimate_offered_par(parsed, rand, funding, schedule, evaluator)
}

pub fn estimate_offered_prepared(
    prepared: crate::rust::interpreter::frontend::PreparedProgram,
    rand: Blake2b512Random,
    funding: &Sig,
    schedule: &PhloScheduleV1<'_>,
    evaluator: &ReducerCore,
) -> Result<CostEstimate, InterpreterError> {
    estimate_offered_par(prepared.into_par(), rand, funding, schedule, evaluator)
}

fn estimate_offered_par(
    parsed: Par,
    rand: Blake2b512Random,
    funding: &Sig,
    schedule: &PhloScheduleV1<'_>,
    evaluator: &ReducerCore,
) -> Result<CostEstimate, InterpreterError> {
    let resolved = match resolve_lexical_names_for_funding(&parsed, rand, &evaluator.urn_map) {
        Ok(resolved) => resolved,
        Err(error) => {
            dismantle_term(parsed);
            return Err(error);
        }
    };
    let estimate = linear_cost_estimate(&resolved, funding, schedule, evaluator);
    dismantle_term(parsed);
    dismantle_term(resolved);
    estimate
}

/// A copy of `core` whose metering and substitution meters are one fresh
/// unlimited machine (arbitration M1).
fn isolated_evaluator(core: &ReducerCore) -> ReducerCore {
    let metering = MeteredMachine::new(RuntimeBudget::new(Cost::unsafe_max()));
    ReducerCore {
        metering: metering.clone(),
        substitute: Substitute { metering },
        residue_seal: false,
        ..core.clone()
    }
}

fn byte_count(len: usize) -> u64 { u64::try_from(len).unwrap_or(u64::MAX) }

fn is_closed(locally_free: &[u8]) -> bool { locally_free.iter().all(|free| *free == 0) }

// ── The metering pre-pass ──────────────────────────────────────────────────

/// The metrics of one subterm: its message height, an upper bound of its
/// prost length, the number of variables that a substitution can replace, and
/// the number of environment copies that the reducer's substitution makes.
/// `Env::shift` copies the whole environment for each receive body and
/// condition, each `new` body, and each match case source and guard
/// (`substitute.rs:686`, `:690`, `:728`, `:769`, `:782`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Metrics {
    pub(crate) height: u32,
    pub(crate) weight: u64,
    pub(crate) vars: u64,
    pub(crate) shifts: u64,
}

impl Metrics {
    fn absorb(&mut self, offset: u32, child: Metrics) {
        self.height = self.height.max(offset.saturating_add(child.height));
        self.weight = self.weight.saturating_add(child.weight);
        self.vars = self.vars.saturating_add(child.vars);
        self.shifts = self.shifts.saturating_add(child.shifts);
    }
}

/// The message types that the walks visit as subterms: `Par` and
/// `CostSignature`. Between two of them lie at most three other messages,
/// apart from the parallel tails of a token stack.
///
/// Changed by the merge of feature/cost-accounted-rho (finding R1-18): the
/// old first sentence was "The two message types that nest without a bound:
/// `Par` and `CostSignature`." That is false for F1R3Lang, whose `CostStack`
/// nests without a bound through its parallel tails. `par_parts` walks the
/// tail tree of each token stack with its own heap stack. It gives each cell
/// of a tail at depth `k` the offset `2 + k`, so `1 + k` stack messages lie
/// between the `Par` and the cell.
///
/// Changed by the merge of feature/cost-accounted-rho (finding R1-16): a
/// `Member` is a `Par` that a pathmap holds. A walk decodes it from its trie
/// key. Its member nesting is one more than the nesting of its owner. A map
/// key or value of a zipper's pathmap lies four messages below its owner, as
/// an `EMap` entry inside an `EZipper` would lie.
#[derive(Clone, Copy)]
enum Subterm<'a> {
    Par(&'a Par),
    Signature(&'a CostSignature),
    Member(&'a Par),
}

impl Subterm<'_> {
    /// Added by the same fix: the member nesting of this subterm as a child of
    /// a subterm at `nesting`.
    fn nesting_below(self, nesting: u32) -> u32 {
        match self {
            Subterm::Member(_) => nesting.saturating_add(1),
            Subterm::Par(_) | Subterm::Signature(_) => nesting,
        }
    }
}

/// The metrics of every `Par` and `CostSignature` of a term, keyed by
/// address. The term stays borrowed for the whole analysis, so each address
/// names one subterm. Changed by the merge of feature/cost-accounted-rho
/// (finding R1-16): the decoded pathmap members stay in the arena of their
/// walk for the same time, so their addresses are not used again.
#[derive(Default)]
pub(crate) struct Measures {
    pars: HashMap<usize, Metrics>,
    signatures: HashMap<usize, Metrics>,
}

impl Measures {
    pub(crate) fn par(&self, par: &Par) -> Option<Metrics> {
        self.pars.get(&(par as *const Par as usize)).copied()
    }

    fn signature(&self, signature: &CostSignature) -> Option<Metrics> {
        self.signatures
            .get(&(signature as *const CostSignature as usize))
            .copied()
    }

    fn get(&self, subterm: Subterm<'_>) -> Metrics {
        // Changed by the merge of feature/cost-accounted-rho (finding R1-16):
        // a member is a `Par`.
        //     Subterm::Par(par) => self.par(par),
        match subterm {
            Subterm::Par(par) | Subterm::Member(par) => self.par(par),
            Subterm::Signature(signature) => self.signature(signature),
        }
        .unwrap_or_default()
    }

    fn insert(&mut self, subterm: Subterm<'_>, metrics: Metrics) {
        // Changed by the merge of feature/cost-accounted-rho (finding R1-16):
        // a member is a `Par`.
        //     Subterm::Par(par) => self.pars.insert(par as *const Par as usize, metrics),
        match subterm {
            Subterm::Par(par) | Subterm::Member(par) => {
                self.pars.insert(par as *const Par as usize, metrics)
            }
            Subterm::Signature(signature) => self
                .signatures
                .insert(signature as *const CostSignature as usize, metrics),
        };
    }
}

/// Added by the merge of feature/cost-accounted-rho (finding R1-16): the
/// decoded members of the pathmaps that one walk visits. Theirs' `EPathMap`
/// holds its members in the vector `ps`. F1R3Lang's holds them as canonical
/// trie keys, and in map mode it stores one value with each key. A walk
/// decodes each key once into `arena`. The arena outlives every address that
/// the walk records. The cache gives the exit step of the pre-pass the
/// members of its enter step, at the same addresses.
pub(crate) struct Members<'a> {
    arena: &'a Arena<Par>,
    cache: HashMap<usize, Vec<(u32, &'a Par)>>,
}

impl<'a> Members<'a> {
    pub(crate) fn new(arena: &'a Arena<Par>) -> Self {
        Self {
            arena,
            cache: HashMap::new(),
        }
    }

    /// Pushes the members of `pathmap` to `children`. Set members lie at
    /// `offset`. Map keys and values lie one message deeper, where theirs'
    /// `EMap` entries lie. `nesting` is the member nesting of the pathmap's
    /// owner. The result is `false` when the members would lie at
    /// `MEMBER_NESTING_LIMIT`. The walk then decodes and pushes nothing.
    fn push(
        &mut self,
        offset: u32,
        pathmap: &'a EPathMap,
        nesting: u32,
        children: &mut Vec<(u32, Subterm<'a>)>,
    ) -> Result<bool, InterpreterError> {
        let key = pathmap as *const EPathMap as usize;
        if let Some(members) = self.cache.get(&key) {
            children.extend(
                members
                    .iter()
                    .map(|&(offset, member)| (offset, Subterm::Member(member))),
            );
            return Ok(true);
        }
        let mode = pathmap.mode();
        match (mode, nesting.saturating_add(1) < MEMBER_NESTING_LIMIT) {
            (EPathMapMode::Empty, _) => return Ok(true),
            (EPathMapMode::Set | EPathMapMode::Map, false) => return Ok(false),
            (EPathMapMode::Set | EPathMapMode::Map, true) => {}
        }
        let arena = self.arena;
        let entry_offset = offset.saturating_add(1);
        let mut members: Vec<(u32, &'a Par)> = Vec::with_capacity(match mode {
            EPathMapMode::Map => pathmap.len().saturating_mul(2),
            EPathMapMode::Set | EPathMapMode::Empty => pathmap.len(),
        });
        // A key that does not decode stops the decoding. The visitors cannot
        // stop early, so they skip the remaining keys.
        let mut failure = None;
        let visited = match mode {
            EPathMapMode::Set => pathmap.entry_trie().for_each_raw_set_entry(|path| {
                let None = failure else {
                    return;
                };
                match decode_trie_path(path) {
                    Ok(member) => members.push((offset, &*arena.alloc(member))),
                    Err(error) => failure = Some(error),
                }
            }),
            EPathMapMode::Map => pathmap.entry_trie().for_each_raw_map_entry(|path, value| {
                let None = failure else {
                    return;
                };
                match decode_trie_path(path) {
                    Ok(member) => {
                        members.push((entry_offset, &*arena.alloc(member)));
                        members.push((entry_offset, value));
                    }
                    Err(error) => failure = Some(error),
                }
            }),
            EPathMapMode::Empty => Ok(()),
        };
        match (visited, failure) {
            (Ok(()), None) => {}
            (Err(error), _) => {
                return Err(InterpreterError::BugFoundError(format!(
                    "the estimate visited a pathmap in the wrong mode: {error}"
                )))
            }
            (Ok(()), Some(error)) => {
                return Err(InterpreterError::BugFoundError(format!(
                    "the estimate cannot decode a pathmap member: {error}"
                )))
            }
        }
        children.extend(
            members
                .iter()
                .map(|&(offset, member)| (offset, Subterm::Member(member))),
        );
        self.cache.insert(key, members);
        Ok(true)
    }
}

fn push_pars<'a>(
    children: &mut Vec<(u32, Subterm<'a>)>,
    offset: u32,
    pars: impl IntoIterator<Item = &'a Par>,
) {
    children.extend(pars.into_iter().map(|par| (offset, Subterm::Par(par))));
}

/// The metrics of the messages of `par` up to its nested `Par` and
/// `CostSignature` subterms, which go to `children` with their message
/// offsets.
///
/// Changed by the merge of feature/cost-accounted-rho (finding R1-16): the
/// members of each pathmap also go to `children`, decoded by `members`.
/// `nesting` is the member nesting of `par`.
// fn par_parts<'a>(par: &'a Par, children: &mut Vec<(u32, Subterm<'a>)>) -> Metrics {
fn par_parts<'a>(
    par: &'a Par,
    nesting: u32,
    members: &mut Members<'a>,
    children: &mut Vec<(u32, Subterm<'a>)>,
) -> Result<Metrics, InterpreterError> {
    let mut own = Metrics {
        height: 1,
        weight: NODE_BYTES.saturating_add(byte_count(par.locally_free.len())),
        vars: 0,
        shifts: 0,
    };
    let mut leaf = false;
    let add = |own: &mut Metrics, bytes: usize| {
        own.weight = own
            .weight
            .saturating_add(NODE_BYTES)
            .saturating_add(byte_count(bytes));
    };
    for send in &par.sends {
        add(&mut own, send.locally_free.len());
        push_pars(children, 2, send.chan.iter().chain(send.data.iter()));
    }
    for receive in &par.receives {
        add(&mut own, receive.locally_free.len());
        for bind in &receive.binds {
            add(&mut own, 0);
            push_pars(children, 3, bind.patterns.iter().chain(bind.source.iter()));
            children.extend(
                bind.cost_signature
                    .iter()
                    .map(|signature| (3, Subterm::Signature(signature))),
            );
            if bind.remainder.is_some() {
                own.vars = own.vars.saturating_add(1);
                leaf = true;
            }
        }
        push_pars(
            children,
            2,
            receive.body.iter().chain(receive.condition.iter()),
        );
        own.shifts = own
            .shifts
            .saturating_add(1)
            .saturating_add(u64::from(receive.condition.is_some()));
    }
    for new in &par.news {
        let names = new.uri.iter().map(String::len).sum::<usize>()
            + new.injections.keys().map(String::len).sum::<usize>();
        add(&mut own, new.locally_free.len().saturating_add(names));
        own.shifts = own.shifts.saturating_add(1);
        for _ in &new.injections {
            add(&mut own, 0);
        }
        push_pars(children, 2, new.p.iter());
        push_pars(children, 3, new.injections.values());
    }
    // Added by the merge of feature/cost-accounted-rho (finding R1-16).
    let mut capped = false;
    for expr in &par.exprs {
        add(&mut own, 0);
        leaf = true;
        // Changed by the same fix: an expression also pushes its pathmap
        // members.
        // expr_parts(expr, &mut own, children);
        capped |= expr_parts(expr, nesting, &mut own, members, children)?;
    }
    for matched in &par.matches {
        add(&mut own, matched.locally_free.len());
        push_pars(children, 2, matched.target.iter());
        for case in &matched.cases {
            add(&mut own, 0);
            push_pars(
                children,
                3,
                case.pattern
                    .iter()
                    .chain(case.source.iter())
                    .chain(case.guard.iter()),
            );
            if let (Some(_), Some(_)) = (&case.pattern, &case.source) {
                own.shifts = own
                    .shifts
                    .saturating_add(1)
                    .saturating_add(u64::from(case.guard.is_some()));
            }
        }
    }
    for unforgeable in &par.unforgeables {
        leaf = true;
        let bytes = match unforgeable.unf_instance.as_ref() {
            Some(UnfInstance::GPrivateBody(private)) => private.id.len(),
            Some(UnfInstance::GDeployIdBody(deploy)) => deploy.sig.len(),
            Some(UnfInstance::GDeployerIdBody(deployer)) => deployer.public_key.len(),
            Some(_) | None => NODE_BYTES as usize,
        };
        add(&mut own, bytes);
    }
    for bundle in &par.bundles {
        add(&mut own, 0);
        push_pars(children, 2, bundle.body.iter());
    }
    for connective in &par.connectives {
        add(&mut own, 0);
        leaf = true;
        match connective.connective_instance.as_ref() {
            Some(ConnectiveInstance::ConnAndBody(body))
            | Some(ConnectiveInstance::ConnOrBody(body)) => push_pars(children, 3, body.ps.iter()),
            Some(ConnectiveInstance::ConnNotBody(body)) => push_pars(children, 2, [body]),
            Some(ConnectiveInstance::VarRefBody(_)) => own.vars = own.vars.saturating_add(1),
            Some(_) | None => {}
        }
    }
    for conditional in &par.conditionals {
        add(&mut own, conditional.locally_free.len());
        push_pars(
            children,
            2,
            conditional
                .condition
                .iter()
                .chain(conditional.if_true.iter())
                .chain(conditional.if_false.iter()),
        );
    }
    for signed in &par.cost_signed_terms {
        add(&mut own, 0);
        push_pars(children, 2, signed.body.iter());
        children.extend(
            signed
                .signature
                .iter()
                .map(|signature| (2, Subterm::Signature(signature))),
        );
    }
    // Changed by the merge of feature/cost-accounted-rho (finding R1-18):
    // F1R3Lang's token stack is a tree. A tail at depth `k` lies `1 + k`
    // messages below `par`, and its cells lie `2 + k` messages below it. The
    // tail walk keeps its own heap stack. A stack without tails gives the
    // metrics of the old loop.
    // for stack in &par.cost_stacks {
    //     add(&mut own, 0);
    //     children.extend(stack.cells.iter().map(|cell| (2, Subterm::Signature(cell))));
    // }
    let mut tail_height = 0;
    let mut tails = Vec::new();
    for stack in &par.cost_stacks {
        tails.push((stack, 0u32));
        while let Some((node, depth)) = tails.pop() {
            add(&mut own, 0);
            if depth > 0 {
                tail_height = tail_height.max(depth.saturating_add(2));
            }
            let offset = depth.saturating_add(2);
            children.extend(
                node.cells
                    .iter()
                    .map(|cell| (offset, Subterm::Signature(cell))),
            );
            let below = depth.saturating_add(1);
            tails.extend(node.parallel_tail.iter().rev().map(|tail| (tail, below)));
        }
    }
    if leaf {
        own.height = PAR_LEAF_HEIGHT;
    }
    // Added by the merge of feature/cost-accounted-rho (finding R1-16): a
    // pathmap whose members lie at `MEMBER_NESTING_LIMIT` makes its owner
    // taller than the sizing bound. It also counts as a variable, so that no
    // reader of the metrics treats the undecoded members as closed.
    if capped {
        own.height = UNSIZED_HEIGHT;
        own.vars = own.vars.saturating_add(1);
    }
    // Added by the merge of feature/cost-accounted-rho (finding R1-18): a
    // tail at depth `k` makes `par` at least `2 + k` messages tall, also when
    // the tail holds no cell.
    own.height = own.height.max(tail_height);
    // own
    Ok(own)
}

/// The operand slots of one expression in field order, each with its message
/// offset: 3 for an operand, and 4 for a map entry or a zipper element, which
/// sit one message deeper. An empty optional slot is `None`. The pre-pass,
/// the freshness check, the evaluation bound and the teardown read the forms
/// through exhaustive matches, so a new form reaches all of them (review
/// cleanup).
fn expr_operands(expr: &Expr) -> Vec<(u32, Option<&Par>)> {
    fn pair<'a>(left: &'a Option<Par>, right: &'a Option<Par>) -> Vec<(u32, Option<&'a Par>)> {
        vec![(3, left.as_ref()), (3, right.as_ref())]
    }
    fn each(offset: u32, pars: &[Par]) -> Vec<(u32, Option<&Par>)> {
        pars.iter().map(|par| (offset, Some(par))).collect()
    }
    let Some(instance) = expr.expr_instance.as_ref() else {
        return Vec::new();
    };
    match instance {
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
        | ExprInstance::GUint8(_)
        | ExprInstance::EVarBody(_) => Vec::new(),
        ExprInstance::ENotBody(body) => vec![(3, body.p.as_ref())],
        ExprInstance::ENegBody(body) => vec![(3, body.p.as_ref())],
        ExprInstance::EMultBody(body) => pair(&body.p1, &body.p2),
        ExprInstance::EDivBody(body) => pair(&body.p1, &body.p2),
        ExprInstance::EModBody(body) => pair(&body.p1, &body.p2),
        ExprInstance::EPlusBody(body) => pair(&body.p1, &body.p2),
        ExprInstance::EMinusBody(body) => pair(&body.p1, &body.p2),
        ExprInstance::ELtBody(body) => pair(&body.p1, &body.p2),
        ExprInstance::ELteBody(body) => pair(&body.p1, &body.p2),
        ExprInstance::EGtBody(body) => pair(&body.p1, &body.p2),
        ExprInstance::EGteBody(body) => pair(&body.p1, &body.p2),
        ExprInstance::EEqBody(body) => pair(&body.p1, &body.p2),
        ExprInstance::ENeqBody(body) => pair(&body.p1, &body.p2),
        ExprInstance::EAndBody(body) => pair(&body.p1, &body.p2),
        ExprInstance::EOrBody(body) => pair(&body.p1, &body.p2),
        ExprInstance::EPercentPercentBody(body) => pair(&body.p1, &body.p2),
        ExprInstance::EPlusPlusBody(body) => pair(&body.p1, &body.p2),
        ExprInstance::EMinusMinusBody(body) => pair(&body.p1, &body.p2),
        ExprInstance::EMatchesBody(body) => pair(&body.target, &body.pattern),
        ExprInstance::EListBody(list) => each(3, &list.ps),
        ExprInstance::ETupleBody(tuple) => each(3, &tuple.ps),
        ExprInstance::ESetBody(set) => each(3, &set.ps),
        ExprInstance::EMapBody(map) => {
            let mut slots = Vec::with_capacity(map.kvs.len().saturating_mul(2));
            for entry in &map.kvs {
                slots.push((4, entry.key.as_ref()));
                slots.push((4, entry.value.as_ref()));
            }
            slots
        }
        ExprInstance::EMethodBody(method) => {
            let mut slots = Vec::with_capacity(method.arguments.len().saturating_add(1));
            slots.push((3, method.target.as_ref()));
            slots.extend(method.arguments.iter().map(|argument| (3, Some(argument))));
            slots
        }
        // Changed by the merge of feature/cost-accounted-rho (finding R1-16):
        // F1R3Lang's `EPathMap` has no `ps` vector. Its members are trie keys,
        // which `expr_parts` decodes through `Members`. The freshness check
        // reads the pathmap's `locally_free`, and the evaluation bound has no
        // known work limit for a pathmap, so neither reads these slots.
        // ExprInstance::EPathmapBody(pathmap) => each(3, &pathmap.ps),
        // ExprInstance::EZipperBody(zipper) => zipper
        //     .pathmap
        //     .as_ref()
        //     .map_or_else(Vec::new, |pathmap| each(4, &pathmap.ps)),
        ExprInstance::EPathmapBody(_) | ExprInstance::EZipperBody(_) => Vec::new(),
    }
}

/// The metrics of one expression up to its operands, which go to `children`
/// from the shared operand slots.
///
/// Changed by the merge of feature/cost-accounted-rho (finding R1-16): the
/// members of a pathmap go to `children` from `members`. The result is `true`
/// when the members lie at `MEMBER_NESTING_LIMIT` and stay undecoded.
// fn expr_parts<'a>(expr: &'a Expr, own: &mut Metrics, children: &mut Vec<(u32, Subterm<'a>)>) {
fn expr_parts<'a>(
    expr: &'a Expr,
    nesting: u32,
    own: &mut Metrics,
    members: &mut Members<'a>,
    children: &mut Vec<(u32, Subterm<'a>)>,
) -> Result<bool, InterpreterError> {
    let add = |own: &mut Metrics, bytes: usize| {
        own.weight = own.weight.saturating_add(byte_count(bytes));
    };
    let open = |own: &mut Metrics, remainder: bool| {
        own.vars = own.vars.saturating_add(u64::from(remainder));
    };
    children.extend(
        expr_operands(expr)
            .into_iter()
            .filter_map(|(offset, par)| par.map(|par| (offset, Subterm::Par(par)))),
    );
    let Some(instance) = expr.expr_instance.as_ref() else {
        // return;
        return Ok(false);
    };
    let mut capped = false;
    match instance {
        ExprInstance::GString(text) | ExprInstance::GUri(text) => add(own, text.len()),
        ExprInstance::GByteArray(bytes) | ExprInstance::GBigInt(bytes) => add(own, bytes.len()),
        ExprInstance::GBigRat(rational) => add(
            own,
            rational.numerator.len() + rational.denominator.len() + NODE_BYTES as usize,
        ),
        ExprInstance::GFixedPoint(fixed) => add(own, fixed.unscaled.len() + NODE_BYTES as usize),
        ExprInstance::EVarBody(_) => open(own, true),
        ExprInstance::EListBody(list) => {
            add(own, list.locally_free.len());
            open(own, list.remainder.is_some());
        }
        ExprInstance::ETupleBody(tuple) => add(own, tuple.locally_free.len()),
        ExprInstance::ESetBody(set) => {
            add(own, set.locally_free.len());
            open(own, set.remainder.is_some());
        }
        ExprInstance::EMapBody(map) => {
            add(own, map.locally_free.len());
            add(own, map.kvs.len().saturating_mul(NODE_BYTES as usize));
            open(own, map.remainder.is_some());
        }
        ExprInstance::EMethodBody(method) => {
            add(own, method.method_name.len() + method.locally_free.len())
        }
        // Changed by the merge of feature/cost-accounted-rho (finding R1-16):
        // the sum of the member bounds does not bound the length of the EPM1
        // trie snapshot, so the pathmap adds its exact prost length, which
        // includes `locally_free`. The members follow as children.
        ExprInstance::EPathmapBody(pathmap) => {
            // add(own, pathmap.locally_free.len());
            add(own, pathmap.encoded_len());
            open(own, pathmap.remainder.is_some());
            capped |= !members.push(3, pathmap, nesting, children)?;
        }
        ExprInstance::EZipperBody(zipper) => {
            add(
                own,
                zipper.current_path.iter().map(Vec::len).sum::<usize>()
                    + zipper.locally_free.len()
                    + NODE_BYTES as usize,
            );
            if let Some(pathmap) = zipper.pathmap.as_ref() {
                // add(own, pathmap.locally_free.len());
                add(own, pathmap.encoded_len());
                open(own, pathmap.remainder.is_some());
                capped |= !members.push(4, pathmap, nesting, children)?;
            }
        }
        ExprInstance::GBool(_)
        | ExprInstance::GInt(_)
        | ExprInstance::GDouble(_)
        | ExprInstance::GUint64(_)
        | ExprInstance::GFloat32(_)
        | ExprInstance::GInt32(_)
        | ExprInstance::GUint32(_)
        | ExprInstance::GUint16(_)
        | ExprInstance::GUint8(_)
        | ExprInstance::ENotBody(_)
        | ExprInstance::ENegBody(_)
        | ExprInstance::EMultBody(_)
        | ExprInstance::EDivBody(_)
        | ExprInstance::EModBody(_)
        | ExprInstance::EPlusBody(_)
        | ExprInstance::EMinusBody(_)
        | ExprInstance::ELtBody(_)
        | ExprInstance::ELteBody(_)
        | ExprInstance::EGtBody(_)
        | ExprInstance::EGteBody(_)
        | ExprInstance::EEqBody(_)
        | ExprInstance::ENeqBody(_)
        | ExprInstance::EAndBody(_)
        | ExprInstance::EOrBody(_)
        | ExprInstance::EPercentPercentBody(_)
        | ExprInstance::EPlusPlusBody(_)
        | ExprInstance::EMinusMinusBody(_)
        | ExprInstance::EMatchesBody(_) => {}
    }
    Ok(capped)
}

fn signature_parts<'a>(
    signature: &'a CostSignature,
    children: &mut Vec<(u32, Subterm<'a>)>,
) -> Metrics {
    let mut own = Metrics {
        height: SIGNATURE_LEAF_HEIGHT,
        weight: NODE_BYTES,
        vars: 0,
        shifts: 0,
    };
    match signature.value.as_ref() {
        Some(CostSignatureValue::Ground(bytes)) => {
            own.weight = own.weight.saturating_add(byte_count(bytes.len()))
        }
        Some(CostSignatureValue::Quote(par)) | Some(CostSignatureValue::Name(par)) => {
            children.push((1, Subterm::Par(par)))
        }
        Some(CostSignatureValue::Compound(compound)) => {
            for element in &compound.elements {
                own.weight = own.weight.saturating_add(NODE_BYTES);
                children.push((2, Subterm::Signature(element)));
            }
        }
        Some(CostSignatureValue::BoundLevel(_)) => own.vars = 1,
        Some(CostSignatureValue::Unit(_)) | None => {}
    }
    own
}

// Changed by the merge of feature/cost-accounted-rho (finding R1-16): the
// parts of a `Par` include its decoded pathmap members.
// fn subterm_parts<'a>(subterm: Subterm<'a>, children: &mut Vec<(u32, Subterm<'a>)>) -> Metrics {
//     match subterm {
//         Subterm::Par(par) => par_parts(par, children),
//         Subterm::Signature(signature) => signature_parts(signature, children),
//     }
// }
fn subterm_parts<'a>(
    subterm: Subterm<'a>,
    nesting: u32,
    members: &mut Members<'a>,
    children: &mut Vec<(u32, Subterm<'a>)>,
) -> Result<Metrics, InterpreterError> {
    match subterm {
        Subterm::Par(par) | Subterm::Member(par) => par_parts(par, nesting, members, children),
        Subterm::Signature(signature) => Ok(signature_parts(signature, children)),
    }
}

/// Measures `root` and every nested subterm once, bottom-up, and records
/// each result in `measures`.
///
/// Changed by the merge of feature/cost-accounted-rho (finding R1-16): each
/// step carries the member nesting of its subterm, and `members` decodes the
/// pathmap members.
fn measure_subterm<'a>(
    root: Subterm<'a>,
    measures: &mut Measures,
    members: &mut Members<'a>,
) -> Result<Metrics, InterpreterError> {
    enum Step<'a> {
        Enter(Subterm<'a>, u32),
        Exit(Subterm<'a>, u32),
    }
    let mut work = vec![Step::Enter(root, 0)];
    let mut children = Vec::new();
    while let Some(step) = work.pop() {
        // Changed by the G1-4a fix (review LOW 3): each step scans the whole
        // width of its node, so it charges that width.
        // reserve(host, HostWorkDimension::StructuralItems, 1)?;
        match step {
            Step::Enter(subterm, nesting) => {
                work.push(Step::Exit(subterm, nesting));
                children.clear();
                subterm_parts(subterm, nesting, members, &mut children)?;
                work.extend(
                    children
                        .drain(..)
                        .map(|(_, child)| Step::Enter(child, child.nesting_below(nesting))),
                );
            }
            Step::Exit(subterm, nesting) => {
                children.clear();
                let mut metrics = subterm_parts(subterm, nesting, members, &mut children)?;
                for (offset, child) in children.drain(..) {
                    metrics.absorb(offset, measures.get(child));
                }
                measures.insert(subterm, metrics);
            }
        }
    }
    Ok(measures.get(root))
}

/// Added by the G1-4a fix (review H-1): whether a signature still holds a
/// variable, anywhere inside it and inside the processes of its `Quote` and
/// `Name` atoms. A variable is an `EVar`, a remainder, a variable reference
/// or a `BoundLevel`, as `subterm_parts` counts them. Substitution replaces
/// such a variable at run time, so the lane of the signature is not static.
/// The scan uses a heap work stack and stops at the first variable.
///
/// Changed by the merge of feature/cost-accounted-rho (finding R1-16): the
/// scan also visits the decoded members of each pathmap. A member that does
/// not decode, or members at `MEMBER_NESTING_LIMIT`, count as a variable.
pub(crate) fn signature_holds_variable(signature: &CostSignature) -> bool {
    let arena = Arena::new();
    let mut members = Members::new(&arena);
    let mut work = vec![(Subterm::Signature(signature), 0)];
    let mut children = Vec::new();
    while let Some((subterm, nesting)) = work.pop() {
        children.clear();
        // if subterm_parts(subterm, &mut children).vars > 0 {
        //     return true;
        // }
        match subterm_parts(subterm, nesting, &mut members, &mut children) {
            Ok(Metrics { vars: 0, .. }) => {}
            Ok(_) | Err(_) => return true,
        }
        // work.extend(children.drain(..).map(|(_, child)| child));
        work.extend(
            children
                .drain(..)
                .map(|(_, child)| (child, child.nesting_below(nesting))),
        );
    }
    false
}

/// Which marker a sized value contains: a received or matched value
/// (poison), or a value that the analysis does not know.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Marker {
    Poison,
    Unknown,
}

fn marker_of(unforgeable: &GUnforgeable) -> Option<Marker> {
    match unforgeable.unf_instance.as_ref() {
        Some(UnfInstance::GPrivateBody(GPrivate { id })) => match id.len() {
            POISON_ID_BYTES => Some(Marker::Poison),
            UNKNOWN_ID_BYTES => Some(Marker::Unknown),
            _ => None,
        },
        Some(_) | None => None,
    }
}

fn private_name(id: Vec<u8>) -> Par {
    Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id })),
    }])
}

/// The first marker in `root`, found without recursion.
///
/// Changed by the merge of feature/cost-accounted-rho (finding R1-16): the
/// scan also visits the decoded members of each pathmap. Members at
/// `MEMBER_NESTING_LIMIT` stay undecoded, so the scan gives `Unknown` there.
fn find_marker(root: &Par) -> Result<Option<Marker>, InterpreterError> {
    let arena = Arena::new();
    let mut members = Members::new(&arena);
    let mut work = vec![(Subterm::Par(root), 0)];
    let mut children = Vec::new();
    while let Some((subterm, nesting)) = work.pop() {
        // Changed by the G1-4a fix (review LOW 3): the step charges the width
        // that it scans.
        // reserve(host, HostWorkDimension::StructuralItems, 1)?;
        children.clear();
        // subterm_parts(subterm, &mut children);
        // if let Subterm::Par(par) = subterm {
        let own = subterm_parts(subterm, nesting, &mut members, &mut children)?;
        if own.height > SIZING_HEIGHT_LIMIT {
            return Ok(Some(Marker::Unknown));
        }
        if let Subterm::Par(par) | Subterm::Member(par) = subterm {
            if let Some(marker) = par.unforgeables.iter().find_map(marker_of) {
                return Ok(Some(marker));
            }
        }
        // work.extend(children.drain(..).map(|(_, child)| child));
        work.extend(
            children
                .drain(..)
                .map(|(_, child)| (child, child.nesting_below(nesting))),
        );
    }
    Ok(None)
}

/// The binder bound of pass 1, counted by one walk over the positions that
/// the plan visits (arbitration H1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct WorkCounts {
    /// Added by the second G1-4a fix (re-review HIGH-1): an upper bound of the
    /// binders that pass 1 holds at once. The walk visits every position of
    /// pass 1 and adds the binders of each `new`, receive body and match case
    /// as pass 1 does, so the bound is at least the peak of pass 1.
    pub(crate) peak_binders: usize,
}

fn count_plan_work(root: &Par) -> WorkCounts {
    let mut counts = WorkCounts::default();
    let mut work = vec![(root, 0u64, true, 0usize)];
    let mut children = Vec::new();
    while let Some((par, depth, execution, binders)) = work.pop() {
        children.clear();
        plan_children(par, depth, execution, &mut children);
        counts.peak_binders = counts.peak_binders.max(binders);
        for new in &par.news {
            counts.peak_binders = counts
                .peak_binders
                .max(binders.saturating_add(new_levels(new)));
        }
        work.extend(
            children
                .drain(..)
                .map(|(child, child_depth, child_execution, levels)| {
                    (
                        child,
                        child_depth,
                        child_execution,
                        binders.saturating_add(levels),
                    )
                }),
        );
    }
    counts
}

/// The children that `signed_demand_par` visits, with the signed depth and
/// the execution flag that it gives each of them. Changed by the second
/// G1-4a fix (re-review HIGH-1): each child also carries the binders that
/// pass 1 pushes before it visits the child. A receive body adds the bind
/// count, a `new` body its levels and a match case its free count.
fn plan_children<'a>(
    par: &'a Par,
    depth: u64,
    execution: bool,
    out: &mut Vec<(&'a Par, u64, bool, usize)>,
) {
    let count = |value: i32| usize::try_from(value).unwrap_or(0);
    for signed in &par.cost_signed_terms {
        out.extend(
            signed
                .body
                .iter()
                .map(|body| (body, depth.saturating_add(1), true, 0)),
        );
    }
    for send in &par.sends {
        out.extend(send.data.iter().map(|datum| (datum, 0, false, 0)));
    }
    for receive in &par.receives {
        out.extend(
            receive
                .body
                .iter()
                .map(|body| (body, 0, true, count(receive.bind_count))),
        );
    }
    for new in &par.news {
        out.extend(
            new.p
                .iter()
                .map(|body| (body, depth, execution, new_levels(new))),
        );
    }
    for matched in &par.matches {
        out.extend(matched.cases.iter().filter_map(|case| {
            case.source
                .as_ref()
                .map(|source| (source, depth, execution, count(case.free_count)))
        }));
    }
    for conditional in &par.conditionals {
        out.extend(
            conditional
                .if_true
                .iter()
                .chain(conditional.if_false.iter())
                .map(|branch| (branch, depth, execution, 0)),
        );
    }
    for bundle in &par.bundles {
        out.extend(bundle.body.iter().map(|body| (body, depth, execution, 0)));
    }
}

/// Added by the second G1-4a fix (re-review HIGH-1): the levels that
/// `push_new` binds for a `new`, the simple names and one level for each URI.
fn new_levels(new: &New) -> usize {
    let count = usize::try_from(new.bind_count).unwrap_or(0);
    count.max(new.uri.len())
}

// ── Pass 1: binder kinds, rules and sizes ──────────────────────────────────

/// The kind of the binder of one de Bruijn level (design §5.2). The sizing
/// environment holds a fresh name for the fresh kinds, the bound value for a
/// URN or an injection, the poison marker for a pattern level, and the
/// unknown marker for a URI that neither the URN map nor the injections bind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BinderKind {
    Fresh,
    FreshHole,
    Urn,
    Injection,
    ReceivePattern,
    MatchPattern,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Binder {
    kind: BinderKind,
    /// The index of a fresh level in the channel tables.
    serial: usize,
    /// The length bound of the value of the level in the sizing environment.
    weight: u64,
}

/// The lane of a region: a static signature, the unit signature, which pays
/// nothing, or a signature that the analysis cannot resolve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Lane {
    Static(SigKey),
    Unit,
    Dynamic,
}

/// One signed scope. The arena keeps one entry per signed term of an
/// execution position, linked to its enclosing scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Scope {
    parent: Option<usize>,
    lane: Lane,
    /// True when this scope or an enclosing one is dynamic.
    dynamic: bool,
    /// The prost length of the region fields of this scope and every
    /// enclosing scope inside a `CostAuthority`: the running total of the seal
    /// (arbitration H1). `None` when a region has no static length.
    seal_bytes: Option<u64>,
}

/// One send or receive at an execution position, with what pass 2 needs.
/// An absent size marks a participant that the analysis cannot size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Participant {
    Send {
        scope: Option<usize>,
        channel: Option<usize>,
        message: Option<u64>,
        introduction: Option<u64>,
    },
    Receive {
        scope: Option<usize>,
        channels: Vec<Option<usize>>,
        binds: Vec<Lane>,
        introduction: Option<u64>,
    },
}

/// One instruction of the post-order program of the vector fold, the
/// instruction set of `StackSafeDemandWalk.v`. The finish of a signed body
/// is a combine, because the vector fold adds no count for an empty body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Instruction {
    Push,
    Local(usize),
    Combine,
    Alternative,
}

/// The channel tables `M`, `T` and `TR` of design §5.5, indexed by fresh
/// serial.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Tables {
    pub(crate) message: Vec<u64>,
    pub(crate) transfer: Vec<u64>,
    pub(crate) trace: Vec<u64>,
}

/// The raw counts of one subterm: compute, introduction, transfer and trace
/// for each lane.
pub(crate) type Vector = HashMap<SigKey, [u64; 4]>;

/// What a sizing step returns: a size, or the rule that leaves the value
/// without a static size.
type Sizing<T> = Result<Result<T, IncompleteReason>, InterpreterError>;

/// A class of values that the evaluation pre-walk tracks (arbitration H2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ValueClass {
    Bool,
    Int(u8),
    Float(u8),
    Str(u64),
    Bytes(u64),
    List,
    Other,
}

#[derive(Clone, Copy, Debug)]
struct Evaluated {
    class: ValueClass,
    growth: u64,
}

/// The output bound of one evaluation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EvaluationBound {
    /// The length bound of the evaluated value.
    pub(crate) output: u64,
}

pub(crate) struct Estimator<'e> {
    core: &'e ReducerCore,
    pub(crate) measures: Measures,
    /// Added by the merge of feature/cost-accounted-rho (finding R1-16): the
    /// pathmap members that the pre-pass decodes. They live as long as
    /// `measures`, which keys their metrics by address.
    decoded: Arena<Par>,
    deploy_key: SigKey,
    deploy_signature: Option<CostSignature>,
    /// The field length of the deploy region inside a `CostAuthority`.
    deploy_seal: Option<u64>,
    binders: Vec<Binder>,
    env: Env<Par>,
    /// Added by the second G1-4a fix (re-review HIGH-1): the peak binder count
    /// of the pre-pass, from which `size_environment` sizes the environment
    /// map once.
    peak_binders: usize,
    /// The largest height and length of a value in the sizing environment.
    env_height: u32,
    env_weight: u64,
    /// The sum of the length bounds of the values in the sizing environment,
    /// which each `Env::shift` of a substitution copies.
    env_bytes: u64,
    /// The length bounds of a fresh name and of a poison marker.
    fresh_weight: u64,
    poison_weight: u64,
    fresh: usize,
    scopes: Vec<Scope>,
    pub(crate) participants: Vec<Participant>,
    pub(crate) program: Vec<Instruction>,
    pub(crate) lanes: BTreeMap<SigKey, CostSignature>,
    pub(crate) reason: Option<IncompleteReason>,
    #[cfg(test)]
    pub(crate) levels: Vec<(usize, usize)>,
    /// Added by the second G1-4a fix, test only: the most binders that pass 1
    /// held at once.
    #[cfg(test)]
    pub(crate) observed_peak: usize,
}

enum Task<'a> {
    Visit {
        par: &'a Par,
        scope: Option<usize>,
        resolving: bool,
    },
    Send(&'a Send, Option<usize>),
    Receive(&'a Receive, Option<usize>),
    New(&'a New, Option<usize>, bool),
    Match(&'a Match, Option<usize>, bool),
    Case {
        case: &'a MatchCase,
        scope: Option<usize>,
        resolving: bool,
        alternative: bool,
    },
    Conditional(&'a If, Option<usize>, bool),
    Bundle(&'a Bundle, Option<usize>, bool),
    Exprs(&'a Par),
    Signed(&'a CostSignedTerm, Option<usize>, bool),
    Stacks(&'a Par),
    Holes(usize, BinderKind),
    Pop(usize),
    Emit(Instruction),
}

impl<'e> Estimator<'e> {
    pub(crate) fn new(funding: &Sig, core: &'e ReducerCore) -> Result<Self, InterpreterError> {
        let deploy_signature = sig_to_cost_signature(funding)
            .ok()
            .and_then(|signature| canonical_cost_signature(&signature).ok());
        let deploy_seal = match deploy_signature.as_ref() {
            Some(signature) => region_field_bytes(signature, 0),
            None => None,
        };
        let mut estimator = Self {
            core,
            measures: Measures::default(),
            decoded: Arena::new(),
            deploy_key: funding.lane_hash(),
            deploy_signature,
            deploy_seal,
            binders: Vec::new(),
            env: Env::new(),
            peak_binders: 0,
            env_height: 0,
            env_weight: 0,
            env_bytes: 0,
            fresh_weight: 0,
            poison_weight: 0,
            fresh: 0,
            scopes: Vec::new(),
            participants: Vec::new(),
            program: Vec::new(),
            lanes: BTreeMap::new(),
            reason: None,
            #[cfg(test)]
            levels: Vec::new(),
            #[cfg(test)]
            observed_peak: 0,
        };
        estimator.fresh_weight = estimator.note_env_value(&fresh_name(0))?;
        estimator.poison_weight =
            estimator.note_env_value(&private_name(vec![0; POISON_ID_BYTES]))?;
        estimator.note_env_value(&private_name(vec![0; UNKNOWN_ID_BYTES]))?;
        Ok(estimator)
    }

    fn violate(&mut self, reason: IncompleteReason) {
        if self.reason.is_none() {
            self.reason = Some(reason);
        }
    }

    /// The pre-pass: the metrics of every subterm, and the environment sized
    /// from the peak binder count.
    pub(crate) fn measure(&mut self, root: &Par) -> Result<WorkCounts, InterpreterError> {
        // Changed by the merge of feature/cost-accounted-rho (finding R1-16):
        // the pre-pass decodes pathmap members into the estimator's arena.
        // measure_subterm(Subterm::Par(root), &mut self.measures)?;
        measure_subterm(
            Subterm::Par(root),
            &mut self.measures,
            &mut Members::new(&self.decoded),
        )?;
        // Changed by the second G1-4a fix (re-review HIGH-1): the pre-pass also
        // sizes the environment map from the peak binder count.
        let counts = count_plan_work(root);
        self.size_environment(counts.peak_binders)?;
        Ok(counts)
    }

    /// Added by the second G1-4a fix (re-review HIGH-1): sizes the empty
    /// environment map once for the whole walk, with room for twice the peak
    /// binder count. A table that runs out of room then rehashes in place,
    /// because it holds at most half of its capacity, so its bucket count
    /// never changes.
    pub(crate) fn size_environment(&mut self, peak: usize) -> Result<(), InterpreterError> {
        if !self.binders.is_empty() || self.env.level != 0 {
            return Err(InterpreterError::BugFoundError(
                "the estimate sized an environment that holds binders".to_string(),
            ));
        }
        let capacity = peak.saturating_mul(2).saturating_add(2);
        self.env.env_map = HashMap::with_capacity(capacity);
        self.peak_binders = peak;
        Ok(())
    }

    fn note_env_value(&mut self, value: &Par) -> Result<u64, InterpreterError> {
        // Changed by the merge of feature/cost-accounted-rho (finding R1-16):
        // the measurement decodes pathmap members into a local arena.
        // let metrics = measure_subterm(Subterm::Par(value), &mut Measures::default())?;
        let arena = Arena::new();
        let metrics = measure_subterm(
            Subterm::Par(value),
            &mut Measures::default(),
            &mut Members::new(&arena),
        )?;
        self.env_height = self.env_height.max(metrics.height);
        self.env_weight = self.env_weight.max(metrics.weight);
        Ok(metrics.weight)
    }

    fn push_binder(
        &mut self,
        kind: BinderKind,
        value: Par,
        weight: u64,
    ) -> Result<(), InterpreterError> {
        let serial = match kind {
            BinderKind::Fresh | BinderKind::FreshHole => {
                let serial = self.fresh;
                self.fresh += 1;
                serial
            }
            _ => 0,
        };
        // Added by the second G1-4a fix (re-review HIGH-1): the walk stays
        // within the peak that sized the environment map, so the map never
        // grows.
        if self.binders.len() >= self.peak_binders {
            return Err(InterpreterError::BugFoundError(
                "the estimate holds more binders than its pre-pass counted".to_string(),
            ));
        }
        self.binders.push(Binder {
            kind,
            serial,
            weight,
        });
        #[cfg(test)]
        {
            self.observed_peak = self.observed_peak.max(self.binders.len());
        }
        self.env_bytes = self.env_bytes.saturating_add(weight);
        self.env.push(value).ok_or_else(|| {
            InterpreterError::BugFoundError(
                "the estimate cannot bind a value: the environment level overflows".to_string(),
            )
        })
    }

    pub(crate) fn push_holes(
        &mut self,
        count: usize,
        kind: BinderKind,
    ) -> Result<(), InterpreterError> {
        self.binders.reserve(count);
        for _ in 0..count {
            self.push_binder(
                kind,
                private_name(vec![0; POISON_ID_BYTES]),
                self.poison_weight,
            )?;
        }
        Ok(())
    }

    /// Binds the levels of a `new` in the order of `util::allocate_new_bindings`:
    /// the simple names, then one level for each URI, which looks up the URN
    /// map before the injections. It returns the number of levels.
    pub(crate) fn push_new(
        &mut self,
        new: &New,
        resolving: bool,
    ) -> Result<usize, InterpreterError> {
        let count = usize::try_from(new.bind_count).unwrap_or(0);
        let simple = count.saturating_sub(new.uri.len());
        self.binders.reserve(simple + new.uri.len());
        let kind = if resolving {
            BinderKind::Fresh
        } else {
            BinderKind::FreshHole
        };
        for _ in 0..simple {
            let value = fresh_name(self.fresh);
            self.push_binder(kind, value, self.fresh_weight)?;
        }
        for uri in &new.uri {
            let (kind, value) = match self.core.urn_map.get(uri) {
                Some(value) => (BinderKind::Urn, value.clone()),
                None => match new.injections.get(uri).and_then(injection_value) {
                    Some(value) => (BinderKind::Injection, value),
                    None => (BinderKind::Unknown, private_name(vec![0; UNKNOWN_ID_BYTES])),
                },
            };
            // Changed by the merge of feature/cost-accounted-rho (finding
            // R1-16): the measurement decodes pathmap members into a local
            // arena.
            // let metrics = measure_subterm(Subterm::Par(&value), &mut Measures::default())?;
            let arena = Arena::new();
            let metrics = measure_subterm(
                Subterm::Par(&value),
                &mut Measures::default(),
                &mut Members::new(&arena),
            )?;
            self.env_height = self.env_height.max(metrics.height);
            self.env_weight = self.env_weight.max(metrics.weight);
            self.push_binder(kind, value, metrics.weight)?;
        }
        Ok(simple + new.uri.len())
    }

    pub(crate) fn pop(&mut self, count: usize) -> Result<(), InterpreterError> {
        let keep = self.binders.len().checked_sub(count).ok_or_else(|| {
            InterpreterError::BugFoundError(
                "the estimate popped more binders than it pushed".to_string(),
            )
        })?;
        let popped = self.binders[keep..]
            .iter()
            .fold(0u64, |total, binder| total.saturating_add(binder.weight));
        self.env_bytes = self.env_bytes.saturating_sub(popped);
        self.binders.truncate(keep);
        for _ in 0..count {
            self.env.level -= 1;
            self.env.env_map.remove(&self.env.level);
        }
        Ok(())
    }

    fn binder(&self, index: usize, shift: usize) -> Option<Binder> {
        let outer = index.checked_sub(shift)?;
        let position = self.binders.len().checked_sub(outer + 1)?;
        self.binders.get(position).copied()
    }

    /// The fresh serial of a channel that is exactly one bound level of kind
    /// fresh or fresh hole (rule X2).
    fn fresh_channel(&self, channel: Option<&Par>) -> Option<usize> {
        let channel = channel?;
        let level = bare_level(channel)?;
        let binder = self.binder(usize::try_from(level).ok()?, 0)?;
        match binder.kind {
            BinderKind::Fresh | BinderKind::FreshHole => Some(binder.serial),
            _ => None,
        }
    }

    /// True when a free index of `locally_free` above `shift` names a fresh
    /// level (rule X3).
    fn names_fresh(&self, locally_free: &[u8], shift: usize) -> bool {
        self.names_kind(locally_free, shift, |kind| {
            matches!(kind, BinderKind::Fresh | BinderKind::FreshHole)
        })
    }

    /// Added by the G1-4a fix: true when a free index of `locally_free` above
    /// `shift` names a level whose kind passes `test`. `names_fresh` used the
    /// same scan for the fresh kinds before the fix.
    fn names_kind(
        &self,
        locally_free: &[u8],
        shift: usize,
        test: impl Fn(BinderKind) -> bool,
    ) -> bool {
        locally_free
            .iter()
            .enumerate()
            .filter(|(_, free)| **free != 0)
            .any(|(index, _)| {
                self.binder(index, shift)
                    .is_some_and(|binder| test(binder.kind))
            })
    }

    /// True when the expression names a fresh level through an operand, its
    /// collection or a bound variable (rule X3). The operands come from the
    /// shared operand slots.
    fn expr_names_fresh(&self, expr: &Expr) -> bool {
        let operands = || {
            expr_operands(expr)
                .into_iter()
                .any(|(_, par)| par.is_some_and(|par| self.names_fresh(&par.locally_free, 0)))
        };
        let Some(instance) = expr.expr_instance.as_ref() else {
            return false;
        };
        match instance {
            ExprInstance::EVarBody(EVar {
                v:
                    Some(Var {
                        var_instance: Some(VarInstance::BoundVar(level)),
                    }),
            }) => usize::try_from(*level).is_ok_and(|level| {
                matches!(
                    self.binder(level, 0).map(|binder| binder.kind),
                    Some(BinderKind::Fresh | BinderKind::FreshHole)
                )
            }),
            ExprInstance::EListBody(list) => self.names_fresh(&list.locally_free, 0),
            ExprInstance::ETupleBody(tuple) => self.names_fresh(&tuple.locally_free, 0),
            ExprInstance::ESetBody(set) => self.names_fresh(&set.locally_free, 0),
            ExprInstance::EMapBody(map) => self.names_fresh(&map.locally_free, 0),
            ExprInstance::EMethodBody(method) => self.names_fresh(&method.locally_free, 0),
            ExprInstance::EPathmapBody(pathmap) => self.names_fresh(&pathmap.locally_free, 0),
            ExprInstance::EZipperBody(zipper) => self.names_fresh(&zipper.locally_free, 0),
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
            | ExprInstance::GUint8(_)
            | ExprInstance::EVarBody(_)
            | ExprInstance::ENotBody(_)
            | ExprInstance::ENegBody(_)
            | ExprInstance::EMultBody(_)
            | ExprInstance::EDivBody(_)
            | ExprInstance::EModBody(_)
            | ExprInstance::EPlusBody(_)
            | ExprInstance::EMinusBody(_)
            | ExprInstance::ELtBody(_)
            | ExprInstance::ELteBody(_)
            | ExprInstance::EGtBody(_)
            | ExprInstance::EGteBody(_)
            | ExprInstance::EEqBody(_)
            | ExprInstance::ENeqBody(_)
            | ExprInstance::EAndBody(_)
            | ExprInstance::EOrBody(_)
            | ExprInstance::EPercentPercentBody(_)
            | ExprInstance::EPlusPlusBody(_)
            | ExprInstance::EMinusMinusBody(_)
            | ExprInstance::EMatchesBody(_) => operands(),
        }
    }

    fn fits(&self, metrics: Option<Metrics>) -> bool {
        metrics.is_some_and(|metrics| {
            metrics
                .height
                .saturating_add(self.env_height)
                .saturating_add(WRAPPER_MARGIN)
                <= SIZING_HEIGHT_LIMIT
        })
    }

    /// The prost length of a channel as the reducer stores it: evaluated,
    /// substituted at depth 0 and unbundled (`eval_send`, `unbundle_receive`).
    fn channel_bytes(&self, channel: &Par, write: bool) -> Sizing<u64> {
        let metrics = self.measures.par(channel);
        if !self.fits(metrics) {
            return Ok(Err(IncompleteReason::UnsizedValue));
        }
        let (evaluated, _) = match self.evaluate(channel)? {
            Ok(evaluated) => evaluated,
            Err(reason) => return Ok(Err(reason)),
        };
        // Changed by R1-03 (merge of feature/cost-accounted-rho): F1R3Lang's
        // substitution takes the term by value. `evaluated` has no later use.
        //     .substitute_and_measure(&evaluated, 0, &self.env)
        let Ok((substituted, _)) = self
            .core
            .substitute
            .substitute_and_measure(evaluated, 0, &self.env)
        else {
            return Ok(Err(IncompleteReason::UnsizedValue));
        };
        let channel = match single_bundle(&substituted) {
            Some(bundle) => match (write && bundle.write_flag) || (!write && bundle.read_flag) {
                true => bundle.body.unwrap_or_default(),
                false => return Ok(Err(IncompleteReason::UnsizedValue)),
            },
            None => substituted,
        };
        match find_marker(&channel)? {
            Some(Marker::Poison) => Ok(Err(IncompleteReason::ReceivedValueInIntroduction)),
            Some(Marker::Unknown) => Ok(Err(IncompleteReason::UnsizedValue)),
            None => Ok(Ok(byte_count(channel.encoded_len()))),
        }
    }

    /// The pre-walk of arbitration H2. It returns the output bound of
    /// evaluating `root` when every evaluated form has a known work limit, and
    /// `None` otherwise. Literals, collections, fixed-width arithmetic,
    /// comparisons, boolean operators, `++` on strings, byte arrays and lists,
    /// and `%%` with a literal map have one.
    pub(crate) fn evaluation_bound(
        &self,
        root: &Par,
    ) -> Result<Option<EvaluationBound>, InterpreterError> {
        enum Step<'a> {
            Par(&'a Par),
            Expr(&'a Expr),
            ParDone(&'a Par),
            ExprDone(&'a Expr),
        }
        if self.measures.par(root).is_none() {
            return Ok(None);
        }
        let mut work = vec![Step::Par(root)];
        let mut values: Vec<Evaluated> = Vec::new();
        while let Some(step) = work.pop() {
            match step {
                Step::Par(par) => {
                    work.push(Step::ParDone(par));
                    work.extend(par.exprs.iter().rev().map(Step::Expr));
                }
                Step::Expr(expr) => {
                    let Some(operands) = evaluated_operands(expr) else {
                        return Ok(None);
                    };
                    work.push(Step::ExprDone(expr));
                    work.extend(operands.into_iter().rev().map(Step::Par));
                }
                Step::ParDone(par) => {
                    let start = values.len().checked_sub(par.exprs.len()).ok_or_else(|| {
                        InterpreterError::BugFoundError(
                            "the evaluation pre-walk lost a value".to_string(),
                        )
                    })?;
                    let parts = values.split_off(start);
                    let growth_sum = parts
                        .iter()
                        .fold(0u64, |total, part| total.saturating_add(part.growth));
                    let class = match (parts.as_slice(), single_expression(par)) {
                        ([part], true) => part.class,
                        _ => ValueClass::Other,
                    };
                    values.push(Evaluated {
                        class,
                        growth: growth_sum,
                    });
                }
                Step::ExprDone(expr) => {
                    let operands = evaluated_operands(expr).map_or(0, |operands| operands.len());
                    let start = values.len().checked_sub(operands).ok_or_else(|| {
                        InterpreterError::BugFoundError(
                            "the evaluation pre-walk lost an operand".to_string(),
                        )
                    })?;
                    let parts = values.split_off(start);
                    let Some(value) = classify_expr(expr, &parts, self.env_weight) else {
                        return Ok(None);
                    };
                    values.push(value);
                }
            }
        }
        let exact = byte_count(root.encoded_len());
        let output = exact.saturating_add(values.last().map_or(0, |value| value.growth));
        Ok(Some(EvaluationBound { output }))
    }

    /// Evaluates `par` with the reducer's `eval_expr` after the pre-walk of
    /// arbitration H2.
    fn evaluate(&self, par: &Par) -> Sizing<(Par, u64)> {
        let Some(bound) = self.evaluation_bound(par)? else {
            return Ok(Err(IncompleteReason::DataEvaluation));
        };
        match self.core.eval_expr(par, &self.env) {
            Ok(evaluated) => Ok(Ok((evaluated, bound.output))),
            Err(_) => Ok(Err(IncompleteReason::DataEvaluation)),
        }
    }

    /// The prost length of one closed datum after the reducer's evaluation and
    /// substitution, as `eval_send` measures it.
    fn datum_length(&self, datum: &Par) -> Sizing<u64> {
        let metrics = self.measures.par(datum);
        if !self.fits(metrics) {
            return Ok(Err(IncompleteReason::UnsizedValue));
        }
        let (evaluated, _) = match self.evaluate(datum)? {
            Ok(evaluated) => evaluated,
            Err(reason) => return Ok(Err(reason)),
        };
        // Changed by R1-03 (merge of feature/cost-accounted-rho): F1R3Lang's
        // substitution takes the term by value. `evaluated` has no later use.
        //     .substitute_and_measure(&evaluated, 0, &self.env)
        match self
            .core
            .substitute
            .substitute_and_measure(evaluated, 0, &self.env)
        {
            Ok((_, len)) => Ok(Ok(len)),
            Err(_) => Ok(Err(IncompleteReason::DataEvaluation)),
        }
    }

    /// The sizes of one send: its message and its introduction bytes.
    fn size_send(&self, send: &Send, scope: Option<usize>) -> Sizing<(u64, u64)> {
        let Some(channel) = send.chan.as_ref() else {
            return Ok(Err(IncompleteReason::UnsizedValue));
        };
        let channel = match self.channel_bytes(channel, true)? {
            Ok(bytes) => bytes,
            Err(reason) => return Ok(Err(reason)),
        };
        let mut pars = 0u64;
        for datum in &send.data {
            if !is_closed(&datum.locally_free) {
                return Ok(Err(IncompleteReason::OpenData));
            }
            let len = match self.datum_length(datum)? {
                Ok(len) => len,
                Err(reason) => return Ok(Err(reason)),
            };
            let Ok(sum) = message_field_bytes(1, len)
                .and_then(|field| pars.checked_add(field).ok_or(ByteAccountingError::Overflow))
            else {
                return Ok(Err(IncompleteReason::UnsizedValue));
            };
            pars = sum;
        }
        let Some(seal) = self.enclosing_seal(scope) else {
            return Ok(Err(IncompleteReason::UnsizedValue));
        };
        let sizes = datum_bytes(pars, &RANDOM_STATE, Some(seal), None).and_then(|message| {
            produce_introduction_bytes(channel, message).map(|introduction| (message, introduction))
        });
        Ok(sizes.map_err(|_| IncompleteReason::UnsizedValue))
    }

    /// The field length of the authority that a participant at `scope`
    /// carries: the enclosing regions, or the deploy region that it opens
    /// when no signed scope encloses it.
    fn enclosing_seal(&self, scope: Option<usize>) -> Option<u64> {
        match scope {
            Some(scope) => self.scopes.get(scope).and_then(|scope| scope.seal_bytes),
            None => self.deploy_seal,
        }
    }

    fn pattern_length(&self, bind: &ReceiveBind) -> Sizing<u64> {
        let mut patterns = Vec::with_capacity(bind.patterns.len());
        for pattern in &bind.patterns {
            let metrics = self.measures.par(pattern);
            if !self.fits(metrics) {
                return Ok(Err(IncompleteReason::UnsizedValue));
            }
            // Changed by R1-03 (merge of feature/cost-accounted-rho): F1R3Lang's
            // substitution takes the term by value, so the borrowed pattern is
            // cloned here, as theirs' wrapper cloned it inside.
            //     .substitute_and_charge(pattern, 1, &self.env)
            let Ok(substituted) =
                self.core
                    .substitute
                    .substitute_and_charge(pattern.clone(), 1, &self.env)
            else {
                return Ok(Err(IncompleteReason::UnsizedValue));
            };
            match find_marker(&substituted)? {
                Some(Marker::Poison) => {
                    return Ok(Err(IncompleteReason::ReceivedValueInIntroduction))
                }
                Some(Marker::Unknown) => return Ok(Err(IncompleteReason::UnsizedValue)),
                None => patterns.push(substituted),
            }
        }
        let pattern = BindPattern {
            patterns,
            remainder: bind.remainder.clone(),
            free_count: bind.free_count,
        };
        Ok(Ok(byte_count(pattern.encoded_len())))
    }

    /// Substitutes `term` as the reducer does when it stores it, measures the
    /// result and checks it for markers (design §5.6, item 5).
    fn substituted_length(&mut self, term: &Par, depth: i32, shift: usize) -> Sizing<u64> {
        let metrics = self.measures.par(term);
        if !self.fits(metrics) {
            return Ok(Err(IncompleteReason::UnsizedValue));
        }
        let shift = i32::try_from(shift).map_err(|_| {
            InterpreterError::BugFoundError("the estimate shift does not fit in i32".to_string())
        })?;
        self.env.shift += shift;
        // Changed by R1-03 (merge of feature/cost-accounted-rho): F1R3Lang's
        // substitution takes the term by value, so the borrowed term is cloned
        // here, as theirs' wrappers cloned it inside.
        //         .substitute_no_sort_and_measure(term, 0, &self.env)
        //         .substitute_and_measure(term, depth, &self.env)
        let substituted = if depth == 0 {
            self.core
                .substitute
                .substitute_no_sort_and_measure(term.clone(), 0, &self.env)
        } else {
            self.core
                .substitute
                .substitute_and_measure(term.clone(), depth, &self.env)
        };
        self.env.shift -= shift;
        let Ok((substituted, len)) = substituted else {
            return Ok(Err(IncompleteReason::UnsizedValue));
        };
        match find_marker(&substituted)? {
            Some(Marker::Poison) => Ok(Err(IncompleteReason::ReceivedValueInIntroduction)),
            Some(Marker::Unknown) => Ok(Err(IncompleteReason::UnsizedValue)),
            None => Ok(Ok(len)),
        }
    }

    /// The introduction bytes of one receive (design §4.3), with the sizes of
    /// its channels, patterns, guard, body and seal.
    fn size_receive(&mut self, receive: &Receive, seal: Option<u64>) -> Sizing<u64> {
        let mut binds = 0u64;
        for bind in &receive.binds {
            let Some(source) = bind.source.as_ref() else {
                return Ok(Err(IncompleteReason::UnsizedValue));
            };
            let channel = match self.channel_bytes(source, false)? {
                Ok(bytes) => bytes,
                Err(reason) => return Ok(Err(reason)),
            };
            if bind
                .patterns
                .iter()
                .any(|pattern| !is_closed(&pattern.locally_free))
            {
                return Ok(Err(IncompleteReason::OpenPattern));
            }
            let pattern = match self.pattern_length(bind)? {
                Ok(bytes) => bytes,
                Err(reason) => return Ok(Err(reason)),
            };
            let Some(sum) = binds
                .checked_add(channel)
                .and_then(|sum| sum.checked_add(pattern))
            else {
                return Ok(Err(IncompleteReason::UnsizedValue));
            };
            binds = sum;
        }
        // The reducer stores a guard after two substitutions. The body
        // substitution of the nearest enclosing receive replaces the levels
        // outside that receive. Then `eval_receive` substitutes at depth 1,
        // which replaces no bound variable. In a complete estimate, no
        // substitution reaches an outer level of a guard. Each such level is a
        // pattern level inside the nearest enclosing receive, or anywhere when
        // no receive encloses the guard. Rule X3 excludes the fresh levels,
        // `receive` excludes the URN, injection and unknown levels, and the
        // marker scan of the enclosing body excludes the pattern levels from
        // farther out. So the stored guard keeps its variables, and this
        // sizing at depth 1 is its size. Its marker scan can find a marker only through a
        // variable reference of depth 1, and it stays for that case (review
        // LOW 2).
        let guard = match receive.condition.as_ref() {
            Some(condition) if condition != &Par::default() => {
                match self.substituted_length(condition, 1, 0)? {
                    Ok(len) => Some(len),
                    Err(reason) => return Ok(Err(reason)),
                }
            }
            _ => None,
        };
        let Some(body) = receive.body.as_ref() else {
            return Ok(Err(IncompleteReason::UnsizedValue));
        };
        let shift = usize::try_from(receive.bind_count).unwrap_or(0);
        let body = match self.substituted_length(body, 0, shift)? {
            Ok(len) => len,
            Err(reason) => return Ok(Err(reason)),
        };
        let Some(seal) = seal else {
            return Ok(Err(IncompleteReason::UnsizedValue));
        };
        let introduction = par_with_random_bytes(body, &RANDOM_STATE)
            .and_then(|body| continuation_bytes(body, guard, Some(seal)))
            .and_then(|continuation| {
                consume_introduction_bytes(binds, continuation, receive.binds.len())
            });
        Ok(introduction.map_err(|_| IncompleteReason::UnsizedValue))
    }

    /// The lane of a signature. Changed by the G1-4a fix:
    /// - A signature taller than the sizing bound never reaches the recursive
    ///   conversions. Its lane is dynamic, with the reason `UnsizedValue`
    ///   (review M-2).
    /// - A signature that still holds a variable names another lane at run
    ///   time, so its lane is dynamic too (review H-1).
    fn lane_of(&mut self, signature: &CostSignature) -> Result<Lane, InterpreterError> {
        let metrics = match self.measures.signature(signature) {
            Some(metrics)
                if metrics.height.saturating_add(WRAPPER_MARGIN) <= SIZING_HEIGHT_LIMIT =>
            {
                metrics
            }
            Some(_) | None => {
                self.violate(IncompleteReason::UnsizedValue);
                return Ok(Lane::Dynamic);
            }
        };
        if metrics.vars > 0 {
            self.violate(IncompleteReason::StaticPlan(
                UnprovableDemand::DynamicAuthority,
            ));
            return Ok(Lane::Dynamic);
        }
        Ok(match cost_signature_to_sig(signature) {
            Ok(Sig::Unit) => Lane::Unit,
            Ok(sig) => {
                let key = sig.lane_hash();
                if let Ok(canonical) = canonical_cost_signature(signature) {
                    self.lanes.entry(key).or_insert(canonical);
                }
                Lane::Static(key)
            }
            Err(_) => Lane::Dynamic,
        })
    }

    /// Added by the G1-4a fix (review M-2): whether every signature of the
    /// term fits the sizing bound, so the recursive canonicalization of the
    /// plan stays within it.
    pub(crate) fn signatures_fit(&self) -> bool {
        self.measures
            .signatures
            .values()
            .all(|metrics| metrics.height.saturating_add(WRAPPER_MARGIN) <= SIZING_HEIGHT_LIMIT)
    }

    fn region_bytes(
        &self,
        signature: &CostSignature,
        lane: Lane,
        discriminator: u32,
    ) -> Result<Option<u64>, InterpreterError> {
        match lane {
            Lane::Dynamic => Ok(None),
            Lane::Static(_) | Lane::Unit => match self.measures.signature(signature) {
                Some(metrics)
                    if metrics.height.saturating_add(WRAPPER_MARGIN) <= SIZING_HEIGHT_LIMIT =>
                {
                    Ok(region_field_bytes(signature, discriminator))
                }
                Some(_) | None => Ok(None),
            },
        }
    }

    /// Opens the scope of a signed term inside `parent`.
    pub(crate) fn open_scope(
        &mut self,
        term: &CostSignedTerm,
        parent: Option<usize>,
    ) -> Result<usize, InterpreterError> {
        let (lane, region) = match term.signature.as_ref() {
            Some(signature) => {
                let lane = self.lane_of(signature)?;
                (lane, self.region_bytes(signature, lane, 0)?)
            }
            None => (Lane::Dynamic, None),
        };
        let parent_scope = parent.and_then(|parent| self.scopes.get(parent).copied());
        let enclosing = match parent_scope {
            Some(parent) => parent.seal_bytes,
            None => Some(0),
        };
        let seal_bytes =
            enclosing.and_then(|enclosing| region.and_then(|region| enclosing.checked_add(region)));
        let dynamic = lane == Lane::Dynamic || parent_scope.is_some_and(|parent| parent.dynamic);
        self.scopes.push(Scope {
            parent,
            lane,
            dynamic,
            seal_bytes,
        });
        Ok(self.scopes.len() - 1)
    }

    pub(crate) fn send(
        &mut self,
        send: &Send,
        scope: Option<usize>,
    ) -> Result<(), InterpreterError> {
        let channel = self.fresh_channel(send.chan.as_ref());
        if channel.is_none() {
            self.violate(IncompleteReason::NonFreshChannel);
        }
        let (message, introduction) = match self.size_send(send, scope)? {
            Ok((message, introduction)) => (Some(message), Some(introduction)),
            Err(reason) => {
                self.violate(reason);
                (None, None)
            }
        };
        self.participants.push(Participant::Send {
            scope,
            channel,
            message,
            introduction,
        });
        self.program
            .push(Instruction::Local(self.participants.len() - 1));
        Ok(())
    }

    pub(crate) fn receive(
        &mut self,
        receive: &Receive,
        scope: Option<usize>,
    ) -> Result<(), InterpreterError> {
        if receive.peek {
            self.violate(IncompleteReason::PeekOrStack);
        }
        let mut channels = Vec::with_capacity(receive.binds.len());
        for bind in &receive.binds {
            let channel = self.fresh_channel(bind.source.as_ref());
            if channel.is_none() {
                self.violate(IncompleteReason::NonFreshChannel);
            }
            channels.push(channel);
        }
        let mut binds = Vec::with_capacity(receive.binds.len());
        let mut bind_seal = Some(0u64);
        for (index, bind) in receive.binds.iter().enumerate() {
            if let Some(signature) = bind.cost_signature.as_ref() {
                let lane = self.lane_of(signature)?;
                let discriminator = u32::try_from(index + 1).unwrap_or(u32::MAX);
                let region = self.region_bytes(signature, lane, discriminator)?;
                bind_seal =
                    bind_seal.and_then(|seal| region.and_then(|region| seal.checked_add(region)));
                binds.push(lane);
            }
        }
        let seal = match (binds.is_empty(), scope) {
            (true, _) => self.enclosing_seal(scope),
            (false, None) => bind_seal,
            (false, Some(scope)) => self
                .scopes
                .get(scope)
                .and_then(|scope| scope.seal_bytes)
                .and_then(|chain| bind_seal.and_then(|binds| chain.checked_add(binds))),
        };
        if let Some(condition) = receive.condition.as_ref() {
            // Changed by the G1-4a fix (review M-1): the normalizer builds the
            // guard in the body scope, so its indices start above the binders
            // of the receive.
            // if self.names_fresh(&condition.locally_free, 0) {
            let binders = usize::try_from(receive.bind_count).unwrap_or(0);
            if self.names_fresh(&condition.locally_free, binders) {
                self.violate(IncompleteReason::EscapingName);
            }
            // Added by the G1-4a fix (reviews M-5 and LOW 2): the body
            // substitution of an enclosing receive replaces each level of a
            // nested guard that lies outside that receive, and the reducer
            // stores the result. A URN, injection or unknown level there takes
            // its value, which the guard sizing at depth 1 does not see. So a
            // guard that names such a level has no size. A pattern level from
            // outside the nearest enclosing receive reaches the marker scan of
            // that receive's body instead (rule X6).
            if self.names_kind(&condition.locally_free, binders, |kind| {
                matches!(
                    kind,
                    BinderKind::Urn | BinderKind::Injection | BinderKind::Unknown
                )
            }) {
                self.violate(IncompleteReason::UnsizedValue);
            }
        }
        let introduction = match self.size_receive(receive, seal)? {
            Ok(introduction) => Some(introduction),
            Err(reason) => {
                self.violate(reason);
                None
            }
        };
        self.participants.push(Participant::Receive {
            scope,
            channels,
            binds,
            introduction,
        });
        self.program
            .push(Instruction::Local(self.participants.len() - 1));
        Ok(())
    }

    /// Rules X3 and X8 on the top-level expressions of an execution position.
    /// Rule X1 already covers a dequotation there.
    pub(crate) fn check_exprs(&mut self, par: &Par) -> Result<(), InterpreterError> {
        for expr in &par.exprs {
            match expr.expr_instance.as_ref() {
                Some(ExprInstance::EMethodBody(_)) => {
                    self.violate(IncompleteReason::MethodInProcessPosition)
                }
                Some(ExprInstance::EVarBody(_)) => {}
                Some(_) | None => {
                    if self.expr_names_fresh(expr) {
                        self.violate(IncompleteReason::EscapingName);
                    }
                }
            }
        }
        Ok(())
    }

    pub(crate) fn check_match(&mut self, matched: &Match) {
        if let Some(target) = matched.target.as_ref() {
            if self.names_fresh(&target.locally_free, 0) {
                self.violate(IncompleteReason::EscapingName);
            }
        }
    }

    pub(crate) fn check_case(&mut self, case: &MatchCase) {
        let shift = usize::try_from(case.free_count).unwrap_or(0);
        let pattern = case
            .pattern
            .as_ref()
            .is_some_and(|pattern| self.names_fresh(&pattern.locally_free, 0));
        let guard = case
            .guard
            .as_ref()
            .is_some_and(|guard| self.names_fresh(&guard.locally_free, shift));
        if pattern || guard {
            self.violate(IncompleteReason::EscapingName);
        }
    }

    pub(crate) fn check_conditional(&mut self, conditional: &If) {
        if let Some(condition) = conditional.condition.as_ref() {
            if self.names_fresh(&condition.locally_free, 0) {
                self.violate(IncompleteReason::EscapingName);
            }
        }
    }

    pub(crate) fn check_stacks(&mut self, par: &Par) {
        if !par.cost_stacks.is_empty() {
            self.violate(IncompleteReason::PeekOrStack);
        }
    }

    pub(crate) fn enter_visit(&mut self, _par: &Par) -> Result<(), InterpreterError> {
        #[cfg(test)]
        self.levels
            .push((_par as *const Par as usize, self.binders.len()));
        self.program.push(Instruction::Push);
        Ok(())
    }

    /// Pass 1: the walk over the execution positions in the reducer's term
    /// order (design §5.2). It emits the program of `compile_term` in
    /// `StackSafeDemandWalk.v`.
    pub(crate) fn walk(&mut self, root: &Par) -> Result<(), InterpreterError> {
        let mut work = vec![Task::Visit {
            par: root,
            scope: None,
            resolving: true,
        }];
        while let Some(task) = work.pop() {
            match task {
                Task::Visit {
                    par,
                    scope,
                    resolving,
                } => {
                    self.enter_visit(par)?;
                    let mut ordered = Vec::with_capacity(
                        par.sends.len()
                            + par.receives.len()
                            + par.news.len()
                            + par.matches.len()
                            + par.conditionals.len()
                            + par.bundles.len()
                            + par.cost_signed_terms.len()
                            + 2,
                    );
                    ordered.extend(par.sends.iter().map(|send| Task::Send(send, scope)));
                    ordered.extend(
                        par.receives
                            .iter()
                            .map(|receive| Task::Receive(receive, scope)),
                    );
                    ordered.extend(par.news.iter().map(|new| Task::New(new, scope, resolving)));
                    ordered.extend(
                        par.matches
                            .iter()
                            .map(|matched| Task::Match(matched, scope, resolving)),
                    );
                    ordered.extend(
                        par.conditionals
                            .iter()
                            .map(|conditional| Task::Conditional(conditional, scope, resolving)),
                    );
                    ordered.extend(
                        par.bundles
                            .iter()
                            .map(|bundle| Task::Bundle(bundle, scope, resolving)),
                    );
                    ordered.push(Task::Exprs(par));
                    ordered.extend(
                        par.cost_signed_terms
                            .iter()
                            .map(|term| Task::Signed(term, scope, resolving)),
                    );
                    ordered.push(Task::Stacks(par));
                    work.extend(ordered.into_iter().rev());
                }
                Task::Send(send, scope) => self.send(send, scope)?,
                Task::Receive(receive, scope) => {
                    self.receive(receive, scope)?;
                    if let Some(body) = receive.body.as_ref() {
                        let count = usize::try_from(receive.bind_count).unwrap_or(0);
                        work.extend(
                            [
                                Task::Holes(count, BinderKind::ReceivePattern),
                                Task::Visit {
                                    par: body,
                                    scope: None,
                                    resolving: false,
                                },
                                Task::Pop(count),
                                Task::Emit(Instruction::Combine),
                            ]
                            .into_iter()
                            .rev(),
                        );
                    }
                }
                Task::New(new, scope, resolving) => {
                    let count = self.push_new(new, resolving)?;
                    match new.p.as_ref() {
                        Some(body) => work.extend(
                            [
                                Task::Visit {
                                    par: body,
                                    scope,
                                    resolving,
                                },
                                Task::Pop(count),
                                Task::Emit(Instruction::Combine),
                            ]
                            .into_iter()
                            .rev(),
                        ),
                        None => work.push(Task::Pop(count)),
                    }
                }
                Task::Match(matched, scope, resolving) => {
                    self.check_match(matched);
                    let mut ordered = Vec::with_capacity(matched.cases.len() + 1);
                    let mut sources = 0usize;
                    for case in &matched.cases {
                        let alternative = case.source.is_some() && sources > 0;
                        sources += usize::from(case.source.is_some());
                        ordered.push(Task::Case {
                            case,
                            scope,
                            resolving,
                            alternative,
                        });
                    }
                    if sources > 0 {
                        ordered.push(Task::Emit(Instruction::Combine));
                    }
                    work.extend(ordered.into_iter().rev());
                }
                Task::Case {
                    case,
                    scope,
                    resolving,
                    alternative,
                } => {
                    self.check_case(case);
                    if let Some(source) = case.source.as_ref() {
                        let count = usize::try_from(case.free_count).unwrap_or(0);
                        if alternative {
                            work.push(Task::Emit(Instruction::Alternative));
                        }
                        work.extend(
                            [
                                Task::Holes(count, BinderKind::MatchPattern),
                                Task::Visit {
                                    par: source,
                                    scope,
                                    resolving,
                                },
                                Task::Pop(count),
                            ]
                            .into_iter()
                            .rev(),
                        );
                    }
                }
                Task::Conditional(conditional, scope, resolving) => {
                    self.check_conditional(conditional);
                    work.extend(
                        [
                            branch_task(conditional.if_true.as_ref(), scope, resolving),
                            branch_task(conditional.if_false.as_ref(), scope, resolving),
                            Task::Emit(Instruction::Alternative),
                            Task::Emit(Instruction::Combine),
                        ]
                        .into_iter()
                        .rev(),
                    );
                }
                Task::Bundle(bundle, scope, resolving) => {
                    if let Some(body) = bundle.body.as_ref() {
                        work.extend(
                            [
                                Task::Visit {
                                    par: body,
                                    scope,
                                    resolving,
                                },
                                Task::Emit(Instruction::Combine),
                            ]
                            .into_iter()
                            .rev(),
                        );
                    }
                }
                Task::Exprs(par) => self.check_exprs(par)?,
                Task::Signed(term, scope, resolving) => {
                    let opened = self.open_scope(term, scope)?;
                    if let Some(body) = term.body.as_ref() {
                        work.extend(
                            [
                                Task::Visit {
                                    par: body,
                                    scope: Some(opened),
                                    resolving,
                                },
                                Task::Emit(Instruction::Combine),
                            ]
                            .into_iter()
                            .rev(),
                        );
                    }
                }
                Task::Stacks(par) => self.check_stacks(par),
                Task::Holes(count, kind) => self.push_holes(count, kind)?,
                Task::Pop(count) => self.pop(count)?,
                Task::Emit(instruction) => self.program.push(instruction),
            }
        }
        match self.binders.is_empty() {
            true => Ok(()),
            false => Err(InterpreterError::BugFoundError(
                "the estimate finished with binders in scope".to_string(),
            )),
        }
    }

    /// The channel tables, filled after pass 1 by one loop over the
    /// participants (design §5.5). A join with a repeated channel counts its
    /// datum once for each bind.
    pub(crate) fn tables(&self) -> Result<Tables, InterpreterError> {
        let mut tables = Tables {
            message: vec![0; self.fresh],
            transfer: vec![0; self.fresh],
            trace: vec![0; self.fresh],
        };
        for participant in &self.participants {
            if let Participant::Send {
                channel: Some(channel),
                message: Some(message),
                ..
            } = participant
            {
                let slot = &mut tables.message[*channel];
                *slot = (*slot).max(*message);
            }
        }
        for participant in &self.participants {
            if let Participant::Receive { channels, .. } = participant {
                let Some(fresh) = channels.iter().copied().collect::<Option<Vec<_>>>() else {
                    continue;
                };
                let transfer = fresh.iter().fold(0u64, |total, channel| {
                    total.saturating_add(tables.message[*channel])
                });
                let trace = comm_trace_bytes(fresh.len()).unwrap_or(u64::MAX);
                for channel in fresh {
                    tables.transfer[channel] = tables.transfer[channel].max(transfer);
                    tables.trace[channel] = tables.trace[channel].max(trace);
                }
            }
        }
        Ok(tables)
    }

    /// Calls `action` with the lane of every static region of the authority
    /// that a participant at `scope` carries. With `deploy`, a participant
    /// outside every signed scope carries one region of the deploy lane. It
    /// returns false, and calls nothing, when a region is dynamic.
    fn for_each_region(
        &self,
        scope: Option<usize>,
        deploy: bool,
        mut action: impl FnMut(SigKey),
    ) -> bool {
        let Some(start) = scope else {
            if deploy {
                action(self.deploy_key);
            }
            return true;
        };
        if self.scopes.get(start).is_none_or(|scope| scope.dynamic) {
            return false;
        }
        let mut current = Some(start);
        while let Some(index) = current {
            let scope = self.scopes[index];
            if let Lane::Static(lane) = scope.lane {
                action(lane);
            }
            current = scope.parent;
        }
        true
    }

    /// Adds the contribution of one participant to `vector` (design §5.5). A
    /// participant without a size or with a dynamic region adds nothing
    /// (design §6.2), and the transfer and trace terms of a channel that is not
    /// fresh are zero.
    pub(crate) fn contribute(
        &self,
        participant: &Participant,
        tables: &Tables,
        vector: &mut Vector,
    ) {
        let mut add = |lane: SigKey, counts: [u64; 4]| {
            let slot = vector.entry(lane).or_insert([0; 4]);
            for (total, count) in slot.iter_mut().zip(counts) {
                *total = total.saturating_add(count);
            }
        };
        match participant {
            Participant::Send {
                scope,
                channel,
                introduction: Some(introduction),
                ..
            } => {
                let (transfer, trace) = match channel {
                    Some(channel) => (tables.transfer[*channel], tables.trace[*channel]),
                    None => (0, 0),
                };
                self.for_each_region(*scope, true, |lane| {
                    add(lane, [1, *introduction, transfer, trace])
                });
            }
            Participant::Receive {
                scope,
                channels,
                binds,
                introduction: Some(introduction),
            } => {
                if binds.contains(&Lane::Dynamic) || !self.for_each_region(*scope, true, |_| {}) {
                    return;
                }
                let (transfer, trace) = match channels.iter().copied().collect::<Option<Vec<_>>>() {
                    Some(fresh) => (
                        fresh.iter().fold(0u64, |total, channel| {
                            total.saturating_add(tables.message[*channel])
                        }),
                        comm_trace_bytes(fresh.len()).unwrap_or(u64::MAX),
                    ),
                    None => (0, 0),
                };
                self.for_each_region(*scope, true, |lane| add(lane, [0, *introduction, 0, 0]));
                let comm = [1, 0, transfer, trace];
                if binds.is_empty() {
                    self.for_each_region(*scope, true, |lane| add(lane, comm));
                } else {
                    self.for_each_region(*scope, false, |lane| add(lane, comm));
                    for lane in binds {
                        if let Lane::Static(lane) = lane {
                            add(*lane, comm);
                        }
                    }
                }
            }
            Participant::Send { .. } | Participant::Receive { .. } => {}
        }
    }

    /// Pass 2: runs the program on a value stack, the machine `run` of
    /// `StackSafeDemandWalk.v`. A merge moves the smaller map into the larger
    /// one, which gives the same vector because both operations commute.
    pub(crate) fn run(&self, tables: &Tables) -> Result<Vector, InterpreterError> {
        let lost =
            || InterpreterError::BugFoundError("the vector fold lost an operand".to_string());
        let mut values: Vec<Vector> = Vec::new();
        for instruction in &self.program {
            match instruction {
                Instruction::Push => values.push(Vector::new()),
                Instruction::Local(index) => {
                    let participant = self.participants.get(*index).ok_or_else(lost)?;
                    let top = values.last_mut().ok_or_else(lost)?;
                    self.contribute(participant, tables, top);
                }
                Instruction::Combine => {
                    let child = values.pop().ok_or_else(lost)?;
                    let parent = values.pop().ok_or_else(lost)?;
                    values.push(merge_vectors(parent, child, u64::saturating_add));
                }
                Instruction::Alternative => {
                    let later = values.pop().ok_or_else(lost)?;
                    let earlier = values.pop().ok_or_else(lost)?;
                    values.push(merge_vectors(earlier, later, u64::max));
                }
            }
        }
        match (values.pop(), values.is_empty()) {
            (Some(vector), true) => Ok(vector),
            _ => Err(lost()),
        }
    }

    /// The estimate from the plan, the vector and the static signatures
    /// (design §5.5 and §5.7). The compute row is the plan's count when rule
    /// X1 holds, and the vector's count otherwise. Changed by the G1-4a fix:
    /// `plan` is `None` when a signature is taller than the sizing bound and
    /// the plan did not run (review M-2), and a counted lane without a
    /// signature has no units, so its counts drop out and the estimate is
    /// incomplete (review LOW 6).
    pub(crate) fn assemble(
        &self,
        plan: Option<Result<super::StaticAuthorityPlan, UnprovableDemand>>,
        vector: Vector,
        static_signatures: Option<BTreeMap<SigKey, CostSignature>>,
        rules: &NativePhloRules,
        schedule: &PhloScheduleV1<'_>,
    ) -> Result<CostEstimate, InterpreterError> {
        let mut rows: BTreeMap<SigKey, [u64; 4]> = vector.into_iter().collect();
        let mut completeness = match &plan {
            Some(Ok(plan)) => {
                for counts in rows.values_mut() {
                    counts[COMPUTE] = 0;
                }
                for (lane, amount) in &plan.demand.0 {
                    rows.entry(*lane).or_insert([0; 4])[COMPUTE] = *amount;
                }
                match &self.reason {
                    Some(reason) => Completeness::Incomplete(reason.clone()),
                    None => Completeness::Complete,
                }
            }
            Some(Err(reason)) => {
                Completeness::Incomplete(IncompleteReason::StaticPlan(reason.clone()))
            }
            None => Completeness::Incomplete(
                self.reason
                    .clone()
                    .unwrap_or(IncompleteReason::UnsizedValue),
            ),
        };
        let mut signatures = static_signatures.unwrap_or_default();
        for (lane, signature) in &self.lanes {
            signatures.entry(*lane).or_insert_with(|| signature.clone());
        }
        if let Some(signature) = self.deploy_signature.as_ref() {
            signatures
                .entry(self.deploy_key)
                .or_insert_with(|| signature.clone());
        }
        let unpriced = rows
            .keys()
            .filter(|lane| !signatures.contains_key(*lane))
            .copied()
            .collect::<Vec<_>>();
        if !unpriced.is_empty() {
            for lane in &unpriced {
                rows.remove(lane);
            }
            if completeness == Completeness::Complete {
                completeness = Completeness::Incomplete(IncompleteReason::UnsizedValue);
            }
        }
        // Changed by dedup (task 11415, D8): the B loop moved to `bound_over`,
        // which `retain_client_lanes` shares. The loop below keeps the
        // quantities and the units of each lane. `rules` comes from
        // `schedule`, so each class has a weight, and every lane of `rows`
        // has a signature after the unpriced lanes left. The classes of one
        // lane are distinct (`NativePhloRules::resolve`), so B has the same
        // terms as before.
        // let mut quantities = BTreeMap::new();
        // let mut bound_phlo = Some(0u64);
        // for (lane, counts) in &rows {
        //     let units = signatures.get(lane).map(signature_units);
        //     for (dimension, count) in NativePhloDimension::ALL.into_iter().zip(counts) {
        //         if *count == 0 {
        //             continue;
        //         }
        //         let class = rules
        //             .class_for(dimension)
        //             .map_err(|error| InterpreterError::ReduceError(error.to_string()))?;
        //         quantities.insert((*lane, class), *count);
        //         let weight = schedule
        //             .classes
        //             .get(class)
        //             .map(|class| class.weight)
        //             .ok_or_else(|| {
        //                 InterpreterError::ReduceError(
        //                     "the schedule has no class for a native dimension".to_string(),
        //                 )
        //             })?;
        //         let units = units.ok_or_else(|| {
        //             InterpreterError::ReduceError(
        //                 "the estimate counted a lane without a static signature".to_string(),
        //             )
        //         })?;
        //         bound_phlo = bound_phlo.and_then(|bound| {
        //             weight
        //                 .checked_mul(units)
        //                 .and_then(|amount| amount.checked_mul(*count))
        //                 .and_then(|amount| bound.checked_add(amount))
        //         });
        //     }
        // }
        let mut quantities = BTreeMap::new();
        let mut units = BTreeMap::new();
        for (lane, counts) in &rows {
            if let Some(signature) = signatures.get(lane) {
                units.insert(*lane, signature_units(signature));
            }
            for (dimension, count) in NativePhloDimension::ALL.into_iter().zip(counts) {
                if *count == 0 {
                    continue;
                }
                let class = rules
                    .class_for(dimension)
                    .map_err(|error| InterpreterError::ReduceError(error.to_string()))?;
                quantities.insert((*lane, class), *count);
            }
        }
        let bound_phlo = bound_over(
            &quantities,
            &units,
            schedule,
            "the schedule has no class for a native dimension",
        )?;
        signatures.retain(|lane, _| rows.contains_key(lane));
        Ok(CostEstimate {
            quantities,
            bound_phlo: bound_phlo.unwrap_or(u64::MAX),
            signatures,
            completeness,
        })
    }
}

/// The task of one `if` branch: its visit, or the empty vector of an absent
/// branch (`compile_branch` in `StackSafeDemandWalk.v`).
fn branch_task(branch: Option<&Par>, scope: Option<usize>, resolving: bool) -> Task<'_> {
    match branch {
        Some(par) => Task::Visit {
            par,
            scope,
            resolving,
        },
        None => Task::Emit(Instruction::Push),
    }
}

fn merge_vectors(left: Vector, right: Vector, combine: fn(u64, u64) -> u64) -> Vector {
    let (mut into, from) = match left.len() >= right.len() {
        true => (left, right),
        false => (right, left),
    };
    for (lane, counts) in from {
        let slot = into.entry(lane).or_insert([0; 4]);
        for (total, count) in slot.iter_mut().zip(counts) {
            *total = combine(*total, count);
        }
    }
    into
}

/// Added by dedup (task 11415, D8): the estimate B of design §5.7 over raw
/// counts. B is the sum of weight times units times count over every lane
/// and class. An overflow gives `None`, which the estimate reports as
/// `u64::MAX`. `assemble` and `retain_client_lanes` share this loop. It
/// checks each quantity in key order. It checks the units of the lane first,
/// and then the weight of the class with the message that the caller names.
fn bound_over(
    quantities: &BTreeMap<(SigKey, usize), u64>,
    units: &BTreeMap<SigKey, u64>,
    schedule: &PhloScheduleV1<'_>,
    missing_class: &str,
) -> Result<Option<u64>, InterpreterError> {
    let mut bound_phlo = Some(0u64);
    for ((lane, class), count) in quantities {
        let lane_units = units.get(lane).ok_or_else(|| {
            InterpreterError::ReduceError(
                "the estimate counted a lane without a static signature".to_string(),
            )
        })?;
        let weight = schedule
            .classes
            .get(*class)
            .map(|class| class.weight)
            .ok_or_else(|| InterpreterError::ReduceError(missing_class.to_string()))?;
        bound_phlo = bound_phlo.and_then(|bound| {
            weight
                .checked_mul(*lane_units)
                .and_then(|amount| amount.checked_mul(*count))
                .and_then(|amount| bound.checked_add(amount))
        });
    }
    Ok(bound_phlo)
}

/// The units of a lane: the Ground, Name and Quote leaves of its signature,
/// which the native meter counts (`authority_units`).
pub(crate) fn signature_units(signature: &CostSignature) -> u64 {
    // Moved by dedup (task 11415) to `signature_leaf_units` in
    // `rholang/src/rust/interpreter/accounting/native_phlo_rules/execution.rs`.
    // let mut pending = vec![signature];
    // let mut units = 0u64;
    // while let Some(current) = pending.pop() {
    //     match current.value.as_ref() {
    //         Some(CostSignatureValue::Ground(_))
    //         | Some(CostSignatureValue::Name(_))
    //         | Some(CostSignatureValue::Quote(_)) => units = units.saturating_add(1),
    //         Some(CostSignatureValue::Compound(compound)) => {
    //             pending.extend(compound.elements.iter())
    //         }
    //         Some(CostSignatureValue::Unit(_)) | Some(CostSignatureValue::BoundLevel(_)) | None => {}
    //     }
    // }
    // units
    signature_leaf_units(signature, &EstimateUnits)
        .expect("the unmetered unit walk fails only when its stack does not fit in memory")
}

/// Added by dedup (task 11415, D8): the unit meter of `signature_units`. It
/// counts every leaf with a saturating sum, and a node that is not a leaf or
/// a compound adds nothing.
struct EstimateUnits;

impl SignatureUnitsMeter for EstimateUnits {
    type Error = InterpreterError;

    fn other(&self) -> Result<(), Self::Error> { Ok(()) }

    fn add(&self, units: u64) -> Result<u64, Self::Error> { Ok(units.saturating_add(1)) }

    fn allocation(&self) -> Self::Error {
        InterpreterError::ReduceError("the stack of a unit walk does not fit in memory".to_string())
    }
}

/// The field length of the region of `signature` inside a `CostAuthority`,
/// from the reducer's own region constructor. The identity is a 32-byte hash
/// for every entropy, so a zero entropy gives the run-time length.
fn region_field_bytes(signature: &CostSignature, discriminator: u32) -> Option<u64> {
    cost_region(signature, &[0; 32], discriminator)
        .ok()
        .and_then(|region| message_field_bytes(1, byte_count(region.encoded_len())).ok())
}

/// The fresh name of a fresh serial: a private name with a 32-byte identity.
/// Distinct serials get distinct identities.
fn fresh_name(serial: usize) -> Par {
    let mut id = vec![0xfe; FRESH_ID_BYTES];
    id[1..9].copy_from_slice(&(serial as u64).to_le_bytes());
    private_name(id)
}

/// The value that `util::allocate_new_bindings` binds for an injection.
fn injection_value(value: &Par) -> Option<Par> {
    if let Some(unforgeable) = RhoUnforgeable::unapply(value) {
        let instance = unforgeable.unf_instance?;
        return Some(Par::default().with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(instance),
        }]));
    }
    let expression = RhoExpression::unapply(value)?;
    let instance = expression.expr_instance?;
    Some(Par::default().with_exprs(vec![Expr {
        expr_instance: Some(instance),
    }]))
}

/// The de Bruijn index of a term that is exactly one bound variable.
fn bare_level(par: &Par) -> Option<i32> {
    let [Expr {
        expr_instance:
            Some(ExprInstance::EVarBody(EVar {
                v:
                    Some(Var {
                        var_instance: Some(VarInstance::BoundVar(level)),
                    }),
            })),
    }] = par.exprs.as_slice()
    else {
        return None;
    };
    let only = par.sends.is_empty()
        && par.receives.is_empty()
        && par.news.is_empty()
        && par.matches.is_empty()
        && par.unforgeables.is_empty()
        && par.bundles.is_empty()
        && par.connectives.is_empty()
        && par.conditionals.is_empty()
        && par.cost_signed_terms.is_empty()
        && par.cost_stacks.is_empty();
    only.then_some(*level)
}

/// True when `eval_single_expr` would accept `par`: one expression and no
/// process.
fn single_expression(par: &Par) -> bool {
    par.exprs.len() == 1
        && par.sends.is_empty()
        && par.receives.is_empty()
        && par.news.is_empty()
        && par.matches.is_empty()
        && par.unforgeables.is_empty()
        && par.bundles.is_empty()
}

/// The `Par` operands that `eval_expr_to_expr` evaluates for one expression
/// form, or `None` for a form without a known work limit (arbitration H2) or
/// with an empty operand slot. The operands come from the shared operand
/// slots.
fn evaluated_operands(expr: &Expr) -> Option<Vec<&Par>> {
    match expr.expr_instance.as_ref()? {
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
        | ExprInstance::GUint8(_)
        | ExprInstance::EVarBody(_)
        | ExprInstance::ENotBody(_)
        | ExprInstance::ENegBody(_)
        | ExprInstance::EMultBody(_)
        | ExprInstance::EDivBody(_)
        | ExprInstance::EModBody(_)
        | ExprInstance::EPlusBody(_)
        | ExprInstance::EMinusBody(_)
        | ExprInstance::ELtBody(_)
        | ExprInstance::ELteBody(_)
        | ExprInstance::EGtBody(_)
        | ExprInstance::EGteBody(_)
        | ExprInstance::EEqBody(_)
        | ExprInstance::ENeqBody(_)
        | ExprInstance::EAndBody(_)
        | ExprInstance::EOrBody(_)
        | ExprInstance::EPlusPlusBody(_)
        | ExprInstance::EPercentPercentBody(_)
        | ExprInstance::EListBody(_)
        | ExprInstance::ETupleBody(_)
        | ExprInstance::ESetBody(_)
        | ExprInstance::EMapBody(_) => expr_operands(expr)
            .into_iter()
            .map(|(_, par)| par)
            .collect(),
        ExprInstance::EMethodBody(_)
        | ExprInstance::EPathmapBody(_)
        | ExprInstance::EZipperBody(_)
        | ExprInstance::EMatchesBody(_)
        | ExprInstance::EMinusMinusBody(_) => None,
    }
}

/// The class and growth of one expression from its evaluated operands, and
/// the loop work of a `%%`. `None` marks a combination without a known work
/// limit (arbitration H2).
fn classify_expr(expr: &Expr, operands: &[Evaluated], env_weight: u64) -> Option<Evaluated> {
    let value = |class: ValueClass, growth: u64| Some(Evaluated { class, growth });
    let operand_growth = operands
        .iter()
        .fold(0u64, |total, operand| total.saturating_add(operand.growth));
    let classes = || operands.iter().map(|operand| operand.class);
    match expr.expr_instance.as_ref()? {
        ExprInstance::GBool(_) => value(ValueClass::Bool, 0),
        ExprInstance::GInt(_) => value(ValueClass::Int(0), 0),
        ExprInstance::GUint64(_) => value(ValueClass::Int(1), 0),
        ExprInstance::GInt32(_) => value(ValueClass::Int(2), 0),
        ExprInstance::GUint32(_) => value(ValueClass::Int(3), 0),
        ExprInstance::GUint16(_) => value(ValueClass::Int(4), 0),
        ExprInstance::GUint8(_) => value(ValueClass::Int(5), 0),
        ExprInstance::GDouble(_) => value(ValueClass::Float(0), 0),
        ExprInstance::GFloat32(_) => value(ValueClass::Float(1), 0),
        ExprInstance::GString(text) => value(ValueClass::Str(byte_count(text.len())), 0),
        ExprInstance::GByteArray(bytes) => value(ValueClass::Bytes(byte_count(bytes.len())), 0),
        ExprInstance::GUri(_)
        | ExprInstance::GBigInt(_)
        | ExprInstance::GBigRat(_)
        | ExprInstance::GFixedPoint(_) => value(ValueClass::Other, 0),
        ExprInstance::EVarBody(_) => value(ValueClass::Other, env_weight),
        ExprInstance::ENotBody(_) => match classes().collect::<Vec<_>>().as_slice() {
            [ValueClass::Bool] => value(ValueClass::Bool, operand_growth),
            _ => None,
        },
        ExprInstance::ENegBody(_) => match classes().collect::<Vec<_>>().as_slice() {
            [class @ (ValueClass::Int(0 | 2) | ValueClass::Float(_))] => {
                value(*class, operand_growth)
            }
            _ => None,
        },
        ExprInstance::EMultBody(_)
        | ExprInstance::EDivBody(_)
        | ExprInstance::EModBody(_)
        | ExprInstance::EPlusBody(_)
        | ExprInstance::EMinusBody(_) => match classes().collect::<Vec<_>>().as_slice() {
            [ValueClass::Int(left), ValueClass::Int(right)] if left == right => {
                value(ValueClass::Int(*left), operand_growth)
            }
            [ValueClass::Float(left), ValueClass::Float(right)] if left == right => {
                value(ValueClass::Float(*left), operand_growth)
            }
            _ => None,
        },
        ExprInstance::ELtBody(_)
        | ExprInstance::ELteBody(_)
        | ExprInstance::EGtBody(_)
        | ExprInstance::EGteBody(_) => match classes().collect::<Vec<_>>().as_slice() {
            [ValueClass::Bool, ValueClass::Bool] | [ValueClass::Str(_), ValueClass::Str(_)] => {
                value(ValueClass::Bool, operand_growth)
            }
            [ValueClass::Int(left), ValueClass::Int(right)] if left == right => {
                value(ValueClass::Bool, operand_growth)
            }
            [ValueClass::Float(left), ValueClass::Float(right)] if left == right => {
                value(ValueClass::Bool, operand_growth)
            }
            _ => None,
        },
        ExprInstance::EEqBody(_) | ExprInstance::ENeqBody(_) => {
            value(ValueClass::Bool, operand_growth)
        }
        ExprInstance::EAndBody(_) | ExprInstance::EOrBody(_) => {
            match classes().collect::<Vec<_>>().as_slice() {
                [ValueClass::Bool, ValueClass::Bool] => value(ValueClass::Bool, operand_growth),
                _ => None,
            }
        }
        ExprInstance::EPlusPlusBody(_) => match classes().collect::<Vec<_>>().as_slice() {
            [ValueClass::Str(left), ValueClass::Str(right)] => {
                value(ValueClass::Str(left.saturating_add(*right)), operand_growth)
            }
            [ValueClass::Bytes(left), ValueClass::Bytes(right)] => value(
                ValueClass::Bytes(left.saturating_add(*right)),
                operand_growth,
            ),
            [ValueClass::List, ValueClass::List] => value(ValueClass::List, operand_growth),
            _ => None,
        },
        ExprInstance::EPercentPercentBody(body) => {
            let operand_classes = classes().collect::<Vec<_>>();
            let [ValueClass::Str(template), _] = operand_classes.as_slice() else {
                return None;
            };
            let (_, _, value_bytes) = interpolation_map(body.p2.as_ref()?)?;
            let template = *template;
            let output = template
                .saturating_add((template / 3).saturating_mul(value_bytes))
                .saturating_add(16);
            Some(Evaluated {
                class: ValueClass::Str(output),
                growth: operand_growth.saturating_add(output),
            })
        }
        ExprInstance::EListBody(_) => value(ValueClass::List, operand_growth),
        ExprInstance::ETupleBody(_) | ExprInstance::ESetBody(_) | ExprInstance::EMapBody(_) => {
            value(ValueClass::Other, operand_growth)
        }
        ExprInstance::EMethodBody(_)
        | ExprInstance::EPathmapBody(_)
        | ExprInstance::EZipperBody(_)
        | ExprInstance::EMatchesBody(_)
        | ExprInstance::EMinusMinusBody(_) => None,
    }
}

/// The key count, the longest key and the longest rendered value of a
/// literal interpolation map: string keys and string, integer, boolean or URI
/// values, as `eval_to_string_pair` accepts them.
fn interpolation_map(par: &Par) -> Option<(u64, u64, u64)> {
    if !single_expression(par) {
        return None;
    }
    let Some(ExprInstance::EMapBody(map)) = par.exprs.first()?.expr_instance.as_ref() else {
        return None;
    };
    let mut key_bytes = 0u64;
    let mut value_bytes = 0u64;
    for pair in &map.kvs {
        let key = literal(pair.key.as_ref()?)?;
        let ExprInstance::GString(key) = key else {
            return None;
        };
        key_bytes = key_bytes.max(byte_count(key.len()));
        let rendered = match literal(pair.value.as_ref()?)? {
            ExprInstance::GString(text) | ExprInstance::GUri(text) => byte_count(text.len()),
            ExprInstance::GInt(_) => 20,
            ExprInstance::GBool(_) => 5,
            _ => return None,
        };
        value_bytes = value_bytes.max(rendered);
    }
    Some((byte_count(map.kvs.len()), key_bytes, value_bytes))
}

fn literal(par: &Par) -> Option<&ExprInstance> {
    match single_expression(par) {
        true => par.exprs.first()?.expr_instance.as_ref(),
        false => None,
    }
}

// Moved by DR-131 P2b to `models/src/rust/rholang/dismantle.rs`, without change,
// so that the cold-store decoder in `models` releases a partly decoded value
// without recursion. Both functions keep their paths here through this
// re-export.
pub use models::rust::rholang::dismantle::{dismantle_term, dismantle_terms};

#[cfg(test)]
mod recursive_oracle;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod stack_safety_tests;

#[cfg(test)]
mod runtime_tests;

#[cfg(test)]
mod prepared_entry_tests {
    use super::*;
    use crate::rust::interpreter::frontend::PreparedProgram;

    #[test]
    fn prepared_estimate_matches_source_estimate() {
        let evaluator = tests::evaluator(HashMap::new());
        let schedule = tests::schedule([1, 2, 3, 4]);
        let funding = tests::funding();
        for source in [
            "Nil",
            "new x in { x!(0) }",
            "new x in { x!(0) | for (@n <- x) { Nil } }",
            "if (true) { Nil } else { Nil }",
        ] {
            let rand = || Blake2b512Random::create_from_bytes(b"prepared estimate");
            let expected = estimate_offered_term(
                source,
                HashMap::new(),
                rand(),
                &funding,
                &schedule,
                &evaluator,
            )
            .expect("source estimate");
            let parsed =
                InterpreterImpl::parse_source(source, HashMap::new()).expect("prepared term");
            let actual = estimate_offered_prepared(
                PreparedProgram::from_normalized(parsed),
                rand(),
                &funding,
                &schedule,
                &evaluator,
            )
            .expect("prepared estimate");
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn prepared_estimate_preserves_name_resolution_failure() {
        let evaluator = tests::evaluator(HashMap::new());
        let schedule = tests::schedule([1; 4]);
        let funding = tests::funding();
        let source = "new x(`rho:missing`) in { x!(0) }";
        let rand = || Blake2b512Random::create_from_bytes(b"prepared estimate rejection");
        let expected = estimate_offered_term(
            source,
            HashMap::new(),
            rand(),
            &funding,
            &schedule,
            &evaluator,
        )
        .expect_err("unbound runtime name");
        let parsed = InterpreterImpl::parse_source(source, HashMap::new()).expect("prepared term");
        let actual = estimate_offered_prepared(
            PreparedProgram::from_normalized(parsed),
            rand(),
            &funding,
            &schedule,
            &evaluator,
        )
        .expect_err("same unbound runtime name");
        assert_eq!(actual.to_string(), expected.to_string());
    }
}
