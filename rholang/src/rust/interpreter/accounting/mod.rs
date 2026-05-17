use std::collections::VecDeque;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use costs::Cost;
use crypto::rust::hash::blake2b256::Blake2b256;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{GPrivate, GUnforgeable, Par};
use models::rust::rholang::implicits::concatenate_pars;
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;

use super::errors::InterpreterError;

pub mod cost_accounting;
pub mod costs;
pub mod has_cost;

const DEPLOY_SIGNATURE_DOMAIN: &[u8] = b"f1r3node:cost-accounted-rho:deploy-signature:v1";
const COST_TRACE_DIGEST_DOMAIN: &[u8] = b"f1r3node:cost-accounted-rho:cost-trace:v1";
pub const MAX_COST_TRACE_EVENTS: u64 = 1_048_576;
pub const MAX_COST_TRACE_PRIMITIVE_DESCRIPTOR_BYTES: usize = 512;
pub const MAX_COST_TRACE_SOURCE_PATH_COMPONENTS: usize = 1024;

#[derive(Clone)]
pub struct RuntimeBudget {
    initial_tokens: Arc<AtomicI64>,
    consumed_tokens: Arc<AtomicI64>,
    signature: Arc<Mutex<Sig>>,
    deploy_id: Arc<Mutex<[u8; 32]>>,
    log: Arc<Mutex<VecDeque<Cost>>>,
    event_log: Arc<Mutex<VecDeque<BillableTokenEvent>>>,
    // Consensus replay uses this full finalization-window trace. It is kept
    // separate from the capped diagnostic event log so observability cleanup
    // cannot remove replay authentication evidence.
    cost_trace_events: Arc<Mutex<Vec<BillableTokenEvent>>>,
    // Slots are reserved before the fuel CAS. This keeps trace retention
    // bounded without serializing successful parallel reservations on the
    // cost-trace vector lock.
    cost_trace_event_slots: Arc<AtomicU64>,
    last_oop_event: Arc<Mutex<Option<BillableTokenEvent>>>,
    max_log_entries: usize,
    unmetered: Arc<AtomicU64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CostTraceDigest {
    // Canonical hash of successful reservations plus the optional OOP
    // boundary. The digest is order-insensitive for successful parallel
    // reservations but still sensitive to event descriptors and OOP boundary.
    pub digest: Vec<u8>,
    pub event_count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourcePath(pub Vec<u32>);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RedexId(pub u64);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BillableKind {
    SourceStep,
    Primitive(String),
    Substitution,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BillableTokenEvent {
    pub deploy_id: [u8; 32],
    pub source_path: SourcePath,
    pub redex_id: RedexId,
    pub local_index: u64,
    pub kind: BillableKind,
    pub weight: u64,
}

impl RuntimeBudget {
    fn resolve_max_log_entries() -> usize { 1024 }

    pub fn new(initial_value: Cost) -> Self {
        let max_log_entries = Self::resolve_max_log_entries();
        let initial_capacity = if max_log_entries == 0 {
            0
        } else if max_log_entries == usize::MAX {
            1024
        } else {
            max_log_entries.min(1024)
        };

        Self {
            initial_tokens: Arc::new(AtomicI64::new(initial_value.value)),
            consumed_tokens: Arc::new(AtomicI64::new(0)),
            signature: Arc::new(Mutex::new(Sig::Unit)),
            deploy_id: Arc::new(Mutex::new([0; 32])),
            log: Arc::new(Mutex::new(VecDeque::with_capacity(initial_capacity))),
            event_log: Arc::new(Mutex::new(VecDeque::with_capacity(initial_capacity))),
            cost_trace_events: Arc::new(Mutex::new(Vec::with_capacity(initial_capacity))),
            cost_trace_event_slots: Arc::new(AtomicU64::new(0)),
            last_oop_event: Arc::new(Mutex::new(None)),
            max_log_entries,
            unmetered: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn unmetered() -> Self {
        let budget = Self::new(Cost::unsafe_max());
        budget.unmetered.store(1, Ordering::Release);
        budget
    }

    pub fn reserve_canonical_with_cost(
        &self,
        event: BillableTokenEvent,
        amount: Cost,
    ) -> Result<(), InterpreterError> {
        self.reserve_canonical(event)?;
        self.append_cost_log(amount);
        Ok(())
    }

    fn append_cost_log(&self, amount: Cost) {
        if self.max_log_entries > 0 {
            let mut log = self.log.lock().unwrap();
            if log.len() >= self.max_log_entries {
                let _ = log.pop_front();
            }
            log.push_back(amount);
        }
    }

    fn append_event_log(&self, event: BillableTokenEvent) {
        if self.max_log_entries > 0 {
            let mut log = self.event_log.lock().unwrap();
            if log.len() >= self.max_log_entries {
                let _ = log.pop_front();
            }
            log.push_back(event);
        }
    }

    fn append_cost_trace_event(&self, event: BillableTokenEvent) {
        self.cost_trace_events
            .lock()
            .expect("cost trace event window")
            .push(event);
    }

    fn validate_billable_event(event: &BillableTokenEvent) -> Result<(), InterpreterError> {
        if event.weight == 0 || event.weight > i64::MAX as u64 {
            return Err(InterpreterError::OutOfPhlogistonsError);
        }

        if event.source_path.0.len() > MAX_COST_TRACE_SOURCE_PATH_COMPONENTS {
            return Err(InterpreterError::OutOfPhlogistonsError);
        }

        if let BillableKind::Primitive(name) = &event.kind {
            if name.len() > MAX_COST_TRACE_PRIMITIVE_DESCRIPTOR_BYTES {
                return Err(InterpreterError::OutOfPhlogistonsError);
            }
        }

        Ok(())
    }

    fn try_reserve_cost_trace_slot(&self) -> Result<(), InterpreterError> {
        let mut current = self.cost_trace_event_slots.load(Ordering::Acquire);
        loop {
            if current >= MAX_COST_TRACE_EVENTS {
                return Err(InterpreterError::OutOfPhlogistonsError);
            }

            match self.cost_trace_event_slots.compare_exchange(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Ok(()),
                Err(next) => current = next,
            }
        }
    }

    fn release_cost_trace_slot(&self) {
        let previous = self.cost_trace_event_slots.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous > 0, "cost trace slot accounting underflow");
    }

    pub fn reserve_canonical(&self, event: BillableTokenEvent) -> Result<(), InterpreterError> {
        if self.unmetered.load(Ordering::Acquire) != 0 {
            return Ok(());
        }

        Self::validate_billable_event(&event)?;
        self.try_reserve_cost_trace_slot()?;

        let weight = event.weight as i64;
        loop {
            let consumed = self.consumed_tokens.load(Ordering::Acquire);
            let initial = self.initial_tokens.load(Ordering::Acquire);
            if consumed < 0 || initial < 0 {
                self.release_cost_trace_slot();
                return Err(InterpreterError::OutOfPhlogistonsError);
            }
            let next = consumed.saturating_add(weight);
            if next > initial {
                self.consumed_tokens.store(initial, Ordering::Release);
                let mut last_oop_event = self.last_oop_event.lock().expect("last OOP event lock");
                if last_oop_event.is_none() {
                    *last_oop_event = Some(event);
                } else {
                    self.release_cost_trace_slot();
                }
                return Err(InterpreterError::OutOfPhlogistonsError);
            }
            if self
                .consumed_tokens
                .compare_exchange(consumed, next, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                self.append_cost_trace_event(event.clone());
                self.append_event_log(event);
                return Ok(());
            }
        }
    }

    pub fn get(&self) -> Cost {
        if self.unmetered.load(Ordering::Acquire) != 0 {
            return Cost::unsafe_max();
        }
        let initial = self.initial_tokens.load(Ordering::Acquire);
        let consumed = self.consumed_tokens.load(Ordering::Acquire);
        Cost::create(initial.saturating_sub(consumed), "token budget remaining")
    }

    pub fn set(&self, new_value: Cost) {
        let token = Token::coalesced(self.signature(), cost_value_to_token_count(new_value.value));
        self.reset_from_token(&token);
    }

    pub fn reset_from_signed_process(&self, signed: &SignedProcess) {
        if let Some(token) = signed.token() {
            self.reset_from_token(token);
        }
    }

    pub fn reset_from_token(&self, token: &Token) {
        self.initial_tokens
            .store(token.remaining_units_i64(), Ordering::Release);
        self.consumed_tokens.store(0, Ordering::Release);
        *self.signature.lock().expect("signature lock") = token.signature();
        self.event_log.lock().expect("event log").clear();
        self.cost_trace_events
            .lock()
            .expect("cost trace event window")
            .clear();
        self.cost_trace_event_slots.store(0, Ordering::Release);
        *self.last_oop_event.lock().expect("last OOP event lock") = None;
    }

    pub fn set_deploy_signature(&self, signature: &[u8]) {
        let mut domain_separated_signature =
            Vec::with_capacity(DEPLOY_SIGNATURE_DOMAIN.len() + signature.len());
        domain_separated_signature.extend_from_slice(DEPLOY_SIGNATURE_DOMAIN);
        domain_separated_signature.extend_from_slice(signature);
        let hash = Blake2b256::hash(domain_separated_signature);
        let mut deploy_id = [0; 32];
        deploy_id.copy_from_slice(&hash[..32]);
        *self.deploy_id.lock().expect("deploy id lock") = deploy_id;
        // Cost-accounting channels are internal capabilities derived from,
        // but not equal to, the wire signature. Domain separation prevents
        // accidental reuse of raw signature bytes as another protocol hash.
        *self.signature.lock().expect("signature lock") = Sig::Hash(hash);
    }

    pub fn signature(&self) -> Sig { self.signature.lock().expect("signature lock").clone() }

    pub fn deploy_id(&self) -> [u8; 32] { *self.deploy_id.lock().expect("deploy id lock") }

    pub fn set_unmetered(&self, unmetered: bool) {
        // System deploys use unmetered mode only around post-evaluation
        // settlement work. The flag intentionally bypasses runtime fuel
        // reservation instead of crediting tokens back to the user budget;
        // turning it off restores the same consumed/remaining counters. New
        // consensus paths should prefer `enter_unmetered_scope`, which
        // restores this flag on every return path.
        self.unmetered
            .store(if unmetered { 1 } else { 0 }, Ordering::Release);
    }

    pub fn enter_unmetered_scope(&self) -> UnmeteredBudgetScope {
        let previous = self.unmetered.swap(1, Ordering::AcqRel);
        UnmeteredBudgetScope {
            budget: self.clone(),
            previous,
        }
    }

    pub fn total_cost(&self) -> Cost {
        if self.unmetered.load(Ordering::Acquire) != 0 {
            return Cost::create(0, "unmetered token budget");
        }
        Cost::create(
            self.consumed_tokens.load(Ordering::Acquire),
            "consumed source-token units",
        )
    }

    pub fn remaining(&self) -> Cost { self.get() }

    pub fn get_log(&self) -> Vec<Cost> { self.log.lock().unwrap().iter().cloned().collect() }

    pub fn get_event_log(&self) -> Vec<BillableTokenEvent> {
        self.event_log.lock().unwrap().iter().cloned().collect()
    }

    pub fn get_canonical_event_log(&self) -> Vec<BillableTokenEvent> {
        let mut events = self.get_event_log();
        events.sort();
        events
    }

    pub fn last_oop_event(&self) -> Option<BillableTokenEvent> {
        self.last_oop_event
            .lock()
            .expect("last OOP event lock")
            .clone()
    }

    pub fn clear_log(&self) { self.log.lock().unwrap().clear(); }

    pub fn clear_event_log(&self) { self.event_log.lock().unwrap().clear(); }

    /// Returns the finalized consensus-trace event count.
    ///
    /// This is intended for the deploy finalization/replay boundary after
    /// evaluation tasks have quiesced. Mid-evaluation observers may see a
    /// reservation before its trace event has been appended.
    pub fn cost_trace_event_count(&self) -> u64 {
        let success_count = self
            .cost_trace_events
            .lock()
            .expect("cost trace event window")
            .len() as u64;
        let oop_count = u64::from(
            self.last_oop_event
                .lock()
                .expect("last OOP event lock")
                .is_some(),
        );
        success_count + oop_count
    }

    /// Builds the finalized consensus cost-trace digest.
    ///
    /// Successful reservation events are canonicalized before hashing so
    /// worker completion order does not affect replay. The optional OOP
    /// boundary is tagged separately. Call this only after the evaluator has
    /// joined all deploy work for the finalization window.
    pub fn cost_trace_digest(&self) -> CostTraceDigest {
        fn push_len_prefixed(bytes: &mut Vec<u8>, data: &[u8]) {
            bytes.extend_from_slice(&(data.len() as u64).to_le_bytes());
            bytes.extend_from_slice(data);
        }

        fn push_event(bytes: &mut Vec<u8>, tag: u8, event: &BillableTokenEvent) {
            bytes.push(tag);
            bytes.extend_from_slice(&event.deploy_id);
            bytes.extend_from_slice(&(event.source_path.0.len() as u64).to_le_bytes());
            for component in &event.source_path.0 {
                bytes.extend_from_slice(&component.to_le_bytes());
            }
            bytes.extend_from_slice(&event.redex_id.0.to_le_bytes());
            bytes.extend_from_slice(&event.local_index.to_le_bytes());
            match &event.kind {
                BillableKind::SourceStep => bytes.push(0),
                BillableKind::Primitive(name) => {
                    bytes.push(1);
                    push_len_prefixed(bytes, name.as_bytes());
                }
                BillableKind::Substitution => bytes.push(2),
            }
            bytes.extend_from_slice(&event.weight.to_le_bytes());
        }

        let mut tagged_events = self
            .cost_trace_events
            .lock()
            .expect("cost trace event window")
            .iter()
            .cloned()
            .map(|event| (0u8, event))
            .collect::<Vec<_>>();
        if let Some(event) = self.last_oop_event() {
            tagged_events.push((1u8, event));
        }
        tagged_events.sort_by(|(left_tag, left), (right_tag, right)| {
            left_tag.cmp(right_tag).then_with(|| left.cmp(right))
        });

        let mut bytes = Vec::new();
        bytes.extend_from_slice(COST_TRACE_DIGEST_DOMAIN);
        bytes.extend_from_slice(&(tagged_events.len() as u64).to_le_bytes());
        for (tag, event) in &tagged_events {
            push_event(&mut bytes, *tag, event);
        }

        CostTraceDigest {
            digest: Blake2b256::hash(bytes).to_vec(),
            event_count: tagged_events.len() as u64,
        }
    }
}

pub struct UnmeteredBudgetScope {
    budget: RuntimeBudget,
    previous: u64,
}

impl Drop for UnmeteredBudgetScope {
    fn drop(&mut self) {
        self.budget
            .unmetered
            .store(self.previous, Ordering::Release);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Sig {
    Unit,
    Hash(Vec<u8>),
    And(Box<Sig>, Box<Sig>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Token {
    Unit,
    Count { sig: Sig, remaining: u64 },
    Gate { sig: Sig, rest: Box<Token> },
}

impl Token {
    pub fn coalesced(sig: Sig, remaining: u64) -> Self { Token::Count { sig, remaining } }

    pub fn gate(sig: Sig, rest: Token) -> Self {
        Token::Gate {
            sig,
            rest: Box::new(rest),
        }
    }

    pub fn signature(&self) -> Sig {
        match self {
            Token::Unit => Sig::Unit,
            Token::Count { sig, .. } | Token::Gate { sig, .. } => sig.clone(),
        }
    }

    pub fn remaining_units(&self) -> u64 {
        match self {
            Token::Unit => 0,
            Token::Count { remaining, .. } => *remaining,
            Token::Gate { rest, .. } => 1u64.saturating_add(rest.remaining_units()),
        }
    }

    fn remaining_units_i64(&self) -> i64 { token_units_to_i64(self.remaining_units()) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignedProcess {
    Signed { process: Par, sig: Sig },
    Token(Token),
    Par(Box<SignedProcess>, Box<SignedProcess>),
}

impl SignedProcess {
    pub fn metered(process: Par, sig: Sig, token_count: u64) -> Self {
        SignedProcess::Par(
            Box::new(SignedProcess::Signed {
                process,
                sig: sig.clone(),
            }),
            Box::new(SignedProcess::Token(Token::coalesced(sig, token_count))),
        )
    }

    pub fn source_process(&self) -> Option<&Par> {
        match self {
            SignedProcess::Signed { process, .. } => Some(process),
            SignedProcess::Token(_) => None,
            SignedProcess::Par(left, right) => {
                left.source_process().or_else(|| right.source_process())
            }
        }
    }

    pub fn token(&self) -> Option<&Token> {
        match self {
            SignedProcess::Signed { .. } => None,
            SignedProcess::Token(token) => Some(token),
            SignedProcess::Par(left, right) => left.token().or_else(|| right.token()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignatureChannel {
    pub par: Par,
}

impl SignatureChannel {
    pub fn from_sig(sig: &Sig) -> Self {
        match sig {
            Sig::Unit => SignatureChannel {
                par: Par::default(),
            },
            Sig::Hash(bytes) => SignatureChannel {
                par: Par::default().with_unforgeables(vec![GUnforgeable {
                    unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
                        id: Blake2b256::hash(bytes.clone()),
                    })),
                }]),
            },
            Sig::And(left, right) => {
                let left_channel = Self::from_sig(left).par;
                let right_channel = Self::from_sig(right).par;
                let combined = concatenate_pars(left_channel, right_channel);
                SignatureChannel {
                    par: ParSortMatcher::sort_match(&combined).term,
                }
            }
        }
    }
}

fn cost_value_to_token_count(value: i64) -> u64 {
    if value < 0 {
        0
    } else {
        value as u64
    }
}

fn token_units_to_i64(value: u64) -> i64 {
    if value > i64::MAX as u64 {
        i64::MAX
    } else {
        value as i64
    }
}

#[cfg(kani)]
mod kani_cost_accounting {
    use super::*;

    #[kani::proof]
    fn cost_value_to_token_count_rejects_negative_values() {
        let value: i64 = kani::any();
        let tokens = cost_value_to_token_count(value);

        if value < 0 {
            assert_eq!(tokens, 0);
        } else {
            assert_eq!(tokens, value as u64);
        }
    }

    #[kani::proof]
    fn token_remaining_units_i64_saturates_to_i64_max() {
        let remaining: u64 = kani::any();
        let as_i64 = token_units_to_i64(remaining);

        if remaining > i64::MAX as u64 {
            assert_eq!(as_i64, i64::MAX);
        } else {
            assert_eq!(as_i64, remaining as i64);
        }
    }
}
