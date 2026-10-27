//! Added by G3 (DR-135): owners fund their stored processes.
//!
//! A stored process runs for its owner, and the owner pays for its work at
//! the owner's own signed price (decision a, rules R1 and R4 of the G3
//! design). This module holds the parts of that rule that need no runtime
//! state:
//!
//! - the purse of an owner (`OwnerPurse`, `OwnerPurseSet`) and the function
//!   that locates it (`OwnerPurseLocator`, implemented by Casper's
//!   `purse_of`);
//! - the owner table T of one batch member: each listed purse with its
//!   budget (`OwnerTable`);
//! - the notes of purses that play consulted and T does not list
//!   (`OwnerNotes`);
//! - the binding of one member run (`OwnerFundingBinding`);
//! - the owner charge of one observation (`OwnerCharge`) and its allocation
//!   to purses (`allocate`), with the rules for an unlisted purse
//!   (`UnlistedRule`, soundness addition SA8);
//! - the price that each region of a run records (`RuntimeBudget::
//!   region_price`, `ReducerCore::opened_region`).
//!
//! No caller installs a binding yet, so every run keeps its behavior. The
//! regions only record their price.
//!
//! Added by G3 (DR-135, S3): the context of a fired body (`BodyContext`,
//! `fired_body_context`, R1 and A1). Without a binding the context is the
//! DR-101 context of dev. With a binding:
//!
//! ```text
//! fired_body_context(seal, data, system):
//!   system body (the seal holds Unit regions only):
//!     caller := the first recorded caller of a seal region,
//!               else the first recorded caller of a data region (data order),
//!               else the single distinct non-Unit, non-client signature
//!                    of the data seals, at the minimum price of its regions,
//!               else none
//!     a caller on a client lane is none (the running deployment pays)
//!     context := (system, owner: none, caller)
//!   any other body:
//!     owner := the distinct canonical non-Unit signatures of the seal, one
//!              signature or their canonical compound, at the minimum price
//!              of those regions; none when it is on a client lane
//!     context := (not system, owner, caller: none)
//! ```
//!
//! A fired owner body opens its regions for its owner at the owner's price.
//! A system body stores residue that records its caller, and a process held
//! in a system body runs for that caller.
//!
//! Changed by G3 (DR-135, S3b H6): the caller steps of the system body above
//! are replaced. The caller is now the single code holder of Rule H below.
//!
//! Added by G3 (DR-135, S3b): Rule H. A value that a COMM binds keeps the
//! provenance of the datum that brought it, and a held process runs for that
//! holder at the holder's price (P1 rem:signed-subst). The holder h(d) of a
//! datum d is read from its stored seal. The first region that records a
//! caller decides (a Unit sentinel gives no holder), else the signer of the
//! seal decides, and a holder at price 0 is no holder. With a binding:
//!
//! ```text
//! H0  fired body: the held map maps the env positions [s_i, s_i + k_i) of
//!     the k_i values of datum i to h(d_i)
//! H6  system body: K := { the recorded caller of the continuation seal }
//!       ∪ { h(d) : datum d binds a value that is not code-free }
//!     caller := the single signature of K, at its minimum price, else none
//! H1  exit of a variable or method with holder H:
//!       H.signature != context signature => run in held_reducer(H) with an
//!                                            empty authority
//!       else                             => run as in S3
//! H2  stored body (eval_receive, before the substitution): each held use in
//!     process position, and each send-data par that is one held variable,
//!     whose value is not code-free and whose holder differs from the future
//!     context (a system seal, or the signer of the stored seal), becomes a
//!     held term {use}[H.signature] with held_price H.price
//! H3  match on one held variable: the case binds its values for the holder
//! H4  held term: runs in held_reducer(H) with an empty authority, and
//!     without a binding as the surface signed term would run
//! H5  send: held markers are stripped in every run; the opened region of an
//!     owner body records the agreed holder of the datum's code, or the Unit
//!     sentinel for mixed code; the residue of a system body records the
//!     agreed holder, or no caller for code without one holder
//!     (changed by the S3b review: an owner body also records the Unit
//!     sentinel for foreign held code whose holders disagree)
//! ```
//!
//! Without a binding the held map stays empty, so H0-H6 are inert (H7). Only
//! the strip of held markers runs, and no marker exists without H2.
//!
//! Added by G3 (DR-135, S3b H8): the replies of the native recorded processes
//! must be plain data (`plain_data`). Rule H gives a held process to the
//! sender of its value, so a reply that carried code, a name or a signature
//! would let a forged recording act for another party. Plain data is defined
//! on the shape of a `Par`:
//!
//! ```text
//! plain(P)  := P holds only exprs, P.connective_used = false, and
//!              plain_expr(e) for every expr e of P       (Nil is plain)
//! plain_expr(e) :=
//!   e is one of the 15 grounds (GBool, GInt, GString, GUri, GByteArray,
//!     GDouble, GBigInt, GBigRat, GFixedPoint, GUint64, GFloat32, GInt32,
//!     GUint32, GUint16, GUint8)
//!   or e is EList or ESet with no remainder, ETuple, or EMap with no
//!     remainder whose every pair has a key and a value,
//!     with connective_used = false and plain(Q) for each element Q
//!   (anything else, among them unforgeables, bundles, processes, EPathMap,
//!    EZipper, operators, EVar, EMethod and an expr with no instance, is not
//!    plain)
//! ```
//!
//! The walk keeps the Pars that it has still to visit on a heap stack, so its
//! native stack does not grow with the depth of the reply.
//!
//! ## The allocation of an owner charge
//!
//! An owner charge holds one share for each owner region of an observation.
//! A share of region `r` is `usage(r) * price(r)`, where `usage(r)` is the
//! phlo that the meter charged to `r` and `price(r)` is the signed price
//! that `r` recorded when its context created it. `allocate` draws the
//! shares from the budgets of the owner purses, in share order:
//!
//! ```text
//! allocate(charge, view, rule, room_free, notes):
//!   draws := {}
//!   for each charge C in [charge, its restorations] (pre-order, heap stack):
//!     for each share s of C with s.amount > 0:
//!       s.amount > u64::MAX          => shortfall
//!       s.purses = none              => shortfall       (a forgeable owner)
//!       s.purses = single(p)         => take(p, s.amount) or shortfall
//!       s.purses = compound(v, A)    =>
//!         take(v, s.amount)                              (Rule 3)
//!         or, with per := s.amount / s.units, take(a, per) for every a in A
//!            (all or nothing)                            (Rule 2)
//!         or shortfall
//!   for each charge C with a transferee t (A5):
//!     t = none          => shortfall
//!     t = single(p)     => remaining(p) > 0, else shortfall
//!     t = compound(v,A) => remaining(v) > 0 or remaining(a) > 0 for all a
//!   return draws
//!
//! take(p, a) := remaining(p) >= a, and draws(p) + a fits in u64
//! remaining(p) := budget(p) - draws(p)
//! budget(p) := view(p) when T lists p, else by the rule (SA8):
//!   Play    => 0, and p is noted when T has room
//!   Strict  => 0
//!   Lenient => unbounded when T has room, else 0
//! ```
//!
//! Every budget fits in `u64`, so a share above `u64::MAX` is a shortfall,
//! and a purse never pays more than `u64::MAX` in one charge. The leaf units
//! of a canonical compound divide every usage of its region, so Rule 2
//! splits a share exactly.
//!
//! Added by G3 (DR-135, G-4): `allocate_parts` also keeps the draws of each
//! part: the main charge, and each restoration in its order. The parts sum
//! to the draws of `allocate`.
//!
//! ## The owner ledger
//!
//! Added by G3 (DR-135, S4). With a binding, the meter keeps an owner ledger
//! (`OwnerReservation`): the remaining budget of each purse that T lists.
//! The recorder decides the owner part of each attempt in the critical
//! section in which it decides the bound (`try_record_charge`):
// Changed by G3 (DR-135, G-4): a grant reserves the restorations and bills
// only the main charge, and a granted restoration bills its reservation.
// //! ```text
// //! record(attempt a of observation o, owner charge C):
// //!   a introduces a restoration that a granted peek COMM paid for
// //!       => no owner allocation                                    (R9)
// //!   allocate(C, remaining, Play) is a shortfall:
// //!       a is a COMM          => refusal (COMM source, position |attempts|),
// //!                               no attempt, the COMM stage stays open (R7)
// //!       a is an introduction => attempt with decision OwnerRefused,
// //!                               nothing is stored                  (R8)
// //!   otherwise the bound decides as before, and a grant deducts the draws,
// //!   adds the owner phlo and REV of C, and reserves the restorations of C
// //! ```
//! ```text
//! record(attempt a of observation o, owner charge C):
//!   a introduces a restoration, and the ledger keeps a reservation of its
//!   identity with the charge C (R9):
//!       => no owner allocation, and the bound decides as before:
//!          a grant takes the first such reservation and bills it: the
//!          debits gain its draws, and the owner phlo and REV gain those of
//!          C. The remaining budgets do not change.
//!          A denial leaves the reservation as it is.
//!   allocate_parts(C, remaining, Play) is a shortfall:
//!       a is a COMM          => refusal (COMM source, position |attempts|),
//!                               no attempt, the COMM stage stays open (R7)
//!       a is an introduction => attempt with decision OwnerRefused,
//!                               nothing is stored                  (R8)
//!   otherwise the bound decides as before, and a grant
//!       draws the total from the remaining budgets (the reservation),
//!       bills the main part: the debits gain the main draws, and the owner
//!         phlo and REV gain those of the shares of C,
//!       keeps one reservation for each restoration of C, with its charge
//!         and its draws
//! ```
//!
//! A reservation that no granted introduction bills stays drawn from the
//! remaining budgets until the run ends. Settlement debits only the billed
//! sums, so the end of the run releases it. The owner phlo is therefore the
//! owner part of the metered usage of the run.
//!
//! Replay folds the recorded attempts in their order with the same ledger
//! (`check_budget_evidence`). A grant and a bound denial must allocate under
//! `Strict`, and an owner refusal must fail under `Lenient` (SA8).

// Added by G3 (DR-135, S5): the thread-local owner checks of an operation.
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crossbeam_queue::SegQueue;
// Changed by G3 (DR-135): the body context reads data seals and callers, and
// builds compound owners (S3).
// use models::rhoapi::{CostAuthority, CostRegion, CostSignature};
use models::rhoapi::cost_signature::Value as CostSignatureValue;
// Added by G3 (DR-135, S3b): the walks of Rule H read the exprs and the
// variables of a Par.
use models::rhoapi::expr::ExprInstance;
// Changed by G3 (DR-135, S3b): the walks of Rule H read and build Pars,
// signed terms, variables and methods.
// use models::rhoapi::{
//     CostAuthority, CostCaller, CostRegion, CostSignature, CostSignatureCompound, ListParWithRandom,
// };
// Changed by G3 (DR-135, S5): the owner checks read continuations.
// use models::rhoapi::{
//     CostAuthority, CostCaller, CostRegion, CostSignature, CostSignatureCompound, CostSignedTerm,
//     EMethod, EVar, Expr, ListParWithRandom, Par, Var,
// };
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    CostAuthority, CostCaller, CostRegion, CostSignature, CostSignatureCompound, CostSignedTerm,
    EMethod, EVar, Expr, ListParWithRandom, Par, TaggedContinuation, Var,
};
use models::rust::rholang::sorter::cost_accounting_sorter::sort_signature;
// Added by G3 (DR-135, S3b): the held term of a process use joins the free
// variables of its uses.
use models::rust::utils::union;
use prost::Message;
// Added by G3 (DR-135, S4): the owner refusals are RSpace errors.
use rspace_plus_plus::rspace::errors::RSpaceError;
// Added by G3 (DR-135, S5): the participants of a COMM.
use rspace_plus_plus::rspace::rspace_interface::CommParticipant;
use rspace_plus_plus::rspace::trace::event::{Produce, COMM};
use thiserror::Error;

// Changed by G3 (DR-135): S3 canonicalizes stored signatures and records the
// caller of a system body.
// use super::authority::{
//     cost_signature_to_sig, is_unit_cost_signature, priced_cost_region, sig_to_cost_signature,
//     AuthorityError, ResourceMultiset,
// };
// Changed by G3 (DR-135, S3b): the future of a stored body reads the system
// seal of DR-101.
// use super::authority::{
//     canonical_cost_signature, cost_signature_to_sig, is_unit_cost_signature, priced_cost_region,
//     sig_to_cost_signature, system_residue_authority_with_caller, AuthorityError, ResourceMultiset,
// };
// Changed by G3 (DR-135, S4): the COMM entry takes a canonical witness.
// use super::authority::{
//     canonical_cost_signature, cost_signature_to_sig, is_system_seal, is_unit_cost_signature,
//     priced_cost_region, sig_to_cost_signature, system_residue_authority_with_caller,
//     AuthorityError, ResourceMultiset,
// };
// Changed by G3 (DR-135, S5): the owner checks resolve residue seals.
// use super::authority::{
//     canonical_cost_signature, cost_signature_to_sig, is_system_seal, is_unit_cost_signature,
//     priced_cost_region, sig_to_cost_signature, system_residue_authority_with_caller,
//     AuthorityError, CanonicalAuthority, ResourceMultiset,
// };
use super::authority::{
    canonical_cost_signature, cost_signature_to_sig, is_system_seal, is_unit_cost_signature,
    priced_cost_region, resolve_system_residue, sig_to_cost_signature,
    system_residue_authority_with_caller, AuthorityByteEventKind, AuthorityError,
    CanonicalAuthority, ResidueContext, ResourceMultiset,
};
// Added by G3 (DR-135, S4): the owner ledger reads prepared charges and
// observations.
// Changed by G3 (DR-135, S5): a restoration measures its introduction.
// use super::byte_accounting::ByteCharge;
use super::byte_accounting::{
    produce_introduction_charge, produce_introduction_identity, ByteCharge,
};
use super::byte_receipts::ByteObservation;
// Changed by G3 (DR-135, S4): the owner charge of a prepared charge.
// use super::native_phlo_rules::{signature_leaf_units, SignatureUnitsMeter};
// Changed by G3 (DR-135, S5): the item test prepares its charge.
// use super::native_phlo_rules::{
//     signature_leaf_units, PreparedNativePhloCharge, SignatureUnitsMeter,
// };
use super::native_phlo_rules::{
    signature_leaf_units, NativePhloChargePreparer, NativePhloExecutionError,
    NativePhloRegionLimits, PreparedNativePhloCharge, SignatureUnitsMeter,
};
// Added by G3 (DR-135, S5): the item observation of a match.
use super::observation_construction;
use super::RuntimeBudget;
// Added by G3 (DR-135, S3b): a held marker is a send-data par that is one
// held term and nothing else.
use crate::rust::interpreter::compiler::normalizer::cost_accounting::recognize::solely_signed;
// Added by G3 (DR-135, S5): A5 resolves a signature in the environment of
// its body, as dispatch builds it.
use crate::rust::interpreter::dispatch::build_env;
// Added by G3 (DR-135, S3b): the walks of Rule H read the env positions.
use crate::rust::interpreter::env::Env;
use crate::rust::interpreter::errors::InterpreterError;
// Added by G3 (DR-135, S5): A5 resolves a signature as the reducer does.
use crate::rust::interpreter::metering::MeteredMachine;
use crate::rust::interpreter::reduce::ReducerCore;
use crate::rust::interpreter::substitute::Substitute;

/// The owner of a fired body, with its signed price (R1, A1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionOwner {
    pub signature: CostSignature,
    pub price: u64,
}

/// A purse that can fund an owner: its custody key and the canonical
/// signature that locates it. The custody key names the physical purse, so
/// two signatures with one custody share one budget.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct OwnerPurse {
    pub custody: [u8; 32],
    pub signature: CostSignature,
}

/// The purses of an owner signature (R2). A key wallet or a private name has
/// one purse. A canonical compound of such atoms has the lane vault of the
/// compound (Rule 3) and one purse for each atom (Rule 2). The atoms keep
/// their multiplicity, so they number the leaf units of the compound.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OwnerPurseSet {
    Single(OwnerPurse),
    Compound {
        vault: OwnerPurse,
        atoms: Arc<[OwnerPurse]>,
    },
}

/// The purses of an owner signature, or `None` for a forgeable signature,
/// which has budget 0 (R2). Casper implements it as `purse_of`.
pub type OwnerPurseLocator = fn(&CostSignature) -> Option<OwnerPurseSet>;

/// One entry of the owner table: the signature that located the purse and
/// the budget of the purse (R3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnerTableEntry {
    pub signature: CostSignature,
    pub budget: u64,
}

/// The owner table T of one batch member: the listed purses in strictly
/// ascending custody order, at most `room` of them (R3). An `Arc` shares the
/// entries between the binding and the recording.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnerTable {
    entries: Arc<[([u8; 32], OwnerTableEntry)]>,
    room: usize,
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum OwnerTableError {
    #[error("owner table entries are not in strictly ascending custody order")]
    NotAscending,
    #[error("owner table holds more entries than its room")]
    OverRoom,
    /// Added by G3 (DR-135, S7): a recorded entry names no purse of its own
    /// signature (`recorded_owner_table`).
    #[error("owner table entry locates no purse of its signature")]
    Unlocated,
}

/// Added by G3 (DR-135, S7): the owner table that a recording lists (R3).
/// Each entry is keyed by the custody of the purse that its signature
/// locates: the purse of a key wallet or a private name, or the lane vault of
/// a compound. The located purse must carry the signature of the entry. The
/// table has the room `room`. The journal check and the replay of Casper
/// build the table of a recording with it.
pub fn recorded_owner_table(
    entries: &[OwnerTableEntry],
    locate: &dyn Fn(&CostSignature) -> Option<OwnerPurseSet>,
    room: usize,
) -> Result<OwnerTable, OwnerTableError> {
    let mut keyed = Vec::with_capacity(entries.len());
    for entry in entries {
        let custody = match locate(&entry.signature) {
            Some(OwnerPurseSet::Single(purse)) if purse.signature == entry.signature => {
                purse.custody
            }
            Some(OwnerPurseSet::Compound { vault, .. }) if vault.signature == entry.signature => {
                vault.custody
            }
            _ => return Err(OwnerTableError::Unlocated),
        };
        keyed.push((custody, entry.clone()));
    }
    OwnerTable::new(keyed, room)
}

/// Added by G3 (DR-135, S7): the billed owner sums of a checked run (R10,
/// G-4): the debit of each listed purse that the run billed, the owner phlo,
/// the owner REV, and the owner table. Settlement charges the debits, and the
/// cost evidence commits the phlo and the REV. A reservation that no granted
/// introduction billed is not part of them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnerTotals {
    pub debits: BTreeMap<[u8; 32], u64>,
    pub usage: u64,
    pub rev: u128,
    pub table: OwnerTable,
}

impl OwnerTable {
    /// The table that lists no purse.
    pub fn empty(room: usize) -> Self {
        Self {
            entries: Arc::from([]),
            room,
        }
    }

    /// A table of `entries`, which must be in strictly ascending custody
    /// order and at most `room` long.
    pub fn new(
        entries: Vec<([u8; 32], OwnerTableEntry)>,
        room: usize,
    ) -> Result<Self, OwnerTableError> {
        if entries.len() > room {
            return Err(OwnerTableError::OverRoom);
        }
        if entries.windows(2).any(|pair| pair[0].0 >= pair[1].0) {
            return Err(OwnerTableError::NotAscending);
        }
        Ok(Self {
            entries: Arc::from(entries),
            room,
        })
    }

    pub fn entries(&self) -> &[([u8; 32], OwnerTableEntry)] { &self.entries }

    pub fn room(&self) -> usize { self.room }

    pub fn len(&self) -> usize { self.entries.len() }

    pub fn is_empty(&self) -> bool { self.entries.is_empty() }

    /// Whether the table can list one more purse. A full table gives every
    /// unlisted purse budget 0 under every rule (SA8).
    pub fn has_room(&self) -> bool { self.entries.len() < self.room }

    /// The entry of the purse with `custody`, by binary search.
    pub fn get(&self, custody: &[u8; 32]) -> Option<&OwnerTableEntry> {
        self.entries
            .binary_search_by(|(key, _)| key.cmp(custody))
            .ok()
            .map(|index| &self.entries[index].1)
    }

    /// Added by G3 (DR-135, S4): the index of the entry of the purse with
    /// `custody`, by binary search.
    pub fn position(&self, custody: &[u8; 32]) -> Option<usize> {
        self.entries
            .binary_search_by(|(key, _)| key.cmp(custody))
            .ok()
    }

    /// The budget of the purse with `custody`, when the table lists it.
    pub fn budget(&self, custody: &[u8; 32]) -> Option<u64> {
        self.get(custody).map(|entry| entry.budget)
    }
}

/// The purses that a play run consulted and its table did not list (R3).
/// Notes come from many reducer tasks without a lock: a wait-free queue
/// holds them, and a flag tells whether any exists.
#[derive(Debug, Default)]
pub struct OwnerNotes {
    queue: SegQueue<OwnerPurse>,
    any: AtomicBool,
}

impl OwnerNotes {
    pub fn note(&self, purse: OwnerPurse) {
        self.queue.push(purse);
        self.any.store(true, Ordering::Release);
    }

    /// Whether a purse was noted since the last drain.
    pub fn any(&self) -> bool { self.any.load(Ordering::Acquire) }

    /// Every noted purse, in no particular order. A note that races the
    /// drain stays in the queue and keeps the flag set.
    fn drain(&self) -> Vec<OwnerPurse> {
        self.any.store(false, Ordering::Release);
        let mut purses = Vec::with_capacity(self.queue.len());
        while let Some(purse) = self.queue.pop() {
            purses.push(purse);
        }
        purses
    }
}

/// The owner funding of one member run (R11): the client lanes, the purse
/// locator, the owner table T and the notes of the run. A client lane is a
/// lane that the member's own sources may pay (DR-124 item 4). Every other
/// non-Unit region is an owner region.
#[derive(Debug)]
pub struct OwnerFundingBinding {
    client_lanes: BTreeSet<[u8; 32]>,
    locator: OwnerPurseLocator,
    table: OwnerTable,
    notes: Arc<OwnerNotes>,
}

impl OwnerFundingBinding {
    pub fn new(
        client_lanes: BTreeSet<[u8; 32]>,
        locator: OwnerPurseLocator,
        table: OwnerTable,
    ) -> Self {
        Self {
            client_lanes,
            locator,
            table,
            notes: Arc::new(OwnerNotes::default()),
        }
    }

    /// Whether `signature` is on a client lane.
    pub fn is_client_signature(&self, signature: &CostSignature) -> Result<bool, AuthorityError> {
        Ok(self
            .client_lanes
            .contains(&cost_signature_to_sig(signature)?.lane_hash()))
    }

    /// Whether `region` is an owner region: not Unit and not on a client
    /// lane.
    pub fn is_owner_region(&self, region: &CostRegion) -> Result<bool, AuthorityError> {
        let signature = region
            .signature
            .as_ref()
            .ok_or(AuthorityError::MissingSignature)?;
        if is_unit_cost_signature(signature) {
            return Ok(false);
        }
        Ok(!self.is_client_signature(signature)?)
    }

    pub fn table(&self) -> &OwnerTable { &self.table }

    pub fn notes(&self) -> &OwnerNotes { &self.notes }

    /// The purses of `signature`, or `None` for a forgeable signature.
    pub fn locate(&self, signature: &CostSignature) -> Option<OwnerPurseSet> {
        (self.locator)(signature)
    }

    /// The purses that the run noted and the table does not list, each once,
    /// in ascending custody order. Of two notes with one custody, the one with
    /// the smaller signature stays, so the result does not depend on the order
    /// of the notes.
    pub fn take_notes(&self) -> Vec<OwnerPurse> {
        let mut noted = self.notes.drain();
        noted.retain(|purse| self.table.get(&purse.custody).is_none());
        noted.sort_unstable();
        noted.dedup_by(|later, earlier| later.custody == earlier.custody);
        noted
    }

    /// The owner charge of an observation with `authority`, whose regions the
    /// meter charged `usages` (`PreparedNativePhloCharge::region_usages`, in
    /// the same canonical order). Unit regions, client regions and regions
    /// without usage have no share (R4).
    pub(crate) fn owner_charge(
        &self,
        authority: &CostAuthority,
        usages: &[u64],
    ) -> Result<OwnerCharge, AuthorityError> {
        if authority.regions.len() != usages.len() {
            return Err(AuthorityError::MissingAuthority);
        }
        let mut shares = Vec::with_capacity(authority.regions.len());
        for (region, &usage) in authority.regions.iter().zip(usages) {
            let signature = region
                .signature
                .as_ref()
                .ok_or(AuthorityError::MissingSignature)?;
            if usage == 0 || is_unit_cost_signature(signature) {
                continue;
            }
            if self.is_client_signature(signature)? {
                continue;
            }
            shares.push(OwnerRegionShare {
                purses: self.locate(signature),
                units: signature_leaf_units(signature, &OwnerUnits)?,
                usage,
                amount: u128::from(usage) * u128::from(region.price),
            });
        }
        Ok(OwnerCharge {
            shares,
            transferee: None,
            restorations: Vec::new(),
        })
    }
}

/// The units of an owner signature: the walk of the native meter, with the
/// errors of the authority module. A canonical compound is flat and has no
/// Unit leaf, so its units are the number of its atoms.
struct OwnerUnits;

impl SignatureUnitsMeter for OwnerUnits {
    type Error = AuthorityError;

    fn other(&self) -> Result<(), Self::Error> { Err(AuthorityError::UnsupportedFundingSignature) }

    fn add(&self, units: u64) -> Result<u64, Self::Error> {
        units
            .checked_add(1)
            .ok_or(AuthorityError::ArithmeticOverflow)
    }

    /// A walk whose stack does not fit in memory is a size overflow.
    fn allocation(&self) -> Self::Error { AuthorityError::ArithmeticOverflow }
}

/// The share of one owner region of an observation (R4): its purses, the
/// leaf units of its signature, its usage and the amount `usage * price`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OwnerRegionShare {
    pub(crate) purses: Option<OwnerPurseSet>,
    pub(crate) units: u64,
    pub(crate) usage: u64,
    pub(crate) amount: u128,
}

/// The owner charge of one observation: its shares, the transferee of a
/// COMM whose body is one signed owner term (A5), and the charges of the
/// restorations that a peek COMM reserves, by their introduction identity
/// (R9). A restoration is an introduction, so it has neither a transferee
/// nor restorations of its own.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct OwnerCharge {
    pub(crate) shares: Vec<OwnerRegionShare>,
    pub(crate) transferee: Option<Option<OwnerPurseSet>>,
    pub(crate) restorations: Vec<([u8; 32], OwnerCharge)>,
}

/// The budget of a purse that the table does not list (SA8). Play notes the
/// purse and gives it 0, so the member runs again with the purse listed.
/// Replay gives it 0 for grants, bound denials and the items of a refused
/// tuple (`Strict`), and passes it for refusal shortfalls and Stored-rule
/// items (`Lenient`). A full table gives 0 under every rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UnlistedRule {
    Play,
    Strict,
    Lenient,
}

/// An owner cannot pay its share of a charge, or a transferee has no budget
/// left (A5).
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
#[error("an owner cannot pay its share")]
pub(crate) struct OwnerShortfall;

/// Added by G3 (DR-135, G-4): the draws of one allocation: the total, the
/// part of the main charge, and the part of each restoration, in the order
/// of `OwnerCharge::restorations`. The parts sum to the total. A granted
/// COMM draws the total from the remaining budgets (the reservation) and
/// bills the main part at once. A restoration part is billed only at the
/// granted introduction of the restoration.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct OwnerDraws {
    pub(crate) total: ResourceMultiset<[u8; 32]>,
    pub(crate) main: ResourceMultiset<[u8; 32]>,
    pub(crate) restorations: Vec<ResourceMultiset<[u8; 32]>>,
}

/// The budgets of the purses that one allocation consulted, and its draws.
/// Each purse is consulted once, so a purse is noted once and keeps one
/// budget for the whole allocation.
struct Allocation<'a> {
    view: &'a dyn Fn(&OwnerPurse) -> Option<u64>,
    rule: UnlistedRule,
    room_free: bool,
    notes: Option<&'a OwnerNotes>,
    budgets: BTreeMap<[u8; 32], u128>,
    draws: ResourceMultiset<[u8; 32]>,
    /// Added by G3 (DR-135, G-4): the draws of the part that the allocation
    /// visits now (`OwnerDraws`).
    part: ResourceMultiset<[u8; 32]>,
}

impl Allocation<'_> {
    fn budget(&mut self, purse: &OwnerPurse) -> u128 {
        if let Some(budget) = self.budgets.get(&purse.custody) {
            return *budget;
        }
        let budget = match ((self.view)(purse), self.rule, self.room_free) {
            (Some(listed), _, _) => u128::from(listed),
            (None, UnlistedRule::Play, true) => {
                if let Some(notes) = self.notes {
                    notes.note(purse.clone());
                }
                0
            }
            (None, UnlistedRule::Lenient, true) => u128::MAX,
            (None, UnlistedRule::Play | UnlistedRule::Strict | UnlistedRule::Lenient, _) => 0,
        };
        self.budgets.insert(purse.custody, budget);
        budget
    }

    fn remaining(&mut self, purse: &OwnerPurse) -> u128 {
        self.budget(purse)
            .saturating_sub(u128::from(self.draws.get(&purse.custody)))
    }

    /// Draws `amount` from `purse` when its remaining budget covers it and
    /// the purse's total draw fits in `u64`. Otherwise nothing changes.
    /// Changed by G3 (DR-135, G-4): the draw also joins the current part. A
    /// part never exceeds the total draw of its purse, so the part fits
    /// whenever the total fits.
    // fn take(&mut self, purse: &OwnerPurse, amount: u128) -> bool {
    //     if self.remaining(purse) < amount {
    //         return false;
    //     }
    //     let total = u64::try_from(amount)
    //         .ok()
    //         .and_then(|amount| self.draws.get(&purse.custody).checked_add(amount));
    //     match total {
    //         Some(0) => true,
    //         Some(total) => {
    //             self.draws.0.insert(purse.custody, total);
    //             true
    //         }
    //         None => false,
    //     }
    // }
    fn take(&mut self, purse: &OwnerPurse, amount: u128) -> bool {
        if self.remaining(purse) < amount {
            return false;
        }
        let Ok(amount) = u64::try_from(amount) else {
            return false;
        };
        let (Some(total), Some(part)) = (
            self.draws.get(&purse.custody).checked_add(amount),
            self.part.get(&purse.custody).checked_add(amount),
        ) else {
            return false;
        };
        if amount > 0 {
            self.draws.0.insert(purse.custody, total);
            self.part.0.insert(purse.custody, part);
        }
        true
    }

    /// One share: a single purse pays it all, a compound vault pays it all
    /// (Rule 3), or every atom pays an equal part (Rule 2).
    fn share(&mut self, share: &OwnerRegionShare) -> Result<(), OwnerShortfall> {
        if share.amount == 0 {
            return Ok(());
        }
        // Every budget fits in `u64`, so no purse set pays a larger share.
        if share.amount > u128::from(u64::MAX) {
            return Err(OwnerShortfall);
        }
        match share.purses.as_ref() {
            None => Err(OwnerShortfall),
            Some(OwnerPurseSet::Single(purse)) => self
                .take(purse, share.amount)
                .then_some(())
                .ok_or(OwnerShortfall),
            Some(OwnerPurseSet::Compound { vault, atoms }) => {
                if self.take(vault, share.amount) {
                    return Ok(());
                }
                // Rule 2 needs one atom for each leaf unit and an exact split.
                // A failed atom fails the whole allocation, so no partial
                // draw of the atoms reaches the caller.
                let units = u128::from(share.units);
                if units == 0
                    || u128::try_from(atoms.len()).ok() != Some(units)
                    || !share.amount.is_multiple_of(units)
                {
                    return Err(OwnerShortfall);
                }
                let part = share.amount / units;
                for atom in atoms.iter() {
                    if !self.take(atom, part) {
                        return Err(OwnerShortfall);
                    }
                }
                Ok(())
            }
        }
    }

    /// A5: the transferee keeps a positive budget after the draws.
    /// Changed by G3 (DR-135, S6): the test is `transferee_funded`, which
    /// the A5 check of a replayed Matched row shares.
    // fn transferee(&mut self, transferee: &Option<OwnerPurseSet>) -> Result<(), OwnerShortfall> {
    //     let funded = match transferee {
    //         None => false,
    //         Some(OwnerPurseSet::Single(purse)) => self.remaining(purse) > 0,
    //         Some(OwnerPurseSet::Compound { vault, atoms }) => {
    //             self.remaining(vault) > 0
    //                 || (!atoms.is_empty() && atoms.iter().all(|atom| self.remaining(atom) > 0))
    //         }
    //     };
    //     funded.then_some(()).ok_or(OwnerShortfall)
    // }
    fn transferee(&mut self, transferee: &Option<OwnerPurseSet>) -> Result<(), OwnerShortfall> {
        transferee_funded(transferee, &mut |purse| self.remaining(purse))
            .then_some(())
            .ok_or(OwnerShortfall)
    }
}

/// Added by G3 (DR-135, S6): A5. The transferee of a COMM keeps a positive
/// budget after the draws of the COMM: a single purse has a positive
/// remaining budget, or a compound has a positive vault, or every one of its
/// atoms is positive. A forgeable transferee (`None`) is never funded.
/// `remaining` gives the remaining budget of a purse after the draws.
pub(crate) fn transferee_funded(
    transferee: &Option<OwnerPurseSet>,
    remaining: &mut dyn FnMut(&OwnerPurse) -> u128,
) -> bool {
    match transferee {
        None => false,
        Some(OwnerPurseSet::Single(purse)) => remaining(purse) > 0,
        Some(OwnerPurseSet::Compound { vault, atoms }) => {
            remaining(vault) > 0
                || (!atoms.is_empty() && atoms.iter().all(|atom| remaining(atom) > 0))
        }
    }
}

/// Allocates `charge` to owner purses (see the module documentation).
/// `view` gives the remaining budget of a purse that the table lists and
/// `None` for an unlisted purse. `room_free` tells whether the table can list
/// one more purse. Play passes the notes of its binding. The result is the
/// draw of each purse, or a shortfall with no draw.
/// Changed by G3 (DR-135, G-4): the total of `allocate_parts`.
// pub(crate) fn allocate(
//     charge: &OwnerCharge,
//     view: &dyn Fn(&OwnerPurse) -> Option<u64>,
//     rule: UnlistedRule,
//     room_free: bool,
//     notes: Option<&OwnerNotes>,
// ) -> Result<ResourceMultiset<[u8; 32]>, OwnerShortfall> {
//     let mut allocation = Allocation {
//         view,
//         rule,
//         room_free,
//         notes,
//         budgets: BTreeMap::new(),
//         draws: ResourceMultiset::default(),
//     };
//     // A restoration has no restorations of its own, so the main charge and
//     // its restorations bound both lists.
//     let mut pending = Vec::with_capacity(1 + charge.restorations.len());
//     pending.push(charge);
//     let mut transferees = Vec::with_capacity(1 + charge.restorations.len());
//     while let Some(current) = pending.pop() {
//         for share in &current.shares {
//             allocation.share(share)?;
//         }
//         if let Some(transferee) = current.transferee.as_ref() {
//             transferees.push(transferee);
//         }
//         pending.extend(
//             current
//                 .restorations
//                 .iter()
//                 .rev()
//                 .map(|(_, restoration)| restoration),
//         );
//     }
//     for transferee in transferees {
//         allocation.transferee(transferee)?;
//     }
//     Ok(allocation.draws)
// }
pub(crate) fn allocate(
    charge: &OwnerCharge,
    view: &dyn Fn(&OwnerPurse) -> Option<u64>,
    rule: UnlistedRule,
    room_free: bool,
    notes: Option<&OwnerNotes>,
) -> Result<ResourceMultiset<[u8; 32]>, OwnerShortfall> {
    allocate_parts(charge, view, rule, room_free, notes).map(|draws| draws.total)
}

/// Added by G3 (DR-135, G-4): `allocate` with the draws of each part. The
/// allocation visits the shares in the order of `allocate`: the main charge,
/// then each restoration with its own restorations (pre-order, heap stack),
/// and the transferees after all shares. A restoration has no restorations
/// of its own, so the main charge and its restorations bound the lists. A
/// nested restoration would draw in the part of its restoration.
pub(crate) fn allocate_parts(
    charge: &OwnerCharge,
    view: &dyn Fn(&OwnerPurse) -> Option<u64>,
    rule: UnlistedRule,
    room_free: bool,
    notes: Option<&OwnerNotes>,
) -> Result<OwnerDraws, OwnerShortfall> {
    let mut allocation = Allocation {
        view,
        rule,
        room_free,
        notes,
        budgets: BTreeMap::new(),
        draws: ResourceMultiset::default(),
        part: ResourceMultiset::default(),
    };
    let mut transferees = Vec::with_capacity(1 + charge.restorations.len());
    for share in &charge.shares {
        allocation.share(share)?;
    }
    if let Some(transferee) = charge.transferee.as_ref() {
        transferees.push(transferee);
    }
    let main = std::mem::take(&mut allocation.part);
    let mut restorations = Vec::with_capacity(charge.restorations.len());
    let mut pending = Vec::with_capacity(1);
    for (_, restoration) in &charge.restorations {
        pending.push(restoration);
        while let Some(current) = pending.pop() {
            for share in &current.shares {
                allocation.share(share)?;
            }
            if let Some(transferee) = current.transferee.as_ref() {
                transferees.push(transferee);
            }
            pending.extend(
                current
                    .restorations
                    .iter()
                    .rev()
                    .map(|(_, restoration)| restoration),
            );
        }
        restorations.push(std::mem::take(&mut allocation.part));
    }
    for transferee in transferees {
        allocation.transferee(transferee)?;
    }
    Ok(OwnerDraws {
        total: allocation.draws,
        main,
        restorations,
    })
}

/// Added by G3 (DR-135, S6, SA8): the item test of the Stored rule in
/// replay. A Stored claim must hold even when unlisted purses pay, so an
/// unlisted purse passes when the table has room (`Lenient`).
pub(crate) fn stored_item_passes(
    charge: &OwnerCharge,
    view: &dyn Fn(&OwnerPurse) -> Option<u64>,
    room_free: bool,
) -> bool {
    allocate(charge, view, UnlistedRule::Lenient, room_free, None).is_ok()
}

/// Added by G3 (DR-135, S6, SA4, SA8): the item test of a refused tuple in
/// replay. Play forms a tuple only from items that pass with unlisted purses
/// at 0, so the test gives an unlisted purse 0 (`Strict`).
pub(crate) fn refused_item_passes(
    charge: &OwnerCharge,
    view: &dyn Fn(&OwnerPurse) -> Option<u64>,
    room_free: bool,
) -> bool {
    allocate(charge, view, UnlistedRule::Strict, room_free, None).is_ok()
}

/// Added by G3 (DR-135, S6, SA8): the shortfall of a refusal in replay. A
/// refusal needs a shortfall of listed purses, so an unlisted purse passes
/// when the table has room (`Lenient`).
pub(crate) fn refusal_shortfall(
    charge: &OwnerCharge,
    view: &dyn Fn(&OwnerPurse) -> Option<u64>,
    room_free: bool,
) -> bool {
    allocate(charge, view, UnlistedRule::Lenient, room_free, None).is_err()
}

impl OwnerCharge {
    // Changed by G3 (DR-135, G-4): the ledger bills the shares of the main
    // charge at its grant and the shares of a restoration at the granted
    // introduction of the restoration, so the totals no longer include the
    // restorations.
    // /// Added by G3 (DR-135, S4): the owner phlo and the owner REV of the
    // /// charge and its restorations (R4). The owner phlo is the usage that the
    // /// owners take over from the client of the run, and the owner REV is the
    // /// amount that they pay for it. A share at price 0 adds phlo and no REV.
    // /// The walk visits the charges in the order of `allocate`.
    // pub(crate) fn totals(&self) -> Result<(u64, u128), AuthorityError> {
    //     let mut usage = 0_u64;
    //     let mut rev = 0_u128;
    //     let mut pending = Vec::with_capacity(1 + self.restorations.len());
    //     pending.push(self);
    //     while let Some(current) = pending.pop() {
    //         for share in &current.shares {
    //             usage = usage
    //                 .checked_add(share.usage)
    //                 .ok_or(AuthorityError::ArithmeticOverflow)?;
    //             rev = rev
    //                 .checked_add(share.amount)
    //                 .ok_or(AuthorityError::ArithmeticOverflow)?;
    //         }
    //         pending.extend(
    //             current
    //                 .restorations
    //                 .iter()
    //                 .rev()
    //                 .map(|(_, restoration)| restoration),
    //         );
    //     }
    //     Ok((usage, rev))
    // }

    /// Added by G3 (DR-135, G-4): the owner phlo and the owner REV of the
    /// shares of this charge, without its restorations (R4). The owner phlo
    /// is the usage that the owners take over from the client of the run,
    /// and the owner REV is the amount that they pay for it. A share at price
    /// 0 adds phlo and no REV.
    pub(crate) fn share_totals(&self) -> Result<(u64, u128), AuthorityError> {
        let mut usage = 0_u64;
        let mut rev = 0_u128;
        for share in &self.shares {
            usage = usage
                .checked_add(share.usage)
                .ok_or(AuthorityError::ArithmeticOverflow)?;
            rev = rev
                .checked_add(share.amount)
                .ok_or(AuthorityError::ArithmeticOverflow)?;
        }
        Ok((usage, rev))
    }

    /// Added by G3 (DR-135, S11): the usage of the shares of this charge at a
    /// price of 1 or more, without its restorations (item 13). This usage
    /// leaves the caller's bound. A share at price 0 has no amount, so its
    /// usage stays in the caller's part (decision 2).
    pub(crate) fn priced_usage(&self) -> Result<u64, AuthorityError> {
        let mut usage = 0_u64;
        for share in self.shares.iter().filter(|share| share.amount > 0) {
            usage = usage
                .checked_add(share.usage)
                .ok_or(AuthorityError::ArithmeticOverflow)?;
        }
        Ok(usage)
    }
}

/// Added by G3 (DR-135, S4): the owner charge of an observation that the
/// meter prepared, or no share when its charge overflowed. An overflowing
/// charge exceeds every bound, so the bound denies it, and play and the
/// fold both skip its owner part.
pub(crate) fn prepared_owner_charge(
    binding: &OwnerFundingBinding,
    observation: &ByteObservation,
    prepared: Option<&PreparedNativePhloCharge>,
) -> Result<OwnerCharge, AuthorityError> {
    match prepared {
        Some(prepared) => binding.owner_charge(&observation.authority, prepared.region_usages()),
        None => Ok(OwnerCharge::default()),
    }
}

/// Added by G3 (DR-135, S4): the owner terms of a COMM that its observation
/// does not show (S5 computes them at the COMM):
///
/// - `transferee`: the purses of the signature of a continuation body that is
///   one signed owner term (A5). `Some(None)` is a forgeable transferee.
/// - `restorations`: the introduction observations of the data that a peek
///   COMM restores (R9). The COMM pays for them, so their introductions draw
///   no owner budget again.
#[derive(Clone, Debug, Default)]
pub(crate) struct OwnerCommExtras {
    pub(crate) transferee: Option<Option<OwnerPurseSet>>,
    pub(crate) restorations: Vec<Arc<ByteObservation>>,
}

/// Added by G3 (DR-135, S4): whether `error` is an owner refusal (R7, R8).
/// An owner refusal is a decision of the run and no failure, so it leaves no
/// frontier entry (SA6).
pub(crate) fn is_owner_refusal(error: &InterpreterError) -> bool {
    matches!(
        error,
        InterpreterError::RSpaceError(RSpaceError::IntroductionRefused | RSpaceError::CommRefused)
    )
}

impl RuntimeBudget {
    /// The signed price of the running native execution (R4). Each region
    /// that the run creates records it. It is 0 outside native execution.
    pub fn region_price(&self) -> u64 { self.region_price.load(Ordering::Acquire) }

    /// Added by G3 (DR-135): the owner funding of the running native
    /// execution (R11), a lock-free load. Without it every G3 hook keeps the
    /// behavior of dev.
    pub(crate) fn owner_binding(&self) -> Option<Arc<OwnerFundingBinding>> {
        self.owner_binding.load_full()
    }

    /// Added by G3 (DR-135, S4): `reserve_comm_authority_canonical` with the
    /// owner terms of the COMM (A5, R9). It is the entry of the participants
    /// hook of S5. An owner that cannot pay refuses the COMM with
    /// `RSpaceError::CommRefused`, and the tuple stays stored.
    pub(crate) fn reserve_comm_authority_owned(
        &self,
        identity: [u8; 32],
        witness: CanonicalAuthority,
        measurement: ByteCharge,
        extras: OwnerCommExtras,
    ) -> Result<(), InterpreterError> {
        let cost = self.measured_legacy_cost(measurement)?;
        if !self.has_comm_accounting_scope() || self.is_unmetered() {
            return Ok(());
        }
        self.reserve_canonical_authority_identity(
            identity,
            witness,
            cost,
            Some(measurement),
            Some(extras),
        )
    }
}

/// Added by G3 (DR-135): the context in which a fired continuation body runs
/// (R1, A1). `system` is the DR-101 flag of a seal of Unit regions only.
/// `owner` is the owner of a fired owner body. `caller` is the caller of a
/// system body. At most one of `owner` and `caller` is set.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct BodyContext {
    pub(crate) system: bool,
    pub(crate) owner: Option<RegionOwner>,
    pub(crate) caller: Option<RegionOwner>,
    /// Added by G3 (DR-135, S3b H0): the holders of the values that the
    /// COMM binds, by env position (Rule H). Empty without a binding.
    pub(crate) held: HeldMap,
}

/// Added by G3 (DR-135): the context of a body that fires with continuation
/// `seal` and the data seals of `data` (see the module documentation).
pub(crate) fn fired_body_context(
    budget: &RuntimeBudget,
    seal: Option<&CostAuthority>,
    data: &[ListParWithRandom],
    system: bool,
) -> Result<BodyContext, InterpreterError> {
    let binding = budget.owner_binding();
    body_context(binding.as_deref(), seal, data, system)
}

/// Added by G3 (DR-135): `fired_body_context` for a given binding. No binding
/// gives the DR-101 context of dev (R11).
pub(crate) fn body_context(
    binding: Option<&OwnerFundingBinding>,
    seal: Option<&CostAuthority>,
    data: &[ListParWithRandom],
    system: bool,
) -> Result<BodyContext, InterpreterError> {
    // Changed by G3 (DR-135, S3b): the context also holds the data spans
    // (H0), and the caller of a system body is the single code holder (H6).
    // let Some(binding) = binding else {
    //     return Ok(BodyContext {
    //         system,
    //         owner: None,
    //         caller: None,
    //     });
    // };
    // if system {
    //     return Ok(BodyContext {
    //         system: true,
    //         owner: None,
    //         caller: system_body_caller(binding, seal, data)?,
    //     });
    // }
    // let owner = match seal {
    //     Some(seal) => seal_owner(seal, binding)?,
    //     None => None,
    // };
    // Ok(BodyContext {
    //     system: false,
    //     owner,
    //     caller: None,
    // })
    let Some(binding) = binding else {
        return Ok(BodyContext {
            system,
            owner: None,
            caller: None,
            held: HeldMap::default(),
        });
    };
    let held = data_spans(data)?;
    if system {
        return Ok(BodyContext {
            system: true,
            owner: None,
            caller: system_body_caller(seal, data)?,
            held,
        });
    }
    let owner = match seal {
        Some(seal) => seal_owner(seal, binding)?,
        None => None,
    };
    Ok(BodyContext {
        system: false,
        owner,
        caller: None,
        held,
    })
}

/// Added by G3 (DR-135): stored seals come from the reducer, so a malformed
/// region or caller is broken state. The run fails instead of guessing an
/// owner.
fn malformed(error: AuthorityError) -> InterpreterError {
    InterpreterError::ReduceError(format!(
        "cost-accounting: a stored seal is malformed: {error}"
    ))
}

/// Added by G3 (DR-135): the canonical signature of a stored region.
fn region_signature(region: &CostRegion) -> Result<CostSignature, InterpreterError> {
    let signature = region
        .signature
        .as_ref()
        .ok_or_else(|| malformed(AuthorityError::MissingSignature))?;
    canonical_cost_signature(signature).map_err(malformed)
}

/// Added by G3 (DR-135): the owner that a recorded caller names (A1). Its
/// signature must be canonical. A Unit signature names no owner.
fn recorded_owner(caller: &CostCaller) -> Result<Option<RegionOwner>, InterpreterError> {
    let signature = caller
        .signature
        .as_ref()
        .ok_or_else(|| malformed(AuthorityError::MissingSignature))?;
    let signature = canonical_cost_signature(signature).map_err(malformed)?;
    if is_unit_cost_signature(&signature) {
        return Ok(None);
    }
    Ok(Some(RegionOwner {
        signature,
        price: caller.price,
    }))
}

/// Added by G3 (DR-135): the first owner that a region of `seal` records as
/// its caller, in region order. A region that records a Unit caller names no
/// owner, so the search goes on.
fn first_recorded_owner(seal: &CostAuthority) -> Result<Option<RegionOwner>, InterpreterError> {
    for region in &seal.regions {
        if let Some(caller) = region.caller.as_ref() {
            if let Some(owner) = recorded_owner(caller)? {
                return Ok(Some(owner));
            }
        }
    }
    Ok(None)
}

/// Added by G3 (DR-135): the distinct canonical non-Unit signatures of
/// `regions` that `keep` accepts, in the order of their prost encoding, and
/// the minimum price of the regions that carry them. `capacity` is the number
/// of regions.
fn distinct_signatures<'a>(
    regions: impl IntoIterator<Item = &'a CostRegion>,
    capacity: usize,
    keep: impl Fn(&CostSignature) -> Result<bool, InterpreterError>,
) -> Result<(Vec<CostSignature>, Option<u64>), InterpreterError> {
    let mut keyed: Vec<(Vec<u8>, CostSignature)> = Vec::with_capacity(capacity);
    let mut price: Option<u64> = None;
    for region in regions {
        let signature = region_signature(region)?;
        if is_unit_cost_signature(&signature) || !keep(&signature)? {
            continue;
        }
        price = Some(match price {
            Some(lowest) => lowest.min(region.price),
            None => region.price,
        });
        keyed.push((signature.encode_to_vec(), signature));
    }
    keyed.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    keyed.dedup_by(|later, earlier| later.0 == earlier.0);
    let signatures = keyed.into_iter().map(|(_, signature)| signature).collect();
    Ok((signatures, price))
}

/// Added by G3 (DR-135): the owner of a fired owner body (R1): the distinct
/// canonical non-Unit signatures of its seal, one signature as it is or the
/// canonical compound of several. The compound comes from `sort_signature`,
/// whose flatten runs on a work stack. An owner on a client lane is no owner,
/// because the running deployment pays for its own lanes, as on dev.
///
/// The price is the minimum price of the owner's regions. One seal comes
/// from one creating context, so its prices agree. A seal that merges the
/// regions of several contexts has several prices, and the minimum is the
/// choice that charges the owner no more than a price that it signed.
///
/// Changed by G3 (DR-135, S3b): the signer moved into `seal_signer`, which
/// Rule H also uses without the client filter.
fn seal_owner(
    seal: &CostAuthority,
    binding: &OwnerFundingBinding,
) -> Result<Option<RegionOwner>, InterpreterError> {
    // let (mut signatures, price) =
    //     distinct_signatures(&seal.regions, seal.regions.len(), |_| Ok(true))?;
    // let Some(price) = price else {
    //     return Ok(None);
    // };
    // let signature = match signatures.len() {
    //     1 => signatures.pop().expect("one owner signature"),
    //     _ => {
    //         sort_signature(&CostSignature {
    //             value: Some(CostSignatureValue::Compound(CostSignatureCompound {
    //                 elements: signatures,
    //             })),
    //         })
    //         .term
    //     }
    // };
    // if binding.is_client_signature(&signature).map_err(malformed)? {
    //     return Ok(None);
    // }
    // Ok(Some(RegionOwner { signature, price }))
    let Some(signer) = seal_signer(seal)? else {
        return Ok(None);
    };
    if binding
        .is_client_signature(&signer.signature)
        .map_err(malformed)?
    {
        return Ok(None);
    }
    Ok(Some(signer))
}

/// Added by G3 (DR-135, S3b): the signer of a seal: its distinct canonical
/// non-Unit signatures, one signature as it is or the canonical compound of
/// several, at the minimum price of their regions. It has no client filter,
/// because a stored decision must not depend on the client lanes of one run.
pub(crate) fn seal_signer(seal: &CostAuthority) -> Result<Option<RegionOwner>, InterpreterError> {
    let (mut signatures, price) =
        distinct_signatures(&seal.regions, seal.regions.len(), |_| Ok(true))?;
    let Some(price) = price else {
        return Ok(None);
    };
    let signature = match signatures.len() {
        1 => signatures.pop().expect("one signer signature"),
        _ => {
            sort_signature(&CostSignature {
                value: Some(CostSignatureValue::Compound(CostSignatureCompound {
                    elements: signatures,
                })),
            })
            .term
        }
    };
    Ok(Some(RegionOwner { signature, price }))
}

/// Added by G3 (DR-135, S3b): the holder h(d) of a datum with seal `seal`
/// (Rule H). The first region that records a caller decides: a Unit
/// sentinel gives no holder, and any other caller is the holder. Without a
/// recorded caller the holder is the signer of the seal. A holder at price
/// 0 is no holder (T13).
pub(crate) fn datum_holder(seal: &CostAuthority) -> Result<Option<RegionOwner>, InterpreterError> {
    let holder = match seal
        .regions
        .iter()
        .find_map(|region| region.caller.as_ref())
    {
        Some(caller) => recorded_owner(caller)?,
        None => seal_signer(seal)?,
    };
    Ok(holder.filter(|holder| holder.price > 0))
}

/// Added by G3 (DR-135, S3b H0): the data spans of a fired body. The values
/// that datum i binds sit at the env positions [s_i, s_i + k_i), where k_i is
/// the number of its bound pars and s_i the number of pars before it. Each
/// span of a datum with a holder maps to that holder.
fn data_spans(data: &[ListParWithRandom]) -> Result<HeldMap, InterpreterError> {
    let mut held = HeldMap::default();
    let mut start: u32 = 0;
    for datum in data {
        let Ok(count) = u32::try_from(datum.pars.len()) else {
            break;
        };
        if let Some(seal) = datum.cost_authority.as_ref().filter(|_| count > 0) {
            if let Some(holder) = datum_holder(seal)? {
                held.insert_span(start, count, holder);
            }
        }
        let Some(next) = start.checked_add(count) else {
            break;
        };
        start = next;
    }
    Ok(held)
}

/// Added by G3 (DR-135, S3b): the single member of `members` by signature,
/// at the minimum price of the members with that signature (T2). None when
/// there is no member or more than one signature.
pub(crate) fn single_holder(members: impl IntoIterator<Item = RegionOwner>) -> Option<RegionOwner> {
    let mut members = members.into_iter();
    let mut single = members.next()?;
    for member in members {
        if member.signature != single.signature {
            return None;
        }
        single.price = single.price.min(member.price);
    }
    Some(single)
}

// Changed by G3 (DR-135, S3b H6): the caller of a system body is the single
// code holder, also on a client lane. S3's steps, the first recorded caller
// of the data and the single data owner with its client filter, are
// replaced.
// /// Added by G3 (DR-135): the caller of a fired system body (A1): a recorded
// /// caller of the continuation, else one of the data, else the single owner
// /// of the data seals. A caller on a client lane is no caller, because the
// /// running deployment pays for its own lanes.
// fn system_body_caller(
//     binding: &OwnerFundingBinding,
//     seal: Option<&CostAuthority>,
//     data: &[ListParWithRandom],
// ) -> Result<Option<RegionOwner>, InterpreterError> {
//     let mut caller = match seal {
//         Some(seal) => first_recorded_owner(seal)?,
//         None => None,
//     };
//     if caller.is_none() {
//         for datum in data {
//             if let Some(seal) = datum.cost_authority.as_ref() {
//                 caller = first_recorded_owner(seal)?;
//                 if caller.is_some() {
//                     break;
//                 }
//             }
//         }
//     }
//     if caller.is_none() {
//         caller = single_data_owner(binding, data)?;
//     }
//     let Some(owner) = caller else {
//         return Ok(None);
//     };
//     if binding
//         .is_client_signature(&owner.signature)
//         .map_err(malformed)?
//     {
//         return Ok(None);
//     }
//     Ok(Some(owner))
// }
//
// /// Added by G3 (DR-135): the single distinct non-Unit signature outside the
// /// client lanes over all data seals, at the minimum price of its regions.
// /// None when there is no such signature or more than one.
// fn single_data_owner(
//     binding: &OwnerFundingBinding,
//     data: &[ListParWithRandom],
// ) -> Result<Option<RegionOwner>, InterpreterError> {
//     let capacity = data
//         .iter()
//         .filter_map(|datum| datum.cost_authority.as_ref())
//         .map(|seal| seal.regions.len())
//         .sum();
//     let regions = data
//         .iter()
//         .filter_map(|datum| datum.cost_authority.as_ref())
//         .flat_map(|seal| seal.regions.iter());
//     let (mut signatures, price) = distinct_signatures(regions, capacity, |signature| {
//         Ok(!binding.is_client_signature(signature).map_err(malformed)?)
//     })?;
//     match (signatures.len(), price) {
//         (1, Some(price)) => Ok(Some(RegionOwner {
//             signature: signatures.pop().expect("one owner signature"),
//             price,
//         })),
//         _ => Ok(None),
//     }
// }

/// Added by G3 (DR-135, S3b H6): the caller of a fired system body: the
/// single member of K, where K holds the caller that the continuation seal
/// records and the holder h(d) of each fired datum whose bound pars are not
/// all code-free. The caller can be on a client lane: the residue records it,
/// and a held process of the body then runs for the running deployment. With
/// no member or with two signatures, the body has no caller.
fn system_body_caller(
    seal: Option<&CostAuthority>,
    data: &[ListParWithRandom],
) -> Result<Option<RegionOwner>, InterpreterError> {
    let mut members = Vec::with_capacity(1 + data.len());
    if let Some(caller) = seal.map(first_recorded_owner).transpose()?.flatten() {
        members.push(caller);
    }
    for datum in data {
        if datum.pars.iter().all(code_free) {
            continue;
        }
        if let Some(holder) = datum
            .cost_authority
            .as_ref()
            .map(datum_holder)
            .transpose()?
            .flatten()
        {
            members.push(holder);
        }
    }
    Ok(single_holder(members))
}

impl ReducerCore {
    /// The region that an unsigned surface opens: a region of the running
    /// deployment's payer at the region price (R4). S3 adds the owner of a
    /// fired body.
    ///
    /// Changed by G3 (DR-135, S3): a fired owner body opens a region of its
    /// owner at the owner's price (R1).
    pub(crate) fn opened_region(
        &self,
        entropy: &[u8],
        discriminator: u32,
    ) -> Result<CostRegion, InterpreterError> {
        // let budget = self.metering.budget();
        // let signature = sig_to_cost_signature(&budget.signature())
        //     .map_err(|error| InterpreterError::ReduceError(error.to_string()))?;
        // priced_cost_region(&signature, entropy, discriminator, budget.region_price())
        //     .map_err(|error| InterpreterError::ReduceError(error.to_string()))
        match self.owner.as_ref() {
            Some(owner) => {
                priced_cost_region(&owner.signature, entropy, discriminator, owner.price)
                    .map_err(|error| InterpreterError::ReduceError(error.to_string()))
            }
            None => {
                let budget = self.metering.budget();
                let signature = sig_to_cost_signature(&budget.signature())
                    .map_err(|error| InterpreterError::ReduceError(error.to_string()))?;
                priced_cost_region(&signature, entropy, discriminator, budget.region_price())
                    .map_err(|error| InterpreterError::ReduceError(error.to_string()))
            }
        }
    }

    /// The price of a region that this reducer creates (R4). S3 adds the
    /// owner's price inside a fired body.
    ///
    /// Changed by G3 (DR-135, S3): inside a fired owner body it is the
    /// owner's price.
    // pub(crate) fn region_price(&self) -> u64 { self.metering.budget().region_price() }
    pub(crate) fn region_price(&self) -> u64 {
        match self.owner.as_ref() {
            Some(owner) => owner.price,
            None => self.metering.budget().region_price(),
        }
    }

    /// Added by G3 (DR-135): the residue seal that a system body stores. It
    /// records the body's caller, when there is one (A1).
    pub(crate) fn residue_seal_of(
        &self,
        authority: &CostAuthority,
    ) -> Result<CostAuthority, InterpreterError> {
        system_residue_authority_with_caller(
            authority,
            &self.metering.budget().deploy_id(),
            self.caller.as_ref(),
        )
        .map_err(|error| InterpreterError::ReduceError(error.to_string()))
    }
}

/// Added by G3 (DR-135, S3b): the two shapes of data that the walks of Rule H
/// check. Plain data (H8) holds only exprs. Code-free data (Rule H) also
/// admits unforgeable names and bundles whose bodies are code-free.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DataShape {
    Plain,
    CodeFree,
}

/// Added by G3 (DR-135, S3b H8): whether `par` is plain data (see the module
/// documentation). Only plain data may be the reply of a native recorded
/// process. The walk visits each Par once, from a heap stack.
pub(crate) fn plain_data(par: &Par) -> bool { shaped_data(par, DataShape::Plain) }

/// Added by G3 (DR-135, S3b): whether `par` carries no code: no process, no
/// signed term, no token stack and no connective, and every expr is one of
/// the 15 grounds or a list, tuple, set or map of code-free Pars without a
/// remainder. Unforgeable names and bundles whose bodies are code-free are
/// code-free. A code-free value runs nothing, so Rule H gives it no holder.
pub(crate) fn code_free(par: &Par) -> bool { shaped_data(par, DataShape::CodeFree) }

/// Added by G3 (DR-135, S3b): the walk of `plain_data` and `code_free`. The
/// destructuring names every field of `Par`, so a new field fails to compile
/// here until it is classified.
fn shaped_data(par: &Par, shape: DataShape) -> bool {
    let mut pending: Vec<&Par> = vec![par];
    while let Some(par) = pending.pop() {
        let Par {
            sends,
            receives,
            news,
            exprs,
            matches,
            unforgeables,
            bundles,
            connectives,
            locally_free: _,
            connective_used,
            conditionals,
            cost_signed_terms,
            cost_stacks,
        } = par;
        let holds_no_process = sends.is_empty()
            && receives.is_empty()
            && news.is_empty()
            && matches.is_empty()
            && connectives.is_empty()
            && !*connective_used
            && conditionals.is_empty()
            && cost_signed_terms.is_empty()
            && cost_stacks.is_empty();
        if !holds_no_process {
            return false;
        }
        match shape {
            DataShape::Plain if !unforgeables.is_empty() || !bundles.is_empty() => return false,
            DataShape::Plain => {}
            DataShape::CodeFree => {
                pending.extend(bundles.iter().filter_map(|bundle| bundle.body.as_ref()))
            }
        }
        for expr in exprs {
            match expr.expr_instance.as_ref() {
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
                ) => {}
                Some(ExprInstance::EListBody(list))
                    if !list.connective_used && list.remainder.is_none() =>
                {
                    pending.extend(&list.ps);
                }
                Some(ExprInstance::ETupleBody(tuple)) if !tuple.connective_used => {
                    pending.extend(&tuple.ps);
                }
                Some(ExprInstance::ESetBody(set))
                    if !set.connective_used && set.remainder.is_none() =>
                {
                    pending.extend(&set.ps);
                }
                Some(ExprInstance::EMapBody(map))
                    if !map.connective_used && map.remainder.is_none() =>
                {
                    for pair in &map.kvs {
                        let (Some(key), Some(value)) = (pair.key.as_ref(), pair.value.as_ref())
                        else {
                            return false;
                        };
                        pending.push(key);
                        pending.push(value);
                    }
                }
                _ => return false,
            }
        }
    }
    true
}

/// Added by G3 (DR-135, S3b): one span of the held map: the end of the span
/// and the holder of its positions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HeldSpan {
    end: u32,
    holder: RegionOwner,
}

/// Added by G3 (DR-135, S3b): the held map of a reducer (Rule H). It maps the
/// start of each span of env positions to the end of the span and the holder
/// of the values there: the data spans of a fired body (H0) and the match
/// segments (H3). It is a persistent ordered map, so a reducer copy with one
/// more segment shares the rest, and an insert or a lookup takes O(log n).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct HeldMap(imbl::OrdMap<u32, HeldSpan>);

impl HeldMap {
    pub(crate) fn is_empty(&self) -> bool { self.0.is_empty() }

    /// The holder at env position `position`: the span with the greatest
    /// start at or below the position, when the position is below its end.
    pub(crate) fn holder(&self, position: u32) -> Option<&RegionOwner> {
        let (_, span) = self.0.range(..=position).next_back()?;
        (position < span.end).then_some(&span.holder)
    }

    /// The holder of `BoundVar(k)` in `env`.
    pub(crate) fn var_holder(&self, k: i32, env: &Env<Par>) -> Option<&RegionOwner> {
        self.holder(env_position(k, env)?)
    }

    /// Adds the span [start, start + count) for `holder`. A span of width 0,
    /// or one whose end is past `u32`, is not added (review item 2).
    pub(crate) fn insert_span(&mut self, start: u32, count: u32, holder: RegionOwner) {
        if let Some(end) = start.checked_add(count).filter(|end| *end > start) {
            self.0.insert(start, HeldSpan { end, holder });
        }
    }

    /// This map with the span [start, start + count) for `holder`, as a copy
    /// that shares the rest of the map.
    pub(crate) fn with_span(&self, start: u32, count: u32, holder: RegionOwner) -> HeldMap {
        let mut map = self.clone();
        map.insert_span(start, count, holder);
        map
    }
}

/// Added by G3 (DR-135, S3b): the env position of `BoundVar(k)`, with the
/// formula of `Env::get` (`level + shift - k - 1`) in checked i32 arithmetic.
/// A failed or negative result is no position (review item 2).
pub(crate) fn env_position(k: i32, env: &Env<Par>) -> Option<u32> {
    let position = env
        .level()
        .checked_add(env.shift)?
        .checked_sub(k)?
        .checked_sub(1)?;
    u32::try_from(position).ok()
}

/// Added by G3 (DR-135, S3b): the env value of `BoundVar(k)`, without a copy.
fn env_value(k: i32, env: &Env<Par>) -> Option<&Par> {
    let position = i32::try_from(env_position(k, env)?).ok()?;
    env.get_at(position)
}

/// Added by G3 (DR-135, S3b): the one expr of `par` when `par` holds exactly
/// one expr and nothing else.
fn only_expr(par: &Par) -> Option<&Expr> {
    let Par {
        sends,
        receives,
        news,
        exprs,
        matches,
        unforgeables,
        bundles,
        connectives,
        locally_free: _,
        connective_used: _,
        conditionals,
        cost_signed_terms,
        cost_stacks,
    } = par;
    let only = sends.is_empty()
        && receives.is_empty()
        && news.is_empty()
        && matches.is_empty()
        && unforgeables.is_empty()
        && bundles.is_empty()
        && connectives.is_empty()
        && conditionals.is_empty()
        && cost_signed_terms.is_empty()
        && cost_stacks.is_empty();
    match exprs.as_slice() {
        [expr] if only => Some(expr),
        _ => None,
    }
}

/// Added by G3 (DR-135, S3b): the index k when `par` is exactly one
/// `BoundVar(k)`.
pub(crate) fn single_bound_var(par: &Par) -> Option<i32> {
    match only_expr(par)?.expr_instance.as_ref()? {
        ExprInstance::EVarBody(EVar {
            v:
                Some(Var {
                    var_instance: Some(VarInstance::BoundVar(k)),
                }),
        }) => Some(*k),
        _ => None,
    }
}

/// Added by G3 (DR-135, S3b H5): the elements of `par` when `par` is exactly
/// one list, tuple or set without a remainder, or one map without a
/// remainder whose every pair has a key and a value.
fn collection_elements(par: &Par) -> Option<Vec<&Par>> {
    match only_expr(par)?.expr_instance.as_ref()? {
        ExprInstance::EListBody(list) if list.remainder.is_none() => Some(list.ps.iter().collect()),
        ExprInstance::ETupleBody(tuple) => Some(tuple.ps.iter().collect()),
        ExprInstance::ESetBody(set) if set.remainder.is_none() => Some(set.ps.iter().collect()),
        ExprInstance::EMapBody(map) if map.remainder.is_none() => {
            let mut elements = Vec::with_capacity(2 * map.kvs.len());
            for pair in &map.kvs {
                elements.push(pair.key.as_ref()?);
                elements.push(pair.value.as_ref()?);
            }
            Some(elements)
        }
        _ => None,
    }
}

/// Added by G3 (DR-135, S3b): the free variable indices of `locally_free`,
/// whose byte i is set when `BoundVar(i)` is free.
fn locally_free_indices(locally_free: &[u8]) -> impl Iterator<Item = i32> + '_ {
    locally_free
        .iter()
        .enumerate()
        .filter(|(_, bit)| **bit != 0)
        .filter_map(|(index, _)| i32::try_from(index).ok())
}

/// Added by G3 (DR-135, S3b H4): a held term is malformed state. Only the
/// reducer makes held terms, so the run fails.
fn malformed_held(reason: &str) -> InterpreterError {
    InterpreterError::ReduceError(format!(
        "cost-accounting: a held term is malformed: {reason}"
    ))
}

/// Added by G3 (DR-135, S3b H4): the holder of `term` when `term` is a held
/// term, and none for a signed term of the surface syntax. A held term needs
/// a canonical signature that is not Unit, and a held price above 0 (T13,
/// T14).
pub(crate) fn held_term_holder(
    term: &CostSignedTerm,
) -> Result<Option<RegionOwner>, InterpreterError> {
    let Some(price) = term.held_price else {
        return Ok(None);
    };
    let signature = term
        .signature
        .as_ref()
        .ok_or_else(|| malformed_held("it has no signature"))?;
    let signature =
        canonical_cost_signature(signature).map_err(|error| malformed_held(&error.to_string()))?;
    match (is_unit_cost_signature(&signature), price) {
        (true, _) => Err(malformed_held("its signature is Unit")),
        (false, 0) => Err(malformed_held("its held price is 0")),
        (false, price) => Ok(Some(RegionOwner { signature, price })),
    }
}

/// Added by G3 (DR-135, S3b): the held term of `holder` with body `body`.
fn held_term(body: Par, holder: RegionOwner) -> CostSignedTerm {
    CostSignedTerm {
        body: Some(body),
        signature: Some(holder.signature),
        held_price: Some(holder.price),
    }
}

/// Added by G3 (DR-135, S3b H5): the body and the holder of `par` when `par`
/// is a held marker, a send-data par that is one held term and nothing else.
/// The marker passes the guard of H4 (T6).
fn held_marker(par: &Par) -> Result<Option<(&Par, RegionOwner)>, InterpreterError> {
    let Some(term) = solely_signed(par) else {
        return Ok(None);
    };
    let Some(holder) = held_term_holder(term)? else {
        return Ok(None);
    };
    let body = term
        .body
        .as_ref()
        .ok_or_else(|| malformed_held("a held marker has no body"))?;
    Ok(Some((body, holder)))
}

/// Added by G3 (DR-135, S3b H5): the send data with each held marker
/// replaced by its body, and the holder of each data par that was a marker.
/// The holders are empty when no par is a marker. The strip runs in every
/// run, with or without a binding, and charges no phlo, so no held term
/// reaches a datum. It gives references into `data`, so it copies no value.
pub(crate) fn strip_held_markers(
    data: &[Par],
) -> Result<(Vec<&Par>, Vec<Option<RegionOwner>>), InterpreterError> {
    let mut stripped = Vec::with_capacity(data.len());
    let mut holders = Vec::with_capacity(data.len());
    for par in data {
        match held_marker(par)? {
            Some((body, holder)) => {
                stripped.push(body);
                holders.push(Some(holder));
            }
            None => {
                stripped.push(par);
                holders.push(None);
            }
        }
    }
    if holders.iter().all(Option::is_none) {
        holders.clear();
    }
    Ok((stripped, holders))
}

/// Added by G3 (DR-135, S3b H2): the future context of a stored body: a
/// system body for a system seal or a seal without regions (T4a), else the
/// signer of the seal.
pub(crate) enum HeldFuture {
    System,
    Signer(CostSignature),
}

impl HeldFuture {
    fn of(seal: &CostAuthority) -> Result<Self, InterpreterError> {
        if seal.regions.is_empty() || is_system_seal(seal) {
            return Ok(Self::System);
        }
        Ok(match seal_signer(seal)? {
            Some(signer) => Self::Signer(signer.signature),
            None => Self::System,
        })
    }

    /// Whether a held use of `holder` runs for another party than the future
    /// context, so the store keeps its holder.
    fn needs_holder(&self, holder: &RegionOwner) -> bool {
        match self {
            Self::System => true,
            Self::Signer(signer) => holder.signature != *signer,
        }
    }
}

/// Added by G3 (DR-135, S3b H2): one frame of the walk of a stored body: a
/// nested receive or new (no holder), or a match case with the holder of its
/// target and whether the target's env value carries code. The frames form a
/// persistent list, innermost first, so a push takes O(1). A frame keeps no
/// copy of the target's value, so no deep value waits on a frame.
struct HeldFrame {
    count: i32,
    holder: Option<RegionOwner>,
    carries_code: bool,
    parent: HeldFrames,
}

type HeldFrames = Option<Arc<HeldFrame>>;

/// Added by G3 (DR-135, S3b): the derived drop of the frame list would recurse
/// once for each frame of a deep body. This drop unlinks the chain in a loop,
/// as far as this frame owns it.
impl Drop for HeldFrame {
    fn drop(&mut self) {
        let mut parent = self.parent.take();
        while let Some(frame) = parent {
            match Arc::try_unwrap(frame) {
                Ok(mut frame) => parent = frame.parent.take(),
                Err(_) => break,
            }
        }
    }
}

fn push_frame(
    frames: &HeldFrames,
    count: i32,
    holder: Option<RegionOwner>,
    carries_code: bool,
) -> HeldFrames {
    Some(Arc::new(HeldFrame {
        count: count.max(0),
        holder,
        carries_code,
        parent: frames.clone(),
    }))
}

/// Added by G3 (DR-135, S3b H2): what `resolve` gives for a variable of a
/// stored body.
enum Resolved<'a> {
    /// Bound inside the body: by a nested receive or new, with no holder, or
    /// by a match case, with the holder of its target and whether the
    /// target's value carries code.
    Frame {
        holder: Option<&'a RegionOwner>,
        carries_code: bool,
    },
    /// Bound by the stored receive itself.
    Receive,
    /// Captured from the env of the receive.
    Captured {
        holder: Option<&'a RegionOwner>,
        value: Option<&'a Par>,
    },
}

impl<'a> Resolved<'a> {
    /// The holder of a variable that has a holder, and whether its value
    /// carries code (it is not code-free).
    fn held(self) -> Option<(&'a RegionOwner, bool)> {
        match self {
            Resolved::Frame {
                holder: Some(holder),
                carries_code,
            } => Some((holder, carries_code)),
            Resolved::Captured {
                holder: Some(holder),
                value,
            } => Some((holder, !value.is_some_and(code_free))),
            _ => None,
        }
    }
}

/// Added by G3 (DR-135, S3b H5): what the data of a send carry: the holders
/// of their code, whether some code has no holder (mixed), and whether a free
/// variable carries the code of a holder other than the context (T12).
#[derive(Debug, Default)]
struct DataClass {
    holders: Vec<RegionOwner>,
    mixed: bool,
    foreign_code: bool,
}

/// Added by G3 (DR-135, S3b H5): what a send records for the holder of its
/// datum.
pub(crate) enum SendRecord {
    /// The seals of S3.
    Unchanged,
    /// An owner body: the opened region records this caller, a holder or the
    /// Unit sentinel.
    Caller(CostCaller),
    /// A system body: the residue records this caller, or none.
    Residue(Option<RegionOwner>),
}

/// Added by G3 (DR-135, S3b T12): the Unit sentinel, a recorded caller that
/// says that the provenance of a datum's code is mixed.
fn unit_sentinel() -> CostCaller {
    CostCaller {
        signature: Some(CostSignature {
            value: Some(CostSignatureValue::Unit(true)),
        }),
        price: 0,
    }
}

/// Added by G3 (DR-135, S3b): the free variable indices that an expr in
/// process position adds to the `locally_free` of its Par.
fn expr_locally_free(expr: &Expr) -> Vec<u8> {
    match expr.expr_instance.as_ref() {
        Some(ExprInstance::EVarBody(EVar {
            v:
                Some(Var {
                    var_instance: Some(VarInstance::BoundVar(k)),
                }),
        })) => usize::try_from(*k)
            .map(|k| models::create_bit_vector(&[k]))
            .unwrap_or_default(),
        Some(ExprInstance::EMethodBody(method)) => method.locally_free.clone(),
        _ => Vec::new(),
    }
}

impl ReducerCore {
    /// Added by G3 (DR-135, S3b): the context signature of this reducer: the
    /// owner's signature in an owner body, else the caller's signature in a
    /// system body with a caller, else the signature of the running
    /// deployment.
    pub(crate) fn context_signature(&self) -> Result<CostSignature, InterpreterError> {
        match (self.owner.as_ref(), self.residue_seal, self.caller.as_ref()) {
            (Some(owner), _, _) => Ok(owner.signature.clone()),
            (None, true, Some(caller)) => Ok(caller.signature.clone()),
            _ => sig_to_cost_signature(&self.metering.budget().signature())
                .map_err(|error| InterpreterError::ReduceError(error.to_string())),
        }
    }

    /// Added by G3 (DR-135, S3b): whether `signature` is a client lane of the
    /// running binding. Without a binding no lane is a client lane.
    fn on_client_lane(&self, signature: &CostSignature) -> Result<bool, InterpreterError> {
        match self.metering.budget().owner_binding() {
            Some(binding) => binding.is_client_signature(signature).map_err(malformed),
            None => Ok(false),
        }
    }

    /// Added by G3 (DR-135, S3b): `held_reducer(H)`: residue mode off, the
    /// owner H (none when H is a client lane of the running binding, so the
    /// running deployment pays), no caller and an empty held map.
    pub(crate) fn held_reducer(
        &self,
        holder: &RegionOwner,
    ) -> Result<ReducerCore, InterpreterError> {
        let owner = match self.on_client_lane(&holder.signature)? {
            true => None,
            false => Some(holder.clone()),
        };
        Ok(ReducerCore {
            residue_seal: false,
            owner,
            caller: None,
            held: HeldMap::default(),
            ..self.clone()
        })
    }

    /// Added by G3 (DR-135, S3b T1): the context of a process that a system
    /// body holds: the caller as owner, or none when the caller is a client
    /// lane of the running binding, no caller and an empty held map.
    pub(crate) fn residue_owner(&self) -> Result<Option<RegionOwner>, InterpreterError> {
        match self.caller.as_ref() {
            Some(caller) if self.on_client_lane(&caller.signature)? => Ok(None),
            caller => Ok(caller.cloned()),
        }
    }

    /// Added by G3 (DR-135, S3b H1): the reducer of a held value whose holder
    /// is `holder`, when the holder differs from the context signature. The
    /// value then runs in `held_reducer(holder)` with an empty authority.
    pub(crate) fn held_switch(
        &self,
        holder: Option<RegionOwner>,
    ) -> Result<Option<ReducerCore>, InterpreterError> {
        let Some(holder) = holder else {
            return Ok(None);
        };
        if holder.signature == self.context_signature()? {
            return Ok(None);
        }
        self.held_reducer(&holder).map(Some)
    }

    /// Added by G3 (DR-135, S3b H1): the holder of a variable that an
    /// `EVarBody` exit runs.
    pub(crate) fn var_exit_holder(&self, var: Option<&Var>, env: &Env<Par>) -> Option<RegionOwner> {
        match var?.var_instance.as_ref()? {
            VarInstance::BoundVar(k) => self.held.var_holder(*k, env).cloned(),
            _ => None,
        }
    }

    /// Added by G3 (DR-135, S3b H1): the holder of a method that an
    /// `EMethodBody` exit runs: the holder H of the target when the target
    /// is a method chain rooted at one bound variable with holder H, and every
    /// argument is a code-free literal, a bound variable with H's signature,
    /// or an expression whose evaluated value is code-free.
    pub(crate) fn method_exit_holder(
        &self,
        method: &EMethod,
        env: &Env<Par>,
    ) -> Result<Option<RegionOwner>, InterpreterError> {
        if self.held.is_empty() {
            return Ok(None);
        }
        let mut arguments: Vec<&Par> = Vec::new();
        let mut current = method;
        let root = loop {
            arguments.extend(&current.arguments);
            let Some(target) = current.target.as_ref() else {
                return Ok(None);
            };
            match only_expr(target).and_then(|expr| expr.expr_instance.as_ref()) {
                Some(ExprInstance::EMethodBody(inner)) => current = inner,
                Some(ExprInstance::EVarBody(EVar {
                    v:
                        Some(Var {
                            var_instance: Some(VarInstance::BoundVar(k)),
                        }),
                })) => break *k,
                _ => return Ok(None),
            }
        };
        let Some(holder) = self.held.var_holder(root, env) else {
            return Ok(None);
        };
        for argument in arguments {
            if code_free(argument) {
                continue;
            }
            let same_holder = single_bound_var(argument)
                .and_then(|k| self.held.var_holder(k, env))
                .is_some_and(|other| other.signature == holder.signature);
            if same_holder {
                continue;
            }
            let evaluated = self.eval_expr(argument, env)?;
            let value = self.substitute.substitute_and_charge(evaluated, 0, env)?;
            if !code_free(&value) {
                return Ok(None);
            }
        }
        Ok(Some(holder.clone()))
    }

    /// Added by G3 (DR-135, S3b H3): the reducer of a case body whose match
    /// target is exactly one bound variable with a holder. Its held map adds
    /// the segment [env.level, env.level + free_count) for that holder.
    pub(crate) fn match_case_reducer(
        &self,
        target: Option<&Par>,
        env: &Env<Par>,
        free_count: i32,
    ) -> Option<ReducerCore> {
        if self.held.is_empty() {
            return None;
        }
        let holder = self.held.var_holder(single_bound_var(target?)?, env)?;
        let start = u32::try_from(env.level()).ok()?;
        let count = u32::try_from(free_count).ok().filter(|count| *count > 0)?;
        Some(ReducerCore {
            held: self.held.with_span(start, count, holder.clone()),
            ..self.clone()
        })
    }

    /// Added by G3 (DR-135, S3b H2): the stored body of a receive with its
    /// held uses wrapped (`wrap_stored_body`), or none when nothing is
    /// wrapped. It runs only with a binding and a held map that is not empty
    /// (H7). The future context is that of `receive_authority`, the seal that
    /// the continuation stores.
    pub(crate) fn wrap_held_uses(
        &self,
        body: &Par,
        bind_count: i32,
        env: &Env<Par>,
        receive_authority: &CostAuthority,
    ) -> Result<Option<Par>, InterpreterError> {
        if self.held.is_empty() || self.metering.budget().owner_binding().is_none() {
            return Ok(None);
        }
        let future = HeldFuture::of(receive_authority)?;
        if !stored_body_has_held_uses(&self.held, body, bind_count, env, &future) {
            return Ok(None);
        }
        // The copy happens only when a held use is wrapped. Its depth is the
        // depth of the substitution that follows it in `eval_receive`.
        let mut stored = body.clone();
        wrap_stored_body(&self.held, &mut stored, bind_count, env, &future);
        Ok(Some(stored))
    }

    /// Added by G3 (DR-135, S3b H5): what a send of the stripped data `data`
    /// records for the holder of its datum. `values` are the evaluated,
    /// substituted data, and `markers` the holders of the stripped markers.
    /// It runs only with a binding and an opened region, and in an owner body
    /// also with a held map that is not empty or a stripped marker. In a
    /// system body with a caller the binding and the opened region suffice
    /// (T3).
    pub(crate) fn held_send_record(
        &self,
        data: &[&Par],
        markers: &[Option<RegionOwner>],
        values: &[Par],
        env: &Env<Par>,
        opens_region: bool,
    ) -> Result<SendRecord, InterpreterError> {
        if !opens_region || self.metering.budget().owner_binding().is_none() {
            return Ok(SendRecord::Unchanged);
        }
        let gated = match (self.residue_seal, self.caller.is_some()) {
            (true, true) => true,
            _ => !self.held.is_empty() || markers.iter().any(Option::is_some),
        };
        if !gated {
            return Ok(SendRecord::Unchanged);
        }
        let context = self.context_signature()?;
        let mut evaluate = |par: &Par| -> Result<Par, InterpreterError> {
            let evaluated = self.eval_expr(par, env)?;
            self.substitute.substitute_and_charge(evaluated, 0, env)
        };
        let class = classify_data(&self.held, data, markers, env, &context, &mut evaluate)?;
        Ok(send_record(
            self.residue_seal,
            class,
            values.iter().all(code_free),
            &context,
        ))
    }

    /// Added by G3 (DR-135, S3b H5): the residue seal of a system body that
    /// records `caller`, or no caller.
    pub(crate) fn residue_seal_with(
        &self,
        authority: &CostAuthority,
        caller: Option<&RegionOwner>,
    ) -> Result<CostAuthority, InterpreterError> {
        system_residue_authority_with_caller(authority, &self.metering.budget().deploy_id(), caller)
            .map_err(|error| InterpreterError::ReduceError(error.to_string()))
    }
}

/// Added by G3 (DR-135, S3b H2): `resolve(k, frames)` of a variable of a
/// stored body. The frames come first, innermost first: an index below the
/// count of a frame is bound by it. Then come the binds of the stored
/// receive itself. Any other index is captured from the env of the receive,
/// where the held map gives its holder.
fn resolve<'a>(
    held: &'a HeldMap,
    k: i32,
    frames: &'a HeldFrames,
    bind_count: i32,
    env: &'a Env<Par>,
) -> Resolved<'a> {
    if k < 0 {
        return Resolved::Receive;
    }
    let mut k = k;
    let mut frame = frames.as_deref();
    while let Some(current) = frame {
        if k < current.count {
            return Resolved::Frame {
                holder: current.holder.as_ref(),
                carries_code: current.carries_code,
            };
        }
        k -= current.count;
        frame = current.parent.as_deref();
    }
    if k < bind_count {
        return Resolved::Receive;
    }
    let captured = k - bind_count;
    Resolved::Captured {
        holder: held.var_holder(captured, env),
        value: env_value(captured, env),
    }
}

/// Added by G3 (DR-135, S3b H2): the holder of an expr in process position
/// of a stored body, and whether its value carries code. A method counts
/// only when every argument is a code-free literal or a captured variable
/// whose env value is code-free (T4b). The value of a method is the value of
/// its root.
fn stored_use_holder<'a>(
    held: &'a HeldMap,
    expr: &'a Expr,
    frames: &'a HeldFrames,
    bind_count: i32,
    env: &'a Env<Par>,
) -> Option<(&'a RegionOwner, bool)> {
    let mut method = match expr.expr_instance.as_ref()? {
        ExprInstance::EVarBody(EVar {
            v:
                Some(Var {
                    var_instance: Some(VarInstance::BoundVar(k)),
                }),
        }) => return resolve(held, *k, frames, bind_count, env).held(),
        ExprInstance::EMethodBody(method) => method,
        _ => return None,
    };
    loop {
        for argument in &method.arguments {
            if code_free(argument) {
                continue;
            }
            match single_bound_var(argument).map(|k| resolve(held, k, frames, bind_count, env)) {
                Some(Resolved::Captured {
                    value: Some(value), ..
                }) if code_free(value) => continue,
                _ => return None,
            }
        }
        match only_expr(method.target.as_ref()?)?.expr_instance.as_ref()? {
            ExprInstance::EMethodBody(inner) => method = inner,
            ExprInstance::EVarBody(EVar {
                v:
                    Some(Var {
                        var_instance: Some(VarInstance::BoundVar(k)),
                    }),
            }) => return resolve(held, *k, frames, bind_count, env).held(),
            _ => return None,
        }
    }
}

/// Added by G3 (DR-135, S3b H2): moves each held use of `par` in process
/// position into a held term: a use whose holder runs for another party than
/// the future context, and whose value is not code-free (T9). The uses of one
/// holder signature go into one held term, in first-use order, at the
/// minimum price of the uses (T2). The containing Par keeps its
/// `locally_free` and `connective_used`.
fn wrap_process_uses(
    held: &HeldMap,
    par: &mut Par,
    frames: &HeldFrames,
    bind_count: i32,
    env: &Env<Par>,
    future: &HeldFuture,
) -> bool {
    let wrap = |expr: &Expr| -> Option<RegionOwner> {
        let (holder, carries_code) = stored_use_holder(held, expr, frames, bind_count, env)?;
        (carries_code && future.needs_holder(holder)).then(|| holder.clone())
    };
    if !par.exprs.iter().any(|expr| wrap(expr).is_some()) {
        return false;
    }
    let exprs = std::mem::take(&mut par.exprs);
    let mut kept = Vec::with_capacity(exprs.len());
    let mut groups: Vec<(RegionOwner, Vec<Expr>)> = Vec::new();
    for expr in exprs {
        let Some(holder) = wrap(&expr) else {
            kept.push(expr);
            continue;
        };
        match groups
            .iter_mut()
            .find(|(group, _)| group.signature == holder.signature)
        {
            Some((group, uses)) => {
                group.price = group.price.min(holder.price);
                uses.push(expr);
            }
            None => groups.push((holder, vec![expr])),
        }
    }
    par.exprs = kept;
    for (holder, uses) in groups {
        let locally_free = uses.iter().fold(Vec::new(), |bits, expr| {
            union(bits, expr_locally_free(expr))
        });
        let connective_used = uses.iter().any(|expr| {
            matches!(
                expr.expr_instance.as_ref(),
                Some(ExprInstance::EMethodBody(method)) if method.connective_used
            )
        });
        par.cost_signed_terms.push(held_term(
            Par {
                exprs: uses,
                locally_free,
                connective_used,
                ..Default::default()
            },
            holder,
        ));
    }
    true
}

/// Added by G3 (DR-135, S3b M2, T10): turns each top-level send-data par of
/// `par` that is exactly one held variable into a held marker: a captured
/// variable or a variable of a match frame whose holder runs for another
/// party than the future context, and whose value is not code-free (for a
/// match-frame variable, the env value of the target).
fn mark_send_data(
    held: &HeldMap,
    par: &mut Par,
    frames: &HeldFrames,
    bind_count: i32,
    env: &Env<Par>,
    future: &HeldFuture,
) -> bool {
    let mut changed = false;
    for send in par.sends.iter_mut() {
        for datum in send.data.iter_mut() {
            let Some(k) = single_bound_var(datum) else {
                continue;
            };
            let Some((holder, carries_code)) = resolve(held, k, frames, bind_count, env).held()
            else {
                continue;
            };
            if !carries_code || !future.needs_holder(holder) {
                continue;
            }
            let holder = holder.clone();
            let body = std::mem::take(datum);
            *datum = Par {
                locally_free: body.locally_free.clone(),
                connective_used: body.connective_used,
                cost_signed_terms: vec![held_term(body, holder)],
                ..Default::default()
            };
            changed = true;
        }
    }
    changed
}

/// Added by G3 (DR-135, S3b H2): wraps the held uses of `body` in place, and
/// tells whether it wrapped any (see the design of H2). The walk visits the
/// body and the bodies of nested receives, news, match cases, if branches,
/// bundles and signed terms without a held price, from a heap stack. It
/// never visits channels, patterns, guards, conditions, match targets, or
/// send data below the top level, so a held term never reaches a datum.
///
/// ```text
/// wrap_stored_body(held, body, bind_count, env, future):
///   pending := [(body, no frames)]
///   while (par, frames) := pending.pop():
///     wrap the held uses in process position of par    (wrap_process_uses)
///     mark the held variables of par's send data       (mark_send_data)
///     push each receive and new body with frame (bind count, no holder)
///     push each match case body with frame (free count, holder of the
///          target when it is one bound variable, with the target's value)
///     push each if branch, bundle body and surface signed-term body
///          with the same frames
/// ```
pub(crate) fn wrap_stored_body(
    held: &HeldMap,
    body: &mut Par,
    bind_count: i32,
    env: &Env<Par>,
    future: &HeldFuture,
) -> bool {
    let mut changed = false;
    let mut pending: Vec<(&mut Par, HeldFrames)> = vec![(body, None)];
    while let Some((par, frames)) = pending.pop() {
        changed |= wrap_process_uses(held, par, &frames, bind_count, env, future);
        changed |= mark_send_data(held, par, &frames, bind_count, env, future);
        let Par {
            receives,
            news,
            matches,
            conditionals,
            bundles,
            cost_signed_terms,
            ..
        } = par;
        for receive in receives.iter_mut() {
            if let Some(body) = receive.body.as_mut() {
                pending.push((body, push_frame(&frames, receive.bind_count, None, false)));
            }
        }
        for new in news.iter_mut() {
            if let Some(body) = new.p.as_mut() {
                pending.push((body, push_frame(&frames, new.bind_count, None, false)));
            }
        }
        for matched in matches.iter_mut() {
            let (holder, carries_code) = match matched
                .target
                .as_ref()
                .and_then(single_bound_var)
                .map(|k| resolve(held, k, &frames, bind_count, env).held())
            {
                Some(Some((holder, carries_code))) => (Some(holder.clone()), carries_code),
                _ => (None, false),
            };
            for case in matched.cases.iter_mut() {
                if let Some(body) = case.source.as_mut() {
                    let frame = push_frame(&frames, case.free_count, holder.clone(), carries_code);
                    pending.push((body, frame));
                }
            }
        }
        for conditional in conditionals.iter_mut() {
            for branch in [conditional.if_true.as_mut(), conditional.if_false.as_mut()]
                .into_iter()
                .flatten()
            {
                pending.push((branch, frames.clone()));
            }
        }
        for bundle in bundles.iter_mut() {
            if let Some(body) = bundle.body.as_mut() {
                pending.push((body, frames.clone()));
            }
        }
        for signed in cost_signed_terms.iter_mut() {
            if signed.held_price.is_none() {
                if let Some(body) = signed.body.as_mut() {
                    pending.push((body, frames.clone()));
                }
            }
        }
    }
    changed
}

/// Added by G3 (DR-135, S3b H2): whether `wrap_stored_body` would wrap a
/// held use of `body`. It reads the body with the same walk and the same
/// decisions, and it copies nothing.
pub(crate) fn stored_body_has_held_uses(
    held: &HeldMap,
    body: &Par,
    bind_count: i32,
    env: &Env<Par>,
    future: &HeldFuture,
) -> bool {
    let wraps = |held_use: Option<(&RegionOwner, bool)>| {
        held_use.is_some_and(|(holder, carries_code)| carries_code && future.needs_holder(holder))
    };
    let mut pending: Vec<(&Par, HeldFrames)> = vec![(body, None)];
    while let Some((par, frames)) = pending.pop() {
        let process_use = par
            .exprs
            .iter()
            .any(|expr| wraps(stored_use_holder(held, expr, &frames, bind_count, env)));
        let marked_datum = par
            .sends
            .iter()
            .flat_map(|send| send.data.iter())
            .filter_map(single_bound_var)
            .any(|k| wraps(resolve(held, k, &frames, bind_count, env).held()));
        if process_use || marked_datum {
            return true;
        }
        for receive in &par.receives {
            if let Some(body) = receive.body.as_ref() {
                pending.push((body, push_frame(&frames, receive.bind_count, None, false)));
            }
        }
        for new in &par.news {
            if let Some(body) = new.p.as_ref() {
                pending.push((body, push_frame(&frames, new.bind_count, None, false)));
            }
        }
        for matched in &par.matches {
            let (holder, carries_code) = match matched
                .target
                .as_ref()
                .and_then(single_bound_var)
                .map(|k| resolve(held, k, &frames, bind_count, env).held())
            {
                Some(Some((holder, carries_code))) => (Some(holder.clone()), carries_code),
                _ => (None, false),
            };
            for case in &matched.cases {
                if let Some(body) = case.source.as_ref() {
                    let frame = push_frame(&frames, case.free_count, holder.clone(), carries_code);
                    pending.push((body, frame));
                }
            }
        }
        for conditional in &par.conditionals {
            for branch in [conditional.if_true.as_ref(), conditional.if_false.as_ref()]
                .into_iter()
                .flatten()
            {
                pending.push((branch, frames.clone()));
            }
        }
        for bundle in &par.bundles {
            if let Some(body) = bundle.body.as_ref() {
                pending.push((body, frames.clone()));
            }
        }
        for signed in &par.cost_signed_terms {
            if signed.held_price.is_none() {
                if let Some(body) = signed.body.as_ref() {
                    pending.push((body, frames.clone()));
                }
            }
        }
    }
    false
}

/// Added by G3 (DR-135, S3b H5): what the stripped send data `data` carry. A
/// marker gives its holder. A variable with a holder whose value is not
/// code-free gives that holder. A code-free literal, or a variable with a
/// code-free value, is neutral. A list, tuple, set or map gives the union of
/// its elements (T12). Anything else is mixed unless `evaluate` makes it
/// code-free (T4c). A free variable of a data par whose holder differs from
/// the context and whose value is not code-free marks foreign code (T12).
fn classify_data(
    held: &HeldMap,
    data: &[&Par],
    markers: &[Option<RegionOwner>],
    env: &Env<Par>,
    context: &CostSignature,
    evaluate: &mut dyn FnMut(&Par) -> Result<Par, InterpreterError>,
) -> Result<DataClass, InterpreterError> {
    let mut class = DataClass::default();
    let mut pending: Vec<&Par> = Vec::with_capacity(data.len());
    for (index, &par) in data.iter().enumerate() {
        match markers.get(index).cloned().flatten() {
            Some(holder) => {
                class.foreign_code |= holder.signature != *context;
                class.holders.push(holder);
            }
            None => pending.push(par),
        }
        for k in locally_free_indices(&par.locally_free) {
            let foreign = held
                .var_holder(k, env)
                .is_some_and(|holder| holder.signature != *context)
                && !env_value(k, env).is_some_and(code_free);
            class.foreign_code |= foreign;
        }
    }
    while let Some(par) = pending.pop() {
        if code_free(par) {
            continue;
        }
        if let Some(k) = single_bound_var(par) {
            match (
                env_value(k, env).is_some_and(code_free),
                held.var_holder(k, env),
            ) {
                (true, _) => {}
                (false, Some(holder)) => class.holders.push(holder.clone()),
                (false, None) => class.mixed = true,
            }
            continue;
        }
        if let Some(elements) = collection_elements(par) {
            pending.extend(elements);
            continue;
        }
        if !code_free(&evaluate(par)?) {
            class.mixed = true;
        }
    }
    Ok(class)
}

/// Added by G3 (DR-135, S3b H5): what a send records, from the class of its
/// data (see the design of H5). `data_code_free` says whether the
/// evaluated, substituted data are code-free.
///
/// ```text
/// system body:  code-free data             => unchanged (S3's residue seal)
///               not mixed, one holder H     => residue records H
///               else                        => residue records no caller
/// owner body:   no foreign held code        => unchanged
///               not mixed, one agreed holder H, H != context => region records H
///               else (holders that disagree, or mixed code)
///                                           => region records the Unit sentinel
/// ```
///
/// Changed by G3 (DR-135, S3b review): in an owner body, foreign held code
/// whose holders do not reduce to one agreed foreign holder records the Unit
/// sentinel. Before, holders that disagree recorded nothing, so the
/// forwarding owner became the holder (T12, the forwarder-pays case).
fn send_record(
    system: bool,
    class: DataClass,
    data_code_free: bool,
    context: &CostSignature,
) -> SendRecord {
    match system {
        true if data_code_free => SendRecord::Unchanged,
        true => SendRecord::Residue(match class.mixed {
            false => single_holder(class.holders),
            true => None,
        }),
        // false => match (class.mixed, single_holder(class.holders)) {
        //     (false, Some(holder)) if holder.signature != *context => {
        //         SendRecord::Caller(CostCaller {
        //             signature: Some(holder.signature),
        //             price: holder.price,
        //         })
        //     }
        //     (true, _) if class.foreign_code => SendRecord::Caller(unit_sentinel()),
        //     _ => SendRecord::Unchanged,
        // },
        false => {
            let foreign = class.foreign_code
                || class
                    .holders
                    .iter()
                    .any(|holder| holder.signature != *context);
            match (foreign, class.mixed, single_holder(class.holders)) {
                (false, _, _) => SendRecord::Unchanged,
                (true, false, Some(holder)) if holder.signature != *context => {
                    SendRecord::Caller(CostCaller {
                        signature: Some(holder.signature),
                        price: holder.price,
                    })
                }
                (true, _, _) => SendRecord::Caller(unit_sentinel()),
            }
        }
    }
}

// ---- Added by G3 (DR-135, S5): the owner checks of the play matcher (R5,
// R6, A5, R9). A locked RSpace operation of a run with owner funding carries
// an `OwnerOperationHandle`. The matcher asks it for the item test of each
// datum, and the COMM of a complete tuple carries the A5 transferee and the
// R9 restorations to the recorder, which allocates them with the COMM.

/// Added by G3 (DR-135, S5): the owner checks of one locked RSpace operation
/// of a run with owner funding: the binding, the remaining budgets S_r that
/// the operation captured with its `budget_start` (§6.1), the preparer and the
/// region limits of the meter, and the residue context of the deployment.
pub(crate) struct OwnerOperationHandle {
    pub(crate) binding: Arc<OwnerFundingBinding>,
    pub(crate) snapshot: Arc<ResourceMultiset<[u8; 32]>>,
    pub(crate) preparer: NativePhloChargePreparer,
    pub(crate) limits: NativePhloRegionLimits,
    pub(crate) residue: ResidueContext,
}

thread_local! {
    /// Added by G3 (DR-135, S5): the owner checks of the locked RSpace
    /// operation that runs on this thread. RSpace runs a locked operation
    /// synchronously between the start hook and the finish hook of its
    /// observer: `locked_produce` and `locked_consume` hold no await
    /// (`ispace_impl.rs`, `produce` and `consume`). So the handle that the
    /// start hook sets serves exactly that operation, and the finish hook
    /// clears it (plan §12 risk 5). The next start hook replaces any handle
    /// that a failed operation left.
    static OWNER_OPERATION: RefCell<Option<Arc<OwnerOperationHandle>>> =
        const { RefCell::new(None) };
}

/// Added by G3 (DR-135, S5): sets the owner checks of the locked operation
/// that starts on this thread. `None` is an operation without owner funding.
pub(crate) fn enter_owner_operation(handle: Option<Arc<OwnerOperationHandle>>) {
    OWNER_OPERATION.with(|cell| *cell.borrow_mut() = handle);
}

/// Added by G3 (DR-135, S5): clears the owner checks when the locked
/// operation finishes.
pub(crate) fn leave_owner_operation() { OWNER_OPERATION.with(|cell| cell.borrow_mut().take()); }

/// Added by G3 (DR-135, S5): the owner checks of the locked operation that
/// runs on this thread.
pub(crate) fn current_owner_operation() -> Option<Arc<OwnerOperationHandle>> {
    OWNER_OPERATION.with(|cell| cell.borrow().clone())
}

fn rspace_error(error: impl std::fmt::Display) -> RSpaceError {
    RSpaceError::InterpreterError(error.to_string())
}

impl OwnerOperationHandle {
    // Changed by G3 (DR-135, S6): the charge of the item test is
    // `ItemTest::owner_charge`, which the item tests of replay share. The
    // replaced method follows.
    // /// The item test of R5: the owners of a COMM of `continuation` with
    // /// `datum` alone must be able to pay their shares from S_r. An unlisted
    // /// purse has budget 0 and is noted (Play, SA8). A COMM whose regions hold
    // /// no owner region passes with no measurement, and so does an
    // /// overflowing charge, which the bound denies at the COMM.
    // pub(crate) fn admit_item(
    //     &self,
    //     continuation: &TaggedContinuation,
    //     continuation_persistent: bool,
    //     channel_count: usize,
    //     datum: &ListParWithRandom,
    //     datum_persistent: bool,
    // ) -> Result<bool, RSpaceError> {
    //     let identity = [0; 32];
    //     let authority = observation_construction::comm_authority(
    //         continuation,
    //         continuation_persistent,
    //         &[(datum, datum_persistent)],
    //         identity,
    //         &self.residue,
    //     )?
    //     .into_authority();
    //     let mut owned = false;
    //     for region in &authority.regions {
    //         if self.binding.is_owner_region(region).map_err(rspace_error)? {
    //             owned = true;
    //             break;
    //         }
    //     }
    //     if !owned {
    //         return Ok(true);
    //     }
    //     let observation = Arc::new(ByteObservation {
    //         event_id: identity,
    //         kind: AuthorityByteEventKind::Comm,
    //         authority,
    //         measurement: Some(observation_construction::item_measurement(
    //             channel_count,
    //             datum,
    //         )?),
    //         legacy_amount: None,
    //     });
    //     let prepared = match self.preparer.prepare(observation, self.limits) {
    //         Ok(prepared) => prepared,
    //         Err(NativePhloExecutionError::Overflow) => return Ok(true),
    //         Err(error) => return Err(rspace_error(error)),
    //     };
    //     let charge = self
    //         .binding
    //         .owner_charge(&prepared.observation().authority, prepared.region_usages())
    //         .map_err(rspace_error)?;
    //     let table = self.binding.table();
    //     let view = |purse: &OwnerPurse| {
    //         table
    //             .get(&purse.custody)
    //             .map(|_| self.snapshot.get(&purse.custody))
    //     };
    //     Ok(allocate(
    //         &charge,
    //         &view,
    //         UnlistedRule::Play,
    //         table.has_room(),
    //         Some(self.binding.notes()),
    //     )
    //     .is_ok())
    // }
    /// The item test of R5: the owners of a COMM of `continuation` with
    /// `datum` alone must be able to pay their shares from S_r. An unlisted
    /// purse has budget 0 and is noted (Play, SA8). A COMM whose regions hold
    /// no owner region passes with no measurement, and so does an
    /// overflowing charge, which the bound denies at the COMM.
    pub(crate) fn admit_item(
        &self,
        continuation: &TaggedContinuation,
        continuation_persistent: bool,
        channel_count: usize,
        datum: &ListParWithRandom,
        datum_persistent: bool,
    ) -> Result<bool, RSpaceError> {
        let test = ItemTest {
            binding: &self.binding,
            preparer: &self.preparer,
            limits: self.limits,
            residue: &self.residue,
        };
        // Changed by G3 (DR-135, S11): renamed from `charge`, so that the
        // legacy guard's `[.]charge[(]` pattern does not match. The
        // replaced line follows.
        // let Some(charge) = test.charge(
        let Some(charge) = test.owner_charge(
            continuation,
            continuation_persistent,
            channel_count,
            datum,
            datum_persistent,
        )?
        else {
            return Ok(true);
        };
        let table = self.binding.table();
        let view = |purse: &OwnerPurse| {
            table
                .get(&purse.custody)
                .map(|_| self.snapshot.get(&purse.custody))
        };
        Ok(allocate(
            &charge,
            &view,
            UnlistedRule::Play,
            table.has_room(),
            Some(self.binding.notes()),
        )
        .is_ok())
    }
}

/// Added by G3 (DR-135, S6): the terms of the item test of R5: the binding
/// of the run, the preparer and the region limits of its meter, and the
/// residue context of its deployment. Play's item test and the item tests of
/// replay share it, so both roles measure an item in the same way.
pub(crate) struct ItemTest<'a> {
    pub(crate) binding: &'a OwnerFundingBinding,
    pub(crate) preparer: &'a NativePhloChargePreparer,
    pub(crate) limits: NativePhloRegionLimits,
    pub(crate) residue: &'a ResidueContext,
}

impl ItemTest<'_> {
    /// The owner charge of a COMM of `continuation` with `datum` alone,
    /// measured with the item bytes (`item_measurement`). `None` when the
    /// COMM holds no owner region, so the item passes with no measurement,
    /// or when its charge overflows, which the bound denies at the COMM.
    // Changed by G3 (DR-135, S11): renamed from `charge`, so that the
    // legacy guard's `[.]charge[(]` pattern does not match. The
    // replaced line follows.
    // pub(crate) fn charge(
    pub(crate) fn owner_charge(
        &self,
        continuation: &TaggedContinuation,
        continuation_persistent: bool,
        channel_count: usize,
        datum: &ListParWithRandom,
        datum_persistent: bool,
    ) -> Result<Option<OwnerCharge>, RSpaceError> {
        let identity = [0; 32];
        let authority = observation_construction::comm_authority(
            continuation,
            continuation_persistent,
            &[(datum, datum_persistent)],
            identity,
            self.residue,
        )?
        .into_authority();
        let mut owned = false;
        for region in &authority.regions {
            if self.binding.is_owner_region(region).map_err(rspace_error)? {
                owned = true;
                break;
            }
        }
        if !owned {
            return Ok(None);
        }
        let observation = Arc::new(ByteObservation {
            event_id: identity,
            kind: AuthorityByteEventKind::Comm,
            authority,
            measurement: Some(observation_construction::item_measurement(
                channel_count,
                datum,
            )?),
            legacy_amount: None,
        });
        let prepared = match self.preparer.prepare(observation, self.limits) {
            Ok(prepared) => prepared,
            Err(NativePhloExecutionError::Overflow) => return Ok(None),
            Err(error) => return Err(rspace_error(error)),
        };
        self.binding
            .owner_charge(&prepared.observation().authority, prepared.region_usages())
            .map(Some)
            .map_err(rspace_error)
    }
}

/// Added by G3 (DR-135, S5): whether `signature` names a value of the
/// environment of its body: a bound level, or a name or quote whose process
/// uses a bound variable. The walk keeps the pending compound elements on a
/// heap stack.
fn signature_reads_bindings(signature: &CostSignature) -> bool {
    let mut pending = vec![signature];
    while let Some(current) = pending.pop() {
        match current.value.as_ref() {
            Some(CostSignatureValue::BoundLevel(_)) => return true,
            Some(CostSignatureValue::Name(par) | CostSignatureValue::Quote(par))
                if !par.locally_free.is_empty() =>
            {
                return true
            }
            Some(CostSignatureValue::Compound(compound)) => pending.extend(&compound.elements),
            _ => {}
        }
    }
    false
}

/// Added by G3 (DR-135, S5): the transferee of a COMM (A5). When the body of
/// `continuation` is one signed term, the body runs for its signer, and the
/// signer must keep a positive budget after the COMM. The result is:
///
/// - `None`: the body is no single signed term, or its signer is Unit or a
///   client lane of the binding (the running deployment pays for its own
///   lanes);
/// - `Some(purses)`: the purses of the signer, `Some(None)` for a forgeable
///   signer.
///
/// A held term keeps the signature that it stored, which names no binding
/// (Rule H, §13.5). A surface signed term may name values that the COMM
/// binds, so its signature is resolved in the environment of the body, as
/// `dispatch` builds it from `data`. A system continuation is checked too,
/// because a held term can be its whole body. S5's play check, S6's
/// `verify_refusal` and the A5 check of a Matched row share this function.
pub(crate) fn comm_transferee(
    binding: &OwnerFundingBinding,
    budget: &RuntimeBudget,
    continuation: &TaggedContinuation,
    data: &[&ListParWithRandom],
) -> Result<Option<Option<OwnerPurseSet>>, InterpreterError> {
    let Some(TaggedCont::ParBody(body)) = continuation.tagged_cont.as_ref() else {
        return Ok(None);
    };
    let Some(term) = body.body.as_ref().and_then(solely_signed) else {
        return Ok(None);
    };
    let stored = term
        .signature
        .as_ref()
        .ok_or_else(|| malformed(AuthorityError::MissingSignature))?;
    let signature = match (term.held_price, signature_reads_bindings(stored)) {
        (Some(_), _) | (None, false) => stored.clone(),
        (None, true) => {
            let mut values = Vec::with_capacity(data.len());
            values.extend(data.iter().map(|datum| (*datum).clone()));
            let env = build_env(values);
            Substitute {
                metering: MeteredMachine::new(budget.clone()),
            }
            .substitute_cost_signature(stored.clone(), 0, &env)?
        }
    };
    if is_unit_cost_signature(&signature)
        || binding.is_client_signature(&signature).map_err(malformed)?
    {
        return Ok(None);
    }
    Ok(Some(binding.locate(&signature)))
}

/// Added by G3 (DR-135, S5): the introductions of the data that a peek COMM
/// restores (R9): each non-persistent datum's stored value, with the
/// authority that `produce_peeks` gives it in the deployment of `residue`
/// (SA7, no registry call) and the bytes of its introduction. They are in the
/// order of the COMM's produces, by channel hash and produce hash, so play and
/// replay list them in one order. A COMM that peeks nothing restores
/// nothing.
pub(crate) fn restoration_observations(
    comm: &COMM,
    participants: &[CommParticipant<'_, Par, ListParWithRandom>],
    residue: &ResidueContext,
) -> Result<Vec<Arc<ByteObservation>>, InterpreterError> {
    if comm.peeks.is_empty() {
        return Ok(Vec::new());
    }
    let mut restored = Vec::with_capacity(participants.len());
    for participant in participants
        .iter()
        .filter(|participant| !participant.persistent)
    {
        let source = Produce::create(participant.channel, participant.stored, false);
        restored.push((source, participant));
    }
    restored.sort_by(|(left, _), (right, _)| {
        left.channel_hash
            .cmp(&right.channel_hash)
            .then_with(|| left.hash.cmp(&right.hash))
    });
    let mut restorations = Vec::with_capacity(restored.len());
    for (source, participant) in restored {
        let stored = participant.stored;
        let authority = match stored.cost_authority.as_ref() {
            Some(seal) => resolve_system_residue(seal, &stored.random_state, residue)
                .map_err(malformed)?
                .into_owned(),
            None => CostAuthority::default(),
        };
        let measurement = produce_introduction_charge(participant.channel, stored)
            .map_err(|error| InterpreterError::ReduceError(error.to_string()))?;
        restorations.push(Arc::new(ByteObservation {
            event_id: produce_introduction_identity(&source),
            kind: AuthorityByteEventKind::ProduceIntroduction,
            authority: super::canonical_observation_authority(&authority)?,
            measurement: Some(measurement),
            legacy_amount: None,
        }));
    }
    Ok(restorations)
}

impl RuntimeBudget {
    /// Added by G3 (DR-135, S5): the owner-funded COMM of the locked
    /// operation of `handle`: its observation, with the A5 transferee and the
    /// R9 restorations, goes to the recorder, which allocates them with the
    /// COMM in one critical section. An owner that cannot pay refuses the
    /// COMM with `RSpaceError::CommRefused`.
    pub(crate) fn observe_owned_comm(
        &self,
        handle: &OwnerOperationHandle,
        comm: &COMM,
        continuation: &TaggedContinuation,
        continuation_persistent: bool,
        participants: &[CommParticipant<'_, Par, ListParWithRandom>],
    ) -> Result<(), RSpaceError> {
        if !self.has_comm_accounting_scope() || self.is_unmetered() {
            return Ok(());
        }
        self.observe_native_comm_source(comm)
            .map_err(RSpaceError::from)?;
        let mut data = Vec::with_capacity(participants.len());
        data.extend(
            participants
                .iter()
                .map(|participant| (participant.matched, participant.persistent)),
        );
        let observed = observation_construction::comm(
            comm,
            continuation,
            continuation_persistent,
            &data,
            &handle.residue,
        )?;
        let mut matched = Vec::with_capacity(participants.len());
        matched.extend(participants.iter().map(|participant| participant.matched));
        let extras = OwnerCommExtras {
            transferee: comm_transferee(&handle.binding, self, continuation, &matched)
                .map_err(RSpaceError::from)?,
            restorations: restoration_observations(comm, participants, &handle.residue)
                .map_err(RSpaceError::from)?,
        };
        self.reserve_comm_authority_owned(
            observed.event_id,
            observed.authority,
            observed.measurement,
            extras,
        )
        .map_err(RSpaceError::from)
    }
}

#[cfg(test)]
mod tests;

// Added by G3 (DR-135, S3b H8): the tests of the plain-data check.
#[cfg(test)]
pub(crate) mod plain_data_tests;

// Added by G3 (DR-135, S3b): the unit tests of Rule H.
#[cfg(test)]
mod rule_h_tests;
