use std::cell::RefCell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JournalOwnerHookPoint {
    AfterCapture,
    AfterPreparation,
    BeforeSuccessorPin,
    AfterSuccessorPin,
    AfterAcknowledgement,
    AfterPublishedCoreLoad,
}

thread_local! {
    static HOOK: RefCell<Option<(JournalOwnerHookPoint, Box<dyn FnOnce()>)>> = RefCell::new(None);
}

pub(crate) struct JournalOwnerHookGuard;

impl Drop for JournalOwnerHookGuard {
    fn drop(&mut self) {
        HOOK.with(|slot| {
            slot.borrow_mut().take();
        });
    }
}

pub(crate) fn arm(
    point: JournalOwnerHookPoint,
    hook: impl FnOnce() + 'static,
) -> JournalOwnerHookGuard {
    rearm(point, hook);
    JournalOwnerHookGuard
}

pub(crate) fn rearm(point: JournalOwnerHookPoint, hook: impl FnOnce() + 'static) {
    HOOK.with(|slot| {
        assert!(slot.borrow().is_none(), "a journal owner hook is already armed");
        *slot.borrow_mut() = Some((point, Box::new(hook)));
    });
}

pub(crate) fn run(point: JournalOwnerHookPoint) {
    let hook = HOOK.with(|slot| {
        let mut hook = slot.borrow_mut();
        if hook.as_ref().is_some_and(|(armed, _)| *armed == point) {
            hook.take()
        } else {
            None
        }
    });
    if let Some((_, hook)) = hook {
        hook();
    }
}
