use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Weak};

use block_storage::rust::casperbuffer::casper_buffer_key_value_storage::CasperBufferKeyValueStorage;
use block_storage::rust::casperbuffer::pending_request_policy::PendingRequestPolicy;
use models::rust::block_hash::{BlockHash, BlockHashSerde};
use parking_lot::Mutex;
use shared::rust::store::key_value_store::KvStoreError;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CompletionPhase {
    Reserved,
    Observed,
    Persisted,
    Cancelled,
}

struct OwnerState<D> {
    policy: PendingRequestPolicy,
    committed: Option<PendingRequestPolicy>,
    terminal: bool,
    data: D,
    operations: Vec<Weak<Completion<D>>>,
}

pub struct RequestOwner<D> {
    registry_identity: Arc<()>,
    hash: BlockHash,
    state: Mutex<OwnerState<D>>,
}

impl<D> RequestOwner<D> {
    pub fn hash(&self) -> &BlockHash { &self.hash }

    pub fn policy(&self) -> PendingRequestPolicy { self.state.lock().policy.clone() }

    pub fn inspect<R>(&self, inspect: impl FnOnce(&PendingRequestPolicy, &D) -> R) -> R {
        let state = self.state.lock();
        inspect(&state.policy, &state.data)
    }
}

struct Registry<D> {
    active: HashMap<BlockHash, Arc<RequestOwner<D>>>,
    live: HashMap<BlockHash, Weak<RequestOwner<D>>>,
    retired: HashMap<BlockHash, RetiredRetryBudget>,
}

#[derive(Clone, Copy)]
struct RetiredRetryBudget {
    attempts: u32,
    quarantine_until: u64,
}

pub struct RequestOwners<D> {
    identity: Arc<()>,
    registry: Mutex<Registry<D>>,
    buffer: CasperBufferKeyValueStorage,
    permits: Arc<Semaphore>,
    completions: Arc<Mutex<VecDeque<Arc<Completion<D>>>>>,
    active_limit: usize,
    new_data: fn(bool) -> D,
    expiry_pass: tokio::sync::Mutex<()>,
}

struct Completion<D> {
    owner: Arc<RequestOwner<D>>,
    phase: Mutex<CompletionPhase>,
    peer: bool,
    _permit: OwnedSemaphorePermit,
}

pub struct RetryOperation<D> {
    completion: Option<Arc<Completion<D>>>,
    queue: Arc<Mutex<VecDeque<Arc<Completion<D>>>>>,
    buffer: CasperBufferKeyValueStorage,
}

pub enum RetrySelection<R> {
    None,
    Suppressed(R),
    Dispatch(bool, R),
}

pub enum PreparedRetry<D, R> {
    None,
    Suppressed(R),
    Dispatch(RetryOperation<D>, R),
}

pub enum Activation<D> {
    Active(Arc<RequestOwner<D>>),
    Ineligible,
    AtCapacity,
}

impl<D> Activation<D> {
    fn into_option(self) -> Option<Arc<RequestOwner<D>>> {
        match self {
            Self::Active(owner) => Some(owner),
            Self::Ineligible | Self::AtCapacity => None,
        }
    }
}

impl<D> RequestOwners<D> {
    pub fn new(
        buffer: CasperBufferKeyValueStorage,
        active_limit: usize,
        operation_limit: usize,
        new_data: fn(bool) -> D,
    ) -> Self {
        Self {
            identity: Arc::new(()),
            registry: Mutex::new(Registry {
                active: HashMap::new(),
                live: HashMap::new(),
                retired: HashMap::new(),
            }),
            buffer,
            permits: Arc::new(Semaphore::new(operation_limit)),
            completions: Arc::new(Mutex::new(VecDeque::with_capacity(operation_limit))),
            active_limit,
            new_data,
            expiry_pass: tokio::sync::Mutex::new(()),
        }
    }

    pub fn buffer(&self) -> &CasperBufferKeyValueStorage { &self.buffer }

    pub fn active_count(&self) -> usize { self.registry.lock().active.len() }

    pub fn active_owners(&self) -> Vec<Arc<RequestOwner<D>>> {
        self.registry.lock().active.values().cloned().collect()
    }

    pub fn deferred_count(&self) -> usize { self.completions.lock().len() }

    pub fn available_operations(&self) -> usize { self.permits.available_permits() }

    #[cfg(test)]
    pub fn retry_budget(&self, hash: &BlockHash) -> Result<(u32, Option<u64>), KvStoreError> {
        let mut registry = self.registry.lock();
        if let Some(retired) = registry.retired.get(hash) {
            return Ok((retired.attempts, Some(retired.quarantine_until)));
        }
        Ok(self
            .lookup_locked(&mut registry, hash, self.new_data)?
            .map_or((0, None), |owner| {
                owner.inspect(|policy, _| {
                    (policy.retry_attempts, policy.retry_budget_quarantine_until)
                })
            }))
    }

    pub fn forget(&self, hash: &BlockHash) -> Result<(), KvStoreError> {
        let mut registry = self.registry.lock();
        self.buffer.remove(BlockHashSerde(hash.clone()))?;
        if let Some(owner) = registry.live.get(hash).and_then(Weak::upgrade) {
            owner.state.lock().terminal = true;
        }
        registry.active.remove(hash);
        registry.live.remove(hash);
        registry.retired.remove(hash);
        Ok(())
    }

    fn retire_locked(
        &self,
        registry: &mut Registry<D>,
        owner: &Arc<RequestOwner<D>>,
        state: &mut OwnerState<D>,
        budget: u32,
        now: u64,
        quarantine_ms: u64,
    ) -> Result<bool, KvStoreError> {
        if state.terminal
            || state.policy.retry_attempts < budget
            || state
                .operations
                .iter()
                .filter_map(Weak::upgrade)
                .any(|operation| *operation.phase.lock() == CompletionPhase::Reserved)
        {
            return Ok(false);
        }
        let deadline = now
            .checked_add(quarantine_ms)
            .filter(|until| *until > now)
            .ok_or_else(|| {
                KvStoreError::InvalidArgument("retry quarantine deadline cannot advance".into())
            })?;
        if let Some(committed) = &state.committed {
            let policy = self.buffer.quarantine_pending_request_policy(
                &BlockHashSerde(owner.hash.clone()),
                committed,
                &state.policy,
                budget,
                deadline,
            )?;
            mark_committed(state, policy);
            state.data = (self.new_data)(true);
        } else {
            let mut policy = state.policy.quarantined_retry_budget(budget, deadline)?;
            policy.requested_as_dependency = false;
            registry
                .retired
                .insert(owner.hash.clone(), RetiredRetryBudget {
                    attempts: policy.retry_attempts,
                    quarantine_until: deadline,
                });
            state.policy = policy;
            state.data = (self.new_data)(false);
            state.terminal = true;
            registry.live.remove(&owner.hash);
        }
        registry.active.remove(&owner.hash);
        Ok(true)
    }

    fn renew_hash(&self, hash: &BlockHash, sweep_time: u64) -> Result<(), KvStoreError> {
        let mut registry = self.registry.lock();
        if registry
            .retired
            .get(hash)
            .is_some_and(|record| record.quarantine_until <= sweep_time)
        {
            registry.retired.remove(hash);
        }
        if let Some(owner) = registry.live.get(hash).and_then(Weak::upgrade) {
            let mut state = owner.state.lock();
            if state.terminal || state.policy.renewed_retry_budget(sweep_time)?.is_none() {
                return Ok(());
            }
            if state.committed.is_some() {
                flush_policy(&self.buffer, hash, &mut state)?;
                let renewed = self.buffer.renew_pending_request_policy(
                    &BlockHashSerde(hash.clone()),
                    state.committed.as_ref().expect("committed policy"),
                    sweep_time,
                )?;
                mark_committed(&mut state, renewed);
            } else {
                state.policy = state
                    .policy
                    .renewed_retry_budget(sweep_time)?
                    .expect("expired policy");
            }
            for operation in state.operations.iter().filter_map(Weak::upgrade) {
                let mut phase = operation.phase.lock();
                if *phase == CompletionPhase::Reserved {
                    *phase = CompletionPhase::Cancelled;
                }
            }
        } else {
            let key = BlockHashSerde(hash.clone());
            if let Some(policy) = self.buffer.pending_request_policy(&key)? {
                if policy.renewed_retry_budget(sweep_time)?.is_some() {
                    self.buffer
                        .renew_pending_request_policy(&key, &policy, sweep_time)?;
                }
            }
        }
        Ok(())
    }

    pub async fn renew_expired(&self, sweep_time: u64) -> Result<(), KvStoreError> {
        let _pass = self.expiry_pass.lock().await;
        let mut first_error = None;
        let hashes: Vec<_> = {
            let registry = self.registry.lock();
            registry
                .active
                .keys()
                .chain(registry.retired.keys())
                .cloned()
                .collect()
        };
        for hash in hashes {
            if let Err(error) = self.renew_hash(&hash, sweep_time) {
                first_error.get_or_insert(error);
            }
        }
        let mut remaining = self.buffer.scan_candidate_count();
        while remaining > 0 {
            let batch = self.buffer.next_expiry_scan_batch(remaining);
            if batch.is_empty() {
                break;
            }
            remaining -= batch.len();
            for hash in batch {
                if let Err(error) = self.renew_hash(&hash.0, sweep_time) {
                    first_error.get_or_insert(error);
                }
            }
            tokio::task::yield_now().await;
        }
        first_error.map_or(Ok(()), Err)
    }

    fn ensure_owner(&self, owner: &RequestOwner<D>) -> Result<(), KvStoreError> {
        if !Arc::ptr_eq(&self.identity, &owner.registry_identity) {
            return Err(KvStoreError::InvalidArgument(
                "request belongs to another registry".into(),
            ));
        }
        Ok(())
    }

    fn lookup_locked(
        &self,
        registry: &mut Registry<D>,
        hash: &BlockHash,
        data: impl FnOnce(bool) -> D,
    ) -> Result<Option<Arc<RequestOwner<D>>>, KvStoreError> {
        if let Some(owner) = registry.live.get(hash).and_then(Weak::upgrade) {
            if !owner.state.lock().terminal {
                return Ok(Some(owner));
            }
        }
        registry.live.retain(|_, owner| owner.strong_count() != 0);
        let Some(policy) = self
            .buffer
            .pending_request_policy(&BlockHashSerde(hash.clone()))?
        else {
            return Ok(None);
        };
        let owner = Arc::new(RequestOwner {
            registry_identity: self.identity.clone(),
            hash: hash.clone(),
            state: Mutex::new(OwnerState {
                committed: Some(policy.clone()),
                policy,
                terminal: false,
                data: data(true),
                operations: Vec::new(),
            }),
        });
        registry.live.insert(hash.clone(), Arc::downgrade(&owner));
        Ok(Some(owner))
    }

    pub fn lookup(
        &self,
        hash: &BlockHash,
        data: impl FnOnce(bool) -> D,
    ) -> Result<Option<Arc<RequestOwner<D>>>, KvStoreError> {
        self.lookup_locked(&mut self.registry.lock(), hash, data)
    }

    #[cfg(any(test, feature = "test-utils"))]
    pub fn activate(
        &self,
        hash: BlockHash,
        initial: PendingRequestPolicy,
        data: impl FnOnce(bool) -> D,
    ) -> Result<Option<Arc<RequestOwner<D>>>, KvStoreError> {
        self.activate_checked(hash, initial, data, false, |_, _| true)
            .map(Activation::into_option)
    }

    pub fn activate_local(
        &self,
        hash: BlockHash,
        initial: PendingRequestPolicy,
        now: u64,
        data: impl FnOnce(bool) -> D,
    ) -> Result<Activation<D>, KvStoreError> {
        self.activate_checked(hash, initial, data, false, |policy, active| {
            active
                || policy
                    .retry_budget_quarantine_until
                    .is_none_or(|until| until <= now)
        })
    }

    pub fn activate_eligible(
        &self,
        hash: BlockHash,
        initial: PendingRequestPolicy,
        now: u64,
        recovery_budget: Option<u32>,
        data: impl FnOnce(bool) -> D,
    ) -> Result<Option<Arc<RequestOwner<D>>>, KvStoreError> {
        self.activate_checked(hash, initial, data, true, |policy, active| {
            policy
                .retry_budget_quarantine_until
                .is_none_or(|until| until <= now)
                && (active || recovery_budget.is_none_or(|budget| policy.retry_attempts < budget))
        })
        .map(Activation::into_option)
    }

    fn activate_checked(
        &self,
        hash: BlockHash,
        mut initial: PendingRequestPolicy,
        data: impl FnOnce(bool) -> D,
        fresh_schedule: bool,
        eligible: impl FnOnce(&PendingRequestPolicy, bool) -> bool,
    ) -> Result<Activation<D>, KvStoreError> {
        let mut registry = self.registry.lock();
        if let Some(owner) = registry.active.get(&hash) {
            return Ok(if eligible(&owner.state.lock().policy, true) {
                Activation::Active(owner.clone())
            } else {
                Activation::Ineligible
            });
        }
        if let Some(retired) = registry.retired.get(&hash) {
            initial.retry_attempts = retired.attempts;
            initial.retry_budget_quarantine_until = Some(retired.quarantine_until);
        }
        let mut data = Some(data);
        let existing = self.lookup_locked(&mut registry, &hash, |pending| {
            data.take().unwrap()(pending)
        })?;
        let allowed = match &existing {
            Some(owner) => eligible(&owner.state.lock().policy, false),
            None => eligible(&initial, false),
        };
        if !allowed {
            return Ok(Activation::Ineligible);
        }
        if registry.active.len() >= self.active_limit {
            return Ok(Activation::AtCapacity);
        }
        let owner = match existing {
            Some(owner) => {
                if fresh_schedule {
                    let mut state = owner.state.lock();
                    let mut policy = state.policy.clone();
                    policy.last_request_timestamp = initial.last_request_timestamp;
                    persist_candidate(&self.buffer, &hash, &mut state, policy)?;
                    state.data = (self.new_data)(false);
                }
                owner
            }
            None => {
                initial.encode()?;
                if initial.revision != 1 {
                    return Err(KvStoreError::InvalidArgument(
                        "new volatile request policy must start at revision one".into(),
                    ));
                }
                let owner = Arc::new(RequestOwner {
                    registry_identity: self.identity.clone(),
                    hash: hash.clone(),
                    state: Mutex::new(OwnerState {
                        policy: initial,
                        committed: None,
                        terminal: false,
                        data: data.take().unwrap()(false),
                        operations: Vec::new(),
                    }),
                });
                registry.live.insert(hash.clone(), Arc::downgrade(&owner));
                owner
            }
        };
        registry.retired.remove(&hash);
        registry.active.insert(hash, owner.clone());
        Ok(Activation::Active(owner))
    }

    pub fn publish_pending_for(
        &self,
        hash: BlockHash,
        mut initial: PendingRequestPolicy,
        data: impl FnOnce(bool) -> D,
        blocks: HashSet<BlockHashSerde>,
        certificates: HashSet<BlockHashSerde>,
    ) -> Result<(), KvStoreError> {
        let requested_as_dependency = initial.requested_as_dependency;
        let mut registry = self.registry.lock();
        if let Some(retired) = registry.retired.get(&hash) {
            initial.retry_attempts = retired.attempts;
            initial.retry_budget_quarantine_until = Some(retired.quarantine_until);
        }
        let mut data = Some(data);
        let owner = match self.lookup_locked(&mut registry, &hash, |pending| {
            data.take().unwrap()(pending)
        })? {
            Some(owner) => owner,
            None => {
                initial.encode()?;
                if initial.revision != 1 {
                    return Err(KvStoreError::InvalidArgument(
                        "new pending request policy must start at revision one".into(),
                    ));
                }
                Arc::new(RequestOwner {
                    registry_identity: self.identity.clone(),
                    hash: hash.clone(),
                    state: Mutex::new(OwnerState {
                        policy: initial,
                        committed: None,
                        terminal: false,
                        data: data.take().unwrap()(false),
                        operations: Vec::new(),
                    }),
                })
            }
        };
        self.publish_locked(
            &mut registry,
            &owner,
            blocks,
            certificates,
            requested_as_dependency,
        )?;
        registry.retired.remove(&hash);
        registry.live.insert(hash, Arc::downgrade(&owner));
        Ok(())
    }

    fn publish_locked(
        &self,
        registry: &mut Registry<D>,
        owner: &Arc<RequestOwner<D>>,
        blocks: HashSet<BlockHashSerde>,
        certificates: HashSet<BlockHashSerde>,
        requested_as_dependency: bool,
    ) -> Result<(), KvStoreError> {
        let mut state = owner.state.lock();
        if state.terminal {
            return Err(KvStoreError::TransactionConflict(
                "request is terminal".into(),
            ));
        }
        let mut updated = next_policy(&state)?;
        updated.requested_as_dependency |= requested_as_dependency;
        self.buffer.publish_pending_request(
            BlockHashSerde(owner.hash.clone()),
            blocks,
            certificates,
            state.committed.as_ref(),
            &updated,
        )?;
        mark_committed(&mut state, updated);
        if registry
            .active
            .get(&owner.hash)
            .is_some_and(|active| Arc::ptr_eq(active, owner))
        {
            registry.active.remove(&owner.hash);
        }
        Ok(())
    }

    pub fn update_policy<R>(
        &self,
        owner: &Arc<RequestOwner<D>>,
        update: impl FnOnce(&mut PendingRequestPolicy, &mut D) -> R,
    ) -> Result<R, KvStoreError>
    where
        D: Clone,
    {
        self.ensure_owner(owner)?;
        let mut state = owner.state.lock();
        if state.terminal {
            return Err(KvStoreError::TransactionConflict(
                "request is terminal".into(),
            ));
        }
        let mut policy = state.policy.clone();
        let mut data = state.data.clone();
        let result = update(&mut policy, &mut data);
        persist_candidate(&self.buffer, &owner.hash, &mut state, policy)?;
        state.data = data;
        Ok(result)
    }

    pub fn terminate(&self, owner: &Arc<RequestOwner<D>>) -> Result<(), KvStoreError> {
        self.ensure_owner(owner)?;
        let mut registry = self.registry.lock();
        let mut state = owner.state.lock();
        if state.terminal {
            return Ok(());
        }
        self.buffer.remove(BlockHashSerde(owner.hash.clone()))?;
        state.terminal = true;
        if registry
            .active
            .get(&owner.hash)
            .is_some_and(|active| Arc::ptr_eq(active, owner))
        {
            registry.active.remove(&owner.hash);
        }
        if registry
            .live
            .get(&owner.hash)
            .and_then(Weak::upgrade)
            .is_some_and(|current| Arc::ptr_eq(&current, owner))
        {
            registry.live.remove(&owner.hash);
        }
        Ok(())
    }

    pub fn reserve_retry_with<R>(
        &self,
        owner: &Arc<RequestOwner<D>>,
        budget: u32,
        now: u64,
        quarantine_ms: u64,
        select: impl FnOnce(&mut PendingRequestPolicy, &mut D, usize) -> RetrySelection<R>,
    ) -> Result<PreparedRetry<D, R>, KvStoreError>
    where
        D: Clone,
    {
        self.ensure_owner(owner)?;
        let mut registry = self.registry.lock();
        if !registry
            .active
            .get(&owner.hash)
            .is_some_and(|active| Arc::ptr_eq(active, owner))
        {
            return Ok(PreparedRetry::None);
        }
        let mut state = owner.state.lock();
        if state.terminal {
            return Ok(PreparedRetry::None);
        }
        state
            .operations
            .retain(|operation| operation.strong_count() != 0);
        let (reserved, reserved_peers) = state
            .operations
            .iter()
            .filter_map(Weak::upgrade)
            .filter(|operation| *operation.phase.lock() == CompletionPhase::Reserved)
            .fold((0usize, 0usize), |(total, peers), operation| {
                (total + 1, peers + usize::from(operation.peer))
            });
        let mut policy = state.policy.clone();
        if policy
            .retry_budget_quarantine_until
            .is_some_and(|until| now < until)
        {
            return Ok(PreparedRetry::None);
        }
        if policy.retry_attempts >= budget {
            self.retire_locked(&mut registry, owner, &mut state, budget, now, quarantine_ms)?;
            return Ok(PreparedRetry::None);
        }
        if u64::from(policy.retry_attempts).saturating_add(reserved as u64) >= u64::from(budget) {
            return Ok(PreparedRetry::None);
        }
        policy.retry_budget_quarantine_until = None;
        let Ok(permit) = self.permits.clone().try_acquire_owned() else {
            return Ok(PreparedRetry::None);
        };
        let mut data = state.data.clone();
        let (peer, selected) = match select(&mut policy, &mut data, reserved_peers) {
            RetrySelection::None => return Ok(PreparedRetry::None),
            RetrySelection::Suppressed(action) => {
                let mut suppressed = state.policy.clone();
                suppressed.last_request_timestamp = policy.last_request_timestamp;
                suppressed.peer_requery_cursor = policy.peer_requery_cursor;
                if suppressed != state.policy {
                    persist_candidate(&self.buffer, &owner.hash, &mut state, suppressed)?;
                }
                return Ok(PreparedRetry::Suppressed(action));
            }
            RetrySelection::Dispatch(peer, action) => (peer, action),
        };
        if policy != state.policy {
            persist_candidate(&self.buffer, &owner.hash, &mut state, policy)?;
        }
        state.data = data;
        let completion = Arc::new(Completion {
            owner: owner.clone(),
            phase: Mutex::new(CompletionPhase::Reserved),
            peer,
            _permit: permit,
        });
        state.operations.push(Arc::downgrade(&completion));
        Ok(PreparedRetry::Dispatch(
            RetryOperation {
                completion: Some(completion),
                queue: self.completions.clone(),
                buffer: self.buffer.clone(),
            },
            selected,
        ))
    }

    pub fn drain_completions(&self, limit: usize) -> Result<usize, KvStoreError> {
        let attempts = limit.min(self.completions.lock().len());
        let mut completed = 0;
        let mut first_error = None;
        for _ in 0..attempts {
            let Some(operation) = self.completions.lock().pop_front() else {
                break;
            };
            let result = {
                let mut state = operation.owner.state.lock();
                if state.terminal
                    || matches!(
                        *operation.phase.lock(),
                        CompletionPhase::Persisted | CompletionPhase::Cancelled
                    )
                {
                    Ok(())
                } else {
                    flush_policy(&self.buffer, &operation.owner.hash, &mut state)
                }
            };
            match result {
                Ok(()) => completed += 1,
                Err(error) => {
                    first_error.get_or_insert(error);
                    self.completions.lock().push_back(operation);
                }
            }
        }
        first_error.map_or(Ok(completed), Err)
    }
}

fn next_policy<D>(state: &OwnerState<D>) -> Result<PendingRequestPolicy, KvStoreError> {
    let mut policy = state.policy.clone();
    policy.revision = state.committed.as_ref().map_or(1, |prior| prior.revision);
    if state.committed.is_some() {
        policy.advance_revision()?;
    }
    Ok(policy)
}

fn mark_committed<D>(state: &mut OwnerState<D>, policy: PendingRequestPolicy) {
    state.policy = policy.clone();
    state.committed = Some(policy);
    state
        .operations
        .retain(|operation| operation.strong_count() != 0);
    for operation in state.operations.iter().filter_map(Weak::upgrade) {
        let mut phase = operation.phase.lock();
        if *phase == CompletionPhase::Observed {
            *phase = CompletionPhase::Persisted;
        }
    }
}

fn persist_candidate<D>(
    buffer: &CasperBufferKeyValueStorage,
    hash: &BlockHash,
    state: &mut OwnerState<D>,
    mut policy: PendingRequestPolicy,
) -> Result<(), KvStoreError> {
    if policy.initial_timestamp != state.policy.initial_timestamp
        || (state.policy.requested_as_dependency && !policy.requested_as_dependency)
        || policy.retry_attempts < state.policy.retry_attempts
        || policy.peer_requery_attempts < state.policy.peer_requery_attempts
    {
        return Err(KvStoreError::InvalidArgument(
            "request update cannot reset history".into(),
        ));
    }
    if let Some(committed) = &state.committed {
        if &policy == committed {
            mark_committed(state, policy);
            return Ok(());
        }
        policy.revision = committed.revision;
        policy.advance_revision()?;
        buffer.update_pending_request_policy(&BlockHashSerde(hash.clone()), committed, &policy)?;
        mark_committed(state, policy);
    } else {
        policy.revision = 1;
        state.policy = policy;
    }
    Ok(())
}

fn flush_policy<D>(
    buffer: &CasperBufferKeyValueStorage,
    hash: &BlockHash,
    state: &mut OwnerState<D>,
) -> Result<(), KvStoreError> {
    persist_candidate(buffer, hash, state, state.policy.clone())
}

impl<D> RetryOperation<D> {
    pub fn complete_action(mut self) -> Result<(), KvStoreError> {
        let operation = self.completion.as_ref().unwrap();
        let mut state = operation.owner.state.lock();
        if state.terminal {
            *operation.phase.lock() = CompletionPhase::Cancelled;
            return Ok(());
        }
        if *operation.phase.lock() != CompletionPhase::Reserved {
            return Ok(());
        }
        state.policy.retry_attempts = state.policy.retry_attempts.saturating_add(1);
        if operation.peer {
            state.policy.peer_requery_attempts =
                state.policy.peer_requery_attempts.saturating_add(1);
        }
        *operation.phase.lock() = CompletionPhase::Observed;
        if state.committed.is_some() {
            flush_policy(&self.buffer, &operation.owner.hash, &mut state)?;
        } else {
            *operation.phase.lock() = CompletionPhase::Persisted;
        }
        drop(state);
        self.completion.take();
        Ok(())
    }
}

impl<D> Drop for RetryOperation<D> {
    fn drop(&mut self) {
        let Some(operation) = self.completion.take() else {
            return;
        };
        let defer = {
            let state = operation.owner.state.lock();
            let mut phase = operation.phase.lock();
            match *phase {
                CompletionPhase::Observed if !state.terminal => true,
                CompletionPhase::Reserved => {
                    *phase = CompletionPhase::Cancelled;
                    false
                }
                _ => false,
            }
        };
        if defer {
            self.queue.lock().push_back(operation);
        }
    }
}
