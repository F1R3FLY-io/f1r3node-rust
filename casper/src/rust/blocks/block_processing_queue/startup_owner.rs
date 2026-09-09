use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use tokio::sync::Notify;

use super::startup_completion::{StartupCompletion, StartupKey, StartupPhase};
use super::startup_snapshot_lease::{LeasePhase, LeaseRole, SnapshotLeases};

#[derive(Clone, Debug)]
struct Identity(Arc<()>);

impl Identity {
    fn fresh() -> Self { Self(Arc::new(())) }
}

impl PartialEq for Identity {
    fn eq(&self, other: &Self) -> bool { Arc::ptr_eq(&self.0, &other.0) }
}

impl Eq for Identity {}

type Key = StartupKey<Identity, Identity>;
type PublicationResult<C, P, E, T> = Result<(T, StartupPublication<C, P, E>), StartupError<E>>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartupError<E> {
    Cancelled,
    Stopped,
    Unavailable,
    Poisoned,
    WrongOwner,
    CallbackConfiguration,
    ScanAbandoned,
    Scan(E),
    Callback(E),
}

#[derive(Clone)]
struct Outcome<E> {
    phase: StartupPhase,
    error: Option<StartupError<E>>,
}

struct Request<E> {
    key: Key,
    outcome: Mutex<Outcome<E>>,
    changed: Notify,
}

impl<E: Clone> Request<E> {
    fn outcome(&self) -> Outcome<E> {
        self.outcome.lock().map_or_else(
            |_| Outcome {
                phase: StartupPhase::Failed,
                error: Some(StartupError::Poisoned),
            },
            |state| state.clone(),
        )
    }

    fn update(&self, phase: StartupPhase, error: Option<StartupError<E>>) {
        let mut state = self
            .outcome
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if !state.phase.is_terminal() {
            state.phase = phase;
            state.error = error;
        }
    }

    async fn wait(&self, terminal_only: bool) -> Outcome<E> {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let state = self.outcome();
            if state.phase.is_terminal() || (!terminal_only && state.phase == StartupPhase::Ready) {
                return state;
            }
            changed.await;
        }
    }
}

struct State<C: ?Sized, P, E: Clone> {
    completion: StartupCompletion<Identity, Identity>,
    context: Option<Weak<C>>,
    request: Option<Arc<Request<E>>>,
    pending: Option<LeasedWork<C, P, E>>,
    leases: SnapshotLeases<Key, Identity>,
}

impl<C: ?Sized, P, E: Clone> State<C, P, E> {
    fn retire_pending(&mut self) -> Option<LeasedWork<C, P, E>> {
        let work = self.pending.take()?;
        assert!(self.leases.retire_stored(&work.identity));
        Some(work)
    }

    fn can_capture(&self, key: &Key) -> bool {
        !self.completion.is_stopped()
            && self.completion.context() == Some(&key.context)
            && self.completion.current().is_some_and(|request| {
                request.key == *key && request.phase == StartupPhase::Pending
            })
    }

    fn synchronize(&self, error: Option<StartupError<E>>) {
        if let (Some(current), Some(request)) = (self.completion.current(), &self.request) {
            if current.key == request.key {
                request.update(current.phase, error);
            }
        }
    }

    fn cancel_current(&mut self, error: StartupError<E>) {
        if let Some(request) = &self.request {
            self.completion.cancel(&request.key);
            self.synchronize(Some(error));
        }
    }
}

pub struct StartupOwner<C: ?Sized, P, E: Clone> {
    state: Mutex<State<C, P, E>>,
    changed: Arc<Notify>,
}

impl<C: ?Sized, P, E: Clone> Default for StartupOwner<C, P, E> {
    fn default() -> Self {
        Self {
            state: Mutex::new(State {
                completion: StartupCompletion::default(),
                context: None,
                request: None,
                pending: None,
                leases: SnapshotLeases::default(),
            }),
            changed: Arc::new(Notify::new()),
        }
    }
}

pub struct PreparedStartupContext<C: ?Sized, P, E: Clone> {
    handle: StartupContext<C, P, E>,
    context: Weak<C>,
}

pub struct StartupContext<C: ?Sized, P, E: Clone> {
    owner: Weak<StartupOwner<C, P, E>>,
    identity: Identity,
}

impl<C: ?Sized, P, E: Clone> Clone for StartupContext<C, P, E> {
    fn clone(&self) -> Self {
        Self {
            owner: self.owner.clone(),
            identity: self.identity.clone(),
        }
    }
}

impl<C: ?Sized, P, E: Clone> PreparedStartupContext<C, P, E> {
    pub fn handle(&self) -> StartupContext<C, P, E> { self.handle.clone() }
}

pub struct StartupRegistration<C: ?Sized, P, E: Clone>(StartupContext<C, P, E>);

pub(crate) struct StartupPublication<C: ?Sized, P, E: Clone> {
    pending: Option<LeasedWork<C, P, E>>,
    request: Option<Arc<Request<E>>>,
    changed: Arc<Notify>,
}

impl<C: ?Sized, P, E: Clone> StartupPublication<C, P, E> {
    pub(crate) fn finish(self) {
        self.changed.notify_one();
        if let Some(request) = self.request {
            request.changed.notify_waiters();
        }
        drop(self.pending);
    }
}

pub struct StartupTicket<C: ?Sized, P, E: Clone> {
    owner: Weak<StartupOwner<C, P, E>>,
    request: Arc<Request<E>>,
}

pub struct ActiveStartup<C: ?Sized, P, E: Clone> {
    owner: Weak<StartupOwner<C, P, E>>,
    key: Key,
    work: Option<LeasedWork<C, P, E>>,
}

pub(super) struct StartupCapture<'a, C: ?Sized, P, E: Clone> {
    ticket: &'a mut StartupTicket<C, P, E>,
    leased: Option<LeasedWork<C, P, E>>,
}

struct LeasedWork<C: ?Sized, P, E: Clone> {
    owner: Weak<StartupOwner<C, P, E>>,
    key: Key,
    identity: Identity,
    role: LeaseRole,
    work: Option<P>,
}

struct SnapshotRetirement<C: ?Sized, P, E: Clone> {
    owner: Weak<StartupOwner<C, P, E>>,
    key: Key,
    identity: Identity,
    role: LeaseRole,
}

impl<C: ?Sized, P, E: Clone> StartupOwner<C, P, E> {
    fn lock(&self) -> Result<MutexGuard<'_, State<C, P, E>>, StartupError<E>> {
        self.state.lock().map_err(|_| StartupError::Poisoned)
    }

    #[cfg(test)]
    pub(crate) fn with_locked_state_for_test<T>(&self, body: impl FnOnce() -> T) -> T {
        let _guard = self.state.lock().unwrap();
        body()
    }

    #[cfg(test)]
    pub(crate) fn state_is_unlocked_for_test(&self) -> bool { self.state.try_lock().is_ok() }

    pub fn prepare(self: &Arc<Self>, context: &Arc<C>) -> PreparedStartupContext<C, P, E> {
        PreparedStartupContext {
            handle: StartupContext {
                owner: Arc::downgrade(self),
                identity: Identity::fresh(),
            },
            context: Arc::downgrade(context),
        }
    }

    pub(crate) fn publish<T>(
        self: &Arc<Self>,
        prepared: PreparedStartupContext<C, P, E>,
        swap: impl FnOnce(StartupRegistration<C, P, E>) -> T,
    ) -> PublicationResult<C, P, E, T> {
        if !Weak::ptr_eq(&prepared.handle.owner, &Arc::downgrade(self)) {
            return Err(StartupError::WrongOwner);
        }
        let mut state = self.lock()?;
        if !state
            .completion
            .publish_context(prepared.handle.identity.clone())
        {
            return Err(StartupError::Stopped);
        }
        state.synchronize(Some(StartupError::Cancelled));
        state.context = Some(prepared.context);
        let cleanup = StartupPublication {
            pending: state.retire_pending(),
            request: state.request.clone(),
            changed: self.changed.clone(),
        };
        let result = swap(StartupRegistration(prepared.handle));
        Ok((result, cleanup))
    }

    pub(crate) fn clear_and_publish<T>(
        &self,
        swap: impl FnOnce() -> T,
    ) -> (T, StartupPublication<C, P, E>) {
        let mut state = self.state.lock().unwrap_or_else(|error| {
            let mut state = error.into_inner();
            state.completion.stop();
            state
        });
        if let Some(identity) = state.completion.context().cloned() {
            state.completion.revoke_context(&identity);
        }
        state.context = None;
        state.synchronize(Some(StartupError::Cancelled));
        let cleanup = StartupPublication {
            pending: state.retire_pending(),
            request: state.request.clone(),
            changed: self.changed.clone(),
        };
        let result = swap();
        (result, cleanup)
    }

    pub fn context(&self) -> Result<Option<Arc<C>>, StartupError<E>> {
        Ok(self.lock()?.context.as_ref().and_then(Weak::upgrade))
    }

    pub fn current_context(
        self: &Arc<Self>,
    ) -> Result<Option<StartupContext<C, P, E>>, StartupError<E>> {
        let state = self.lock()?;
        Ok(state.completion.context().map(|identity| StartupContext {
            owner: Arc::downgrade(self),
            identity: identity.clone(),
        }))
    }

    pub async fn changed(&self) { self.changed.notified().await; }

    pub fn activate(self: &Arc<Self>) -> Result<Option<ActiveStartup<C, P, E>>, StartupError<E>> {
        let mut state = self.lock()?;
        let Some(work) = state.pending.as_ref() else {
            return Ok(None);
        };
        let key = work.key.clone();
        if !state.can_capture(&key)
            || state.completion.active().is_some()
            || state.leases.active().is_some()
        {
            return Ok(None);
        }
        let Some(identity) = state.leases.activate(&key) else {
            return Ok(None);
        };
        assert_eq!(state.completion.activate(), Some(key.clone()));
        let mut work = state
            .pending
            .take()
            .expect("stored lease owns pending work");
        assert_eq!(work.identity, identity);
        work.role = LeaseRole::Active;
        state.synchronize(None);
        Ok(Some(ActiveStartup {
            owner: Arc::downgrade(self),
            key,
            work: Some(work),
        }))
    }

    pub fn stop(&self) { self.stop_deferred().finish(); }

    pub(crate) fn stop_deferred(&self) -> StartupPublication<C, P, E> {
        {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            state.completion.stop();
            state.context = None;
            state.synchronize(Some(StartupError::Stopped));
            StartupPublication {
                pending: state.retire_pending(),
                request: state.request.clone(),
                changed: self.changed.clone(),
            }
        }
    }

    pub fn is_stopped(&self) -> bool {
        self.state
            .lock()
            .map_or(true, |state| state.completion.is_stopped())
    }

    fn cancel(&self, key: &Key) {
        let cleanup = {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            if !state.completion.cancel(key) {
                return;
            }
            state.synchronize(Some(StartupError::Cancelled));
            StartupPublication {
                pending: state.retire_pending(),
                request: state.request.clone(),
                changed: self.changed.clone(),
            }
        };
        cleanup.finish();
    }

    fn revoke(&self, identity: &Identity) {
        let cleanup = {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            if !state.completion.revoke_context(identity) {
                return;
            }
            state.context = None;
            state.synchronize(Some(StartupError::Cancelled));
            StartupPublication {
                pending: state.retire_pending(),
                request: state.request.clone(),
                changed: self.changed.clone(),
            }
        };
        cleanup.finish();
    }

    fn fail(&self, key: &Key, error: StartupError<E>) -> Result<bool, StartupError<E>> {
        let request = {
            let mut state = self.lock()?;
            if !state.completion.fail(key) {
                return Ok(false);
            }
            state.synchronize(Some(error));
            state.request.clone()
        };
        if let Some(request) = request {
            request.changed.notify_waiters();
        }
        Ok(true)
    }
}

impl<C: ?Sized, P, E: Clone> Drop for StartupOwner<C, P, E> {
    fn drop(&mut self) { self.stop(); }
}

impl<C: ?Sized, P, E: Clone> StartupContext<C, P, E> {
    pub fn is_current(&self) -> Result<bool, StartupError<E>> {
        let Some(owner) = self.owner.upgrade() else {
            return Ok(false);
        };
        let state = owner.lock()?;
        Ok(!state.completion.is_stopped() && state.completion.context() == Some(&self.identity))
    }

    pub fn context(&self) -> Result<Option<Arc<C>>, StartupError<E>> {
        let Some(owner) = self.owner.upgrade() else {
            return Ok(None);
        };
        let state = owner.lock()?;
        if state.completion.is_stopped() || state.completion.context() != Some(&self.identity) {
            return Ok(None);
        }
        Ok(state.context.as_ref().and_then(Weak::upgrade))
    }

    pub fn request_startup(
        &self,
        callback_required: bool,
    ) -> Result<StartupTicket<C, P, E>, StartupError<E>> {
        let owner = self.owner.upgrade().ok_or(StartupError::Unavailable)?;
        let request = Arc::new(Request {
            key: StartupKey {
                context: self.identity.clone(),
                request: Identity::fresh(),
            },
            outcome: Mutex::new(Outcome {
                phase: StartupPhase::Pending,
                error: None,
            }),
            changed: Notify::new(),
        });
        let cleanup = {
            let mut state = owner.lock()?;
            if state.completion.is_stopped() {
                return Err(StartupError::Stopped);
            }
            if state.completion.context() != Some(&self.identity) {
                return Err(StartupError::Cancelled);
            }
            state.cancel_current(StartupError::Cancelled);
            let installed = state
                .completion
                .install(request.key.clone(), callback_required);
            assert!(installed);
            StartupPublication {
                pending: state.retire_pending(),
                request: state.request.replace(request.clone()),
                changed: owner.changed.clone(),
            }
        };
        cleanup.finish();
        Ok(StartupTicket {
            owner: self.owner.clone(),
            request,
        })
    }
}

impl<C: ?Sized, P, E: Clone> Drop for StartupRegistration<C, P, E> {
    fn drop(&mut self) {
        if let Some(owner) = self.0.owner.upgrade() {
            owner.revoke(&self.0.identity);
        }
    }
}

impl<C: ?Sized, P, E: Clone> StartupTicket<C, P, E> {
    pub fn phase(&self) -> StartupPhase { self.request.outcome().phase }

    pub(super) fn try_reserve_snapshot(
        &mut self,
    ) -> Result<Option<StartupCapture<'_, C, P, E>>, StartupError<E>> {
        let owner = self.owner.upgrade().ok_or(StartupError::Unavailable)?;
        let identity = {
            let mut state = owner.lock()?;
            if !state.can_capture(&self.request.key) {
                return Err(self
                    .request
                    .outcome()
                    .error
                    .unwrap_or(StartupError::Cancelled));
            }
            if state.leases.pending().is_some() {
                return Ok(None);
            }
            let identity = Identity::fresh();
            assert!(state
                .leases
                .reserve(self.request.key.clone(), identity.clone()));
            identity
        };
        let leased = LeasedWork {
            owner: self.owner.clone(),
            key: self.request.key.clone(),
            identity,
            role: LeaseRole::Pending,
            work: None,
        };
        Ok(Some(StartupCapture {
            ticket: self,
            leased: Some(leased),
        }))
    }

    pub(crate) async fn capture_snapshot<F>(&mut self, build: F) -> Result<(), StartupError<E>>
    where F: FnOnce() -> Result<P, E> {
        loop {
            let request = self.request.clone();
            let changed = request.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if let Some(capture) = self.try_reserve_snapshot()? {
                return capture.capture(build);
            }
            changed.await;
        }
    }

    pub async fn finish<F, Fut>(self, callback: Option<F>) -> Result<(), StartupError<E>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<(), E>>,
    {
        let outcome = self.request.wait(false).await;
        if outcome.phase != StartupPhase::Ready {
            return Self::result(outcome);
        }
        let owner = self.owner.upgrade().ok_or(StartupError::Unavailable)?;
        let permitted = {
            let mut state = owner.lock()?;
            if state.completion.current().is_some_and(|request| {
                request.key == self.request.key && request.callback_required != callback.is_some()
            }) {
                state.completion.fail(&self.request.key);
                state.synchronize(Some(StartupError::CallbackConfiguration));
                false
            } else {
                let changed = if callback.is_some() {
                    state.completion.authorize_callback(&self.request.key)
                } else {
                    state.completion.commit_success(&self.request.key)
                };
                if changed {
                    state.synchronize(None);
                }
                changed
            }
        };
        self.request.changed.notify_waiters();
        if !permitted || callback.is_none() {
            return Self::result(self.request.outcome());
        }
        drop(owner);
        if let Some(callback) = callback {
            let result = tokio::select! {
                result = async { callback().await } => result,
                outcome = self.request.wait(true) => return Self::result(outcome),
            };
            let owner = self.owner.upgrade().ok_or(StartupError::Unavailable)?;
            if let Err(error) = result {
                owner.fail(&self.request.key, StartupError::Callback(error))?;
            } else {
                let mut state = owner.lock()?;
                if state.completion.callback_succeeded(&self.request.key) {
                    state.completion.commit_success(&self.request.key);
                    state.synchronize(None);
                }
            }
        }
        self.request.changed.notify_waiters();
        Self::result(self.request.outcome())
    }

    fn result(outcome: Outcome<E>) -> Result<(), StartupError<E>> {
        if outcome.phase == StartupPhase::Succeeded {
            Ok(())
        } else {
            Err(outcome.error.unwrap_or(StartupError::Cancelled))
        }
    }
}

impl<C: ?Sized, P, E: Clone> Drop for StartupTicket<C, P, E> {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.upgrade() {
            owner.cancel(&self.request.key);
        }
    }
}

impl<C: ?Sized, P, E: Clone> StartupCapture<'_, C, P, E> {
    pub(crate) fn capture<F>(mut self, build: F) -> Result<(), StartupError<E>>
    where F: FnOnce() -> Result<P, E> {
        let owner = self
            .ticket
            .owner
            .upgrade()
            .ok_or(StartupError::Unavailable)?;
        let identity = self
            .leased
            .as_ref()
            .expect("capture owns a lease")
            .identity
            .clone();
        {
            let mut state = owner.lock()?;
            if !state.can_capture(&self.ticket.request.key) {
                return Err(self
                    .ticket
                    .request
                    .outcome()
                    .error
                    .unwrap_or(StartupError::Cancelled));
            }
            assert!(state.leases.begin_capture(&identity));
        }
        let result = build();
        match result {
            Ok(work) => self.leased.as_mut().expect("capture owns a lease").work = Some(work),
            Err(error) => {
                owner.fail(&self.ticket.request.key, StartupError::Scan(error.clone()))?;
                return Err(StartupError::Scan(error));
            }
        }
        {
            let mut state = owner.lock()?;
            assert!(state.leases.finish_capture(&identity));
            if !state.can_capture(&self.ticket.request.key) {
                return Err(self
                    .ticket
                    .request
                    .outcome()
                    .error
                    .unwrap_or(StartupError::Cancelled));
            }
            assert!(state.leases.store(&self.ticket.request.key, &identity));
            assert!(state.pending.is_none());
            state.pending = self.leased.take();
        }
        owner.changed.notify_one();
        Ok(())
    }
}

impl<C: ?Sized, P, E: Clone> Drop for LeasedWork<C, P, E> {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.upgrade() {
            let mut state = owner
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            match self.role {
                LeaseRole::Pending => {
                    if state
                        .leases
                        .pending()
                        .is_some_and(|entry| entry.phase == LeasePhase::Stored)
                    {
                        state.leases.retire_stored(&self.identity);
                    } else {
                        state.leases.abandon_capture(&self.identity);
                    }
                }
                LeaseRole::Active => {
                    state.leases.retire_active(&self.identity);
                }
            }
        }
        let retirement = SnapshotRetirement {
            owner: self.owner.clone(),
            key: self.key.clone(),
            identity: self.identity.clone(),
            role: self.role,
        };
        drop(self.work.take());
        drop(retirement);
    }
}

impl<C: ?Sized, P, E: Clone> Drop for SnapshotRetirement<C, P, E> {
    fn drop(&mut self) {
        let Some(owner) = self.owner.upgrade() else {
            return;
        };
        let request = {
            let mut state = owner
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if !state.leases.destroyed(self.role, &self.identity) {
                return;
            }
            assert!(state.leases.release(self.role, &self.identity));
            if self.role == LeaseRole::Active {
                if state.completion.current().is_some_and(|request| {
                    request.key == self.key
                        && matches!(
                            request.phase,
                            StartupPhase::Presence | StartupPhase::Admission
                        )
                }) {
                    state.completion.fail(&self.key);
                    state.synchronize(Some(StartupError::ScanAbandoned));
                }
                state.completion.retire_active(&self.key);
            }
            state.request.clone()
        };
        owner.changed.notify_one();
        if let Some(request) = request {
            request.changed.notify_waiters();
        }
    }
}

impl<C: ?Sized, P, E: Clone> ActiveStartup<C, P, E> {
    pub(super) fn work_mut(&mut self) -> &mut P {
        self.work
            .as_mut()
            .expect("active startup owns its lease")
            .work
            .as_mut()
            .expect("active startup owns its work")
    }

    pub fn context(&self) -> Result<Option<Arc<C>>, StartupError<E>> {
        let owner = self.owner.upgrade().ok_or(StartupError::Unavailable)?;
        let state = owner.lock()?;
        if state
            .completion
            .current()
            .is_none_or(|request| request.key != self.key || request.phase.is_terminal())
        {
            return Ok(None);
        }
        Ok(state.context.as_ref().and_then(Weak::upgrade))
    }

    pub fn presence_complete(&self) -> Result<bool, StartupError<E>> { self.advance(false) }

    pub fn scan_complete(&self) -> Result<bool, StartupError<E>> { self.advance(true) }

    fn advance(&self, complete: bool) -> Result<bool, StartupError<E>> {
        let owner = self.owner.upgrade().ok_or(StartupError::Unavailable)?;
        let request = {
            let mut state = owner.lock()?;
            let changed = if complete {
                state.completion.scan_complete(&self.key)
            } else {
                state.completion.presence_complete(&self.key)
            };
            if !changed {
                return Ok(false);
            }
            state.synchronize(None);
            state.request.clone()
        };
        if let Some(request) = request {
            request.changed.notify_waiters();
        }
        Ok(true)
    }

    pub fn fail(&self, error: E) -> Result<bool, StartupError<E>> {
        self.owner
            .upgrade()
            .ok_or(StartupError::Unavailable)?
            .fail(&self.key, StartupError::Scan(error))
    }
}

impl<C: ?Sized, P, E: Clone> Drop for ActiveStartup<C, P, E> {
    fn drop(&mut self) { drop(self.work.take()); }
}

#[cfg(test)]
#[path = "startup_owner_tests.rs"]
mod tests;
