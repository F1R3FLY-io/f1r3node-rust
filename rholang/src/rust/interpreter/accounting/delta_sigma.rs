//! Per-signature demand analyzer `Δ_s` and supply closure `Σ_s`. This is the
//! pure, linear-time structural proof producer for cost-accounted Rho
//! (Definitions 17–19 and Theorem 20 of `cost-accounted-rho.tex`). Native
//! `CostSignedTerm` regions survive normalization, substitution, storage, and
//! replay, so the analysis maintains the complete stack of enclosing authority
//! regions and attributes each potential communication introduction to every
//! region that the runtime would attach to that interaction.
//!
//! A canonical well-wrapped redex has one signed layer around one interaction,
//! making the structural count coincide with Definition 17. Native Rholang also
//! accepts a wider surface form in which one signed scope encloses several
//! communication introductions. That form is conservatively bounded by every
//! possible firing of the shared region. This is essential because independent
//! ambient partners can cause the introductions to participate in distinct
//! COMMs. It may over-reserve, but it cannot under-reserve; exact production
//! settlement refunds the unused suffix.
//!
//! ## The load-bearing equivalence (consensus-critical, gate↔runtime bridge)
//!
//! The static `Δ_s` is a certified finite upper bound on the runtime's realized
//! atomic-COMM authority debit for the non-persistent, statically resolvable
//! fragment. Parallel components add; mutually exclusive `match` and `if`
//! branches take their component-wise maximum. A receive continuation begins a
//! fresh authority scope, matching the runtime's force-by-unwrapping dispatch;
//! a nested signed term extends the current scope. Persistent I/O, dynamic
//! authority, and unresolved dequotation are unprovable structurally.
//! Production admission uses exact state-bound execution evidence rooted in the
//! authenticated pre-state, and replay independently reproduces the event set,
//! physical settlement, and post-state root.
//!
//! D3 (DR-9 one-token-per-COMM, OD-3): the RSpace match observer emits one
//! `BillableTokenEvent{kind: Comm}` only after a complete binary or join match
//! has been selected and before its state mutation becomes visible. Structural
//! reducer events (`eval_send`, `eval_receive`, `eval_new`, `eval_match`,
//! `eval_if`) remain diagnostic and contribute zero to consensus cost. Thus the
//! static Send+Receive count is a conservative reservation, not the realized
//! cost and not an event-by-event dual of the runtime trace.
//!
//! ## `?!` / uniform-signing desugaring (§7.4 — "8 not 6")
//!
//! The §7.4 semantic count requires the synchronous-send sugar `x?!(args)` to be
//! expanded to `new ret in { x!(ret, args) | for(_ <- ret){ cont } }` — a send +
//! a for-comprehension on each side — so the count reflects the desugared form
//! the runtime executes (8), not the syntactic signed-layer count (6). The
//! f1r3node normalizer ALREADY performs this expansion: `?!` is desugared by
//! `compiler/normalizer/processes/p_send_sync_normalizer.rs` at normalization
//! time, so a normalized `Par` passed to [`demand`] already contains the
//! desugared send + for nodes. [`desugar_for_funding`] therefore does NOT
//! re-expand `?!` (that would double-count); it is the identity on an
//! already-normalized `Par` and exists to make the desugar contract explicit at
//! the funding boundary (see its doc comment). Uniform signing likewise needs no
//! expansion here: `CostSignedTerm` nodes and signed receive binds already carry
//! the normalized authority structure.
//!
//! ## Purity
//!
//! This module is PURE and linear-time: it operates on `Par` + `Sig` + integer
//! supply maps only — no RSpace, no async, no I/O. The native gate constructs
//! each raw `Σ_s` from the signature's canonical SystemVault balance and
//! available located stacks, then feeds the integer projection into this module
//! as a `BTreeMap<SigKey, i64>`.

use std::collections::BTreeMap;

use models::rhoapi::cost_signature::Value as CostSignatureValue;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    Bundle, CostSignature, CostSignedTerm, CostStack, If, Match, New, Par, Receive, Send,
};
use models::rust::rholang::sorter::cost_accounting_sorter::sort_signature;
use prost::Message;

use super::authority::{
    canonical_cost_signature, cost_signature_to_sig, AuthorityError, DemandBound, ResourceMultiset,
    UnprovableDemand,
};
use super::Sig;

/// Canonical per-signature map key. Equal to `Sig::lane_hash` — the SAME
/// canonical, axis-independent, permutation-invariant digest WD-D0's lane pool
/// (`accounting/mod.rs`) keys lanes by and that StageB's `supply_channel`
/// (`SignatureChannel::from_sig`) anchors located authority stacks to. Keying
/// the `effective_supply` map by this digest means the gate's per-group
/// authority lookups, runtime lane keys, and stack channels all agree.
pub type SigKey = [u8; 32];

/// Compute the canonical `SigKey` for a signature (its `Sig::lane_hash`). Thin
/// re-export so callers (the D2 gate, the supply consumer) key the supply map by
/// the same basis without reaching into `accounting/mod.rs` internals.
#[inline]
pub fn sig_key(sig: &Sig) -> SigKey { sig.lane_hash() }

/// Match a resolved COMM channel against the installed signer channels, returning
/// the matched signer's lane key or `None`. This is the ONE shared channel→lane
/// decision used by BOTH the static dual ([`demand_by_sig`]) and the runtime
/// per-redex attribution (`metering::MeteredMachine`'s channel match), so the two
/// can never drift — the consensus bridge (W1 Phase 3 §3.1). `signer_channels`
/// are `(SignatureChannel::from_sig(signer).par.encode_to_vec(), signer.lane_hash())`
/// pairs, i.e. the wire encoding of the envelope's [`Sig::signer_channels`]. A
/// channel matching NO installed signer returns `None`, and the caller attributes
/// that COMM to the envelope — never inventing a foreign lane (§3.4).
///
/// [`Sig::signer_channels`]: super::Sig::signer_channels
pub fn match_channel_to_lane(
    channel: &Par,
    signer_channels: &[(Vec<u8>, SigKey)],
) -> Option<SigKey> {
    let encoded = channel.encode_to_vec();
    signer_channels
        .iter()
        .find(|(chan_bytes, _)| chan_bytes.as_slice() == encoded.as_slice())
        .map(|(_, lane)| *lane)
}

/// The static demand analysis result for one signature `s` over a desugared
/// `Par` (cost-accounted-rho Def 17 + Thm 20 over-approximation).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DemandEntry {
    /// `certified_upper_bound = Δ_s` over the statically-resolvable term:
    /// the number of potential communication introductions (send / receive
    /// ONLY; new / match / if are excluded) attributed to `s`. Each introduction
    /// contributes once to every enclosing native signed region. Unsigned
    /// introductions contribute to the envelope lane. `i64` matches the supply
    /// unit (`Σ_s`, a balance), so the funding comparison is one integer
    /// inequality in identical units.
    pub certified_upper_bound: i64,
    /// `true` iff the term contains an unresolvable dequotation `*x` or
    /// persistent I/O. Either can reuse behavior beyond a finite syntax-node
    /// count, so no structural finite upper bound has been proved. A finite
    /// safety margin cannot repair that missing proof, and the structural gate
    /// rejects it; production instead relies on exact state-bound evidence.
    pub unknown: bool,
}

impl DemandEntry {
    /// The zero demand (no communication introductions, fully resolvable). Identity for
    /// [`DemandEntry::combine`].
    pub const ZERO: DemandEntry = DemandEntry {
        certified_upper_bound: 0,
        unknown: false,
    };

    /// Parallel/sequential composition of two sub-results: demands add
    /// (Def 17 `Δ_s(T | U) = Δ_s(T) + Δ_s(U)`), and `unknown` is sticky (any
    /// unresolvable sub-term makes the whole term's demand an over-approximation).
    /// Saturating add keeps the bound well-defined even for adversarially huge
    /// ASTs (the AST size is bounded upstream by the term-count limit in
    /// `reduce.rs::eval_inner`, but saturation is the safe direction regardless).
    #[inline]
    fn combine(self, other: DemandEntry) -> DemandEntry {
        DemandEntry {
            certified_upper_bound: self
                .certified_upper_bound
                .saturating_add(other.certified_upper_bound),
            unknown: self.unknown || other.unknown,
        }
    }

    #[inline]
    fn alternative(self, other: DemandEntry) -> DemandEntry {
        DemandEntry {
            certified_upper_bound: self.certified_upper_bound.max(other.certified_upper_bound),
            unknown: self.unknown || other.unknown,
        }
    }

    /// Add one potential communication introduction to the certified upper bound.
    #[inline]
    fn plus_one(self) -> DemandEntry {
        DemandEntry {
            certified_upper_bound: self.certified_upper_bound.saturating_add(1),
            unknown: self.unknown,
        }
    }
}

/// `Δ(desugared)` — the total token demand of a fully-desugared `Par` across all
/// native signed regions and the unsigned envelope lane. Returns a finite upper
/// bound plus an `unknown` flag set when the structural proof cannot establish a
/// finite bound (Theorem 20).
///
/// Linear time in the size of the AST: a single structural pass, O(1) work per
/// node, no normalization or fixpoint.
///
/// Counted nodes are `Send` and `Receive`; they are potential participants, not
/// runtime `Comm` events. `New`, `Match`, and `If` are recursed but do not add a
/// unit. An `EVarBody` in process position that is a `bound_var` / `free_var`,
/// or any persistent send/receive, makes the structural result unprovable.
///
/// `deploy_sig` supplies the authority lane only for introductions outside an
/// explicit `CostSignedTerm` or signed receive clause. Explicit nested regions
/// retain their own canonical lanes.
pub fn demand(desugared: &Par, deploy_sig: &Sig) -> DemandEntry {
    let analysis = signed_demand_par(desugared, deploy_sig.lane_hash(), true);
    DemandEntry {
        certified_upper_bound: analysis.lanes.values().fold(0i64, |total, entry| {
            total.saturating_add(entry.certified_upper_bound)
        }),
        unknown: analysis.unprovable.is_some(),
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct SignedDemand {
    lanes: BTreeMap<SigKey, DemandEntry>,
    transfer_lanes: BTreeMap<SigKey, DemandEntry>,
    guaranteed_supply: ResourceMultiset<SigKey>,
    unprovable: Option<UnprovableDemand>,
    has_introduction: bool,
}

impl SignedDemand {
    fn add_lane(&mut self, lane: SigKey) {
        bump_lane(&mut self.lanes, lane, DemandEntry::ZERO.plus_one());
    }

    fn combine(mut self, other: SignedDemand) -> SignedDemand {
        add_lane_demands(&mut self.lanes, other.lanes);
        add_lane_demands(&mut self.transfer_lanes, other.transfer_lanes);
        self.guaranteed_supply = match self.guaranteed_supply.checked_add(&other.guaranteed_supply)
        {
            Ok(supply) => supply,
            Err(_) => {
                self.reject(UnprovableDemand::UnsupportedSyntax);
                ResourceMultiset::default()
            }
        };
        if self.unprovable.is_none() {
            self.unprovable = other.unprovable;
        }
        self.has_introduction |= other.has_introduction;
        self
    }

    fn alternative(mut self, other: SignedDemand) -> SignedDemand {
        merge_alternative_lanes(&mut self.lanes, other.lanes);
        merge_alternative_lanes(&mut self.transfer_lanes, other.transfer_lanes);
        self.guaranteed_supply.0.retain(|lane, amount| {
            let other_amount = other.guaranteed_supply.get(lane);
            *amount = (*amount).min(other_amount);
            *amount > 0
        });
        if self.unprovable.is_none() {
            self.unprovable = other.unprovable;
        }
        self.has_introduction |= other.has_introduction;
        self
    }

    fn reject(&mut self, reason: UnprovableDemand) {
        if self.unprovable.is_none() {
            self.unprovable = Some(reason);
        }
    }
}

#[derive(Clone, Copy)]
struct ScopeLink {
    parent: Option<usize>,
    lane: Option<SigKey>,
}

fn add_scope_demand(
    result: &mut SignedDemand,
    deploy_key: SigKey,
    scopes: &[ScopeLink],
    scope: Option<usize>,
) {
    if scope.is_none() {
        result.add_lane(deploy_key);
    } else {
        let mut current = scope;
        while let Some(index) = current {
            let link = scopes[index];
            if let Some(lane) = link.lane {
                result.add_lane(lane);
            }
            current = link.parent;
        }
    }
    result.has_introduction = true;
}

fn add_scope_transfer(
    result: &mut SignedDemand,
    deploy_key: SigKey,
    scopes: &[ScopeLink],
    scope: Option<usize>,
) {
    if scope.is_none() {
        bump_lane(
            &mut result.transfer_lanes,
            deploy_key,
            DemandEntry::ZERO.plus_one(),
        );
    } else {
        let mut current = scope;
        while let Some(index) = current {
            let link = scopes[index];
            if let Some(lane) = link.lane {
                bump_lane(
                    &mut result.transfer_lanes,
                    lane,
                    DemandEntry::ZERO.plus_one(),
                );
            }
            current = link.parent;
        }
    }
}

fn signature_lane(signature: &CostSignature) -> Result<Option<SigKey>, UnprovableDemand> {
    cost_signature_to_sig(signature)
        .map(|signature| (signature != Sig::Unit).then(|| signature.lane_hash()))
        .map_err(|error| match error {
            super::authority::AuthorityError::UnresolvedBoundLevel => {
                UnprovableDemand::DynamicAuthority
            }
            _ => UnprovableDemand::UnsupportedSyntax,
        })
}

enum DemandTask<'a> {
    Visit(&'a Par, Option<usize>, bool),
    Signed(&'a CostSignedTerm, Option<usize>),
    SignedFinish(Option<SigKey>),
    Stack(&'a CostStack, Option<usize>, bool),
    Send(&'a Send, Option<usize>),
    Receive(&'a Receive, Option<usize>),
    New(&'a New, Option<usize>, bool),
    Match(&'a Match, Option<usize>, bool),
    Conditional(&'a If, Option<usize>, bool),
    Bundle(&'a Bundle, Option<usize>, bool),
    ScanUnknown(&'a Par),
    Empty,
    Combine,
    Alternative,
}

fn signed_demand_par(par: &Par, deploy_key: SigKey, execution_position: bool) -> SignedDemand {
    let mut scopes = Vec::<ScopeLink>::new();
    let mut work = vec![DemandTask::Visit(par, None, execution_position)];
    let mut values = Vec::<SignedDemand>::new();
    while let Some(task) = work.pop() {
        match task {
            DemandTask::Visit(par, scope, execution) => {
                values.push(SignedDemand::default());
                let mut ordered = Vec::new();
                for term in &par.cost_signed_terms {
                    ordered.push(DemandTask::Signed(term, scope));
                }
                if execution {
                    for stack in &par.cost_stacks {
                        ordered.push(DemandTask::Stack(stack, scope, false));
                    }
                }
                for send in &par.sends {
                    ordered.push(DemandTask::Send(send, scope));
                }
                for receive in &par.receives {
                    ordered.push(DemandTask::Receive(receive, scope));
                }
                for new in &par.news {
                    ordered.push(DemandTask::New(new, scope, execution));
                }
                for mat in &par.matches {
                    ordered.push(DemandTask::Match(mat, scope, execution));
                }
                for conditional in &par.conditionals {
                    ordered.push(DemandTask::Conditional(conditional, scope, execution));
                }
                for bundle in &par.bundles {
                    ordered.push(DemandTask::Bundle(bundle, scope, execution));
                }
                if execution {
                    ordered.push(DemandTask::ScanUnknown(par));
                }
                work.extend(ordered.into_iter().rev());
            }
            DemandTask::Signed(term, scope) => {
                let resolved = match term.signature.as_ref().map(signature_lane) {
                    Some(Ok(lane)) => Some(lane),
                    Some(Err(reason)) => {
                        values.last_mut().expect("parent demand").reject(reason);
                        None
                    }
                    None => {
                        values
                            .last_mut()
                            .expect("parent demand")
                            .reject(UnprovableDemand::UnsupportedSyntax);
                        None
                    }
                };
                match term.body.as_ref() {
                    Some(body) => {
                        if let Some(lane) = resolved {
                            let nested = scopes.len();
                            scopes.push(ScopeLink {
                                parent: scope,
                                lane,
                            });
                            work.push(DemandTask::SignedFinish(lane));
                            work.push(DemandTask::Visit(body, Some(nested), true));
                        }
                    }
                    None => values
                        .last_mut()
                        .expect("parent demand")
                        .reject(UnprovableDemand::UnsupportedSyntax),
                }
            }
            DemandTask::SignedFinish(lane) => {
                let mut child = values.pop().expect("signed body demand");
                if !child.has_introduction {
                    if let Some(lane) = lane {
                        child.add_lane(lane);
                    }
                }
                let parent = values.pop().expect("signed parent demand");
                values.push(parent.combine(child));
            }
            DemandTask::Stack(stack, scope, nested) => {
                let result = values.last_mut().expect("stack parent demand");
                if stack.cells.is_empty() && stack.parallel_tail.is_empty() && !nested {
                    result.reject(UnprovableDemand::UnsupportedSyntax);
                    continue;
                }
                for cell in &stack.cells {
                    match signature_lane(cell) {
                        Ok(Some(lane)) => {
                            add_scope_demand(result, deploy_key, &scopes, scope);
                            add_scope_transfer(result, deploy_key, &scopes, scope);
                            let amount = result.guaranteed_supply.get(&lane);
                            match amount.checked_add(1) {
                                Some(amount) => {
                                    result.guaranteed_supply.0.insert(lane, amount);
                                }
                                None => result.reject(UnprovableDemand::UnsupportedSyntax),
                            }
                        }
                        Ok(None) => result.reject(UnprovableDemand::UnsupportedSyntax),
                        Err(UnprovableDemand::DynamicAuthority) => {
                            add_scope_demand(result, deploy_key, &scopes, scope);
                            add_scope_transfer(result, deploy_key, &scopes, scope);
                        }
                        Err(reason) => result.reject(reason),
                    }
                }
                work.extend(
                    stack
                        .parallel_tail
                        .iter()
                        .rev()
                        .map(|branch| DemandTask::Stack(branch, scope, true)),
                );
            }
            DemandTask::Send(send, scope) => {
                let result = values.last_mut().expect("send parent demand");
                add_scope_demand(result, deploy_key, &scopes, scope);
                if send.persistent {
                    result.reject(UnprovableDemand::UnboundedControlFlow);
                }
                for datum in send.data.iter().rev() {
                    work.push(DemandTask::Combine);
                    work.push(DemandTask::Visit(datum, None, false));
                }
            }
            DemandTask::Receive(receive, scope) => {
                let result = values.last_mut().expect("receive parent demand");
                let signed_binds = receive
                    .binds
                    .iter()
                    .filter(|bind| bind.cost_signature.is_some())
                    .count();
                if scope.is_some() {
                    add_scope_demand(result, deploy_key, &scopes, scope);
                }
                if signed_binds == 0 {
                    if scope.is_none() {
                        add_scope_demand(result, deploy_key, &scopes, scope);
                    }
                } else if signed_binds == receive.binds.len() {
                    for bind in &receive.binds {
                        match bind.cost_signature.as_ref().map(signature_lane) {
                            Some(Ok(Some(lane))) => result.add_lane(lane),
                            Some(Ok(None)) => {}
                            Some(Err(reason)) => result.reject(reason),
                            None => result.reject(UnprovableDemand::UnsupportedSyntax),
                        }
                    }
                } else {
                    result.reject(UnprovableDemand::UnsupportedSyntax);
                }
                result.has_introduction = true;
                if receive.persistent {
                    result.reject(UnprovableDemand::UnboundedControlFlow);
                }
                if let Some(body) = receive.body.as_ref() {
                    work.push(DemandTask::Combine);
                    work.push(DemandTask::Visit(body, None, true));
                }
            }
            DemandTask::New(new, scope, execution) => {
                if let Some(body) = new.p.as_ref() {
                    work.push(DemandTask::Combine);
                    work.push(DemandTask::Visit(body, scope, execution));
                }
            }
            DemandTask::Match(mat, scope, execution) => {
                let mut ordered = Vec::new();
                for source in mat.cases.iter().filter_map(|case| case.source.as_ref()) {
                    if !ordered.is_empty() {
                        ordered.push(DemandTask::Visit(source, scope, execution));
                        ordered.push(DemandTask::Alternative);
                    } else {
                        ordered.push(DemandTask::Visit(source, scope, execution));
                    }
                }
                if !ordered.is_empty() {
                    ordered.push(DemandTask::Combine);
                    work.extend(ordered.into_iter().rev());
                }
            }
            DemandTask::Conditional(conditional, scope, execution) => {
                work.push(DemandTask::Combine);
                work.push(DemandTask::Alternative);
                match conditional.if_false.as_ref() {
                    Some(branch) => work.push(DemandTask::Visit(branch, scope, execution)),
                    None => work.push(DemandTask::Empty),
                }
                match conditional.if_true.as_ref() {
                    Some(branch) => work.push(DemandTask::Visit(branch, scope, execution)),
                    None => work.push(DemandTask::Empty),
                }
            }
            DemandTask::Bundle(bundle, scope, execution) => {
                if let Some(body) = bundle.body.as_ref() {
                    work.push(DemandTask::Combine);
                    work.push(DemandTask::Visit(body, scope, execution));
                }
            }
            DemandTask::ScanUnknown(par) => {
                let result = values.last_mut().expect("expression parent demand");
                for expr in &par.exprs {
                    if let Some(ExprInstance::EVarBody(evar)) = &expr.expr_instance {
                        if let Some(var) = &evar.v {
                            if matches!(
                                var.var_instance,
                                Some(VarInstance::BoundVar(_)) | Some(VarInstance::FreeVar(_))
                            ) {
                                result.reject(UnprovableDemand::RecursiveDequotation);
                            }
                        }
                    }
                }
            }
            DemandTask::Empty => values.push(SignedDemand::default()),
            DemandTask::Combine => {
                let child = values.pop().expect("child demand");
                let parent = values.pop().expect("parent demand");
                values.push(parent.combine(child));
            }
            DemandTask::Alternative => {
                let right = values.pop().expect("right branch demand");
                let left = values.pop().expect("left branch demand");
                values.push(left.alternative(right));
            }
        }
    }
    values.pop().expect("root demand")
}

pub fn static_authority_signatures(
    par: &Par,
) -> Result<BTreeMap<SigKey, CostSignature>, AuthorityError> {
    fn has_bound_level(signature: &CostSignature) -> Result<bool, AuthorityError> {
        let mut work = vec![signature];
        let mut dynamic = false;
        while let Some(current) = work.pop() {
            match current.value.as_ref() {
                Some(CostSignatureValue::BoundLevel(_)) => dynamic = true,
                Some(CostSignatureValue::Compound(compound)) if compound.elements.len() >= 2 => {
                    work.extend(compound.elements.iter().rev());
                }
                Some(CostSignatureValue::Compound(_)) => {
                    return Err(AuthorityError::MalformedCompound)
                }
                Some(CostSignatureValue::Unit(false)) => {
                    return Err(AuthorityError::NonCanonicalSignature)
                }
                Some(_) => {
                    canonical_cost_signature(current)?;
                }
                None => return Err(AuthorityError::MissingSignature),
            }
        }
        Ok(dynamic)
    }

    fn insert(
        signatures: &mut BTreeMap<SigKey, CostSignature>,
        signature: &CostSignature,
    ) -> Result<(), AuthorityError> {
        if has_bound_level(signature)? {
            if sort_signature(signature).term != *signature {
                return Err(AuthorityError::NonCanonicalSignature);
            }
            return Ok(());
        }
        let signature = canonical_cost_signature(signature)?;
        let runtime_signature = cost_signature_to_sig(&signature)?;
        if runtime_signature == Sig::Unit {
            return Ok(());
        }
        let key = runtime_signature.lane_hash();
        match signatures.get(&key) {
            Some(existing) if existing != &signature => Err(AuthorityError::EventSignatureConflict),
            Some(_) => Ok(()),
            None => {
                signatures.insert(key, signature);
                Ok(())
            }
        }
    }

    enum CollectTask<'a> {
        Par(&'a Par),
        RequiredSignature(Option<&'a CostSignature>),
        RequiredBody(Option<&'a Par>),
        Stack(&'a CostStack, bool),
    }

    fn collect(
        par: &Par,
        signatures: &mut BTreeMap<SigKey, CostSignature>,
    ) -> Result<(), AuthorityError> {
        let mut work = vec![CollectTask::Par(par)];
        while let Some(task) = work.pop() {
            match task {
                CollectTask::Par(par) => {
                    let mut ordered = Vec::new();
                    for term in &par.cost_signed_terms {
                        ordered.push(CollectTask::RequiredSignature(term.signature.as_ref()));
                        ordered.push(CollectTask::RequiredBody(term.body.as_ref()));
                    }
                    for stack in &par.cost_stacks {
                        ordered.push(CollectTask::Stack(stack, false));
                    }
                    for send in &par.sends {
                        for datum in &send.data {
                            ordered.push(CollectTask::Par(datum));
                        }
                    }
                    for receive in &par.receives {
                        for bind in &receive.binds {
                            if let Some(signature) = &bind.cost_signature {
                                ordered.push(CollectTask::RequiredSignature(Some(signature)));
                            }
                        }
                        if let Some(body) = &receive.body {
                            ordered.push(CollectTask::Par(body));
                        }
                    }
                    for new in &par.news {
                        if let Some(body) = &new.p {
                            ordered.push(CollectTask::Par(body));
                        }
                    }
                    for mat in &par.matches {
                        for case in &mat.cases {
                            if let Some(source) = &case.source {
                                ordered.push(CollectTask::Par(source));
                            }
                        }
                    }
                    for conditional in &par.conditionals {
                        if let Some(branch) = &conditional.if_true {
                            ordered.push(CollectTask::Par(branch));
                        }
                        if let Some(branch) = &conditional.if_false {
                            ordered.push(CollectTask::Par(branch));
                        }
                    }
                    for bundle in &par.bundles {
                        if let Some(body) = &bundle.body {
                            ordered.push(CollectTask::Par(body));
                        }
                    }
                    work.extend(ordered.into_iter().rev());
                }
                CollectTask::RequiredSignature(signature) => {
                    insert(
                        signatures,
                        signature.ok_or(AuthorityError::MissingSignature)?,
                    )?;
                }
                CollectTask::RequiredBody(body) => {
                    work.push(CollectTask::Par(
                        body.ok_or(AuthorityError::MissingAuthority)?,
                    ));
                }
                CollectTask::Stack(stack, nested) => {
                    if stack.cells.is_empty() && stack.parallel_tail.is_empty() && !nested {
                        return Err(AuthorityError::MissingSignature);
                    }
                    for cell in &stack.cells {
                        insert(signatures, cell)?;
                    }
                    work.extend(
                        stack
                            .parallel_tail
                            .iter()
                            .rev()
                            .map(|branch| CollectTask::Stack(branch, true)),
                    );
                }
            }
        }
        Ok(())
    }

    let mut signatures = BTreeMap::new();
    collect(par, &mut signatures)?;
    Ok(signatures)
}

pub fn demand_bound(desugared: &Par, deploy_sig: &Sig) -> DemandBound<SigKey> {
    let analysis = signed_demand_par(desugared, deploy_sig.lane_hash(), true);
    if let Some(reason) = analysis.unprovable {
        return DemandBound::Unprovable(reason);
    }
    let mut bound = ResourceMultiset::default();
    for (lane, entry) in analysis.lanes {
        let amount = match u64::try_from(entry.certified_upper_bound) {
            Ok(amount) => amount,
            Err(_) => return DemandBound::Unprovable(UnprovableDemand::UnsupportedSyntax),
        };
        if amount > 0 {
            bound.0.insert(lane, amount);
        }
    }
    DemandBound::FiniteUpperBound {
        bound,
        proof: b"rho-native-wrapping-upper-bound-v2".to_vec(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaticAuthorityPlan {
    pub demand: ResourceMultiset<SigKey>,
    pub transfer_demand: ResourceMultiset<SigKey>,
    pub guaranteed_program_supply: ResourceMultiset<SigKey>,
    pub external_reservation: ResourceMultiset<SigKey>,
}

pub fn static_authority_plan(
    desugared: &Par,
    deploy_sig: &Sig,
) -> Result<StaticAuthorityPlan, UnprovableDemand> {
    fn lanes_to_resources(
        lanes: BTreeMap<SigKey, DemandEntry>,
    ) -> Result<ResourceMultiset<SigKey>, UnprovableDemand> {
        let mut resources = ResourceMultiset::default();
        for (lane, entry) in lanes {
            let amount = u64::try_from(entry.certified_upper_bound)
                .map_err(|_| UnprovableDemand::UnsupportedSyntax)?;
            if amount > 0 {
                resources.0.insert(lane, amount);
            }
        }
        Ok(resources)
    }

    let analysis = signed_demand_par(desugared, deploy_sig.lane_hash(), true);
    if let Some(reason) = analysis.unprovable {
        return Err(reason);
    }
    let demand = lanes_to_resources(analysis.lanes)?;
    let transfer_demand = lanes_to_resources(analysis.transfer_lanes)?;
    let cost_demand = demand
        .checked_sub(&transfer_demand)
        .map_err(|_| UnprovableDemand::UnsupportedSyntax)?;
    let mut residual_cost = ResourceMultiset::default();
    for (lane, amount) in &cost_demand.0 {
        let residual = amount.saturating_sub(analysis.guaranteed_supply.get(lane));
        if residual > 0 {
            residual_cost.0.insert(*lane, residual);
        }
    }
    let external_reservation = transfer_demand
        .checked_add(&residual_cost)
        .map_err(|_| UnprovableDemand::UnsupportedSyntax)?;
    Ok(StaticAuthorityPlan {
        demand,
        transfer_demand,
        guaranteed_program_supply: analysis.guaranteed_supply,
        external_reservation,
    })
}

/// Per-`SigKey` static demand — the multi-lane generalization of [`demand`] (W1
/// Phase 3 §3.1). Walks the SAME process/name positions as [`demand_par`] (so the
/// per-lane counts sum back to [`demand`]), but buckets each potential
/// introduction by `region_sig(channel)`: a channel that matches an installed
/// signer channel attributes to that signer's lane; every other COMM — and the
/// `unknown` over-approximation — attributes to `envelope_key` (§3.4: never a
/// foreign lane).
///
/// `region_sig` MUST use the runtime's channel projection — [`match_channel_to_lane`]
/// bound to the same `signer_channels` the RSpace observer uses — so static and
/// realized lane labels have one interpretation. The structural counts remain a
/// conservative introduction bound, not an event-for-event runtime trace. This
/// compatibility projection attributes ordinary data-channel introductions to
/// `envelope_key`; native `CostSignedTerm` authority is analyzed by
/// [`demand_bound`] and realized by the reducer without channel inference.
pub fn demand_by_sig(
    desugared: &Par,
    envelope_key: SigKey,
    region_sig: &dyn Fn(&Par) -> Option<SigKey>,
) -> BTreeMap<SigKey, DemandEntry> {
    let mut acc: BTreeMap<SigKey, DemandEntry> = BTreeMap::new();
    demand_by_sig_into(desugared, envelope_key, region_sig, &mut acc);
    acc
}

/// Fold one sub-result into a lane's accumulator (Def 17 add; `unknown` sticky).
fn bump_lane(acc: &mut BTreeMap<SigKey, DemandEntry>, lane: SigKey, entry: DemandEntry) {
    let slot = acc.entry(lane).or_insert(DemandEntry::ZERO);
    *slot = slot.combine(entry);
}

fn merge_alternative_lanes(
    acc: &mut BTreeMap<SigKey, DemandEntry>,
    alternative: BTreeMap<SigKey, DemandEntry>,
) {
    for (lane, entry) in alternative {
        let slot = acc.entry(lane).or_insert(DemandEntry::ZERO);
        *slot = slot.alternative(entry);
    }
}

fn add_lane_demands(
    acc: &mut BTreeMap<SigKey, DemandEntry>,
    demand: BTreeMap<SigKey, DemandEntry>,
) {
    for (lane, entry) in demand {
        bump_lane(acc, lane, entry);
    }
}

enum LaneTask<'a> {
    Visit(&'a Par),
    Child(&'a Par),
    Match(&'a Match),
    Conditional(&'a If),
    Empty,
    Combine,
    Alternative,
}

fn demand_by_sig_into(
    par: &Par,
    envelope_key: SigKey,
    region_sig: &dyn Fn(&Par) -> Option<SigKey>,
    acc: &mut BTreeMap<SigKey, DemandEntry>,
) {
    let mut work = vec![LaneTask::Visit(par)];
    let mut values = Vec::<BTreeMap<SigKey, DemandEntry>>::new();
    while let Some(task) = work.pop() {
        match task {
            LaneTask::Visit(par) => {
                let mut local = BTreeMap::new();
                for send in &par.sends {
                    let lane = send
                        .chan
                        .as_ref()
                        .and_then(|channel| region_sig(channel))
                        .unwrap_or(envelope_key);
                    let mut entry = DemandEntry::ZERO.plus_one();
                    entry.unknown = send.persistent;
                    bump_lane(&mut local, lane, entry);
                }
                for receive in &par.receives {
                    let lane = receive
                        .binds
                        .first()
                        .and_then(|bind| bind.source.as_ref())
                        .and_then(|source| region_sig(source))
                        .unwrap_or(envelope_key);
                    let mut entry = DemandEntry::ZERO.plus_one();
                    entry.unknown = receive.persistent;
                    bump_lane(&mut local, lane, entry);
                }
                for expr in &par.exprs {
                    if let Some(ExprInstance::EVarBody(evar)) = &expr.expr_instance {
                        if let Some(var) = &evar.v {
                            if matches!(
                                var.var_instance,
                                Some(VarInstance::BoundVar(_)) | Some(VarInstance::FreeVar(_))
                            ) {
                                bump_lane(&mut local, envelope_key, DemandEntry {
                                    certified_upper_bound: 0,
                                    unknown: true,
                                });
                            }
                        }
                    }
                }
                values.push(local);
                let mut ordered = Vec::new();
                for receive in &par.receives {
                    if let Some(body) = &receive.body {
                        ordered.push(LaneTask::Child(body));
                    }
                }
                for new in &par.news {
                    if let Some(body) = &new.p {
                        ordered.push(LaneTask::Child(body));
                    }
                }
                for mat in &par.matches {
                    ordered.push(LaneTask::Match(mat));
                }
                for conditional in &par.conditionals {
                    ordered.push(LaneTask::Conditional(conditional));
                }
                for bundle in &par.bundles {
                    if let Some(body) = &bundle.body {
                        ordered.push(LaneTask::Child(body));
                    }
                }
                work.extend(ordered.into_iter().rev());
            }
            LaneTask::Child(child) => {
                work.push(LaneTask::Combine);
                work.push(LaneTask::Visit(child));
            }
            LaneTask::Match(mat) => {
                let mut ordered = Vec::new();
                for source in mat.cases.iter().filter_map(|case| case.source.as_ref()) {
                    if !ordered.is_empty() {
                        ordered.push(LaneTask::Visit(source));
                        ordered.push(LaneTask::Alternative);
                    } else {
                        ordered.push(LaneTask::Visit(source));
                    }
                }
                if !ordered.is_empty() {
                    ordered.push(LaneTask::Combine);
                    work.extend(ordered.into_iter().rev());
                }
            }
            LaneTask::Conditional(conditional) => {
                work.push(LaneTask::Combine);
                work.push(LaneTask::Alternative);
                match conditional.if_false.as_ref() {
                    Some(branch) => work.push(LaneTask::Visit(branch)),
                    None => work.push(LaneTask::Empty),
                }
                match conditional.if_true.as_ref() {
                    Some(branch) => work.push(LaneTask::Visit(branch)),
                    None => work.push(LaneTask::Empty),
                }
            }
            LaneTask::Empty => values.push(BTreeMap::new()),
            LaneTask::Combine => {
                let child = values.pop().expect("child lane demand");
                add_lane_demands(values.last_mut().expect("parent lane demand"), child);
            }
            LaneTask::Alternative => {
                let right = values.pop().expect("right branch demand");
                merge_alternative_lanes(values.last_mut().expect("left branch demand"), right);
            }
        }
    }
    add_lane_demands(acc, values.pop().expect("root lane demand"));
}

/// §7.4 desugaring boundary for the funding analysis. The §7.4 semantic count
/// ("8 not 6") requires `?!` (synchronous send) to be expanded to a send + a
/// for-comprehension on each side, and uniform signing to be expanded to its
/// nested signed layers — so that [`demand`] counts the form the runtime
/// actually executes rather than the syntactic surface.
///
/// In f1r3node this expansion is performed UPSTREAM by the normalizer: `?!` is
/// desugared by `compiler/normalizer/processes/p_send_sync_normalizer.rs` into
/// `new ret in { chan!(ret, args) | for(_ <- ret){ cont } }`; uniform and
/// lollipop signing are normalized into native `CostSignedTerm` layers. A `Par`
/// produced by `Compiler::source_to_adt` (the same path the runtime evaluates
/// through) is therefore ALREADY in the desugared form [`demand`] requires.
/// Re-expanding here would double-count the normalized nodes.
///
/// This function is consequently the identity on an already-normalized `Par`. It
/// exists to (a) make the desugar-then-count contract explicit at the funding
/// boundary, and (b) provide a single, named seam to extend should a future
/// front-end deliver a `Par` that is NOT pre-desugared (none does today). The
/// returned value is `Cow`-free (an owned clone) to keep the boundary a total
/// function over `&Par`.
#[inline]
pub fn desugar_for_funding(par: &Par) -> Par { par.clone() }

/// The Split/Join supply closure `effectiveΣ` (cost-accounted-rho §B.1
/// decomposition equivalence; Appendix A eq:app-st-signed-compound). Given the
/// RAW per-signature supplies `Σ_s` (the integer projection of canonical vault
/// balance plus located stack capacity, keyed by `Sig::lane_hash`), produce the EFFECTIVE supplies
/// that account for the interchangeability between a combined compound stack
/// `s₁∘s₂` and the minimum of its component stacks:
///
/// ```text
/// effectiveΣ_{s₁∘s₂} = Σ_{s₁∘s₂} + min(Σ_{s₁}, Σ_{s₂})
/// effectiveΣ_{s₁}    = Σ_{s₁}    + Σ_{s₁∘s₂}
/// ```
///
/// Intuition: a deploy demanding the compound `s₁∘s₂` may draw either from the
/// compound pool directly OR from a matched pair of component tokens — so its
/// effective compound supply is the compound balance plus the number of pairs the
/// components can form (`min`). Dually, a deploy demanding a single component
/// `s₁` may draw from `s₁`'s own pool OR from the compound pool (a compound token
/// satisfies a component obligation) — so its effective single supply is the sum.
///
/// ## Native realization
///
/// The spec states this closure over the abstract signature algebra `s₁∘s₂`. At
/// the substrate, funding compounds are represented by `Sig::And` (the proto
/// `Tensor`); a funding authority is either a single atom or a `Sig::And` tree.
/// Threshold remains an admission predicate rather than a funding former. This
/// function reconstructs the closure from
/// the raw-supply map by, for each compound key present, locating its component
/// keys (themselves derivable as `Sig::lane_hash` of the components) and applying
/// the two equations. Because the input map is keyed by opaque `SigKey` digests
/// (not structured `Sig`s — the gate indexes authority by lane digest), the
/// closure is computed structurally from a companion list of the in-scope
/// signatures supplied by the caller. To keep this function a PURE map→map
/// transform with no `Sig`-reconstruction-from-digest (which is not invertible),
/// the closure is expressed over an explicit `decomposition` describing which
/// compound key splits into which two component keys; the no-decomposition case
/// (a flat map of independent atoms) is the identity, which is the common
/// single-signature and disjoint-multi-signature fast path.
///
/// The caller (D2 gate) — which HAS the structured envelope `Sig`s in hand —
/// builds the `decomposition` by walking each `Sig::And`/compound envelope and
/// emitting `(lane_hash(compound), lane_hash(left), lane_hash(right))`. Atoms and
/// already-disjoint signatures contribute no decomposition entry and pass through
/// unchanged.
pub fn effective_supply(raw: &BTreeMap<SigKey, i64>) -> BTreeMap<SigKey, i64> {
    // With no decomposition information, every signature is treated as an
    // independent atom: `effectiveΣ_s = Σ_s` (the closure's identity case). This
    // is the single-signature fast path and the disjoint-multi-signature path,
    // where no compound pool exists to fold in.
    effective_supply_with(raw, &[])
}

/// Describes one Split/Join decomposition: a compound signature's supply key and
/// the two component keys it splits into (cost-accounted-rho §B.1). Built by the
/// gate from a structured compound envelope (`lane_hash(compound)`,
/// `lane_hash(left)`, `lane_hash(right)`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decomposition {
    /// `lane_hash(s₁∘s₂)` — the compound supply key.
    pub compound: SigKey,
    /// `lane_hash(s₁)` — the left component supply key.
    pub left: SigKey,
    /// `lane_hash(s₂)` — the right component supply key.
    pub right: SigKey,
}

/// The Split/Join closure with explicit compound decompositions (the general
/// form of [`effective_supply`]). Applies, for each decomposition
/// `(s₁∘s₂, s₁, s₂)`, ONLY the Join (combine) term:
///
/// ```text
/// effectiveΣ_{s₁∘s₂} = Σ_{s₁∘s₂} + min(Σ_{s₁}, Σ_{s₂})   // Join: a matched pair builds a compound
/// effectiveΣ_{s₁}    = Σ_{s₁}                            // no-weakening: a single component is
/// effectiveΣ_{s₂}    = Σ_{s₂}                            //   NOT augmented by the compound pool
/// ```
///
/// The DUAL of Join — crediting a single component `s₁` with the compound pool
/// `Σ_{s₁∘s₂}` — is **weakening** and is FORBIDDEN: discharging a single-`s₁`
/// demand by consuming a compound token `s₁∘s₂` silently discards the `s₂`
/// authority (Rocq `CAJoinConservation.join_no_weakening` — `s₁∘s₂` carries
/// strictly more signature atoms than `s₁`, so it cannot be discharged as `s₁`
/// alone). Funding `s₁` from a compound requires an explicit, observable `Split`
/// reduction that credits `Σ⟦s₂⟧` with the orphaned half (the runtime Splitter),
/// NEVER a static admission credit (Cost-Accounted Rho "Weakening Is Forbidden",
/// §3.7.5; WD-D2 §D2.9-R2). An earlier version also inserted `effectiveΣ_{s₁} = Σ_{s₁} +
/// Σ_{s₁∘s₂}`; that over-credit (a code-only outlier matching no proof/doc/model)
/// admitted a single-sig group against a capacity the conservation-preserving
/// settlement (`GroupShape::Single` draws its own pool only) cannot honor — a
/// latent underflow once any compound pool is provisioned. Removed here to match
/// the verified model.
///
/// using the RAW balances (`min` is computed on the raw component supplies so the
/// closure is well-defined regardless of decomposition order — the result is a
/// pure function of `raw` and the decomposition set). Keys not mentioned in any
/// decomposition pass through with `effectiveΣ_s = Σ_s`. A balance absent from
/// `raw` reads as `0` (supply-realization Decision 2 — 0 when absent).
pub fn effective_supply_with(
    raw: &BTreeMap<SigKey, i64>,
    decompositions: &[Decomposition],
) -> BTreeMap<SigKey, i64> {
    // Start from the identity (effectiveΣ_s = Σ_s for every present key), then
    // fold in each compound's contribution. Reading raw balances throughout
    // (never the partially-updated `effective`) keeps the closure order-
    // independent and a pure function of (raw, decompositions).
    let mut effective = raw.clone();

    let read_raw = |key: &SigKey| -> i64 { raw.get(key).copied().unwrap_or(0) };

    for decomposition in decompositions {
        let sigma_compound = read_raw(&decomposition.compound);
        let sigma_left = read_raw(&decomposition.left);
        let sigma_right = read_raw(&decomposition.right);

        // effectiveΣ_{s₁∘s₂} = Σ_{s₁∘s₂} + min(Σ_{s₁}, Σ_{s₂})  (Join — build a
        // compound from a matched component pair; the authority-preserving
        // direction. `sigma_left`/`sigma_right` are read only for this `min`.)
        let compound_effective = sigma_compound.saturating_add(sigma_left.min(sigma_right));
        effective.insert(decomposition.compound, compound_effective);

        // NO single-component over-credit (no-weakening — WD-D2 §D2.9-R2): a
        // single component `s₁`/`s₂` is NOT augmented by the compound pool
        // `Σ_{s₁∘s₂}`. Consuming a compound `s₁∘s₂` to discharge a single-`s₁`
        // demand would discard the `s₂` authority (Rocq
        // `CAJoinConservation.join_no_weakening`); funding `s₁` from a compound
        // requires an explicit `Split` reduction crediting `Σ⟦s₂⟧`, never a static
        // admission credit. So `s₁` and `s₂` pass through at their raw balance (the
        // identity `effective = raw.clone()` set above) — the settlement's
        // `GroupShape::Single` draws the own pool only, so this matches exactly.
    }

    effective
}

/// The funding decision for one signature group (cost-accounted-rho Def 19 +
/// Thm 20): a deploy (or canonical-order prefix of a signature group) is fundable
/// iff the EFFECTIVE supply meets or exceeds a finite certified demand.
///
/// ```text
/// fundable  ⇔  !unknown ∧ effective_supply_s ≥ certified_upper_bound
/// ```
///
/// A finite `DemandBound` is checked against supply exactly. Unprovable demand
/// is rejected; a future GSLT proof producer can instead provide a checked
/// finite upper bound through the same abstraction.
#[inline]
pub fn is_funded(analysis: &DemandEntry, effective_supply_s: i64) -> bool {
    if analysis.unknown {
        return false;
    }
    i128::from(effective_supply_s) >= i128::from(analysis.certified_upper_bound)
}

#[cfg(test)]
mod tests {
    use models::rhoapi::cost_signature::Value as CostSignatureValue;
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::var::VarInstance;
    use models::rhoapi::{
        Bundle, CostSignature, CostSignatureCompound, CostSignedTerm, CostStack, EVar, Expr, If,
        Match, MatchCase, New, Par, Receive, ReceiveBind, Send, Var,
    };
    use proptest::prelude::*;

    use super::*;

    fn atom(tag: u8) -> Sig { Sig::Ground(vec![tag, tag, tag, tag]) }

    fn empty_par() -> Par { Par::default() }

    fn send_on(chan: Par) -> Send {
        Send {
            chan: Some(chan),
            data: Vec::new(),
            persistent: false,
            locally_free: Vec::new(),
            connective_used: false,
        }
    }

    fn par_with_sends(n: usize) -> Par {
        let mut par = Par::default();
        for _ in 0..n {
            par.sends.push(send_on(empty_par()));
        }
        par
    }

    fn wire_ground(tag: u8) -> CostSignature {
        CostSignature {
            value: Some(CostSignatureValue::Ground(vec![tag])),
        }
    }

    fn signed(body: Par, signature: CostSignature) -> Par {
        models::par_from_default! {
            cost_signed_terms: vec![CostSignedTerm {
                body: Some(body),
                signature: Some(signature),
            }],
            ..Par::default()
        }
    }

    fn signed_terms(mut par: Par) -> Vec<CostSignedTerm> {
        std::mem::take(&mut par.cost_signed_terms)
    }

    // ── demand: structural counting ────────────────────────────────────────

    #[test]
    fn empty_par_has_zero_demand() {
        let entry = demand(&empty_par(), &atom(1));
        assert_eq!(entry.certified_upper_bound, 0);
        assert!(!entry.unknown);
    }

    #[test]
    fn each_send_counts_one() {
        let entry = demand(&par_with_sends(3), &atom(1));
        assert_eq!(entry.certified_upper_bound, 3);
        assert!(!entry.unknown);
    }

    #[test]
    fn one_wrapper_reserves_every_potential_surface_event() {
        let signature = wire_ground(7);
        let bound = demand_bound(&signed(par_with_sends(2), signature.clone()), &atom(1));
        let DemandBound::FiniteUpperBound { bound, .. } = bound else {
            panic!("finite bound expected")
        };
        let lane = cost_signature_to_sig(&signature).unwrap().lane_hash();
        assert_eq!(bound.0, BTreeMap::from([(lane, 2)]));
    }

    #[test]
    fn unit_wrapper_has_zero_static_demand_and_no_supply_lane() {
        let signature = CostSignature {
            value: Some(CostSignatureValue::Unit(true)),
        };
        let par = signed(par_with_sends(3), signature);
        let DemandBound::FiniteUpperBound { bound, .. } = demand_bound(&par, &atom(1)) else {
            panic!("finite bound expected")
        };

        assert!(bound.0.is_empty());
        assert!(static_authority_signatures(&par).unwrap().is_empty());
    }

    #[test]
    fn stack_construction_reserves_each_cell_from_the_deploy_authority() {
        let target = wire_ground(7);
        let par = models::par_from_default! {
            cost_stacks: vec![CostStack {
                cells: vec![target.clone(), target.clone(), target.clone()],
                parallel_tail: Vec::new(),
            }],
            ..Par::default()
        };
        let deploy = atom(1);
        let DemandBound::FiniteUpperBound { bound, .. } = demand_bound(&par, &deploy) else {
            panic!("finite bound expected")
        };

        assert_eq!(bound.0, BTreeMap::from([(deploy.lane_hash(), 3)]));
        assert_eq!(
            static_authority_signatures(&par).unwrap(),
            BTreeMap::from([(cost_signature_to_sig(&target).unwrap().lane_hash(), target,)])
        );
    }

    #[test]
    fn stack_construction_reserves_each_cell_from_the_enclosing_authority() {
        let source = wire_ground(8);
        let target = wire_ground(7);
        let body = models::par_from_default! {
            cost_stacks: vec![CostStack {
                cells: vec![target.clone(), target],
                parallel_tail: Vec::new(),
            }],
            ..Par::default()
        };
        let DemandBound::FiniteUpperBound { bound, .. } =
            demand_bound(&signed(body, source.clone()), &atom(1))
        else {
            panic!("finite bound expected")
        };

        assert_eq!(
            bound.0,
            BTreeMap::from([(cost_signature_to_sig(&source).unwrap().lane_hash(), 2,)])
        );
    }

    #[test]
    fn dynamic_stack_target_still_has_a_finite_source_transfer_bound() {
        let dynamic = CostSignature {
            value: Some(CostSignatureValue::BoundLevel(0)),
        };
        let source = wire_ground(8);
        let body = models::par_from_default! {
            cost_stacks: vec![CostStack {
                cells: vec![dynamic],
                parallel_tail: Vec::new(),
            }],
            ..Par::default()
        };
        let DemandBound::FiniteUpperBound { bound, .. } =
            demand_bound(&signed(body, source.clone()), &atom(1))
        else {
            panic!("finite bound expected")
        };

        assert_eq!(
            bound.0,
            BTreeMap::from([(cost_signature_to_sig(&source).unwrap().lane_hash(), 1,)])
        );
    }

    #[test]
    fn empty_and_unit_stack_cells_are_unprovable() {
        let empty = models::par_from_default! {
            cost_stacks: vec![CostStack { cells: Vec::new(), parallel_tail: Vec::new() }],
            ..Par::default()
        };
        let unit = models::par_from_default! {
            cost_stacks: vec![CostStack {
                cells: vec![CostSignature {
                    value: Some(CostSignatureValue::Unit(true)),
                }],
                parallel_tail: Vec::new(),
            }],
            ..Par::default()
        };

        assert_eq!(
            demand_bound(&empty, &atom(1)),
            DemandBound::Unprovable(UnprovableDemand::UnsupportedSyntax)
        );
        assert_eq!(
            demand_bound(&unit, &atom(1)),
            DemandBound::Unprovable(UnprovableDemand::UnsupportedSyntax)
        );
        assert_eq!(
            static_authority_signatures(&empty),
            Err(AuthorityError::MissingSignature)
        );
    }

    #[test]
    fn stack_values_sent_as_data_are_not_materialized_transfers() {
        let target = wire_ground(7);
        let mut par = Par::default();
        par.sends.push(Send {
            chan: Some(Par::default()),
            data: vec![models::par_from_default! {
                cost_stacks: vec![CostStack {
                    cells: vec![target],
                    parallel_tail: Vec::new(),
                }],
                ..Par::default()
            }],
            persistent: false,
            locally_free: Vec::new(),
            connective_used: false,
        });
        let deploy = atom(1);
        let DemandBound::FiniteUpperBound { bound, .. } = demand_bound(&par, &deploy) else {
            panic!("finite bound expected")
        };

        assert_eq!(bound.0, BTreeMap::from([(deploy.lane_hash(), 1)]));
    }

    #[test]
    fn program_stack_supply_replaces_only_its_target_external_reservation() {
        let source = wire_ground(8);
        let target = wire_ground(7);
        let stack = signed(
            models::par_from_default! {
                cost_stacks: vec![CostStack {
                    cells: vec![target.clone()],
                    parallel_tail: Vec::new(),
                }],
                ..Par::default()
            },
            source.clone(),
        );
        let mut par = stack;
        par.cost_signed_terms
            .extend(signed_terms(signed(par_with_sends(1), target.clone())));

        let plan = static_authority_plan(&par, &atom(1)).unwrap();
        let source_lane = cost_signature_to_sig(&source).unwrap().lane_hash();
        let target_lane = cost_signature_to_sig(&target).unwrap().lane_hash();
        assert_eq!(
            plan.demand.0,
            BTreeMap::from([(source_lane, 1), (target_lane, 1)])
        );
        assert_eq!(plan.transfer_demand.0, BTreeMap::from([(source_lane, 1)]));
        assert_eq!(
            plan.guaranteed_program_supply.0,
            BTreeMap::from([(target_lane, 1)])
        );
        assert_eq!(
            plan.external_reservation.0,
            BTreeMap::from([(source_lane, 1)])
        );
    }

    #[test]
    fn alternative_branch_supply_cannot_fund_a_different_branch() {
        let source = wire_ground(8);
        let target = wire_ground(7);
        let stack_branch = signed(
            models::par_from_default! {
                cost_stacks: vec![CostStack {
                    cells: vec![target.clone()],
                    parallel_tail: Vec::new(),
                }],
                ..Par::default()
            },
            source.clone(),
        );
        let demand_branch = signed(par_with_sends(1), target.clone());
        let par = models::par_from_default! {
            conditionals: vec![If {
                condition: Some(Par::default()),
                if_true: Some(stack_branch),
                if_false: Some(demand_branch),
                locally_free: Vec::new(),
                connective_used: false,
            }],
            ..Par::default()
        };

        let plan = static_authority_plan(&par, &atom(1)).unwrap();
        let source_lane = cost_signature_to_sig(&source).unwrap().lane_hash();
        let target_lane = cost_signature_to_sig(&target).unwrap().lane_hash();
        assert!(plan.guaranteed_program_supply.0.is_empty());
        assert_eq!(
            plan.external_reservation.0,
            BTreeMap::from([(source_lane, 1), (target_lane, 1)])
        );
    }

    #[test]
    fn nested_unit_wrapper_is_neutral_for_outer_authority() {
        let outer = wire_ground(7);
        let unit = CostSignature {
            value: Some(CostSignatureValue::Unit(true)),
        };
        let par = signed(signed(par_with_sends(2), unit), outer.clone());
        let DemandBound::FiniteUpperBound { bound, .. } = demand_bound(&par, &atom(1)) else {
            panic!("finite bound expected")
        };

        assert_eq!(
            bound.0,
            BTreeMap::from([(cost_signature_to_sig(&outer).unwrap().lane_hash(), 2)])
        );
    }

    proptest! {
        #[test]
        fn wrapped_surface_bound_grows_with_introduction_count(count in 1usize..128) {
            let signature = wire_ground(7);
            let bound = demand_bound(&signed(par_with_sends(count), signature.clone()), &atom(1));
            let DemandBound::FiniteUpperBound { bound, .. } = bound else {
                prop_assert!(false, "finite bound expected");
                return Ok(());
            };
            let lane = cost_signature_to_sig(&signature).unwrap().lane_hash();
            prop_assert_eq!(bound.get(&lane), count as u64);
        }
    }

    #[test]
    fn independently_wrapped_surfaces_reserve_independent_purses() {
        let left = wire_ground(7);
        let right = wire_ground(8);
        let mut par = signed(par_with_sends(1), left.clone());
        par.cost_signed_terms
            .extend(signed_terms(signed(par_with_sends(1), right.clone())));
        let DemandBound::FiniteUpperBound { bound, .. } = demand_bound(&par, &atom(1)) else {
            panic!("finite bound expected")
        };
        assert_eq!(
            bound.0,
            BTreeMap::from([
                (cost_signature_to_sig(&left).unwrap().lane_hash(), 1),
                (cost_signature_to_sig(&right).unwrap().lane_hash(), 1),
            ])
        );
    }

    #[test]
    fn compound_wrapper_is_one_indivisible_requirement() {
        let compound = CostSignature {
            value: Some(CostSignatureValue::Compound(CostSignatureCompound {
                elements: vec![wire_ground(7), wire_ground(8)],
            })),
        };
        let DemandBound::FiniteUpperBound { bound, .. } =
            demand_bound(&signed(par_with_sends(2), compound.clone()), &atom(1))
        else {
            panic!("finite bound expected")
        };
        assert_eq!(
            bound.0,
            BTreeMap::from([(cost_signature_to_sig(&compound).unwrap().lane_hash(), 2)])
        );
    }

    #[test]
    fn unresolved_located_purse_requires_state_bound_evidence() {
        let dynamic = CostSignature {
            value: Some(CostSignatureValue::BoundLevel(0)),
        };
        assert_eq!(
            demand_bound(&signed(par_with_sends(1), dynamic), &atom(1)),
            DemandBound::Unprovable(UnprovableDemand::DynamicAuthority)
        );
    }

    #[test]
    fn execution_capacity_defers_runtime_bound_authority_to_state_bound_evidence() {
        let resolved = wire_ground(7);
        let dynamic = CostSignature {
            value: Some(CostSignatureValue::BoundLevel(0)),
        };
        let mut par = signed(par_with_sends(1), resolved.clone());
        par.cost_signed_terms
            .extend(signed_terms(signed(par_with_sends(1), dynamic)));

        let signatures = static_authority_signatures(&par).unwrap();
        assert_eq!(signatures.len(), 1);
        assert_eq!(
            signatures.get(&cost_signature_to_sig(&resolved).unwrap().lane_hash()),
            Some(&resolved)
        );
    }

    #[test]
    fn execution_capacity_rejects_malformed_dynamic_compounds() {
        let malformed = CostSignature {
            value: Some(CostSignatureValue::Compound(CostSignatureCompound {
                elements: vec![CostSignature {
                    value: Some(CostSignatureValue::BoundLevel(0)),
                }],
            })),
        };
        assert_eq!(
            static_authority_signatures(&signed(par_with_sends(1), malformed)),
            Err(AuthorityError::MalformedCompound)
        );
    }

    #[test]
    fn persistent_send_is_structurally_unprovable() {
        let mut send = send_on(empty_par());
        send.persistent = true;
        let mut par = Par::default();
        par.sends.push(send);
        let entry = demand(&par, &atom(1));
        assert_eq!(entry.certified_upper_bound, 1);
        assert!(entry.unknown);
    }

    #[test]
    fn receive_counts_self_plus_body() {
        // for(_ <- chan){ chan2!() | chan3!() }  ⇒ 1 (receive) + 2 (body sends).
        let mut body = Par::default();
        body.sends.push(send_on(empty_par()));
        body.sends.push(send_on(empty_par()));
        let receive = Receive {
            binds: vec![ReceiveBind {
                patterns: Vec::new(),
                source: Some(empty_par()),
                remainder: None,
                free_count: 0,
                cost_signature: None,
            }],
            body: Some(body),
            persistent: false,
            peek: false,
            bind_count: 0,
            locally_free: Vec::new(),
            connective_used: false,
            condition: None,
        };
        let mut par = Par::default();
        par.receives.push(receive);
        let entry = demand(&par, &atom(1));
        assert_eq!(entry.certified_upper_bound, 3);
        assert!(!entry.unknown);
    }

    #[test]
    fn persistent_receive_is_structurally_unprovable() {
        let receive = Receive {
            binds: vec![ReceiveBind {
                patterns: Vec::new(),
                source: Some(empty_par()),
                remainder: None,
                free_count: 0,
                cost_signature: None,
            }],
            body: Some(empty_par()),
            persistent: true,
            peek: false,
            bind_count: 0,
            locally_free: Vec::new(),
            connective_used: false,
            condition: None,
        };
        let mut par = Par::default();
        par.receives.push(receive);
        let entry = demand(&par, &atom(1));
        assert_eq!(entry.certified_upper_bound, 1);
        assert!(entry.unknown);
    }

    #[test]
    fn new_does_not_count_but_recurses_scoped_body() {
        // D3 (DR-9, OD-3): `new x in { send | send }` ⇒ 2 (the two body sends
        // only). The `new` node is a DIAGNOSTIC `Reduction`, NOT a `Comm`, so it
        // contributes 0; its scoped body is still recursed. This is the §7.4
        // "9 → 8" re-pin in miniature (the `new` no longer adds a token).
        let new = New {
            bind_count: 1,
            p: Some(par_with_sends(2)),
            uri: Vec::new(),
            injections: Default::default(),
            locally_free: Vec::new(),
        };
        let mut par = Par::default();
        par.news.push(new);
        let entry = demand(&par, &atom(1));
        assert_eq!(entry.certified_upper_bound, 2);
        assert!(!entry.unknown);
    }

    #[test]
    fn conditional_reserves_maximum_alternative() {
        let conditional = If {
            condition: Some(Par::default()),
            if_true: Some(par_with_sends(2)),
            if_false: Some(par_with_sends(5)),
            locally_free: Vec::new(),
            connective_used: false,
        };
        let mut par = Par::default();
        par.conditionals.push(conditional);
        let entry = demand(&par, &atom(1));
        assert_eq!(entry.certified_upper_bound, 5);
        assert!(!entry.unknown);
    }

    #[test]
    fn empty_signed_region_retains_one_unit_layer_demand() {
        let signature = wire_ground(7);
        let DemandBound::FiniteUpperBound { bound, .. } =
            demand_bound(&signed(Par::default(), signature.clone()), &atom(1))
        else {
            panic!("finite bound expected")
        };
        assert_eq!(
            bound.get(&cost_signature_to_sig(&signature).unwrap().lane_hash()),
            1,
        );
    }

    #[test]
    fn nested_signed_lane_analysis_is_stack_safe() {
        std::thread::Builder::new()
            .stack_size(128 * 1024)
            .spawn(|| {
                let mut par = par_with_sends(1);
                for depth in 0..2048 {
                    par = signed(par, wire_ground((depth % 4) as u8));
                }
                let entry = demand(&par, &atom(9));
                assert_eq!(entry.certified_upper_bound, 2048);
                assert!(!entry.unknown);
                assert_eq!(static_authority_signatures(&par).unwrap().len(), 4);
            })
            .unwrap()
            .join()
            .unwrap();
    }

    proptest! {
        #[test]
        fn conditional_reserves_componentwise_maximum_for_isolated_purses(
            left_true in 0usize..64,
            right_true in 0usize..64,
            left_false in 0usize..64,
            right_false in 0usize..64,
        ) {
            let left = wire_ground(7);
            let right = wire_ground(8);
            let branch = |left_count, right_count| {
                let mut par = signed(par_with_sends(left_count), left.clone());
                par.cost_signed_terms.extend(
                    signed_terms(signed(par_with_sends(right_count), right.clone())),
                );
                par
            };
            let par = models::par_from_default! {
                conditionals: vec![If {
                    condition: Some(Par::default()),
                    if_true: Some(branch(left_true, right_true)),
                    if_false: Some(branch(left_false, right_false)),
                    locally_free: Vec::new(),
                    connective_used: false,
                }],
                ..Par::default()
            };
            let DemandBound::FiniteUpperBound { bound, .. } = demand_bound(&par, &atom(1)) else {
                prop_assert!(false, "finite bound expected");
                return Ok(());
            };
            let left_lane = cost_signature_to_sig(&left).unwrap().lane_hash();
            let right_lane = cost_signature_to_sig(&right).unwrap().lane_hash();
            prop_assert_eq!(
                bound.get(&left_lane),
                left_true.max(left_false).max(1) as u64,
            );
            prop_assert_eq!(
                bound.get(&right_lane),
                right_true.max(right_false).max(1) as u64,
            );
        }
    }

    #[test]
    fn match_reserves_maximum_alternative() {
        let make_case = |sends| MatchCase {
            pattern: Some(Par::default()),
            source: Some(par_with_sends(sends)),
            free_count: 0,
            guard: None,
        };
        let matched = Match {
            target: Some(Par::default()),
            cases: vec![make_case(3), make_case(7), make_case(4)],
            locally_free: Vec::new(),
            connective_used: false,
        };
        let par = Par::default().with_matches(vec![matched]);
        let entry = demand(&par, &atom(1));
        assert_eq!(entry.certified_upper_bound, 7);
        assert!(!entry.unknown);
    }

    #[test]
    fn signed_payload_demand_is_preserved_through_communication() {
        let mut send = send_on(empty_par());
        send.data.push(par_with_sends(1));
        let mut par = Par::default();
        par.sends.push(send);
        let entry = demand(&par, &atom(1));
        assert_eq!(entry.certified_upper_bound, 2);
        assert!(!entry.unknown);
    }

    #[test]
    fn bound_name_in_channel_position_is_not_unknown() {
        // for(y <- x){ Nil } where x is a `new`-bound name: the source channel is
        // an EVar(bound_var) in NAME position. That is a channel reference, NOT a
        // `*x` process dequotation, so it must NOT trigger `unknown`. The receive
        // counts 1 (itself); the empty body counts 0.
        let receive = Receive {
            binds: vec![ReceiveBind {
                patterns: Vec::new(),
                // x as a bound-var name in the source (channel) position.
                source: Some(eval_of_bound_var(0)),
                remainder: None,
                free_count: 0,
                cost_signature: None,
            }],
            body: Some(empty_par()),
            persistent: false,
            peek: false,
            bind_count: 0,
            locally_free: Vec::new(),
            connective_used: false,
            condition: None,
        };
        let mut par = Par::default();
        par.receives.push(receive);
        let entry = demand(&par, &atom(1));
        assert_eq!(entry.certified_upper_bound, 1);
        assert!(
            !entry.unknown,
            "a bound name in channel position is not an unresolved dequotation"
        );
    }

    // ── demand: the unknown (over-approximation) trigger ───────────────────

    fn eval_of_bound_var(level: i32) -> Par {
        // `*x` where x is a bound name → an un-inlined EVar(bound_var) in
        // process position.
        let mut par = Par::default();
        par.exprs.push(Expr {
            expr_instance: Some(ExprInstance::EVarBody(EVar {
                v: Some(Var {
                    var_instance: Some(VarInstance::BoundVar(level)),
                }),
            })),
        });
        par
    }

    #[test]
    fn unresolved_eval_sets_unknown() {
        let entry = demand(&eval_of_bound_var(0), &atom(1));
        assert!(entry.unknown);
        // The dereference itself contributes no known COMM node — the demand is
        // entirely the (unknown) resolved process.
        assert_eq!(entry.certified_upper_bound, 0);
    }

    #[test]
    fn unknown_is_sticky_across_parallel_composition() {
        // send | *x  ⇒ known 1 send, unknown true.
        let mut par = par_with_sends(1);
        let mut eval = eval_of_bound_var(0);
        par.exprs = std::mem::take(&mut eval.exprs);
        let entry = demand(&par, &atom(1));
        assert_eq!(entry.certified_upper_bound, 1);
        assert!(entry.unknown);
    }

    #[test]
    fn absent_optional_processes_and_inert_expressions_have_zero_demand() {
        let mut par = Par::default();
        par.receives.push(Receive {
            body: None,
            ..Default::default()
        });
        par.news.push(New {
            p: None,
            ..Default::default()
        });
        par.matches.push(Match {
            cases: vec![MatchCase {
                source: None,
                ..Default::default()
            }],
            ..Default::default()
        });
        par.bundles.push(Bundle {
            body: None,
            write_flag: true,
            read_flag: true,
        });
        par.exprs.extend([
            Expr::default(),
            Expr {
                expr_instance: Some(ExprInstance::EVarBody(EVar { v: None })),
            },
            Expr {
                expr_instance: Some(ExprInstance::EVarBody(EVar {
                    v: Some(Var { var_instance: None }),
                })),
            },
        ]);

        let entry = demand(&par, &atom(1));
        assert_eq!(entry.certified_upper_bound, 1);
        assert!(!entry.unknown);
    }

    #[test]
    fn bundle_body_and_free_variable_dequotation_are_accounted() {
        let mut par = Par::default();
        par.bundles.push(Bundle {
            body: Some(par_with_sends(2)),
            write_flag: true,
            read_flag: false,
        });
        par.exprs.push(Expr {
            expr_instance: Some(ExprInstance::EVarBody(EVar {
                v: Some(Var {
                    var_instance: Some(VarInstance::FreeVar(0)),
                }),
            })),
        });

        let entry = demand(&par, &atom(1));
        assert_eq!(entry.certified_upper_bound, 2);
        assert!(entry.unknown);
    }

    #[test]
    fn alternative_demand_preserves_unknown_from_either_branch() {
        let unknown_case = MatchCase {
            source: Some(eval_of_bound_var(0)),
            ..Default::default()
        };
        let known_case = MatchCase {
            source: Some(par_with_sends(2)),
            ..Default::default()
        };
        for cases in [vec![unknown_case.clone(), known_case.clone()], vec![
            known_case.clone(),
            unknown_case.clone(),
        ]] {
            let par = Par::default().with_matches(vec![Match {
                cases,
                ..Default::default()
            }]);
            let entry = demand(&par, &atom(1));
            assert_eq!(entry.certified_upper_bound, 2);
            assert!(entry.unknown);
        }
    }

    // ── desugar_for_funding: identity on a normalized Par ──────────────────

    #[test]
    fn desugar_for_funding_is_identity_on_normalized_par() {
        let par = par_with_sends(2);
        assert_eq!(desugar_for_funding(&par), par);
    }

    // ── effective_supply: the Split/Join closure arithmetic ────────────────

    #[test]
    fn effective_supply_identity_when_no_decomposition() {
        let mut raw = BTreeMap::new();
        raw.insert([1u8; 32], 5_i64);
        raw.insert([2u8; 32], 7_i64);
        let effective = effective_supply(&raw);
        assert_eq!(effective, raw);
    }

    #[test]
    fn effective_supply_split_join_closure_arithmetic() {
        // Σ_{s1} = 4, Σ_{s2} = 6, Σ_{s1∘s2} = 10.
        // effectiveΣ_{s1∘s2} = 10 + min(4,6) = 14   (Join term)
        // effectiveΣ_{s1}    = 4   (no-weakening: NOT 4+10 — a compound pool does
        // effectiveΣ_{s2}    = 6    NOT augment a single component; §D2.9-R2)
        let s1 = [1u8; 32];
        let s2 = [2u8; 32];
        let compound = [3u8; 32];
        let mut raw = BTreeMap::new();
        raw.insert(s1, 4_i64);
        raw.insert(s2, 6_i64);
        raw.insert(compound, 10_i64);

        let effective = effective_supply_with(&raw, &[Decomposition {
            compound,
            left: s1,
            right: s2,
        }]);

        assert_eq!(effective.get(&compound), Some(&14));
        // No-weakening (§D2.9-R2): the single components pass through at their RAW
        // balance, NOT credited with the compound pool (was 14 / 16 pre-R2).
        assert_eq!(effective.get(&s1), Some(&4));
        assert_eq!(effective.get(&s2), Some(&6));
    }

    #[test]
    fn effective_supply_treats_absent_component_as_zero() {
        // Only the compound pool exists; components absent ⇒ read as 0.
        // effectiveΣ_{s1∘s2} = 8 + min(0,0) = 8
        // No-weakening (§D2.9-R2): absent single components are NOT credited with
        // the compound pool — they pass through UNSET (the gate reads absent ⇒ 0).
        let s1 = [1u8; 32];
        let s2 = [2u8; 32];
        let compound = [3u8; 32];
        let mut raw = BTreeMap::new();
        raw.insert(compound, 8_i64);

        let effective = effective_supply_with(&raw, &[Decomposition {
            compound,
            left: s1,
            right: s2,
        }]);

        assert_eq!(effective.get(&compound), Some(&8));
        assert_eq!(effective.get(&s1), None);
        assert_eq!(effective.get(&s2), None);
    }

    #[test]
    fn effective_supply_closure_is_order_independent() {
        // The closure reads raw balances throughout, so applying two
        // decompositions in either order yields the same map.
        let a = [1u8; 32];
        let b = [2u8; 32];
        let ab = [3u8; 32];
        let c = [4u8; 32];
        let d = [5u8; 32];
        let cd = [6u8; 32];
        let mut raw = BTreeMap::new();
        for (k, v) in [(a, 2), (b, 3), (ab, 1), (c, 7), (d, 5), (cd, 4)] {
            raw.insert(k, v as i64);
        }
        let decomposition_ab = Decomposition {
            compound: ab,
            left: a,
            right: b,
        };
        let decomposition_cd = Decomposition {
            compound: cd,
            left: c,
            right: d,
        };

        let forward = effective_supply_with(&raw, &[decomposition_ab, decomposition_cd]);
        let backward = effective_supply_with(&raw, &[decomposition_cd, decomposition_ab]);
        assert_eq!(forward, backward);
    }

    // ── is_funded: exact finite-bound decision ────────────────────────────

    fn resolvable(lower: i64) -> DemandEntry {
        DemandEntry {
            certified_upper_bound: lower,
            unknown: false,
        }
    }

    fn unresolvable(lower: i64) -> DemandEntry {
        DemandEntry {
            certified_upper_bound: lower,
            unknown: true,
        }
    }

    #[test]
    fn resolvable_funded_at_def19_boundary() {
        assert!(is_funded(&resolvable(8), 8)); // Σ = Δ
        assert!(is_funded(&resolvable(8), 9));
        assert!(is_funded(&resolvable(8), 100));
    }

    #[test]
    fn resolvable_rejected_below_demand() {
        assert!(!is_funded(&resolvable(8), 7)); // Σ = Δ-1
    }

    #[test]
    fn funding_is_supply_greater_than_or_equal_to_demand() {
        assert!(is_funded(&resolvable(3), 3)); // Σ = Δ
        assert!(!is_funded(&resolvable(3), 2)); // Σ < Δ
    }

    #[test]
    fn unknown_demand_requires_a_finite_proof() {
        assert!(!is_funded(&unresolvable(5), 9));
        assert!(!is_funded(&unresolvable(5), i64::MAX));
    }

    #[test]
    fn unknown_reject_is_independent_of_supply() {
        let analysis = unresolvable(6);
        assert!(!is_funded(&analysis, 6));
        assert!(!is_funded(&analysis, 9));
    }

    #[test]
    fn is_funded_handles_extreme_supply() {
        assert!(!is_funded(&unresolvable(i64::MAX), i64::MAX));
        assert!(is_funded(&resolvable(1), i64::MAX));
    }

    // ── sig_key: agrees with Sig::lane_hash ────────────────────────────────

    #[test]
    fn sig_key_equals_lane_hash() {
        let sig = atom(7);
        assert_eq!(sig_key(&sig), sig.lane_hash());
    }

    // ── #17 funding slots (§4.7): a slot signature flows through the SAME
    //    generic demand / supply / keying machinery as any ground signature ──

    #[test]
    fn funding_slot_signature_flows_through_generic_demand_and_funding() {
        // §4.7: a funding slot is a fresh unforgeable `new`-created name used AS
        // a signature (`{for(y<-x)P}_{s₁ ⊸ slot}`). Dynamic bound slots are
        // represented by `CostSignature::BoundLevel`; this static unit fixture
        // uses the resolved `Sig::Ground` form. Δ_s counts the scoped COMM
        // introductions and funding is Def 19 `Σ_slot ≥ Δ_slot`; an ABSENT slot pool (Σ = 0)
        // rejects a positive demand (§7.6 — "checks tokens on the
        // slot"), and the slot is keyed by the SAME canonical `lane_hash`/`from_sig`
        // basis as any ground signature. This pins the funding-slot path, which
        // was previously only inferred from the generic machinery (not tested).
        let slot = Sig::Ground(vec![0x5a; 32]); // a fresh slot name used as a signature
        let other_slot = Sig::Ground(vec![0x5b; 32]);
        let client = atom(1);
        // `s₁ ⊸ slot` resolves, at the gate, to the compound envelope `s₁ ∘ slot`.
        let compound = Sig::And(Box::new(client.clone()), Box::new(slot.clone()));

        // K token-consuming COMM nodes (sends) in the desugared body.
        let k: i64 = 3;
        let par = par_with_sends(k as usize);

        // Δ_slot counts the COMM nodes and is fully resolvable (no `*x`).
        let d_slot = demand(&par, &slot);
        assert_eq!(d_slot.certified_upper_bound, k);
        assert!(!d_slot.unknown);

        // The compound slot envelope counts the SAME COMMs (whole-signature
        // attribution, Def 7.4 — the envelope's structure does not change Δ).
        let d_comp = demand(&par, &compound);
        assert_eq!(d_comp.certified_upper_bound, d_slot.certified_upper_bound);
        assert!(!d_comp.unknown);

        // The OSLF funds judgment applies to the slot exactly as to any signature:
        // resolvable demand ⇒ Def 19 `Σ_slot ≥ Δ_slot`.
        assert!(is_funded(&d_slot, k)); // Σ = Δ ⇒ funded
        assert!(is_funded(&d_slot, k + 5));
        assert!(!is_funded(&d_slot, k - 1)); // under-supplied ⇒ rejected
                                             // An ABSENT / empty slot pool (Σ = 0) with positive demand is rejected.
        assert!(!is_funded(&d_slot, 0));

        // The slot is keyed via the same canonical `lane_hash` basis as any
        // ground signature, and distinct slots — and the compound — get distinct,
        // collision-free keys (the slot is unforgeable).
        assert_eq!(sig_key(&slot), slot.lane_hash());
        assert_ne!(sig_key(&slot), sig_key(&other_slot));
        assert_ne!(sig_key(&slot), sig_key(&compound));
    }
}

#[cfg(kani)]
mod kani_funding {
    //! D3 (DR-9) bounded model check of the per-signature funding/settlement
    //! NO-UNDERFLOW property (Commit 2 — replaces the retired
    //! `escrow = limit × price` kani). The settlement debit is the per-COMM
    //! demand `Δ_s`; an admitted deploy's realized balance debit must never
    //! underflow its reserved canonical vault authority (`post = pre − Δ ≥ 0`).
    use super::*;

    #[kani::proof]
    fn funded_settlement_debit_never_underflows_supply() {
        let demand: i64 = kani::any();
        let supply: i64 = kani::any();
        // Bound the inputs to the balance domain modeled by this harness.
        kani::assume(demand >= 0 && demand <= 1_000_000);
        kani::assume(supply >= 0 && supply <= 1_000_000);

        let analysis = DemandEntry {
            certified_upper_bound: demand,
            unknown: false,
        };
        if is_funded(&analysis, supply) {
            // Resolvable demand (`unknown == false`) ⇒ Def 19 `Σ ≥ Δ`, so the
            // settlement write `post = Σ − Δ` is non-negative.
            assert!(supply - demand >= 0);
        }
    }

    #[kani::proof]
    fn resolvable_reject_below_demand() {
        let demand: i64 = kani::any();
        let supply: i64 = kani::any();
        kani::assume(demand >= 0 && demand <= 1_000_000);
        kani::assume(supply >= 0 && supply <= 1_000_000);

        let analysis = DemandEntry {
            certified_upper_bound: demand,
            unknown: false,
        };
        // Resolvable demand: Σ strictly below Δ is not funded.
        if supply < demand {
            assert!(!is_funded(&analysis, supply));
        }
    }

    #[kani::proof]
    fn unknown_demand_is_unprovable() {
        let demand: i64 = kani::any();
        let supply: i64 = kani::any();
        kani::assume(demand >= 0 && demand <= 1_000_000);
        kani::assume(supply >= 0 && supply <= 1_000_000);

        let analysis = DemandEntry {
            certified_upper_bound: demand,
            unknown: true,
        };
        assert!(!is_funded(&analysis, supply));
    }
}
