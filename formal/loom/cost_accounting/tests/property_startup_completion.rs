use std::collections::BTreeMap;

use proptest::prelude::*;

#[path = "../../../../casper/src/rust/blocks/block_processing_queue/startup_completion.rs"]
mod startup_completion;
use startup_completion::{StartupCompletion, StartupKey, StartupPhase as Phase};

type Kernel = StartupCompletion<u64, u64>;
type Key = StartupKey<u64, u64>;

fn key(context: u64, request: u64) -> Key { Key { context, request } }

fn ready(callback: bool) -> (Kernel, Key) {
    let mut control = Kernel::default();
    let key = key(1, 1);
    assert!(control.publish_context(1));
    assert!(control.install(key.clone(), callback));
    assert_eq!(control.activate(), Some(key.clone()));
    assert!(control.presence_complete(&key));
    assert!(control.scan_complete(&key));
    assert!(control.retire_active(&key));
    (control, key)
}

#[test]
fn callback_and_no_callback_paths_require_complete_scan_and_exact_phase() {
    for callback in [false, true] {
        let mut control = Kernel::default();
        let id = key(u64::MAX, u64::MAX);
        assert!(!control.install(id.clone(), callback));
        assert!(control.publish_context(id.context));
        assert!(control.install(id.clone(), callback));
        assert!(!control.install(id.clone(), callback));
        assert!(!control.scan_complete(&id));
        assert!(!control.authorize_callback(&id));
        assert!(!control.commit_success(&id));
        assert_eq!(control.activate(), Some(id.clone()));
        assert!(!control.scan_complete(&id));
        assert!(control.presence_complete(&id));
        assert!(!control.presence_complete(&id));
        assert!(!control.commit_success(&id));
        assert!(control.scan_complete(&id));
        assert!(!control.scan_complete(&id));
        assert_eq!(control.authorize_callback(&id), callback);
        assert!(!control.authorize_callback(&id));
        if callback {
            assert!(!control.commit_success(&id));
        }
        assert_eq!(control.callback_succeeded(&id), callback);
        assert!(!control.callback_succeeded(&id));
        assert!(control.commit_success(&id));
        assert!(!control.commit_success(&id));
        assert!(!control.cancel(&id));
        assert!(!control.fail(&id));
        assert!(control.stop());
        assert_eq!(control.current().unwrap().phase, Phase::Succeeded);
        assert_eq!(control.active(), Some(&id));
        assert!(control.retire_active(&id));
        assert!(!control.retire_active(&id));
    }
}

#[test]
fn old_scan_completion_and_retirement_cannot_mutate_replacement() {
    let mut control = Kernel::default();
    let old = key(1, 1);
    let new = key(1, 2);
    control.publish_context(1);
    assert!(control.install(old.clone(), true));
    assert_eq!(control.activate(), Some(old.clone()));
    assert!(control.install(new.clone(), false));
    assert!(!control.presence_complete(&old));
    assert!(!control.scan_complete(&old));
    assert!(!control.cancel(&old));
    assert!(!control.fail(&old));
    assert!(control.activate().is_none());
    assert_eq!(control.current().unwrap().key, new);
    assert!(control.retire_active(&old));
    assert_eq!(control.activate(), Some(new.clone()));
    assert!(!control.retire_active(&old));
    assert_eq!(control.active(), Some(&new));
}

#[test]
fn replacement_before_authorization_and_during_callback_prevents_success() {
    for boundary in 0..3 {
        let (mut control, old) = ready(true);
        if boundary > 0 {
            assert!(control.authorize_callback(&old));
        }
        if boundary > 1 {
            assert!(control.callback_succeeded(&old));
        }
        assert!(control.publish_context(2));
        let new = key(2, 2);
        assert!(control.install(new.clone(), false));
        assert!(!control.authorize_callback(&old));
        assert!(!control.callback_succeeded(&old));
        assert!(!control.commit_success(&old));
        assert!(!control.revoke_context(&1));
        assert_eq!(control.context(), Some(&2));
        assert_eq!(control.current().unwrap().key, new);
    }
}

#[test]
fn terminal_failure_and_stop_cannot_reopen_a_ticket() {
    for failure in [false, true] {
        let (mut control, id) = ready(true);
        assert!(if failure {
            control.fail(&id)
        } else {
            control.cancel(&id)
        });
        let expected = if failure {
            Phase::Failed
        } else {
            Phase::Cancelled
        };
        assert!(!control.authorize_callback(&id));
        assert!(!control.commit_success(&id));
        assert!(control.stop());
        assert!(!control.stop());
        assert!(!control.publish_context(2));
        assert!(!control.install(key(2, 2), false));
        assert!(control.activate().is_none());
        assert!(control.is_stopped());
        assert_eq!(control.current().unwrap().phase, expected);
    }
}

#[test]
fn wrong_context_with_the_same_request_cannot_change_any_phase_or_owner() {
    let mut control = Kernel::default();
    let id = key(1, 7);
    let wrong = key(2, 7);
    control.publish_context(1);
    control.install(id.clone(), true);
    assert_eq!(control.activate(), Some(id.clone()));
    assert!(!control.presence_complete(&wrong));
    assert!(!control.retire_active(&wrong));
    assert!(!control.cancel(&wrong));
    assert!(!control.fail(&wrong));
    assert_eq!(control.active(), Some(&id));
    assert!(control.presence_complete(&id));
    assert!(!control.scan_complete(&wrong));
    assert!(control.scan_complete(&id));
    assert!(!control.authorize_callback(&wrong));
    assert!(control.authorize_callback(&id));
    assert!(!control.callback_succeeded(&wrong));
    assert!(!control.commit_success(&id));
    assert!(control.callback_succeeded(&id));
    assert!(!control.commit_success(&wrong));
    assert!(control.commit_success(&id));
}

#[derive(Clone, Copy, Debug)]
enum Action {
    Publish,
    Install(bool),
    Activate,
    Presence,
    Complete,
    Retire,
    Authorize,
    Callback,
    Success,
    Fail,
    Cancel,
    Revoke,
    Stop,
}

fn actions() -> impl Strategy<Value = Action> {
    prop_oneof![
        2 => Just(Action::Publish), 2 => any::<bool>().prop_map(Action::Install),
        6 => Just(Action::Activate), 6 => Just(Action::Presence), 6 => Just(Action::Complete),
        3 => Just(Action::Retire), 6 => Just(Action::Authorize), 6 => Just(Action::Success),
        6 => Just(Action::Callback),
        1 => Just(Action::Fail), 1 => Just(Action::Cancel), 1 => Just(Action::Revoke),
    ]
}

#[derive(Default)]
struct Reference {
    context: Option<u64>,
    current: Option<u64>,
    active: Option<u64>,
    requests: BTreeMap<u64, (u64, Vec<Action>, bool)>,
    stopped: bool,
}

impl Reference {
    fn phase(&self, id: u64) -> Phase {
        self.requests[&id]
            .1
            .iter()
            .fold(Phase::Pending, |phase, event| match event {
                Action::Activate => Phase::Presence,
                Action::Presence => Phase::Admission,
                Action::Complete => Phase::Ready,
                Action::Authorize => Phase::Authorized,
                Action::Callback => Phase::CallbackSucceeded,
                Action::Success => Phase::Succeeded,
                Action::Fail => Phase::Failed,
                Action::Cancel => Phase::Cancelled,
                _ => phase,
            })
    }

    fn cancel_current(&mut self) {
        if let Some(id) = self.current {
            if !matches!(
                self.phase(id),
                Phase::Succeeded | Phase::Failed | Phase::Cancelled
            ) {
                self.requests.get_mut(&id).unwrap().1.push(Action::Cancel);
            }
        }
    }

    fn live(&self, key: &Key) -> bool {
        !self.stopped
            && self.context == Some(key.context)
            && self.current == Some(key.request)
            && self
                .requests
                .get(&key.request)
                .is_some_and(|record| record.0 == key.context)
    }

    fn apply(&mut self, action: Action, key: &Key) -> bool {
        let accepted = match action {
            Action::Publish => {
                if self.stopped {
                    return false;
                }
                self.cancel_current();
                self.context = Some(key.context);
                return true;
            }
            Action::Install(callback) => {
                if self.stopped
                    || self.context != Some(key.context)
                    || self.current == Some(key.request)
                    || self.active == Some(key.request)
                {
                    return false;
                }
                self.cancel_current();
                self.requests
                    .insert(key.request, (key.context, Vec::new(), callback));
                self.current = Some(key.request);
                return true;
            }
            Action::Activate => {
                if self.active.is_some()
                    || !self.live(key)
                    || self.phase(key.request) != Phase::Pending
                {
                    return false;
                }
                self.active = Some(key.request);
                true
            }
            Action::Retire => {
                if self.active != Some(key.request)
                    || self
                        .requests
                        .get(&key.request)
                        .is_none_or(|record| record.0 != key.context)
                {
                    return false;
                }
                self.active = None;
                return true;
            }
            Action::Revoke => {
                if self.context != Some(key.context) {
                    return false;
                }
                self.context = None;
                self.cancel_current();
                return true;
            }
            Action::Stop => {
                if self.stopped {
                    return false;
                }
                self.stopped = true;
                self.context = None;
                self.cancel_current();
                return true;
            }
            Action::Presence => {
                self.live(key)
                    && self.active == Some(key.request)
                    && self.phase(key.request) == Phase::Presence
            }
            Action::Complete => {
                self.live(key)
                    && self.active == Some(key.request)
                    && self.phase(key.request) == Phase::Admission
            }
            Action::Authorize => {
                self.live(key)
                    && self.phase(key.request) == Phase::Ready
                    && self.requests[&key.request].2
            }
            Action::Success => {
                self.live(key)
                    && self.phase(key.request)
                        == if self.requests[&key.request].2 {
                            Phase::CallbackSucceeded
                        } else {
                            Phase::Ready
                        }
            }
            Action::Fail => {
                self.live(key)
                    && !matches!(
                        self.phase(key.request),
                        Phase::Succeeded | Phase::Failed | Phase::Cancelled
                    )
            }
            Action::Cancel => {
                self.current == Some(key.request)
                    && self
                        .requests
                        .get(&key.request)
                        .is_some_and(|record| record.0 == key.context)
                    && !matches!(
                        self.phase(key.request),
                        Phase::Succeeded | Phase::Failed | Phase::Cancelled
                    )
            }
            Action::Callback => {
                self.live(key)
                    && self.phase(key.request) == Phase::Authorized
                    && self.requests[&key.request].2
            }
        };
        if accepted {
            self.requests.get_mut(&key.request).unwrap().1.push(action);
        }
        accepted
    }
}

proptest! {
    #[test]
    fn arbitrary_histories_match_independent_event_log(
        events in proptest::collection::vec((actions(), any::<u16>()), 0..512),
        stop_at in proptest::option::of(0usize..512),
    ) {
        let mut kernel = Kernel::default();
        let mut reference = Reference::default();
        let mut known = vec![key(0, 0)];
        let mut generation = 0;
        for (index, (mut action, selector)) in events.into_iter().enumerate() {
            if stop_at == Some(index) {
                action = Action::Stop;
            }
            generation += 1;
            let mut selected = known[usize::from(selector) % known.len()].clone();
            let preferred = match selector % 4 {
                0 | 1 => reference.current,
                2 => reference.active,
                _ => None,
            };
            if let Some(id) = preferred {
                selected = key(reference.requests[&id].0, id);
            }
            match action {
                Action::Publish => selected.context = generation,
                Action::Install(_) => {
                    selected = key(reference.context.unwrap_or(0), generation);
                    known.push(selected.clone());
                }
                Action::Activate => if let Some(id) = reference.current {
                    selected = key(reference.requests[&id].0, id);
                },
                _ => {},
            }
            if selector & 0x8000 != 0 && !matches!(action, Action::Publish | Action::Install(_) | Action::Activate) {
                selected.context ^= 1 << 63;
            }
            let expected = reference.apply(action, &selected);
            let actual = match action {
                Action::Publish => kernel.publish_context(selected.context),
                Action::Install(callback) => kernel.install(selected.clone(), callback),
                Action::Activate => kernel.activate().is_some(),
                Action::Presence => kernel.presence_complete(&selected),
                Action::Complete => kernel.scan_complete(&selected),
                Action::Retire => kernel.retire_active(&selected),
                Action::Authorize => kernel.authorize_callback(&selected),
                Action::Callback => kernel.callback_succeeded(&selected),
                Action::Success => kernel.commit_success(&selected),
                Action::Fail => kernel.fail(&selected),
                Action::Cancel => kernel.cancel(&selected),
                Action::Revoke => kernel.revoke_context(&selected.context),
                Action::Stop => kernel.stop(),
            };
            prop_assert_eq!(actual, expected, "action={:?} key={:?}", action, selected);
            prop_assert_eq!(kernel.context().copied(), reference.context);
            prop_assert_eq!(kernel.is_stopped(), reference.stopped);
            prop_assert_eq!(kernel.active().map(|id| id.request), reference.active);
            prop_assert_eq!(kernel.current().map(|request| request.key.request), reference.current);
            if let Some(id) = reference.current {
                prop_assert_eq!(kernel.current().unwrap().phase, reference.phase(id));
            }
        }
    }
}
